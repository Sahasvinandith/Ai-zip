mod compressor;
mod decompressor;
mod models;
mod parser;

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::thread;

use compressor::LogCompressor;
use crossbeam_channel::{Receiver, Sender, bounded};
use decompressor::LogDecompressor;
use models::LogEntry;
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

    // Simple argument parsing for --threads
    let mut num_threads = 4; // Default
    if args.len() >= 6 && args[4] == "--threads" {
        if let Ok(n) = args[5].parse::<usize>() {
            num_threads = n;
        }
    }

    match mode.as_str() {
        "compress" => {
            println!(
                "Compressing {} -> {} using {} threads",
                input_path, output_path, num_threads
            );

            // 1. Setup Channels
            // Job: (Sequence ID, Log String)
            // Result: (Sequence ID, Option<LogEntry>)
            let (job_tx, job_rx): (Sender<(usize, String)>, Receiver<(usize, String)>) =
                bounded(1000);
            let (res_tx, res_rx): (
                Sender<(usize, Option<LogEntry>)>,
                Receiver<(usize, Option<LogEntry>)>,
            ) = bounded(1000);

            // 2. Spawn Workers
            let mut worker_handles = Vec::new();
            for _ in 0..num_threads {
                let job_rx_clone = job_rx.clone();
                let res_tx_clone = res_tx.clone();
                worker_handles.push(thread::spawn(move || {
                    for (seq, line) in job_rx_clone {
                        let parsed = parse_line(&line);
                        // Send result back
                        if res_tx_clone.send((seq, parsed)).is_err() {
                            break; // Main thread likely dropped receiver
                        }
                    }
                }));
            }
            // Drop original tx so receiver closes when all workers are done
            drop(res_tx);

            // 3. Spawn Sequencer (Writer)
            let output_path_clone = output_path.to_string();
            let writer_handle = thread::spawn(move || -> std::io::Result<usize> {
                let mut compressor = LogCompressor::new(&output_path_clone)?;
                let mut buffer: HashMap<usize, Option<LogEntry>> = HashMap::new();
                let mut next_expected_seq = 0;
                let mut count = 0;

                for (seq, entry) in res_rx {
                    buffer.insert(seq, entry);

                    // Drain buffer in order
                    while let Some(item) = buffer.remove(&next_expected_seq) {
                        if let Some(valid_entry) = item {
                            compressor.ingest(valid_entry)?;
                            count += 1;
                        }
                        next_expected_seq += 1;
                    }
                }
                compressor.finish()?;
                Ok(count)
            });

            // 4. Reader (Main Thread)
            let input_file = File::open(input_path)?;
            let mut reader = BufReader::new(input_file);
            let mut current_entry_lines = String::new();
            let mut line_buffer = String::new();
            let mut seq_counter = 0;

            while reader.read_line(&mut line_buffer)? > 0 {
                if is_log_start(&line_buffer) {
                    if !current_entry_lines.is_empty() {
                        // Push Job
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
                // seq_counter += 1; // Unnecessary for last element
            }

            // Close Job Queue
            drop(job_tx);

            // 5. Wait for Workers
            for h in worker_handles {
                h.join().expect("Worker panicked");
            }

            // 6. Wait for Writer
            let count = writer_handle.join().expect("Writer panicked")?;
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
