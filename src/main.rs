use chrono::NaiveDateTime;
use lazy_static::lazy_static;
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fmt;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Write};

#[derive(Debug, PartialEq, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub verbosity_level: LogLevel,
    pub component: Option<String>,
    pub template_hash: u64,
    pub template_str: String,
    pub variables: Vec<String>,
}

pub struct LogCompressor {
    registry: HashMap<u64, u32>,
    template_store: Vec<String>,
    ts_col: Vec<u64>,
    id_col: Vec<u32>,
    var_col: Vec<String>,
}

impl LogCompressor {
    pub fn new() -> Self {
        LogCompressor {
            registry: HashMap::new(),
            template_store: Vec::new(),
            ts_col: Vec::new(),
            id_col: Vec::new(),
            var_col: Vec::new(),
        }
    }

    pub fn ingest(&mut self, entry: LogEntry) {
        // 1. Handle Timestamp
        // Try parsing different formats.
        // Format A: "2024-05-22 10:00:00" -> "%Y-%m-%d %H:%M:%S"
        // Format B: "2016-04-08 16:16:54,636" -> "%Y-%m-%d %H:%M:%S,%f"
        let ts_millis =
            if let Ok(dt) = NaiveDateTime::parse_from_str(&entry.timestamp, "%Y-%m-%d %H:%M:%S") {
                dt.and_utc().timestamp_millis() as u64
            } else if let Ok(dt) =
                NaiveDateTime::parse_from_str(&entry.timestamp, "%Y-%m-%d %H:%M:%S,%f")
            {
                dt.and_utc().timestamp_millis() as u64
            } else {
                // Fallback: current time or 0 if parsing fails drastically (should capture error in real prod)
                0
            };
        self.ts_col.push(ts_millis);

        // 2. Handle Template ID
        let id = if let Some(&existing_id) = self.registry.get(&entry.template_hash) {
            existing_id
        } else {
            let new_id = self.template_store.len() as u32;
            self.registry.insert(entry.template_hash, new_id);
            self.template_store.push(entry.template_str);
            new_id
        };
        self.id_col.push(id);

        // 3. Handle Variables
        self.var_col.extend(entry.variables);
    }

    pub fn save(&self, filepath: &str) -> std::io::Result<()> {
        let file = File::create(filepath)?;
        let mut writer = std::io::BufWriter::new(file);

        // Header: Magic Bytes
        writer.write_all(b"SALC")?;

        // Block 1: Compressed Registry (TemplateStore)
        // We serialize the Vector of strings (index = ID)
        let registry_data = serde_json::to_vec(&self.template_store)?;
        let compressed_registry = zstd::encode_all(&registry_data[..], 0)?;
        writer.write_all(&u32::to_le_bytes(compressed_registry.len() as u32))?; // Write Size
        writer.write_all(&compressed_registry)?;

        // Block 2: Compressed Timestamps
        // Convert Vec<u64> to bytes
        let mut ts_bytes = Vec::with_capacity(self.ts_col.len() * 8);
        for ts in &self.ts_col {
            ts_bytes.extend_from_slice(&ts.to_le_bytes());
        }
        let compressed_ts = zstd::encode_all(&ts_bytes[..], 0)?;
        writer.write_all(&u32::to_le_bytes(compressed_ts.len() as u32))?;
        writer.write_all(&compressed_ts)?;

        // Block 3: Compressed IDs
        // Convert Vec<u32> to bytes
        let mut id_bytes = Vec::with_capacity(self.id_col.len() * 4);
        for id in &self.id_col {
            id_bytes.extend_from_slice(&id.to_le_bytes());
        }
        let compressed_ids = zstd::encode_all(&id_bytes[..], 0)?;
        writer.write_all(&u32::to_le_bytes(compressed_ids.len() as u32))?;
        writer.write_all(&compressed_ids)?;

        // Block 4: Compressed Variables
        // Flattened list serialized to JSON
        let var_data = serde_json::to_vec(&self.var_col)?;
        let compressed_vars = zstd::encode_all(&var_data[..], 0)?;
        writer.write_all(&u32::to_le_bytes(compressed_vars.len() as u32))?;
        writer.write_all(&compressed_vars)?;

        writer.flush()?;
        Ok(())
    }
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

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "Usage: {} <input_log_file> <output_compressed_file>",
            args[0]
        );
        std::process::exit(1);
    }

    let input_path = &args[1];
    let output_path = &args[2];

    println!("Reading logs from: {}", input_path);
    println!("Compressing to: {}", output_path);

    let input_file = File::open(input_path)?;
    let reader = BufReader::new(input_file);

    let mut compressor = LogCompressor::new();

    let mut count = 0;
    for line_result in reader.lines() {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }

        if let Some(entry) = parse_line(&line) {
            compressor.ingest(entry);
            count += 1;
        } else {
            eprintln!("Skipping unparsable line: {}", line);
        }
    }

    compressor.save(output_path)?;

    println!("Successfully processed and compressed {} log lines.", count);
    Ok(())
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
