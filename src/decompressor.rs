use chrono::DateTime;
use std::fs::File;
use std::io::{Read, Write};

use crate::models::LogLevel;

pub struct LogDecompressor;

impl LogDecompressor {
    pub fn decompress(input_path: &str, output_path: &str) -> std::io::Result<()> {
        let mut file = File::open(input_path)?;
        let output_file = File::create(output_path)?;
        let mut writer = std::io::BufWriter::new(output_file);

        // 1. Magic Bytes check
        let mut magic = [0u8; 4];
        file.read_exact(&mut magic)?;
        if &magic != b"SALC" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid file format",
            ));
        }

        let mut template_store: Vec<String> = Vec::new();

        // Loop until EOF
        loop {
            // Helper to read compressed block
            // Returns Ok(Some(data)) if successful, Ok(None) if EOF on size read
            let read_block = |f: &mut File| -> std::io::Result<Option<Vec<u8>>> {
                let mut size_buf = [0u8; 4];
                match f.read_exact(&mut size_buf) {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                    Err(e) => return Err(e),
                }

                let size = u32::from_le_bytes(size_buf) as usize;
                let mut compressed_data = vec![0u8; size];
                f.read_exact(&mut compressed_data)?;
                let decoded = zstd::decode_all(&compressed_data[..])?;
                Ok(Some(decoded))
            };

            // 2. Deserialize Blocks
            // Block 1: Registry Delta
            let registry_bytes = match read_block(&mut file)? {
                Some(b) => b,
                None => break, // EOF reached cleanly between blocks
            };
            let new_templates: Vec<String> = serde_json::from_slice(&registry_bytes)?;
            template_store.extend(new_templates);

            // Block 2: Timestamps
            let ts_bytes = read_block(&mut file)?.ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Unexpected EOF in block")
            })?;
            let mut ts_col = Vec::new();
            for chunk in ts_bytes.chunks_exact(8) {
                let ts = u64::from_le_bytes(chunk.try_into().unwrap());
                ts_col.push(ts);
            }

            // Block 3: Levels
            let lvl_bytes = read_block(&mut file)?.ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Unexpected EOF in block")
            })?;
            let lvl_col: Vec<u8> = lvl_bytes;

            // Block 4: IDs
            let id_bytes = read_block(&mut file)?.ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Unexpected EOF in block")
            })?;
            let mut id_col = Vec::new();
            for chunk in id_bytes.chunks_exact(4) {
                let id = u32::from_le_bytes(chunk.try_into().unwrap());
                id_col.push(id);
            }

            // Block 5: Variables
            let var_bytes = read_block(&mut file)?.ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Unexpected EOF in block")
            })?;
            let var_col: Vec<String> = serde_json::from_slice(&var_bytes)?;

            // 3. Reconstruction Loop for this Block
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
        }

        writer.flush()?;
        Ok(())
    }
}
