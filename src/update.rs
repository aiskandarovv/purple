use std::io::Read;
use std::path::Path;
use std::sync::mpsc;

use anyhow::{Context, Result};

use crate::event::AppEvent;

/// Current compiled-in version from Cargo.toml.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Parse a semver string "X.Y.Z" into a tuple.
fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.splitn(3, '.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Returns true if `latest` is strictly newer than `current`.
fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// Extract version string from GitHub release JSON.
fn extract_version(json: &serde_json::Value) -> Result<String> {
    let tag = json["tag_name"]
        .as_str()
        .context("Missing tag_name in release")?;

    let version = tag.strip_prefix('v').unwrap_or(tag);

    if parse_version(version).is_none() {
        anyhow::bail!("Invalid version format: {}", version);
    }

    Ok(version.to_string())
}

/// Fetch the latest release version from GitHub.
fn check_latest_version(agent: &ureq::Agent) -> Result<String> {
    let resp = agent
        .get("https://api.github.com/repos/erickochen/purple/releases/latest")
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", &format!("purple-ssh/{}", current_version()))
        .call()
        .context("Failed to fetch latest release. GitHub may be rate-limited.")?;

    let mut body = Vec::new();
    resp.into_reader()
        .take(1_048_576) // 1 MB limit for API response
        .read_to_end(&mut body)
        .context("Failed to read release JSON")?;

    let json: serde_json::Value =
        serde_json::from_slice(&body).context("Failed to parse release JSON")?;

    extract_version(&json)
}

/// TTL for version check cache (24 hours).
const VERSION_CHECK_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Parse cache file content and determine if a newer version is available.
/// Returns `Some(Some(version))` if cache is fresh and a newer version exists,
/// `Some(None)` if cache is fresh and we are up-to-date,
/// `None` if cache content is corrupt, expired or unparseable.
fn parse_version_cache(content: &str, now_secs: u64, current: &str) -> Option<Option<String>> {
    let mut lines = content.lines();
    let timestamp: u64 = lines.next()?.parse().ok()?;
    let version = lines.next()?.to_string();

    if version.is_empty() || parse_version(&version).is_none() {
        return None; // Corrupt version string
    }

    if now_secs.saturating_sub(timestamp) > VERSION_CHECK_TTL.as_secs() {
        return None; // Cache expired
    }

    if is_newer(current, &version) {
        Some(Some(version))
    } else {
        Some(None) // Up-to-date, no API call needed
    }
}

/// Read cached version check result from ~/.purple/last_version_check.
/// Returns `Some(Some(version))` if cache is fresh and a newer version exists,
/// `Some(None)` if cache is fresh and we are up-to-date,
/// `None` if cache is missing, corrupt or expired.
fn read_cached_version() -> Option<Option<String>> {
    let path = dirs::home_dir()?.join(".purple").join("last_version_check");
    let content = std::fs::read_to_string(&path).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    parse_version_cache(&content, now, current_version())
}

/// Write version check result to ~/.purple/last_version_check.
fn write_version_cache(version: &str) {
    let Some(dir) = dirs::home_dir().map(|h| h.join(".purple")) else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = std::fs::write(dir.join("last_version_check"), format!("{}\n{}\n", now, version));
}

/// Spawn a background thread to check for updates. Sends an event if a newer version exists.
/// Uses a local cache (~/.purple/last_version_check) with a 24h TTL to avoid unnecessary
/// GitHub API calls on frequent startup. Silently does nothing on any error.
pub fn spawn_version_check(tx: mpsc::Sender<AppEvent>) {
    let _ = std::thread::Builder::new()
        .name("version-check".to_string())
        .spawn(move || {
            // Check cache first — skip API call if fresh result exists
            match read_cached_version() {
                Some(Some(version)) => {
                    let _ = tx.send(AppEvent::UpdateAvailable { version });
                    return;
                }
                Some(None) => return, // Up-to-date, cache still fresh
                None => {}            // Cache missing or expired, fetch
            }

            // Short timeout: fire-and-forget background check,
            // don't tie up thread resources for 30s like the provider agent
            let agent = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(5))
                .build();

            if let Ok(latest) = check_latest_version(&agent) {
                write_version_cache(&latest);
                if is_newer(current_version(), &latest) {
                    let _ = tx.send(AppEvent::UpdateAvailable { version: latest });
                }
            }
        });
}

/// Format text as bold, respecting NO_COLOR.
fn bold(text: &str) -> String {
    if std::env::var_os("NO_COLOR").is_some() {
        text.to_string()
    } else {
        format!("\x1b[1m{}\x1b[0m", text)
    }
}

/// Format text as bold purple, respecting NO_COLOR.
fn bold_purple(text: &str) -> String {
    if std::env::var_os("NO_COLOR").is_some() {
        text.to_string()
    } else {
        format!("\x1b[1;35m{}\x1b[0m", text)
    }
}

/// Install method detected from binary path.
enum InstallMethod {
    Homebrew,
    Cargo,
    CurlOrManual,
}

/// Check if exe_path is under a Homebrew Cellar directory.
/// Validates that the Cellar path ends with a "Cellar" component and
/// that the binary sits in the expected .../Cellar/<formula>/.../ structure.
fn is_homebrew_path(exe_path: &Path, cellar: &Path) -> bool {
    // Cellar dir must end with "Cellar" component
    if cellar.file_name().and_then(|n| n.to_str()) != Some("Cellar") {
        return false;
    }
    // Path::starts_with is component-aware: /usr/local won't match /usr/local-bin
    if !exe_path.starts_with(cellar) {
        return false;
    }
    // Must have at least one component after Cellar (the formula name)
    exe_path
        .strip_prefix(cellar)
        .is_ok_and(|rest| rest.components().count() >= 1)
}

/// Check if exe_path's parent is exactly <cargo_home>/bin.
fn is_cargo_path(exe_path: &Path, cargo_home: &Path) -> bool {
    let cargo_bin = cargo_home.join("bin");
    exe_path.parent() == Some(cargo_bin.as_path())
}

/// Detect how purple was installed by checking the binary path against
/// known package manager directories. Uses Path::starts_with for
/// component-aware comparison (prevents /usr/local matching /usr/local-bin).
/// Env vars (HOMEBREW_CELLAR, HOMEBREW_PREFIX, CARGO_HOME) are treated
/// as hints and validated structurally before trusting. Falls back to
/// well-known default paths. Fails open to CurlOrManual when uncertain.
fn detect_install_method(exe_path: &Path) -> InstallMethod {
    // Homebrew: check HOMEBREW_CELLAR env var first (most specific),
    // then derive Cellar from HOMEBREW_PREFIX, then fall back to
    // well-known default Cellar locations
    if let Ok(cellar) = std::env::var("HOMEBREW_CELLAR") {
        if is_homebrew_path(exe_path, Path::new(&cellar)) {
            return InstallMethod::Homebrew;
        }
    }
    if let Ok(prefix) = std::env::var("HOMEBREW_PREFIX") {
        let cellar = std::path::PathBuf::from(&prefix).join("Cellar");
        if is_homebrew_path(exe_path, &cellar) {
            return InstallMethod::Homebrew;
        }
    }
    // Default Cellar locations (Apple Silicon + Intel)
    for cellar in ["/opt/homebrew/Cellar", "/usr/local/Cellar"] {
        if is_homebrew_path(exe_path, Path::new(cellar)) {
            return InstallMethod::Homebrew;
        }
    }

    // Cargo: check CARGO_HOME env var first, then check if parent
    // is a "bin" dir inside a ".cargo" dir (component-aware fallback)
    if let Ok(cargo_home) = std::env::var("CARGO_HOME") {
        if is_cargo_path(exe_path, Path::new(&cargo_home)) {
            return InstallMethod::Cargo;
        }
    }
    if let Some(parent) = exe_path.parent() {
        if parent.file_name().and_then(|n| n.to_str()) == Some("bin") {
            if let Some(grandparent) = parent.parent() {
                if grandparent.file_name().and_then(|n| n.to_str()) == Some(".cargo") {
                    return InstallMethod::Cargo;
                }
            }
        }
    }

    InstallMethod::CurlOrManual
}

/// Detect the update command appropriate for how purple was installed.
pub fn update_hint() -> &'static str {
    if std::env::consts::OS != "macos" {
        return "cargo install purple-ssh";
    }
    if let Ok(exe) = std::env::current_exe() {
        let path = std::fs::canonicalize(&exe).unwrap_or(exe);
        return match detect_install_method(&path) {
            InstallMethod::Homebrew => "brew upgrade erickochen/purple/purple",
            InstallMethod::Cargo => "cargo install purple-ssh",
            InstallMethod::CurlOrManual => "purple update",
        };
    }
    "purple update"
}

/// Self-update the purple binary to the latest release.
pub fn self_update() -> Result<()> {
    // macOS only
    if std::env::consts::OS != "macos" {
        anyhow::bail!(
            "Self-update is available on macOS only.\n  \
             Update via: cargo install purple-ssh"
        );
    }

    println!("\n  {} updater\n", bold("purple."));

    // Resolve current binary path
    let exe_path = std::env::current_exe().context("Failed to detect binary path")?;
    let exe_path = std::fs::canonicalize(&exe_path).unwrap_or(exe_path);
    println!("  Binary: {}", exe_path.display());

    // Detect package manager installations
    match detect_install_method(&exe_path) {
        InstallMethod::Homebrew => {
            anyhow::bail!(
                "purple appears to be installed via Homebrew.\n  \
                 Update with: brew upgrade erickochen/purple/purple"
            );
        }
        InstallMethod::Cargo => {
            anyhow::bail!(
                "purple appears to be installed via cargo.\n  \
                 Update with: cargo install purple-ssh"
            );
        }
        InstallMethod::CurlOrManual => {}
    }

    // Fetch latest version
    print!("  Checking for updates... ");
    let agent = crate::providers::http_agent();
    let latest = check_latest_version(&agent)?;
    let current = current_version();

    if !is_newer(current, &latest) {
        println!("already on v{} (latest).", current);
        return Ok(());
    }

    println!("v{} available (current: v{}).", latest, current);

    // Detect target
    let target = match std::env::consts::ARCH {
        "aarch64" => "aarch64-apple-darwin",
        "x86_64" => "x86_64-apple-darwin",
        arch => anyhow::bail!("Unsupported architecture: {}", arch),
    };

    // Check we can write to the binary location
    let parent = exe_path
        .parent()
        .context("Binary has no parent directory")?;

    // Warn when running via sudo — creates root-owned cache files
    if std::env::var_os("SUDO_USER").is_some() {
        eprintln!(
            "  {} Running via sudo. Consider fixing directory permissions instead.",
            bold("!"),
        );
    }

    if !is_writable(parent) {
        anyhow::bail!(
            "No write permission to {}.\n  Check directory permissions or run with elevated privileges.",
            parent.display()
        );
    }

    // Clean up stale staged binaries from interrupted previous updates
    clean_stale_staged(parent);

    // Set up temp directory (create_dir fails if path exists, preventing symlink attacks)
    let tmp_dir = std::env::temp_dir().join(format!(
        "purple_update_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir(&tmp_dir).context("Failed to create temp directory")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_dir, std::fs::Permissions::from_mode(0o700))
            .context("Failed to set temp directory permissions")?;
    }

    // Ensure cleanup on any exit path
    let _cleanup = TempCleanup(&tmp_dir);

    let tarball_name = format!("purple-{}-{}.tar.gz", latest, target);
    let base_url = format!(
        "https://github.com/erickochen/purple/releases/download/v{}",
        latest
    );

    // Download tarball
    print!("  Downloading v{}... ", latest);
    let tarball_path = tmp_dir.join(&tarball_name);
    download_file(
        &agent,
        &format!("{}/{}", base_url, tarball_name),
        &tarball_path,
    )?;

    // Download checksum
    let sha_path = tmp_dir.join(format!("{}.sha256", tarball_name));
    download_file(
        &agent,
        &format!("{}/{}.sha256", base_url, tarball_name),
        &sha_path,
    )?;
    println!("done.");

    // Verify checksum
    print!("  Verifying checksum... ");
    verify_checksum(&tarball_path, &sha_path)?;
    println!("ok.");

    // Extract
    print!("  Installing... ");
    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(&tarball_path)
        .arg("-C")
        .arg(&tmp_dir)
        .status()
        .context("Failed to run tar")?;
    if !status.success() {
        anyhow::bail!("tar extraction failed");
    }

    let new_binary = tmp_dir.join("purple");
    if !new_binary.exists() {
        anyhow::bail!("Binary not found in archive");
    }

    // Atomic replacement: stage new binary in the same directory via O_EXCL
    // (prevents symlink attacks), then rename over the target (atomic within
    // the same filesystem)
    let staged_path = parent.join(format!(".purple_new_{}", std::process::id()));
    {
        use std::io::Write;
        let source = std::fs::read(&new_binary).context("Failed to read new binary")?;
        let mut dest = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true) // O_EXCL: fails if path exists (prevents symlink following)
            .open(&staged_path)
            .context("Failed to create staged binary")?;
        dest.write_all(&source)
            .context("Failed to write staged binary")?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged_path, std::fs::Permissions::from_mode(0o755))
            .context("Failed to set permissions")?;
    }

    if let Err(e) = std::fs::rename(&staged_path, &exe_path) {
        // Clean up staged file on failure
        let _ = std::fs::remove_file(&staged_path);
        return Err(e).context("Failed to replace binary");
    }

    println!("done.");
    println!(
        "\n  {} installed at {}.\n",
        bold_purple(&format!("purple v{}", latest)),
        exe_path.display()
    );

    Ok(())
}

/// Download a file from a URL.
fn download_file(agent: &ureq::Agent, url: &str, dest: &Path) -> Result<()> {
    let resp = agent.get(url).call().with_context(|| {
        format!("Failed to download {}", url)
    })?;

    let mut bytes = Vec::new();
    resp.into_reader()
        .take(100 * 1024 * 1024) // 100 MB limit
        .read_to_end(&mut bytes)
        .context("Failed to read download")?;

    if bytes.is_empty() {
        anyhow::bail!("Empty response from {}", url);
    }

    std::fs::write(dest, bytes).context("Failed to write file")?;
    Ok(())
}

/// Verify SHA256 checksum of a file.
fn verify_checksum(file: &Path, sha_file: &Path) -> Result<()> {
    let expected = std::fs::read_to_string(sha_file)
        .context("Failed to read checksum file")?;
    let expected = expected
        .split_whitespace()
        .next()
        .context("Empty checksum file")?;

    let output = std::process::Command::new("shasum")
        .args(["-a", "256"])
        .arg(file)
        .output()
        .context("Failed to run shasum")?;

    if !output.status.success() {
        anyhow::bail!("shasum failed");
    }

    let actual = String::from_utf8_lossy(&output.stdout);
    let actual = actual
        .split_whitespace()
        .next()
        .context("Empty shasum output")?;

    if expected != actual {
        anyhow::bail!(
            "Checksum mismatch.\n    Expected: {}\n    Got:      {}",
            expected,
            actual
        );
    }

    Ok(())
}

/// Remove stale `.purple_new_*` files from previous interrupted updates.
fn clean_stale_staged(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(".purple_new_") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

/// Check if a directory is writable.
fn is_writable(path: &Path) -> bool {
    let probe = path.join(format!(".purple_write_test_{}", std::process::id()));
    if std::fs::File::create(&probe).is_ok() {
        let _ = std::fs::remove_file(&probe);
        true
    } else {
        false
    }
}

/// RAII guard that removes a temp directory on drop.
struct TempCleanup<'a>(&'a Path);

impl Drop for TempCleanup<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("1.5.0"), Some((1, 5, 0)));
        assert_eq!(parse_version("0.1.2"), Some((0, 1, 2)));
        assert_eq!(parse_version("10.20.30"), Some((10, 20, 30)));
    }

    #[test]
    fn test_parse_version_invalid() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("abc"), None);
        assert_eq!(parse_version("1.2.x"), None);
        assert_eq!(parse_version("1.5.0-rc1"), None);
    }

    #[test]
    fn test_is_newer_patch() {
        assert!(is_newer("1.5.0", "1.5.1"));
        assert!(!is_newer("1.5.1", "1.5.0"));
    }

    #[test]
    fn test_is_newer_minor() {
        assert!(is_newer("1.5.0", "1.6.0"));
        assert!(!is_newer("1.6.0", "1.5.0"));
    }

    #[test]
    fn test_is_newer_major() {
        assert!(is_newer("1.5.0", "2.0.0"));
        assert!(!is_newer("2.0.0", "1.5.0"));
    }

    #[test]
    fn test_is_newer_equal() {
        assert!(!is_newer("1.5.0", "1.5.0"));
    }

    #[test]
    fn test_is_newer_invalid() {
        assert!(!is_newer("1.5.0", "bad"));
        assert!(!is_newer("bad", "1.5.0"));
    }

    #[test]
    fn test_extract_version_with_v_prefix() {
        let json = serde_json::json!({"tag_name": "v1.6.0"});
        assert_eq!(extract_version(&json).unwrap(), "1.6.0");
    }

    #[test]
    fn test_extract_version_without_prefix() {
        let json = serde_json::json!({"tag_name": "1.6.0"});
        assert_eq!(extract_version(&json).unwrap(), "1.6.0");
    }

    #[test]
    fn test_extract_version_missing_tag() {
        let json = serde_json::json!({"name": "Release"});
        assert!(extract_version(&json).is_err());
    }

    #[test]
    fn test_extract_version_invalid_format() {
        let json = serde_json::json!({"tag_name": "v1.2.3-rc1"});
        assert!(extract_version(&json).is_err());
    }

    #[test]
    fn test_current_version_is_valid() {
        assert!(parse_version(current_version()).is_some());
    }

    // --- is_homebrew_path tests ---

    #[test]
    fn test_homebrew_cellar_apple_silicon() {
        let path = Path::new("/opt/homebrew/Cellar/purple/1.5.0/bin/purple");
        assert!(is_homebrew_path(path, Path::new("/opt/homebrew/Cellar")));
    }

    #[test]
    fn test_homebrew_cellar_intel() {
        let path = Path::new("/usr/local/Cellar/purple/1.5.0/bin/purple");
        assert!(is_homebrew_path(path, Path::new("/usr/local/Cellar")));
    }

    #[test]
    fn test_homebrew_cellar_rejects_non_cellar_suffix() {
        // Env var points to a dir that doesn't end in "Cellar"
        let path = Path::new("/opt/homebrew/lib/purple");
        assert!(!is_homebrew_path(path, Path::new("/opt/homebrew/lib")));
    }

    #[test]
    fn test_homebrew_cellar_rejects_bare_cellar() {
        // Binary directly inside Cellar with no formula subdirectory
        let path = Path::new("/opt/homebrew/Cellar");
        assert!(!is_homebrew_path(path, Path::new("/opt/homebrew/Cellar")));
    }

    #[test]
    fn test_homebrew_cellar_rejects_prefix_overlap() {
        // /usr/local/Cellar-custom is not /usr/local/Cellar
        // Path::starts_with is component-aware so this must not match
        let path = Path::new("/usr/local/Cellar-custom/purple/bin/purple");
        assert!(!is_homebrew_path(path, Path::new("/usr/local/Cellar")));
    }

    // --- is_cargo_path tests ---

    #[test]
    fn test_cargo_default_path() {
        let path = Path::new("/Users/user/.cargo/bin/purple");
        assert!(is_cargo_path(path, Path::new("/Users/user/.cargo")));
    }

    #[test]
    fn test_cargo_custom_home() {
        let path = Path::new("/data/rust/cargo/bin/purple");
        assert!(is_cargo_path(path, Path::new("/data/rust/cargo")));
    }

    #[test]
    fn test_cargo_rejects_nested_bin() {
        // Binary in a subdir of bin — not a direct cargo install
        let path = Path::new("/Users/user/.cargo/bin/subdir/purple");
        assert!(!is_cargo_path(path, Path::new("/Users/user/.cargo")));
    }

    #[test]
    fn test_cargo_rejects_prefix_overlap() {
        // /.cargo-custom/bin is not /.cargo/bin
        let path = Path::new("/Users/user/.cargo-custom/bin/purple");
        assert!(!is_cargo_path(path, Path::new("/Users/user/.cargo")));
    }

    // --- detect_install_method tests (path-only, no env vars) ---

    #[test]
    fn test_detect_homebrew_cellar() {
        let path = Path::new("/opt/homebrew/Cellar/purple/1.5.0/bin/purple");
        assert!(matches!(detect_install_method(path), InstallMethod::Homebrew));
    }

    #[test]
    fn test_detect_homebrew_default_intel() {
        let path = Path::new("/usr/local/Cellar/purple/1.5.0/bin/purple");
        assert!(matches!(detect_install_method(path), InstallMethod::Homebrew));
    }

    #[test]
    fn test_detect_cargo_default() {
        let path = Path::new("/Users/user/.cargo/bin/purple");
        assert!(matches!(detect_install_method(path), InstallMethod::Cargo));
    }

    #[test]
    fn test_detect_curl_usr_local_bin() {
        let path = Path::new("/usr/local/bin/purple");
        assert!(matches!(detect_install_method(path), InstallMethod::CurlOrManual));
    }

    #[test]
    fn test_detect_curl_local_bin() {
        let path = Path::new("/Users/user/.local/bin/purple");
        assert!(matches!(detect_install_method(path), InstallMethod::CurlOrManual));
    }

    #[test]
    fn test_detect_no_false_positive_homebrew_in_name() {
        let path = Path::new("/Users/user/homebrew-tools/bin/purple");
        assert!(matches!(detect_install_method(path), InstallMethod::CurlOrManual));
    }

    // --- fail-open: ambiguous paths default to CurlOrManual ---

    #[test]
    fn test_detect_unknown_path() {
        let path = Path::new("/some/random/path/purple");
        assert!(matches!(detect_install_method(path), InstallMethod::CurlOrManual));
    }

    #[test]
    fn test_detect_root_path() {
        let path = Path::new("/purple");
        assert!(matches!(detect_install_method(path), InstallMethod::CurlOrManual));
    }

    // --- parse_version_cache tests ---

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn test_cache_fresh_newer_version() {
        let now = now_secs();
        let content = format!("{}\n99.0.0\n", now);
        // 99.0.0 is newer than any current version
        assert_eq!(
            parse_version_cache(&content, now, "1.5.0"),
            Some(Some("99.0.0".to_string()))
        );
    }

    #[test]
    fn test_cache_fresh_up_to_date() {
        let now = now_secs();
        let content = format!("{}\n1.5.0\n", now);
        // Same version: up-to-date
        assert_eq!(parse_version_cache(&content, now, "1.5.0"), Some(None));
    }

    #[test]
    fn test_cache_fresh_older_version() {
        let now = now_secs();
        let content = format!("{}\n1.0.0\n", now);
        // Cached version is older than current: up-to-date
        assert_eq!(parse_version_cache(&content, now, "1.5.0"), Some(None));
    }

    #[test]
    fn test_cache_expired() {
        let now = now_secs();
        let old = now - VERSION_CHECK_TTL.as_secs() - 1;
        let content = format!("{}\n99.0.0\n", old);
        assert_eq!(parse_version_cache(&content, now, "1.5.0"), None);
    }

    #[test]
    fn test_cache_exactly_at_ttl() {
        let now = now_secs();
        let at_ttl = now - VERSION_CHECK_TTL.as_secs();
        let content = format!("{}\n99.0.0\n", at_ttl);
        // At exactly TTL boundary: still valid (saturating_sub > TTL, not >=)
        assert_eq!(
            parse_version_cache(&content, now, "1.5.0"),
            Some(Some("99.0.0".to_string()))
        );
    }

    #[test]
    fn test_cache_empty_content() {
        assert_eq!(parse_version_cache("", now_secs(), "1.5.0"), None);
    }

    #[test]
    fn test_cache_missing_version_line() {
        let content = format!("{}\n", now_secs());
        assert_eq!(parse_version_cache(&content, now_secs(), "1.5.0"), None);
    }

    #[test]
    fn test_cache_non_numeric_timestamp() {
        assert_eq!(
            parse_version_cache("abc\n99.0.0\n", now_secs(), "1.5.0"),
            None
        );
    }

    #[test]
    fn test_cache_invalid_version_format() {
        let now = now_secs();
        let content = format!("{}\nnot-a-version\n", now);
        assert_eq!(parse_version_cache(&content, now, "1.5.0"), None);
    }

    #[test]
    fn test_cache_empty_version() {
        let now = now_secs();
        // Second line is empty
        let content = format!("{}\n\n", now);
        assert_eq!(parse_version_cache(&content, now, "1.5.0"), None);
    }

    #[test]
    fn test_cache_only_timestamp() {
        let content = format!("{}", now_secs());
        assert_eq!(parse_version_cache(&content, now_secs(), "1.5.0"), None);
    }

    #[test]
    fn test_cache_garbage() {
        assert_eq!(parse_version_cache("garbage", now_secs(), "1.5.0"), None);
    }
}
