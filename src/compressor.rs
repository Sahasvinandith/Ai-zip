use chrono::NaiveDateTime;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

use crate::drain_registry::DrainRegistry;
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
    nl_col: Vec<bool>,
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
            nl_col: Vec::new(),
            max_lines_per_chunk: 200_000,
            current_line_count: 0,
            last_template_count: 0,
            chunk_counter: 0,
            registry,
        }
    }

    pub fn ingest(&mut self, entry: PreDigestedEntry) -> Option<RawChunk> {
        // 1. Handle Timestamp
        let ts_millis = parse_timestamp_millis(entry.timestamp.as_deref().unwrap_or(""));
        self.ts_col.push(ts_millis);

        // 2. Handle Level
        self.lvl_col.push(entry.verbosity_level.to_u8());

        // 3. Handle Template ID (Already looked up!)
        self.id_col.push(entry.template_id);

        // 4. Handle Variables
        self.var_col.extend(entry.variables);

        // 5. Handle Newline
        self.nl_col.push(entry.has_newline);

        self.current_line_count += 1;

        if self.current_line_count >= self.max_lines_per_chunk {
            return Some(self.take_chunk());
        }
        None
    }

    pub fn take_chunk(&mut self) -> RawChunk {
        // Get new templates added since last chunk
        let current_store = self.registry.template_store.read().unwrap();

        let registry_delta: Vec<String> = current_store[self.last_template_count..]
            .iter()
            .map(|(_, tmpl)| tmpl.clone())
            .collect();

        self.last_template_count = current_store.len();
        drop(current_store); // Release lock ASAP

        let chunk = RawChunk {
            chunk_id: self.chunk_counter,
            registry_delta,
            ts_col: std::mem::take(&mut self.ts_col),
            lvl_col: std::mem::take(&mut self.lvl_col),
            id_col: std::mem::take(&mut self.id_col),
            var_col: std::mem::take(&mut self.var_col),
            nl_col: std::mem::take(&mut self.nl_col),
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

// use crate::models::{CompressedChunk, LogEntry, LogLevel, PreDigestedEntry, RawChunk};
// Removed unused imports

// 1. Chunk Accumulator (Single Threaded - Fast)
// ... LogAccumulator logic will be updated shortly ...

// Thread-Safe Registry for KEY (Drain64) -> VALUE (LocalSeq32) mapping
// This maps the 64-bit template hash from Drain to a sequential 32-bit ID for compression.
pub struct SharedRegistry {
    id_map: RwLock<HashMap<u64, u32>>,
    template_store: RwLock<Vec<(u64, String)>>, // (Hash, TemplateString)
    drain: DrainRegistry,
}

impl SharedRegistry {
    pub fn new() -> Self {
        SharedRegistry {
            id_map: RwLock::new(HashMap::new()),
            template_store: RwLock::new(Vec::new()),
            drain: DrainRegistry::new(),
        }
    }

    /// Gets the template string + ID + Variables for the raw content.
    /// This performs both "Drain Learning" AND "Registry Lookup/Allocation".
    pub fn get_or_learn(&self, content: &str) -> (u32, String, Vec<String>) {
        // 1. Ask Drain for Template + Variables
        let (drain_hash, template_str, vars) = self.drain.get_or_learn(content);

        // 2. Map Drain Hash (u64) -> Local Sequential ID (u32)
        // Optimistic Read
        {
            let map_read = self.id_map.read().unwrap();
            if let Some(&local_id) = map_read.get(&drain_hash) {
                return (local_id, template_str, vars);
            }
        }

        // Write Lock
        let mut map_write = self.id_map.write().unwrap();
        let mut store_write = self.template_store.write().unwrap();

        if let Some(&local_id) = map_write.get(&drain_hash) {
            return (local_id, template_str, vars);
        }

        let new_id = store_write.len() as u32;
        map_write.insert(drain_hash, new_id);
        store_write.push((drain_hash, template_str.clone()));

        (new_id, template_str, vars)
    }

    pub fn dump(&self) -> Vec<(u32, u64, String)> {
        let store = self.template_store.read().unwrap();

        // Store has (Hash, Template) at index ID
        let mut result = Vec::with_capacity(store.len());
        for (id, (hash, tmpl)) in store.iter().enumerate() {
            result.push((id as u32, *hash, tmpl.clone()));
        }
        result
    }
}

// 2. Compression Logic (Stateless - Parallel)
pub fn compress_chunk(raw: RawChunk) -> std::io::Result<CompressedChunk> {
    let zstd_level = 3; // Level 3: good balance of speed and ratio

    // 1. Registry
    let registry_data = serde_json::to_vec(&raw.registry_delta)?;
    let registry_blob = zstd::encode_all(&registry_data[..], zstd_level)?;

    // 2. Timestamps (Delta-Encoded)
    // First timestamp is absolute u64, rest are i64 deltas
    let mut ts_bytes = Vec::with_capacity(raw.ts_col.len() * 8);
    if let Some(&first) = raw.ts_col.first() {
        ts_bytes.extend_from_slice(&first.to_le_bytes()); // 8 bytes absolute
        let mut prev = first;
        for &ts in &raw.ts_col[1..] {
            let delta = ts as i64 - prev as i64;
            ts_bytes.extend_from_slice(&delta.to_le_bytes()); // 8 bytes delta
            prev = ts;
        }
    }
    let ts_blob = zstd::encode_all(&ts_bytes[..], zstd_level)?;

    // 3. Levels
    let lvl_blob = zstd::encode_all(&raw.lvl_col[..], zstd_level)?;

    // 4. IDs
    let mut id_bytes = Vec::with_capacity(raw.id_col.len() * 4);
    for id in &raw.id_col {
        id_bytes.extend_from_slice(&id.to_le_bytes());
    }
    let id_blob = zstd::encode_all(&id_bytes[..], zstd_level)?;

    // 5. Variables (Length-Prefixed Binary)
    // Format: [u16 len][raw bytes][u16 len][raw bytes]...
    let mut var_data = Vec::with_capacity(raw.var_col.len() * 20);
    for var in &raw.var_col {
        let bytes = var.as_bytes();
        var_data.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        var_data.extend_from_slice(bytes);
    }
    let var_blob = zstd::encode_all(&var_data[..], zstd_level)?;

    // 6. Newlines (Bitpacked)
    let mut nl_bytes = Vec::with_capacity((raw.nl_col.len() + 7) / 8);
    let mut byte: u8 = 0;
    let mut bit = 0;
    for &has_nl in &raw.nl_col {
        if has_nl {
            byte |= 1 << bit;
        }
        bit += 1;
        if bit == 8 {
            nl_bytes.push(byte);
            byte = 0;
            bit = 0;
        }
    }
    if bit > 0 {
        nl_bytes.push(byte);
    }
    let nl_blob = zstd::encode_all(&nl_bytes[..], zstd_level)?;

    Ok(CompressedChunk {
        chunk_id: raw.chunk_id,
        registry_blob,
        ts_blob,
        lvl_blob,
        id_blob,
        var_blob,
        nl_blob,
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
        writer.write_all(b"STZ1")?;
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
            + chunk.var_blob.len()
            + 4
            + chunk.nl_blob.len();

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

        // 6. Newlines
        self.buffer
            .extend_from_slice(&u32::to_le_bytes(chunk.nl_blob.len() as u32));
        self.buffer.extend_from_slice(&chunk.nl_blob);

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

use crate::parser::parse_line;
use crossbeam_channel::{Receiver, Sender, bounded};
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;
use std::thread;

/// Compress a single file from `input_path` and write the .stz stream to `writer`.
/// This is a self-contained pipeline including reading, parsing, and compressing.
pub fn compress_file<W: Write + Send + 'static>(
    input_path: &Path,
    mut writer: W,
    num_threads: usize,
    debug_mode: bool,
    benchmark_mode: bool,
) -> std::io::Result<()> {
    // 1. Setup Channels
    // Parse Pipeline: Job -> Result (PreDigestedEntry)
    let (job_tx, job_rx): (
        Sender<(usize, String, bool)>,
        Receiver<(usize, String, bool)>,
    ) = bounded(100000);
    let (res_tx, res_rx): (
        Sender<(usize, Option<PreDigestedEntry>)>,
        Receiver<(usize, Option<PreDigestedEntry>)>,
    ) = bounded(100000);

    // Compress Pipeline: RawChunk -> CompressedChunk
    let (raw_chunk_tx, raw_chunk_rx): (Sender<RawChunk>, Receiver<RawChunk>) = bounded(50);

    // Only need comp_chunk channels if NOT debug/benchmark mode
    let (comp_chunk_tx, comp_chunk_rx): (Sender<CompressedChunk>, Receiver<CompressedChunk>) =
        bounded(50);

    // Universal Shared Registry (New for each file)
    let registry = Arc::new(SharedRegistry::new());

    // 2. Spawn Parse Workers
    let parse_threads = num_threads;
    let compress_threads = if debug_mode || benchmark_mode {
        0
    } else {
        if num_threads > 2 { num_threads / 2 } else { 1 }
    };

    println!(
        "Spawning {} Parse Workers for {}",
        parse_threads,
        input_path.display()
    );

    let mut worker_handles = Vec::new(); // sotres handles of parsing threads
    for _ in 0..parse_threads {
        let job_rx_clone = job_rx.clone();
        let res_tx_clone = res_tx.clone();
        let registry_clone = registry.clone(); // Shared Registry

        worker_handles.push(thread::spawn(move || {
            for (seq, line, has_newline) in job_rx_clone {
                if let Some(mut entry) = parse_line(&line) {
                    entry.has_newline = has_newline;
                    // Handle multi-line entries: only send first line to Drain,
                    // preserve continuation lines as raw content.
                    let (first_line, continuation) = match entry.template_str.find('\n') {
                        Some(pos) => (&entry.template_str[..pos], Some(&entry.template_str[pos..])),
                        None => (entry.template_str.as_str(), None),
                    };

                    // Strip trailing \r from first line before sending to Drain
                    let drain_input = first_line.trim_end_matches('\r');

                    let (template_id, _final_template_str, mut variables) =
                        registry_clone.get_or_learn(drain_input);

                    // Append continuation lines as a single raw variable.
                    // If the first line had a trailing \r (stripped for Drain),
                    // prepend it to the continuation to preserve \r\n sequence.
                    if let Some(cont) = continuation {
                        let has_cr = first_line.ends_with('\r');
                        if has_cr {
                            let mut full_cont = String::with_capacity(1 + cont.len());
                            full_cont.push('\r');
                            full_cont.push_str(cont);
                            variables.push(full_cont);
                        } else {
                            variables.push(cont.to_string());
                        }
                    } else if first_line.ends_with('\r') {
                        // Single-line entry with trailing \r: store it as continuation
                        variables.push("\r".to_string());
                    }

                    let pre_digested = PreDigestedEntry {
                        timestamp: entry.timestamp,
                        verbosity_level: entry.verbosity_level,
                        template_id: template_id as u32,
                        variables,
                        has_newline: entry.has_newline,
                    };

                    if res_tx_clone.send((seq, Some(pre_digested))).is_err() {
                        break;
                    }
                } else {
                    if res_tx_clone.send((seq, None)).is_err() {
                        break;
                    }
                }
            }
        }));
    }
    drop(res_tx); // Close locally

    // 3. Spawn Sequencer (Batcher)
    let registry_clone2 = registry.clone();
    let sequencer_handle = thread::spawn(move || -> std::io::Result<usize> {
        let mut accumulator = LogAccumulator::new(registry_clone2);
        let mut buffer: HashMap<usize, Option<PreDigestedEntry>> = HashMap::new();
        let mut next_expected_seq = 0;
        let mut count = 0;

        for (seq, entry) in res_rx {
            buffer.insert(seq, entry);

            // Drain buffer in order
            while let Some(item) = buffer.remove(&next_expected_seq) {
                if let Some(valid_entry) = item {
                    if let Some(chunk) = accumulator.ingest(valid_entry) {
                        raw_chunk_tx.send(chunk).expect("Downstream died");
                    }
                    count += 1;
                }
                next_expected_seq += 1;
            }
        }

        // Flush last partial chunk
        let last_chunk = accumulator.take_chunk();
        if !last_chunk.ts_col.is_empty() {
            raw_chunk_tx.send(last_chunk).expect("Downstream died");
        }

        Ok(count)
    });

    // 4. Input Output Handling
    // 4a. Writer Thread (Consumer)

    // We need to adapt ChunkWriter to take a generic Write.
    // However, ChunkWriter currently owns a BufWriter<File>.
    // To minimize changes, we'll support writing to generic Writer in a slightly modified struct
    // or just pass the Generic Writer to a new kind of ChunkWriter.

    // Let's create a generic Writer handle here.

    let mut writer_handle = None;
    let mut compress_handles = Vec::new();

    if debug_mode {
        // Debug mode Logic
        let registry_clone3 = registry.clone();

        // DebugChunkWriter expects File path. Refactoring it to take writer is annoying.
        // For now, let's just panic or error if debug mode is used with Archive (since we pass a writer).
        // Actually, for single file CLI usage, we pass a file path usually.
        // Let's assume for this Refactor we are focusing on Archive/Stream.
        // If the user provided a generic writer, we can't easily open a file by path inside DebugWriter.
        // So for DebugMode: we need a DebugStreamWriter.

        // ... Ignoring Debug/Benchmark refactor for stream support for now, assuming standard compression ...
        if true {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Debug/Benchmark not supported in stream mode yet",
            ));
        }
    } else if benchmark_mode {
        if true {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Debug/Benchmark not supported in stream mode yet",
            ));
        }
    } else {
        println!("Spawning {} Compress Workers", compress_threads);

        for _ in 0..compress_threads {
            let raw_rx = raw_chunk_rx.clone();
            let comp_tx = comp_chunk_tx.clone();
            compress_handles.push(thread::spawn(move || {
                for raw_chunk in raw_rx {
                    match compress_chunk(raw_chunk) {
                        Ok(comp_chunk) => {
                            comp_tx.send(comp_chunk).unwrap();
                        }
                        Err(e) => eprintln!("Compression Error: {}", e),
                    }
                }
            }));
        }
        // drop(raw_chunk_tx); // Removed because raw_chunk_tx is moved to sequencer

        // Sequencer (Ordering Compressed Chunks)
        // Since we have multiple compress threads, output might be out of order.
        // But `writer` needs to write in order?
        // Actually, the previous implementation didn't strictly re-order compressed chunks
        // because `ChunkWriter` just consumed from `comp_chunk_rx`.
        // Wait, `ChunkWriter` in previous specific implementation just read from `comp_chunk_rx`.
        // If multiple compress threads are running, `comp_chunk_rx` will receive chunks in random completion order.
        // But `ChunkWriter` just wrote them.
        // Does file format support out-of-order chunks?
        // Yes, `chunk_id` is in the chunk struct, but the decompressor reads sequentially.
        // If we write Chunk 2 before Chunk 1, Decompressor reads Chunk 2 first.
        // Decompressor logic:
        // `loop { read_block ... }`
        // It doesn't check chunk_id ordering.
        // BUT `LogAccumulator` relies on `registry_delta` being sequential from `last_template_count`.
        // If Chunk 2 (depends on templates 10-20) is written before Chunk 1 (defines templates 1-10),
        // Decompressor reads Chunk 2, sees it needs templates 10-20.
        // But `template_store` is empty!
        // So CHUNK ORDER MATTERS for registry state consistency.

        // Previous generic implementation:
        // `comp_chunk_rx` was populated by parallel workers.
        // It seems the previous implementation MIGHT HAVE HAD A BUG if chunks finished out of order!
        // Or `zstd` compression is fast enough/channel constrained enough it didn't manifest?
        // To be safe, we must re-order compressed chunks before writing.

        writer_handle = Some(thread::spawn(move || -> std::io::Result<()> {
            // We need a buffering writer that re-orders via chunk_id.
            let mut buffer: HashMap<usize, CompressedChunk> = HashMap::new();
            let mut next_id = 0;

            // Wrap our generic writer in BufWriter
            let mut buf_writer = std::io::BufWriter::new(writer);

            // Since we are writing to a stream (potentially archive), we don't write "STZ1" magic bytes here.
            // The caller (ArchiveWriter) writes headers/magic.
            // BUT wait, `ChunkWriter::new` wrote "STZ1".
            // If we are compressing a single file in `main.rs`, we expect "STZ1".
            // If we are inside an archive, we don't want "STZ1" for every file?
            // Actually, `ArchiveReader` calls `decompress_stream`. `decompressor.rs` checks for "STZ1".
            // If we want `compress_file` to be universal, maybe it should NOT write magic?
            // Or we should write magic every time, and treat archive entries as mini-stz files.
            // My plan said: "5. Compressed Content (Traditional .stz blob)".
            // So YES, we write magic "STZ1".
            buf_writer.write_all(b"STZ1")?;

            for chunk in comp_chunk_rx {
                buffer.insert(chunk.chunk_id, chunk);

                while let Some(c) = buffer.remove(&next_id) {
                    // Write chunk c
                    // Borrowed logic from ChunkWriter::write_chunk
                    write_chunk_internal(&mut buf_writer, c)?;
                    next_id += 1;
                }
            }
            buf_writer.flush()?;
            Ok(())
        }));
    }

    // 5. Reader (Main Thread)
    // We need to read the input path.
    let input_file = File::open(input_path)?;
    let mut reader = BufReader::new(input_file);
    let mut line_buffer = Vec::new();
    let mut seq_counter = 0;

    while reader.read_until(b'\n', &mut line_buffer)? > 0 {
        let mut line_string = String::with_capacity(line_buffer.len());
        for chunk in line_buffer.utf8_chunks() {
            line_string.push_str(chunk.valid());
            if !chunk.invalid().is_empty() {
                for &b in chunk.invalid() {
                    use std::fmt::Write;
                    write!(line_string, "__BYTE_{:02X}__", b).unwrap();
                }
            }
        }

        let line_cow: std::borrow::Cow<'_, str> = std::borrow::Cow::Owned(line_string);

        let has_newline = line_cow.ends_with('\n');
        let trimmed = if has_newline {
            &line_cow[..line_cow.len() - 1]
        } else {
            &line_cow
        };

        job_tx
            .send((seq_counter, trimmed.to_string(), has_newline))
            .expect("Workers died");
        seq_counter += 1;

        line_buffer.clear();
    }
    drop(job_tx); // Close job_tx to signal parse workers to finish

    // Wait for all threads
    for h in worker_handles {
        h.join().unwrap();
    }
    let _ = sequencer_handle.join().unwrap();
    drop(comp_chunk_tx); // Close comp channel to signal writer
    if !debug_mode && !benchmark_mode {
        for h in compress_handles {
            h.join().unwrap();
        }
        if let Some(h) = writer_handle {
            h.join().unwrap()?;
        }
    }

    Ok(())
}

fn write_chunk_internal<W: Write>(writer: &mut W, chunk: CompressedChunk) -> std::io::Result<()> {
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

    // 6. Newlines
    writer.write_all(&u32::to_le_bytes(chunk.nl_blob.len() as u32))?;
    writer.write_all(&chunk.nl_blob)?;

    Ok(())
}
