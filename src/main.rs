mod compressor;
mod decompressor;
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
use parser::{is_log_start, parse_line};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!(
            "Usage: {} <mode> <input_file> <output_file> [--threads N]",
            args[0]
        );
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
            // Simple argument parsing for --threads, --debug, --benchmark
            let mut num_threads = 8; // Default
            let mut debug_mode = false;
            let mut benchmark_mode = false;

            for arg in &args {
                if arg == "--debug" {
                    debug_mode = true;
                }
                if arg.starts_with("--threads=") {
                    // Handle --threads=N (not supported in previous loop, simplified here)
                }
            }
            // Better CLI parsing
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

            println!(
                "Compressing {} -> {} using {} threads (Debug: {}, Benchmark: {})",
                input_path, output_path, num_threads, debug_mode, benchmark_mode
            );

            // 1. Setup Channels
            // Parse Pipeline: Job -> Result (PreDigestedEntry)
            let (job_tx, job_rx): (Sender<(usize, String)>, Receiver<(usize, String)>) =
                bounded(100000);
            let (res_tx, res_rx): (
                Sender<(usize, Option<PreDigestedEntry>)>,
                Receiver<(usize, Option<PreDigestedEntry>)>,
            ) = bounded(100000);

            // Compress Pipeline: RawChunk -> CompressedChunk
            let (raw_chunk_tx, raw_chunk_rx): (Sender<RawChunk>, Receiver<RawChunk>) = bounded(50);

            // Only need comp_chunk channels if NOT debug/benchmark mode
            let (comp_chunk_tx, comp_chunk_rx): (
                Sender<CompressedChunk>,
                Receiver<CompressedChunk>,
            ) = bounded(50);

            // Universal Shared Registry
            let registry = Arc::new(SharedRegistry::new());

            // 2. Spawn Parse Workers
            let parse_threads = num_threads;
            // In DEBUG mode, we might want minimal compression threads or none if we skip it
            // Implementation: Debug mode skips Zstd compression entirely.
            // So we don't spawn compress workers or Writer for CompressedChunks.
            // Instead we spawn a Debug Writer that takes RawChunks directly.

            let compress_threads = if debug_mode || benchmark_mode {
                0
            } else {
                if num_threads > 2 { num_threads / 2 } else { 1 }
            };

            println!("Spawning {} Parse Workers", parse_threads);

            let mut worker_handles = Vec::new(); // sotres handles of parsing threads
            for _ in 0..parse_threads {
                let job_rx_clone = job_rx.clone();
                let res_tx_clone = res_tx.clone();
                let registry_clone = registry.clone(); // Shared Registry

                worker_handles.push(thread::spawn(move || {
                    for (seq, line) in job_rx_clone {
                        if let Some(entry) = parse_line(&line) {
                            // Parallel Dictionary Lookup!
                            let template_id = registry_clone
                                .get_or_register(entry.template_str, entry.template_hash);

                            let pre_digested = PreDigestedEntry {
                                timestamp: entry.timestamp,
                                verbosity_level: entry.verbosity_level,
                                template_id,
                                variables: entry.variables,
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
            // Re-orders logs and fills RawChunks
            let registry_clone2 = registry.clone(); // Sequencer needs registry to get Delta
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

            // 4. BRANCH: Debug vs Benchmark vs Standard
            let writer_handle;
            let mut compress_handles = Vec::new();

            if debug_mode {
                // DEBUG PIPELINE: Sequencer -> DebugWriter (Consumer)
                // No intermediate compression threads.
                // We reuse the main thread or spawn a thread for writing to keep main for reading.

                let output_path_clone = output_path.to_string();
                let registry_clone3 = registry.clone();

                writer_handle = thread::spawn(move || -> std::io::Result<()> {
                    // Be careful: raw_chunk_rx needs to be consumed.
                    let mut debug_writer =
                        compressor::DebugChunkWriter::new(&output_path_clone, registry_clone3)?;

                    // We need to re-order RawChunks?
                    // Actually Sequencer produces RawChunks in order (chunk_counter increment).
                    // So we can just consume them directly from channel.

                    for raw_chunk in raw_chunk_rx {
                        debug_writer.write_chunk(raw_chunk)?;
                    }
                    debug_writer.finish()?;
                    Ok(())
                });

                // We don't use comp_chunk_tx
                drop(comp_chunk_tx);
            } else if benchmark_mode {
                let output_path_clone = output_path.to_string();
                let registry_clone3 = registry.clone();

                writer_handle = thread::spawn(move || -> std::io::Result<()> {
                    let mut bench_writer =
                        compressor::BenchmarkWriter::new(&output_path_clone, registry_clone3)?;
                    for raw_chunk in raw_chunk_rx {
                        bench_writer.write_chunk(raw_chunk)?;
                    }
                    bench_writer.finish()?;
                    Ok(())
                });
                drop(comp_chunk_tx);
            } else {
                // STANDARD PIPELINE: Sequencer -> Compressors -> Writer

                println!("Spawning {} Compress Workers", compress_threads);

                // 4. Spawn Compress Workers
                for _ in 0..compress_threads {
                    let raw_rx = raw_chunk_rx.clone();
                    let comp_tx = comp_chunk_tx.clone();
                    compress_handles.push(thread::spawn(move || {
                        for raw_chunk in raw_rx {
                            match compress_chunk(raw_chunk) {
                                Ok(comp_chunk) => {
                                    if comp_tx.send(comp_chunk).is_err() {
                                        break;
                                    }
                                }
                                Err(e) => eprintln!("Compression error: {}", e),
                            }
                        }
                    }));
                }
                drop(comp_chunk_tx); // Close unused sender so writer can finish

                // 5. Spawn Writer (Filesystem)
                // Re-orders chunks and writes to file
                let output_path_clone = output_path.to_string();
                writer_handle = thread::spawn(move || -> std::io::Result<()> {
                    let mut chunk_writer = ChunkWriter::new(&output_path_clone)?;
                    let mut chunk_buffer: HashMap<usize, CompressedChunk> = HashMap::new();
                    let mut next_chunk_id = 0;

                    for comp_chunk in comp_chunk_rx {
                        chunk_buffer.insert(comp_chunk.chunk_id, comp_chunk);

                        while let Some(chunk) = chunk_buffer.remove(&next_chunk_id) {
                            chunk_writer.write_chunk(chunk)?;
                            next_chunk_id += 1;
                        }
                    }
                    chunk_writer.finish()?;

                    Ok(())
                });
            }

            // 6. Reader (Main Thread)
            let input_file = File::open(input_path)?;
            let mut reader = BufReader::new(input_file);
            let mut current_entry_lines = String::new();
            let mut line_buffer = String::new();
            let mut seq_counter = 0;

            while reader.read_line(&mut line_buffer)? > 0 {
                if is_log_start(&line_buffer) {
                    if !current_entry_lines.is_empty() {
                        job_tx
                            .send((seq_counter, current_entry_lines.clone()))
                            .expect("Workers died");
                        seq_counter += 1;
                    }
                    current_entry_lines = line_buffer.clone();
                } else {
                    current_entry_lines.push_str(&line_buffer);
                }
                line_buffer.clear();
            }

            // Flush final entry
            if !current_entry_lines.is_empty() {
                job_tx
                    .send((seq_counter, current_entry_lines))
                    .expect("Workers died");
            }
            drop(job_tx);

            // 7. Clean up
            for h in worker_handles {
                h.join().expect("Parse Worker panicked");
            }

            let total_entries = sequencer_handle.join().expect("Sequencer panicked")?;
            // Sequencer finishing closes raw_chunk_tx, which stops Compressors

            for h in compress_handles {
                h.join().expect("Compress Worker panicked");
            }
            // Compressors finishing closes comp_chunk_tx, which stops Writer

            writer_handle.join().expect("Writer panicked")?;

            println!("Done. Processed {} entries.", total_entries);
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
