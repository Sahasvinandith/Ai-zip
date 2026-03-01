#!/bin/bash

# Create the output directory if it doesn't exist
mkdir -p ./checks_zip

# List of files to process
files=(
    "new_file_2.log"
    "hadoop-hdfs-namenode-mesos-01.log"
)

echo "Starting Compression & Integrity Tests with ZIP..."
echo "================================================="

for file in "${files[@]}"; do
    input_file="./Big_logs/$file"
    # Using dynamic naming for artifacts to avoid collisions
    compressed_file="./checks_zip/${file}.zip"
    decompressed_file="./checks_zip/${file}.decompressed.log"

    echo "Processing: $file"

    if [ ! -f "$input_file" ]; then
        echo "!! Error: Input file $input_file not found."
        continue
    fi

    # 1. Compress
    echo ">> [Compressing with zip]"
    # Remove existing zip if any to avoid appending
    rm -f "$compressed_file"
    # -q for quiet, -j for junk paths (don't preserve directory structure)
    time zip -q -j "$compressed_file" "$input_file"
    
    # 2. Check Size
    if [ -f "$compressed_file" ]; then
        size=$(du -h --si "$compressed_file" | cut -f1)
        echo ">> [Artifact Size]: $size"
    else
        echo "!! Error: Compressed file not generated."
        continue
    fi

    # 3. Decompress
    echo ">> [Decompressing with unzip]"
    # -p extracts file to stdout, which we redirect
    time unzip -p "$compressed_file" > "$decompressed_file"

    # 4. Integrity Check
    echo ">> [Verifying Integrity]"
    if diff -q "$input_file" "$decompressed_file" >/dev/null; then
        echo ">> [Result]: PASS (Files verify)"
    else
        echo "!! [Result]: FAIL (Content mismatch)"
    fi

    echo "-----------------------------------------"
done
