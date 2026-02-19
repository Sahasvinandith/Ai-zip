use crate::models::{LogEntry, LogLevel};
use chrono::{Datelike, NaiveDateTime};
use lazy_static::lazy_static;
use regex::Regex;

pub fn parse_line(raw_line: &str) -> Option<LogEntry> {
    lazy_static! {
        // HADOOP: 2015-08-21 11:16:10,187 INFO ...
        static ref RE_HADOOP: Regex = Regex::new(r"^(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3})\s+(?P<lvl>\w+)\s").unwrap();

        // SYSLOG: Aug 21 11:16:10 HOST APP: ...
        static ref RE_SYSLOG: Regex = Regex::new(r"^(?P<ts>[A-Z][a-z]{2}\s+\d{1,2}\s\d{2}:\d{2}:\d{2})\s+(?P<host>[\w\.\-]+)\s+(?P<app>[\w\.\-]+):\s").unwrap();

        // NOVA: 2015-08-21 11:16:10.187 PID LEVEL ...
        static ref RE_NOVA: Regex = Regex::new(r"^(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3})\s+(?P<pid>\d+)\s+(?P<lvl>\w+)\s").unwrap();
    }

    // 1. Try HADOOP
    if let Some(caps) = RE_HADOOP.captures(raw_line) {
        let timestamp = caps.name("ts").unwrap().as_str().to_string();
        let level_str = caps.name("lvl").unwrap().as_str();
        let verbosity_level = LogLevel::from(level_str);

        let match_end = caps.get(0).unwrap().end();
        let body = raw_line[match_end..].trim_end_matches('\n');

        return Some(LogEntry {
            timestamp: Some(timestamp),
            verbosity_level,
            component: None,
            template_hash: 0,
            template_str: body.to_string(),
            variables: Vec::new(),
            has_newline: true,
        });
    }

    // 2. Try SYSLOG
    // DISABLED: This destructive parsing alters timestamps and inserts "INFO", causing fidelity loss.
    // Falls back to RAW mode for perfect preservation.
    /*
    if let Some(caps) = RE_SYSLOG.captures(raw_line) {
        let ts_raw = caps.name("ts").unwrap().as_str();
        let host = caps.name("host").unwrap().as_str();
        let app = caps.name("app").unwrap().as_str();

        // Inject current year
        let current_year = chrono::Local::now().year();
        let timestamp = format!("{} {}", current_year, ts_raw);

        // Reconstruct body with host/app to preserve them
        let match_end = caps.get(0).unwrap().end();
        let msg_body = raw_line[match_end..].trim_end_matches('\n');
        let full_body = format!("{} {}: {}", host, app, msg_body);

        return Some(LogEntry {
            timestamp: Some(timestamp),
            verbosity_level: LogLevel::INFO, // Syslog implies INFO usually
            component: None,
            template_hash: 0,
            template_str: full_body,
            variables: Vec::new(),
            has_newline: true,
        });
    }
    */

    // 3. Try NOVA
    if let Some(caps) = RE_NOVA.captures(raw_line) {
        let ts_raw = caps.name("ts").unwrap().as_str();
        let timestamp = ts_raw.replace('.', ","); // Normalize to HADOOP format
        let level_str = caps.name("lvl").unwrap().as_str();
        let verbosity_level = LogLevel::from(level_str);

        let match_end = caps.get(0).unwrap().end();
        let body = raw_line[match_end..].trim_end_matches('\n');

        return Some(LogEntry {
            timestamp: Some(timestamp),
            verbosity_level,
            component: None,
            template_hash: 0,
            template_str: body.to_string(),
            variables: Vec::new(),
            has_newline: true,
        });
    }

    // 4. Fallback: RAW (Stack traces, etc)
    Some(LogEntry {
        timestamp: None,
        verbosity_level: LogLevel::RAW,
        component: None,
        template_hash: 0,
        template_str: raw_line.trim_end_matches('\n').to_string(),
        variables: Vec::new(),
        has_newline: true,
    })
}
