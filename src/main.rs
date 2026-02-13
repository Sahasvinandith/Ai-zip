mod compressor;
mod decompressor;
mod models;
mod parser;

use std::env;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;

use compressor::LogCompressor;
use decompressor::LogDecompressor;
use parser::{is_log_start, parse_line};

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
