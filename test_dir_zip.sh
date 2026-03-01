#!/bin/bash

INPUT_DIR="./Big_logs"
ARCHIVE_FILE="compressed_big_logs.zip"
OUTPUT_DIR="./decompressed_zip"

echo "Starting Directory Compression & Integrity Tests with ZIP..."
echo "=========================================================="

# 1. Clean up previous runs
rm -f "$ARCHIVE_FILE"
rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

echo ">> [Compressing Directory: $INPUT_DIR with zip]"
time zip -r -q "$ARCHIVE_FILE" "$INPUT_DIR"

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
echo ">> [Decompressing Archive: $ARCHIVE_FILE to $OUTPUT_DIR with unzip]"
time unzip -q "$ARCHIVE_FILE" -d "$OUTPUT_DIR"

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
    # Using relative INPUT_DIR string inside OUTPUT_DIR because zip preserves the path
    output_file="$OUTPUT_DIR/${INPUT_DIR#./}/$filename"
    
    if [ ! -f "$output_file" ]; then
        echo "!! [FAIL] Missing decompressed file: $filename"
        ALL_PASSED=false
        continue
    fi

    # cmp -s does a silent byte-by-byte comparison. 
    # Extremely memory efficient, won't load the whole file into RAM.
    if cmp -s "$input_file" "$output_file"; then
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
