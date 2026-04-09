# STZ: Semantic Template Zip

![Rust](https://img.shields.io/badge/built_with-Rust-dca282.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)

**STZ** (Project `AI_zip`) is a high-performance log compression tool written in Rust. Unlike general-purpose compressors (like GZIP or ZSTD) that treat data as a raw stream of bytes, STZ understands the *semantics* of log files. Using Drain3-based template mining, it separates static log templates from dynamic variables and compresses them independently, achieving superior compression ratios.

## Key Features

- **Structure-Aware Compression:** Automatically detects repeated log patterns (templates) and stores them only once.
- **Variable Separation:** Extracts dynamic values (timestamps, IDs, error codes) for specialized compression.
- **High Performance:** Built with Rust for safety and speed, utilizing `zstd` for backend block compression.
- **CLI Utility:** Simple command-line interface for easy integration into ETL pipelines.
- **Delta-Encoded Timestamps:** Adjacent timestamps are stored as compact deltas, further reducing file size.
- **Binary Variable Encoding:** Variables use a compact length-prefixed binary format instead of JSON.
- **Directory Archive Support:** Compress entire log directories into a single `.stz` archive with shared template registries.

## Installation

Ensure you have [Rust and Cargo](https://rustup.rs/) installed on your system.

```bash
# Clone the repository
git clone https://github.com/your-username/AI_zip.git
cd AI_zip

# Build the project in release mode for maximum performance
cargo build --release
```

The compiled binary will be available at `./target/release/AI_zip`.

## Usage

STZ operates in two primary modes: `compress` and `decompress`.

### Compressing Logs

To compress a raw log file or directory into the optimized `.stz` format:

```bash
cargo run --release -- compress <input_log_file> <output_file.stz> [OPTIONS]
cargo run --release -- compress <input_dir/> <output_file.stz>
```

**Options:**
- `--threads <N>`: Number of parallel worker threads (default: 8).
- `--debug`: Disables final ZSTD compression and binary packing. Useful for inspecting internal templating results.
- `--benchmark`: Disables all disk I/O and compression. Measures only the Drain3-based parsing and variable extraction pipeline speed.

**Example:**
```bash
cargo run --release -- compress system.log compressed.stz --threads 12
```

### Decompressing Logs

To restore a `.stz` file back to its original text format:

```bash
cargo run --release -- decompress <input_file.stz> <output_log_file>
```

**Example:**
```bash
cargo run --release -- decompress compressed.stz restored_system.log
```

## How It Works

Standard compressors look for small repeated substrings (LZ77). STZ takes a semantic approach:

1. **Parsing:** Reads each log line and identifies static structure versus dynamic variables.
2. **Tokenization:**
   - *Template:* `INFO: User login failed for <VAR> at <VAR>`
   - *Variables:* `["User123", "192.168.1.5"]`
3. **Columnar Storage:**
   - Templates are stored in a global **Registry** (low entropy, compresses extremely well).
   - Variables, timestamps, and log levels are stored in separate columnar arrays and compressed independently.
4. **Result:** Massive reduction in redundancy compared to compressing full text repeatedly.

## Evaluation

Compression performance on diverse real-world log datasets, compared against standard ZIP and TAR.XZ baselines. All STZ runs used the default thread count.

| Dataset | Original Size | Utility | Compressed Size | Encoding Time | Decoding Time |
|---------|--------------|---------|----------------|---------------|---------------|
| HDFS datanode-01 | 698.0 MiB | STZ2 | 24.0 MiB | 11.78 s | 5.21 s |
| | | ZIP | 37.8 MiB | 21.53 s | 3.81 s |
| | | TAR.XZ | 26.4 MiB | N/A | N/A |
| Windows_1 | 462.29 MiB | STZ2 | 3.7 MiB | 13.99 s | 5.75 s |
| | | ZIP | 28.0 MiB | 3.99 s | 2.09 s |
| | | TAR.XZ | 2.4 MiB | 7.11 s | 0.63 s |
| Windows_3 | 9.11 GiB | STZ2 | 76.0 MiB | 5m 59.1 s | 2m 07.0 s |
| | | ZIP | 576.0 MiB | 1m 13.5 s | 39.15 s |
| | | TAR.XZ | 48.0 MiB | 1m 46.0 s | 14.07 s |
| Windows | 26.09 GiB | STZ2 | 191.46 MiB | 19m 04s | 2m 07s |
| | | ZIP | 1.7 GiB | 3m 48s | 1m 37s |
| | | TAR.XZ | 129 MiB | 5m 50s | 41.67 s |
| Thunderbird_1 | 245.63 MiB | STZ2 | 17.0 MiB | 1m 14.3 s | 4.51 s |
| | | ZIP | 21.0 MiB | 2.67 s | 1.01 s |
| | | TAR.XZ | 11.0 MiB | 8.00 s | 0.27 s |
| Thunderbird_2 | 3.03 GiB | STZ2 | 160 MiB | 68m 18.45 s | 47.40 s |
| | | ZIP | 195 MiB | 35.44 s | 14.90 s |
| | | TAR.XZ | 111 MiB | 1m 21.32 s | 4.89 s |

STZ achieves competitive compression ratios against general-purpose tools while preserving full semantic fidelity — the decompressed output is byte-for-byte identical to the original log file.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
