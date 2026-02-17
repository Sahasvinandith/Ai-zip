#!/bin/bash

# Create the output directory if it doesn't exist
mkdir -p ./checks

# List of files to process
files=(
    "hadoop-hdfs-datanode-mesos-32.log"
    "hadoop-hdfs-secondarynamenode-mesos-01.log"
    "hadoop-hdfs-datanode-mesos-31.log"
    "hadoop-hdfs-datanode-mesos-17.log"
    "hadoop-hdfs-datanode-mesos-01.log"
    "new_file_1.log"
    "new_file_2.log"
    "hadoop-hdfs-namenode-mesos-01.log"
)

echo "Starting Compression & Integrity Tests..."
echo "========================================="

for file in "${files[@]}"; do
    input_file="./Big_logs/$file"
    # Using dynamic naming for artifacts to avoid collisions
    compressed_file="./checks/${file}.stz"
    decompressed_file="./checks/${file}.decompressed.log"

    echo "Processing: $file"

    # 1. Compress
    # Outputting time to stderr as per standard `time` behavior
    echo ">> [Compressing]"
    time cargo run -- compress "$input_file" "$compressed_file"
    
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
