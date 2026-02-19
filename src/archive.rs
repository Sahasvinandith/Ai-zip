use crate::compressor;
use crate::decompressor;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const STZ_ARCHIVE_MAGIC: &[u8] = b"STZ\x02";

pub fn create_archive(
    input_root: &str,
    output_path: &str,
    num_threads: usize,
) -> std::io::Result<()> {
    let mut out_file = File::create(output_path)?;
    out_file.write_all(STZ_ARCHIVE_MAGIC)?;

    let root_path = Path::new(input_root);

    for entry in WalkDir::new(root_path) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let file_path = entry.path();

            // Compute relative path
            let relative_path = file_path
                .strip_prefix(root_path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            let relative_path_str = relative_path.to_string_lossy();

            println!("Archiving: {}", relative_path_str);

            // 1. Write Header
            let path_bytes = relative_path_str.as_bytes();
            out_file.write_all(&(path_bytes.len() as u32).to_le_bytes())?;
            out_file.write_all(path_bytes)?;

            // 2. Write Placeholder for Content Length (u64)
            let len_pos = out_file.stream_position()?;
            out_file.write_all(&0u64.to_le_bytes())?;

            // 3. Compress File to Stream
            let start_pos = out_file.stream_position()?;

            // We need to clone the file handle because compress_file sends it to a thread,
            // requiring 'static lifetime or ownership. A simple reference won't work.
            let out_file_clone = out_file.try_clone()?;
            compressor::compress_file(file_path, out_file_clone, num_threads, false, false)?;

            let end_pos = out_file.stream_position()?;

            // 4. Update Content Length
            let content_len = end_pos - start_pos;
            out_file.seek(SeekFrom::Start(len_pos))?;
            out_file.write_all(&content_len.to_le_bytes())?;
            out_file.seek(SeekFrom::Start(end_pos))?;
        }
    }
    println!("Archive created at {}", output_path);
    Ok(())
}

pub fn extract_archive(input_path: &str, output_root: &str) -> std::io::Result<()> {
    let mut in_file = File::open(input_path)?;

    let mut magic = [0u8; 4];
    in_file.read_exact(&mut magic)?;

    if &magic != STZ_ARCHIVE_MAGIC {
        // Fallback to legacy single file decompression if generic magic is found?
        // But STZ\x02 identifies Archive.
        // If it's STZ1 (bytes), it's a single file.
        if &magic == b"STZ1" {
            println!(
                "Detected single file archive (STZ1). Extracting to {}",
                output_root
            );
            // In this case, output_root is treated as the file path?
            // Or should we fail?
            // User said: "decompress original names or file".
            // If legacy file, we probably expect output_path to be a file.
            // But existing CLI: `decompress <input_stz> <output_file>`
            // New CLI: `decompress <input_stz> <output_dir>`?
            // To be backwards compatible with main.rs logic:
            // main.rs handles dispatching.
            // If we are here, we expect an archive.
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Not a directory archive (STZ2). Use legacy decompress for single files.",
            ));
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid Archive Header",
        ));
    }

    loop {
        // 1. Read Path Length
        let mut len_buf = [0u8; 4];
        match in_file.read_exact(&mut len_buf) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break, // EOF
            Err(e) => return Err(e),
        }
        let path_len = u32::from_le_bytes(len_buf) as usize;

        // 2. Read Path
        let mut path_bytes = vec![0u8; path_len];
        in_file.read_exact(&mut path_bytes)?;
        let relative_path = String::from_utf8_lossy(&path_bytes).to_string();

        // 3. Read Content Length
        let mut size_buf = [0u8; 8];
        in_file.read_exact(&mut size_buf)?;
        let content_len = u64::from_le_bytes(size_buf);

        // 4. Extract
        let target_path = Path::new(output_root).join(&relative_path);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        println!("Extracting: {} ({} bytes)", relative_path, content_len);

        // We need to read `content_len` bytes and decompress them.
        // `decompress_stream` takes a Reader.
        // We can limit the reader using `take`.

        // However, `decompressor::decompress` (legacy) took paths.
        // We need to refactor decompressor to accept a Reader.
        // For now, let's create a partial file writer? No, we should refactor decompressor.
        // But wait, `decompressor.rs` wasn't refactored yet!
        // Task said "Design and implement ArchiveReader".
        // I need to update decompressor.rs to expose `decompress_stream`.

        // Workaround: Read content to temp buffer (if small) or use `take`.
        // `decompressor::decompress_stream` needs to be implemented.
        let mut handle = in_file.take(content_len);

        // Skip/Verify STZ1 magic bytes inside the entry
        let mut entry_magic = [0u8; 4];
        if handle.read_exact(&mut entry_magic).is_ok() {
            if &entry_magic != b"STZ1" {
                // Warn but try to proceed? Or fail?
                // Some compressors implementation might change.
                // But `compress_file` writes it.
                eprintln!("Warning: Entry {} missing STZ1 magic", relative_path);
            }
        } else {
            eprintln!("Error: Empty entry {}", relative_path);
        }

        let mut out_file = File::create(&target_path)?;
        // We need `decompress_stream`!
        decompressor::LogDecompressor::decompress_to_writer(&mut handle, &mut out_file)?;

        // Recover original file handle
        in_file = handle.into_inner();
    }

    Ok(())
}
