use drain_rs::DrainTree;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, RwLock};

pub struct DrainRegistry {
    tree: Arc<RwLock<DrainTree>>,
}

impl DrainRegistry {
    pub fn new() -> Self {
        let tree = DrainTree::new()
            .max_depth(2)
            .max_children(100)
            .min_similarity(0.4);

        DrainRegistry {
            tree: Arc::new(RwLock::new(tree)),
        }
    }

    /// Learns the template from the content.
    /// Returns (TemplateID, TemplateString, ExtractedVariables).
    ///
    /// IMPORTANT: We always call `add_log_line` (write path) to ensure the
    /// template is properly generalized. Using the read-only `log_group` would
    /// return stale un-generalized templates, causing variable extraction to
    /// miss differences and producing corrupted decompression output.
    ///
    /// Templates are returned with `<VAR>` placeholders (converted from Drain's
    /// `<*>`) for compatibility with the decompressor.
    pub fn get_or_learn(&self, content: &str) -> (u64, String, Vec<String>) {
        let mut tree = self.tree.write().unwrap();

        // Workaround: Escape tabs to prevent Drain from stripping them as whitespace
        let content_escaped = content.replace('\t', "__TAB__");

        let cluster_str = if let Some(cluster) = tree.add_log_line(&content_escaped) {
            cluster.as_string()
        } else {
            // Fallback: treat entire content as the template (no variables)
            content_escaped.to_string()
        };

        drop(tree); // release lock before extraction work

        let vars = self.extract_variables(&content_escaped, &cluster_str);

        // Convert Drain's <*> to <VAR> for decompressor compatibility
        let template_with_var = cluster_str.replace("<*>", "<VAR>");
        let id = self.compute_hash(&template_with_var);

        (id, template_with_var, vars)
    }

    fn compute_hash(&self, s: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        s.hash(&mut hasher);
        hasher.finish()
    }

    /// Extract variables by comparing content tokens against template tokens.
    /// Any token in the template that is `<*>` indicates the corresponding
    /// content token is a variable.
    fn extract_variables(&self, content: &str, template: &str) -> Vec<String> {
        let content_tokens: Vec<&str> = content.split_whitespace().collect();
        let template_tokens: Vec<&str> = template.split_whitespace().collect();

        let mut vars = Vec::new();

        for (i, t_token) in template_tokens.iter().enumerate() {
            if i >= content_tokens.len() {
                break;
            }
            if *t_token == "<*>" {
                vars.push(content_tokens[i].to_string());
            }
        }

        vars
    }

    #[allow(dead_code)]
    pub fn dump(&self) -> std::collections::HashMap<u64, String> {
        let tree = self.tree.read().unwrap();
        let clusters = tree.log_groups();
        let mut map = std::collections::HashMap::new();
        for cluster in clusters {
            let s = cluster.as_string().replace("<*>", "<VAR>");
            let id = self.compute_hash(&s);
            map.insert(id, s);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_extraction_basic() {
        let registry = DrainRegistry::new();

        // First log creates the initial template (exact match, no wildcards)
        let (_, tmpl1, vars1) =
            registry.get_or_learn("sent block report 0xAAA containing 1 storage");
        println!("After log 1 - Template: {}, Vars: {:?}", tmpl1, vars1);

        // Second log should trigger generalization
        let (_, tmpl2, vars2) =
            registry.get_or_learn("sent block report 0xBBB containing 2 storage");
        println!("After log 2 - Template: {}, Vars: {:?}", tmpl2, vars2);

        // Template should now have <VAR> placeholders
        assert!(
            tmpl2.contains("<VAR>"),
            "Template should contain <VAR> after generalization: {}",
            tmpl2
        );

        // Variables should be extracted for the differing tokens
        assert!(
            !vars2.is_empty(),
            "Should have extracted variables: {:?}",
            vars2
        );
        assert!(
            vars2.iter().any(|v| v.contains("0xBBB")),
            "Should contain 0xBBB. Vars: {:?}",
            vars2
        );
    }

    #[test]
    fn test_placeholder_consistency() {
        let registry = DrainRegistry::new();

        // Train with two similar logs
        registry.get_or_learn("Error in module Alpha code 100");
        let (_, tmpl, vars) = registry.get_or_learn("Error in module Beta code 200");

        println!("Template: {}, Vars: {:?}", tmpl, vars);

        // Template should use <VAR>, NOT <*>
        assert!(
            !tmpl.contains("<*>"),
            "Template must not contain <*>: {}",
            tmpl
        );
        // If generalized, should use <VAR>
        if tmpl.contains("<VAR>") {
            let var_count = tmpl.matches("<VAR>").count();
            assert_eq!(
                vars.len(),
                var_count,
                "Variable count ({}) must match <VAR> count ({}) in template: {}",
                vars.len(),
                var_count,
                tmpl
            );
        }
    }
}
