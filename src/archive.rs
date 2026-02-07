use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use tar::Builder;
use tracing::info;

use crate::error::Result;

pub fn compress_show(
    show_path: &Path,
    archive_dir: &Path,
    compression_level: i32,
) -> Result<PathBuf> {
    fs::create_dir_all(archive_dir)?;

    let show_name = show_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "show".to_string());

    let archive_path = archive_dir.join(format!("{}.tar.zst", show_name));

    info!(
        source = %show_path.display(),
        dest = %archive_path.display(),
        "Compressing show to archive"
    );

    let file = File::create(&archive_path)?;
    let writer = BufWriter::with_capacity(1024 * 1024, file);
    let encoder = zstd::Encoder::new(writer, compression_level)?;
    let mut encoder = encoder.auto_finish();

    let mut tar = Builder::new(&mut encoder);
    tar.append_dir_all(&show_name, show_path)?;
    tar.finish()?;

    drop(tar);
    drop(encoder);

    delete_show_files(show_path)?;

    info!(dest = %archive_path.display(), "Show archived successfully");
    Ok(archive_path)
}

pub fn delete_show_files(show_path: &Path) -> Result<()> {
    if show_path.is_dir() {
        info!(path = %show_path.display(), "Deleting show directory");
        fs::remove_dir_all(show_path)?;
    }
    Ok(())
}
