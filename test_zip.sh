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
OUT_DIR="./zip_checks"
mkdir -p "$OUT_DIR"

echo "Starting ZIP Compression & Decompression Benchmarks..."
echo "Folder: $PARENT_DIR"
echo "========================================="

# Find all log files in the specified directory
# (Customize the -name "*.log" filter if your files have a different extension)
for input_file in "$PARENT_DIR"/*.log; do
    # Skip if no .log files were found (glob didn't expand)
    [ -e "$input_file" ] || continue

    filename=$(basename "$input_file")
    compressed_file="$OUT_DIR/${filename}.zip"
    decompressed_file="$OUT_DIR/${filename}_extracted.log"

    echo "Processing: $filename"

    # 1. Compress
    echo ">> [Compressing with zip]"
    # Using the exact format requested, but placing the output in OUT_DIR
    time zip -q -j "$compressed_file" "$input_file"
    
    # 2. Check Size
    if [ -f "$compressed_file" ]; then
        # Use du -h (--si) to get human-readable size
        size=$(du -h --si "$compressed_file" | cut -f1)
        echo ">> [Compressed Size]: $size"
    else
        echo "!! Error: Compressed file not generated."
        continue
    fi

    # 3. Decompress
    echo ">> [Decompressing with unzip]"
    # Extract file to stdout and redirect to a decompressed file
    time unzip -p "$compressed_file" > "$decompressed_file"

    echo "-----------------------------------------"
done

echo "Benchmark complete! Artifacts are stored in $OUT_DIR/"
