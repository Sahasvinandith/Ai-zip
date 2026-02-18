#!/bin/bash

# Create the output directory if it doesn't exist
mkdir -p ./checks

# List of files to process
files=(
    "Apache.log"
)

echo "Starting Compression & Integrity Tests..."
echo "========================================="

for file in "${files[@]}"; do
    input_file="./$file"
    # Using dynamic naming for artifacts to avoid collisions
    compressed_file="./checks/${file}.stz"
    decompressed_file="./checks/${file}.decompressed.log"

    echo "Processing: $file"

    # 1. Compress
    # Outputting time to stderr as per standard `time` behavior
    echo ">> [Compressing]"
    time cargo run -- compress "$input_file" "$compressed_file" --threads 6
    
    # 2. Check Size
    if [ -f "$compressed_file" ]; then
        # du -h provides human-readable size (e.g., 4.0K, 12M)
        size=$(du -h "$compressed_file" | cut -f1)
        echo ">> [Artifact Size]: $size"
    else
        echo "!! Error: Compressed file not generated."
        continue
    fi

    # 3. Decompress
    echo ">> [Decompressing]"
    time cargo run -- decompress "$compressed_file" "$decompressed_file"

    # 4. Integrity Check
    echo ">> [Verifying Integrity]"
    # git diff returns 0 if files are identical, 1 if different
    if git diff --no-index --quiet "$input_file" "$decompressed_file"; then
        echo ">> [Result]: PASS (Files verify)"
    else
        echo "!! [Result]: FAIL (Content mismatch)"
        # Optional: Remove --quiet above to see the actual diff output
    fi

    echo "-----------------------------------------"
done
