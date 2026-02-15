use chrono::NaiveDateTime;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

use crate::models::{CompressedChunk, LogEntry, RawChunk};

// 1. Chunk Accumulator (Single Threaded - Fast)
pub struct LogAccumulator {
    registry: HashMap<u64, u32>,
    template_store: Vec<String>,
    ts_col: Vec<u64>,
    lvl_col: Vec<u8>,
    id_col: Vec<u32>,
    var_col: Vec<String>,
    max_lines_per_chunk: usize,
    current_line_count: usize,
    last_template_count: usize,
    chunk_counter: usize,
}

impl LogAccumulator {
    pub fn new() -> Self {
        LogAccumulator {
            registry: HashMap::new(),
            template_store: Vec::new(),
            ts_col: Vec::new(),
            lvl_col: Vec::new(),
            id_col: Vec::new(),
            var_col: Vec::new(),
            max_lines_per_chunk: 200_000,
            current_line_count: 0,
            last_template_count: 0,
            chunk_counter: 0,
        }
    }

    pub fn ingest(&mut self, entry: LogEntry) -> Option<RawChunk> {
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

        self.current_line_count += 1;

        println!("Current line count: {}", self.current_line_count);

        if self.current_line_count >= self.max_lines_per_chunk {
            println!("Chunk size exceeded. Taking chunk.");
            return Some(self.take_chunk());
        }
        None
    }

    pub fn take_chunk(&mut self) -> RawChunk {
        let registry_delta = self.template_store[self.last_template_count..].to_vec();
        self.last_template_count = self.template_store.len();

        let chunk = RawChunk {
            chunk_id: self.chunk_counter,
            registry_delta,
            ts_col: std::mem::take(&mut self.ts_col),
            lvl_col: std::mem::take(&mut self.lvl_col),
            id_col: std::mem::take(&mut self.id_col),
            var_col: std::mem::take(&mut self.var_col),
        };

        self.chunk_counter += 1;
        self.current_line_count = 0;

        // Re-allocate with capacity to avoid frequent reallocs
        self.ts_col.reserve(self.max_lines_per_chunk);
        self.lvl_col.reserve(self.max_lines_per_chunk);
        self.id_col.reserve(self.max_lines_per_chunk);
        self.var_col.reserve(self.max_lines_per_chunk * 2); // heuristic

        chunk
    }
}

// 2. Compression Logic (Stateless - Parallel)
pub fn compress_chunk(raw: RawChunk) -> std::io::Result<CompressedChunk> {
    let mut raw_size = 0;

    // 1. Registry
    let registry_data = serde_json::to_vec(&raw.registry_delta)?;
    raw_size += registry_data.len();
    let registry_blob = zstd::encode_all(&registry_data[..], 0)?;

    // 2. Timestamps
    let mut ts_bytes = Vec::with_capacity(raw.ts_col.len() * 8);
    for ts in &raw.ts_col {
        ts_bytes.extend_from_slice(&ts.to_le_bytes());
    }
    raw_size += ts_bytes.len();
    let ts_blob = zstd::encode_all(&ts_bytes[..], 0)?;

    // 3. Levels
    raw_size += raw.lvl_col.len();
    let lvl_blob = zstd::encode_all(&raw.lvl_col[..], 0)?;

    // 4. IDs
    let mut id_bytes = Vec::with_capacity(raw.id_col.len() * 4);
    for id in &raw.id_col {
        id_bytes.extend_from_slice(&id.to_le_bytes());
    }
    raw_size += id_bytes.len();
    let id_blob = zstd::encode_all(&id_bytes[..], 0)?;

    // 5. Variables
    let var_data = serde_json::to_vec(&raw.var_col)?;
    raw_size += var_data.len();
    let var_blob = zstd::encode_all(&var_data[..], 0)?;

    Ok(CompressedChunk {
        chunk_id: raw.chunk_id,
        raw_size_bytes: raw_size,
        registry_blob,
        ts_blob,
        lvl_blob,
        id_blob,
        var_blob,
    })
}

// 3. Writer (Single Threaded - Serial)
pub struct ChunkWriter {
    writer: std::io::BufWriter<File>,
}

impl ChunkWriter {
    pub fn new(filepath: &str) -> std::io::Result<Self> {
        let file = File::create(filepath)?;
        let mut writer = std::io::BufWriter::new(file);
        writer.write_all(b"SALC")?;
        Ok(ChunkWriter { writer })
    }

    pub fn write_chunk(&mut self, chunk: CompressedChunk) -> std::io::Result<()> {
        let writer = &mut self.writer;

        // 1. Registry
        writer.write_all(&u32::to_le_bytes(chunk.registry_blob.len() as u32))?;
        writer.write_all(&chunk.registry_blob)?;

        // 2. Timestamps
        writer.write_all(&u32::to_le_bytes(chunk.ts_blob.len() as u32))?;
        writer.write_all(&chunk.ts_blob)?;

        // 3. Levels
        writer.write_all(&u32::to_le_bytes(chunk.lvl_blob.len() as u32))?;
        writer.write_all(&chunk.lvl_blob)?;

        // 4. IDs
        writer.write_all(&u32::to_le_bytes(chunk.id_blob.len() as u32))?;
        writer.write_all(&chunk.id_blob)?;

        // 5. Variables
        writer.write_all(&u32::to_le_bytes(chunk.var_blob.len() as u32))?;
        writer.write_all(&chunk.var_blob)?;

        Ok(())
    }

    pub fn finish(&mut self) -> std::io::Result<()> {
        self.writer.flush()?;
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
