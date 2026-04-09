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

        let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let pb = indicatif::ProgressBar::new(file_size);
        pb.set_style(indicatif::ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"));
        pb.inc(4);

        if &magic != b"STZ1" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid file format",
            ));
        }

        let mut template_store: Vec<String> = Vec::new();
        let res =
            Self::decompress_to_writer(&mut file, &mut writer, &mut template_store, Some(&pb));
        pb.finish_with_message("Decompression complete");
        res
    }

    pub fn decompress_to_writer<R: Read, W: Write>(
        reader: &mut R,
        writer: &mut W,
        template_store: &mut Vec<String>,
        pb: Option<&indicatif::ProgressBar>,
    ) -> std::io::Result<()> {
        // Loop until EOF
        loop {
            // Helper to read compressed block
            // Returns Ok(Some(data)) if successful, Ok(None) if EOF on size read
            // We need to define this closure inside, but it captures nothing from env except reader.

            // Refactored read_block to be inline or helper function that takes reader?
            // Closure is fine.

            let mut size_buf = [0u8; 4];
            let size_read = reader.read(&mut size_buf)?;
            if size_read == 0 {
                break; // EOF
            }
            if let Some(p) = pb {
                p.inc(size_read as u64);
            }
            if size_read < 4 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Incomplete block size",
                ));
            }

            let size = u32::from_le_bytes(size_buf) as usize;
            let mut compressed_data = vec![0u8; size];
            reader.read_exact(&mut compressed_data)?;
            if let Some(p) = pb {
                p.inc(size as u64);
            }
            let decoded = zstd::decode_all(&compressed_data[..])?;

            // Block 1: Registry Delta (Decoded above)
            let registry_bytes = decoded;

            let new_templates: Vec<String> = serde_json::from_slice(&registry_bytes)?;
            template_store.extend(new_templates);

            // Helper for remaining blocks
            let read_next_block = |r: &mut R| -> std::io::Result<Vec<u8>> {
                let mut sz_buf = [0u8; 4];
                r.read_exact(&mut sz_buf)?;
                if let Some(p) = pb {
                    p.inc(4);
                }
                let sz = u32::from_le_bytes(sz_buf) as usize;
                let mut data = vec![0u8; sz];
                r.read_exact(&mut data)?;
                if let Some(p) = pb {
                    p.inc(sz as u64);
                }
                let dec = zstd::decode_all(&data[..])?;
                Ok(dec)
            };

            // Block 2: Timestamps
            let ts_bytes = read_next_block(reader)?;
            // Delta-decode timestamps: first is absolute u64, rest are i64 deltas
            let mut ts_col = Vec::new();
            if ts_bytes.len() >= 8 {
                let first = u64::from_le_bytes(ts_bytes[..8].try_into().unwrap());
                ts_col.push(first);
                let mut prev = first;
                for chunk in ts_bytes[8..].chunks_exact(8) {
                    let delta = i64::from_le_bytes(chunk.try_into().unwrap());
                    let ts = (prev as i64 + delta) as u64;
                    ts_col.push(ts);
                    prev = ts;
                }
            }

            // Block 3: Levels
            let lvl_bytes = read_next_block(reader)?;
            let lvl_col: Vec<u8> = lvl_bytes.clone();

            // Block 4: IDs
            let id_bytes = read_next_block(reader)?;
            let mut id_col = Vec::new();
            for chunk in id_bytes.chunks_exact(4) {
                let id = u32::from_le_bytes(chunk.try_into().unwrap());
                id_col.push(id);
            }

            // Block 5: Variables
            let var_bytes = read_next_block(reader)?;
            // Decode length-prefixed binary variables
            let mut var_col: Vec<String> = Vec::new();
            let mut pos = 0;
            while pos + 2 <= var_bytes.len() {
                let len = u16::from_le_bytes([var_bytes[pos], var_bytes[pos + 1]]) as usize;
                pos += 2;
                if pos + len > var_bytes.len() {
                    break;
                }
                let s = String::from_utf8_lossy(&var_bytes[pos..pos + len]).to_string();
                pos += len;
                var_col.push(s);
            }

            // Block 6: Newlines
            let nl_bytes = read_next_block(reader)?;

            let mut nl_col = Vec::with_capacity(id_col.len());
            for byte in &nl_bytes {
                for bit in 0..8 {
                    nl_col.push((byte >> bit) & 1 == 1);
                }
            }

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

                // Extract variables for <VAR> placeholders
                let mut current_vars = Vec::new();
                for _ in 0..var_count {
                    if var_idx < var_col.len() {
                        current_vars.push(&var_col[var_idx]);
                        var_idx += 1;
                    }
                }

                // Check for continuation line variable (extra var beyond <VAR> count)
                // This is stored when a log entry spans multiple lines or has trailing \r
                let continuation = if var_idx < var_col.len() {
                    let next_var = &var_col[var_idx];
                    if next_var.starts_with('\r') || next_var.starts_with('\n') {
                        var_idx += 1;
                        Some(next_var.as_str())
                    } else {
                        None
                    }
                } else {
                    None
                };

                let mut reconstructed = String::new();
                let parts: Vec<&str> = template_str.split("<VAR>").collect();
                // println!("Decompressing ID: {}, template: {}, vars: {:?}", id, template_str, current_vars);

                for (j, part) in parts.iter().enumerate() {
                    reconstructed.push_str(part);
                    if j < current_vars.len() {
                        reconstructed.push_str(current_vars[j]);
                    }
                }

                // Append continuation lines if present
                if let Some(cont) = continuation {
                    reconstructed.push_str(cont);
                }

                let reconstructed = reconstructed.replace("__TAB__", "\t");

                // Format Timestamp
                let secs = (ts_col[i] / 1000) as i64;
                let nsecs = ((ts_col[i] % 1000) * 1_000_000) as u32;
                let dt = DateTime::from_timestamp(secs, nsecs).unwrap_or_default();
                // Restore timestamp format (using comma for millis as seen in source)
                let ts_str = dt.format("%Y-%m-%d %H:%M:%S,%3f").to_string();

                // Restore Level
                let lvl = LogLevel::from_u8(if i < lvl_col.len() { lvl_col[i] } else { 0 });

                let final_str = if lvl == LogLevel::RAW {
                    let msg = reconstructed.replace("__TAB__", "\t");
                    msg
                } else {
                    let lvl_str = if lvl == LogLevel::UNKNOWN {
                        String::new()
                    } else {
                        lvl.to_string()
                    };

                    if lvl_str.is_empty() {
                        format!("{} {}", ts_str, reconstructed)
                    } else {
                        format!("{} {} {}", ts_str, lvl_str, reconstructed)
                    }
                };

                // Write with decoding
                Self::write_decoded(writer, &final_str)?;

                // Handle newline
                if i < nl_col.len() && !nl_col[i] {
                    // No newline
                } else {
                    writer.write_all(b"\n")?;
                }
            }
        }

        writer.flush()?;
        Ok(())
    }

    fn write_decoded<W: Write>(writer: &mut W, s: &str) -> std::io::Result<()> {
        let mut last_pos = 0;
        // Find potential escape sequences "__BYTE_XX__"
        // Length of sequence is 11 chars.
        // We can scan manually.

        // Simple scan loop
        let mut chars = s.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '_' && s[i..].starts_with("__BYTE_") {
                // Check if we have enough chars left
                if i + 11 <= s.len() && &s[i + 9..i + 11] == "__" {
                    // Try parse hex
                    if let Ok(byte) = u8::from_str_radix(&s[i + 7..i + 9], 16) {
                        // Flush previous
                        writer.write_all(s[last_pos..i].as_bytes())?;
                        // Write byte
                        writer.write_all(&[byte])?;

                        // Skip
                        for _ in 0..10 {
                            chars.next();
                        } // skip _BYTE_XX__ (10 chars after first _)
                        last_pos = i + 11;
                    }
                }
            }
        }

        // Write remainder
        if last_pos < s.len() {
            writer.write_all(s[last_pos..].as_bytes())?;
        }
        Ok(())
    }
}
