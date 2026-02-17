use lazy_static::lazy_static;
use regex::Regex;

use crate::models::{LogEntry, LogLevel};

pub fn parse_line(raw_line: &str) -> Option<LogEntry> {
    // Step A: Header Parsing
    // Regex matches: Timestamp (simplified) and Level
    lazy_static! {
        // Timestamp + Level (greedy whitespace match after level)
        // Use [ \t] to avoid matching newlines/CRs
        static ref HEADER_RE: Regex = Regex::new(r"^(?P<ts>[\d\-]+\s[\d:,]+)\s+(?P<lvl>\w+)[ \t]").unwrap();
    }

    let caps = HEADER_RE.captures(raw_line)?;
    let timestamp = caps.name("ts")?.as_str().to_string();
    let level_str = caps.name("lvl")?.as_str();
    let verbosity_level = LogLevel::from(level_str);

    // Body Extraction
    let match_end = caps.get(0)?.end();
    let body = raw_line[match_end..].trim_end_matches('\n');

    // DRAIN3 INTEGRATION: Stop regex extraction here.
    // We return the raw body as the "template_str" for now,
    // and empty variables. The Sequencer will process this.

    let template_str = body.to_string();
    let variables = Vec::new();
    let template_hash = 0; // Placeholder, sequencer will fill.

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
