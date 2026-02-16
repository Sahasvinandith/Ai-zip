use chrono::NaiveDateTime;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

use crate::models::{CompressedChunk, PreDigestedEntry, RawChunk};
// use crate::models::{CompressedChunk, LogEntry, LogLevel, PreDigestedEntry, RawChunk};
// Removed unused imports
use std::sync::{Arc, RwLock};

// 1. Chunk Accumulator (Single Threaded - Fast)
pub struct LogAccumulator {
    ts_col: Vec<u64>,
    lvl_col: Vec<u8>,
    id_col: Vec<u32>,
    var_col: Vec<String>,
    max_lines_per_chunk: usize,
    current_line_count: usize,
    last_template_count: usize, // We need to track registry state
    chunk_counter: usize,
    registry: Arc<SharedRegistry>, // Reference to shared registry to get delta
}

impl LogAccumulator {
    pub fn new(registry: Arc<SharedRegistry>) -> Self {
        LogAccumulator {
            ts_col: Vec::new(),
            lvl_col: Vec::new(),
            id_col: Vec::new(),
            var_col: Vec::new(),
            max_lines_per_chunk: 200_000,
            current_line_count: 0,
            last_template_count: 0,
            chunk_counter: 0,
            registry,
        }
    }

    pub fn ingest(&mut self, entry: PreDigestedEntry) -> Option<RawChunk> {
        // 1. Handle Timestamp
        let ts_millis = parse_timestamp_millis(&entry.timestamp);
        self.ts_col.push(ts_millis);

        // 2. Handle Level
        self.lvl_col.push(entry.verbosity_level.to_u8());

        // 3. Handle Template ID (Already looked up!)
        self.id_col.push(entry.template_id);

        // 4. Handle Variables
        self.var_col.extend(entry.variables);

        self.current_line_count += 1;

        if self.current_line_count >= self.max_lines_per_chunk {
            return Some(self.take_chunk());
        }
        None
    }

    pub fn take_chunk(&mut self) -> RawChunk {
        // Get new templates added since last chunk
        let current_store = self.registry.template_store.read().unwrap();
        let registry_delta = current_store[self.last_template_count..].to_vec();
        self.last_template_count = current_store.len();
        drop(current_store); // Release lock ASAP

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

        // Re-allocate
        self.ts_col.reserve(self.max_lines_per_chunk);
        self.lvl_col.reserve(self.max_lines_per_chunk);
        self.id_col.reserve(self.max_lines_per_chunk);
        self.var_col.reserve(self.max_lines_per_chunk * 2);

        chunk
    }
}

// Thread-Safe Registry for Parallel Lookup
pub struct SharedRegistry {
    map: RwLock<HashMap<u64, u32>>,
    template_store: RwLock<Vec<String>>,
}

impl SharedRegistry {
    pub fn new() -> Self {
        SharedRegistry {
            map: RwLock::new(HashMap::new()),
            template_store: RwLock::new(Vec::new()),
        }
    }

    pub fn get_or_register(&self, template_str: String, template_hash: u64) -> u32 {
        // 1. Optimistic Read
        {
            let map_read = self.map.read().unwrap();
            if let Some(&id) = map_read.get(&template_hash) {
                return id;
            }
        }

        // 2. Write (Lock Escalation)
        let mut map_write = self.map.write().unwrap();
        let mut store_write = self.template_store.write().unwrap();

        // Double check (another thread might have inserted it)
        if let Some(&id) = map_write.get(&template_hash) {
            return id;
        }

        let new_id = store_write.len() as u32;
        map_write.insert(template_hash, new_id);
        store_write.push(template_str);
        new_id
    }

    pub fn dump(&self) -> Vec<(u32, u64, String)> {
        let store = self.template_store.read().unwrap();
        let map = self.map.read().unwrap();

        let mut result = Vec::with_capacity(store.len());

        // Invert map for display: map is Hash -> ID
        // unique ID -> Hash (One-to-one mapping in theory, but multiple hashes could map to same ID if collision logic was weird, but here it's 1:1)
        // Actually, get_or_register: if hash exists -> return ID.
        // So multiple hashes could point to same ID?
        // No, map.insert(hash, new_id). New ID is only created if hash NOT in map.
        // So Hash <-> ID is 1:1.

        // Let's iterate the map to link Hash to ID.
        for (&hash, &id) in map.iter() {
            if (id as usize) < store.len() {
                result.push((id, hash, store[id as usize].clone()));
            }
        }

        // Sort by ID
        result.sort_by_key(|k| k.0);
        result
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
    buffer: Vec<u8>,
    buffer_limit: usize,
}

impl ChunkWriter {
    pub fn new(filepath: &str) -> std::io::Result<Self> {
        let file = File::create(filepath)?;
        let mut writer = std::io::BufWriter::new(file);
        writer.write_all(b"SALC")?;
        Ok(ChunkWriter {
            writer,
            buffer: Vec::with_capacity(20 * 1024 * 1024), // Pre-alloc 20MB
            buffer_limit: 20 * 1024 * 1024,
        })
    }

    pub fn write_chunk(&mut self, chunk: CompressedChunk) -> std::io::Result<()> {
        // Calculate total size of this chunk payload
        let chunk_size = 4
            + chunk.registry_blob.len()
            + 4
            + chunk.ts_blob.len()
            + 4
            + chunk.lvl_blob.len()
            + 4
            + chunk.id_blob.len()
            + 4
            + chunk.var_blob.len();

        // Flush if buffer would overflow
        if self.buffer.len() + chunk_size > self.buffer_limit {
            self.writer.write_all(&self.buffer)?;
            self.buffer.clear();
        }

        // Write to Memory Buffer
        // 1. Registry
        self.buffer
            .extend_from_slice(&u32::to_le_bytes(chunk.registry_blob.len() as u32));
        self.buffer.extend_from_slice(&chunk.registry_blob);

        // 2. Timestamps
        self.buffer
            .extend_from_slice(&u32::to_le_bytes(chunk.ts_blob.len() as u32));
        self.buffer.extend_from_slice(&chunk.ts_blob);

        // 3. Levels
        self.buffer
            .extend_from_slice(&u32::to_le_bytes(chunk.lvl_blob.len() as u32));
        self.buffer.extend_from_slice(&chunk.lvl_blob);

        // 4. IDs
        self.buffer
            .extend_from_slice(&u32::to_le_bytes(chunk.id_blob.len() as u32));
        self.buffer.extend_from_slice(&chunk.id_blob);

        // 5. Variables
        self.buffer
            .extend_from_slice(&u32::to_le_bytes(chunk.var_blob.len() as u32));
        self.buffer.extend_from_slice(&chunk.var_blob);

        Ok(())
    }

    pub fn finish(&mut self) -> std::io::Result<()> {
        if !self.buffer.is_empty() {
            self.writer.write_all(&self.buffer)?;
            self.buffer.clear();
        }
        self.writer.flush()?;
        Ok(())
    }
}

pub struct DebugChunkWriter {
    writer: std::io::BufWriter<File>,
    registry_ref: Arc<SharedRegistry>,
}

impl DebugChunkWriter {
    pub fn new(filepath: &str, registry_ref: Arc<SharedRegistry>) -> std::io::Result<Self> {
        let file = File::create(filepath)?;
        let writer = std::io::BufWriter::new(file);
        Ok(DebugChunkWriter {
            writer,
            registry_ref,
        })
    }

    pub fn write_chunk(&mut self, chunk: RawChunk) -> std::io::Result<()> {
        writeln!(self.writer, "=== CHUNK {} ===", chunk.chunk_id)?;
        writeln!(self.writer, "Rows: {}", chunk.ts_col.len())?;

        writeln!(
            self.writer,
            "TS_COL (First 10): {:?}",
            chunk.ts_col.iter().take(10).collect::<Vec<_>>()
        )?;
        writeln!(
            self.writer,
            "LVL_COL (First 10): {:?}",
            chunk.lvl_col.iter().take(10).collect::<Vec<_>>()
        )?;
        writeln!(
            self.writer,
            "ID_COL (First 10): {:?}",
            chunk.id_col.iter().take(10).collect::<Vec<_>>()
        )?;
        writeln!(
            self.writer,
            "VAR_COL (First 10): {:?}",
            chunk.var_col.iter().take(10).collect::<Vec<_>>()
        )?;

        writeln!(self.writer, "================\n")?;
        Ok(())
    }
    pub fn finish(&mut self) -> std::io::Result<()> {
        writeln!(self.writer, "=== REGISTRY DUMP ===")?;
        let snapshot = self.registry_ref.dump();
        for (id, hash, tmpl) in snapshot {
            writeln!(self.writer, "{}: [{:016x}] \"{}\"", id, hash, tmpl)?;
        }
        self.writer.flush()?;
        Ok(())
    }
}

pub struct BenchmarkWriter {
    writer: std::io::BufWriter<File>,
    registry_ref: Arc<SharedRegistry>,
    total_rows: usize,
}

impl BenchmarkWriter {
    pub fn new(filepath: &str, registry_ref: Arc<SharedRegistry>) -> std::io::Result<Self> {
        let file = File::create(filepath)?;
        let writer = std::io::BufWriter::new(file);
        Ok(BenchmarkWriter {
            writer,
            registry_ref,
            total_rows: 0,
        })
    }

    pub fn write_chunk(&mut self, chunk: RawChunk) -> std::io::Result<()> {
        self.total_rows += chunk.ts_col.len();
        Ok(())
    }

    pub fn finish(&mut self) -> std::io::Result<()> {
        writeln!(
            self.writer,
            "Benchmark Complete. Processed {} rows.",
            self.total_rows
        )?;
        writeln!(self.writer, "=== REGISTRY DUMP ===")?;
        let snapshot = self.registry_ref.dump();

        for (id, hash, tmpl) in snapshot {
            writeln!(self.writer, "{}: [{:016x}] \"{}\"", id, hash, tmpl)?;
        }
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
