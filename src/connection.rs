use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

/// Launch an SSH connection to the given host alias.
/// Uses the system `ssh` binary with inherited stdin/stdout/stderr.
/// Passes `-F <config_path>` so the alias resolves against the correct config file.
pub fn connect(alias: &str, config_path: &Path) -> Result<std::process::ExitStatus> {
    let status = Command::new("ssh")
        .arg("-F")
        .arg(config_path)
        .arg("--")
        .arg(alias)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to launch ssh for '{}'", alias))?;
    Ok(status)
}
