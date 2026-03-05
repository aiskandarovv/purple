use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde_json::Value;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct Proxmox {
    pub base_url: String,
    pub verify_tls: bool,
}

// --- Serde helpers ---

/// Deserialize a value that may be `null` or missing as `T::default()`.
/// `#[serde(default)]` only covers missing keys; this also handles explicit nulls.
fn null_to_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(|o| o.unwrap_or_default())
}

/// Deserialize a value that may be a string, integer or boolean into `Option<String>`.
/// Proxmox's Perl JSON serializer sometimes returns integer `1` instead of string `"1"`
/// for config values like `agent`.
fn lenient_string<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Option::<Value>::deserialize(d)? {
        Some(Value::String(s)) => Ok(Some(s)),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(Value::Bool(b)) => Ok(Some(if b { "1".to_string() } else { "0".to_string() })),
        _ => Ok(None),
    }
}

/// Deserialize a value that may be an integer, boolean or null as u8.
/// Handles `"template": true` (→ 1), `"template": 1`, `"template": null` (→ 0).
fn lenient_u8<'de, D>(d: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Option::<Value>::deserialize(d)? {
        Some(Value::Number(n)) => Ok(n.as_u64().unwrap_or(0) as u8),
        Some(Value::Bool(b)) => Ok(if b { 1 } else { 0 }),
        _ => Ok(0),
    }
}

// --- Serde structs ---

#[derive(Deserialize)]
struct PveResponse<T> {
    data: T,
}

#[derive(Deserialize)]
struct ClusterResource {
    #[serde(rename = "type")]
    resource_type: String,
    #[serde(default, deserialize_with = "null_to_default")]
    vmid: u64,
    #[serde(default, deserialize_with = "null_to_default")]
    name: String,
    #[serde(default, deserialize_with = "null_to_default")]
    node: String,
    #[serde(default, deserialize_with = "null_to_default")]
    status: String,
    #[serde(default, deserialize_with = "lenient_u8")]
    template: u8,
    #[serde(default)]
    tags: Option<String>,
    #[serde(default)]
    ip: Option<String>,
}

#[derive(Deserialize, Default)]
struct VmConfig {
    #[serde(default, deserialize_with = "lenient_string")]
    agent: Option<String>,
    /// Catch-all for dynamic fields like ipconfig0-9, net0-9.
    #[serde(flatten)]
    extra: HashMap<String, Value>,
}

// Guest agent response is double-wrapped: {"data": {"result": [...]}}
// data or result may be null when the agent is starting up or unavailable.
#[derive(Deserialize)]
struct GuestAgentNetworkResponse {
    #[serde(default, deserialize_with = "null_to_default")]
    data: GuestAgentResult,
}

#[derive(Deserialize, Default)]
struct GuestAgentResult {
    #[serde(default, deserialize_with = "null_to_default")]
    result: Vec<GuestInterface>,
}

#[derive(Deserialize)]
struct GuestInterface {
    #[serde(default, deserialize_with = "null_to_default")]
    name: String,
    #[serde(default, deserialize_with = "null_to_default", rename = "ip-addresses")]
    ip_addresses: Vec<GuestIpAddress>,
}

#[derive(Deserialize)]
struct GuestIpAddress {
    #[serde(default, deserialize_with = "null_to_default", rename = "ip-address")]
    ip_address: String,
    #[serde(default, deserialize_with = "null_to_default", rename = "ip-address-type")]
    ip_address_type: String,
}

// LXC container interfaces from /lxc/{vmid}/interfaces
#[derive(Deserialize, Default)]
struct LxcInterface {
    #[serde(default, deserialize_with = "null_to_default")]
    name: String,
    // Legacy PVE format: inet/inet6 CIDR strings
    #[serde(default)]
    inet: Option<String>,
    #[serde(default)]
    inet6: Option<String>,
    // Newer PVE format: same ip-addresses array shape as QEMU guest agent
    #[serde(default, deserialize_with = "null_to_default", rename = "ip-addresses")]
    ip_addresses: Vec<GuestIpAddress>,
}

/// Outcome of resolving an IP for a single VM/container.
enum ResolveOutcome {
    /// Successfully resolved an IP address.
    Resolved(String),
    /// VM is stopped, cannot resolve runtime IP.
    Stopped,
    /// No IP could be determined (running but no static or agent IP).
    NoIp,
    /// API call failed (HTTP error, parse error).
    Failed,
    /// API call failed with 401/403 (authentication or permission error).
    AuthFailed,
}

// --- Helper functions ---

/// Build the PVE auth header value. Prepends "PVEAPIToken=" if not already present.
fn auth_header(token: &str) -> String {
    if token.starts_with("PVEAPIToken=") {
        token.to_string()
    } else {
        format!("PVEAPIToken={}", token)
    }
}

/// Normalize base URL: trim whitespace, strip trailing slash and /api2/json suffix.
fn normalize_url(url: &str) -> String {
    let mut u = url.trim().trim_end_matches('/').to_string();
    if u.ends_with("/api2/json") {
        u.truncate(u.len() - "/api2/json".len());
    }
    u
}

/// Returns true if the IP is a loopback or link-local address that should not be
/// used as an SSH hostname.
fn is_unusable_ip(ip: &str) -> bool {
    if ip.is_empty() {
        return true;
    }
    // IPv4 loopback (127.0.0.0/8) and link-local (169.254.0.0/16)
    if ip.starts_with("127.") || ip.starts_with("169.254.") {
        return true;
    }
    // IPv6 loopback and link-local
    let ip_lc = ip.to_ascii_lowercase();
    ip_lc == "::1" || ip_lc.starts_with("fe80:") || ip_lc.starts_with("fe80%")
}

/// Parse a static IP from ipconfig0 value like "ip=10.0.0.1/24,gw=10.0.0.1".
/// Prefers IPv4 (ip=). Falls back to IPv6 (ip6=) if ip= is dhcp or absent.
/// Returns None if both are dhcp/auto or absent.
fn parse_ipconfig_ip(ipconfig: &str) -> Option<String> {
    let mut ipv6_candidate = None;
    for part in ipconfig.split(',') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("ip=") {
            if value.is_empty() || value.eq_ignore_ascii_case("dhcp") || value.eq_ignore_ascii_case("manual") {
                continue;
            }
            return Some(super::strip_cidr(value).to_string());
        }
        if let Some(value) = part.strip_prefix("ip6=") {
            if value.is_empty() || value.eq_ignore_ascii_case("dhcp") || value.eq_ignore_ascii_case("auto") || value.eq_ignore_ascii_case("manual") {
                continue;
            }
            if ipv6_candidate.is_none() {
                ipv6_candidate = Some(super::strip_cidr(value).to_string());
            }
        }
    }
    ipv6_candidate
}

/// Parse a static IP from LXC net0 value like "name=eth0,bridge=vmbr0,ip=10.0.0.2/24,...".
/// Prefers IPv4 (ip=). Falls back to IPv6 (ip6=) if ip= is dhcp or absent.
fn parse_lxc_net_ip(net0: &str) -> Option<String> {
    let mut ipv6_candidate = None;
    for part in net0.split(',') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("ip=") {
            if value.is_empty() || value.eq_ignore_ascii_case("dhcp") || value.eq_ignore_ascii_case("manual") {
                continue;
            }
            return Some(super::strip_cidr(value).to_string());
        }
        if let Some(value) = part.strip_prefix("ip6=") {
            if value.is_empty() || value.eq_ignore_ascii_case("dhcp") || value.eq_ignore_ascii_case("auto") || value.eq_ignore_ascii_case("manual") {
                continue;
            }
            if ipv6_candidate.is_none() {
                ipv6_candidate = Some(super::strip_cidr(value).to_string());
            }
        }
    }
    ipv6_candidate
}

/// Extract sorted string values for keys matching a prefix (e.g. "ipconfig" -> ipconfig0..9).
fn extract_numbered_values(extra: &HashMap<String, Value>, prefix: &str) -> Vec<String> {
    let mut entries: Vec<(u32, String)> = extra
        .iter()
        .filter_map(|(k, v)| {
            let suffix = k.strip_prefix(prefix)?;
            let n: u32 = suffix.parse().ok()?;
            let s = v.as_str()?.to_string();
            Some((n, s))
        })
        .collect();
    entries.sort_by_key(|(n, _)| *n);
    entries.into_iter().map(|(_, v)| v).collect()
}

/// Check if the QEMU agent is enabled. The agent field can be:
/// "1", "enabled=1", "1,fstrim_cloned_disks=1,type=virtio", etc.
fn is_agent_enabled(agent: Option<&str>) -> bool {
    let s = match agent {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    // First comma-separated token is the enable flag
    let first = s.split(',').next().unwrap_or("");
    if first == "1" {
        return true;
    }
    if let Some(val) = first.strip_prefix("enabled=") {
        return val == "1";
    }
    false
}

/// Parse PVE tags string. PVE uses semicolons (PVE 7), commas (PVE 8) or spaces
/// as tag separators. Split on all three for compatibility.
fn parse_pve_tags(tags: Option<&str>) -> Vec<String> {
    let s = match tags {
        Some(s) if !s.is_empty() => s,
        _ => return Vec::new(),
    };
    s.split([';', ',', ' '])
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Select the best IP from guest agent interfaces.
/// Skips loopback, link-local. Prefers IPv4.
fn select_guest_agent_ip(interfaces: &[GuestInterface]) -> Option<String> {
    let mut ipv4_candidate = None;
    let mut ipv6_candidate = None;

    for iface in interfaces {
        if iface.name == "lo" {
            continue;
        }
        for addr in &iface.ip_addresses {
            let ip = super::strip_cidr(&addr.ip_address);
            if ip.is_empty() {
                continue;
            }
            if addr.ip_address_type == "ipv4" {
                if ip.starts_with("169.254.") || ip.starts_with("127.") {
                    continue;
                }
                if ipv4_candidate.is_none() {
                    ipv4_candidate = Some(ip.to_string());
                }
            } else if addr.ip_address_type == "ipv6" {
                let ip_lc = ip.to_ascii_lowercase();
                if ip_lc.starts_with("fe80:") || ip_lc.starts_with("fe80%") || ip_lc == "::1" {
                    continue;
                }
                if ipv6_candidate.is_none() {
                    ipv6_candidate = Some(ip.to_string());
                }
            }
        }
    }

    ipv4_candidate.or(ipv6_candidate)
}

/// Select the best IP from LXC container interfaces.
/// Handles both the legacy inet/inet6 string format and the newer ip-addresses array format.
/// Skips loopback, link-local. Prefers IPv4.
fn select_lxc_interface_ip(interfaces: &[LxcInterface]) -> Option<String> {
    let mut ipv4_candidate = None;
    let mut ipv6_candidate = None;

    for iface in interfaces {
        if iface.name == "lo" {
            continue;
        }
        // Legacy format: inet/inet6 CIDR strings
        if let Some(ref inet) = iface.inet {
            let ip = super::strip_cidr(inet.split_whitespace().next().unwrap_or(inet));
            if !ip.is_empty() && !ip.starts_with("169.254.") && !ip.starts_with("127.") && ipv4_candidate.is_none() {
                ipv4_candidate = Some(ip.to_string());
            }
        }
        if let Some(ref inet6) = iface.inet6 {
            let ip = super::strip_cidr(inet6.split_whitespace().next().unwrap_or(inet6));
            let ip_lc = ip.to_ascii_lowercase();
            if !ip.is_empty()
                && !ip_lc.starts_with("fe80:") && !ip_lc.starts_with("fe80%")
                && ip_lc != "::1"
                && ipv6_candidate.is_none()
            {
                ipv6_candidate = Some(ip.to_string());
            }
        }
        // Newer format: ip-addresses array.
        // LXC uses "inet"/"inet6" for ip-address-type (unlike QEMU guest agent
        // which uses "ipv4"/"ipv6"), so we accept both variants.
        for addr in &iface.ip_addresses {
            let ip = super::strip_cidr(&addr.ip_address);
            if ip.is_empty() {
                continue;
            }
            let t = addr.ip_address_type.as_str();
            if t == "ipv4" || t == "inet" {
                if ip.starts_with("169.254.") || ip.starts_with("127.") {
                    continue;
                }
                if ipv4_candidate.is_none() {
                    ipv4_candidate = Some(ip.to_string());
                }
            } else if t == "ipv6" || t == "inet6" {
                let ip_lc = ip.to_ascii_lowercase();
                if ip_lc.starts_with("fe80:") || ip_lc.starts_with("fe80%") || ip_lc == "::1" {
                    continue;
                }
                if ipv6_candidate.is_none() {
                    ipv6_candidate = Some(ip.to_string());
                }
            }
        }
    }

    ipv4_candidate.or(ipv6_candidate)
}

impl Proxmox {
    fn make_agent(&self) -> Result<ureq::Agent, ProviderError> {
        if self.verify_tls {
            Ok(super::http_agent())
        } else {
            super::http_agent_insecure()
        }
    }
}

impl Provider for Proxmox {
    fn name(&self) -> &str {
        "proxmox"
    }

    fn short_label(&self) -> &str {
        "pve"
    }

    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        self.fetch_hosts_with_progress(token, cancel, &|_| {})
    }

    fn fetch_hosts_with_progress(
        &self,
        token: &str,
        cancel: &AtomicBool,
        progress: &dyn Fn(&str),
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        let base = normalize_url(&self.base_url);
        if base.is_empty() {
            return Err(ProviderError::Http("No Proxmox URL configured.".to_string()));
        }
        if !base.to_ascii_lowercase().starts_with("https://") {
            return Err(ProviderError::Http(
                "Proxmox URL must use HTTPS. Update the URL in ~/.purple/providers.".to_string(),
            ));
        }

        let agent = self.make_agent()?;
        let auth = auth_header(token);

        // Phase 1: Fetch VM/container resources (type=vm returns both qemu and lxc)
        progress("Fetching resources...");
        let url = format!("{}/api2/json/cluster/resources?type=vm", base);
        let resp: PveResponse<Vec<ClusterResource>> = agent
            .get(&url)
            .set("Authorization", &auth)
            .call()
            .map_err(map_ureq_error)?
            .into_json()
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }

        // Filter for VMs and containers, skip templates
        let resources: Vec<&ClusterResource> = resp
            .data
            .iter()
            .filter(|r| (r.resource_type == "qemu" || r.resource_type == "lxc") && r.template == 0)
            .collect();

        let total = resources.len();
        progress(&format!("{} VMs/containers found.", total));

        // Phase 2: Resolve IPs for each resource
        let mut hosts = Vec::new();
        let mut fetch_failures = 0usize;
        let mut auth_failures = 0usize;
        let mut skipped_no_ip = 0usize;
        let mut skipped_stopped = 0usize;
        let mut resolved_count = 0usize;

        // N+1 API calls (one per VM). No rate limiting for v1. For very large clusters
        // (hundreds of VMs), consider adding a small delay between calls.
        for (i, resource) in resources.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            progress(&format!("Resolving IPs ({}/{})...", i + 1, total));

            // Use the IP from cluster/resources if available (free, no N+1 call).
            let cluster_ip = resource.ip.as_deref()
                .map(|ip| super::strip_cidr(ip).to_string())
                .filter(|ip| !is_unusable_ip(ip));
            let outcome = if let Some(ip) = cluster_ip {
                ResolveOutcome::Resolved(ip)
            } else if resource.resource_type == "qemu" {
                self.resolve_qemu_ip(&agent, &base, &auth, resource)
            } else {
                self.resolve_lxc_ip(&agent, &base, &auth, resource)
            };

            let ip = match outcome {
                ResolveOutcome::Resolved(ip) => {
                    resolved_count += 1;
                    ip
                }
                ResolveOutcome::Stopped => {
                    skipped_stopped += 1;
                    continue;
                }
                ResolveOutcome::NoIp => {
                    skipped_no_ip += 1;
                    continue;
                }
                ResolveOutcome::Failed => {
                    fetch_failures += 1;
                    continue;
                }
                ResolveOutcome::AuthFailed => {
                    fetch_failures += 1;
                    auth_failures += 1;
                    continue;
                }
            };

            // Build tags: PVE tags + resource type (dedup in case type already appears as a PVE tag)
            let mut tags = parse_pve_tags(resource.tags.as_deref());
            tags.push(resource.resource_type.clone());
            tags.sort();
            tags.dedup();

            hosts.push(ProviderHost {
                server_id: format!("{}:{}", resource.resource_type, resource.vmid),
                name: if resource.name.is_empty() {
                    format!("{}-{}", resource.resource_type, resource.vmid)
                } else {
                    resource.name.clone()
                },
                ip,
                tags,
            });
        }

        // Summary
        let mut parts = Vec::new();
        parts.push(format!("{} resolved", resolved_count));
        if skipped_no_ip > 0 {
            parts.push(format!("{} skipped (no IP)", skipped_no_ip));
        }
        if skipped_stopped > 0 {
            parts.push(format!("{} skipped (stopped)", skipped_stopped));
        }
        if fetch_failures > 0 {
            let label = if auth_failures == fetch_failures {
                format!("{} failed (authentication)", fetch_failures)
            } else if auth_failures > 0 {
                format!("{} failed ({} authentication)", fetch_failures, auth_failures)
            } else {
                format!("{} failed", fetch_failures)
            };
            parts.push(label);
        }
        progress(&parts.join(", "));

        if fetch_failures > 0 {
            if hosts.is_empty() {
                let msg = if auth_failures > 0 {
                    format!(
                        "Authentication failed for all {} VMs. Check your API token permissions.",
                        total
                    )
                } else {
                    format!("Failed to fetch details for all {} VMs", total)
                };
                return Err(ProviderError::Http(msg));
            }
            return Err(ProviderError::PartialResult {
                hosts,
                failures: fetch_failures,
                total,
            });
        }

        Ok(hosts)
    }
}

impl Proxmox {
    fn resolve_qemu_ip(
        &self,
        agent: &ureq::Agent,
        base: &str,
        auth: &str,
        resource: &ClusterResource,
    ) -> ResolveOutcome {
        // Step 1: Get VM config for ipconfig0
        let config_url = format!(
            "{}/api2/json/nodes/{}/qemu/{}/config",
            base, resource.node, resource.vmid
        );
        let config: VmConfig = match agent
            .get(&config_url)
            .set("Authorization", auth)
            .call()
        {
            Ok(resp) => match resp.into_json::<PveResponse<VmConfig>>() {
                Ok(r) => r.data,
                Err(_) => return ResolveOutcome::Failed,
            },
            Err(ureq::Error::Status(401, _) | ureq::Error::Status(403, _)) => {
                return ResolveOutcome::AuthFailed;
            }
            Err(_) => return ResolveOutcome::Failed,
        };

        // Try static IP from ipconfig0..9
        for ipconfig in extract_numbered_values(&config.extra, "ipconfig") {
            if let Some(ip) = parse_ipconfig_ip(&ipconfig) {
                return ResolveOutcome::Resolved(ip);
            }
        }

        // Step 2: Try guest agent if VM is running and agent is enabled
        if resource.status != "running" {
            return ResolveOutcome::Stopped;
        }

        if !is_agent_enabled(config.agent.as_deref()) {
            return ResolveOutcome::NoIp;
        }

        let agent_url = format!(
            "{}/api2/json/nodes/{}/qemu/{}/agent/network-get-interfaces",
            base, resource.node, resource.vmid
        );
        match agent.get(&agent_url).set("Authorization", auth).call() {
            Ok(resp) => {
                match resp.into_json::<GuestAgentNetworkResponse>() {
                    Ok(ga) => match select_guest_agent_ip(&ga.data.result) {
                        Some(ip) => ResolveOutcome::Resolved(ip),
                        None => ResolveOutcome::NoIp,
                    },
                    Err(_) => ResolveOutcome::Failed,
                }
            }
            Err(ureq::Error::Status(500, _))
            | Err(ureq::Error::Status(501, _)) => {
                // Agent not responding or not supported
                ResolveOutcome::NoIp
            }
            Err(ureq::Error::Status(401, _) | ureq::Error::Status(403, _)) => {
                ResolveOutcome::AuthFailed
            }
            Err(_) => {
                // Network errors, timeouts, etc.
                ResolveOutcome::Failed
            }
        }
    }

    fn resolve_lxc_ip(
        &self,
        agent: &ureq::Agent,
        base: &str,
        auth: &str,
        resource: &ClusterResource,
    ) -> ResolveOutcome {
        // Step 1: Get container config for net0
        let config_url = format!(
            "{}/api2/json/nodes/{}/lxc/{}/config",
            base, resource.node, resource.vmid
        );
        let config: VmConfig = match agent
            .get(&config_url)
            .set("Authorization", auth)
            .call()
        {
            Ok(resp) => match resp.into_json::<PveResponse<VmConfig>>() {
                Ok(r) => r.data,
                Err(_) => return ResolveOutcome::Failed,
            },
            Err(ureq::Error::Status(401, _) | ureq::Error::Status(403, _)) => {
                return ResolveOutcome::AuthFailed;
            }
            Err(_) => return ResolveOutcome::Failed,
        };

        // Try static IP from net0..9
        for net in extract_numbered_values(&config.extra, "net") {
            if let Some(ip) = parse_lxc_net_ip(&net) {
                return ResolveOutcome::Resolved(ip);
            }
        }

        // Step 2: Try runtime interfaces if container is running
        if resource.status != "running" {
            return ResolveOutcome::Stopped;
        }

        let iface_url = format!(
            "{}/api2/json/nodes/{}/lxc/{}/interfaces",
            base, resource.node, resource.vmid
        );
        match agent.get(&iface_url).set("Authorization", auth).call() {
            Ok(resp) => {
                match resp.into_json::<PveResponse<Vec<LxcInterface>>>() {
                    Ok(r) => match select_lxc_interface_ip(&r.data) {
                        Some(ip) => ResolveOutcome::Resolved(ip),
                        None => ResolveOutcome::NoIp,
                    },
                    Err(_) => ResolveOutcome::Failed,
                }
            }
            Err(ureq::Error::Status(401, _) | ureq::Error::Status(403, _)) => {
                ResolveOutcome::AuthFailed
            }
            Err(ureq::Error::Status(500, _))
            | Err(ureq::Error::Status(404, _))
            | Err(ureq::Error::Status(501, _)) => {
                // 500: container restarting or PVE hiccup
                // 404/501: endpoint may not exist on older PVE
                ResolveOutcome::NoIp
            }
            Err(_) => ResolveOutcome::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Serde tests ---

    #[test]
    fn test_parse_cluster_resources() {
        let json = r#"{"data": [
            {"type": "qemu", "vmid": 100, "name": "web-1", "node": "pve1", "status": "running", "template": 0, "tags": "prod;web"},
            {"type": "lxc", "vmid": 200, "name": "dns-1", "node": "pve1", "status": "running", "template": 0},
            {"type": "qemu", "vmid": 999, "name": "template", "node": "pve1", "status": "stopped", "template": 1},
            {"type": "storage", "id": "local", "node": "pve1", "status": "available"}
        ]}"#;
        let resp: PveResponse<Vec<ClusterResource>> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 4);
        let vms: Vec<_> = resp.data.iter()
            .filter(|r| (r.resource_type == "qemu" || r.resource_type == "lxc") && r.template == 0)
            .collect();
        assert_eq!(vms.len(), 2);
        assert_eq!(vms[0].vmid, 100);
        assert_eq!(vms[1].vmid, 200);
    }

    #[test]
    fn test_cluster_resource_ip_field() {
        let json = r#"{"data": [
            {"type": "qemu", "vmid": 100, "name": "web-1", "node": "pve1", "status": "running", "template": 0, "ip": "10.0.0.5"},
            {"type": "lxc",  "vmid": 200, "name": "dns-1", "node": "pve1", "status": "running", "template": 0}
        ]}"#;
        let resp: PveResponse<Vec<ClusterResource>> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data[0].ip.as_deref(), Some("10.0.0.5"));
        assert_eq!(resp.data[1].ip, None);
    }

    #[test]
    fn test_parse_guest_agent_response_double_wrapped() {
        let json = r#"{"data": {"result": [
            {"name": "lo", "ip-addresses": [{"ip-address": "127.0.0.1", "ip-address-type": "ipv4"}]},
            {"name": "eth0", "ip-addresses": [
                {"ip-address": "10.0.0.5", "ip-address-type": "ipv4"},
                {"ip-address": "fe80::1", "ip-address-type": "ipv6"}
            ]}
        ]}}"#;
        let resp: GuestAgentNetworkResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.result.len(), 2);
        assert_eq!(resp.data.result[1].ip_addresses[0].ip_address, "10.0.0.5");
    }

    #[test]
    fn test_parse_lxc_interfaces() {
        let json = r#"{"data": [
            {"name": "lo", "inet": "127.0.0.1/8", "inet6": "::1/128"},
            {"name": "eth0", "inet": "10.0.0.10/24", "inet6": "fd00::10/64"}
        ]}"#;
        let resp: PveResponse<Vec<LxcInterface>> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.data[1].inet.as_deref(), Some("10.0.0.10/24"));
    }

    // --- extract_numbered_values tests ---

    #[test]
    fn test_extract_numbered_values_sorted() {
        let mut extra = HashMap::new();
        extra.insert("ipconfig2".into(), Value::String("ip=10.0.2.1/24".into()));
        extra.insert("ipconfig0".into(), Value::String("ip=dhcp".into()));
        extra.insert("ipconfig1".into(), Value::String("ip=10.0.1.1/24".into()));
        extra.insert("agent".into(), Value::String("1".into()));
        let values = extract_numbered_values(&extra, "ipconfig");
        assert_eq!(values, vec!["ip=dhcp", "ip=10.0.1.1/24", "ip=10.0.2.1/24"]);
    }

    #[test]
    fn test_extract_numbered_values_skips_non_string() {
        let mut extra = HashMap::new();
        extra.insert("net0".into(), Value::String("name=eth0,ip=10.0.0.1/24".into()));
        extra.insert("net1".into(), Value::Number(serde_json::Number::from(42)));
        let values = extract_numbered_values(&extra, "net");
        assert_eq!(values, vec!["name=eth0,ip=10.0.0.1/24"]);
    }

    #[test]
    fn test_vmconfig_flatten_deserialization() {
        let json = r#"{"agent": "1", "ipconfig0": "ip=dhcp", "ipconfig1": "ip=10.0.1.1/24", "net0": "name=eth0,bridge=vmbr0,ip=dhcp", "cores": 4}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent, Some("1".to_string()));
        let ipconfigs = extract_numbered_values(&config.extra, "ipconfig");
        assert_eq!(ipconfigs, vec!["ip=dhcp", "ip=10.0.1.1/24"]);
        let nets = extract_numbered_values(&config.extra, "net");
        assert_eq!(nets, vec!["name=eth0,bridge=vmbr0,ip=dhcp"]);
    }

    #[test]
    fn test_multi_nic_ipconfig_fallback() {
        // ipconfig0 is DHCP, ipconfig1 has static IP
        let mut extra = HashMap::new();
        extra.insert("ipconfig0".into(), Value::String("ip=dhcp".into()));
        extra.insert("ipconfig1".into(), Value::String("ip=10.0.1.5/24".into()));
        let mut result = None;
        for ipconfig in extract_numbered_values(&extra, "ipconfig") {
            if let Some(ip) = parse_ipconfig_ip(&ipconfig) {
                result = Some(ip);
                break;
            }
        }
        assert_eq!(result, Some("10.0.1.5".to_string()));
    }

    // --- parse_ipconfig_ip tests ---

    #[test]
    fn test_parse_ipconfig_static() {
        assert_eq!(parse_ipconfig_ip("ip=10.0.0.1/24,gw=10.0.0.1"), Some("10.0.0.1".to_string()));
    }

    #[test]
    fn test_parse_ipconfig_dhcp() {
        assert_eq!(parse_ipconfig_ip("ip=dhcp"), None);
    }

    #[test]
    fn test_parse_ipconfig_ip6_only() {
        assert_eq!(
            parse_ipconfig_ip("ip6=2001:db8::1/64,gw6=2001:db8::ffff"),
            Some("2001:db8::1".to_string())
        );
    }

    #[test]
    fn test_parse_ipconfig_dhcp_with_ip6_static() {
        assert_eq!(
            parse_ipconfig_ip("ip=dhcp,ip6=fd00::1/64"),
            Some("fd00::1".to_string())
        );
    }

    #[test]
    fn test_parse_ipconfig_ip6_dhcp() {
        assert_eq!(parse_ipconfig_ip("ip6=dhcp"), None);
    }

    #[test]
    fn test_parse_ipconfig_ip6_auto() {
        assert_eq!(parse_ipconfig_ip("ip6=auto"), None);
    }

    #[test]
    fn test_parse_ipconfig_ipv4_preferred_over_ipv6() {
        assert_eq!(
            parse_ipconfig_ip("ip=10.0.0.1/24,ip6=2001:db8::1/64"),
            Some("10.0.0.1".to_string())
        );
    }

    #[test]
    fn test_parse_ipconfig_both_dhcp() {
        assert_eq!(parse_ipconfig_ip("ip=dhcp,ip6=dhcp"), None);
    }

    #[test]
    fn test_parse_ipconfig_no_ip_key() {
        assert_eq!(parse_ipconfig_ip("gw=10.0.0.1"), None);
    }

    #[test]
    fn test_parse_ipconfig_ipv6() {
        assert_eq!(
            parse_ipconfig_ip("ip=2001:db8::1/64,gw=2001:db8::ffff"),
            Some("2001:db8::1".to_string())
        );
    }

    // --- parse_lxc_net_ip tests ---

    #[test]
    fn test_parse_lxc_net_static() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip=10.0.0.2/24,gw=10.0.0.1"),
            Some("10.0.0.2".to_string())
        );
    }

    #[test]
    fn test_parse_lxc_net_dhcp() {
        assert_eq!(parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip=dhcp"), None);
    }

    #[test]
    fn test_parse_lxc_net_ip6_only() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip6=fd00::2/64"),
            Some("fd00::2".to_string())
        );
    }

    #[test]
    fn test_parse_lxc_net_dhcp_with_ip6_static() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip=dhcp,ip6=fd00::2/64"),
            Some("fd00::2".to_string())
        );
    }

    #[test]
    fn test_parse_lxc_net_ip6_auto() {
        assert_eq!(parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip6=auto"), None);
    }

    #[test]
    fn test_parse_lxc_net_ip6_manual() {
        assert_eq!(parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip6=manual"), None);
    }

    #[test]
    fn test_parse_ipconfig_ip6_manual() {
        assert_eq!(parse_ipconfig_ip("ip6=manual"), None);
    }

    #[test]
    fn test_parse_ipconfig_dhcp_and_ip6_manual() {
        assert_eq!(parse_ipconfig_ip("ip=dhcp,ip6=manual"), None);
    }

    #[test]
    fn test_parse_ipconfig_ip_manual() {
        assert_eq!(parse_ipconfig_ip("ip=manual"), None);
    }

    #[test]
    fn test_parse_ipconfig_ip_empty() {
        assert_eq!(parse_ipconfig_ip("ip="), None);
    }

    #[test]
    fn test_parse_ipconfig_ip6_empty() {
        assert_eq!(parse_ipconfig_ip("ip6="), None);
    }

    #[test]
    fn test_parse_ipconfig_manual_with_ip6_static() {
        assert_eq!(
            parse_ipconfig_ip("ip=manual,ip6=fd00::1/64"),
            Some("fd00::1".to_string())
        );
    }

    #[test]
    fn test_parse_lxc_net_ip_manual() {
        assert_eq!(parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip=manual"), None);
    }

    #[test]
    fn test_parse_lxc_net_ip_empty() {
        assert_eq!(parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip="), None);
    }

    #[test]
    fn test_parse_lxc_net_ip6_empty() {
        assert_eq!(parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip6="), None);
    }

    #[test]
    fn test_parse_lxc_net_manual_with_ip6_static() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip=manual,ip6=fd00::2/64"),
            Some("fd00::2".to_string())
        );
    }

    // --- is_agent_enabled tests ---

    #[test]
    fn test_agent_enabled_simple() {
        assert!(is_agent_enabled(Some("1")));
    }

    #[test]
    fn test_agent_disabled_simple() {
        assert!(!is_agent_enabled(Some("0")));
    }

    #[test]
    fn test_agent_enabled_explicit() {
        assert!(is_agent_enabled(Some("enabled=1")));
    }

    #[test]
    fn test_agent_enabled_with_options() {
        assert!(is_agent_enabled(Some("1,fstrim_cloned_disks=1,type=virtio")));
    }

    #[test]
    fn test_agent_disabled_explicit() {
        assert!(!is_agent_enabled(Some("enabled=0")));
    }

    #[test]
    fn test_agent_none() {
        assert!(!is_agent_enabled(None));
    }

    #[test]
    fn test_agent_empty() {
        assert!(!is_agent_enabled(Some("")));
    }

    // --- parse_pve_tags tests ---

    #[test]
    fn test_tags_semicolons() {
        assert_eq!(parse_pve_tags(Some("prod;web;us-east")), vec!["prod", "web", "us-east"]);
    }

    #[test]
    fn test_tags_commas() {
        assert_eq!(parse_pve_tags(Some("prod,web,us-east")), vec!["prod", "web", "us-east"]);
    }

    #[test]
    fn test_tags_mixed() {
        assert_eq!(parse_pve_tags(Some("prod;web,us-east")), vec!["prod", "web", "us-east"]);
    }

    #[test]
    fn test_tags_empty() {
        assert!(parse_pve_tags(None).is_empty());
        assert!(parse_pve_tags(Some("")).is_empty());
    }

    #[test]
    fn test_tags_whitespace() {
        assert_eq!(parse_pve_tags(Some(" prod ; web ")), vec!["prod", "web"]);
    }

    #[test]
    fn test_tags_lowercased() {
        assert_eq!(parse_pve_tags(Some("PROD;Web")), vec!["prod", "web"]);
    }

    #[test]
    fn test_tags_spaces() {
        assert_eq!(parse_pve_tags(Some("prod web us-east")), vec!["prod", "web", "us-east"]);
    }

    #[test]
    fn test_tags_mixed_all_separators() {
        assert_eq!(parse_pve_tags(Some("prod;web,db us-east")), vec!["prod", "web", "db", "us-east"]);
    }

    // --- auth_header tests ---

    #[test]
    fn test_auth_header_without_prefix() {
        assert_eq!(auth_header("user@pam!tok=secret"), "PVEAPIToken=user@pam!tok=secret");
    }

    #[test]
    fn test_auth_header_with_prefix() {
        assert_eq!(
            auth_header("PVEAPIToken=user@pam!tok=secret"),
            "PVEAPIToken=user@pam!tok=secret"
        );
    }

    // --- normalize_url tests ---

    #[test]
    fn test_normalize_url_trailing_slash() {
        assert_eq!(normalize_url("https://pve:8006/"), "https://pve:8006");
    }

    #[test]
    fn test_normalize_url_api_suffix() {
        assert_eq!(
            normalize_url("https://pve:8006/api2/json"),
            "https://pve:8006"
        );
    }

    #[test]
    fn test_normalize_url_bare() {
        assert_eq!(normalize_url("https://pve:8006"), "https://pve:8006");
    }

    #[test]
    fn test_normalize_url_api_suffix_trailing_slash() {
        assert_eq!(
            normalize_url("https://pve:8006/api2/json/"),
            "https://pve:8006"
        );
    }

    #[test]
    fn test_normalize_url_whitespace() {
        assert_eq!(normalize_url("  https://pve:8006  "), "https://pve:8006");
        assert_eq!(normalize_url("https://pve:8006 "), "https://pve:8006");
        assert_eq!(normalize_url(" https://pve:8006"), "https://pve:8006");
    }

    // --- select_guest_agent_ip tests ---

    #[test]
    fn test_guest_agent_ipv4_preferred() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "2001:db8::1".into(), ip_address_type: "ipv6".into() },
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_guest_agent_skips_loopback() {
        let interfaces = vec![
            GuestInterface {
                name: "lo".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.0.0.1".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_skips_link_local() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "169.254.1.1".into(), ip_address_type: "ipv4".into() },
                    GuestIpAddress { ip_address: "fe80::1".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_skips_link_local_uppercase() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "FE80::1".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_ipv6_fallback() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "2001:db8::1".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), Some("2001:db8::1".to_string()));
    }

    // --- select_lxc_interface_ip tests ---

    #[test]
    fn test_lxc_inet_preferred() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: Some("10.0.0.10/24".into()), inet6: Some("fd00::10/64".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.10".to_string()));
    }

    #[test]
    fn test_lxc_inet6_fallback() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: None, inet6: Some("fd00::10/64".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("fd00::10".to_string()));
    }

    #[test]
    fn test_lxc_skips_loopback() {
        let interfaces = vec![
            LxcInterface { name: "lo".into(), inet: Some("127.0.0.1/8".into()), inet6: None, ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_skips_link_local_ipv6_colon() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: None, inet6: Some("fe80::1/64".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_skips_link_local_ipv6_zone_id() {
        // fe80%eth0 zone-id format must be filtered the same way as guest agent
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: None, inet6: Some("fe80%eth0/64".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_skips_link_local_ipv6_zone_id_uppercase() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: None, inet6: Some("FE80%eth0/64".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    // --- server_id format ---

    #[test]
    fn test_server_id_format() {
        let resource = ClusterResource {
            resource_type: "qemu".into(),
            vmid: 100,
            name: "web-1".into(),
            node: "pve1".into(),
            status: "running".into(),
            template: 0,
            tags: None,
            ip: None,
        };
        assert_eq!(format!("{}:{}", resource.resource_type, resource.vmid), "qemu:100");
    }

    // --- resource type tag injection ---

    #[test]
    fn test_resource_type_tag_added() {
        let mut tags = parse_pve_tags(Some("prod;web"));
        tags.push("qemu".to_string());
        tags.sort();
        tags.dedup();
        assert_eq!(tags, vec!["prod", "qemu", "web"]);
    }

    #[test]
    fn test_resource_type_tag_no_duplicate_when_pve_tag_matches() {
        // VM with PVE tag "qemu" must not produce ["prod", "qemu", "qemu"]
        let mut tags = parse_pve_tags(Some("prod;qemu"));
        tags.push("qemu".to_string());
        tags.sort();
        tags.dedup();
        assert_eq!(tags, vec!["prod", "qemu"]);
    }

    #[test]
    fn test_lxc_resource_type_tag_no_duplicate() {
        let mut tags = parse_pve_tags(Some("lxc;db"));
        tags.push("lxc".to_string());
        tags.sort();
        tags.dedup();
        assert_eq!(tags, vec!["db", "lxc"]);
    }

    // --- template filtering ---

    #[test]
    fn test_template_filtered() {
        let resources = [
            ClusterResource {
                resource_type: "qemu".into(), vmid: 100, name: "vm".into(),
                node: "n".into(), status: "running".into(), template: 0, tags: None, ip: None,
            },
            ClusterResource {
                resource_type: "qemu".into(), vmid: 999, name: "tmpl".into(),
                node: "n".into(), status: "stopped".into(), template: 1, tags: None, ip: None,
            },
        ];
        let filtered: Vec<_> = resources.iter()
            .filter(|r| r.template == 0)
            .collect();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].vmid, 100);
    }

    // --- loopback IP filtering ---

    #[test]
    fn test_guest_agent_skips_loopback_ip_on_non_lo_iface() {
        // 127.x.x.x on a non-lo interface must still be skipped
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.0.0.1".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_skips_loopback_range() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.1.2.3".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_skips_ipv6_loopback() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "::1".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_loopback_then_real_ip() {
        // loopback on non-lo must not prevent picking real IP from another interface
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.0.0.1".into(), ip_address_type: "ipv4".into() },
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_lxc_skips_loopback_ip_on_non_lo_iface() {
        // 127.x.x.x on a non-lo interface must still be skipped
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: Some("127.0.0.1/8".into()), inet6: None, ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_skips_ipv6_loopback() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: None, inet6: Some("::1/128".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    // --- LxcInterface ip-addresses format (fix 1) ---

    #[test]
    fn test_lxc_ip_addresses_format_ipv4() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "ipv4".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_lxc_ip_addresses_format_skips_loopback() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.0.0.1".into(), ip_address_type: "ipv4".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_ip_addresses_format_skips_link_local() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "fe80::1".into(), ip_address_type: "ipv6".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_ip_addresses_format_ipv4_preferred_over_ipv6() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "2001:db8::1".into(), ip_address_type: "ipv6".into() },
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "ipv4".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_lxc_inet_takes_precedence_over_ip_addresses() {
        // If both formats present, inet wins (encountered first in code)
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                inet: Some("192.168.1.1/24".into()),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "ipv4".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("192.168.1.1".to_string()));
    }

    // LXC ip-addresses uses "inet"/"inet6" (unlike QEMU "ipv4"/"ipv6")

    #[test]
    fn test_lxc_ip_addresses_inet_type_ipv4() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "inet".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_lxc_ip_addresses_inet6_type() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "2001:db8::1".into(), ip_address_type: "inet6".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("2001:db8::1".to_string()));
    }

    #[test]
    fn test_lxc_ip_addresses_inet_preferred_over_inet6() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "2001:db8::1".into(), ip_address_type: "inet6".into() },
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "inet".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_lxc_ip_addresses_inet_skips_loopback() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.0.0.1".into(), ip_address_type: "inet".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_ip_addresses_inet6_skips_link_local() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "fe80::1".into(), ip_address_type: "inet6".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    // --- strip_cidr in guest agent (fix 4) ---

    #[test]
    fn test_guest_agent_strips_cidr_ipv4() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "10.0.0.5/24".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_guest_agent_strips_cidr_ipv6() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "2001:db8::1/64".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), Some("2001:db8::1".to_string()));
    }

    // --- Fe80 mixed-case filtering (fix 3) ---

    #[test]
    fn test_guest_agent_skips_mixed_case_link_local() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "Fe80::1".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_skips_mixed_case_link_local_inet6() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet6: Some("Fe80::1/64".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_ip_addresses_strips_cidr() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "10.0.0.5/24".into(), ip_address_type: "ipv4".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    // --- name fallback ---

    #[test]
    fn test_name_fallback_when_empty() {
        let resource = ClusterResource {
            resource_type: "lxc".into(), vmid: 200, name: String::new(),
            node: "n".into(), status: "running".into(), template: 0, tags: None, ip: None,
        };
        let name = if resource.name.is_empty() {
            format!("{}-{}", resource.resource_type, resource.vmid)
        } else {
            resource.name.clone()
        };
        assert_eq!(name, "lxc-200");
    }

    // --- null-safe deserialization tests ---

    #[test]
    fn test_guest_agent_result_null_is_empty() {
        let json = r#"{"result": null}"#;
        let result: GuestAgentResult = serde_json::from_str(json).unwrap();
        assert!(result.result.is_empty());
    }

    #[test]
    fn test_guest_agent_result_missing_is_empty() {
        let json = r#"{}"#;
        let result: GuestAgentResult = serde_json::from_str(json).unwrap();
        assert!(result.result.is_empty());
    }

    #[test]
    fn test_guest_interface_null_ip_addresses() {
        let json = r#"{"name": "eth0", "ip-addresses": null}"#;
        let iface: GuestInterface = serde_json::from_str(json).unwrap();
        assert_eq!(iface.name, "eth0");
        assert!(iface.ip_addresses.is_empty());
    }

    #[test]
    fn test_lxc_interface_null_ip_addresses() {
        let json = r#"{"name": "eth0", "ip-addresses": null}"#;
        let iface: LxcInterface = serde_json::from_str(json).unwrap();
        assert_eq!(iface.name, "eth0");
        assert!(iface.ip_addresses.is_empty());
    }

    #[test]
    fn test_full_guest_agent_response_with_null_result() {
        let json = r#"{"data": {"result": null}}"#;
        let resp: GuestAgentNetworkResponse = serde_json::from_str(json).unwrap();
        assert!(resp.data.result.is_empty());
    }

    #[test]
    fn test_full_guest_agent_response_with_null_data() {
        let json = r#"{"data": null}"#;
        let resp: GuestAgentNetworkResponse = serde_json::from_str(json).unwrap();
        assert!(resp.data.result.is_empty());
    }

    #[test]
    fn test_guest_interface_null_name() {
        let json = r#"{"name": null, "ip-addresses": [{"ip-address": "10.0.0.1", "ip-address-type": "ipv4"}]}"#;
        let iface: GuestInterface = serde_json::from_str(json).unwrap();
        assert_eq!(iface.name, "");
        assert_eq!(iface.ip_addresses.len(), 1);
    }

    #[test]
    fn test_guest_ip_address_null_fields() {
        let json = r#"{"ip-address": null, "ip-address-type": null}"#;
        let addr: GuestIpAddress = serde_json::from_str(json).unwrap();
        assert_eq!(addr.ip_address, "");
        assert_eq!(addr.ip_address_type, "");
    }

    #[test]
    fn test_lxc_interface_null_name() {
        let json = r#"{"name": null, "inet": "10.0.0.1/24"}"#;
        let iface: LxcInterface = serde_json::from_str(json).unwrap();
        assert_eq!(iface.name, "");
        assert_eq!(iface.inet.as_deref(), Some("10.0.0.1/24"));
    }

    #[test]
    fn test_guest_agent_response_with_null_interface_name_in_array() {
        // An interface with null name in the result array must not crash the entire deserialization
        let json = r#"{"data": {"result": [
            {"name": null, "ip-addresses": [{"ip-address": "10.0.0.5", "ip-address-type": "ipv4"}]},
            {"name": "eth0", "ip-addresses": [{"ip-address": "192.168.1.1", "ip-address-type": "ipv4"}]}
        ]}}"#;
        let resp: GuestAgentNetworkResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.result.len(), 2);
        // First interface has empty name (from null), should still be processed
        let ip = select_guest_agent_ip(&resp.data.result);
        assert_eq!(ip, Some("10.0.0.5".to_string()));
    }

    // --- is_unusable_ip tests ---

    #[test]
    fn test_unusable_ip_loopback_ipv4() {
        assert!(is_unusable_ip("127.0.0.1"));
        assert!(is_unusable_ip("127.1.2.3"));
    }

    #[test]
    fn test_unusable_ip_link_local_ipv4() {
        assert!(is_unusable_ip("169.254.1.1"));
        assert!(is_unusable_ip("169.254.0.0"));
    }

    #[test]
    fn test_unusable_ip_loopback_ipv6() {
        assert!(is_unusable_ip("::1"));
    }

    #[test]
    fn test_unusable_ip_link_local_ipv6() {
        assert!(is_unusable_ip("fe80::1"));
        assert!(is_unusable_ip("FE80::1"));
        assert!(is_unusable_ip("fe80%eth0"));
    }

    #[test]
    fn test_unusable_ip_empty() {
        assert!(is_unusable_ip(""));
    }

    #[test]
    fn test_usable_ip_private() {
        assert!(!is_unusable_ip("10.0.0.1"));
        assert!(!is_unusable_ip("192.168.1.1"));
        assert!(!is_unusable_ip("172.16.0.1"));
    }

    #[test]
    fn test_usable_ip_public() {
        assert!(!is_unusable_ip("8.8.8.8"));
        assert!(!is_unusable_ip("2001:db8::1"));
    }

    // --- lenient deserialization tests ---

    #[test]
    fn test_vmconfig_agent_as_string() {
        let json = r#"{"agent": "1,fstrim_cloned_disks=1"}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent.as_deref(), Some("1,fstrim_cloned_disks=1"));
    }

    #[test]
    fn test_vmconfig_agent_as_integer() {
        // Proxmox Perl JSON serializer may return integer instead of string
        let json = r#"{"agent": 1}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent.as_deref(), Some("1"));
        assert!(is_agent_enabled(config.agent.as_deref()));
    }

    #[test]
    fn test_vmconfig_agent_as_integer_zero() {
        let json = r#"{"agent": 0}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent.as_deref(), Some("0"));
        assert!(!is_agent_enabled(config.agent.as_deref()));
    }

    #[test]
    fn test_vmconfig_agent_as_boolean() {
        let json = r#"{"agent": true}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent.as_deref(), Some("1"));
        assert!(is_agent_enabled(config.agent.as_deref()));
    }

    #[test]
    fn test_vmconfig_agent_as_null() {
        let json = r#"{"agent": null}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent, None);
        assert!(!is_agent_enabled(config.agent.as_deref()));
    }

    #[test]
    fn test_vmconfig_agent_missing() {
        let json = r#"{}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent, None);
    }

    #[test]
    fn test_cluster_resource_null_name() {
        let json = r#"{"type": "qemu", "vmid": 100, "name": null, "node": "pve1", "status": "running", "template": 0}"#;
        let r: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(r.name, "");
    }

    #[test]
    fn test_cluster_resource_null_vmid() {
        let json = r#"{"type": "qemu", "vmid": null, "name": "test", "node": "pve1", "status": "running", "template": 0}"#;
        let r: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(r.vmid, 0);
    }

    #[test]
    fn test_cluster_resource_null_status() {
        let json = r#"{"type": "qemu", "vmid": 100, "name": "test", "node": "pve1", "status": null, "template": 0}"#;
        let r: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(r.status, "");
    }

    #[test]
    fn test_cluster_resource_template_as_boolean() {
        let json = r#"{"type": "qemu", "vmid": 100, "name": "tmpl", "node": "pve1", "status": "stopped", "template": true}"#;
        let r: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(r.template, 1);
    }

    #[test]
    fn test_cluster_resource_template_as_null() {
        let json = r#"{"type": "qemu", "vmid": 100, "name": "vm", "node": "pve1", "status": "running", "template": null}"#;
        let r: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(r.template, 0);
    }

    #[test]
    fn test_cluster_resource_partial_null_in_array() {
        // One resource with null name in a list must not crash the entire deserialization
        let json = r#"{"data": [
            {"type": "qemu", "vmid": 100, "name": null, "node": "pve1", "status": "running", "template": 0},
            {"type": "lxc", "vmid": 200, "name": "dns-1", "node": "pve1", "status": "running", "template": 0}
        ]}"#;
        let resp: PveResponse<Vec<ClusterResource>> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.data[0].name, "");
        assert_eq!(resp.data[1].name, "dns-1");
    }

    // --- is_agent_enabled edge cases ---

    #[test]
    fn test_agent_disabled_with_options() {
        // Agent disabled but with extra options
        assert!(!is_agent_enabled(Some("0,fstrim_cloned_disks=1")));
    }

    #[test]
    fn test_agent_enabled_explicit_with_options() {
        assert!(is_agent_enabled(Some("enabled=1,fstrim_cloned_disks=1")));
    }

    #[test]
    fn test_agent_disabled_explicit_with_options() {
        assert!(!is_agent_enabled(Some("enabled=0,type=virtio")));
    }

    #[test]
    fn test_agent_garbage_value() {
        assert!(!is_agent_enabled(Some("yes")));
    }

    #[test]
    fn test_agent_enabled_2_not_treated_as_enabled() {
        // Only "1" means enabled
        assert!(!is_agent_enabled(Some("2")));
    }

    // --- extract_numbered_values edge cases ---

    #[test]
    fn test_extract_numbered_values_empty_map() {
        let extra = HashMap::new();
        assert!(extract_numbered_values(&extra, "ipconfig").is_empty());
    }

    #[test]
    fn test_extract_numbered_values_non_sequential() {
        // Gaps in numbering (0, 3, 7) should still sort correctly
        let mut extra = HashMap::new();
        extra.insert("net7".into(), Value::String("c".into()));
        extra.insert("net0".into(), Value::String("a".into()));
        extra.insert("net3".into(), Value::String("b".into()));
        let values = extract_numbered_values(&extra, "net");
        assert_eq!(values, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_extract_numbered_values_ignores_non_numeric_suffix() {
        let mut extra = HashMap::new();
        extra.insert("net0".into(), Value::String("valid".into()));
        extra.insert("network".into(), Value::String("invalid".into()));
        extra.insert("net_extra".into(), Value::String("invalid".into()));
        let values = extract_numbered_values(&extra, "net");
        assert_eq!(values, vec!["valid"]);
    }

    // --- normalize_url edge cases ---

    #[test]
    fn test_normalize_url_no_port() {
        assert_eq!(normalize_url("https://pve.example.com"), "https://pve.example.com");
    }

    #[test]
    fn test_normalize_url_with_subpath() {
        assert_eq!(normalize_url("https://pve:8006/pve"), "https://pve:8006/pve");
    }

    #[test]
    fn test_normalize_url_multiple_trailing_slashes() {
        // trim_end_matches strips all trailing slashes
        assert_eq!(normalize_url("https://pve:8006//"), "https://pve:8006");
    }

    // --- server_id for lxc ---

    #[test]
    fn test_server_id_format_lxc() {
        let resource = ClusterResource {
            resource_type: "lxc".into(),
            vmid: 200,
            name: "dns-1".into(),
            node: "pve1".into(),
            status: "running".into(),
            template: 0,
            tags: None,
            ip: None,
        };
        assert_eq!(format!("{}:{}", resource.resource_type, resource.vmid), "lxc:200");
    }

    // --- guest agent: multiple interfaces, second has IPv4 ---

    #[test]
    fn test_guest_agent_picks_ipv4_from_second_interface() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "fe80::1".into(), ip_address_type: "ipv6".into() },
                ],
            },
            GuestInterface {
                name: "eth1".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_guest_agent_empty_interfaces() {
        let interfaces: Vec<GuestInterface> = Vec::new();
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_empty_ip_address_skipped() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    // --- LXC multi-NIC: net0 dhcp, net1 static ---

    #[test]
    fn test_lxc_multi_nic_net0_dhcp_net1_static() {
        let mut extra = HashMap::new();
        extra.insert("net0".into(), Value::String("name=eth0,bridge=vmbr0,ip=dhcp".into()));
        extra.insert("net1".into(), Value::String("name=eth1,bridge=vmbr1,ip=10.0.1.5/24".into()));
        let mut result = None;
        for net in extract_numbered_values(&extra, "net") {
            if let Some(ip) = parse_lxc_net_ip(&net) {
                result = Some(ip);
                break;
            }
        }
        assert_eq!(result, Some("10.0.1.5".to_string()));
    }

    // --- LXC interface: link-local IPv4 skipped ---

    #[test]
    fn test_lxc_skips_link_local_ipv4() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: Some("169.254.1.1/16".into()), inet6: None, ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_link_local_v4_falls_through_to_inet6() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                inet: Some("169.254.1.1/16".into()),
                inet6: Some("fd00::10/64".into()),
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("fd00::10".to_string()));
    }

    // --- cluster/resources IP field with CIDR ---

    #[test]
    fn test_cluster_ip_with_cidr_stripped() {
        let ip_raw = "10.0.0.5/24";
        let stripped = crate::providers::strip_cidr(ip_raw).to_string();
        assert_eq!(stripped, "10.0.0.5");
        assert!(!is_unusable_ip(&stripped));
    }

    #[test]
    fn test_cluster_ip_unusable_filtered() {
        // cluster/resources may return 127.0.0.1 or fe80::1
        let ip1 = crate::providers::strip_cidr("127.0.0.1").to_string();
        assert!(is_unusable_ip(&ip1));

        let ip2 = crate::providers::strip_cidr("fe80::1/64").to_string();
        assert!(is_unusable_ip(&ip2));
    }

    // --- lenient_u8 edge cases ---

    #[test]
    fn test_cluster_resource_template_as_false() {
        let json = r#"{"type": "qemu", "vmid": 100, "name": "vm", "node": "n", "status": "running", "template": false}"#;
        let r: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(r.template, 0);
    }

    // --- tags with all separator types combined ---

    #[test]
    fn test_tags_consecutive_separators_produce_no_empty() {
        // Multiple adjacent separators should not produce empty strings
        let tags = parse_pve_tags(Some("a;;b,,c  d"));
        assert_eq!(tags, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn test_tags_single_tag() {
        assert_eq!(parse_pve_tags(Some("production")), vec!["production"]);
    }

    // --- parse_ipconfig_ip with spaces around comma ---

    #[test]
    fn test_parse_ipconfig_whitespace_around_parts() {
        // Some PVE configs may have whitespace around commas
        assert_eq!(
            parse_ipconfig_ip("ip=10.0.0.1/24, gw=10.0.0.1"),
            Some("10.0.0.1".to_string())
        );
    }

    // --- parse_lxc_net_ip IPv6 preference ---

    #[test]
    fn test_parse_lxc_net_ipv4_preferred_over_ipv6() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip=192.168.1.5/24,ip6=fd00::5/64"),
            Some("192.168.1.5".to_string())
        );
    }

    // --- LXC interface: multiple NICs, first lo, second has IP ---

    #[test]
    fn test_lxc_multi_interface_with_lo_first() {
        let interfaces = vec![
            LxcInterface { name: "lo".into(), inet: Some("127.0.0.1/8".into()), inet6: Some("::1/128".into()), ..Default::default() },
            LxcInterface { name: "eth0".into(), inet: Some("10.0.0.10/24".into()), inet6: None, ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.10".to_string()));
    }

    // --- guest agent: multi-NIC with lo, link-local, then real IP ---

    #[test]
    fn test_guest_agent_realistic_multi_nic() {
        let interfaces = vec![
            GuestInterface {
                name: "lo".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.0.0.1".into(), ip_address_type: "ipv4".into() },
                    GuestIpAddress { ip_address: "::1".into(), ip_address_type: "ipv6".into() },
                ],
            },
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "fe80::be24:11ff:fecf:a0e6".into(), ip_address_type: "ipv6".into() },
                    GuestIpAddress { ip_address: "10.0.0.100".into(), ip_address_type: "ipv4".into() },
                    GuestIpAddress { ip_address: "2001:db8::100".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        // Should pick 10.0.0.100 (first valid IPv4, skipping lo and fe80)
        assert_eq!(select_guest_agent_ip(&interfaces), Some("10.0.0.100".to_string()));
    }

    // --- LXC: ip-addresses with inet type skips link-local ---

    #[test]
    fn test_lxc_ip_addresses_inet_skips_link_local_v4() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "169.254.1.1".into(), ip_address_type: "inet".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    // --- parse_ipconfig with DHCP case insensitive ---

    #[test]
    fn test_parse_ipconfig_dhcp_case_insensitive() {
        assert_eq!(parse_ipconfig_ip("ip=DHCP"), None);
        assert_eq!(parse_ipconfig_ip("ip=Dhcp"), None);
    }

    #[test]
    fn test_parse_ipconfig_manual_case_insensitive() {
        assert_eq!(parse_ipconfig_ip("ip=MANUAL"), None);
        assert_eq!(parse_ipconfig_ip("ip=Manual"), None);
    }

    #[test]
    fn test_parse_ipconfig_ip6_auto_case_insensitive() {
        assert_eq!(parse_ipconfig_ip("ip6=AUTO"), None);
        assert_eq!(parse_ipconfig_ip("ip6=Auto"), None);
    }

    #[test]
    fn test_parse_lxc_net_dhcp_case_insensitive() {
        assert_eq!(parse_lxc_net_ip("name=eth0,ip=DHCP"), None);
        assert_eq!(parse_lxc_net_ip("name=eth0,ip=Dhcp"), None);
    }

    #[test]
    fn test_parse_lxc_net_ip6_auto_case_insensitive() {
        assert_eq!(parse_lxc_net_ip("name=eth0,ip6=AUTO"), None);
        assert_eq!(parse_lxc_net_ip("name=eth0,ip6=Auto"), None);
    }

    // --- VmConfig empty/null defaults ---

    #[test]
    fn test_vmconfig_default() {
        let json = r#"{}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent, None);
        assert!(config.extra.is_empty());
    }

    // --- Realistic PVE 8 cluster/resources with tags ---

    #[test]
    fn test_cluster_resources_pve8_comma_tags() {
        // PVE 8 uses comma-separated tags
        let json = r#"{"data": [
            {"type": "qemu", "vmid": 100, "name": "web-1", "node": "pve1", "status": "running", "template": 0, "tags": "production,web,us-east"}
        ]}"#;
        let resp: PveResponse<Vec<ClusterResource>> = serde_json::from_str(json).unwrap();
        let tags = parse_pve_tags(resp.data[0].tags.as_deref());
        assert_eq!(tags, vec!["production", "web", "us-east"]);
    }

    // --- auth_header with complex token ---

    #[test]
    fn test_auth_header_complex_token() {
        assert_eq!(
            auth_header("user@pve!api-token=12345678-abcd-efgh-ijkl-123456789012"),
            "PVEAPIToken=user@pve!api-token=12345678-abcd-efgh-ijkl-123456789012"
        );
    }

    #[test]
    fn test_auth_header_ldap_user() {
        assert_eq!(
            auth_header("user@ldap!tok=secret-value"),
            "PVEAPIToken=user@ldap!tok=secret-value"
        );
    }

    // --- vmid edge cases ---

    #[test]
    fn test_vmid_zero_is_valid() {
        // vmid=0 from null deserialization should still work
        let resource = ClusterResource {
            resource_type: "qemu".into(),
            vmid: 0,
            name: "test".into(),
            node: "n".into(),
            status: "running".into(),
            template: 0,
            tags: None,
            ip: None,
        };
        assert_eq!(format!("{}:{}", resource.resource_type, resource.vmid), "qemu:0");
    }

    // --- cluster/resources ip field with multiple IPs ---

    #[test]
    fn test_cluster_ip_comma_separated_treated_as_single() {
        // If the API ever returns comma-separated IPs, strip_cidr won't help
        // but is_unusable_ip will catch it as non-matching
        let ip_raw = "10.0.0.5,10.0.0.6";
        let stripped = crate::providers::strip_cidr(ip_raw);
        // No slash found, returns original. The IP won't match any unusable pattern
        // but it's also not a valid single IP for SSH
        assert_eq!(stripped, "10.0.0.5,10.0.0.6");
    }

    // --- PVE 7 semicolons vs PVE 8 commas ---

    #[test]
    fn test_pve7_semicolon_tags() {
        let tags = parse_pve_tags(Some("prod;web;us-east"));
        assert_eq!(tags, vec!["prod", "web", "us-east"]);
    }

    #[test]
    fn test_pve8_comma_tags() {
        let tags = parse_pve_tags(Some("prod,web,us-east"));
        assert_eq!(tags, vec!["prod", "web", "us-east"]);
    }

    #[test]
    fn test_pve_space_tags() {
        let tags = parse_pve_tags(Some("prod web us-east"));
        assert_eq!(tags, vec!["prod", "web", "us-east"]);
    }

    // --- resource filtering: only qemu and lxc, not storage/node ---

    #[test]
    fn test_resource_type_filter_storage_excluded() {
        let resources = [
            ClusterResource {
                resource_type: "storage".into(), vmid: 0, name: "local".into(),
                node: "n".into(), status: "available".into(), template: 0, tags: None, ip: None,
            },
            ClusterResource {
                resource_type: "node".into(), vmid: 0, name: "pve1".into(),
                node: "pve1".into(), status: "online".into(), template: 0, tags: None, ip: None,
            },
            ClusterResource {
                resource_type: "qemu".into(), vmid: 100, name: "vm".into(),
                node: "pve1".into(), status: "running".into(), template: 0, tags: None, ip: None,
            },
        ];
        let filtered: Vec<_> = resources.iter()
            .filter(|r| (r.resource_type == "qemu" || r.resource_type == "lxc") && r.template == 0)
            .collect();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].resource_type, "qemu");
    }

    // --- Guest agent with CIDR on ip_address ---

    #[test]
    fn test_guest_agent_ip_with_cidr_prefix() {
        // Some QEMU guest agents return "10.0.0.5/24" format
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "10.0.0.5/24".into(), ip_address_type: "ipv4".into() },
                    GuestIpAddress { ip_address: "fd00::5/64".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        // IPv4 should be returned with CIDR stripped
        assert_eq!(select_guest_agent_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    // --- LXC interface with inet that has whitespace before CIDR ---

    #[test]
    fn test_lxc_inet_with_scope_info() {
        // Some PVE versions include scope info after the IP
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                inet: Some("10.0.0.10/24 brd 10.0.0.255".into()),
                inet6: None,
                ..Default::default()
            },
        ];
        // split_whitespace().next() should extract just "10.0.0.10/24"
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.10".to_string()));
    }

    // --- normalize_url with HTTP (rejected by fetch_hosts) ---

    #[test]
    fn test_normalize_url_http_preserved() {
        // normalize_url doesn't validate scheme, that's done in fetch_hosts
        assert_eq!(normalize_url("http://pve:8006"), "http://pve:8006");
    }

    // --- Guest agent response with nested empty data ---

    #[test]
    fn test_guest_agent_response_empty_data_object() {
        let json = r#"{"data": {}}"#;
        let resp: GuestAgentNetworkResponse = serde_json::from_str(json).unwrap();
        assert!(resp.data.result.is_empty());
    }

    // --- LXC interface with only lo (all other IPs filtered) ---

    #[test]
    fn test_lxc_only_lo_interface() {
        let interfaces = vec![
            LxcInterface {
                name: "lo".into(),
                inet: Some("127.0.0.1/8".into()),
                inet6: Some("::1/128".into()),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.0.0.1".into(), ip_address_type: "inet".into() },
                    GuestIpAddress { ip_address: "::1".into(), ip_address_type: "inet6".into() },
                ],
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    // --- VmConfig with boolean false for agent ---

    #[test]
    fn test_vmconfig_agent_as_boolean_false() {
        let json = r#"{"agent": false}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent.as_deref(), Some("0"));
        assert!(!is_agent_enabled(config.agent.as_deref()));
    }

    // =========================================================================
    // lenient_string edge cases
    // =========================================================================

    #[test]
    fn test_lenient_string_boolean_false_to_zero() {
        // lenient_string converts false → "0"
        let json = r#"{"agent": false}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent, Some("0".to_string()));
    }

    #[test]
    fn test_lenient_string_boolean_true_to_one() {
        let json = r#"{"agent": true}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent, Some("1".to_string()));
    }

    #[test]
    fn test_lenient_string_number_to_string() {
        let json = r#"{"agent": 42}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent, Some("42".to_string()));
    }

    #[test]
    fn test_lenient_string_null_to_none() {
        let json = r#"{"agent": null}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent, None);
    }

    #[test]
    fn test_lenient_string_zero_to_string() {
        let json = r#"{"agent": 0}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent, Some("0".to_string()));
    }

    // =========================================================================
    // lenient_u8 edge cases
    // =========================================================================

    #[test]
    fn test_lenient_u8_boolean_true() {
        let json = r#"{"vmid": 100, "name": "test", "status": "running", "type": "qemu", "template": true}"#;
        let res: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(res.template, 1);
    }

    #[test]
    fn test_lenient_u8_boolean_false_to_zero() {
        let json = r#"{"vmid": 100, "name": "test", "status": "running", "type": "qemu", "template": false}"#;
        let res: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(res.template, 0);
    }

    #[test]
    fn test_lenient_u8_null_to_zero() {
        let json = r#"{"vmid": 100, "name": "test", "status": "running", "type": "qemu", "template": null}"#;
        let res: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(res.template, 0);
    }

    #[test]
    fn test_lenient_u8_missing_to_zero() {
        let json = r#"{"vmid": 100, "name": "test", "status": "running", "type": "qemu"}"#;
        let res: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(res.template, 0);
    }

    #[test]
    fn test_lenient_u8_large_number_wraps() {
        // 256 as u64 cast to u8 wraps to 0
        let json = r#"{"vmid": 100, "name": "test", "status": "running", "type": "qemu", "template": 256}"#;
        let res: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(res.template, 0); // 256 % 256 = 0
    }

    // =========================================================================
    // is_agent_enabled edge cases
    // =========================================================================

    #[test]
    fn test_agent_enabled_with_spaces() {
        // "enabled= 1" → split by comma gives "enabled= 1"
        // strip_prefix("enabled=") gives " 1" which != "1"
        assert!(!is_agent_enabled(Some("enabled= 1")));
    }

    #[test]
    fn test_agent_enabled_only_commas() {
        assert!(!is_agent_enabled(Some(",,")));
    }

    #[test]
    fn test_agent_enabled_many_options() {
        assert!(is_agent_enabled(Some("1,fstrim_cloned_disks=1,type=virtio")));
    }

    #[test]
    fn test_agent_enabled_explicit_zero_with_options() {
        assert!(!is_agent_enabled(Some("0,fstrim_cloned_disks=1")));
    }

    // =========================================================================
    // extract_numbered_values edge cases
    // =========================================================================

    #[test]
    fn test_extract_numbered_values_null_value() {
        let mut extra = HashMap::new();
        extra.insert("net0".into(), Value::Null);
        let values = extract_numbered_values(&extra, "net");
        assert!(values.is_empty()); // Null.as_str() returns None
    }

    #[test]
    fn test_extract_numbered_values_boolean_value() {
        let mut extra = HashMap::new();
        extra.insert("net0".into(), Value::Bool(true));
        let values = extract_numbered_values(&extra, "net");
        assert!(values.is_empty()); // Bool.as_str() returns None
    }

    #[test]
    fn test_extract_numbered_values_number_value() {
        let mut extra = HashMap::new();
        extra.insert("net0".into(), Value::Number(serde_json::Number::from(42)));
        let values = extract_numbered_values(&extra, "net");
        assert!(values.is_empty()); // Number.as_str() returns None
    }

    #[test]
    fn test_extract_numbered_values_empty_prefix() {
        // Empty prefix matches everything but suffix must be numeric
        let mut extra = HashMap::new();
        extra.insert("0".into(), Value::String("first".into()));
        extra.insert("1".into(), Value::String("second".into()));
        extra.insert("abc".into(), Value::String("not-matched".into()));
        let values = extract_numbered_values(&extra, "");
        assert_eq!(values, vec!["first", "second"]);
    }

    // =========================================================================
    // select_lxc_interface_ip whitespace edge cases
    // =========================================================================

    #[test]
    fn test_lxc_inet_leading_whitespace() {
        let interfaces = vec![LxcInterface {
            name: "eth0".into(),
            inet: Some("  10.0.0.1/24".into()),
            ..Default::default()
        }];
        // split_whitespace handles leading spaces
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.1".to_string()));
    }

    #[test]
    fn test_lxc_inet_tab_separated() {
        let interfaces = vec![LxcInterface {
            name: "eth0".into(),
            inet: Some("10.0.0.1/24\tbrd\t10.0.0.255".into()),
            ..Default::default()
        }];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.1".to_string()));
    }

    #[test]
    fn test_lxc_inet6_multiple_tokens() {
        let interfaces = vec![LxcInterface {
            name: "eth0".into(),
            inet6: Some("fe80::1/64 scope link".into()),
            ..Default::default()
        }];
        // fe80 is link-local, should be skipped
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_inet6_global_with_scope() {
        let interfaces = vec![LxcInterface {
            name: "eth0".into(),
            inet6: Some("2001:db8::1/128 scope global".into()),
            ..Default::default()
        }];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("2001:db8::1".to_string()));
    }

    // =========================================================================
    // normalize_url edge cases
    // =========================================================================

    #[test]
    fn test_normalize_url_empty_string() {
        assert_eq!(normalize_url(""), "");
    }

    #[test]
    fn test_normalize_url_whitespace_only() {
        assert_eq!(normalize_url("   "), "");
    }

    #[test]
    fn test_normalize_url_trailing_slashes_and_api() {
        assert_eq!(
            normalize_url("https://pve:8006/api2/json/"),
            "https://pve:8006"
        );
    }

    #[test]
    fn test_normalize_url_just_api_path() {
        assert_eq!(normalize_url("https://pve:8006/api2/json"), "https://pve:8006");
    }

    #[test]
    fn test_normalize_url_no_trailing() {
        assert_eq!(normalize_url("https://pve:8006"), "https://pve:8006");
    }

    // =========================================================================
    // select_lxc_interface_ip: empty inet string
    // =========================================================================

    #[test]
    fn test_lxc_inet_empty_string() {
        let interfaces = vec![LxcInterface {
            name: "eth0".into(),
            inet: Some(String::new()),
            ..Default::default()
        }];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    // =========================================================================
    // select_lxc_interface_ip: loopback via inet type in ip-addresses array
    // =========================================================================

    #[test]
    fn test_lxc_ip_addresses_inet_loopback_skipped() {
        let interfaces = vec![LxcInterface {
            name: "eth0".into(),
            ip_addresses: vec![GuestIpAddress {
                ip_address: "127.0.0.1".into(),
                ip_address_type: "inet".into(),
            }],
            ..Default::default()
        }];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    // =========================================================================
    // select_guest_agent_ip: two valid IPv4 on different interfaces (picks first)
    // =========================================================================

    #[test]
    fn test_guest_agent_two_ipv4_picks_first() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![GuestIpAddress {
                    ip_address: "10.0.0.1".into(),
                    ip_address_type: "ipv4".into(),
                }],
            },
            GuestInterface {
                name: "eth1".into(),
                ip_addresses: vec![GuestIpAddress {
                    ip_address: "10.0.0.2".into(),
                    ip_address_type: "ipv4".into(),
                }],
            },
        ];
        assert_eq!(
            select_guest_agent_ip(&interfaces),
            Some("10.0.0.1".to_string())
        );
    }

    // =========================================================================
    // select_guest_agent_ip: empty ip_address skipped
    // =========================================================================

    #[test]
    fn test_guest_agent_empty_ip_skipped() {
        let interfaces = vec![GuestInterface {
            name: "eth0".into(),
            ip_addresses: vec![
                GuestIpAddress {
                    ip_address: String::new(),
                    ip_address_type: "ipv4".into(),
                },
                GuestIpAddress {
                    ip_address: "10.0.0.1".into(),
                    ip_address_type: "ipv4".into(),
                },
            ],
        }];
        assert_eq!(
            select_guest_agent_ip(&interfaces),
            Some("10.0.0.1".to_string())
        );
    }

    // =========================================================================
    // lenient_u8: float value (1.5 -> as_u64 is None -> 0)
    // =========================================================================

    #[test]
    fn test_lenient_u8_float_to_zero() {
        let json = r#"{"vmid": 100, "name": "test", "status": "running", "type": "qemu", "template": 1.5}"#;
        let res: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(res.template, 0);
    }

    // =========================================================================
    // lenient_string: array value -> None
    // =========================================================================

    #[test]
    fn test_lenient_string_array_to_none() {
        let json = r#"{"agent": [1, 2]}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent, None);
    }

    // =========================================================================
    // parse_ipconfig_ip: ip=manual skipped
    // =========================================================================

    #[test]
    fn test_ipconfig_manual_skipped() {
        assert_eq!(parse_ipconfig_ip("ip=manual,gw=10.0.0.1"), None);
    }

    // =========================================================================
    // parse_ipconfig_ip: ip6=auto skipped
    // =========================================================================

    #[test]
    fn test_ipconfig_ip6_auto_skipped() {
        assert_eq!(parse_ipconfig_ip("ip6=auto"), None);
    }

    // =========================================================================
    // parse_ipconfig_ip: ip6=manual skipped
    // =========================================================================

    #[test]
    fn test_ipconfig_ip6_manual_skipped() {
        assert_eq!(parse_ipconfig_ip("ip6=manual"), None);
    }

    // =========================================================================
    // parse_lxc_net_ip: ip=manual skipped
    // =========================================================================

    #[test]
    fn test_lxc_net_manual_skipped() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip=manual"),
            None
        );
    }

    // =========================================================================
    // parse_lxc_net_ip: ip6=manual skipped
    // =========================================================================

    #[test]
    fn test_lxc_net_ip6_manual_skipped() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip6=manual"),
            None
        );
    }

    // =========================================================================
    // parse_lxc_net_ip: ip6=auto skipped, falls through to None
    // =========================================================================

    #[test]
    fn test_lxc_net_ip6_auto_skipped() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip6=auto"),
            None
        );
    }

    // =========================================================================
    // auth_header: already has PVEAPIToken= prefix
    // =========================================================================

    #[test]
    fn test_auth_header_already_prefixed() {
        assert_eq!(
            auth_header("PVEAPIToken=user@pam!token=abc"),
            "PVEAPIToken=user@pam!token=abc"
        );
    }

    #[test]
    fn test_auth_header_prepends_prefix() {
        assert_eq!(
            auth_header("user@pam!token=abc"),
            "PVEAPIToken=user@pam!token=abc"
        );
    }

    // =========================================================================
    // parse_pve_tags: comma-separated (PVE 8 format)
    // =========================================================================

    #[test]
    fn test_pve_tags_comma_separated() {
        assert_eq!(parse_pve_tags(Some("web,prod,us")), vec!["web", "prod", "us"]);
    }

    // =========================================================================
    // parse_pve_tags: mixed separators
    // =========================================================================

    #[test]
    fn test_pve_tags_mixed_separators() {
        assert_eq!(
            parse_pve_tags(Some("web;prod,us east")),
            vec!["web", "prod", "us", "east"]
        );
    }

    // =========================================================================
    // is_unusable_ip: unspecified IPv6 "::"
    // =========================================================================

    #[test]
    fn test_is_unusable_ip_unspecified_v6() {
        // "::" is the unspecified address, but the function only checks for ::1, fe80:, fe80%.
        // So "::" is NOT considered unusable by this function (it's valid but rarely useful).
        assert!(!is_unusable_ip("::"));
    }

    #[test]
    fn test_is_unusable_ip_normal_v4() {
        assert!(!is_unusable_ip("10.0.0.1"));
    }

    #[test]
    fn test_is_unusable_ip_empty() {
        assert!(is_unusable_ip(""));
    }

    // =========================================================================
    // select_lxc_interface_ip: inet6 with loopback "::1"
    // =========================================================================

    #[test]
    fn test_lxc_inet6_loopback_skipped() {
        let interfaces = vec![LxcInterface {
            name: "eth0".into(),
            inet6: Some("::1/128".into()),
            ..Default::default()
        }];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    // =========================================================================
    // GuestAgentNetworkResponse: null data
    // =========================================================================

    #[test]
    fn test_guest_agent_response_null_data() {
        let json = r#"{"data": null}"#;
        let resp: GuestAgentNetworkResponse = serde_json::from_str(json).unwrap();
        assert!(resp.data.result.is_empty());
    }

    // =========================================================================
    // GuestAgentNetworkResponse: null result inside data
    // =========================================================================

    #[test]
    fn test_guest_agent_response_null_result() {
        let json = r#"{"data": {"result": null}}"#;
        let resp: GuestAgentNetworkResponse = serde_json::from_str(json).unwrap();
        assert!(resp.data.result.is_empty());
    }

    // =========================================================================
    // ClusterResource with all null fields (null_to_default coverage)
    // =========================================================================

    #[test]
    fn test_cluster_resource_all_null_fields() {
        let json = r#"{"type": "qemu", "vmid": null, "name": null, "node": null, "status": null}"#;
        let res: ClusterResource = serde_json::from_str(json).unwrap();
        assert_eq!(res.vmid, 0);
        assert_eq!(res.name, "");
        assert_eq!(res.node, "");
        assert_eq!(res.status, "");
    }

    // =========================================================================
    // parse_ipconfig_ip: only ip6, no ip key at all
    // =========================================================================

    #[test]
    fn test_ipconfig_only_ip6() {
        assert_eq!(
            parse_ipconfig_ip("ip6=2001:db8::1/64,gw6=2001:db8::1"),
            Some("2001:db8::1".to_string())
        );
    }

    // =========================================================================
    // parse_ipconfig_ip: both ip (non-dhcp) and ip6 -> prefers ip (IPv4)
    // =========================================================================

    #[test]
    fn test_ipconfig_prefers_ipv4_over_ipv6() {
        assert_eq!(
            parse_ipconfig_ip("ip=10.0.0.1/24,ip6=2001:db8::1/64"),
            Some("10.0.0.1".to_string())
        );
    }

    // =========================================================================
    // parse_ipconfig_ip: ip=dhcp with ip6 static -> falls back to ip6
    // =========================================================================

    #[test]
    fn test_ipconfig_dhcp_falls_back_to_ip6() {
        assert_eq!(
            parse_ipconfig_ip("ip=dhcp,ip6=2001:db8::1/64"),
            Some("2001:db8::1".to_string())
        );
    }

    // =========================================================================
    // parse_lxc_net_ip: full net0 line with static IP
    // =========================================================================

    #[test]
    fn test_lxc_net_full_line() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,hwaddr=AA:BB:CC:DD:EE:FF,ip=192.168.1.100/24,gw=192.168.1.1"),
            Some("192.168.1.100".to_string())
        );
    }

    // =========================================================================
    // parse_lxc_net_ip: ip=dhcp falls back to ip6
    // =========================================================================

    #[test]
    fn test_lxc_net_dhcp_falls_back_to_ip6() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip=dhcp,ip6=fd00::1/64"),
            Some("fd00::1".to_string())
        );
    }
}
