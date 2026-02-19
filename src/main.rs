mod archive;
mod compressor;
mod decompressor;
mod drain_registry;
mod models;
mod parser;

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::sync::Arc;
use std::thread;

use compressor::{ChunkWriter, LogAccumulator, SharedRegistry, compress_chunk};
use crossbeam_channel::{Receiver, Sender, bounded};
use decompressor::LogDecompressor;
use models::{CompressedChunk, PreDigestedEntry, RawChunk};
use parser::parse_line;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!(
            "Usage: {} <mode> <input_file> <output_file> [--threads N]",
            args[0]
        );
        eprintln!(
            "Modes: \n  compress   - Compress log file to .stz\n  decompress - Decompress .stz file to text"
        );
        std::process::exit(1);
    }

    let mode = &args[1];
    let input_path = &args[2];
    let output_path = &args[3];

    match mode.as_str() {
        "compress" => {
            // Simple argument parsing
            let mut num_threads = 8;
            let mut debug_mode = false;
            let mut benchmark_mode = false;

            let mut i = 4;
            while i < args.len() {
                if args[i] == "--threads" && i + 1 < args.len() {
                    if let Ok(n) = args[i + 1].parse::<usize>() {
                        num_threads = n;
                    }
                    i += 2;
                } else if args[i] == "--debug" {
                    debug_mode = true;
                    i += 1;
                } else if args[i] == "--benchmark" {
                    benchmark_mode = true;
                    i += 1;
                } else {
                    i += 1;
                }
            }

            let path = std::path::Path::new(input_path);
            if path.is_dir() {
                if debug_mode || benchmark_mode {
                    eprintln!(
                        "Error: Debug/Benchmark modes not supported for directory archives yet."
                    );
                    std::process::exit(1);
                }
                println!(
                    "Compressing directory: {} -> {} using {} threads",
                    input_path, output_path, num_threads
                );
                archive::create_archive(input_path, output_path, num_threads)?;
            } else {
                println!(
                    "Compressing file: {} -> {} using {} threads (Debug: {}, Benchmark: {})",
                    input_path, output_path, num_threads, debug_mode, benchmark_mode
                );
                // Standard file compression
                let output_file = File::create(output_path)?;
                let writer = std::io::BufWriter::new(output_file);
                compressor::compress_file(path, writer, num_threads, debug_mode, benchmark_mode)?;
            }
        }
        "decompress" => {
            // Check magic bytes to distinguish archive vs single file
            let mut file = std::fs::File::open(input_path)?;
            let mut magic = [0u8; 4];
            let is_archive = if let Ok(_) = std::io::Read::read_exact(&mut file, &mut magic) {
                &magic == b"STZ\x02"
            } else {
                false
            };
            drop(file); // Close to reopen in extract_archive

            if is_archive {
                println!(
                    "Detected Directory Archive (STZ2). Extracting to {}",
                    output_path
                );
                archive::extract_archive(input_path, output_path)?;
            } else {
                println!(
                    "Detected Single File (STZ1 or unknown). Decompressing to {}",
                    output_path
                );
                LogDecompressor::decompress(input_path, output_path)?;
            }
            println!("Done.");
        }
        _ => {
            eprintln!("Invalid mode. Use 'compress' or 'decompress'.");
            std::process::exit(1);
        }
    }

    Ok(())
}
