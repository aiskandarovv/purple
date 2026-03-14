use std::fs;
use std::io;
use std::path::Path;

/// Atomic write: write content to a PID-suffixed temp file with chmod 600, then rename.
/// Uses O_EXCL (create_new) to prevent symlink attacks on the temp file path.
/// Cleans up the temp file on failure.
pub fn atomic_write(path: &Path, content: &[u8]) -> io::Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension(format!("purple_tmp.{}", std::process::id()));

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        // Try O_EXCL first. If a stale tmp file exists from a crashed run, remove
        // it and retry once. This avoids a TOCTOU gap from removing before creating.
        let open = || {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp_path)
        };
        let mut file = match open() {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&tmp_path);
                open().map_err(|e| {
                    io::Error::new(
                        e.kind(),
                        format!("Failed to create temp file {}: {}", tmp_path.display(), e),
                    )
                })?
            }
            Err(e) => {
                return Err(io::Error::new(
                    e.kind(),
                    format!("Failed to create temp file {}: {}", tmp_path.display(), e),
                ));
            }
        };
        file.write_all(content)?;
    }

    #[cfg(not(unix))]
    fs::write(&tmp_path, content)?;

    let result = fs::rename(&tmp_path, path);
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}
