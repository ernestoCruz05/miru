//! Episode compression utilities using zstd
//!
//! Compressed files have the `.zst` extension appended to the original filename.
//! e.g., `Episode 01.mkv` becomes `Episode 01.mkv.zst`

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use tracing::{debug, info};

use crate::error::Result;

const ZSTD_EXTENSION: &str = "zst";

/// Check if a file is compressed (has .zst extension)
pub fn is_compressed(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e == ZSTD_EXTENSION)
        .unwrap_or(false)
}

/// Get the compressed path for a file (appends .zst)
pub fn compressed_path(path: &Path) -> PathBuf {
    let mut new_path = path.as_os_str().to_owned();
    new_path.push(".zst");
    PathBuf::from(new_path)
}

/// Get the decompressed path for a compressed file (removes .zst)
pub fn decompressed_path(path: &Path) -> Option<PathBuf> {
    if !is_compressed(path) {
        return None;
    }
    
    let path_str = path.to_string_lossy();
    let new_path = path_str.trim_end_matches(".zst");
    Some(PathBuf::from(new_path))
}

/// Compress a file in place using zstd
/// Returns the path to the compressed file
pub fn compress_file(path: &Path, level: i32) -> Result<PathBuf> {
    let dest_path = compressed_path(path);
    
    info!(
        source = %path.display(),
        dest = %dest_path.display(),
        level = level,
        "Compressing file"
    );

    let input_file = File::open(path)?;
    let input_size = input_file.metadata()?.len();
    let reader = BufReader::with_capacity(1024 * 1024, input_file); // 1MB buffer

    let output_file = File::create(&dest_path)?;
    let writer = BufWriter::with_capacity(1024 * 1024, output_file);

    let mut encoder = zstd::Encoder::new(writer, level)?;
    
    // Copy with progress (could add callback for UI later)
    let mut reader = reader;
    let mut buffer = vec![0u8; 1024 * 1024]; // 1MB chunks
    let mut total_read = 0u64;

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        encoder.write_all(&buffer[..bytes_read])?;
        total_read += bytes_read as u64;
        
        // Log progress every ~100MB
        if total_read % (100 * 1024 * 1024) < (1024 * 1024) {
            debug!(
                progress = format!("{:.1}%", (total_read as f64 / input_size as f64) * 100.0),
                "Compression progress"
            );
        }
    }

    encoder.finish()?;

    // Get compression stats
    let output_size = std::fs::metadata(&dest_path)?.len();
    let ratio = (output_size as f64 / input_size as f64) * 100.0;
    
    info!(
        input_size = input_size,
        output_size = output_size,
        ratio = format!("{:.1}%", ratio),
        "Compression complete"
    );

    // Remove original file
    std::fs::remove_file(path)?;

    Ok(dest_path)
}

/// Decompress a file to a temporary location
/// Returns the path to the decompressed file
pub fn decompress_to_temp(path: &Path) -> Result<PathBuf> {
    let original_name = decompressed_path(path)
        .and_then(|p| p.file_name().map(|n| n.to_owned()))
        .unwrap_or_else(|| std::ffi::OsString::from("video.mkv"));

    // Create temp file with original extension for proper playback
    let temp_dir = tempfile::tempdir()?;
    let temp_path = temp_dir.into_path(); // Keep the directory after TempDir is dropped
    let dest_path = temp_path.join(original_name);

    info!(
        source = %path.display(),
        dest = %dest_path.display(),
        "Decompressing file for playback"
    );

    let input_file = File::open(path)?;
    let reader = BufReader::with_capacity(1024 * 1024, input_file);

    let output_file = File::create(&dest_path)?;
    let mut writer = BufWriter::with_capacity(1024 * 1024, output_file);

    let mut decoder = zstd::Decoder::new(reader)?;
    
    std::io::copy(&mut decoder, &mut writer)?;
    writer.flush()?;

    info!(dest = %dest_path.display(), "Decompression complete");

    Ok(dest_path)
}

/// Decompress a file back to its original location (in-place)
/// Removes the compressed file after successful decompression
pub fn decompress_file(path: &Path) -> Result<PathBuf> {
    let dest_path = decompressed_path(path)
        .ok_or_else(|| crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "File is not compressed",
        )))?;

    info!(
        source = %path.display(),
        dest = %dest_path.display(),
        "Decompressing file"
    );

    let input_file = File::open(path)?;
    let reader = BufReader::with_capacity(1024 * 1024, input_file);

    let output_file = File::create(&dest_path)?;
    let mut writer = BufWriter::with_capacity(1024 * 1024, output_file);

    let mut decoder = zstd::Decoder::new(reader)?;
    
    std::io::copy(&mut decoder, &mut writer)?;
    writer.flush()?;

    // Remove compressed file
    std::fs::remove_file(path)?;

    info!(dest = %dest_path.display(), "Decompression complete");

    Ok(dest_path)
}
