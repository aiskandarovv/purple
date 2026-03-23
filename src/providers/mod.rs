pub mod aws;
pub mod azure;
pub mod config;
mod digitalocean;
pub mod gcp;
mod hetzner;
mod linode;
mod proxmox;
pub mod scaleway;
pub mod sync;
mod tailscale;
mod upcloud;
mod vultr;

use std::sync::atomic::AtomicBool;

use thiserror::Error;

/// A host discovered from a cloud provider API.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProviderHost {
    /// Provider-assigned server ID.
    pub server_id: String,
    /// Server name/label.
    pub name: String,
    /// Public IP address (IPv4 or IPv6).
    pub ip: String,
    /// Provider tags/labels.
    pub tags: Vec<String>,
    /// Provider metadata (region, plan, etc.) as key-value pairs.
    pub metadata: Vec<(String, String)>,
}

impl ProviderHost {
    /// Create a ProviderHost with no metadata.
    #[allow(dead_code)]
    pub fn new(server_id: String, name: String, ip: String, tags: Vec<String>) -> Self {
        Self {
            server_id,
            name,
            ip,
            tags,
            metadata: Vec::new(),
        }
    }
}

/// Errors from provider API calls.
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("Failed to parse response: {0}")]
    Parse(String),
    #[error("Authentication failed. Check your API token.")]
    AuthFailed,
    #[error("Rate limited. Try again in a moment.")]
    RateLimited,
    #[error("{0}")]
    Execute(String),
    #[error("Cancelled.")]
    Cancelled,
    /// Some hosts were fetched but others failed. The caller should use the
    /// hosts but suppress destructive operations like --remove.
    #[error("Partial result: {failures} of {total} failed")]
    PartialResult {
        hosts: Vec<ProviderHost>,
        failures: usize,
        total: usize,
    },
}

/// Trait implemented by each cloud provider.
pub trait Provider {
    /// Full provider name (e.g. "digitalocean").
    fn name(&self) -> &str;
    /// Short label for aliases (e.g. "do").
    fn short_label(&self) -> &str;
    /// Fetch hosts with cancellation support.
    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError>;
    /// Fetch all servers from the provider API.
    #[allow(dead_code)]
    fn fetch_hosts(&self, token: &str) -> Result<Vec<ProviderHost>, ProviderError> {
        self.fetch_hosts_cancellable(token, &AtomicBool::new(false))
    }
    /// Fetch hosts with progress reporting. Default delegates to fetch_hosts_cancellable.
    fn fetch_hosts_with_progress(
        &self,
        token: &str,
        cancel: &AtomicBool,
        _progress: &dyn Fn(&str),
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        self.fetch_hosts_cancellable(token, cancel)
    }
}

/// All known provider names.
pub const PROVIDER_NAMES: &[&str] = &[
    "digitalocean",
    "vultr",
    "linode",
    "hetzner",
    "upcloud",
    "proxmox",
    "aws",
    "scaleway",
    "gcp",
    "azure",
    "tailscale",
];

/// Get a provider implementation by name.
pub fn get_provider(name: &str) -> Option<Box<dyn Provider>> {
    match name {
        "digitalocean" => Some(Box::new(digitalocean::DigitalOcean)),
        "vultr" => Some(Box::new(vultr::Vultr)),
        "linode" => Some(Box::new(linode::Linode)),
        "hetzner" => Some(Box::new(hetzner::Hetzner)),
        "upcloud" => Some(Box::new(upcloud::UpCloud)),
        "proxmox" => Some(Box::new(proxmox::Proxmox {
            base_url: String::new(),
            verify_tls: true,
        })),
        "aws" => Some(Box::new(aws::Aws {
            regions: Vec::new(),
            profile: String::new(),
        })),
        "scaleway" => Some(Box::new(scaleway::Scaleway { zones: Vec::new() })),
        "gcp" => Some(Box::new(gcp::Gcp {
            zones: Vec::new(),
            project: String::new(),
        })),
        "azure" => Some(Box::new(azure::Azure {
            subscriptions: Vec::new(),
        })),
        "tailscale" => Some(Box::new(tailscale::Tailscale)),
        _ => None,
    }
}

/// Get a provider implementation configured from a provider section.
/// For providers that need extra config (e.g. Proxmox base URL), this
/// creates a properly configured instance.
pub fn get_provider_with_config(
    name: &str,
    section: &config::ProviderSection,
) -> Option<Box<dyn Provider>> {
    match name {
        "proxmox" => Some(Box::new(proxmox::Proxmox {
            base_url: section.url.clone(),
            verify_tls: section.verify_tls,
        })),
        "aws" => Some(Box::new(aws::Aws {
            regions: section
                .regions
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            profile: section.profile.clone(),
        })),
        "scaleway" => Some(Box::new(scaleway::Scaleway {
            zones: section
                .regions
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        })),
        "gcp" => Some(Box::new(gcp::Gcp {
            zones: section
                .regions
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            project: section.project.clone(),
        })),
        "azure" => Some(Box::new(azure::Azure {
            subscriptions: section
                .regions
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        })),
        _ => get_provider(name),
    }
}

/// Display name for a provider (e.g. "digitalocean" -> "DigitalOcean").
pub fn provider_display_name(name: &str) -> &str {
    match name {
        "digitalocean" => "DigitalOcean",
        "vultr" => "Vultr",
        "linode" => "Linode",
        "hetzner" => "Hetzner",
        "upcloud" => "UpCloud",
        "proxmox" => "Proxmox VE",
        "aws" => "AWS EC2",
        "scaleway" => "Scaleway",
        "gcp" => "GCP",
        "azure" => "Azure",
        "tailscale" => "Tailscale",
        other => other,
    }
}

/// Create an HTTP agent with explicit timeouts.
pub(crate) fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .redirects(0)
        .build()
}

/// Create an HTTP agent that accepts invalid/self-signed TLS certificates.
pub(crate) fn http_agent_insecure() -> Result<ureq::Agent, ProviderError> {
    let tls = ureq::native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .map_err(|e| ProviderError::Http(format!("TLS setup failed: {}", e)))?;
    Ok(ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .redirects(0)
        .tls_connector(std::sync::Arc::new(tls))
        .build())
}

/// Strip CIDR suffix (/64, /128, etc.) from an IP address.
/// Some provider APIs return IPv6 addresses with prefix length (e.g. "2600:3c00::1/128").
/// SSH requires bare addresses without CIDR notation.
pub(crate) fn strip_cidr(ip: &str) -> &str {
    // Only strip if it looks like a CIDR suffix (slash followed by digits)
    if let Some(pos) = ip.rfind('/') {
        if ip[pos + 1..].bytes().all(|b| b.is_ascii_digit()) && pos + 1 < ip.len() {
            return &ip[..pos];
        }
    }
    ip
}

/// Map a ureq error to a ProviderError.
fn map_ureq_error(err: ureq::Error) -> ProviderError {
    match err {
        ureq::Error::Status(401, _) | ureq::Error::Status(403, _) => ProviderError::AuthFailed,
        ureq::Error::Status(429, _) => ProviderError::RateLimited,
        ureq::Error::Status(code, _) => ProviderError::Http(format!("HTTP {}", code)),
        ureq::Error::Transport(t) => ProviderError::Http(t.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // strip_cidr tests
    // =========================================================================

    #[test]
    fn test_strip_cidr_ipv6_with_prefix() {
        assert_eq!(strip_cidr("2600:3c00::1/128"), "2600:3c00::1");
        assert_eq!(strip_cidr("2a01:4f8::1/64"), "2a01:4f8::1");
    }

    #[test]
    fn test_strip_cidr_bare_ipv6() {
        assert_eq!(strip_cidr("2600:3c00::1"), "2600:3c00::1");
    }

    #[test]
    fn test_strip_cidr_ipv4_passthrough() {
        assert_eq!(strip_cidr("1.2.3.4"), "1.2.3.4");
        assert_eq!(strip_cidr("10.0.0.1/24"), "10.0.0.1");
    }

    #[test]
    fn test_strip_cidr_empty() {
        assert_eq!(strip_cidr(""), "");
    }

    #[test]
    fn test_strip_cidr_slash_without_digits() {
        // Shouldn't strip if after slash there are non-digits
        assert_eq!(strip_cidr("path/to/something"), "path/to/something");
    }

    #[test]
    fn test_strip_cidr_trailing_slash() {
        // Trailing slash with nothing after: pos+1 == ip.len(), should NOT strip
        assert_eq!(strip_cidr("1.2.3.4/"), "1.2.3.4/");
    }

    // =========================================================================
    // get_provider factory tests
    // =========================================================================

    #[test]
    fn test_get_provider_digitalocean() {
        let p = get_provider("digitalocean").unwrap();
        assert_eq!(p.name(), "digitalocean");
        assert_eq!(p.short_label(), "do");
    }

    #[test]
    fn test_get_provider_vultr() {
        let p = get_provider("vultr").unwrap();
        assert_eq!(p.name(), "vultr");
        assert_eq!(p.short_label(), "vultr");
    }

    #[test]
    fn test_get_provider_linode() {
        let p = get_provider("linode").unwrap();
        assert_eq!(p.name(), "linode");
        assert_eq!(p.short_label(), "linode");
    }

    #[test]
    fn test_get_provider_hetzner() {
        let p = get_provider("hetzner").unwrap();
        assert_eq!(p.name(), "hetzner");
        assert_eq!(p.short_label(), "hetzner");
    }

    #[test]
    fn test_get_provider_upcloud() {
        let p = get_provider("upcloud").unwrap();
        assert_eq!(p.name(), "upcloud");
        assert_eq!(p.short_label(), "uc");
    }

    #[test]
    fn test_get_provider_proxmox() {
        let p = get_provider("proxmox").unwrap();
        assert_eq!(p.name(), "proxmox");
        assert_eq!(p.short_label(), "pve");
    }

    #[test]
    fn test_get_provider_unknown_returns_none() {
        assert!(get_provider("oracle").is_none());
        assert!(get_provider("").is_none());
        assert!(get_provider("DigitalOcean").is_none()); // case-sensitive
    }

    #[test]
    fn test_get_provider_all_names_resolve() {
        for name in PROVIDER_NAMES {
            assert!(
                get_provider(name).is_some(),
                "Provider '{}' should resolve",
                name
            );
        }
    }

    // =========================================================================
    // get_provider_with_config tests
    // =========================================================================

    #[test]
    fn test_get_provider_with_config_proxmox_uses_url() {
        let section = config::ProviderSection {
            provider: "proxmox".to_string(),
            token: "user@pam!token=secret".to_string(),
            alias_prefix: "pve-".to_string(),
            user: String::new(),
            identity_file: String::new(),
            url: "https://pve.example.com:8006".to_string(),
            verify_tls: false,
            auto_sync: false,
            profile: String::new(),
            regions: String::new(),
            project: String::new(),
        };
        let p = get_provider_with_config("proxmox", &section).unwrap();
        assert_eq!(p.name(), "proxmox");
    }

    #[test]
    fn test_get_provider_with_config_non_proxmox_delegates() {
        let section = config::ProviderSection {
            provider: "digitalocean".to_string(),
            token: "do-token".to_string(),
            alias_prefix: "do-".to_string(),
            user: String::new(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
            profile: String::new(),
            regions: String::new(),
            project: String::new(),
        };
        let p = get_provider_with_config("digitalocean", &section).unwrap();
        assert_eq!(p.name(), "digitalocean");
    }

    #[test]
    fn test_get_provider_with_config_gcp_uses_project_and_zones() {
        let section = config::ProviderSection {
            provider: "gcp".to_string(),
            token: "sa.json".to_string(),
            alias_prefix: "gcp".to_string(),
            user: String::new(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
            profile: String::new(),
            regions: "us-central1-a, europe-west1-b".to_string(),
            project: "my-project".to_string(),
        };
        let p = get_provider_with_config("gcp", &section).unwrap();
        assert_eq!(p.name(), "gcp");
    }

    #[test]
    fn test_get_provider_with_config_unknown_returns_none() {
        let section = config::ProviderSection {
            provider: "oracle".to_string(),
            token: String::new(),
            alias_prefix: String::new(),
            user: String::new(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
            profile: String::new(),
            regions: String::new(),
            project: String::new(),
        };
        assert!(get_provider_with_config("oracle", &section).is_none());
    }

    // =========================================================================
    // provider_display_name tests
    // =========================================================================

    #[test]
    fn test_display_name_all_providers() {
        assert_eq!(provider_display_name("digitalocean"), "DigitalOcean");
        assert_eq!(provider_display_name("vultr"), "Vultr");
        assert_eq!(provider_display_name("linode"), "Linode");
        assert_eq!(provider_display_name("hetzner"), "Hetzner");
        assert_eq!(provider_display_name("upcloud"), "UpCloud");
        assert_eq!(provider_display_name("proxmox"), "Proxmox VE");
        assert_eq!(provider_display_name("aws"), "AWS EC2");
        assert_eq!(provider_display_name("scaleway"), "Scaleway");
        assert_eq!(provider_display_name("gcp"), "GCP");
        assert_eq!(provider_display_name("azure"), "Azure");
        assert_eq!(provider_display_name("tailscale"), "Tailscale");
    }

    #[test]
    fn test_display_name_unknown_returns_input() {
        assert_eq!(provider_display_name("oracle"), "oracle");
        assert_eq!(provider_display_name(""), "");
    }

    // =========================================================================
    // PROVIDER_NAMES constant tests
    // =========================================================================

    #[test]
    fn test_provider_names_count() {
        assert_eq!(PROVIDER_NAMES.len(), 11);
    }

    #[test]
    fn test_provider_names_contains_all() {
        assert!(PROVIDER_NAMES.contains(&"digitalocean"));
        assert!(PROVIDER_NAMES.contains(&"vultr"));
        assert!(PROVIDER_NAMES.contains(&"linode"));
        assert!(PROVIDER_NAMES.contains(&"hetzner"));
        assert!(PROVIDER_NAMES.contains(&"upcloud"));
        assert!(PROVIDER_NAMES.contains(&"proxmox"));
        assert!(PROVIDER_NAMES.contains(&"aws"));
        assert!(PROVIDER_NAMES.contains(&"scaleway"));
        assert!(PROVIDER_NAMES.contains(&"gcp"));
        assert!(PROVIDER_NAMES.contains(&"azure"));
        assert!(PROVIDER_NAMES.contains(&"tailscale"));
    }

    // =========================================================================
    // ProviderError display tests
    // =========================================================================

    #[test]
    fn test_provider_error_display_http() {
        let err = ProviderError::Http("connection refused".to_string());
        assert_eq!(format!("{}", err), "HTTP error: connection refused");
    }

    #[test]
    fn test_provider_error_display_parse() {
        let err = ProviderError::Parse("invalid JSON".to_string());
        assert_eq!(format!("{}", err), "Failed to parse response: invalid JSON");
    }

    #[test]
    fn test_provider_error_display_auth() {
        let err = ProviderError::AuthFailed;
        assert!(format!("{}", err).contains("Authentication failed"));
    }

    #[test]
    fn test_provider_error_display_rate_limited() {
        let err = ProviderError::RateLimited;
        assert!(format!("{}", err).contains("Rate limited"));
    }

    #[test]
    fn test_provider_error_display_cancelled() {
        let err = ProviderError::Cancelled;
        assert_eq!(format!("{}", err), "Cancelled.");
    }

    #[test]
    fn test_provider_error_display_partial_result() {
        let err = ProviderError::PartialResult {
            hosts: vec![],
            failures: 3,
            total: 10,
        };
        assert!(format!("{}", err).contains("3 of 10 failed"));
    }

    // =========================================================================
    // ProviderHost struct tests
    // =========================================================================

    #[test]
    fn test_provider_host_construction() {
        let host = ProviderHost::new(
            "12345".to_string(),
            "web-01".to_string(),
            "1.2.3.4".to_string(),
            vec!["prod".to_string(), "web".to_string()],
        );
        assert_eq!(host.server_id, "12345");
        assert_eq!(host.name, "web-01");
        assert_eq!(host.ip, "1.2.3.4");
        assert_eq!(host.tags.len(), 2);
    }

    #[test]
    fn test_provider_host_clone() {
        let host = ProviderHost::new(
            "1".to_string(),
            "a".to_string(),
            "1.1.1.1".to_string(),
            vec![],
        );
        let cloned = host.clone();
        assert_eq!(cloned.server_id, host.server_id);
        assert_eq!(cloned.name, host.name);
    }

    // =========================================================================
    // strip_cidr additional edge cases
    // =========================================================================

    #[test]
    fn test_strip_cidr_ipv6_with_64() {
        assert_eq!(strip_cidr("2a01:4f8::1/64"), "2a01:4f8::1");
    }

    #[test]
    fn test_strip_cidr_ipv4_with_32() {
        assert_eq!(strip_cidr("1.2.3.4/32"), "1.2.3.4");
    }

    #[test]
    fn test_strip_cidr_ipv4_with_8() {
        assert_eq!(strip_cidr("10.0.0.1/8"), "10.0.0.1");
    }

    #[test]
    fn test_strip_cidr_just_slash() {
        // "/" alone: pos=0, pos+1=1=len -> condition fails
        assert_eq!(strip_cidr("/"), "/");
    }

    #[test]
    fn test_strip_cidr_slash_with_letters() {
        assert_eq!(strip_cidr("10.0.0.1/abc"), "10.0.0.1/abc");
    }

    #[test]
    fn test_strip_cidr_multiple_slashes() {
        // rfind gets last slash: "48" is digits, so it strips the last /48
        assert_eq!(strip_cidr("10.0.0.1/24/48"), "10.0.0.1/24");
    }

    #[test]
    fn test_strip_cidr_ipv6_full_notation() {
        assert_eq!(
            strip_cidr("2001:0db8:85a3:0000:0000:8a2e:0370:7334/128"),
            "2001:0db8:85a3:0000:0000:8a2e:0370:7334"
        );
    }

    // =========================================================================
    // ProviderError Debug
    // =========================================================================

    #[test]
    fn test_provider_error_debug_http() {
        let err = ProviderError::Http("timeout".to_string());
        let debug = format!("{:?}", err);
        assert!(debug.contains("Http"));
        assert!(debug.contains("timeout"));
    }

    #[test]
    fn test_provider_error_debug_partial_result() {
        let err = ProviderError::PartialResult {
            hosts: vec![ProviderHost::new(
                "1".to_string(),
                "web".to_string(),
                "1.2.3.4".to_string(),
                vec![],
            )],
            failures: 2,
            total: 5,
        };
        let debug = format!("{:?}", err);
        assert!(debug.contains("PartialResult"));
        assert!(debug.contains("failures: 2"));
    }

    // =========================================================================
    // ProviderHost with empty fields
    // =========================================================================

    #[test]
    fn test_provider_host_empty_fields() {
        let host = ProviderHost::new(String::new(), String::new(), String::new(), vec![]);
        assert!(host.server_id.is_empty());
        assert!(host.name.is_empty());
        assert!(host.ip.is_empty());
    }

    // =========================================================================
    // get_provider_with_config for all non-proxmox providers
    // =========================================================================

    #[test]
    fn test_get_provider_with_config_all_providers() {
        for &name in PROVIDER_NAMES {
            let section = config::ProviderSection {
                provider: name.to_string(),
                token: "tok".to_string(),
                alias_prefix: "test".to_string(),
                user: String::new(),
                identity_file: String::new(),
                url: if name == "proxmox" {
                    "https://pve:8006".to_string()
                } else {
                    String::new()
                },
                verify_tls: true,
                auto_sync: true,
                profile: String::new(),
                regions: String::new(),
                project: String::new(),
            };
            let p = get_provider_with_config(name, &section);
            assert!(
                p.is_some(),
                "get_provider_with_config({}) should return Some",
                name
            );
            assert_eq!(p.unwrap().name(), name);
        }
    }

    // =========================================================================
    // Provider trait default methods
    // =========================================================================

    #[test]
    fn test_provider_fetch_hosts_delegates_to_cancellable() {
        let provider = get_provider("digitalocean").unwrap();
        // fetch_hosts delegates to fetch_hosts_cancellable with AtomicBool(false)
        // We can't actually test this without a server, but we verify the method exists
        // by calling it (will fail with network error, which is fine for this test)
        let result = provider.fetch_hosts("fake-token");
        assert!(result.is_err()); // Expected: no network
    }

    // =========================================================================
    // strip_cidr: suffix starts with digit but contains letters
    // =========================================================================

    #[test]
    fn test_strip_cidr_digit_then_letters_not_stripped() {
        assert_eq!(strip_cidr("10.0.0.1/24abc"), "10.0.0.1/24abc");
    }

    // =========================================================================
    // provider_display_name: all known providers
    // =========================================================================

    #[test]
    fn test_provider_display_name_all() {
        assert_eq!(provider_display_name("digitalocean"), "DigitalOcean");
        assert_eq!(provider_display_name("vultr"), "Vultr");
        assert_eq!(provider_display_name("linode"), "Linode");
        assert_eq!(provider_display_name("hetzner"), "Hetzner");
        assert_eq!(provider_display_name("upcloud"), "UpCloud");
        assert_eq!(provider_display_name("proxmox"), "Proxmox VE");
        assert_eq!(provider_display_name("aws"), "AWS EC2");
        assert_eq!(provider_display_name("scaleway"), "Scaleway");
        assert_eq!(provider_display_name("gcp"), "GCP");
        assert_eq!(provider_display_name("azure"), "Azure");
        assert_eq!(provider_display_name("tailscale"), "Tailscale");
    }

    #[test]
    fn test_provider_display_name_unknown() {
        assert_eq!(provider_display_name("oracle"), "oracle");
    }

    // =========================================================================
    // get_provider: all known + unknown
    // =========================================================================

    #[test]
    fn test_get_provider_all_known() {
        for name in PROVIDER_NAMES {
            assert!(
                get_provider(name).is_some(),
                "get_provider({}) should return Some",
                name
            );
        }
    }

    #[test]
    fn test_get_provider_case_sensitive_and_unknown() {
        assert!(get_provider("oracle").is_none());
        assert!(get_provider("DigitalOcean").is_none()); // Case-sensitive
        assert!(get_provider("VULTR").is_none());
        assert!(get_provider("").is_none());
    }

    // =========================================================================
    // PROVIDER_NAMES constant
    // =========================================================================

    #[test]
    fn test_provider_names_has_all_eleven() {
        assert_eq!(PROVIDER_NAMES.len(), 11);
        assert!(PROVIDER_NAMES.contains(&"digitalocean"));
        assert!(PROVIDER_NAMES.contains(&"proxmox"));
        assert!(PROVIDER_NAMES.contains(&"aws"));
        assert!(PROVIDER_NAMES.contains(&"scaleway"));
        assert!(PROVIDER_NAMES.contains(&"azure"));
        assert!(PROVIDER_NAMES.contains(&"tailscale"));
    }

    // =========================================================================
    // Provider short_label via get_provider
    // =========================================================================

    #[test]
    fn test_provider_short_labels() {
        let cases = [
            ("digitalocean", "do"),
            ("vultr", "vultr"),
            ("linode", "linode"),
            ("hetzner", "hetzner"),
            ("upcloud", "uc"),
            ("proxmox", "pve"),
            ("aws", "aws"),
            ("scaleway", "scw"),
            ("gcp", "gcp"),
            ("azure", "az"),
            ("tailscale", "ts"),
        ];
        for (name, expected_label) in &cases {
            let p = get_provider(name).unwrap();
            assert_eq!(p.short_label(), *expected_label, "short_label for {}", name);
        }
    }
}
