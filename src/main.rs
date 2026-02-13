use chrono::{DateTime, NaiveDateTime};
use lazy_static::lazy_static;
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fmt;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read, Write};

#[derive(Debug, PartialEq, Clone, Serialize)]
pub enum LogLevel {
    INFO = 1,
    DEBUG = 2,
    ERROR = 3,
    WARN = 4,
    UNKNOWN = 0,
}

impl LogLevel {
    fn to_u8(&self) -> u8 {
        match self {
            LogLevel::INFO => 1,
            LogLevel::DEBUG => 2,
            LogLevel::ERROR => 3,
            LogLevel::WARN => 4,
            LogLevel::UNKNOWN => 0,
        }
    }

    fn from_u8(val: u8) -> Self {
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

pub struct LogCompressor {
    registry: HashMap<u64, u32>,
    template_store: Vec<String>,
    ts_col: Vec<u64>,
    lvl_col: Vec<u8>,
    id_col: Vec<u32>,
    var_col: Vec<String>,
}

impl LogCompressor {
    pub fn new() -> Self {
        LogCompressor {
            registry: HashMap::new(),
            template_store: Vec::new(),
            ts_col: Vec::new(),
            lvl_col: Vec::new(),
            id_col: Vec::new(),
            var_col: Vec::new(),
        }
    }

    pub fn ingest(&mut self, entry: LogEntry) {
        // 1. Handle Timestamp
        let ts_millis = parse_timestamp_millis(&entry.timestamp);
        self.ts_col.push(ts_millis);

        // 2. Handle Level
        self.lvl_col.push(entry.verbosity_level.to_u8());

        // 3. Handle Template ID
        let id = if let Some(&existing_id) = self.registry.get(&entry.template_hash) {
            existing_id
        } else {
            let new_id = self.template_store.len() as u32;
            self.registry.insert(entry.template_hash, new_id);
            self.template_store.push(entry.template_str);
            new_id
        };
        self.id_col.push(id);

        // 4. Handle Variables
        self.var_col.extend(entry.variables);
    }

    pub fn save(&self, filepath: &str) -> std::io::Result<()> {
        let file = File::create(filepath)?;
        let mut writer = std::io::BufWriter::new(file);

        // Header: Magic Bytes
        writer.write_all(b"SALC")?;

        // Block 1: Compressed Registry (TemplateStore)
        let registry_data = serde_json::to_vec(&self.template_store)?;
        let compressed_registry = zstd::encode_all(&registry_data[..], 0)?;
        writer.write_all(&u32::to_le_bytes(compressed_registry.len() as u32))?;
        writer.write_all(&compressed_registry)?;

        // Block 2: Compressed Timestamps
        let mut ts_bytes = Vec::with_capacity(self.ts_col.len() * 8);
        for ts in &self.ts_col {
            ts_bytes.extend_from_slice(&ts.to_le_bytes());
        }
        let compressed_ts = zstd::encode_all(&ts_bytes[..], 0)?;
        writer.write_all(&u32::to_le_bytes(compressed_ts.len() as u32))?;
        writer.write_all(&compressed_ts)?;

        // Block 3: Compressed Levels (New)
        let compressed_lvl = zstd::encode_all(&self.lvl_col[..], 0)?;
        writer.write_all(&u32::to_le_bytes(compressed_lvl.len() as u32))?;
        writer.write_all(&compressed_lvl)?;

        // Block 4: Compressed IDs
        let mut id_bytes = Vec::with_capacity(self.id_col.len() * 4);
        for id in &self.id_col {
            id_bytes.extend_from_slice(&id.to_le_bytes());
        }
        let compressed_ids = zstd::encode_all(&id_bytes[..], 0)?;
        writer.write_all(&u32::to_le_bytes(compressed_ids.len() as u32))?;
        writer.write_all(&compressed_ids)?;

        // Block 5: Compressed Variables
        let var_data = serde_json::to_vec(&self.var_col)?;
        let compressed_vars = zstd::encode_all(&var_data[..], 0)?;
        writer.write_all(&u32::to_le_bytes(compressed_vars.len() as u32))?;
        writer.write_all(&compressed_vars)?;

        writer.flush()?;
        Ok(())
    }
}

// Helper function to robustly parse timestamp to millis
fn parse_timestamp_millis(raw_ts: &str) -> u64 {
    // 1. Normalize comma to dot
    let normalized = raw_ts.replace(',', ".");

    // 2. Pad fractional part to 9 digits if present
    // Split into integer part and fractional part
    let parts: Vec<&str> = normalized.split('.').collect();

    let parsable_ts = if parts.len() == 2 {
        let frac = parts[1];
        if frac.len() < 9 {
            // Pad with zeros to right
            format!("{}.{:0<9}", parts[0], frac)
        } else {
            normalized.clone()
        }
    } else {
        normalized.clone()
    };

    if let Ok(dt) = NaiveDateTime::parse_from_str(&parsable_ts, "%Y-%m-%d %H:%M:%S.%f") {
        dt.and_utc().timestamp_millis() as u64
    } else if let Ok(dt) = NaiveDateTime::parse_from_str(&parsable_ts, "%Y-%m-%d %H:%M:%S") {
        dt.and_utc().timestamp_millis() as u64
    } else {
        0
    }
}

pub struct LogDecompressor;

impl LogDecompressor {
    pub fn decompress(input_path: &str, output_path: &str) -> std::io::Result<()> {
        let mut file = File::open(input_path)?;

        // 1. Magic Bytes check
        let mut magic = [0u8; 4];
        file.read_exact(&mut magic)?;
        if &magic != b"SALC" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid file format",
            ));
        }

        // Helper to read compressed block
        let read_block = |f: &mut File| -> std::io::Result<Vec<u8>> {
            let mut size_buf = [0u8; 4];
            f.read_exact(&mut size_buf)?;
            let size = u32::from_le_bytes(size_buf) as usize;
            let mut compressed_data = vec![0u8; size];
            f.read_exact(&mut compressed_data)?;
            zstd::decode_all(&compressed_data[..])
        };

        // 2. Deserialize Blocks
        // Block 1: Registry
        let registry_bytes = read_block(&mut file)?;
        let template_store: Vec<String> = serde_json::from_slice(&registry_bytes)?;

        // Block 2: Timestamps
        let ts_bytes = read_block(&mut file)?;
        let mut ts_col = Vec::new();
        for chunk in ts_bytes.chunks_exact(8) {
            let ts = u64::from_le_bytes(chunk.try_into().unwrap());
            ts_col.push(ts);
        }

        // Block 3: Levels
        let lvl_bytes = read_block(&mut file)?;
        let lvl_col: Vec<u8> = lvl_bytes;

        // Block 4: IDs
        let id_bytes = read_block(&mut file)?;
        let mut id_col = Vec::new();
        for chunk in id_bytes.chunks_exact(4) {
            let id = u32::from_le_bytes(chunk.try_into().unwrap());
            id_col.push(id);
        }

        // Block 5: Variables
        let var_bytes = read_block(&mut file)?;
        let var_col: Vec<String> = serde_json::from_slice(&var_bytes)?;

        // 3. Reconstruction Loop
        let output_file = File::create(output_path)?;
        let mut writer = std::io::BufWriter::new(output_file);

        let mut var_idx = 0;
        for i in 0..id_col.len() {
            let id = id_col[i] as usize;
            if id >= template_store.len() {
                continue;
            }
            let template_str = &template_store[id];

            // Count <VAR>
            let var_count = template_str.matches("<VAR>").count();

            // Extract variables
            let mut current_vars = Vec::new();
            for _ in 0..var_count {
                if var_idx < var_col.len() {
                    current_vars.push(&var_col[var_idx]);
                    var_idx += 1;
                }
            }

            // Interpolate
            let mut reconstructed = String::new();
            let parts: Vec<&str> = template_str.split("<VAR>").collect();

            for (j, part) in parts.iter().enumerate() {
                reconstructed.push_str(part);
                if j < current_vars.len() {
                    reconstructed.push_str(current_vars[j]);
                }
            }

            // Format Timestamp
            let secs = (ts_col[i] / 1000) as i64;
            let nsecs = ((ts_col[i] % 1000) * 1_000_000) as u32;
            let dt = DateTime::from_timestamp(secs, nsecs).unwrap_or_default();
            // Restore timestamp format (using comma for millis as seen in source)
            let ts_str = dt.format("%Y-%m-%d %H:%M:%S,%3f").to_string();

            // Restore Level
            let lvl = LogLevel::from_u8(if i < lvl_col.len() { lvl_col[i] } else { 0 });
            let lvl_str = if lvl == LogLevel::UNKNOWN {
                String::new()
            } else {
                lvl.to_string()
            };

            if lvl_str.is_empty() {
                write!(writer, "{} {}", ts_str, reconstructed)?;
            } else {
                write!(writer, "{} {} {}", ts_str, lvl_str, reconstructed)?;
            }
        }

        writer.flush()?;
        Ok(())
    }
}

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
    // We want to replace Variables with <VAR> but keep whitespace/newlines intact.
    // Order matters in regex alternation constructed via ORing:
    // 1. Strings (Quotes)
    // 2. IP Addresses
    // 3. Paths (heuristic: contains / and maybe digits/dots/hyphens)
    // 4. Digits (integers, decimals)

    // Combining into one massive regex is safer for tokenization-replacement
    lazy_static! {
        static ref VAR_REGEX: Regex = Regex::new(
            r#"(?x)
            # 1. Quoted Strings (Single or Double)
            (['"][^'"]*['"]) |
            # 2. IP Addresses (IPv4)
            (\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b) |
            # 3. File Paths (Must contain /, allowing alphanumeric, dots, hyphens, underscores)
            ([\w\.\-_]*\/[\w\.\-/_]*) |
            # 4. Digits (Hex, Decimals, Integers) - \b boundary check to avoid partial word match?
            # Relaxed: just sequences of digits if not part of a word?
            # Or safer: \b\d+\b ? Let's use user's heuristic: "Contains digits" token. 
            # Implies: if a word has a digit, it's a var? Regex replace is trickier.
            # Let's stick to explicit Digits or WordsWithDigits
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
fn is_log_start(line: &str) -> bool {
    lazy_static! {
        static ref START_RE: Regex = Regex::new(r"^\d{4}-\d{2}-\d{2}").unwrap();
    }
    START_RE.is_match(line)
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: {} <mode> <input_file> <output_file>", args[0]);
        eprintln!(
            "Modes: \n  compress   - Compress log file to .salc\n  decompress - Decompress .salc file to text"
        );
        std::process::exit(1);
    }

    let mode = &args[1];
    let input_path = &args[2];
    let output_path = &args[3];

    match mode.as_str() {
        "compress" => {
            println!("Compressing {} -> {}", input_path, output_path);
            let input_file = File::open(input_path)?;
            let mut reader = BufReader::new(input_file);

            let mut compressor = LogCompressor::new();
            let mut count = 0;

            // Multi-line Aggregation Loop
            let mut current_entry_lines = String::new();

            let mut line_buffer = String::new();
            while reader.read_line(&mut line_buffer)? > 0 {
                // Check if this line starts a new log entry
                if is_log_start(&line_buffer) {
                    // If we have a previous entry accumulated, process it
                    if !current_entry_lines.is_empty() {
                        // Parse and Ingest
                        if let Some(entry) = parse_line(&current_entry_lines) {
                            compressor.ingest(entry);
                            count += 1;
                        } else {
                            // Fallback: This "New Entry" might be unparsable, or previous buffer was junk.
                        }
                    }
                    // Start new accumulator
                    current_entry_lines = line_buffer.clone();
                } else {
                    // Continuation line (Stack trace, etc.)
                    // Append to current accumulator
                    current_entry_lines.push_str(&line_buffer);
                }

                line_buffer.clear();
            }

            // Process the final accumulated entry
            if !current_entry_lines.is_empty() {
                if let Some(entry) = parse_line(&current_entry_lines) {
                    compressor.ingest(entry);
                    count += 1;
                }
            }

            compressor.save(output_path)?;
            println!("Done. Processed {} entries.", count);
        }
        "decompress" => {
            println!("Decompressing {} -> {}", input_path, output_path);
            LogDecompressor::decompress(input_path, output_path)?;
            println!("Done.");
        }
        _ => {
            eprintln!("Invalid mode. Use 'compress' or 'decompress'.");
            std::process::exit(1);
        }
    }

    Ok(())
}
