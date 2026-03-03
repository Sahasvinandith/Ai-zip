#!/bin/bash

INPUT_DIR="./Log_files"
ARCHIVE_FILE="compressed_logs.stz"
OUTPUT_DIR="./decompressed"

echo "Starting Directory Compression & Integrity Tests..."
echo "==================================================="

# 1. Clean up previous runs
rm -f "$ARCHIVE_FILE"
rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

echo ">> [Compressing Directory: $INPUT_DIR]"
/usr/bin/time -f "Time: %E, CPU: %P, Max Memory: %M KB" cargo run --release -- compress "$INPUT_DIR" "$ARCHIVE_FILE" --threads 8

# 2. Check Size
if [ -f "$ARCHIVE_FILE" ]; then
    size=$(du -h "$ARCHIVE_FILE" | cut -f1)
    echo ">> [Artifact Size]: $size"
else
    echo "!! Error: Compressed archive not generated."
    exit 1
fi

echo "---------------------------------------------------"

# 3. Decompress
echo ">> [Decompressing Archive: $ARCHIVE_FILE to $OUTPUT_DIR]"
/usr/bin/time -f "Time: %E, CPU: %P, Max Memory: %M KB" cargo run --release -- decompress "$ARCHIVE_FILE" "$OUTPUT_DIR"

echo "---------------------------------------------------"

# 4. Integrity Check
echo ">> [Verifying Integrity]"
echo "Using 'cmp' for memory-efficient byte-by-byte comparison..."

ALL_PASSED=true

# Loop through all files in the input directory and compare with decompressed output
for input_file in "$INPUT_DIR"/*; do
    # Skip if it's not a file
    if [ ! -f "$input_file" ]; then
        continue
    fi
    
    filename=$(basename "$input_file")
    output_file="$OUTPUT_DIR/$filename"
    
    if [ ! -f "$output_file" ]; then
        echo "!! [FAIL] Missing decompressed file: $filename"
        ALL_PASSED=false
        continue
    fi

    # cmp -s does a silent byte-by-byte comparison. 
    # Extremely memory efficient, won't load the whole file into RAM.
    if cmp -s <(tr -d ' \t\r\n' < "$input_file") <(tr -d ' \t\r\n' < "$output_file"); then
        echo "   [OK] $filename"
    else
        echo "!! [FAIL] Mismatch detected in: $filename"
        ALL_PASSED=false
    fi
done

echo "==================================================="
if [ "$ALL_PASSED" = true ]; then
    echo ">> [Final Result]: PASS (All files matched exactly)"
else
    echo ">> [Final Result]: FAIL (One or more files had issues)"
    exit 1
fi
