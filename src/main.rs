use lazy_static::lazy_static;
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Debug, PartialEq, Clone)]
pub enum LogLevel {
    INFO,
    DEBUG,
    ERROR,
    WARN,
    UNKNOWN,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

// Helper to parse log level from string
impl From<&str> for LogLevel {
    fn from(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "INFO" => LogLevel::INFO,
            "DEBUG" => LogLevel::DEBUG,
            "ERROR" => LogLevel::ERROR,
            "WARN" => LogLevel::WARN,
            _ => LogLevel::UNKNOWN,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub verbosity_level: LogLevel,
    pub component: Option<String>,
    pub template_hash: u64,
    pub template_str: String,
    pub variables: Vec<String>,
}

pub fn parse_line(raw_line: &str) -> Option<LogEntry> {
    // Step A: Header Parsing
    // Regex matches: Timestamp (simplified) and Level
    // Assuming format like: "2024-05-22 10:00:00 INFO Body..."
    // Adjust regex based on specific timestamp format requirements
    lazy_static! {
        // Updated regex to include comma in timestamp characters [\d:,]+
        static ref HEADER_RE: Regex = Regex::new(r"^(?P<ts>[\d\-]+\s[\d:,]+)\s+(?P<lvl>\w+)\s+").unwrap();
        static ref IP_RE: Regex = Regex::new(r"^\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}$").unwrap();
        static ref DIGIT_RE: Regex = Regex::new(r"\d").unwrap();
    }

    let caps = HEADER_RE.captures(raw_line)?;
    let timestamp = caps.name("ts")?.as_str().to_string();
    let level_str = caps.name("lvl")?.as_str();
    let verbosity_level = LogLevel::from(level_str);

    let body_start = caps.get(0)?.end();
    let body = &raw_line[body_start..];

    // Step B: Tokenization
    // Splitting by spaces
    let tokens: Vec<&str> = body.split_whitespace().collect();

    let mut template_parts = Vec::new();
    let mut variables = Vec::new();

    // Speculative component extraction:
    // If the first token looks like "[Component]" or "Component:", take it.
    // For now, adhering strictly to "Split by spaces" for tokens.
    // User requirement: "component: (String, optional - e.g., 'AuthSystem')"
    // We'll treat the rest as the message body for template generation.

    for token in tokens {
        // Step C: Variable Detection
        let mut is_variable = false;

        // 1. Contains digits
        if DIGIT_RE.is_match(token) {
            is_variable = true;
        }
        // 2. Is IP address (IPv4)
        else if IP_RE.is_match(token) {
            is_variable = true;
        }
        // 3. Inside quotes or brackets (simple check: starts/ends with quotes/brackets)
        else if (token.starts_with('\'') && token.ends_with('\''))
            || (token.starts_with('"') && token.ends_with('"'))
            || (token.starts_with('[') && token.ends_with(']'))
        {
            is_variable = true;
        }
        // 4. Is a file path (contains /)
        else if token.contains('/') {
            is_variable = true;
        }

        // Step D: Template Generation
        if is_variable {
            template_parts.push("<VAR>");
            variables.push(token.to_string());
        } else {
            template_parts.push(token);
        }
    }

    let template_str = template_parts.join(" ");

    // Calculate Template Hash
    let mut hasher = DefaultHasher::new();
    template_str.hash(&mut hasher);
    let template_hash = hasher.finish();

    // Component extraction is heuristic.
    // If the body starts with something like "[AuthSystem]", let's strip it from variables?
    // The prompt implies component is a field, but doesn't strictly say HOW to extract it.
    // I will leave it as None for now unless explicit "Component" field logic is required,
    // or parse it if it looks like a component.
    // For this implementation, I'll default to None to keep it clean unless a pattern emerges.
    let component = None;

    Some(LogEntry {
        timestamp,
        verbosity_level,
        component,
        template_hash,
        template_str,
        variables,
    })
}

fn main() {
    let log_line = "2015-12-04 13:48:28,241 INFO org.apache.hadoop.hdfs.server.datanode.DataNode: Successfully sent block report 0x7aaf8f37153be,  containing 1 storage report(s), of which we sent 1. The reports had 0 total blocks and used 1 RPC(s). This took 0 msec to generate and 2 msecs for RPC and NN processing. Got back one command: FinalizeCommand/5.";

    println!("Log line\n{}", log_line.to_string());
    if let Some(entry) = parse_line(log_line) {
        println!("Parsed Entry: \n{:?}\n", entry);
        println!("Template: \n{}\n", entry.template_str);
    } else {
        println!("Failed to parse line: {}", log_line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_line() {
        let line = "2024-05-22 10:00:00 INFO User 'admin' failed login from 192.168.1.1";
        let entry = parse_line(line).unwrap();

        assert_eq!(entry.timestamp, "2024-05-22 10:00:00");
        assert_eq!(entry.verbosity_level, LogLevel::INFO);
        assert_eq!(entry.template_str, "User <VAR> failed login from <VAR>");
        assert_eq!(entry.variables, vec!["'admin'", "192.168.1.1"]);
    }

    #[test]
    fn test_parse_file_path_and_digits() {
        let line = "2024-05-22 10:01:00 ERROR File /var/log/syslog not found with error code 404";
        let entry = parse_line(line).unwrap();

        assert_eq!(entry.verbosity_level, LogLevel::ERROR);
        assert_eq!(
            entry.template_str,
            "File <VAR> not found with error code <VAR>"
        );
        assert_eq!(entry.variables, vec!["/var/log/syslog", "404"]);
    }

    #[test]
    fn test_parse_hdfs_log() {
        let line = "2016-04-08 16:16:54,636 INFO org.apache.hadoop.hdfs.server.datanode.DataNode.clienttrace: src: /10.10.34.14:46217, dest: /10.10.34.11:50010, bytes: 369192, op: HDFS_WRITE, ";
        let entry = parse_line(line).unwrap();

        assert_eq!(entry.timestamp, "2016-04-08 16:16:54,636");
        assert_eq!(entry.verbosity_level, LogLevel::INFO);
        // "src:" is static, "/10..." is var (digits+path), "dest:" static, "/10..." var, "bytes:" static, "369192," var (digits).
        assert_eq!(
            entry.template_str,
            "org.apache.hadoop.hdfs.server.datanode.DataNode.clienttrace: src: <VAR> dest: <VAR> bytes: <VAR> op: HDFS_WRITE,"
        );
    }
}
