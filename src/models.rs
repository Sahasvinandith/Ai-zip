use serde::Serialize;
use std::fmt;

#[derive(Debug, PartialEq, Clone, Serialize)]
pub enum LogLevel {
    INFO = 1,
    DEBUG = 2,
    ERROR = 3,
    WARN = 4,
    UNKNOWN = 0,
}

impl LogLevel {
    pub fn to_u8(&self) -> u8 {
        match self {
            LogLevel::INFO => 1,
            LogLevel::DEBUG => 2,
            LogLevel::ERROR => 3,
            LogLevel::WARN => 4,
            LogLevel::UNKNOWN => 0,
        }
    }

    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => LogLevel::INFO,
            2 => LogLevel::DEBUG,
            3 => LogLevel::ERROR,
            4 => LogLevel::WARN,
            _ => LogLevel::UNKNOWN,
        }
    }
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

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub verbosity_level: LogLevel,
    pub component: Option<String>,
    pub template_hash: u64,
    pub template_str: String,
    pub variables: Vec<String>,
}
