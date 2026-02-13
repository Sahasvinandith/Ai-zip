use chrono::NaiveDateTime;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

use crate::models::LogEntry;

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
