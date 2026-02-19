use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, PartialEq, Clone, Serialize)]
pub enum LogLevel {
    INFO = 1,
    DEBUG = 2,
    ERROR = 3,
    WARN = 4,
    FATAL = 5,
    TRACE = 6,
    RAW = 255,
    UNKNOWN = 0,
}

impl LogLevel {
    pub fn to_u8(&self) -> u8 {
        match self {
            LogLevel::INFO => 1,
            LogLevel::DEBUG => 2,
            LogLevel::ERROR => 3,
            LogLevel::WARN => 4,
            LogLevel::FATAL => 5,
            LogLevel::TRACE => 6,
            LogLevel::RAW => 255,
            LogLevel::UNKNOWN => 0,
        }
    }

    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => LogLevel::INFO,
            2 => LogLevel::DEBUG,
            3 => LogLevel::ERROR,
            4 => LogLevel::WARN,
            5 => LogLevel::FATAL,
            6 => LogLevel::TRACE,
            255 => LogLevel::RAW,
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
            "FATAL" => LogLevel::FATAL,
            "TRACE" => LogLevel::TRACE,
            "RAW" => LogLevel::RAW,
            _ => LogLevel::UNKNOWN,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: Option<String>,
    pub verbosity_level: LogLevel,
    // pub component: Option<String>,
    // pub template_hash: u64,
    pub template_str: String,
    // pub variables: Vec<String>,
    pub has_newline: bool,
}

#[derive(Debug, Clone)]
pub struct PreDigestedEntry {
    pub timestamp: Option<String>,
    pub verbosity_level: LogLevel,
    pub template_id: u32,
    pub variables: Vec<String>,
    pub has_newline: bool,
}

#[derive(Debug)]
pub struct RawChunk {
    pub chunk_id: usize,
    pub registry_delta: Vec<String>,
    pub ts_col: Vec<u64>,
    pub lvl_col: Vec<u8>,
    pub id_col: Vec<u32>,
    pub var_col: Vec<String>,
    pub nl_col: Vec<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CompressedChunk {
    pub chunk_id: usize,
    pub registry_blob: Vec<u8>,
    pub ts_blob: Vec<u8>,
    pub lvl_blob: Vec<u8>,
    pub id_blob: Vec<u8>,
    pub var_blob: Vec<u8>,
    pub nl_blob: Vec<u8>,
}
