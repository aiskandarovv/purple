use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use ratatui::widgets::ListState;

/// Sort mode for file browser panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserSort {
    Name,
    Date,
    DateAsc,
}

/// A file or directory entry in the browser.
#[derive(Debug, Clone, PartialEq)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    /// Modification time as Unix timestamp (seconds since epoch).
    pub modified: Option<i64>,
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
    pub sort: BrowserSort,
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
/// Sorts: directories first, then by name or date. Filters dotfiles based on show_hidden.
pub fn list_local(path: &Path, show_hidden: bool, sort: BrowserSort) -> anyhow::Result<Vec<FileEntry>> {
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
        let modified = metadata.modified().ok().and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        });
        entries.push(FileEntry { name, is_dir, size, modified });
    }
    sort_entries(&mut entries, sort);
    Ok(entries)
}

/// Sort file entries: directories first, then by the chosen mode.
pub fn sort_entries(entries: &mut [FileEntry], sort: BrowserSort) {
    match sort {
        BrowserSort::Name => {
            entries.sort_by(|a, b| {
                b.is_dir.cmp(&a.is_dir).then_with(|| {
                    a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase())
                })
            });
        }
        BrowserSort::Date => {
            entries.sort_by(|a, b| {
                b.is_dir.cmp(&a.is_dir).then_with(|| {
                    // Newest first: reverse order
                    b.modified.unwrap_or(0).cmp(&a.modified.unwrap_or(0))
                })
            });
        }
        BrowserSort::DateAsc => {
            entries.sort_by(|a, b| {
                b.is_dir.cmp(&a.is_dir).then_with(|| {
                    // Oldest first; unknown dates sort to the end
                    a.modified.unwrap_or(i64::MAX).cmp(&b.modified.unwrap_or(i64::MAX))
                })
            });
        }
    }
}

/// Parse `ls -lhAL` output into FileEntry list.
/// With -L, symlinks are dereferenced so their target type is shown directly.
/// Recognizes directories via 'd' permission prefix. Skips the "total" line.
/// Broken symlinks are omitted by ls -L (they cannot be transferred anyway).
pub fn parse_ls_output(output: &str, show_hidden: bool, sort: BrowserSort) -> Vec<FileEntry> {
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
        // Parse date from month/day/time-or-year (parts[5..=7])
        let modified = parse_ls_date(parts[5], parts[6], parts[7]);
        entries.push(FileEntry {
            name: name.to_string(),
            is_dir,
            size,
            modified,
        });
    }
    sort_entries(&mut entries, sort);
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

/// Parse the date fields from `ls -l` with `LC_ALL=C`.
/// Recent files: "Jan 1 12:34" (month day HH:MM).
/// Old files: "Jan 1 2024" (month day year).
/// Returns approximate Unix timestamp or None if unparseable.
fn parse_ls_date(month_str: &str, day_str: &str, time_or_year: &str) -> Option<i64> {
    let month = match month_str {
        "Jan" => 0, "Feb" => 1, "Mar" => 2, "Apr" => 3,
        "May" => 4, "Jun" => 5, "Jul" => 6, "Aug" => 7,
        "Sep" => 8, "Oct" => 9, "Nov" => 10, "Dec" => 11,
        _ => return None,
    };
    let day: i64 = day_str.parse().ok()?;
    if !(1..=31).contains(&day) {
        return None;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let now_year = epoch_to_year(now);

    if time_or_year.contains(':') {
        // Recent format: "HH:MM"
        let mut parts = time_or_year.splitn(2, ':');
        let hour: i64 = parts.next()?.parse().ok()?;
        let min: i64 = parts.next()?.parse().ok()?;
        // Determine year: if month/day is in the future, it's last year
        let mut year = now_year;
        let approx = approximate_epoch(year, month, day, hour, min);
        if approx > now + 86400 {
            year -= 1;
        }
        Some(approximate_epoch(year, month, day, hour, min))
    } else {
        // Old format: "2024" (year)
        let year: i64 = time_or_year.parse().ok()?;
        if !(1970..=2100).contains(&year) {
            return None;
        }
        Some(approximate_epoch(year, month, day, 0, 0))
    }
}

/// Rough Unix timestamp from date components (no leap second precision needed).
fn approximate_epoch(year: i64, month: i64, day: i64, hour: i64, min: i64) -> i64 {
    // Days from 1970-01-01 to start of year
    let y = year - 1970;
    let mut days = y * 365 + (y + 1) / 4; // approximate leap years
    // Days to start of month (non-leap approximation, close enough for sorting)
    let month_days = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    days += month_days[month as usize];
    // Add leap day if applicable
    if month > 1 && year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
        days += 1;
    }
    days += day - 1;
    days * 86400 + hour * 3600 + min * 60
}

/// Convert epoch seconds to a year (correctly handles year boundaries).
fn epoch_to_year(ts: i64) -> i64 {
    let mut y = 1970 + ts / 31_557_600;
    if approximate_epoch(y, 0, 1, 0, 0) > ts {
        y -= 1;
    } else if approximate_epoch(y + 1, 0, 1, 0, 0) <= ts {
        y += 1;
    }
    y
}

fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Format a Unix timestamp as a relative or short date string.
/// Returns strings like "2m ago", "3h ago", "5d ago", "Jan 15", "Mar 2024".
pub fn format_relative_time(ts: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let diff = now - ts;
    if diff < 0 {
        // Future timestamp (clock skew), just show date
        return format_short_date(ts);
    }
    if diff < 60 {
        return "just now".to_string();
    }
    if diff < 3600 {
        return format!("{}m ago", diff / 60);
    }
    if diff < 86400 {
        return format!("{}h ago", diff / 3600);
    }
    if diff < 86400 * 30 {
        return format!("{}d ago", diff / 86400);
    }
    format_short_date(ts)
}

/// Format a timestamp as "Mon DD" (same year) or "Mon YYYY" (different year).
fn format_short_date(ts: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let now_year = epoch_to_year(now);
    let ts_year = epoch_to_year(ts);

    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                   "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

    // Approximate month and day from day-of-year
    let year_start = approximate_epoch(ts_year, 0, 1, 0, 0);
    let day_of_year = ((ts - year_start) / 86400).max(0) as usize;
    let feb = if is_leap_year(ts_year) { 29 } else { 28 };
    let month_lengths = [31, feb, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0;
    let mut remaining = day_of_year;
    for (i, &len) in month_lengths.iter().enumerate() {
        if remaining < len {
            m = i;
            break;
        }
        remaining -= len;
        m = i + 1;
    }
    let m = m.min(11);
    let d = remaining + 1;

    if ts_year == now_year {
        format!("{} {:>2}", months[m], d)
    } else {
        format!("{} {}", months[m], ts_year)
    }
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
        let msg = filter_ssh_warnings(result.stderr.trim());
        if msg.is_empty() {
            anyhow::bail!("Failed to connect.")
        } else {
            anyhow::bail!("{}", msg)
        }
    }
}

/// Fetch remote directory listing synchronously (used by spawn_remote_listing).
#[allow(clippy::too_many_arguments)]
pub fn fetch_remote_listing(
    alias: &str,
    config_path: &Path,
    remote_path: &str,
    show_hidden: bool,
    sort: BrowserSort,
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
        Ok(r) if r.status.success() => Ok(parse_ls_output(&r.stdout, show_hidden, sort)),
        Ok(r) => {
            let msg = filter_ssh_warnings(r.stderr.trim());
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
    sort: BrowserSort,
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
            sort,
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
pub fn filter_ssh_warnings(stderr: &str) -> String {
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
        let entries = parse_ls_output(output, true, BrowserSort::Name);
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
        let entries = parse_ls_output(output, false, BrowserSort::Name);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "visible");

        let entries = parse_ls_output(output, true, BrowserSort::Name);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_parse_ls_symlink_to_file_dereferenced() {
        // With -L, symlink to file appears as regular file
        let output = "\
total 4
-rw-r--r--  1 user user   11 Jan  1 12:00 link
";
        let entries = parse_ls_output(output, true, BrowserSort::Name);
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
        let entries = parse_ls_output(output, true, BrowserSort::Name);
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
        let entries = parse_ls_output(output, true, BrowserSort::Name);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "my file name.txt");
    }

    #[test]
    fn test_parse_ls_empty() {
        let output = "total 0\n";
        let entries = parse_ls_output(output, true, BrowserSort::Name);
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

        let entries = list_local(&base, true, BrowserSort::Name).unwrap();
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

        let entries = list_local(&base, false, BrowserSort::Name).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "visible");

        let entries = list_local(&base, true, BrowserSort::Name).unwrap();
        assert_eq!(entries.len(), 2);

        let _ = std::fs::remove_dir_all(&base);
    }

    // =========================================================================
    // filter_ssh_warnings
    // =========================================================================

    #[test]
    fn test_filter_ssh_warnings_filters_warnings() {
        let stderr = "\
** WARNING: connection is not using a post-quantum key exchange algorithm.
** This session may be vulnerable to \"store now, decrypt later\" attacks.
** The server may need to be upgraded. See https://openssh.com/pq.html
scp: '/root/file.rpm': No such file or directory";
        assert_eq!(
            filter_ssh_warnings(stderr),
            "scp: '/root/file.rpm': No such file or directory"
        );
    }

    #[test]
    fn test_filter_ssh_warnings_keeps_plain_error() {
        let stderr = "scp: /etc/shadow: Permission denied\n";
        assert_eq!(filter_ssh_warnings(stderr), "scp: /etc/shadow: Permission denied");
    }

    #[test]
    fn test_filter_ssh_warnings_empty() {
        assert_eq!(filter_ssh_warnings(""), "");
        assert_eq!(filter_ssh_warnings("  \n  \n"), "");
    }

    #[test]
    fn test_filter_ssh_warnings_warning_prefix() {
        let stderr = "Warning: Permanently added '10.0.0.1' to the list of known hosts.\nPermission denied (publickey).";
        assert_eq!(filter_ssh_warnings(stderr), "Permission denied (publickey).");
    }

    #[test]
    fn test_filter_ssh_warnings_lowercase_see_https() {
        let stderr = "For details, see https://openssh.com/legacy.html\nConnection refused";
        assert_eq!(filter_ssh_warnings(stderr), "Connection refused");
    }

    #[test]
    fn test_filter_ssh_warnings_only_warnings() {
        let stderr = "** WARNING: connection is not using a post-quantum key exchange algorithm.\n** This session may be vulnerable to \"store now, decrypt later\" attacks.\n** The server may need to be upgraded. See https://openssh.com/pq.html";
        assert_eq!(filter_ssh_warnings(stderr), "");
    }

    // =========================================================================
    // approximate_epoch (known dates)
    // =========================================================================

    #[test]
    fn test_approximate_epoch_known_dates() {
        // 2024-01-01 00:00 UTC = 1704067200
        let ts = approximate_epoch(2024, 0, 1, 0, 0);
        assert_eq!(ts, 1704067200);
        // 2000-01-01 00:00 UTC = 946684800
        let ts = approximate_epoch(2000, 0, 1, 0, 0);
        assert_eq!(ts, 946684800);
        // 1970-01-01 00:00 UTC = 0
        assert_eq!(approximate_epoch(1970, 0, 1, 0, 0), 0);
    }

    #[test]
    fn test_approximate_epoch_leap_year() {
        // 2024-02-29 should differ from 2024-03-01 by 86400
        let feb29 = approximate_epoch(2024, 1, 29, 0, 0);
        let mar01 = approximate_epoch(2024, 2, 1, 0, 0);
        assert_eq!(mar01 - feb29, 86400);
    }

    // =========================================================================
    // epoch_to_year
    // =========================================================================

    #[test]
    fn test_epoch_to_year() {
        assert_eq!(epoch_to_year(0), 1970);
        // 2023-01-01 00:00 UTC = 1672531200
        assert_eq!(epoch_to_year(1672531200), 2023);
        // 2024-01-01 00:00 UTC = 1704067200
        assert_eq!(epoch_to_year(1704067200), 2024);
        // 2024-12-31 23:59:59
        assert_eq!(epoch_to_year(1735689599), 2024);
        // 2025-01-01 00:00:00
        assert_eq!(epoch_to_year(1735689600), 2025);
    }

    // =========================================================================
    // parse_ls_date
    // =========================================================================

    #[test]
    fn test_parse_ls_date_recent_format() {
        // "Jan 15 12:34" - should return a timestamp
        let ts = parse_ls_date("Jan", "15", "12:34");
        assert!(ts.is_some());
        let ts = ts.unwrap();
        // Should be within the last year
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        assert!(ts <= now + 86400);
        assert!(ts > now - 366 * 86400);
    }

    #[test]
    fn test_parse_ls_date_old_format() {
        let ts = parse_ls_date("Mar", "5", "2023");
        assert!(ts.is_some());
        let ts = ts.unwrap();
        // Should be in 2023
        assert_eq!(epoch_to_year(ts), 2023);
    }

    #[test]
    fn test_parse_ls_date_invalid_month() {
        assert!(parse_ls_date("Foo", "1", "12:00").is_none());
    }

    #[test]
    fn test_parse_ls_date_invalid_day() {
        assert!(parse_ls_date("Jan", "0", "12:00").is_none());
        assert!(parse_ls_date("Jan", "32", "12:00").is_none());
    }

    #[test]
    fn test_parse_ls_date_invalid_year() {
        assert!(parse_ls_date("Jan", "1", "1969").is_none());
    }

    // =========================================================================
    // format_relative_time
    // =========================================================================

    #[test]
    fn test_format_relative_time_ranges() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        assert_eq!(format_relative_time(now), "just now");
        assert_eq!(format_relative_time(now - 30), "just now");
        assert_eq!(format_relative_time(now - 120), "2m ago");
        assert_eq!(format_relative_time(now - 7200), "2h ago");
        assert_eq!(format_relative_time(now - 86400 * 3), "3d ago");
    }

    #[test]
    fn test_format_relative_time_old_date() {
        // A date far in the past should show short date format
        let old = approximate_epoch(2020, 5, 15, 0, 0);
        let result = format_relative_time(old);
        assert!(result.contains("2020"), "Expected year in '{}' for old date", result);
    }

    #[test]
    fn test_format_relative_time_future() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        // Future timestamp should not panic and should show date
        let result = format_relative_time(now + 86400 * 30);
        assert!(!result.is_empty());
    }

    // =========================================================================
    // format_short_date
    // =========================================================================

    #[test]
    fn test_format_short_date_different_year() {
        let ts = approximate_epoch(2020, 2, 15, 0, 0); // Mar 15 2020
        let result = format_short_date(ts);
        assert!(result.contains("2020"), "Expected year in '{}'", result);
        assert!(result.starts_with("Mar"), "Expected Mar in '{}'", result);
    }

    #[test]
    fn test_format_short_date_leap_year() {
        // Mar 1 2024 (leap year, different year) should show "Mar 2024"
        let ts = approximate_epoch(2024, 2, 1, 0, 0);
        let result = format_short_date(ts);
        assert!(result.starts_with("Mar"), "Expected Mar in '{}'", result);
        assert!(result.contains("2024"), "Expected 2024 in '{}'", result);
        // Verify Feb 29 and Mar 1 are distinct days (86400 apart)
        let feb29 = approximate_epoch(2024, 1, 29, 12, 0);
        let mar01 = approximate_epoch(2024, 2, 1, 12, 0);
        let feb29_date = format_short_date(feb29);
        let mar01_date = format_short_date(mar01);
        assert!(feb29_date.starts_with("Feb"), "Expected Feb in '{}'", feb29_date);
        assert!(mar01_date.starts_with("Mar"), "Expected Mar in '{}'", mar01_date);
    }

    // =========================================================================
    // sort_entries (date mode)
    // =========================================================================

    #[test]
    fn test_sort_entries_date_dirs_first_newest_first() {
        let mut entries = vec![
            FileEntry { name: "old.txt".into(), is_dir: false, size: Some(100), modified: Some(1000) },
            FileEntry { name: "new.txt".into(), is_dir: false, size: Some(200), modified: Some(3000) },
            FileEntry { name: "mid.txt".into(), is_dir: false, size: Some(150), modified: Some(2000) },
            FileEntry { name: "adir".into(), is_dir: true, size: None, modified: Some(500) },
        ];
        sort_entries(&mut entries, BrowserSort::Date);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "adir");
        assert_eq!(entries[1].name, "new.txt");
        assert_eq!(entries[2].name, "mid.txt");
        assert_eq!(entries[3].name, "old.txt");
    }

    #[test]
    fn test_sort_entries_name_mode() {
        let mut entries = vec![
            FileEntry { name: "zebra.txt".into(), is_dir: false, size: Some(100), modified: Some(3000) },
            FileEntry { name: "alpha.txt".into(), is_dir: false, size: Some(200), modified: Some(1000) },
            FileEntry { name: "mydir".into(), is_dir: true, size: None, modified: Some(2000) },
        ];
        sort_entries(&mut entries, BrowserSort::Name);
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].name, "alpha.txt");
        assert_eq!(entries[2].name, "zebra.txt");
    }

    // =========================================================================
    // parse_ls_output with modified field
    // =========================================================================

    #[test]
    fn test_parse_ls_output_populates_modified() {
        let output = "\
total 4
-rw-r--r--  1 user user  512 Jan  1 12:00 file.txt
";
        let entries = parse_ls_output(output, true, BrowserSort::Name);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].modified.is_some(), "modified should be populated");
    }

    #[test]
    fn test_parse_ls_output_date_sort() {
        // Use year format to avoid ambiguity with current date
        let output = "\
total 12
-rw-r--r--  1 user user  100 Jan  1  2020 old.txt
-rw-r--r--  1 user user  200 Jun 15  2023 new.txt
-rw-r--r--  1 user user  150 Mar  5  2022 mid.txt
";
        let entries = parse_ls_output(output, true, BrowserSort::Date);
        assert_eq!(entries.len(), 3);
        // Should be sorted newest first (2023 > 2022 > 2020)
        assert_eq!(entries[0].name, "new.txt");
        assert_eq!(entries[1].name, "mid.txt");
        assert_eq!(entries[2].name, "old.txt");
    }

    // =========================================================================
    // list_local with modified field
    // =========================================================================

    #[test]
    fn test_list_local_populates_modified() {
        let base = std::env::temp_dir().join(format!("purple_fb_mtime_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("test.txt"), "hello").unwrap();

        let entries = list_local(&base, true, BrowserSort::Name).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].modified.is_some(), "modified should be populated for local files");

        let _ = std::fs::remove_dir_all(&base);
    }

    // =========================================================================
    // epoch_to_year boundary
    // =========================================================================

    #[test]
    fn test_epoch_to_year_2100_boundary() {
        let ts_2100 = approximate_epoch(2100, 0, 1, 0, 0);
        assert_eq!(epoch_to_year(ts_2100), 2100);
        assert_eq!(epoch_to_year(ts_2100 - 1), 2099);
        let mid_2100 = approximate_epoch(2100, 5, 15, 12, 0);
        assert_eq!(epoch_to_year(mid_2100), 2100);
    }

    // =========================================================================
    // parse_ls_date edge cases
    // =========================================================================

    #[test]
    fn test_parse_ls_date_midnight() {
        let ts = parse_ls_date("Jan", "1", "00:00");
        assert!(ts.is_some(), "00:00 should parse successfully");
        let ts = ts.unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        assert!(ts <= now + 86400);
        assert!(ts > now - 366 * 86400);
    }

    // =========================================================================
    // sort_entries edge cases
    // =========================================================================

    #[test]
    fn test_sort_entries_date_with_none_modified() {
        let mut entries = vec![
            FileEntry { name: "known.txt".into(), is_dir: false, size: Some(100), modified: Some(5000) },
            FileEntry { name: "unknown.txt".into(), is_dir: false, size: Some(200), modified: None },
            FileEntry { name: "recent.txt".into(), is_dir: false, size: Some(300), modified: Some(9000) },
        ];
        sort_entries(&mut entries, BrowserSort::Date);
        assert_eq!(entries[0].name, "recent.txt");
        assert_eq!(entries[1].name, "known.txt");
        assert_eq!(entries[2].name, "unknown.txt");
    }

    #[test]
    fn test_sort_entries_date_asc_oldest_first() {
        let mut entries = vec![
            FileEntry { name: "old.txt".into(), is_dir: false, size: Some(100), modified: Some(1000) },
            FileEntry { name: "new.txt".into(), is_dir: false, size: Some(200), modified: Some(3000) },
            FileEntry { name: "mid.txt".into(), is_dir: false, size: Some(150), modified: Some(2000) },
            FileEntry { name: "adir".into(), is_dir: true, size: None, modified: Some(500) },
        ];
        sort_entries(&mut entries, BrowserSort::DateAsc);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "adir");
        assert_eq!(entries[1].name, "old.txt");
        assert_eq!(entries[2].name, "mid.txt");
        assert_eq!(entries[3].name, "new.txt");
    }

    #[test]
    fn test_sort_entries_date_asc_none_modified_sorts_to_end() {
        let mut entries = vec![
            FileEntry { name: "known.txt".into(), is_dir: false, size: Some(100), modified: Some(5000) },
            FileEntry { name: "unknown.txt".into(), is_dir: false, size: Some(200), modified: None },
            FileEntry { name: "old.txt".into(), is_dir: false, size: Some(300), modified: Some(1000) },
        ];
        sort_entries(&mut entries, BrowserSort::DateAsc);
        assert_eq!(entries[0].name, "old.txt");
        assert_eq!(entries[1].name, "known.txt");
        assert_eq!(entries[2].name, "unknown.txt"); // None sorts to end
    }

    #[test]
    fn test_parse_ls_output_date_asc_sort() {
        let output = "\
total 12
-rw-r--r--  1 user user  100 Jan  1  2020 old.txt
-rw-r--r--  1 user user  200 Jun 15  2023 new.txt
-rw-r--r--  1 user user  150 Mar  5  2022 mid.txt
";
        let entries = parse_ls_output(output, true, BrowserSort::DateAsc);
        assert_eq!(entries.len(), 3);
        // Should be sorted oldest first (2020 < 2022 < 2023)
        assert_eq!(entries[0].name, "old.txt");
        assert_eq!(entries[1].name, "mid.txt");
        assert_eq!(entries[2].name, "new.txt");
    }

    #[test]
    fn test_sort_entries_date_multiple_dirs() {
        let mut entries = vec![
            FileEntry { name: "old_dir".into(), is_dir: true, size: None, modified: Some(1000) },
            FileEntry { name: "new_dir".into(), is_dir: true, size: None, modified: Some(3000) },
            FileEntry { name: "mid_dir".into(), is_dir: true, size: None, modified: Some(2000) },
            FileEntry { name: "file.txt".into(), is_dir: false, size: Some(100), modified: Some(5000) },
        ];
        sort_entries(&mut entries, BrowserSort::Date);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "new_dir");
        assert_eq!(entries[1].name, "mid_dir");
        assert_eq!(entries[2].name, "old_dir");
        assert_eq!(entries[3].name, "file.txt");
    }

    // =========================================================================
    // format_relative_time boundaries
    // =========================================================================

    #[test]
    fn test_format_relative_time_exactly_60s() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        assert_eq!(format_relative_time(now - 60), "1m ago");
        assert_eq!(format_relative_time(now - 59), "just now");
    }

    // =========================================================================
    // parse_ls_output date sort with dirs
    // =========================================================================

    #[test]
    fn test_parse_ls_output_date_sort_with_dirs() {
        let output = "\
total 16
drwxr-xr-x  2 user user 4096 Jan  1  2020 old_dir
-rw-r--r--  1 user user  200 Jun 15  2023 new_file.txt
drwxr-xr-x  2 user user 4096 Dec  1  2023 new_dir
-rw-r--r--  1 user user  100 Mar  5  2022 old_file.txt
";
        let entries = parse_ls_output(output, true, BrowserSort::Date);
        assert_eq!(entries.len(), 4);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "new_dir");
        assert!(entries[1].is_dir);
        assert_eq!(entries[1].name, "old_dir");
        assert_eq!(entries[2].name, "new_file.txt");
        assert_eq!(entries[3].name, "old_file.txt");
    }
}
