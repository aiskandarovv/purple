use std::collections::HashMap;
use std::io::Read as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;

use base64::Engine as _;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error, strip_cidr};

pub struct Tailscale;

// =========================================================================
// CLI structs (`tailscale status --json` uses PascalCase)
// =========================================================================

#[derive(Deserialize)]
struct CliStatus {
    #[serde(rename = "Peer")]
    #[serde(default)]
    peer: HashMap<String, CliPeer>,
}

#[derive(Deserialize)]
struct CliPeer {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "HostName")]
    host_name: String,
    #[serde(rename = "TailscaleIPs")]
    #[serde(default)]
    tailscale_ips: Vec<String>,
    #[serde(rename = "OS")]
    #[serde(default)]
    os: String,
    #[serde(rename = "Online")]
    #[serde(default)]
    online: Option<bool>,
    #[serde(rename = "Tags")]
    #[serde(default)]
    tags: Vec<String>,
}

// =========================================================================
// API structs (camelCase)
// =========================================================================

#[derive(Deserialize)]
struct ApiResponse {
    devices: Vec<ApiDevice>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiDevice {
    node_id: String,
    hostname: String,
    name: String,
    #[serde(default)]
    addresses: Vec<String>,
    #[serde(default)]
    os: String,
    #[serde(default = "default_authorized")]
    authorized: bool,
    #[serde(default)]
    connected_to_control: bool,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    tags: Vec<String>,
}

/// Default for authorized field: true (most API devices are authorized;
/// missing field should not silently filter out devices).
fn default_authorized() -> bool {
    true
}

/// Deserialize a Vec that may be null in JSON (Tailscale API can return
/// `"tags": null` instead of omitting the field or using an empty array).
fn deserialize_null_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<Vec<String>>::deserialize(deserializer).map(|v| v.unwrap_or_default())
}

// =========================================================================
// Helpers
// =========================================================================

/// Select the best IP from a list of Tailscale addresses.
/// Prefers IPv4 (100.x) over IPv6 (fd7a:). Strips CIDR suffixes.
fn select_ip(ips: &[String]) -> Option<String> {
    // Prefer IPv4
    if let Some(ip) = ips.iter().find(|ip| ip.starts_with("100.")) {
        return Some(strip_cidr(ip).to_string());
    }
    // Fall back to first available
    ips.first().map(|ip| strip_cidr(ip).to_string())
}

/// Strip the `tag:` prefix from Tailscale tags.
fn strip_tag_prefix(tag: &str) -> String {
    tag.strip_prefix("tag:").unwrap_or(tag).to_string()
}

/// Find the tailscale binary. Checks PATH first, then macOS app bundle.
fn find_tailscale_binary() -> Result<PathBuf, ProviderError> {
    // Check PATH via shell builtin (more portable than `which`)
    let found = std::process::Command::new("sh")
        .args(["-c", "command -v tailscale"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    if let Ok(output) = found {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    // macOS app bundle fallback (the CLI binary inside the GUI app)
    let macos_path = PathBuf::from("/Applications/Tailscale.app/Contents/MacOS/Tailscale");
    if macos_path.exists() {
        return Ok(macos_path);
    }

    Err(ProviderError::Execute(
        "Tailscale CLI not found. Install from https://tailscale.com/download or add it to PATH."
            .to_string(),
    ))
}

// =========================================================================
// Provider impl
// =========================================================================

impl Provider for Tailscale {
    fn name(&self) -> &str {
        "tailscale"
    }

    fn short_label(&self) -> &str {
        "ts"
    }

    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        if token.is_empty() {
            self.fetch_from_cli(cancel)
        } else {
            self.fetch_from_api(token, cancel)
        }
    }
}

impl Tailscale {
    fn fetch_from_cli(&self, cancel: &AtomicBool) -> Result<Vec<ProviderHost>, ProviderError> {
        let binary = find_tailscale_binary()?;

        let mut child = std::process::Command::new(&binary)
            .args(["status", "--json"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ProviderError::Execute(format!("Failed to run tailscale: {}", e)))?;

        // Read stdout in a background thread to avoid pipe deadlock.
        // If the child produces more output than the OS pipe buffer (~64KB),
        // it blocks until the parent reads. We must read concurrently.
        let stdout_pipe = child.stdout.take();
        let stdout_handle = std::thread::spawn(move || -> Result<String, String> {
            match stdout_pipe {
                Some(mut pipe) => {
                    let mut buf = String::new();
                    pipe.read_to_string(&mut buf)
                        .map_err(|e| format!("Failed to read tailscale stdout: {}", e))?;
                    Ok(buf)
                }
                None => Err("No stdout from tailscale".to_string()),
            }
        });

        let start = Instant::now();
        let timeout = Duration::from_secs(30);

        let exit_err: Option<ProviderError> = loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                break Some(ProviderError::Cancelled);
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        let stderr = child
                            .stderr
                            .take()
                            .map(|mut s| {
                                let mut buf = String::new();
                                s.read_to_string(&mut buf).ok();
                                buf
                            })
                            .unwrap_or_default();
                        break Some(ProviderError::Execute(format!(
                            "tailscale status failed: {}",
                            stderr.trim()
                        )));
                    }
                    break None;
                }
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        break Some(ProviderError::Execute(
                            "Tailscale CLI timed out after 30s.".to_string(),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Some(ProviderError::Execute(format!(
                        "Failed to wait for tailscale: {}",
                        e
                    )));
                }
            }
        };

        // Always join the stdout reader thread to prevent thread leaks.
        // All error paths above call kill()+wait() so the pipe is closed
        // and the thread will receive EOF promptly.
        let stdout_result = stdout_handle.join();

        if let Some(err) = exit_err {
            return Err(err);
        }

        let stdout_data = stdout_result
            .map_err(|_| ProviderError::Parse("stdout reader thread panicked".to_string()))?
            .map_err(ProviderError::Parse)?;

        let status: CliStatus = serde_json::from_str(&stdout_data).map_err(|e| {
            ProviderError::Parse(format!("Failed to parse tailscale output: {}", e))
        })?;

        Self::hosts_from_cli(status)
    }

    fn hosts_from_cli(status: CliStatus) -> Result<Vec<ProviderHost>, ProviderError> {
        let mut hosts = Vec::new();

        // Sort by peer key for deterministic output (HashMap iteration is random)
        let mut peers: Vec<_> = status.peer.into_iter().collect();
        peers.sort_by(|a, b| a.0.cmp(&b.0));

        for (_key, peer) in peers {
            let ip = match select_ip(&peer.tailscale_ips) {
                Some(ip) => ip,
                None => continue,
            };

            let tags: Vec<String> = peer.tags.iter().map(|t| strip_tag_prefix(t)).collect();

            let status_str = match peer.online {
                Some(true) => "online",
                Some(false) => "offline",
                None => "unknown",
            };

            let mut metadata = Vec::new();
            if !peer.os.is_empty() {
                metadata.push(("os".to_string(), peer.os.clone()));
            }
            metadata.push(("status".to_string(), status_str.to_string()));

            hosts.push(ProviderHost {
                server_id: peer.id,
                name: peer.host_name,
                ip,
                tags,
                metadata,
            });
        }

        Ok(hosts)
    }

    fn fetch_from_api(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        // Validate token prefix
        if token.starts_with("tskey-auth-") {
            return Err(ProviderError::Execute(
                "This is a device auth key, not an API key. Use a key starting with tskey-api-."
                    .to_string(),
            ));
        }

        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }

        let agent = super::http_agent();

        // Tailscale API keys (tskey-api-*) use HTTP Basic auth (key as username,
        // empty password). OAuth access tokens use Bearer auth.
        let auth_header = if token.starts_with("tskey-") {
            let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{}:", token));
            format!("Basic {}", encoded)
        } else {
            format!("Bearer {}", token)
        };

        let resp: ApiResponse = agent
            .get("https://api.tailscale.com/api/v2/tailnet/-/devices?fields=all")
            .set("Authorization", &auth_header)
            .call()
            .map_err(map_ureq_error)?
            .into_json()
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        Self::hosts_from_api(resp)
    }

    fn hosts_from_api(resp: ApiResponse) -> Result<Vec<ProviderHost>, ProviderError> {
        let mut hosts = Vec::new();

        for device in resp.devices {
            // Skip unauthorized devices
            if !device.authorized {
                continue;
            }

            let ip = match select_ip(&device.addresses) {
                Some(ip) => ip,
                None => continue,
            };

            // Use hostname, or strip FQDN from name if hostname is empty
            let name = if device.hostname.is_empty() {
                device
                    .name
                    .split('.')
                    .next()
                    .unwrap_or(&device.name)
                    .to_string()
            } else {
                device.hostname.clone()
            };

            let tags: Vec<String> = device.tags.iter().map(|t| strip_tag_prefix(t)).collect();

            let mut metadata = Vec::new();
            if !device.os.is_empty() {
                metadata.push(("os".to_string(), device.os.clone()));
            }
            let status_str = if device.connected_to_control {
                "online"
            } else {
                "offline"
            };
            metadata.push(("status".to_string(), status_str.to_string()));

            hosts.push(ProviderHost {
                server_id: device.node_id,
                name,
                ip,
                tags,
                metadata,
            });
        }

        Ok(hosts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // CLI parsing
    // =========================================================================

    #[test]
    fn test_parse_cli_status_basic() {
        let json = r#"{
            "Peer": {
                "abc123": {
                    "ID": "n12345",
                    "HostName": "web-server",
                    "TailscaleIPs": ["100.64.0.1", "fd7a:115c:a1e0::1"],
                    "OS": "linux",
                    "Online": true,
                    "Tags": ["tag:server"]
                }
            }
        }"#;
        let status: CliStatus = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_cli(status).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].server_id, "n12345");
        assert_eq!(hosts[0].name, "web-server");
        assert_eq!(hosts[0].ip, "100.64.0.1");
        assert_eq!(hosts[0].tags, vec!["server"]);
        assert!(
            hosts[0]
                .metadata
                .iter()
                .any(|(k, v)| k == "os" && v == "linux")
        );
        assert!(
            hosts[0]
                .metadata
                .iter()
                .any(|(k, v)| k == "status" && v == "online")
        );
    }

    #[test]
    fn test_parse_cli_status_no_peers() {
        let json = r#"{"Peer": {}}"#;
        let status: CliStatus = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_cli(status).unwrap();
        assert!(hosts.is_empty());
    }

    #[test]
    fn test_parse_cli_status_null_peer() {
        let json = r#"{}"#;
        let status: CliStatus = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_cli(status).unwrap();
        assert!(hosts.is_empty());
    }

    #[test]
    fn test_parse_cli_peer_no_ips_skipped() {
        let json = r#"{
            "Peer": {
                "abc": {
                    "ID": "n1",
                    "HostName": "no-ip",
                    "TailscaleIPs": [],
                    "OS": "linux",
                    "Online": true,
                    "Tags": []
                }
            }
        }"#;
        let status: CliStatus = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_cli(status).unwrap();
        assert!(hosts.is_empty());
    }

    #[test]
    fn test_parse_cli_peer_ipv4_preferred() {
        let json = r#"{
            "Peer": {
                "abc": {
                    "ID": "n1",
                    "HostName": "dual",
                    "TailscaleIPs": ["fd7a:115c:a1e0::1", "100.64.0.5"],
                    "OS": "",
                    "Online": true,
                    "Tags": []
                }
            }
        }"#;
        let status: CliStatus = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_cli(status).unwrap();
        assert_eq!(hosts[0].ip, "100.64.0.5");
    }

    #[test]
    fn test_parse_cli_peer_ipv6_fallback() {
        let json = r#"{
            "Peer": {
                "abc": {
                    "ID": "n1",
                    "HostName": "v6only",
                    "TailscaleIPs": ["fd7a:115c:a1e0::1"],
                    "OS": "",
                    "Online": true,
                    "Tags": []
                }
            }
        }"#;
        let status: CliStatus = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_cli(status).unwrap();
        assert_eq!(hosts[0].ip, "fd7a:115c:a1e0::1");
    }

    #[test]
    fn test_parse_cli_tags_stripped() {
        let json = r#"{
            "Peer": {
                "abc": {
                    "ID": "n1",
                    "HostName": "tagged",
                    "TailscaleIPs": ["100.64.0.1"],
                    "OS": "",
                    "Online": true,
                    "Tags": ["tag:server", "tag:prod", "notag"]
                }
            }
        }"#;
        let status: CliStatus = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_cli(status).unwrap();
        assert_eq!(hosts[0].tags, vec!["server", "prod", "notag"]);
    }

    #[test]
    fn test_parse_cli_online_null() {
        let json = r#"{
            "Peer": {
                "abc": {
                    "ID": "n1",
                    "HostName": "unknown-state",
                    "TailscaleIPs": ["100.64.0.1"],
                    "OS": "",
                    "Online": null,
                    "Tags": []
                }
            }
        }"#;
        let status: CliStatus = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_cli(status).unwrap();
        assert!(
            hosts[0]
                .metadata
                .iter()
                .any(|(k, v)| k == "status" && v == "unknown")
        );
    }

    #[test]
    fn test_parse_cli_extra_fields_ignored() {
        let json = r#"{
            "Version": "1.50.0",
            "Self": {"ID": "self1", "HostName": "my-machine"},
            "MagicDNSSuffix": "tailnet.ts.net",
            "Peer": {
                "abc": {
                    "ID": "n1",
                    "HostName": "remote",
                    "TailscaleIPs": ["100.64.0.1"],
                    "OS": "linux",
                    "Online": true,
                    "Tags": [],
                    "ExtraField": "ignored",
                    "RxBytes": 12345
                }
            }
        }"#;
        let status: CliStatus = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_cli(status).unwrap();
        assert_eq!(hosts.len(), 1);
    }

    // =========================================================================
    // API parsing
    // =========================================================================

    #[test]
    fn test_parse_api_response_basic() {
        let json = r#"{
            "devices": [
                {
                    "nodeId": "nDEV1",
                    "hostname": "api-server",
                    "name": "api-server.tailnet.ts.net",
                    "addresses": ["100.64.0.10", "fd7a:115c:a1e0::a"],
                    "os": "linux",
                    "authorized": true,
                    "connectedToControl": true,
                    "tags": ["tag:web"]
                }
            ]
        }"#;
        let resp: ApiResponse = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_api(resp).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].server_id, "nDEV1");
        assert_eq!(hosts[0].name, "api-server");
        assert_eq!(hosts[0].ip, "100.64.0.10");
        assert_eq!(hosts[0].tags, vec!["web"]);
        assert!(
            hosts[0]
                .metadata
                .iter()
                .any(|(k, v)| k == "os" && v == "linux")
        );
        assert!(
            hosts[0]
                .metadata
                .iter()
                .any(|(k, v)| k == "status" && v == "online")
        );
    }

    #[test]
    fn test_parse_api_connected_to_control_false() {
        let json = r#"{
            "devices": [
                {
                    "nodeId": "n1",
                    "hostname": "offline-dev",
                    "name": "offline-dev.ts.net",
                    "addresses": ["100.64.0.1"],
                    "os": "linux",
                    "authorized": true,
                    "connectedToControl": false,
                    "tags": []
                }
            ]
        }"#;
        let resp: ApiResponse = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_api(resp).unwrap();
        assert_eq!(hosts.len(), 1);
        assert!(
            hosts[0]
                .metadata
                .iter()
                .any(|(k, v)| k == "status" && v == "offline")
        );
    }

    #[test]
    fn test_parse_api_extra_fields_ignored() {
        let json = r#"{
            "devices": [
                {
                    "nodeId": "n1",
                    "hostname": "full",
                    "name": "full.ts.net",
                    "addresses": ["100.64.0.1"],
                    "os": "linux",
                    "authorized": true,
                    "connectedToControl": true,
                    "tags": [],
                    "lastSeen": "2025-01-01T00:00:00Z",
                    "clientVersion": "1.50.0",
                    "updateAvailable": false,
                    "machineKey": "mkey:abc123",
                    "nodeKey": "nodekey:xyz789",
                    "user": "user@example.com",
                    "keyExpiryDisabled": true,
                    "isExternal": false
                }
            ]
        }"#;
        let resp: ApiResponse = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_api(resp).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].name, "full");
    }

    #[test]
    fn test_parse_api_unauthorized_skipped() {
        let json = r#"{
            "devices": [
                {
                    "nodeId": "n1",
                    "hostname": "authorized",
                    "name": "authorized.ts.net",
                    "addresses": ["100.64.0.1"],
                    "os": "linux",
                    "authorized": true,
                    "tags": []
                },
                {
                    "nodeId": "n2",
                    "hostname": "unauthorized",
                    "name": "unauthorized.ts.net",
                    "addresses": ["100.64.0.2"],
                    "os": "linux",
                    "authorized": false,
                    "tags": []
                }
            ]
        }"#;
        let resp: ApiResponse = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_api(resp).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].name, "authorized");
    }

    #[test]
    fn test_parse_api_tags_null() {
        let json = r#"{
            "devices": [
                {
                    "nodeId": "n1",
                    "hostname": "notags",
                    "name": "notags.ts.net",
                    "addresses": ["100.64.0.1"],
                    "os": "linux",
                    "authorized": true
                }
            ]
        }"#;
        let resp: ApiResponse = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_api(resp).unwrap();
        assert!(hosts[0].tags.is_empty());
    }

    #[test]
    fn test_parse_api_tags_explicit_null() {
        // Tailscale API can return "tags": null (not just missing)
        let json = r#"{
            "devices": [
                {
                    "nodeId": "n1",
                    "hostname": "nulltags",
                    "name": "nulltags.ts.net",
                    "addresses": ["100.64.0.1"],
                    "os": "linux",
                    "authorized": true,
                    "tags": null
                }
            ]
        }"#;
        let resp: ApiResponse = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_api(resp).unwrap();
        assert_eq!(hosts.len(), 1);
        assert!(hosts[0].tags.is_empty());
    }

    #[test]
    fn test_parse_api_hostname_from_name() {
        let json = r#"{
            "devices": [
                {
                    "nodeId": "n1",
                    "hostname": "",
                    "name": "my-server.tailnet.ts.net",
                    "addresses": ["100.64.0.1"],
                    "os": "linux",
                    "authorized": true,
                    "tags": []
                }
            ]
        }"#;
        let resp: ApiResponse = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_api(resp).unwrap();
        assert_eq!(hosts[0].name, "my-server");
    }

    #[test]
    fn test_parse_cli_multiple_peers() {
        // Keys intentionally in reverse alphabetical order to verify sort
        let json = r#"{
            "Peer": {
                "zzz": {
                    "ID": "n1",
                    "HostName": "server-z",
                    "TailscaleIPs": ["100.64.0.1"],
                    "OS": "linux",
                    "Online": true,
                    "Tags": []
                },
                "aaa": {
                    "ID": "n2",
                    "HostName": "server-a",
                    "TailscaleIPs": ["100.64.0.2"],
                    "OS": "darwin",
                    "Online": false,
                    "Tags": ["tag:dev"]
                }
            }
        }"#;
        let status: CliStatus = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_cli(status).unwrap();
        assert_eq!(hosts.len(), 2);
        // Sorted by peer key: "aaa" before "zzz"
        assert_eq!(hosts[0].name, "server-a");
        assert_eq!(hosts[1].name, "server-z");
    }

    #[test]
    fn test_parse_cli_offline_peer_included() {
        let json = r#"{
            "Peer": {
                "abc": {
                    "ID": "n1",
                    "HostName": "offline-host",
                    "TailscaleIPs": ["100.64.0.1"],
                    "OS": "linux",
                    "Online": false,
                    "Tags": []
                }
            }
        }"#;
        let status: CliStatus = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_cli(status).unwrap();
        assert_eq!(hosts.len(), 1);
        assert!(
            hosts[0]
                .metadata
                .iter()
                .any(|(k, v)| k == "status" && v == "offline")
        );
    }

    #[test]
    fn test_parse_api_device_no_addresses_skipped() {
        let json = r#"{
            "devices": [
                {
                    "nodeId": "n1",
                    "hostname": "no-addr",
                    "name": "no-addr.ts.net",
                    "addresses": [],
                    "os": "linux",
                    "authorized": true,
                    "tags": []
                }
            ]
        }"#;
        let resp: ApiResponse = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_api(resp).unwrap();
        assert!(hosts.is_empty());
    }

    #[test]
    fn test_parse_api_missing_authorized_defaults_true() {
        // Devices without "authorized" field should NOT be silently skipped.
        let json = r#"{
            "devices": [
                {
                    "nodeId": "n1",
                    "hostname": "implicit-auth",
                    "name": "implicit-auth.ts.net",
                    "addresses": ["100.64.0.1"],
                    "os": "linux",
                    "tags": []
                }
            ]
        }"#;
        let resp: ApiResponse = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_api(resp).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].name, "implicit-auth");
    }

    #[test]
    fn test_parse_api_multiple_devices() {
        let json = r#"{
            "devices": [
                {
                    "nodeId": "n1",
                    "hostname": "web",
                    "name": "web.ts.net",
                    "addresses": ["100.64.0.1"],
                    "os": "linux",
                    "authorized": true,
                    "tags": []
                },
                {
                    "nodeId": "n2",
                    "hostname": "db",
                    "name": "db.ts.net",
                    "addresses": ["100.64.0.2"],
                    "os": "linux",
                    "authorized": true,
                    "tags": ["tag:prod"]
                }
            ]
        }"#;
        let resp: ApiResponse = serde_json::from_str(json).unwrap();
        let hosts = Tailscale::hosts_from_api(resp).unwrap();
        assert_eq!(hosts.len(), 2);
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    #[test]
    fn test_select_ip_prefers_ipv4() {
        let ips = vec!["fd7a:115c:a1e0::1".to_string(), "100.64.0.5".to_string()];
        assert_eq!(select_ip(&ips), Some("100.64.0.5".to_string()));
    }

    #[test]
    fn test_select_ip_ipv6_fallback() {
        let ips = vec!["fd7a:115c:a1e0::1".to_string()];
        assert_eq!(select_ip(&ips), Some("fd7a:115c:a1e0::1".to_string()));
    }

    #[test]
    fn test_select_ip_strips_cidr() {
        let ips = vec!["100.64.0.1/32".to_string()];
        assert_eq!(select_ip(&ips), Some("100.64.0.1".to_string()));
    }

    #[test]
    fn test_select_ip_empty() {
        let ips: Vec<String> = vec![];
        assert_eq!(select_ip(&ips), None);
    }

    #[test]
    fn test_strip_tag_prefix() {
        assert_eq!(strip_tag_prefix("tag:server"), "server");
        assert_eq!(strip_tag_prefix("tag:prod"), "prod");
        assert_eq!(strip_tag_prefix("notag"), "notag");
        assert_eq!(strip_tag_prefix(""), "");
    }

    // =========================================================================
    // Trait
    // =========================================================================

    #[test]
    fn test_tailscale_name() {
        let ts = Tailscale;
        assert_eq!(ts.name(), "tailscale");
    }

    #[test]
    fn test_tailscale_short_label() {
        let ts = Tailscale;
        assert_eq!(ts.short_label(), "ts");
    }

    // =========================================================================
    // Token validation
    // =========================================================================

    #[test]
    fn test_auth_key_rejected() {
        let ts = Tailscale;
        let cancel = AtomicBool::new(false);
        let result = ts.fetch_hosts_cancellable("tskey-auth-abc123", &cancel);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("device auth key"),
            "Error should mention device auth key: {}",
            err
        );
    }
}
