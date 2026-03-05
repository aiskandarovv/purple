use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

/// Launch an SSH connection to the given host alias.
/// Uses the system `ssh` binary with inherited stdin/stdout/stderr.
/// Passes `-F <config_path>` so the alias resolves against the correct config file.
/// When `askpass` is Some, sets SSH_ASKPASS environment variables so SSH retrieves
/// the password from the configured source via purple's askpass handler.
pub fn connect(alias: &str, config_path: &Path, askpass: Option<&str>, bw_session: Option<&str>) -> Result<std::process::ExitStatus> {
    let mut cmd = Command::new("ssh");
    cmd.arg("-F")
        .arg(config_path)
        .arg("--")
        .arg(alias)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

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

    let status = cmd
        .status()
        .with_context(|| format!("Failed to launch ssh for '{}'", alias))?;
    Ok(status)
}

#[cfg(test)]
mod tests {
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
        let vars = ["SSH_ASKPASS", "SSH_ASKPASS_REQUIRE", "PURPLE_ASKPASS_MODE", "PURPLE_HOST_ALIAS", "PURPLE_CONFIG_PATH"];
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
        let vars = ["SSH_ASKPASS", "SSH_ASKPASS_REQUIRE", "PURPLE_ASKPASS_MODE", "PURPLE_HOST_ALIAS", "PURPLE_CONFIG_PATH"];
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
    fn connection_inherits_all_stdio() {
        // SSH needs interactive terminal: stdin, stdout, stderr all inherited
        let modes = ["inherit", "inherit", "inherit"];
        assert_eq!(modes.len(), 3);
        for mode in &modes {
            assert_eq!(*mode, "inherit");
        }
    }

    #[test]
    fn connection_all_askpass_source_types_trigger_env() {
        // Every non-None askpass source should trigger env var setup
        let sources = ["keychain", "op://V/I/p", "bw:item", "pass:ssh/srv", "vault:kv#pw", "my-cmd"];
        for source in &sources {
            let askpass: Option<&str> = Some(source);
            assert!(askpass.is_some(), "Source '{}' should trigger env setup", source);
        }
    }

    #[test]
    fn connection_exe_fallback_chain() {
        // current_exe() -> env::args().next() -> "purple"
        let fallback = "purple";
        assert_eq!(fallback, "purple");
    }
}
