pub mod aws;
pub mod azure;
pub mod config;
mod digitalocean;
pub mod gcp;
mod hetzner;
mod i3d;
mod leaseweb;
mod linode;
pub mod oracle;
pub mod ovh;
mod proxmox;
pub mod scaleway;
pub mod sync;
mod tailscale;
mod transip;
mod upcloud;
mod vultr;

use std::sync::atomic::AtomicBool;

use log::{error, warn};
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
    "oracle",
    "ovh",
    "leaseweb",
    "i3d",
    "transip",
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
        "oracle" => Some(Box::new(oracle::Oracle {
            regions: Vec::new(),
            compartment: String::new(),
        })),
        "ovh" => Some(Box::new(ovh::Ovh {
            project: String::new(),
            endpoint: String::new(),
        })),
        "leaseweb" => Some(Box::new(leaseweb::Leaseweb)),
        "i3d" => Some(Box::new(i3d::I3d)),
        "transip" => Some(Box::new(transip::TransIp)),
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
        "oracle" => Some(Box::new(oracle::Oracle {
            regions: section
                .regions
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            compartment: section.compartment.clone(),
        })),
        "ovh" => Some(Box::new(ovh::Ovh {
            project: section.project.clone(),
            endpoint: section.regions.clone(),
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
        "oracle" => "Oracle Cloud",
        "ovh" => "OVHcloud",
        "leaseweb" => "Leaseweb",
        "i3d" => "i3D.net",
        "transip" => "TransIP",
        other => other,
    }
}

/// Create an HTTP agent with explicit timeouts.
pub(crate) fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .max_redirects(0)
        .build()
        .new_agent()
}

/// Create an HTTP agent that accepts invalid/self-signed TLS certificates.
pub(crate) fn http_agent_insecure() -> Result<ureq::Agent, ProviderError> {
    Ok(ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .max_redirects(0)
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .disable_verification(true)
                .build(),
        )
        .build()
        .new_agent())
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

/// RFC 3986 percent-encoding for URL query parameters.
/// Encodes all characters except unreserved ones (A-Z, a-z, 0-9, '-', '_', '.', '~').
pub(crate) fn percent_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

/// Date components from a Unix epoch timestamp (no chrono dependency).
pub(crate) struct EpochDate {
    pub year: u64,
    pub month: u64, // 1-based
    pub day: u64,   // 1-based
    pub hours: u64,
    pub minutes: u64,
    pub seconds: u64,
    /// Days since epoch (for weekday calculation)
    pub epoch_days: u64,
}

/// Convert Unix epoch seconds to date components.
pub(crate) fn epoch_to_date(epoch_secs: u64) -> EpochDate {
    let secs_per_day = 86400u64;
    let epoch_days = epoch_secs / secs_per_day;
    let mut remaining_days = epoch_days;
    let day_secs = epoch_secs % secs_per_day;

    let mut year = 1970u64;
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_per_month: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0usize;
    while month < 12 && remaining_days >= days_per_month[month] {
        remaining_days -= days_per_month[month];
        month += 1;
    }

    EpochDate {
        year,
        month: (month + 1) as u64,
        day: remaining_days + 1,
        hours: day_secs / 3600,
        minutes: (day_secs % 3600) / 60,
        seconds: day_secs % 60,
        epoch_days,
    }
}

/// Map a ureq error to a ProviderError.
fn map_ureq_error(err: ureq::Error) -> ProviderError {
    match err {
        ureq::Error::StatusCode(code) => match code {
            401 | 403 => {
                error!("[external] HTTP {code}: authentication failed");
                ProviderError::AuthFailed
            }
            429 => {
                warn!("[external] HTTP 429: rate limited");
                ProviderError::RateLimited
            }
            _ => {
                error!("[external] HTTP {code}");
                ProviderError::Http(format!("HTTP {}", code))
            }
        },
        other => {
            error!("[external] Request failed: {other}");
            ProviderError::Http(other.to_string())
        }
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
    // percent_encode tests
    // =========================================================================

    #[test]
    fn test_percent_encode_unreserved_passthrough() {
        assert_eq!(percent_encode("abc123-_.~"), "abc123-_.~");
    }

    #[test]
    fn test_percent_encode_spaces_and_specials() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
        assert_eq!(percent_encode("a=b&c"), "a%3Db%26c");
        assert_eq!(percent_encode("/path"), "%2Fpath");
    }

    #[test]
    fn test_percent_encode_empty() {
        assert_eq!(percent_encode(""), "");
    }

    #[test]
    fn test_percent_encode_plus_equals_slash() {
        assert_eq!(percent_encode("a+b=c/d"), "a%2Bb%3Dc%2Fd");
    }

    // =========================================================================
    // epoch_to_date tests
    // =========================================================================

    #[test]
    fn test_epoch_to_date_unix_epoch() {
        let d = epoch_to_date(0);
        assert_eq!((d.year, d.month, d.day), (1970, 1, 1));
        assert_eq!((d.hours, d.minutes, d.seconds), (0, 0, 0));
    }

    #[test]
    fn test_epoch_to_date_known_date() {
        // 2024-01-15 12:30:45 UTC = 1705321845
        let d = epoch_to_date(1705321845);
        assert_eq!((d.year, d.month, d.day), (2024, 1, 15));
        assert_eq!((d.hours, d.minutes, d.seconds), (12, 30, 45));
    }

    #[test]
    fn test_epoch_to_date_leap_year() {
        // 2024-02-29 00:00:00 UTC = 1709164800
        let d = epoch_to_date(1709164800);
        assert_eq!((d.year, d.month, d.day), (2024, 2, 29));
    }

    #[test]
    fn test_epoch_to_date_end_of_year() {
        // 2023-12-31 23:59:59 UTC = 1704067199
        let d = epoch_to_date(1704067199);
        assert_eq!((d.year, d.month, d.day), (2023, 12, 31));
        assert_eq!((d.hours, d.minutes, d.seconds), (23, 59, 59));
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
        assert!(get_provider("unknown_provider").is_none());
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
            compartment: String::new(),
            vault_role: String::new(),
            vault_addr: String::new(),
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
            compartment: String::new(),
            vault_role: String::new(),
            vault_addr: String::new(),
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
            compartment: String::new(),
            vault_role: String::new(),
            vault_addr: String::new(),
        };
        let p = get_provider_with_config("gcp", &section).unwrap();
        assert_eq!(p.name(), "gcp");
    }

    #[test]
    fn test_get_provider_with_config_unknown_returns_none() {
        let section = config::ProviderSection {
            provider: "unknown_provider".to_string(),
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
            compartment: String::new(),
            vault_role: String::new(),
            vault_addr: String::new(),
        };
        assert!(get_provider_with_config("unknown_provider", &section).is_none());
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
        assert_eq!(provider_display_name("oracle"), "Oracle Cloud");
        assert_eq!(provider_display_name("ovh"), "OVHcloud");
        assert_eq!(provider_display_name("leaseweb"), "Leaseweb");
        assert_eq!(provider_display_name("i3d"), "i3D.net");
        assert_eq!(provider_display_name("transip"), "TransIP");
    }

    #[test]
    fn test_display_name_unknown_returns_input() {
        assert_eq!(
            provider_display_name("unknown_provider"),
            "unknown_provider"
        );
        assert_eq!(provider_display_name(""), "");
    }

    // =========================================================================
    // PROVIDER_NAMES constant tests
    // =========================================================================

    #[test]
    fn test_provider_names_count() {
        assert_eq!(PROVIDER_NAMES.len(), 16);
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
        assert!(PROVIDER_NAMES.contains(&"oracle"));
        assert!(PROVIDER_NAMES.contains(&"ovh"));
        assert!(PROVIDER_NAMES.contains(&"leaseweb"));
        assert!(PROVIDER_NAMES.contains(&"i3d"));
        assert!(PROVIDER_NAMES.contains(&"transip"));
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
                compartment: String::new(),
                vault_role: String::new(),
                vault_addr: String::new(),
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
        assert_eq!(provider_display_name("oracle"), "Oracle Cloud");
        assert_eq!(provider_display_name("ovh"), "OVHcloud");
        assert_eq!(provider_display_name("leaseweb"), "Leaseweb");
        assert_eq!(provider_display_name("i3d"), "i3D.net");
        assert_eq!(provider_display_name("transip"), "TransIP");
    }

    #[test]
    fn test_provider_display_name_unknown() {
        assert_eq!(
            provider_display_name("unknown_provider"),
            "unknown_provider"
        );
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
        assert!(get_provider("unknown_provider").is_none());
        assert!(get_provider("DigitalOcean").is_none()); // Case-sensitive
        assert!(get_provider("VULTR").is_none());
        assert!(get_provider("").is_none());
    }

    // =========================================================================
    // PROVIDER_NAMES constant
    // =========================================================================

    #[test]
    fn test_provider_names_has_all_sixteen() {
        assert_eq!(PROVIDER_NAMES.len(), 16);
        assert!(PROVIDER_NAMES.contains(&"digitalocean"));
        assert!(PROVIDER_NAMES.contains(&"proxmox"));
        assert!(PROVIDER_NAMES.contains(&"aws"));
        assert!(PROVIDER_NAMES.contains(&"scaleway"));
        assert!(PROVIDER_NAMES.contains(&"azure"));
        assert!(PROVIDER_NAMES.contains(&"tailscale"));
        assert!(PROVIDER_NAMES.contains(&"oracle"));
        assert!(PROVIDER_NAMES.contains(&"ovh"));
        assert!(PROVIDER_NAMES.contains(&"leaseweb"));
        assert!(PROVIDER_NAMES.contains(&"i3d"));
        assert!(PROVIDER_NAMES.contains(&"transip"));
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
            ("oracle", "oci"),
            ("ovh", "ovh"),
            ("leaseweb", "lsw"),
            ("i3d", "i3d"),
            ("transip", "tip"),
        ];
        for (name, expected_label) in &cases {
            let p = get_provider(name).unwrap();
            assert_eq!(p.short_label(), *expected_label, "short_label for {}", name);
        }
    }

    // =========================================================================
    // http_agent construction tests
    // =========================================================================

    #[test]
    fn test_http_agent_creates_agent() {
        // Smoke test: agent construction should not panic
        let _agent = http_agent();
    }

    #[test]
    fn test_http_agent_insecure_creates_agent() {
        // Smoke test: insecure agent construction should succeed
        let agent = http_agent_insecure();
        assert!(agent.is_ok());
    }

    // =========================================================================
    // map_ureq_error tests
    // =========================================================================

    #[test]
    fn test_map_ureq_error_401_is_auth_failed() {
        let err = map_ureq_error(ureq::Error::StatusCode(401));
        assert!(matches!(err, ProviderError::AuthFailed));
    }

    #[test]
    fn test_map_ureq_error_403_is_auth_failed() {
        let err = map_ureq_error(ureq::Error::StatusCode(403));
        assert!(matches!(err, ProviderError::AuthFailed));
    }

    #[test]
    fn test_map_ureq_error_429_is_rate_limited() {
        let err = map_ureq_error(ureq::Error::StatusCode(429));
        assert!(matches!(err, ProviderError::RateLimited));
    }

    #[test]
    fn test_map_ureq_error_500_is_http() {
        let err = map_ureq_error(ureq::Error::StatusCode(500));
        match err {
            ProviderError::Http(msg) => assert_eq!(msg, "HTTP 500"),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_map_ureq_error_404_is_http() {
        let err = map_ureq_error(ureq::Error::StatusCode(404));
        match err {
            ProviderError::Http(msg) => assert_eq!(msg, "HTTP 404"),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_map_ureq_error_502_is_http() {
        let err = map_ureq_error(ureq::Error::StatusCode(502));
        match err {
            ProviderError::Http(msg) => assert_eq!(msg, "HTTP 502"),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_map_ureq_error_503_is_http() {
        let err = map_ureq_error(ureq::Error::StatusCode(503));
        match err {
            ProviderError::Http(msg) => assert_eq!(msg, "HTTP 503"),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_map_ureq_error_200_is_http() {
        // Edge case: 200 should still map (even though it shouldn't occur in practice)
        let err = map_ureq_error(ureq::Error::StatusCode(200));
        match err {
            ProviderError::Http(msg) => assert_eq!(msg, "HTTP 200"),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_map_ureq_error_non_status_is_http() {
        // Transport/other errors should map to Http with a message
        let err = map_ureq_error(ureq::Error::HostNotFound);
        match err {
            ProviderError::Http(msg) => assert!(!msg.is_empty()),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_map_ureq_error_all_auth_codes_covered() {
        // Verify only 401 and 403 produce AuthFailed (not 400, 402, etc.)
        for code in [400, 402, 405, 406, 407, 408, 409, 410] {
            let err = map_ureq_error(ureq::Error::StatusCode(code));
            assert!(
                matches!(err, ProviderError::Http(_)),
                "status {} should be Http, not AuthFailed",
                code
            );
        }
    }

    #[test]
    fn test_map_ureq_error_only_429_is_rate_limited() {
        // Verify only 429 produces RateLimited
        for code in [428, 430, 431] {
            let err = map_ureq_error(ureq::Error::StatusCode(code));
            assert!(
                !matches!(err, ProviderError::RateLimited),
                "status {} should not be RateLimited",
                code
            );
        }
    }

    #[test]
    fn test_map_ureq_error_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let err = map_ureq_error(ureq::Error::Io(io_err));
        match err {
            ProviderError::Http(msg) => assert!(msg.contains("refused"), "got: {}", msg),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_map_ureq_error_timeout() {
        let err = map_ureq_error(ureq::Error::Timeout(ureq::Timeout::Global));
        match err {
            ProviderError::Http(msg) => assert!(!msg.is_empty()),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_map_ureq_error_connection_failed() {
        let err = map_ureq_error(ureq::Error::ConnectionFailed);
        match err {
            ProviderError::Http(msg) => assert!(!msg.is_empty()),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_map_ureq_error_bad_uri() {
        let err = map_ureq_error(ureq::Error::BadUri("no scheme".to_string()));
        match err {
            ProviderError::Http(msg) => assert!(msg.contains("no scheme"), "got: {}", msg),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_map_ureq_error_too_many_redirects() {
        let err = map_ureq_error(ureq::Error::TooManyRedirects);
        match err {
            ProviderError::Http(msg) => assert!(!msg.is_empty()),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_map_ureq_error_redirect_failed() {
        let err = map_ureq_error(ureq::Error::RedirectFailed);
        match err {
            ProviderError::Http(msg) => assert!(!msg.is_empty()),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_map_ureq_error_all_status_codes_1xx_to_5xx() {
        // Exhaustive check: every status code maps to some ProviderError
        for code in [
            100, 200, 201, 301, 302, 400, 401, 403, 404, 429, 500, 502, 503, 504,
        ] {
            let err = map_ureq_error(ureq::Error::StatusCode(code));
            match code {
                401 | 403 => assert!(
                    matches!(err, ProviderError::AuthFailed),
                    "status {} should be AuthFailed",
                    code
                ),
                429 => assert!(
                    matches!(err, ProviderError::RateLimited),
                    "status {} should be RateLimited",
                    code
                ),
                _ => assert!(
                    matches!(err, ProviderError::Http(_)),
                    "status {} should be Http",
                    code
                ),
            }
        }
    }

    // =========================================================================
    // HTTP integration tests (mockito)
    // Verifies end-to-end: agent -> request -> response -> deserialization
    // =========================================================================

    #[test]
    fn test_http_get_json_response() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/test")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"name": "test-server", "id": 42}"#)
            .create();

        let agent = http_agent();
        let mut resp = agent
            .get(&format!("{}/api/test", server.url()))
            .call()
            .unwrap();

        #[derive(serde::Deserialize)]
        struct TestResp {
            name: String,
            id: u32,
        }

        let body: TestResp = resp.body_mut().read_json().unwrap();
        assert_eq!(body.name, "test-server");
        assert_eq!(body.id, 42);
        mock.assert();
    }

    #[test]
    fn test_http_get_with_bearer_header() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/hosts")
            .match_header("Authorization", "Bearer my-secret-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"hosts": []}"#)
            .create();

        let agent = http_agent();
        let resp = agent
            .get(&format!("{}/api/hosts", server.url()))
            .header("Authorization", "Bearer my-secret-token")
            .call();

        assert!(resp.is_ok());
        mock.assert();
    }

    #[test]
    fn test_http_get_with_custom_header() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/servers")
            .match_header("X-Auth-Token", "scw-token-123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"servers": []}"#)
            .create();

        let agent = http_agent();
        let resp = agent
            .get(&format!("{}/api/servers", server.url()))
            .header("X-Auth-Token", "scw-token-123")
            .call();

        assert!(resp.is_ok());
        mock.assert();
    }

    #[test]
    fn test_http_401_maps_to_auth_failed() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/test")
            .with_status(401)
            .with_body("Unauthorized")
            .create();

        let agent = http_agent();
        let err = agent
            .get(&format!("{}/api/test", server.url()))
            .call()
            .unwrap_err();

        let provider_err = map_ureq_error(err);
        assert!(matches!(provider_err, ProviderError::AuthFailed));
        mock.assert();
    }

    #[test]
    fn test_http_403_maps_to_auth_failed() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/test")
            .with_status(403)
            .with_body("Forbidden")
            .create();

        let agent = http_agent();
        let err = agent
            .get(&format!("{}/api/test", server.url()))
            .call()
            .unwrap_err();

        let provider_err = map_ureq_error(err);
        assert!(matches!(provider_err, ProviderError::AuthFailed));
        mock.assert();
    }

    #[test]
    fn test_http_429_maps_to_rate_limited() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/test")
            .with_status(429)
            .with_body("Too Many Requests")
            .create();

        let agent = http_agent();
        let err = agent
            .get(&format!("{}/api/test", server.url()))
            .call()
            .unwrap_err();

        let provider_err = map_ureq_error(err);
        assert!(matches!(provider_err, ProviderError::RateLimited));
        mock.assert();
    }

    #[test]
    fn test_http_500_maps_to_http_error() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/test")
            .with_status(500)
            .with_body("Internal Server Error")
            .create();

        let agent = http_agent();
        let err = agent
            .get(&format!("{}/api/test", server.url()))
            .call()
            .unwrap_err();

        let provider_err = map_ureq_error(err);
        match provider_err {
            ProviderError::Http(msg) => assert_eq!(msg, "HTTP 500"),
            other => panic!("expected Http, got {:?}", other),
        }
        mock.assert();
    }

    #[test]
    fn test_http_post_form_encoding() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/oauth/token")
            .match_header("content-type", "application/x-www-form-urlencoded")
            .match_body(
                "grant_type=client_credentials&client_id=my-app&client_secret=secret123&scope=api",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"access_token": "eyJ.abc.def"}"#)
            .create();

        let agent = http_agent();
        let client_id = "my-app".to_string();
        let client_secret = "secret123".to_string();
        let mut resp = agent
            .post(&format!("{}/oauth/token", server.url()))
            .send_form([
                ("grant_type", "client_credentials"),
                ("client_id", client_id.as_str()),
                ("client_secret", client_secret.as_str()),
                ("scope", "api"),
            ])
            .unwrap();

        #[derive(serde::Deserialize)]
        struct TokenResp {
            access_token: String,
        }

        let body: TokenResp = resp.body_mut().read_json().unwrap();
        assert_eq!(body.access_token, "eyJ.abc.def");
        mock.assert();
    }

    #[test]
    fn test_http_read_to_string() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/xml")
            .with_status(200)
            .with_header("content-type", "text/xml")
            .with_body("<root><item>hello</item></root>")
            .create();

        let agent = http_agent();
        let mut resp = agent
            .get(&format!("{}/api/xml", server.url()))
            .call()
            .unwrap();

        let body = resp.body_mut().read_to_string().unwrap();
        assert_eq!(body, "<root><item>hello</item></root>");
        mock.assert();
    }

    #[test]
    fn test_http_body_reader_with_take() {
        // Simulates the update.rs pattern: body_mut().as_reader().take(N)
        use std::io::Read;

        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/download")
            .with_status(200)
            .with_body("binary-content-here-12345")
            .create();

        let agent = http_agent();
        let mut resp = agent
            .get(&format!("{}/download", server.url()))
            .call()
            .unwrap();

        let mut bytes = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(1_048_576)
            .read_to_end(&mut bytes)
            .unwrap();

        assert_eq!(bytes, b"binary-content-here-12345");
        mock.assert();
    }

    #[test]
    fn test_http_body_reader_take_truncates() {
        // Verify .take() actually limits the read
        use std::io::Read;

        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/large")
            .with_status(200)
            .with_body("abcdefghijklmnopqrstuvwxyz")
            .create();

        let agent = http_agent();
        let mut resp = agent
            .get(&format!("{}/large", server.url()))
            .call()
            .unwrap();

        let mut bytes = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(10) // Only read 10 bytes
            .read_to_end(&mut bytes)
            .unwrap();

        assert_eq!(bytes, b"abcdefghij");
        mock.assert();
    }

    #[test]
    fn test_http_no_redirects() {
        // Verify that our agent does NOT follow redirects (max_redirects=0).
        // In ureq v3, 3xx responses are returned as Ok (not errors) when redirects are disabled.
        // The target endpoint is never hit, proving no redirect was followed.
        let mut server = mockito::Server::new();
        let redirect_mock = server
            .mock("GET", "/redirect")
            .with_status(302)
            .with_header("Location", "/target")
            .create();
        let target_mock = server.mock("GET", "/target").with_status(200).create();

        let agent = http_agent();
        let resp = agent
            .get(&format!("{}/redirect", server.url()))
            .call()
            .unwrap();

        assert_eq!(resp.status(), 302);
        redirect_mock.assert();
        target_mock.expect(0); // Target must NOT have been hit
    }

    #[test]
    fn test_http_invalid_json_returns_parse_error() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/bad")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("this is not json")
            .create();

        let agent = http_agent();
        let mut resp = agent
            .get(&format!("{}/api/bad", server.url()))
            .call()
            .unwrap();

        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct Expected {
            name: String,
        }

        let result: Result<Expected, _> = resp.body_mut().read_json();
        assert!(result.is_err());
        mock.assert();
    }

    #[test]
    fn test_http_empty_json_body_returns_parse_error() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/empty")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("")
            .create();

        let agent = http_agent();
        let mut resp = agent
            .get(&format!("{}/api/empty", server.url()))
            .call()
            .unwrap();

        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct Expected {
            name: String,
        }

        let result: Result<Expected, _> = resp.body_mut().read_json();
        assert!(result.is_err());
        mock.assert();
    }

    #[test]
    fn test_http_multiple_headers() {
        // Simulates AWS pattern: multiple headers on same request
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/aws")
            .match_header("Authorization", "AWS4-HMAC-SHA256 cred=test")
            .match_header("x-amz-date", "20260324T120000Z")
            .with_status(200)
            .with_header("content-type", "text/xml")
            .with_body("<result/>")
            .create();

        let agent = http_agent();
        let mut resp = agent
            .get(&format!("{}/api/aws", server.url()))
            .header("Authorization", "AWS4-HMAC-SHA256 cred=test")
            .header("x-amz-date", "20260324T120000Z")
            .call()
            .unwrap();

        let body = resp.body_mut().read_to_string().unwrap();
        assert_eq!(body, "<result/>");
        mock.assert();
    }

    #[test]
    fn test_http_connection_refused_maps_to_http_error() {
        // Connect to a port that's not listening
        let agent = http_agent();
        let err = agent.get("http://127.0.0.1:1").call().unwrap_err();

        let provider_err = map_ureq_error(err);
        match provider_err {
            ProviderError::Http(msg) => assert!(!msg.is_empty()),
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn test_http_nested_json_deserialization() {
        // Simulates the real provider response pattern with nested structures
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/api/droplets")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "data": [
                    {"id": "1", "name": "web-01", "ip": "1.2.3.4"},
                    {"id": "2", "name": "web-02", "ip": "5.6.7.8"}
                ],
                "meta": {"total": 2}
            }"#,
            )
            .create();

        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct Host {
            id: String,
            name: String,
            ip: String,
        }
        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct Meta {
            total: u32,
        }
        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct Resp {
            data: Vec<Host>,
            meta: Meta,
        }

        let agent = http_agent();
        let mut resp = agent
            .get(&format!("{}/api/droplets", server.url()))
            .call()
            .unwrap();

        let body: Resp = resp.body_mut().read_json().unwrap();
        assert_eq!(body.data.len(), 2);
        assert_eq!(body.data[0].name, "web-01");
        assert_eq!(body.data[1].ip, "5.6.7.8");
        assert_eq!(body.meta.total, 2);
        mock.assert();
    }

    #[test]
    fn test_http_xml_deserialization_with_quick_xml() {
        // Simulates the AWS EC2 pattern: XML response parsed with quick-xml
        let mut server = mockito::Server::new();
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <DescribeInstancesResponse>
                <reservationSet>
                    <item>
                        <instancesSet>
                            <item>
                                <instanceId>i-abc123</instanceId>
                                <instanceState><name>running</name></instanceState>
                            </item>
                        </instancesSet>
                    </item>
                </reservationSet>
            </DescribeInstancesResponse>"#;

        let mock = server
            .mock("GET", "/ec2")
            .with_status(200)
            .with_header("content-type", "text/xml")
            .with_body(xml)
            .create();

        let agent = http_agent();
        let mut resp = agent.get(&format!("{}/ec2", server.url())).call().unwrap();

        let body = resp.body_mut().read_to_string().unwrap();
        // Verify we can parse the XML with quick-xml after reading via ureq v3
        #[derive(serde::Deserialize)]
        struct InstanceState {
            name: String,
        }
        #[derive(serde::Deserialize)]
        struct Instance {
            #[serde(rename = "instanceId")]
            instance_id: String,
            #[serde(rename = "instanceState")]
            instance_state: InstanceState,
        }
        #[derive(serde::Deserialize)]
        struct InstanceSet {
            item: Vec<Instance>,
        }
        #[derive(serde::Deserialize)]
        struct Reservation {
            #[serde(rename = "instancesSet")]
            instances_set: InstanceSet,
        }
        #[derive(serde::Deserialize)]
        struct ReservationSet {
            item: Vec<Reservation>,
        }
        #[derive(serde::Deserialize)]
        struct DescribeResp {
            #[serde(rename = "reservationSet")]
            reservation_set: ReservationSet,
        }

        let parsed: DescribeResp = quick_xml::de::from_str(&body).unwrap();
        assert_eq!(
            parsed.reservation_set.item[0].instances_set.item[0].instance_id,
            "i-abc123"
        );
        assert_eq!(
            parsed.reservation_set.item[0].instances_set.item[0]
                .instance_state
                .name,
            "running"
        );
        mock.assert();
    }
}
