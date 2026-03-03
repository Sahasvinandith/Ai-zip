#!/bin/bash

if [ -z "$1" ]; then
    echo "Usage: $0 <parent_directory>"
    echo "Example: $0 ./Big_logs"
    exit 1
fi

PARENT_DIR="$1"

if [ ! -d "$PARENT_DIR" ]; then
    echo "Error: Directory '$PARENT_DIR' does not exist."
    exit 1
fi

# Directory to store compressed files and decompression outputs
OUT_DIR="./tar_xz_checks"
mkdir -p "$OUT_DIR"

echo "Starting tar.xz Compression & Decompression Benchmarks..."
echo "Folder: $PARENT_DIR"
echo "========================================="

# Clean up trailing slash to correctly get the basename
PARENT_DIR="${PARENT_DIR%/}"
DIR_NAME=$(basename "$PARENT_DIR")

compressed_file="$OUT_DIR/${DIR_NAME}.tar.xz"
decompressed_dir="$OUT_DIR/${DIR_NAME}_extracted"

echo "Processing Directory: $PARENT_DIR"

# 1. Compress
echo ">> [Compressing with tar_xz]"
/usr/bin/time -f "Time: %E, CPU: %P, Max Memory: %M KB" tar -cJf "$compressed_file" "$PARENT_DIR"

# 2. Check Size
if [ -f "$compressed_file" ]; then
    # Use du -h (--si) to get human-readable size
    size=$(du -h --si "$compressed_file" | cut -f1)
    echo ">> [Compressed Size]: $size"
else
    echo "!! Error: Compressed file not generated."
    exit 1
fi

# 3. Decompress
echo ">> [Decompressing tar_xz]"
mkdir -p "$decompressed_dir"
# Extract into the dedicated decompression directory to avoid overwriting originals
/usr/bin/time -f "Time: %E, CPU: %P, Max Memory: %M KB" tar -xf "$compressed_file" -C "$decompressed_dir"

echo "-----------------------------------------"
echo "Benchmark complete! Artifacts are stored in $OUT_DIR/"
