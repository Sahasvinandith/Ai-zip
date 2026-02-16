# SALC: Structure-Aware Log Compressor

![Rust](https://img.shields.io/badge/built_with-Rust-dca282.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)

**SALC** (Project `AI_zip`) is a high-performance log compression tool written in Rust. Unlike general-purpose compressors (like GZIP or ZSTD) that treat data as a raw stream of bytes, SALC understands the *structure* of log files. By separating static log templates from dynamic variables, it achieves superior compression ratios and enables advanced features like search-without-decompression.

## 🚀 Key Features

*   **Structure-Aware Compression:** Automatically detects repeated log patterns (templates) and stores them only once.
*   **Variable Separation:** Extracts dynamic values (timestamps, IDs, error codes) for specialized compression.
*   **High Performance:** Built with Rust for safety and speed, utilizing `zstd` for backend block compression.
*   **CLI Utility:** Simple command-line interface for easy integration into ETL pipelines.

## 🛠️ Installation

Ensure you have [Rust and Cargo](https://rustup.rs/) installed on your system.

```bash
# Clone the repository
git clone https://github.com/your-username/AI_zip.git
cd AI_zip

# Build the project in release mode for maximum performance
cargo build --release
```

The compiled binary will be available at `./target/release/AI_zip`.

## 📖 Usage

SALC operates in two primary modes: `compress` and `decompress`.

### Compressing Logs

To compress a raw log file into the optimized `.salc` format:

```bash
cargo run --release -- compress <input_log_file> <output_file.salc> [OPTIONS]
```

**Options:**
*   `--threads <N>`: specifies the number of parallel worker threads (default: 8).
*   `--debug`: Disables final ZSTD compression and binary packing. Useful for inspecting the internal templating results.
*   `--benchmark`: Disables all disk I/O and compression. Used specifically to measure the speed of the Drain3-based parsing and variable extraction pipeline.

**Example:**
```bash
cargo run --release -- compress system.log compressed.salc --threads 12
```

### Decompressing Logs

To restore a `.salc` file back to its original text format:

```bash
cargo run --release -- decompress <input_file.salc> <output_log_file>
```

**Example:**
```bash
cargo run --release -- decompress compressed.salc restored_system.log
```

## 🧠 How It Works

Standard compressors look for small repeated substrings (LZ77). SALC takes a semantic approach:

1.  **Parsing:** Reads the log line and identifies the static structure versus dynamic variables.
2.  **Tokenization:**
    *   *Template:* `INFO: User login failed for [USER_ID] at [IP]`
    *   *Variables:* `["User123", "192.168.1.5"]`
3.  **Storage:**
    *   Templates are stored in a global **Registry** (low entropy, high compression).
    *   Variables are stored in columnar arrays and compressed separately (often integers or timestamps).
4.  **Result:** Massive reduction in redundancy compared to compressing the full text string repeatedly.

## 🔮 Roadmap

*   [ ] **Delta Encoding:** optimizing timestamp storage by saving differences rather than full values.
*   [ ] **Integer Optimization:** Detecting and storing numeric variables in binary format instead of text strings.
*   [ ] **Search-Without-Decompression:** Ability to `grep` errors by scanning the lightweight ID column without inflating the entire file.

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
