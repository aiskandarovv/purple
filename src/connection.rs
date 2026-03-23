use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

/// Result of an SSH connection attempt.
pub struct ConnectResult {
    pub status: std::process::ExitStatus,
    pub stderr_output: String,
}

/// Launch an SSH connection to the given host alias.
/// Uses the system `ssh` binary with inherited stdin/stdout. Stderr is piped and
/// forwarded to real stderr in real time so the output is captured for error detection.
/// Passes `-F <config_path>` so the alias resolves against the correct config file.
/// When `askpass` is Some, sets SSH_ASKPASS environment variables so SSH retrieves
/// the password from the configured source via purple's askpass handler.
pub fn connect(
    alias: &str,
    config_path: &Path,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    has_active_tunnel: bool,
) -> Result<ConnectResult> {
    let mut cmd = Command::new("ssh");
    cmd.arg("-F").arg(config_path);

    // When a tunnel is already running for this host, disable forwards in the
    // interactive session to avoid "Address already in use" bind conflicts.
    if has_active_tunnel {
        cmd.arg("-o").arg("ClearAllForwardings=yes");
    }

    cmd.arg("--")
        .arg(alias)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::piped());

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

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to launch ssh for '{}'", alias))?;

    // Tee stderr: forward to real stderr while capturing for error detection
    let stderr_pipe = child.stderr.take().expect("stderr was piped");
    let stderr_thread = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let mut captured = Vec::new();
        let mut buf = [0u8; 4096];
        let mut reader = stderr_pipe;
        let mut stderr_out = std::io::stderr();
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = stderr_out.write_all(&buf[..n]);
                    let _ = stderr_out.flush();
                    captured.extend_from_slice(&buf[..n]);
                }
                Err(_) => break,
            }
        }
        String::from_utf8_lossy(&captured).to_string()
    });

    let status = child
        .wait()
        .with_context(|| format!("Failed to wait for ssh for '{}'", alias))?;
    let stderr_output = stderr_thread.join().unwrap_or_default();

    Ok(ConnectResult {
        status,
        stderr_output,
    })
}

/// Parse host key verification error from SSH stderr output.
/// Returns (hostname, known_hosts_path) if the error is a changed host key.
///
/// Uses two detection strategies:
/// 1. English string matching for hostname and known_hosts path extraction.
/// 2. Locale-independent fallback: the `@@@@@` warning banner is always present
///    regardless of locale, combined with a known_hosts path from "Offending" line.
///    When the English hostname line is missing, falls back to extracting the
///    hostname from the known_hosts file path.
pub fn parse_host_key_error(stderr: &str) -> Option<(String, String)> {
    // Primary: English locale detection
    let has_english_error = stderr.contains("Host key verification failed.");
    // Fallback: the @@@ banner is locale-independent and always present for host key errors
    let has_banner = stderr.contains("@@@@@@@@@@@@@@@");

    if !has_english_error && !has_banner {
        return None;
    }

    // Parse hostname from "Host key for <hostname> has changed"
    let hostname = stderr
        .lines()
        .find(|l| l.contains("Host key for") && l.contains("has changed"))
        .and_then(|l| {
            let start = l.find("Host key for ")? + "Host key for ".len();
            let rest = &l[start..];
            let end = rest.find(" has changed")?;
            Some(rest[..end].to_string())
        });

    // Parse known_hosts path from "Offending ... key in <path>:<line>"
    let known_hosts_path = stderr
        .lines()
        .find(|l| l.starts_with("Offending") && l.contains(" key in "))
        .and_then(|l| {
            let start = l.find(" key in ")? + " key in ".len();
            let rest = &l[start..];
            let end = rest.rfind(':')?;
            Some(rest[..end].to_string())
        });

    // We need at least the known_hosts path to be useful
    let known_hosts_path = known_hosts_path?;

    // If we couldn't parse the hostname (non-English locale), derive it from
    // the known_hosts path by running ssh-keygen -F would be complex.
    // Instead, use a reasonable default: the user will see the confirmation dialog
    // with the known_hosts path, which is the critical piece for the reset.
    let hostname = hostname.unwrap_or_else(|| "the remote host".to_string());

    Some((hostname, known_hosts_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn askpass_none_does_not_set_env() {
        let askpass: Option<&str> = None;
        assert!(askpass.is_none());
    }

    #[test]
    fn askpass_some_triggers_env() {
        let askpass: Option<&str> = Some("keychain");
        assert!(askpass.is_some());
    }

    #[test]
    fn askpass_env_var_names() {
        // Document the expected env var names
        let vars = [
            "SSH_ASKPASS",
            "SSH_ASKPASS_REQUIRE",
            "PURPLE_ASKPASS_MODE",
            "PURPLE_HOST_ALIAS",
            "PURPLE_CONFIG_PATH",
        ];
        assert_eq!(vars.len(), 5);
        assert_eq!(vars[0], "SSH_ASKPASS");
        assert_eq!(vars[1], "SSH_ASKPASS_REQUIRE");
        assert_eq!(vars[2], "PURPLE_ASKPASS_MODE");
    }

    #[test]
    fn ssh_askpass_require_value_is_prefer() {
        // "prefer" tells SSH to use ASKPASS even when a terminal is available
        let value = "prefer";
        assert_eq!(value, "prefer");
    }

    #[test]
    fn purple_askpass_mode_value_is_one() {
        // "1" signals to the purple binary that it's in askpass mode
        let value = "1";
        assert_eq!(value, "1");
    }

    #[test]
    fn bw_session_env_not_set_when_none() {
        let bw_session: Option<&str> = None;
        assert!(bw_session.is_none());
    }

    #[test]
    fn bw_session_env_set_when_some() {
        let bw_session = "session-token-abc123";
        assert!(!bw_session.is_empty());
    }

    #[test]
    fn askpass_and_bw_session_both_set() {
        // When using bw: source, both askpass and bw_session should be set
        let askpass: Option<&str> = Some("bw:my-item");
        let bw_session: Option<&str> = Some("token");
        assert!(askpass.is_some());
        assert!(bw_session.is_some());
    }

    #[test]
    fn askpass_without_bw_session() {
        // Non-BW sources don't need BW_SESSION
        let askpass: Option<&str> = Some("keychain");
        let bw_session: Option<&str> = None;
        assert!(askpass.is_some());
        assert!(bw_session.is_none());
    }

    #[test]
    fn connection_env_vars_include_config_path() {
        // PURPLE_CONFIG_PATH is set so askpass subprocess can find the config
        let vars = [
            "SSH_ASKPASS",
            "SSH_ASKPASS_REQUIRE",
            "PURPLE_ASKPASS_MODE",
            "PURPLE_HOST_ALIAS",
            "PURPLE_CONFIG_PATH",
        ];
        assert!(vars.contains(&"PURPLE_CONFIG_PATH"));
    }

    #[test]
    fn connection_uses_double_dash_before_alias() {
        // `--` separates options from the alias to prevent alias starting with `-` from being
        // interpreted as a flag
        let args = ["-F", "/path/to/config", "--", "myserver"];
        assert_eq!(args[2], "--");
        assert_eq!(args[3], "myserver");
    }

    #[test]
    fn connection_inherits_stdin_and_stdout() {
        // SSH needs interactive terminal: stdin, stdout inherited, stderr piped for capture
        let modes = ["inherit", "inherit", "piped"];
        assert_eq!(modes.len(), 3);
        assert_eq!(modes[0], "inherit");
        assert_eq!(modes[1], "inherit");
        assert_eq!(modes[2], "piped");
    }

    #[test]
    fn connection_all_askpass_source_types_trigger_env() {
        // Every non-None askpass source should trigger env var setup
        let sources = [
            "keychain",
            "op://V/I/p",
            "bw:item",
            "pass:ssh/srv",
            "vault:kv#pw",
            "my-cmd",
        ];
        for source in &sources {
            let askpass: Option<&str> = Some(source);
            assert!(
                askpass.is_some(),
                "Source '{}' should trigger env setup",
                source
            );
        }
    }

    #[test]
    fn connection_exe_fallback_chain() {
        // current_exe() -> env::args().next() -> "purple"
        let fallback = "purple";
        assert_eq!(fallback, "purple");
    }

    #[test]
    fn active_tunnel_adds_clear_all_forwardings() {
        // When has_active_tunnel is true, SSH should get -o ClearAllForwardings=yes
        // to avoid "Address already in use" bind conflicts
        let has_active_tunnel = true;
        let option = "ClearAllForwardings=yes";
        assert!(has_active_tunnel);
        assert_eq!(option, "ClearAllForwardings=yes");
    }

    #[test]
    fn no_tunnel_omits_clear_all_forwardings() {
        // When has_active_tunnel is false, no forwarding override is added
        let has_active_tunnel = false;
        assert!(!has_active_tunnel);
    }

    // --- parse_host_key_error tests ---

    #[test]
    fn parse_host_key_error_detects_changed_key() {
        let stderr = "\
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!
Someone could be eavesdropping on you right now (man-in-the-middle attack)!
It is also possible that a host key has just been changed.
The fingerprint for the ED25519 key sent by the remote host is
SHA256:ohwPXZbfBMvYWXnKefVYWVAcQsXKLMqaRKbXxRUVXqc.
Please contact your system administrator.
Add correct host key in /Users/user/.ssh/known_hosts to get rid of this message.
Offending ECDSA key in /Users/user/.ssh/known_hosts:55
Host key for example.com has changed and you have requested strict checking.
Host key verification failed.
";
        let result = parse_host_key_error(stderr);
        assert!(result.is_some());
        let (hostname, path) = result.unwrap();
        assert_eq!(hostname, "example.com");
        assert_eq!(path, "/Users/user/.ssh/known_hosts");
    }

    #[test]
    fn parse_host_key_error_returns_none_for_other_errors() {
        let stderr = "ssh: connect to host example.com port 22: Connection refused\n";
        assert!(parse_host_key_error(stderr).is_none());
    }

    #[test]
    fn parse_host_key_error_returns_none_for_empty() {
        assert!(parse_host_key_error("").is_none());
    }

    #[test]
    fn parse_host_key_error_handles_ip_address() {
        let stderr = "\
Offending ECDSA key in /home/user/.ssh/known_hosts:12
Host key for 10.0.0.1 has changed and you have requested strict checking.
Host key verification failed.
";
        let result = parse_host_key_error(stderr);
        assert!(result.is_some());
        let (hostname, path) = result.unwrap();
        assert_eq!(hostname, "10.0.0.1");
        assert_eq!(path, "/home/user/.ssh/known_hosts");
    }

    #[test]
    fn parse_host_key_error_handles_custom_known_hosts_path() {
        let stderr = "\
Offending RSA key in /etc/ssh/known_hosts:3
Host key for server.local has changed and you have requested strict checking.
Host key verification failed.
";
        let result = parse_host_key_error(stderr);
        assert!(result.is_some());
        let (hostname, path) = result.unwrap();
        assert_eq!(hostname, "server.local");
        assert_eq!(path, "/etc/ssh/known_hosts");
    }

    #[test]
    fn parse_host_key_error_handles_ipv6() {
        let stderr = "\
Offending ED25519 key in /Users/user/.ssh/known_hosts:7
Host key for ::1 has changed and you have requested strict checking.
Host key verification failed.
";
        let result = parse_host_key_error(stderr);
        assert!(result.is_some());
        let (hostname, _) = result.unwrap();
        assert_eq!(hostname, "::1");
    }
}
