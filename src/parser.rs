use lazy_static::lazy_static;
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::models::{LogEntry, LogLevel};

pub fn parse_line(raw_line: &str) -> Option<LogEntry> {
    // Step A: Header Parsing
    // Regex matches: Timestamp (simplified) and Level
    lazy_static! {
        // Timestamp + Level (greedy whitespace match after level)
        static ref HEADER_RE: Regex = Regex::new(r"^(?P<ts>[\d\-]+\s[\d:,]+)\s+(?P<lvl>\w+)\s").unwrap();
    }

    let caps = HEADER_RE.captures(raw_line)?;
    let timestamp = caps.name("ts")?.as_str().to_string();
    let level_str = caps.name("lvl")?.as_str();
    let verbosity_level = LogLevel::from(level_str);

    // Body Extraction
    let match_end = caps.get(0)?.end();
    let body = &raw_line[match_end..]; // Everything after "YYYY.. LEVEL "

    // Structure Preserving Replacement Logic
    lazy_static! {
        static ref VAR_REGEX: Regex = Regex::new(
            r#"(?x)
            # 1. Quoted Strings (Single or Double)
            (['"][^'"]*['"]) |
            # 2. IP Addresses (IPv4)
            (\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b) |
            # 3. File Paths (Must contain /, allowing alphanumeric, dots, hyphens, underscores)
            ([\w\.\-_]*\/[\w\.\-/_]*) |
            # 4. Digits (Hex, Decimals, Integers)
            (\b\w*\d\w*\b)
            "#
        )
        .unwrap();
    }

    let mut variables = Vec::new();

    // First pass: Collect variables in order
    for mat in VAR_REGEX.find_iter(body) {
        variables.push(mat.as_str().to_string());
    }

    // Second pass: Replace with <VAR>
    let template_str = VAR_REGEX.replace_all(body, "<VAR>").to_string();

    let mut hasher = DefaultHasher::new();
    template_str.hash(&mut hasher);
    let template_hash = hasher.finish();

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

// Function to check if a line is a start of a new log entry
pub fn is_log_start(line: &str) -> bool {
    lazy_static! {
        static ref START_RE: Regex = Regex::new(r"^\d{4}-\d{2}-\d{2}").unwrap();
    }
    START_RE.is_match(line)
}
