use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::fs_util;

/// A saved command snippet.
#[derive(Debug, Clone, PartialEq)]
pub struct Snippet {
    pub name: String,
    pub command: String,
    pub description: String,
}

/// Result of running a snippet on a host.
pub struct SnippetResult {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Snippet storage backed by ~/.purple/snippets (INI-style).
#[derive(Debug, Clone, Default)]
pub struct SnippetStore {
    pub snippets: Vec<Snippet>,
    /// Override path for save(). None uses the default ~/.purple/snippets.
    pub path_override: Option<PathBuf>,
}

fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".purple/snippets"))
}

impl SnippetStore {
    /// Load snippets from ~/.purple/snippets.
    /// Returns empty store if file doesn't exist (normal first-use).
    pub fn load() -> Self {
        let path = match config_path() {
            Some(p) => p,
            None => return Self::default(),
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                eprintln!("! Could not read {}: {}", path.display(), e);
                return Self::default();
            }
        };
        Self::parse(&content)
    }

    /// Parse INI-style snippet config.
    pub fn parse(content: &str) -> Self {
        let mut snippets = Vec::new();
        let mut current: Option<Snippet> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if let Some(snippet) = current.take() {
                    if !snippet.command.is_empty()
                        && !snippets.iter().any(|s: &Snippet| s.name == snippet.name)
                    {
                        snippets.push(snippet);
                    }
                }
                let name = trimmed[1..trimmed.len() - 1].trim().to_string();
                if snippets.iter().any(|s| s.name == name) {
                    current = None;
                    continue;
                }
                current = Some(Snippet {
                    name,
                    command: String::new(),
                    description: String::new(),
                });
            } else if let Some(ref mut snippet) = current {
                if let Some((key, value)) = trimmed.split_once('=') {
                    let key = key.trim();
                    let value = value.trim().to_string();
                    match key {
                        "command" => snippet.command = value,
                        "description" => snippet.description = value,
                        _ => {}
                    }
                }
            }
        }
        if let Some(snippet) = current {
            if !snippet.command.is_empty()
                && !snippets.iter().any(|s| s.name == snippet.name)
            {
                snippets.push(snippet);
            }
        }
        Self {
            snippets,
            path_override: None,
        }
    }

    /// Save snippets to ~/.purple/snippets (atomic write, chmod 600).
    pub fn save(&self) -> io::Result<()> {
        let path = match &self.path_override {
            Some(p) => p.clone(),
            None => match config_path() {
                Some(p) => p,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "Could not determine home directory",
                    ))
                }
            },
        };

        let mut content = String::new();
        for (i, snippet) in self.snippets.iter().enumerate() {
            if i > 0 {
                content.push('\n');
            }
            content.push_str(&format!("[{}]\n", snippet.name));
            content.push_str(&format!("command={}\n", snippet.command));
            if !snippet.description.is_empty() {
                content.push_str(&format!("description={}\n", snippet.description));
            }
        }

        fs_util::atomic_write(&path, content.as_bytes())
    }

    /// Get a snippet by name.
    pub fn get(&self, name: &str) -> Option<&Snippet> {
        self.snippets.iter().find(|s| s.name == name)
    }

    /// Add or replace a snippet.
    pub fn set(&mut self, snippet: Snippet) {
        if let Some(existing) = self.snippets.iter_mut().find(|s| s.name == snippet.name) {
            *existing = snippet;
        } else {
            self.snippets.push(snippet);
        }
    }

    /// Remove a snippet by name.
    pub fn remove(&mut self, name: &str) {
        self.snippets.retain(|s| s.name != name);
    }
}

/// Validate a snippet name: non-empty, no whitespace, no `#`, no `[`, no `]`,
/// no control characters.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Snippet name cannot be empty.".to_string());
    }
    if name.contains(char::is_whitespace) {
        return Err("Snippet name cannot contain whitespace.".to_string());
    }
    if name.contains('#') || name.contains('[') || name.contains(']') {
        return Err("Snippet name cannot contain #, [ or ].".to_string());
    }
    if name.contains(|c: char| c.is_control()) {
        return Err("Snippet name cannot contain control characters.".to_string());
    }
    Ok(())
}

/// Validate a snippet command: non-empty, no control characters (except tab).
pub fn validate_command(command: &str) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err("Command cannot be empty.".to_string());
    }
    if command.contains(|c: char| c.is_control() && c != '\t') {
        return Err("Command cannot contain control characters.".to_string());
    }
    Ok(())
}

/// Run a snippet on a single host via SSH.
/// When `capture` is true, stdout/stderr are piped and returned in the result.
/// When `capture` is false, stdout/stderr are inherited (streamed to terminal
/// in real-time) and the returned strings are empty.
pub fn run_snippet(
    alias: &str,
    config_path: &Path,
    command: &str,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    capture: bool,
    has_active_tunnel: bool,
) -> anyhow::Result<SnippetResult> {
    let mut cmd = Command::new("ssh");
    cmd.arg("-F")
        .arg(config_path)
        .arg("-o")
        .arg("ConnectTimeout=10");

    // When a tunnel is already running for this host, disable forwards
    // to avoid "Address already in use" bind conflicts.
    if has_active_tunnel {
        cmd.arg("-o").arg("ClearAllForwardings=yes");
    }

    cmd.arg("--")
        .arg(alias)
        .arg(command)
        .stdin(Stdio::inherit());

    if capture {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    }

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

    if capture {
        let output = cmd
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to run ssh for '{}': {}", alias, e))?;

        Ok(SnippetResult {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    } else {
        let status = cmd
            .status()
            .map_err(|e| anyhow::anyhow!("Failed to run ssh for '{}': {}", alias, e))?;

        Ok(SnippetResult {
            status,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Parse
    // =========================================================================

    #[test]
    fn test_parse_empty() {
        let store = SnippetStore::parse("");
        assert!(store.snippets.is_empty());
    }

    #[test]
    fn test_parse_single_snippet() {
        let content = "\
[check-disk]
command=df -h
description=Check disk usage
";
        let store = SnippetStore::parse(content);
        assert_eq!(store.snippets.len(), 1);
        let s = &store.snippets[0];
        assert_eq!(s.name, "check-disk");
        assert_eq!(s.command, "df -h");
        assert_eq!(s.description, "Check disk usage");
    }

    #[test]
    fn test_parse_multiple_snippets() {
        let content = "\
[check-disk]
command=df -h

[uptime]
command=uptime
description=Check server uptime
";
        let store = SnippetStore::parse(content);
        assert_eq!(store.snippets.len(), 2);
        assert_eq!(store.snippets[0].name, "check-disk");
        assert_eq!(store.snippets[1].name, "uptime");
    }

    #[test]
    fn test_parse_comments_and_blanks() {
        let content = "\
# Snippet config

[check-disk]
# Main command
command=df -h
";
        let store = SnippetStore::parse(content);
        assert_eq!(store.snippets.len(), 1);
        assert_eq!(store.snippets[0].command, "df -h");
    }

    #[test]
    fn test_parse_duplicate_sections_first_wins() {
        let content = "\
[check-disk]
command=df -h

[check-disk]
command=du -sh *
";
        let store = SnippetStore::parse(content);
        assert_eq!(store.snippets.len(), 1);
        assert_eq!(store.snippets[0].command, "df -h");
    }

    #[test]
    fn test_parse_snippet_without_command_skipped() {
        let content = "\
[empty]
description=No command here

[valid]
command=ls -la
";
        let store = SnippetStore::parse(content);
        assert_eq!(store.snippets.len(), 1);
        assert_eq!(store.snippets[0].name, "valid");
    }

    #[test]
    fn test_parse_unknown_keys_ignored() {
        let content = "\
[check-disk]
command=df -h
unknown=value
foo=bar
";
        let store = SnippetStore::parse(content);
        assert_eq!(store.snippets.len(), 1);
        assert_eq!(store.snippets[0].command, "df -h");
    }

    #[test]
    fn test_parse_whitespace_in_section_name() {
        let content = "[ check-disk ]\ncommand=df -h\n";
        let store = SnippetStore::parse(content);
        assert_eq!(store.snippets[0].name, "check-disk");
    }

    #[test]
    fn test_parse_whitespace_around_key_value() {
        let content = "[check-disk]\n  command  =  df -h  \n";
        let store = SnippetStore::parse(content);
        assert_eq!(store.snippets[0].command, "df -h");
    }

    #[test]
    fn test_parse_command_with_equals() {
        let content = "[env-check]\ncommand=env | grep HOME=\n";
        let store = SnippetStore::parse(content);
        assert_eq!(store.snippets[0].command, "env | grep HOME=");
    }

    #[test]
    fn test_parse_line_without_equals_ignored() {
        let content = "[check]\ncommand=ls\ngarbage_line\n";
        let store = SnippetStore::parse(content);
        assert_eq!(store.snippets[0].command, "ls");
    }

    // =========================================================================
    // Get / Set / Remove
    // =========================================================================

    #[test]
    fn test_get_found() {
        let store = SnippetStore::parse("[check]\ncommand=ls\n");
        assert!(store.get("check").is_some());
    }

    #[test]
    fn test_get_not_found() {
        let store = SnippetStore::parse("");
        assert!(store.get("nope").is_none());
    }

    #[test]
    fn test_set_adds_new() {
        let mut store = SnippetStore::default();
        store.set(Snippet {
            name: "check".to_string(),
            command: "ls".to_string(),
            description: String::new(),
        });
        assert_eq!(store.snippets.len(), 1);
    }

    #[test]
    fn test_set_replaces_existing() {
        let mut store = SnippetStore::parse("[check]\ncommand=ls\n");
        store.set(Snippet {
            name: "check".to_string(),
            command: "df -h".to_string(),
            description: String::new(),
        });
        assert_eq!(store.snippets.len(), 1);
        assert_eq!(store.snippets[0].command, "df -h");
    }

    #[test]
    fn test_remove() {
        let mut store = SnippetStore::parse("[check]\ncommand=ls\n[uptime]\ncommand=uptime\n");
        store.remove("check");
        assert_eq!(store.snippets.len(), 1);
        assert_eq!(store.snippets[0].name, "uptime");
    }

    #[test]
    fn test_remove_nonexistent_noop() {
        let mut store = SnippetStore::parse("[check]\ncommand=ls\n");
        store.remove("nope");
        assert_eq!(store.snippets.len(), 1);
    }

    // =========================================================================
    // Validate name
    // =========================================================================

    #[test]
    fn test_validate_name_valid() {
        assert!(validate_name("check-disk").is_ok());
        assert!(validate_name("restart_nginx").is_ok());
        assert!(validate_name("a").is_ok());
    }

    #[test]
    fn test_validate_name_empty() {
        assert!(validate_name("").is_err());
    }

    #[test]
    fn test_validate_name_whitespace() {
        assert!(validate_name("check disk").is_err());
        assert!(validate_name("check\tdisk").is_err());
    }

    #[test]
    fn test_validate_name_special_chars() {
        assert!(validate_name("check#disk").is_err());
        assert!(validate_name("[check]").is_err());
    }

    #[test]
    fn test_validate_name_control_chars() {
        assert!(validate_name("check\x00disk").is_err());
    }

    // =========================================================================
    // Validate command
    // =========================================================================

    #[test]
    fn test_validate_command_valid() {
        assert!(validate_command("df -h").is_ok());
        assert!(validate_command("cat /etc/hosts | grep localhost").is_ok());
        assert!(validate_command("echo 'hello\tworld'").is_ok()); // tab allowed
    }

    #[test]
    fn test_validate_command_empty() {
        assert!(validate_command("").is_err());
    }

    #[test]
    fn test_validate_command_whitespace_only() {
        assert!(validate_command("   ").is_err());
        assert!(validate_command(" \t ").is_err());
    }

    #[test]
    fn test_validate_command_control_chars() {
        assert!(validate_command("ls\x00-la").is_err());
    }

    // =========================================================================
    // Save / roundtrip
    // =========================================================================

    #[test]
    fn test_save_roundtrip() {
        let mut store = SnippetStore::default();
        store.set(Snippet {
            name: "check-disk".to_string(),
            command: "df -h".to_string(),
            description: "Check disk usage".to_string(),
        });
        store.set(Snippet {
            name: "uptime".to_string(),
            command: "uptime".to_string(),
            description: String::new(),
        });

        // Serialize
        let mut content = String::new();
        for (i, snippet) in store.snippets.iter().enumerate() {
            if i > 0 {
                content.push('\n');
            }
            content.push_str(&format!("[{}]\n", snippet.name));
            content.push_str(&format!("command={}\n", snippet.command));
            if !snippet.description.is_empty() {
                content.push_str(&format!("description={}\n", snippet.description));
            }
        }

        // Re-parse
        let reparsed = SnippetStore::parse(&content);
        assert_eq!(reparsed.snippets.len(), 2);
        assert_eq!(reparsed.snippets[0].name, "check-disk");
        assert_eq!(reparsed.snippets[0].command, "df -h");
        assert_eq!(reparsed.snippets[0].description, "Check disk usage");
        assert_eq!(reparsed.snippets[1].name, "uptime");
        assert_eq!(reparsed.snippets[1].command, "uptime");
        assert!(reparsed.snippets[1].description.is_empty());
    }

    #[test]
    fn test_save_to_temp_file() {
        let dir = std::env::temp_dir().join(format!("purple_snippet_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("snippets");

        let mut store = SnippetStore {
            path_override: Some(path.clone()),
            ..Default::default()
        };
        store.set(Snippet {
            name: "test".to_string(),
            command: "echo hello".to_string(),
            description: "Test snippet".to_string(),
        });
        store.save().unwrap();

        // Read back
        let content = std::fs::read_to_string(&path).unwrap();
        let reloaded = SnippetStore::parse(&content);
        assert_eq!(reloaded.snippets.len(), 1);
        assert_eq!(reloaded.snippets[0].name, "test");
        assert_eq!(reloaded.snippets[0].command, "echo hello");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    // =========================================================================
    // Edge cases
    // =========================================================================

    #[test]
    fn test_set_multiple_then_remove_all() {
        let mut store = SnippetStore::default();
        for name in ["a", "b", "c"] {
            store.set(Snippet {
                name: name.to_string(),
                command: "cmd".to_string(),
                description: String::new(),
            });
        }
        assert_eq!(store.snippets.len(), 3);
        store.remove("a");
        store.remove("b");
        store.remove("c");
        assert!(store.snippets.is_empty());
    }

    #[test]
    fn test_snippet_with_complex_command() {
        let content = "[complex]\ncommand=for i in $(seq 1 5); do echo $i; done\n";
        let store = SnippetStore::parse(content);
        assert_eq!(
            store.snippets[0].command,
            "for i in $(seq 1 5); do echo $i; done"
        );
    }

    #[test]
    fn test_snippet_command_with_pipes_and_redirects() {
        let content = "[logs]\ncommand=tail -100 /var/log/syslog | grep error | head -20\n";
        let store = SnippetStore::parse(content);
        assert_eq!(
            store.snippets[0].command,
            "tail -100 /var/log/syslog | grep error | head -20"
        );
    }

    #[test]
    fn test_description_optional() {
        let content = "[check]\ncommand=ls\n";
        let store = SnippetStore::parse(content);
        assert!(store.snippets[0].description.is_empty());
    }

    #[test]
    fn test_description_with_equals() {
        let content = "[env]\ncommand=env\ndescription=Check HOME= and PATH= vars\n";
        let store = SnippetStore::parse(content);
        assert_eq!(store.snippets[0].description, "Check HOME= and PATH= vars");
    }

    #[test]
    fn test_name_with_equals_roundtrip() {
        let mut store = SnippetStore::default();
        store.set(Snippet {
            name: "check=disk".to_string(),
            command: "df -h".to_string(),
            description: String::new(),
        });

        let mut content = String::new();
        for (i, snippet) in store.snippets.iter().enumerate() {
            if i > 0 {
                content.push('\n');
            }
            content.push_str(&format!("[{}]\n", snippet.name));
            content.push_str(&format!("command={}\n", snippet.command));
            if !snippet.description.is_empty() {
                content.push_str(&format!("description={}\n", snippet.description));
            }
        }

        let reparsed = SnippetStore::parse(&content);
        assert_eq!(reparsed.snippets.len(), 1);
        assert_eq!(reparsed.snippets[0].name, "check=disk");
    }

    #[test]
    fn test_validate_name_with_equals() {
        assert!(validate_name("check=disk").is_ok());
    }

    #[test]
    fn test_parse_only_comments_and_blanks() {
        let content = "# comment\n\n# another\n";
        let store = SnippetStore::parse(content);
        assert!(store.snippets.is_empty());
    }

    #[test]
    fn test_parse_section_without_close_bracket() {
        let content = "[incomplete\ncommand=ls\n";
        let store = SnippetStore::parse(content);
        assert!(store.snippets.is_empty());
    }

    #[test]
    fn test_parse_trailing_content_after_last_section() {
        let content = "[check]\ncommand=ls\n";
        let store = SnippetStore::parse(content);
        assert_eq!(store.snippets.len(), 1);
        assert_eq!(store.snippets[0].command, "ls");
    }

    #[test]
    fn test_set_overwrite_preserves_order() {
        let mut store = SnippetStore::default();
        store.set(Snippet { name: "a".into(), command: "1".into(), description: String::new() });
        store.set(Snippet { name: "b".into(), command: "2".into(), description: String::new() });
        store.set(Snippet { name: "c".into(), command: "3".into(), description: String::new() });
        store.set(Snippet { name: "b".into(), command: "updated".into(), description: String::new() });
        assert_eq!(store.snippets.len(), 3);
        assert_eq!(store.snippets[0].name, "a");
        assert_eq!(store.snippets[1].name, "b");
        assert_eq!(store.snippets[1].command, "updated");
        assert_eq!(store.snippets[2].name, "c");
    }

    #[test]
    fn test_validate_command_with_tab() {
        assert!(validate_command("echo\thello").is_ok());
    }

    #[test]
    fn test_validate_command_with_newline() {
        assert!(validate_command("echo\nhello").is_err());
    }

    #[test]
    fn test_validate_name_newline() {
        assert!(validate_name("check\ndisk").is_err());
    }

}
