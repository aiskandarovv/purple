use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use ratatui::widgets::ListState;

/// A file or directory entry in the browser.
#[derive(Debug, Clone, PartialEq)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

/// Which pane is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserPane {
    Local,
    Remote,
}

/// Pending copy operation awaiting confirmation.
pub struct CopyRequest {
    pub sources: Vec<String>,
    pub source_pane: BrowserPane,
    pub has_dirs: bool,
}

/// State for the dual-pane file browser overlay.
pub struct FileBrowserState {
    pub alias: String,
    pub askpass: Option<String>,
    pub active_pane: BrowserPane,
    // Local
    pub local_path: PathBuf,
    pub local_entries: Vec<FileEntry>,
    pub local_list_state: ListState,
    pub local_selected: HashSet<String>,
    pub local_error: Option<String>,
    // Remote
    pub remote_path: String,
    pub remote_entries: Vec<FileEntry>,
    pub remote_list_state: ListState,
    pub remote_selected: HashSet<String>,
    pub remote_error: Option<String>,
    pub remote_loading: bool,
    // Options
    pub show_hidden: bool,
    // Copy confirmation
    pub confirm_copy: Option<CopyRequest>,
    // Transfer in progress
    pub transferring: Option<String>,
    // Transfer error (shown as dismissible dialog)
    pub transfer_error: Option<String>,
    // Whether the initial remote connection has been recorded in history
    pub connection_recorded: bool,
}

/// List local directory entries.
/// Sorts: directories first, then alphabetical. Filters dotfiles based on show_hidden.
pub fn list_local(path: &Path, show_hidden: bool) -> anyhow::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let metadata = entry.metadata()?;
        let is_dir = metadata.is_dir();
        let size = if is_dir { None } else { Some(metadata.len()) };
        entries.push(FileEntry { name, is_dir, size });
    }
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });
    Ok(entries)
}

/// Parse `ls -lhAL` output into FileEntry list.
/// With -L, symlinks are dereferenced so their target type is shown directly.
/// Recognizes directories via 'd' permission prefix. Skips the "total" line.
/// Broken symlinks are omitted by ls -L (they cannot be transferred anyway).
pub fn parse_ls_output(output: &str, show_hidden: bool) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("total ") {
            continue;
        }
        // ls -l format: permissions links owner group size month day time name
        // Split on whitespace runs, taking 9 fields (last gets the rest including spaces)
        let mut parts: Vec<&str> = Vec::with_capacity(9);
        let mut rest = line;
        for _ in 0..8 {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            parts.push(&rest[..end]);
            rest = &rest[end..];
        }
        rest = rest.trim_start();
        if !rest.is_empty() {
            parts.push(rest);
        }
        if parts.len() < 9 {
            continue;
        }
        let permissions = parts[0];
        let is_dir = permissions.starts_with('d');
        let name = parts[8];
        // Skip empty names
        if name.is_empty() {
            continue;
        }
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        // Parse human-readable size (e.g. "1.1K", "4.0M", "512")
        let size = if is_dir {
            None
        } else {
            Some(parse_human_size(parts[4]))
        };
        entries.push(FileEntry {
            name: name.to_string(),
            is_dir,
            size,
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });
    entries
}

/// Parse a human-readable size string like "1.1K", "4.0M", "512" into bytes.
fn parse_human_size(s: &str) -> u64 {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }
    let last = s.as_bytes()[s.len() - 1];
    let multiplier = match last {
        b'K' => 1024,
        b'M' => 1024 * 1024,
        b'G' => 1024 * 1024 * 1024,
        b'T' => 1024u64 * 1024 * 1024 * 1024,
        _ => 1,
    };
    let num_str = if multiplier > 1 {
        &s[..s.len() - 1]
    } else {
        s
    };
    let num: f64 = num_str.parse().unwrap_or(0.0);
    (num * multiplier as f64) as u64
}

/// Shell-escape a path with single quotes: /path -> '/path'
/// Internal single quotes escaped as '\''
fn shell_escape(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

/// Get the remote home directory via `pwd`.
pub fn get_remote_home(
    alias: &str,
    config_path: &Path,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    has_active_tunnel: bool,
) -> anyhow::Result<String> {
    let result = crate::snippet::run_snippet(
        alias,
        config_path,
        "pwd",
        askpass,
        bw_session,
        true,
        has_active_tunnel,
    )?;
    if result.status.success() {
        Ok(result.stdout.trim().to_string())
    } else {
        anyhow::bail!("Failed to get remote home: {}", result.stderr.trim())
    }
}

/// Fetch remote directory listing synchronously (used by spawn_remote_listing).
pub fn fetch_remote_listing(
    alias: &str,
    config_path: &Path,
    remote_path: &str,
    show_hidden: bool,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    has_tunnel: bool,
) -> Result<Vec<FileEntry>, String> {
    let command = format!("LC_ALL=C ls -lhAL {}", shell_escape(remote_path));
    let result = crate::snippet::run_snippet(
        alias,
        config_path,
        &command,
        askpass,
        bw_session,
        true,
        has_tunnel,
    );
    match result {
        Ok(r) if r.status.success() => Ok(parse_ls_output(&r.stdout, show_hidden)),
        Ok(r) => {
            let msg = r.stderr.trim().to_string();
            if msg.is_empty() {
                Err(format!("ls exited with code {}.", r.status.code().unwrap_or(1)))
            } else {
                Err(msg)
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Spawn background thread for remote directory listing.
/// Sends result back via the provided sender function.
#[allow(clippy::too_many_arguments)]
pub fn spawn_remote_listing<F>(
    alias: String,
    config_path: PathBuf,
    remote_path: String,
    show_hidden: bool,
    askpass: Option<String>,
    bw_session: Option<String>,
    has_tunnel: bool,
    send: F,
) where
    F: FnOnce(String, String, Result<Vec<FileEntry>, String>) + Send + 'static,
{
    std::thread::spawn(move || {
        let listing = fetch_remote_listing(
            &alias,
            &config_path,
            &remote_path,
            show_hidden,
            askpass.as_deref(),
            bw_session.as_deref(),
            has_tunnel,
        );
        send(alias, remote_path, listing);
    });
}

/// Result of an scp transfer.
pub struct ScpResult {
    pub status: ExitStatus,
    pub stderr_output: String,
}

/// Run scp in the background with captured stderr for error reporting.
/// Stderr is piped and captured so errors can be extracted. Progress percentage
/// is not available because scp only outputs progress to a TTY, not to a pipe.
/// Stdin is null (askpass handles authentication). Stdout is null (scp has no
/// meaningful stdout output).
pub fn run_scp(
    alias: &str,
    config_path: &Path,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    has_active_tunnel: bool,
    scp_args: &[String],
) -> anyhow::Result<ScpResult> {
    let mut cmd = Command::new("scp");
    cmd.arg("-F").arg(config_path);

    if has_active_tunnel {
        cmd.arg("-o").arg("ClearAllForwardings=yes");
    }

    for arg in scp_args {
        cmd.arg(arg);
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    if askpass.is_some() {
        let exe = std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().to_string())
            .or_else(|| std::env::args().next())
            .unwrap_or_else(|| "purple".to_string());
        cmd.env("SSH_ASKPASS", &exe)
            .env("SSH_ASKPASS_REQUIRE", "prefer")
            .env("PURPLE_ASKPASS_MODE", "1")
            .env("PURPLE_HOST_ALIAS", alias)
            .env("PURPLE_CONFIG_PATH", config_path.as_os_str());
    }

    if let Some(token) = bw_session {
        cmd.env("BW_SESSION", token);
    }

    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run scp: {}", e))?;

    let stderr_output = String::from_utf8_lossy(&output.stderr).to_string();

    Ok(ScpResult { status: output.status, stderr_output })
}

/// Filter SSH warning noise from stderr, keeping only actionable error lines.
/// Strips lines like "** WARNING: connection is not using a post-quantum key exchange".
pub fn extract_scp_error(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with("** ")
                && !trimmed.starts_with("Warning:")
                && !trimmed.contains("see https://")
                && !trimmed.contains("See https://")
                && !trimmed.starts_with("The server may need")
                && !trimmed.starts_with("This session may be")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build scp arguments for a file transfer.
/// Returns the args to pass after `scp -F <config>`.
///
/// Remote paths are NOT shell-escaped because scp is invoked via Command::arg()
/// which bypasses the shell entirely. The colon in `alias:path` is the only
/// special character scp interprets. Paths with spaces, globbing chars etc. are
/// passed through literally by the OS exec layer.
pub fn build_scp_args(
    alias: &str,
    source_pane: BrowserPane,
    local_path: &Path,
    remote_path: &str,
    filenames: &[String],
    has_dirs: bool,
) -> Vec<String> {
    let mut args = Vec::new();
    if has_dirs {
        args.push("-r".to_string());
    }
    args.push("--".to_string());

    match source_pane {
        // Upload: local files -> remote
        BrowserPane::Local => {
            for name in filenames {
                args.push(local_path.join(name).to_string_lossy().to_string());
            }
            let dest = format!("{}:{}", alias, remote_path);
            args.push(dest);
        }
        // Download: remote files -> local
        BrowserPane::Remote => {
            let base = remote_path.trim_end_matches('/');
            for name in filenames {
                let rpath = format!("{}/{}", base, name);
                args.push(format!("{}:{}", alias, rpath));
            }
            args.push(local_path.to_string_lossy().to_string());
        }
    }
    args
}

/// Format a file size in human-readable form.
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // shell_escape
    // =========================================================================

    #[test]
    fn test_shell_escape_simple() {
        assert_eq!(shell_escape("/home/user"), "'/home/user'");
    }

    #[test]
    fn test_shell_escape_with_single_quote() {
        assert_eq!(shell_escape("/home/it's"), "'/home/it'\\''s'");
    }

    #[test]
    fn test_shell_escape_with_spaces() {
        assert_eq!(shell_escape("/home/my dir"), "'/home/my dir'");
    }

    // =========================================================================
    // parse_ls_output
    // =========================================================================

    #[test]
    fn test_parse_ls_basic() {
        let output = "\
total 24
drwxr-xr-x  2 user user 4096 Jan  1 12:00 subdir
-rw-r--r--  1 user user  512 Jan  1 12:00 file.txt
-rw-r--r--  1 user user 1.1K Jan  1 12:00 big.log
";
        let entries = parse_ls_output(output, true);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "subdir");
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].size, None);
        // Files sorted alphabetically after dirs
        assert_eq!(entries[1].name, "big.log");
        assert!(!entries[1].is_dir);
        assert_eq!(entries[1].size, Some(1126)); // 1.1 * 1024
        assert_eq!(entries[2].name, "file.txt");
        assert!(!entries[2].is_dir);
        assert_eq!(entries[2].size, Some(512));
    }

    #[test]
    fn test_parse_ls_hidden_filter() {
        let output = "\
total 8
-rw-r--r--  1 user user  100 Jan  1 12:00 .hidden
-rw-r--r--  1 user user  200 Jan  1 12:00 visible
";
        let entries = parse_ls_output(output, false);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "visible");

        let entries = parse_ls_output(output, true);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_parse_ls_symlink_to_file_dereferenced() {
        // With -L, symlink to file appears as regular file
        let output = "\
total 4
-rw-r--r--  1 user user   11 Jan  1 12:00 link
";
        let entries = parse_ls_output(output, true);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "link");
        assert!(!entries[0].is_dir);
    }

    #[test]
    fn test_parse_ls_symlink_to_dir_dereferenced() {
        // With -L, symlink to directory appears as directory
        let output = "\
total 4
drwxr-xr-x  3 user user 4096 Jan  1 12:00 link
";
        let entries = parse_ls_output(output, true);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "link");
        assert!(entries[0].is_dir);
    }

    #[test]
    fn test_parse_ls_filename_with_spaces() {
        let output = "\
total 4
-rw-r--r--  1 user user  100 Jan  1 12:00 my file name.txt
";
        let entries = parse_ls_output(output, true);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "my file name.txt");
    }

    #[test]
    fn test_parse_ls_empty() {
        let output = "total 0\n";
        let entries = parse_ls_output(output, true);
        assert!(entries.is_empty());
    }

    // =========================================================================
    // parse_human_size
    // =========================================================================

    #[test]
    fn test_parse_human_size() {
        assert_eq!(parse_human_size("512"), 512);
        assert_eq!(parse_human_size("1.0K"), 1024);
        assert_eq!(parse_human_size("1.5M"), 1572864);
        assert_eq!(parse_human_size("2.0G"), 2147483648);
    }

    // =========================================================================
    // format_size
    // =========================================================================

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(1073741824), "1.0 GB");
    }

    // =========================================================================
    // build_scp_args
    // =========================================================================

    #[test]
    fn test_build_scp_args_upload() {
        let args = build_scp_args(
            "myhost",
            BrowserPane::Local,
            Path::new("/home/user/docs"),
            "/remote/path/",
            &["file.txt".to_string()],
            false,
        );
        assert_eq!(args, vec![
            "--",
            "/home/user/docs/file.txt",
            "myhost:/remote/path/",
        ]);
    }

    #[test]
    fn test_build_scp_args_download() {
        let args = build_scp_args(
            "myhost",
            BrowserPane::Remote,
            Path::new("/home/user/docs"),
            "/remote/path",
            &["file.txt".to_string()],
            false,
        );
        assert_eq!(args, vec![
            "--",
            "myhost:/remote/path/file.txt",
            "/home/user/docs",
        ]);
    }

    #[test]
    fn test_build_scp_args_spaces_in_path() {
        let args = build_scp_args(
            "myhost",
            BrowserPane::Remote,
            Path::new("/local"),
            "/remote/my path",
            &["my file.txt".to_string()],
            false,
        );
        // No shell escaping: Command::arg() passes paths literally
        assert_eq!(args, vec![
            "--",
            "myhost:/remote/my path/my file.txt",
            "/local",
        ]);
    }

    #[test]
    fn test_build_scp_args_with_dirs() {
        let args = build_scp_args(
            "myhost",
            BrowserPane::Local,
            Path::new("/local"),
            "/remote/",
            &["mydir".to_string()],
            true,
        );
        assert_eq!(args[0], "-r");
    }

    // =========================================================================
    // list_local
    // =========================================================================

    #[test]
    fn test_list_local_sorts_dirs_first() {
        let base = std::env::temp_dir().join(format!("purple_fb_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir(base.join("zdir")).unwrap();
        std::fs::write(base.join("afile.txt"), "hello").unwrap();
        std::fs::write(base.join("bfile.txt"), "world").unwrap();

        let entries = list_local(&base, true).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "zdir");
        assert_eq!(entries[1].name, "afile.txt");
        assert_eq!(entries[2].name, "bfile.txt");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_list_local_hidden() {
        let base = std::env::temp_dir().join(format!("purple_fb_hidden_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join(".hidden"), "").unwrap();
        std::fs::write(base.join("visible"), "").unwrap();

        let entries = list_local(&base, false).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "visible");

        let entries = list_local(&base, true).unwrap();
        assert_eq!(entries.len(), 2);

        let _ = std::fs::remove_dir_all(&base);
    }

    // =========================================================================
    // extract_scp_error
    // =========================================================================

    #[test]
    fn test_extract_scp_error_filters_warnings() {
        let stderr = "\
** WARNING: connection is not using a post-quantum key exchange algorithm.
** This session may be vulnerable to \"store now, decrypt later\" attacks.
** The server may need to be upgraded. See https://openssh.com/pq.html
scp: '/root/file.rpm': No such file or directory";
        assert_eq!(
            extract_scp_error(stderr),
            "scp: '/root/file.rpm': No such file or directory"
        );
    }

    #[test]
    fn test_extract_scp_error_keeps_plain_error() {
        let stderr = "scp: /etc/shadow: Permission denied\n";
        assert_eq!(extract_scp_error(stderr), "scp: /etc/shadow: Permission denied");
    }

    #[test]
    fn test_extract_scp_error_empty() {
        assert_eq!(extract_scp_error(""), "");
        assert_eq!(extract_scp_error("  \n  \n"), "");
    }
}
