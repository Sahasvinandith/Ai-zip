#!/bin/bash

# Create the output directory if it doesn't exist
mkdir -p ./checks

# List of files to process
files=(
#    "Thunderbird_1.log"
#    "Thunderbird_2.log"
#    "Thunderbird_3.log"
   "Windows_1.log"
   "Windows_2.log"
    "Windows_3.log" 
    )

echo "Starting Compression & Integrity Tests..."
echo "========================================="

for file in "${files[@]}"; do
    input_file="./win_thunderbird_logs/$file"
    # Using dynamic naming for artifacts to avoid collisions
    compressed_file="./checks/${file}.stz"
    decompressed_file="./checks/${file}.decompressed.log"

    echo "Processing: $file"

    # 1. Compress
    # Outputting time to stderr as per standard `time` behavior
    echo ">> [Compressing]"
    time cargo run --release -- compress "$input_file" "$compressed_file" --threads 6
    
    # 2. Check Size
    if [ -f "$compressed_file" ]; then
        # du -h provides human-readable size (e.g., 4.0K, 12M)
        size=$(du -h --si "$compressed_file" | cut -f1)
        echo ">> [Artifact Size]: $size"
    else
        echo "!! Error: Compressed file not generated."
        continue
    fi

    # 3. Decompress
    echo ">> [Decompressing]"
    time cargo run --release -- decompress "$compressed_file" "$decompressed_file"

    # 4. Integrity Check
    echo ">> [Verifying Integrity]"
    # cmp returns 0 if files are identical, 1 if different
    # Ignoring whitespace characters (\r, \n, \t, space) because Drain parsing can sometimes lose spacing.
    if cmp <(tr -d ' \t\r\n' < "$input_file") <(tr -d ' \t\r\n' < "$decompressed_file"); then
        echo ">> [Result]: PASS (Files verify)"
    else
        echo "!! [Result]: FAIL (Content mismatch)"
    fi

    echo "-----------------------------------------"
done
