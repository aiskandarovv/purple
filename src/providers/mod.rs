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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    fn fetch_hosts_with_progress(
        &self,
        token: &str,
        cancel: &AtomicBool,
        _progress: &dyn Fn(&str),
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        self.fetch_hosts_cancellable(token, cancel)
    }
}

/// Parse a comma-separated provider config field into a list of trimmed,
/// non-empty entries. Used for regions/zones/subscriptions.
fn parse_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Factory for a provider implementation from an optional config section.
/// `None` yields a default-constructed instance; `Some(section)` wires the
/// section's fields into the provider struct.
type ProviderBuild = fn(Option<&config::ProviderSection>) -> Box<dyn Provider>;

/// Static registry entry describing one provider. Adding a provider means
/// adding exactly one `ProviderDescriptor` to `PROVIDERS` below.
pub struct ProviderDescriptor {
    /// Slug used in config files and aliases.
    pub name: &'static str,
    /// Human-readable name shown in the UI.
    pub display: &'static str,
    /// Builder. Must not allocate or fail.
    pub build: ProviderBuild,
}

/// Single source of truth for the provider registry. Adding a new provider
/// means one entry here plus the provider module itself.
pub const PROVIDERS: &[ProviderDescriptor] = &[
    ProviderDescriptor {
        name: "digitalocean",
        display: "DigitalOcean",
        build: |_| Box::new(digitalocean::DigitalOcean),
    },
    ProviderDescriptor {
        name: "vultr",
        display: "Vultr",
        build: |_| Box::new(vultr::Vultr),
    },
    ProviderDescriptor {
        name: "linode",
        display: "Linode",
        build: |_| Box::new(linode::Linode),
    },
    ProviderDescriptor {
        name: "hetzner",
        display: "Hetzner",
        build: |_| Box::new(hetzner::Hetzner),
    },
    ProviderDescriptor {
        name: "upcloud",
        display: "UpCloud",
        build: |_| Box::new(upcloud::UpCloud),
    },
    ProviderDescriptor {
        name: "proxmox",
        display: "Proxmox VE",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(proxmox::Proxmox {
                base_url: s.url,
                verify_tls: s.verify_tls,
            })
        },
    },
    ProviderDescriptor {
        name: "aws",
        display: "AWS EC2",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(aws::Aws {
                regions: parse_csv(&s.regions),
                profile: s.profile,
            })
        },
    },
    ProviderDescriptor {
        name: "scaleway",
        display: "Scaleway",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(scaleway::Scaleway {
                zones: parse_csv(&s.regions),
            })
        },
    },
    ProviderDescriptor {
        name: "gcp",
        display: "GCP",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(gcp::Gcp {
                zones: parse_csv(&s.regions),
                project: s.project,
            })
        },
    },
    ProviderDescriptor {
        name: "azure",
        display: "Azure",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(azure::Azure {
                subscriptions: parse_csv(&s.regions),
            })
        },
    },
    ProviderDescriptor {
        name: "tailscale",
        display: "Tailscale",
        build: |_| Box::new(tailscale::Tailscale),
    },
    ProviderDescriptor {
        name: "oracle",
        display: "Oracle Cloud",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(oracle::Oracle {
                regions: parse_csv(&s.regions),
                compartment: s.compartment,
            })
        },
    },
    ProviderDescriptor {
        name: "ovh",
        display: "OVHcloud",
        // OVH overloads `regions` as the API endpoint (e.g. "ovh-eu").
        // Known quirk flagged in the architecture review; kept as-is to
        // avoid schema migration in this refactor.
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(ovh::Ovh {
                project: s.project,
                endpoint: s.regions,
            })
        },
    },
    ProviderDescriptor {
        name: "leaseweb",
        display: "Leaseweb",
        build: |_| Box::new(leaseweb::Leaseweb),
    },
    ProviderDescriptor {
        name: "i3d",
        display: "i3D.net",
        build: |_| Box::new(i3d::I3d),
    },
    ProviderDescriptor {
        name: "transip",
        display: "TransIP",
        build: |_| Box::new(transip::TransIp),
    },
];

/// Look up a descriptor by name.
fn descriptor(name: &str) -> Option<&'static ProviderDescriptor> {
    PROVIDERS.iter().find(|p| p.name == name)
}

/// All known provider names, in registration order.
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

// Compile-time guard: PROVIDER_NAMES and PROVIDERS must stay in lockstep.
const _: () = {
    assert!(
        PROVIDER_NAMES.len() == PROVIDERS.len(),
        "PROVIDER_NAMES and PROVIDERS length must match",
    );
};

/// Get a provider implementation by name with default configuration.
pub fn get_provider(name: &str) -> Option<Box<dyn Provider>> {
    descriptor(name).map(|d| (d.build)(None))
}

/// Get a provider implementation configured from a provider section.
pub fn get_provider_with_config(
    name: &str,
    section: &config::ProviderSection,
) -> Option<Box<dyn Provider>> {
    descriptor(name).map(|d| (d.build)(Some(section)))
}

/// Display name for a provider (e.g. "digitalocean" -> "DigitalOcean").
pub fn provider_display_name(name: &str) -> &str {
    descriptor(name).map(|d| d.display).unwrap_or(name)
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
#[path = "mod_tests.rs"]
mod tests;
