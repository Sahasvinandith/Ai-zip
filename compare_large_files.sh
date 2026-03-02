#!/bin/bash

if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <file1> <file2>"
    exit 1
fi

FILE1="$1"
FILE2="$2"

if [ ! -f "$FILE1" ]; then
    echo "Error: File $FILE1 does not exist."
    exit 1
fi

if [ ! -f "$FILE2" ]; then
    echo "Error: File $FILE2 does not exist."
    exit 1
fi

# Create a temporary directory for the split files
TMP_DIR="./split_compare_tmp"
rm -rf "$TMP_DIR"
mkdir -p "$TMP_DIR/file1_parts"
mkdir -p "$TMP_DIR/file2_parts"

echo "Splitting $FILE1 into chunks of 2,000,000 lines..."
split -l 80000 "$FILE1" "$TMP_DIR/file1_parts/part_"

echo "Splitting $FILE2 into chunks of 2,000,000 lines..."
split -l 80000 "$FILE2" "$TMP_DIR/file2_parts/part_"

echo "Starting comparison..."

# Iterate through the chunks of the first file
for part1 in "$TMP_DIR/file1_parts/"*; do
    # Handle the case where the directory is empty
    if [ ! -e "$part1" ]; then
        break
    fi

    filename=$(basename "$part1")
    part2="$TMP_DIR/file2_parts/$filename"

    # Check if the corresponding chunk exists in the second file
    if [ ! -f "$part2" ]; then
        echo "❌ Difference detected: $FILE1 has more content than $FILE2 (Missing $filename in file2 parts)."
        exit 1
    fi

    echo "Comparing $filename..."
    
    # Run diff and check the exit status
    # Note: diff returns 0 for identical, 1 for different, >1 for trouble.
    if ! diff -w -q "$part1" "$part2" > /dev/null; then
        echo "❌ Difference detected in chunk $filename!"
        echo "Preview of the differences:"
        diff "$part1" "$part2" | head -n 20
        echo "..."
        echo "Process stopped due to detected changes."
        exit 1
    fi
done

# Check if file2 has more chunks than file1
for part2 in "$TMP_DIR/file2_parts/"*; do
    if [ ! -e "$part2" ]; then
        break
    fi

    filename=$(basename "$part2")
    part1="$TMP_DIR/file1_parts/$filename"

    if [ ! -f "$part1" ]; then
        echo "❌ Difference detected: $FILE2 has more content than $FILE1 (Missing $filename in file1 parts)."
        exit 1
    fi
done

echo "✅ Files are completely identical!"

# Clean up
echo "Cleaning up temporary files..."
rm -rf "$TMP_DIR"

exit 0
