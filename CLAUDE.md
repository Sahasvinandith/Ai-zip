# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**STZ** (AI_zip) is a semantic log compression tool written in Rust. It uses [Drain3](https://github.com/benwtrent/drain-rs) to mine log templates at runtime, separating static template structure from dynamic variables and compressing them as independent columnar streams. The output format is `.stz`.

## Build & Run Commands

```bash
# Debug build
cargo build

# Release build (always use for performance testing)
cargo build --release

# Compress a single log file
cargo run --release -- compress <input.log> <output.stz> [--threads N]

# Compress with debug mode (skips ZSTD + binary packing, for inspecting templates)
cargo run --release -- compress <input.log> <output.stz> --debug

# Compress with benchmark mode (no disk I/O, measures Drain parsing speed only)
cargo run --release -- compress <input.log> <output.stz> --benchmark

# Compress a directory into an archive
cargo run --release -- compress <input_dir/> <output.stz>

# Decompress a single file or archive
cargo run --release -- decompress <input.stz> <output.log>

# Run tests
cargo test

# Run a single test
cargo test <test_name>

# Check without building
cargo check
```

## Architecture

### Compression Pipeline (multi-threaded)

`compress_file()` in `compressor.rs` orchestrates a parallel pipeline:

1. **Reader** (main thread): reads input line-by-line, sends `(seq, line, has_newline)` tuples to `job_tx`
2. **Parse Workers** (`--threads N`): each worker calls `parse_line()` → `registry.get_or_learn()`, returns `(seq, PreDigestedEntry)` via `res_tx`
3. **Sequencer** (single thread): reorders out-of-sequence parse results using a `HashMap` buffer, feeds them into `LogAccumulator`, emits `RawChunk` every 200,000 lines
4. **Compress Workers** (`N/2` threads): takes `RawChunk`, applies per-column ZSTD compression, emits `CompressedChunk`
5. **Writer** (single thread): reorders `CompressedChunk`s by `chunk_id` and writes to output — **chunk order is critical** because each chunk only contains a `registry_delta` (new templates since the last chunk)

### Key Data Structures (`models.rs`)

- `LogEntry`: parsed log line (timestamp, level, body)
- `PreDigestedEntry`: after Drain lookup; contains `template_id` (u32) + extracted `variables`
- `RawChunk`: columnar arrays for 200k lines — `ts_col`, `lvl_col`, `id_col`, `var_col`, `nl_col`, plus `registry_delta`
- `CompressedChunk`: ZSTD-compressed blobs of each column

### Encoding Details (`compressor.rs`)

| Column | Encoding |
|--------|----------|
| Timestamps | Delta-encoded (first absolute u64, rest i64 deltas), then ZSTD |
| Log levels | Raw `u8` bytes, ZSTD |
| Template IDs | Raw `u32` LE bytes, ZSTD |
| Variables | Length-prefixed binary (`u16 len` + bytes), ZSTD |
| Newlines | Bit-packed, ZSTD |
| Registry delta | JSON array of template strings, ZSTD |

### Template Registry (`drain_registry.rs`, `compressor.rs`)

- `DrainRegistry` wraps `drain-rs`'s `DrainTree` (configured: `max_depth=2`, `max_children=100`, `min_similarity=0.4`)
- Always uses the write path (`add_log_line`) — never the read-only path — to ensure templates are properly generalized before variable extraction
- Drain's `<*>` placeholders are converted to `<VAR>` for decompressor compatibility
- `SharedRegistry` maps Drain's u64 hash → sequential u32 local ID using `RwLock<HashMap>` with optimistic read / pessimistic write locking

### Parser (`parser.rs`)

Supports three log formats via regex, with RAW fallback:
- **HADOOP**: `YYYY-MM-DD HH:MM:SS,mmm LEVEL ...`
- **NOVA**: `YYYY-MM-DD HH:MM:SS.mmm PID LEVEL ...`
- **RAW**: anything else (stack traces, continuation lines)

SYSLOG format is intentionally disabled (causes fidelity loss).

Multi-line log entries: only the first line goes through Drain; continuation lines are stored as a raw variable appended to the variable list.

### File Format

- **Single file (STZ1)**: magic `STZ\x01` + sequential compressed chunks
- **Directory archive (STZ2)**: magic `STZ\x02` + entries of `[u32 path_len][path bytes][u64 content_len][STZ1 blob]`

Magic bytes are checked at runtime in `main.rs` to dispatch to single-file or archive decompression.

### Archive (`archive.rs`)

- `create_archive`: iterates directory with `walkdir`, writes STZ2 header, calls `compress_file()` for each file with a shared `SharedRegistry` (templates shared across all files)
- `extract_archive`: reads STZ2 format, creates output dirs, calls `LogDecompressor::decompress_to_writer()` with a shared `global_template_store`

## Known Limitations / Gotchas

- `--debug` and `--benchmark` flags only work for single file compression, not directory archives
- Chunk order in the output file is critical — the decompressor reconstructs the template store incrementally using `registry_delta` from each chunk in order
- Non-UTF8 bytes in log files are replaced with `__BYTE_XX__` placeholders during compression
- Tabs in log content are escaped to `__TAB__` before sending to Drain (Drain strips tabs as whitespace)
