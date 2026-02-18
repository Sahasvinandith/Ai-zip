<<<<<<< HEAD
<<<<<<< HEAD
use chrono::{Datelike, Local, NaiveDateTime};
=======
use chrono::{Datelike, Local};
>>>>>>> master
=======
use crate::models::{LogEntry, LogLevel};
use chrono::{Datelike, NaiveDateTime};
>>>>>>> master
use lazy_static::lazy_static;
use regex::Regex;

pub fn parse_line(raw_line: &str) -> Option<LogEntry> {
<<<<<<< HEAD
    // 1. Try matching known formats
    // Order: Hadoop (most specific) -> Nova -> Syslog (least specific date)

    lazy_static! {
        // HADOOP: "2016-07-28 15:09:01,967 INFO ..."
        static ref RE_HADOOP: Regex = Regex::new(r"^(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3})\s+(?P<lvl>\w+)\s").unwrap();

        // NOVA: "2017-05-14 19:39:02.007 2931 INFO ..."
        // Matches timestamp, PID (ignored), Level
        static ref RE_NOVA: Regex = Regex::new(r"^(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3})\s+(?P<pid>\d+)\s+(?P<lvl>\w+)\s").unwrap();

        // SYSLOG: "Jun  9 06:06:20 combo kernel: ..."
        // Matches MMM dd HH:mm:ss, Host, App (until colon)
        static ref RE_SYSLOG: Regex = Regex::new(r"^(?P<ts>[A-Z][a-z]{2}\s+\d+\s\d{2}:\d{2}:\d{2})\s+(?P<host>\S+)\s+(?P<app>[^:]+):\s").unwrap();
    }

    if let Some(caps) = RE_HADOOP.captures(raw_line) {
        let timestamp = caps.name("ts")?.as_str().to_string(); // Already correct format
        let level_str = caps.name("lvl")?.as_str();
        let verbosity_level = LogLevel::from(level_str);

        let match_end = caps.get(0)?.end();
        let body = raw_line[match_end..].trim_end_matches('\n').to_string();
=======
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
>>>>>>> master

        return Some(LogEntry {
            timestamp: Some(timestamp),
            verbosity_level,
            component: None,
            template_hash: 0,
<<<<<<< HEAD
            template_str: body,
=======
            template_str: body.to_string(),
>>>>>>> master
            variables: Vec::new(),
            has_newline: true,
        });
    }

<<<<<<< HEAD
    if let Some(caps) = RE_NOVA.captures(raw_line) {
        let raw_ts = caps.name("ts")?.as_str();
        let timestamp = raw_ts.replace('.', ","); // Normalize . to ,
        let level_str = caps.name("lvl")?.as_str();
        let verbosity_level = LogLevel::from(level_str);

        let match_end = caps.get(0)?.end();
        let body = raw_line[match_end..].trim_end_matches('\n').to_string();

=======
    // 2. Try SYSLOG
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

    // 3. Try NOVA
    if let Some(caps) = RE_NOVA.captures(raw_line) {
        let ts_raw = caps.name("ts").unwrap().as_str();
        let timestamp = ts_raw.replace('.', ","); // Normalize to HADOOP format
        let level_str = caps.name("lvl").unwrap().as_str();
        let verbosity_level = LogLevel::from(level_str);

        let match_end = caps.get(0).unwrap().end();
        let body = raw_line[match_end..].trim_end_matches('\n');

>>>>>>> master
        return Some(LogEntry {
            timestamp: Some(timestamp),
            verbosity_level,
            component: None,
            template_hash: 0,
<<<<<<< HEAD
            template_str: body,
=======
            template_str: body.to_string(),
>>>>>>> master
            variables: Vec::new(),
            has_newline: true,
        });
    }

<<<<<<< HEAD
    if let Some(caps) = RE_SYSLOG.captures(raw_line) {
        let raw_ts = caps.name("ts")?.as_str();
        // Parse "Jun  9 06:06:20" and inject current year
        // We assume current year for simplicity as log doesn't have it
        let current_year = Local::now().year();
        let ts_with_year = format!("{} {}", current_year, raw_ts);
        // Parse strictly to validate
        if let Ok(dt) = NaiveDateTime::parse_from_str(&ts_with_year, "%Y %b %d %H:%M:%S") {
            let timestamp = dt.format("%Y-%m-%d %H:%M:%S,000").to_string();

            // Syslog usually has implicit level (PRI), we'll default to INFO or extract if available
            // Here we just default to INFO as regex doesn't capture level.
            // The "app" might be "kernel", "sshd", etc.
            let app = caps.name("app").map(|m| m.as_str().to_string());
            let host = caps.name("host").map(|m| m.as_str().to_string());

            let match_end = caps.get(0)?.end();
            let body = raw_line[match_end..].trim_end_matches('\n').to_string();

            // Reconstruct full body with host/app if possible
            let full_body = if let (Some(h), Some(a)) = (&host, &app) {
                format!("{} {}: {}", h, a, body)
            } else {
                body.to_string()
            };

            return Some(LogEntry {
                timestamp: Some(timestamp),
                verbosity_level: LogLevel::INFO,
                component: None,
                template_hash: 0,
                template_str: full_body,
                variables: Vec::new(),
            });
        }
    }

    // Fallback: Raw line
    // Treat the entire line as body, no timestamp.
=======
    // 4. Fallback: RAW (Stack traces, etc)
>>>>>>> master
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
<<<<<<< HEAD

// Function to check if a line is a start of a new log entry
=======
>>>>>>> master
