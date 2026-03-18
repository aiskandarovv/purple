use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct Gcp {
    pub zones: Vec<String>,
    pub project: String,
}

/// All GCP Compute Engine zones with display names.
/// Single source of truth. GCP_ZONE_GROUPS references slices of this array.
/// This list only affects the TUI zone picker. Unlisted zones are still synced
/// when no zone filter is configured (empty = all zones).
pub const GCP_ZONES: &[(&str, &str)] = &[
    // US Central (0..4)
    ("us-central1-a", "Iowa A"),
    ("us-central1-b", "Iowa B"),
    ("us-central1-c", "Iowa C"),
    ("us-central1-f", "Iowa F"),
    // US East (4..13)
    ("us-east1-b", "South Carolina B"),
    ("us-east1-c", "South Carolina C"),
    ("us-east1-d", "South Carolina D"),
    ("us-east4-a", "Virginia A"),
    ("us-east4-b", "Virginia B"),
    ("us-east4-c", "Virginia C"),
    ("us-east5-a", "Columbus A"),
    ("us-east5-b", "Columbus B"),
    ("us-east5-c", "Columbus C"),
    // US South (13..16)
    ("us-south1-a", "Dallas A"),
    ("us-south1-b", "Dallas B"),
    ("us-south1-c", "Dallas C"),
    // US West (16..28)
    ("us-west1-a", "Oregon A"),
    ("us-west1-b", "Oregon B"),
    ("us-west1-c", "Oregon C"),
    ("us-west2-a", "Los Angeles A"),
    ("us-west2-b", "Los Angeles B"),
    ("us-west2-c", "Los Angeles C"),
    ("us-west3-a", "Salt Lake City A"),
    ("us-west3-b", "Salt Lake City B"),
    ("us-west3-c", "Salt Lake City C"),
    ("us-west4-a", "Las Vegas A"),
    ("us-west4-b", "Las Vegas B"),
    ("us-west4-c", "Las Vegas C"),
    // North America (28..37)
    ("northamerica-northeast1-a", "Montreal A"),
    ("northamerica-northeast1-b", "Montreal B"),
    ("northamerica-northeast1-c", "Montreal C"),
    ("northamerica-northeast2-a", "Toronto A"),
    ("northamerica-northeast2-b", "Toronto B"),
    ("northamerica-northeast2-c", "Toronto C"),
    ("northamerica-south1-a", "Queretaro A"),
    ("northamerica-south1-b", "Queretaro B"),
    ("northamerica-south1-c", "Queretaro C"),
    // South America (37..43)
    ("southamerica-east1-a", "Sao Paulo A"),
    ("southamerica-east1-b", "Sao Paulo B"),
    ("southamerica-east1-c", "Sao Paulo C"),
    ("southamerica-west1-a", "Santiago A"),
    ("southamerica-west1-b", "Santiago B"),
    ("southamerica-west1-c", "Santiago C"),
    // Europe West (43..70)
    ("europe-west1-b", "Belgium B"),
    ("europe-west1-c", "Belgium C"),
    ("europe-west1-d", "Belgium D"),
    ("europe-west2-a", "London A"),
    ("europe-west2-b", "London B"),
    ("europe-west2-c", "London C"),
    ("europe-west3-a", "Frankfurt A"),
    ("europe-west3-b", "Frankfurt B"),
    ("europe-west3-c", "Frankfurt C"),
    ("europe-west4-a", "Netherlands A"),
    ("europe-west4-b", "Netherlands B"),
    ("europe-west4-c", "Netherlands C"),
    ("europe-west6-a", "Zurich A"),
    ("europe-west6-b", "Zurich B"),
    ("europe-west6-c", "Zurich C"),
    ("europe-west8-a", "Milan A"),
    ("europe-west8-b", "Milan B"),
    ("europe-west8-c", "Milan C"),
    ("europe-west9-a", "Paris A"),
    ("europe-west9-b", "Paris B"),
    ("europe-west9-c", "Paris C"),
    ("europe-west10-a", "Berlin A"),
    ("europe-west10-b", "Berlin B"),
    ("europe-west10-c", "Berlin C"),
    ("europe-west12-a", "Turin A"),
    ("europe-west12-b", "Turin B"),
    ("europe-west12-c", "Turin C"),
    // Europe Other (70..82)
    ("europe-north1-a", "Finland A"),
    ("europe-north1-b", "Finland B"),
    ("europe-north1-c", "Finland C"),
    ("europe-north2-a", "Stockholm A"),
    ("europe-north2-b", "Stockholm B"),
    ("europe-north2-c", "Stockholm C"),
    ("europe-central2-a", "Warsaw A"),
    ("europe-central2-b", "Warsaw B"),
    ("europe-central2-c", "Warsaw C"),
    ("europe-southwest1-a", "Madrid A"),
    ("europe-southwest1-b", "Madrid B"),
    ("europe-southwest1-c", "Madrid C"),
    // Asia East (82..88)
    ("asia-east1-a", "Taiwan A"),
    ("asia-east1-b", "Taiwan B"),
    ("asia-east1-c", "Taiwan C"),
    ("asia-east2-a", "Hong Kong A"),
    ("asia-east2-b", "Hong Kong B"),
    ("asia-east2-c", "Hong Kong C"),
    // Asia Northeast (88..97)
    ("asia-northeast1-a", "Tokyo A"),
    ("asia-northeast1-b", "Tokyo B"),
    ("asia-northeast1-c", "Tokyo C"),
    ("asia-northeast2-a", "Osaka A"),
    ("asia-northeast2-b", "Osaka B"),
    ("asia-northeast2-c", "Osaka C"),
    ("asia-northeast3-a", "Seoul A"),
    ("asia-northeast3-b", "Seoul B"),
    ("asia-northeast3-c", "Seoul C"),
    // Asia South (97..103)
    ("asia-south1-a", "Mumbai A"),
    ("asia-south1-b", "Mumbai B"),
    ("asia-south1-c", "Mumbai C"),
    ("asia-south2-a", "Delhi A"),
    ("asia-south2-b", "Delhi B"),
    ("asia-south2-c", "Delhi C"),
    // Asia Southeast (103..109)
    ("asia-southeast1-a", "Singapore A"),
    ("asia-southeast1-b", "Singapore B"),
    ("asia-southeast1-c", "Singapore C"),
    ("asia-southeast2-a", "Jakarta A"),
    ("asia-southeast2-b", "Jakarta B"),
    ("asia-southeast2-c", "Jakarta C"),
    // Australia (109..115)
    ("australia-southeast1-a", "Sydney A"),
    ("australia-southeast1-b", "Sydney B"),
    ("australia-southeast1-c", "Sydney C"),
    ("australia-southeast2-a", "Melbourne A"),
    ("australia-southeast2-b", "Melbourne B"),
    ("australia-southeast2-c", "Melbourne C"),
    // Middle East (115..124)
    ("me-west1-a", "Tel Aviv A"),
    ("me-west1-b", "Tel Aviv B"),
    ("me-west1-c", "Tel Aviv C"),
    ("me-central1-a", "Doha A"),
    ("me-central1-b", "Doha B"),
    ("me-central1-c", "Doha C"),
    ("me-central2-a", "Dammam A"),
    ("me-central2-b", "Dammam B"),
    ("me-central2-c", "Dammam C"),
    // Africa (124..127)
    ("africa-south1-a", "Johannesburg A"),
    ("africa-south1-b", "Johannesburg B"),
    ("africa-south1-c", "Johannesburg C"),
];

/// Zone group labels with start..end indices into GCP_ZONES.
pub const GCP_ZONE_GROUPS: &[(&str, usize, usize)] = &[
    ("US Central", 0, 4),
    ("US East", 4, 13),
    ("US South", 13, 16),
    ("US West", 16, 28),
    ("North America", 28, 37),
    ("South America", 37, 43),
    ("Europe West", 43, 70),
    ("Europe Other", 70, 82),
    ("Asia East", 82, 88),
    ("Asia Northeast", 88, 97),
    ("Asia South", 97, 103),
    ("Asia Southeast", 103, 109),
    ("Australia", 109, 115),
    ("Middle East", 115, 124),
    ("Africa", 124, 127),
];

// --- Serde response models ---

#[derive(Deserialize)]
struct AggregatedListResponse {
    #[serde(default)]
    items: std::collections::HashMap<String, InstancesScopedList>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct InstancesScopedList {
    #[serde(default)]
    instances: Vec<GcpInstance>,
}

#[derive(Deserialize)]
struct GcpInstance {
    id: String,
    name: String,
    #[serde(default)]
    status: String,
    #[serde(rename = "machineType", default)]
    machine_type: String,
    #[serde(rename = "networkInterfaces", default)]
    network_interfaces: Vec<NetworkInterface>,
    #[serde(default)]
    disks: Vec<Disk>,
    #[serde(default)]
    tags: Option<GcpTags>,
    #[serde(default)]
    labels: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    zone: String,
}

#[derive(Deserialize)]
struct NetworkInterface {
    #[serde(rename = "accessConfigs", default)]
    access_configs: Vec<AccessConfig>,
    #[serde(rename = "networkIP", default)]
    network_ip: String,
    #[serde(rename = "ipv6AccessConfigs", default)]
    ipv6_access_configs: Vec<Ipv6AccessConfig>,
}

#[derive(Deserialize)]
struct AccessConfig {
    #[serde(rename = "natIP", default)]
    nat_ip: String,
}

#[derive(Deserialize)]
struct Ipv6AccessConfig {
    #[serde(rename = "externalIpv6", default)]
    external_ipv6: String,
}

#[derive(Deserialize)]
struct Disk {
    #[serde(default)]
    licenses: Vec<String>,
}

#[derive(Deserialize)]
struct GcpTags {
    #[serde(default)]
    items: Vec<String>,
}

/// Extract the last segment of a URL path (e.g. ".../zones/us-central1-a" -> "us-central1-a").
fn last_url_segment(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or("")
}

/// Select the best IP for an instance.
/// Prefers external (natIP) > internal (networkIP) > external IPv6.
fn select_ip(instance: &GcpInstance) -> Option<String> {
    for ni in &instance.network_interfaces {
        for ac in &ni.access_configs {
            if !ac.nat_ip.is_empty() {
                return Some(ac.nat_ip.clone());
            }
        }
    }
    for ni in &instance.network_interfaces {
        if !ni.network_ip.is_empty() {
            return Some(ni.network_ip.clone());
        }
    }
    for ni in &instance.network_interfaces {
        for v6 in &ni.ipv6_access_configs {
            if !v6.external_ipv6.is_empty() {
                return Some(v6.external_ipv6.clone());
            }
        }
    }
    None
}

/// Build metadata key-value pairs for an instance.
fn build_metadata(instance: &GcpInstance) -> Vec<(String, String)> {
    let mut metadata = Vec::new();
    let zone = last_url_segment(&instance.zone);
    if !zone.is_empty() {
        metadata.push(("region".to_string(), zone.to_string()));
    }
    let machine = last_url_segment(&instance.machine_type);
    if !machine.is_empty() {
        metadata.push(("plan".to_string(), machine.to_string()));
    }
    // OS from first disk's first license
    if let Some(disk) = instance.disks.first() {
        if let Some(license) = disk.licenses.first() {
            let os = last_url_segment(license);
            if !os.is_empty() {
                metadata.push(("os".to_string(), os.to_string()));
            }
        }
    }
    if !instance.status.is_empty() {
        metadata.push(("status".to_string(), instance.status.clone()));
    }
    metadata
}

/// Build tags from GCP tags and labels.
fn build_tags(instance: &GcpInstance) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some(ref t) = instance.tags {
        tags.extend(t.items.clone());
    }
    if let Some(ref labels) = instance.labels {
        for (k, v) in labels {
            if v.is_empty() {
                tags.push(k.clone());
            } else {
                tags.push(format!("{}:{}", k, v));
            }
        }
    }
    tags
}

/// Detect whether a token string is a path to a service account JSON key file.
/// Checks for .json extension (case-insensitive).
fn is_json_key_file(token: &str) -> bool {
    token.to_ascii_lowercase().ends_with(".json")
}

/// Service account key file fields we need.
#[derive(Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: String,
}

/// Create a JWT and exchange it for an access token via Google's OAuth2 endpoint.
fn resolve_service_account_token(path: &str) -> Result<String, ProviderError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ProviderError::Http(format!("Failed to read key file {}: {}", path, e)))?;
    let key: ServiceAccountKey = serde_json::from_str(&content)
        .map_err(|e| ProviderError::Http(format!("Failed to parse key file: {}", e)))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let header = r#"{"alg":"RS256","typ":"JWT"}"#;
    let claims = serde_json::json!({
        "iss": key.client_email,
        "scope": "https://www.googleapis.com/auth/compute.readonly",
        "aud": "https://oauth2.googleapis.com/token",
        "iat": now,
        "exp": now + 3600
    });
    let claims_str = claims.to_string();

    let header_b64 = URL_SAFE_NO_PAD.encode(header.as_bytes());
    let claims_b64 = URL_SAFE_NO_PAD.encode(claims_str.as_bytes());
    let signing_input = format!("{}.{}", header_b64, claims_b64);

    // Parse the PEM private key and sign with RSA-SHA256
    let der = rsa::pkcs8::DecodePrivateKey::from_pkcs8_pem(&key.private_key)
        .map_err(|e| ProviderError::Http(format!("Failed to parse private key: {}", e)))?;
    let signing_key = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(der);
    use rsa::signature::{Signer, SignatureEncoding};
    let signature = signing_key.sign(signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    let jwt = format!("{}.{}", signing_input, sig_b64);

    // Exchange JWT for access token
    let agent = super::http_agent();
    let resp = agent
        .post("https://oauth2.googleapis.com/token")
        .send_form(&[
            ("grant_type", "urn:ietf:params:oauth:grant_type:jwt-bearer"),
            ("assertion", &jwt),
        ])
        .map_err(map_ureq_error)?;

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let token_resp: TokenResponse = resp
        .into_json()
        .map_err(|e| ProviderError::Parse(format!("Token response: {}", e)))?;

    Ok(token_resp.access_token)
}

/// Resolve token: if it's a path to a JSON key file, exchange it for an access token.
/// Otherwise, use it as a raw access token.
fn resolve_token(token: &str) -> Result<String, ProviderError> {
    if is_json_key_file(token) {
        resolve_service_account_token(token)
    } else {
        Ok(token.to_string())
    }
}

/// Percent-encode a page token for use in a URL query parameter.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

impl Provider for Gcp {
    fn name(&self) -> &str {
        "gcp"
    }

    fn short_label(&self) -> &str {
        "gcp"
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
        if self.project.is_empty() {
            return Err(ProviderError::Http(
                "No GCP project configured. Set the Project ID in the provider settings.".to_string(),
            ));
        }

        // Validate project ID format: lowercase letters, digits, hyphens, dots and colons
        // (dots and colons for domain-scoped projects like example.com:my-project)
        if !self.project.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | ':')) {
            return Err(ProviderError::Http(format!(
                "Invalid GCP project ID '{}'. Must contain only lowercase letters, digits, hyphens, dots and colons.",
                self.project
            )));
        }

        progress("Authenticating...");
        let access_token = resolve_token(token)?;

        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }

        let zone_filter: HashSet<&str> = self.zones.iter().map(|s| s.as_str()).collect();
        let agent = super::http_agent();
        let mut all_hosts = Vec::new();
        let mut page_token: Option<String> = None;

        for page in 0u32.. {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            // Safety guard: prevent infinite pagination loops
            if page > 500 {
                break;
            }

            let mut url = format!(
                "https://compute.googleapis.com/compute/v1/projects/{}/aggregated/instances?maxResults=500&returnPartialSuccess=true",
                self.project
            );
            if let Some(ref pt) = page_token {
                url.push_str(&format!("&pageToken={}", url_encode(pt)));
            }

            progress(&format!("Fetching instances ({} so far)...", all_hosts.len()));

            let response = match agent
                .get(&url)
                .set("Authorization", &format!("Bearer {}", access_token))
                .call()
            {
                Ok(r) => r,
                Err(e) => {
                    let err = map_ureq_error(e);
                    // If we already fetched some hosts, return a partial result
                    if !all_hosts.is_empty() {
                        let fetched = all_hosts.len();
                        progress(&format!("{} instances, page {} failed", fetched, page + 1));
                        return Err(ProviderError::PartialResult {
                            hosts: all_hosts,
                            failures: 1,
                            total: page as usize + 1,
                        });
                    }
                    return Err(err);
                }
            };

            let resp: AggregatedListResponse = match response.into_json() {
                Ok(r) => r,
                Err(e) => {
                    if !all_hosts.is_empty() {
                        let fetched = all_hosts.len();
                        progress(&format!("{} instances, page {} failed to parse", fetched, page + 1));
                        return Err(ProviderError::PartialResult {
                            hosts: all_hosts,
                            failures: 1,
                            total: page as usize + 1,
                        });
                    }
                    return Err(ProviderError::Parse(format!("{}", e)));
                }
            };

            for (scope_key, scoped_list) in &resp.items {
                // scope_key is like "zones/us-central1-a"
                let zone = last_url_segment(scope_key);

                // Client-side zone filter (empty = all zones)
                if !zone_filter.is_empty() && !zone_filter.contains(zone) {
                    continue;
                }

                for instance in &scoped_list.instances {
                    if let Some(ip) = select_ip(instance) {
                        all_hosts.push(ProviderHost {
                            server_id: instance.id.clone(),
                            name: instance.name.clone(),
                            ip,
                            tags: build_tags(instance),
                            metadata: build_metadata(instance),
                        });
                    }
                }
            }

            match resp.next_page_token {
                Some(ref t) if !t.is_empty() => page_token = Some(t.clone()),
                _ => break,
            }
        }

        progress(&format!("{} instances", all_hosts.len()));
        Ok(all_hosts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // URL segment extraction
    // =========================================================================

    #[test]
    fn test_last_url_segment() {
        assert_eq!(last_url_segment("projects/my-project/zones/us-central1-a"), "us-central1-a");
        assert_eq!(last_url_segment("projects/p/machineTypes/e2-micro"), "e2-micro");
        assert_eq!(last_url_segment(""), "");
        assert_eq!(last_url_segment("no-slashes"), "no-slashes");
    }

    // =========================================================================
    // Token detection
    // =========================================================================

    #[test]
    fn test_is_json_key_file() {
        assert!(is_json_key_file("/path/to/service-account.json"));
        assert!(is_json_key_file("sa.json"));
        assert!(is_json_key_file("SA.JSON"));
        assert!(is_json_key_file("key.Json"));
        assert!(!is_json_key_file("ya29.some-access-token"));
        assert!(!is_json_key_file(""));
    }

    // =========================================================================
    // URL encoding
    // =========================================================================

    #[test]
    fn test_url_encode_plain() {
        assert_eq!(url_encode("abc123"), "abc123");
    }

    #[test]
    fn test_url_encode_special_chars() {
        assert_eq!(url_encode("a+b=c/d"), "a%2Bb%3Dc%2Fd");
    }

    #[test]
    fn test_url_encode_empty() {
        assert_eq!(url_encode(""), "");
    }

    // =========================================================================
    // Response parsing
    // =========================================================================

    #[test]
    fn test_parse_aggregated_list_response() {
        let json = r#"{
            "items": {
                "zones/us-central1-a": {
                    "instances": [
                        {
                            "id": "1234567890123456789",
                            "name": "web-1",
                            "status": "RUNNING",
                            "machineType": "projects/p/zones/us-central1-a/machineTypes/e2-micro",
                            "zone": "projects/p/zones/us-central1-a",
                            "networkInterfaces": [{
                                "networkIP": "10.0.0.2",
                                "accessConfigs": [{"natIP": "35.192.0.1"}]
                            }],
                            "disks": [{"licenses": ["projects/debian-cloud/global/licenses/debian-11"]}]
                        }
                    ]
                }
            }
        }"#;
        let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
        let instances = &resp.items["zones/us-central1-a"].instances;
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].name, "web-1");
        assert_eq!(instances[0].id, "1234567890123456789");
        assert_eq!(instances[0].status, "RUNNING");
    }

    #[test]
    fn test_parse_empty_zone() {
        let json = r#"{
            "items": {
                "zones/us-east1-b": {
                    "warning": {"code": "NO_RESULTS_ON_PAGE"}
                }
            }
        }"#;
        let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
        let scoped = &resp.items["zones/us-east1-b"];
        assert!(scoped.instances.is_empty());
    }

    #[test]
    fn test_parse_pagination_token() {
        let json = r#"{"items": {}, "nextPageToken": "abc123"}"#;
        let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.next_page_token.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_parse_no_pagination_token() {
        let json = r#"{"items": {}}"#;
        let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
        assert!(resp.next_page_token.is_none());
    }

    #[test]
    fn test_parse_empty_pagination_token() {
        let json = r#"{"items": {}, "nextPageToken": ""}"#;
        let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.next_page_token.as_deref(), Some(""));
        // The fetch loop treats empty string as "no more pages"
    }

    // =========================================================================
    // IP selection
    // =========================================================================

    fn instance_with_ips(nat_ip: &str, network_ip: &str) -> GcpInstance {
        GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![NetworkInterface {
                access_configs: if nat_ip.is_empty() {
                    vec![]
                } else {
                    vec![AccessConfig { nat_ip: nat_ip.to_string() }]
                },
                network_ip: network_ip.to_string(),
                ipv6_access_configs: vec![],
            }],
            disks: vec![],
            tags: None,
            labels: None,
            zone: String::new(),
        }
    }

    #[test]
    fn test_select_ip_prefers_nat() {
        let inst = instance_with_ips("35.192.0.1", "10.0.0.2");
        assert_eq!(select_ip(&inst), Some("35.192.0.1".to_string()));
    }

    #[test]
    fn test_select_ip_falls_back_to_internal() {
        let inst = instance_with_ips("", "10.0.0.2");
        assert_eq!(select_ip(&inst), Some("10.0.0.2".to_string()));
    }

    #[test]
    fn test_select_ip_no_interfaces() {
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![],
            disks: vec![],
            tags: None,
            labels: None,
            zone: String::new(),
        };
        assert_eq!(select_ip(&inst), None);
    }

    #[test]
    fn test_select_ip_empty_network_ip() {
        let inst = instance_with_ips("", "");
        assert_eq!(select_ip(&inst), None);
    }

    #[test]
    fn test_select_ip_multiple_interfaces_cross_interface() {
        // First interface has only internal, second has external
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![
                NetworkInterface {
                    access_configs: vec![],
                    network_ip: "10.0.0.2".to_string(),
                    ipv6_access_configs: vec![],
                },
                NetworkInterface {
                    access_configs: vec![AccessConfig { nat_ip: "35.192.0.1".to_string() }],
                    network_ip: "10.0.1.2".to_string(),
                    ipv6_access_configs: vec![],
                },
            ],
            disks: vec![],
            tags: None,
            labels: None,
            zone: String::new(),
        };
        // Should prefer external IP from second interface over internal from first
        assert_eq!(select_ip(&inst), Some("35.192.0.1".to_string()));
    }

    #[test]
    fn test_select_ip_falls_back_to_ipv6() {
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![NetworkInterface {
                access_configs: vec![],
                network_ip: String::new(),
                ipv6_access_configs: vec![Ipv6AccessConfig {
                    external_ipv6: "2600:1900:4000:318::".to_string(),
                }],
            }],
            disks: vec![],
            tags: None,
            labels: None,
            zone: String::new(),
        };
        assert_eq!(select_ip(&inst), Some("2600:1900:4000:318::".to_string()));
    }

    #[test]
    fn test_select_ip_prefers_ipv4_over_ipv6() {
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![NetworkInterface {
                access_configs: vec![AccessConfig { nat_ip: "35.192.0.1".to_string() }],
                network_ip: "10.0.0.2".to_string(),
                ipv6_access_configs: vec![Ipv6AccessConfig {
                    external_ipv6: "2600:1900:4000:318::".to_string(),
                }],
            }],
            disks: vec![],
            tags: None,
            labels: None,
            zone: String::new(),
        };
        assert_eq!(select_ip(&inst), Some("35.192.0.1".to_string()));
    }

    #[test]
    fn test_select_ip_prefers_internal_over_ipv6() {
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![NetworkInterface {
                access_configs: vec![],
                network_ip: "10.0.0.2".to_string(),
                ipv6_access_configs: vec![Ipv6AccessConfig {
                    external_ipv6: "2600:1900:4000:318::".to_string(),
                }],
            }],
            disks: vec![],
            tags: None,
            labels: None,
            zone: String::new(),
        };
        assert_eq!(select_ip(&inst), Some("10.0.0.2".to_string()));
    }

    #[test]
    fn test_select_ip_ipv6_empty_returns_none() {
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![NetworkInterface {
                access_configs: vec![],
                network_ip: String::new(),
                ipv6_access_configs: vec![Ipv6AccessConfig {
                    external_ipv6: String::new(),
                }],
            }],
            disks: vec![],
            tags: None,
            labels: None,
            zone: String::new(),
        };
        assert_eq!(select_ip(&inst), None);
    }

    #[test]
    fn test_select_ip_ipv6_cross_interface() {
        // First interface has no IPs, second has IPv6
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![
                NetworkInterface {
                    access_configs: vec![],
                    network_ip: String::new(),
                    ipv6_access_configs: vec![],
                },
                NetworkInterface {
                    access_configs: vec![],
                    network_ip: String::new(),
                    ipv6_access_configs: vec![Ipv6AccessConfig {
                        external_ipv6: "2600:1900:4000:318::".to_string(),
                    }],
                },
            ],
            disks: vec![],
            tags: None,
            labels: None,
            zone: String::new(),
        };
        assert_eq!(select_ip(&inst), Some("2600:1900:4000:318::".to_string()));
    }

    // =========================================================================
    // Metadata
    // =========================================================================

    #[test]
    fn test_metadata_full() {
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "web-1".to_string(),
            status: "RUNNING".to_string(),
            machine_type: "projects/p/zones/us-central1-a/machineTypes/e2-micro".to_string(),
            network_interfaces: vec![],
            disks: vec![Disk {
                licenses: vec!["projects/debian-cloud/global/licenses/debian-11".to_string()],
            }],
            tags: None,
            labels: None,
            zone: "projects/p/zones/us-central1-a".to_string(),
        };
        let meta = build_metadata(&inst);
        assert_eq!(meta, vec![
            ("region".to_string(), "us-central1-a".to_string()),
            ("plan".to_string(), "e2-micro".to_string()),
            ("os".to_string(), "debian-11".to_string()),
            ("status".to_string(), "RUNNING".to_string()),
        ]);
    }

    #[test]
    fn test_metadata_empty_fields() {
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "bare".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![],
            disks: vec![],
            tags: None,
            labels: None,
            zone: String::new(),
        };
        let meta = build_metadata(&inst);
        assert!(meta.is_empty());
    }

    #[test]
    fn test_metadata_no_licenses() {
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: "RUNNING".to_string(),
            machine_type: "projects/p/machineTypes/n1-standard-1".to_string(),
            network_interfaces: vec![],
            disks: vec![Disk { licenses: vec![] }],
            tags: None,
            labels: None,
            zone: "projects/p/zones/us-east1-b".to_string(),
        };
        let meta = build_metadata(&inst);
        assert_eq!(meta.len(), 3); // region, plan, status (no os)
        assert!(!meta.iter().any(|(k, _)| k == "os"));
    }

    // =========================================================================
    // Tags from labels and network tags
    // =========================================================================

    #[test]
    fn test_build_tags_from_network_tags() {
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![],
            disks: vec![],
            tags: Some(GcpTags { items: vec!["http-server".to_string(), "https-server".to_string()] }),
            labels: None,
            zone: String::new(),
        };
        let tags = build_tags(&inst);
        assert_eq!(tags, vec!["http-server", "https-server"]);
    }

    #[test]
    fn test_build_tags_from_labels() {
        let mut labels = std::collections::HashMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        labels.insert("team".to_string(), "".to_string());
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![],
            disks: vec![],
            tags: None,
            labels: Some(labels),
            zone: String::new(),
        };
        let tags = build_tags(&inst);
        assert!(tags.contains(&"env:prod".to_string()));
        assert!(tags.contains(&"team".to_string()));
    }

    #[test]
    fn test_build_tags_empty() {
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![],
            disks: vec![],
            tags: None,
            labels: None,
            zone: String::new(),
        };
        assert!(build_tags(&inst).is_empty());
    }

    #[test]
    fn test_build_tags_empty_items_vec() {
        let inst = GcpInstance {
            id: "123".to_string(),
            name: "test".to_string(),
            status: String::new(),
            machine_type: String::new(),
            network_interfaces: vec![],
            disks: vec![],
            tags: Some(GcpTags { items: vec![] }),
            labels: Some(std::collections::HashMap::new()),
            zone: String::new(),
        };
        assert!(build_tags(&inst).is_empty());
    }

    // =========================================================================
    // Zone constants
    // =========================================================================

    #[test]
    fn test_gcp_zones_count() {
        assert_eq!(GCP_ZONES.len(), 127);
    }

    #[test]
    fn test_gcp_zone_groups_cover_all_zones() {
        let total: usize = GCP_ZONE_GROUPS.iter().map(|&(_, s, e)| e - s).sum();
        assert_eq!(total, GCP_ZONES.len());
        let mut expected_start = 0;
        for &(_, start, end) in GCP_ZONE_GROUPS {
            assert_eq!(start, expected_start, "Gap or overlap in zone groups");
            assert!(end > start, "Empty zone group");
            expected_start = end;
        }
        assert_eq!(expected_start, GCP_ZONES.len());
    }

    #[test]
    fn test_gcp_zones_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for (code, _) in GCP_ZONES {
            assert!(seen.insert(code), "Duplicate zone: {}", code);
        }
    }

    #[test]
    fn test_gcp_zones_contains_common() {
        let codes: Vec<&str> = GCP_ZONES.iter().map(|(c, _)| *c).collect();
        assert!(codes.contains(&"us-central1-a"));
        assert!(codes.contains(&"europe-west1-b"));
        assert!(codes.contains(&"asia-east1-a"));
        assert!(codes.contains(&"asia-northeast1-a"));
        assert!(codes.contains(&"asia-south1-a"));
        assert!(codes.contains(&"europe-west4-a"));
        assert!(codes.contains(&"europe-north1-a"));
        assert!(codes.contains(&"me-west1-a"));
        assert!(codes.contains(&"africa-south1-a"));
        assert!(codes.contains(&"australia-southeast2-a"));
    }

    // =========================================================================
    // Project ID validation
    // =========================================================================

    #[test]
    fn test_gcp_valid_project_id() {
        // Valid project IDs should pass validation (will fail at network, not validation)
        let gcp = Gcp {
            zones: vec![],
            project: "my-project-123".to_string(),
        };
        let result = gcp.fetch_hosts("fake-token");
        // Should NOT be a project validation error
        if let Err(ProviderError::Http(msg)) = &result {
            assert!(!msg.contains("Invalid GCP project ID"), "got: {}", msg);
        }
    }

    #[test]
    fn test_gcp_domain_scoped_project_id() {
        let gcp = Gcp {
            zones: vec![],
            project: "example.com:my-project".to_string(),
        };
        let result = gcp.fetch_hosts("fake-token");
        if let Err(ProviderError::Http(msg)) = &result {
            assert!(!msg.contains("Invalid GCP project ID"), "got: {}", msg);
        }
    }

    #[test]
    fn test_gcp_rejects_uppercase_project_id() {
        let gcp = Gcp {
            zones: vec![],
            project: "My-Project".to_string(),
        };
        let result = gcp.fetch_hosts("fake-token");
        match result {
            Err(ProviderError::Http(msg)) => assert!(msg.contains("Invalid GCP project ID")),
            other => panic!("Expected Http error for uppercase project, got: {:?}", other),
        }
    }

    #[test]
    fn test_gcp_rejects_special_chars_in_project_id() {
        let gcp = Gcp {
            zones: vec![],
            project: "my_project".to_string(),
        };
        let result = gcp.fetch_hosts("fake-token");
        match result {
            Err(ProviderError::Http(msg)) => assert!(msg.contains("Invalid GCP project ID")),
            other => panic!("Expected Http error for underscore project, got: {:?}", other),
        }
    }

    #[test]
    fn test_gcp_rejects_space_in_project_id() {
        let gcp = Gcp {
            zones: vec![],
            project: "my project".to_string(),
        };
        let result = gcp.fetch_hosts("fake-token");
        match result {
            Err(ProviderError::Http(msg)) => assert!(msg.contains("Invalid GCP project ID")),
            other => panic!("Expected Http error for space in project, got: {:?}", other),
        }
    }

    // =========================================================================
    // Empty zones accepted (sync all)
    // =========================================================================

    #[test]
    fn test_gcp_empty_zones_accepted() {
        // Empty zones should not cause a validation error (syncs all zones)
        let gcp = Gcp {
            zones: vec![],
            project: "my-project".to_string(),
        };
        let result = gcp.fetch_hosts("fake-token");
        // Should fail at network level, not validation
        if let Err(ProviderError::Http(msg)) = &result {
            assert!(!msg.contains("zone"), "got: {}", msg);
        }
    }

    // =========================================================================
    // Provider trait
    // =========================================================================

    #[test]
    fn test_gcp_provider_name() {
        let gcp = Gcp { zones: vec![], project: String::new() };
        assert_eq!(gcp.name(), "gcp");
        assert_eq!(gcp.short_label(), "gcp");
    }

    #[test]
    fn test_gcp_no_project_error() {
        let gcp = Gcp { zones: vec![], project: String::new() };
        let result = gcp.fetch_hosts("fake-token");
        match result {
            Err(ProviderError::Http(msg)) => assert!(msg.contains("No GCP project")),
            other => panic!("Expected Http error, got: {:?}", other),
        }
    }

    // =========================================================================
    // Instance ID is string-encoded uint64
    // =========================================================================

    #[test]
    fn test_instance_id_is_string() {
        let json = r#"{
            "items": {
                "zones/us-central1-a": {
                    "instances": [{
                        "id": "12345678901234567890",
                        "name": "test",
                        "networkInterfaces": [],
                        "disks": []
                    }]
                }
            }
        }"#;
        let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
        let inst = &resp.items["zones/us-central1-a"].instances[0];
        assert_eq!(inst.id, "12345678901234567890");
    }

    // =========================================================================
    // IPv6 deserialization
    // =========================================================================

    #[test]
    fn test_parse_ipv6_access_configs() {
        let json = r#"{
            "items": {
                "zones/us-central1-a": {
                    "instances": [{
                        "id": "123",
                        "name": "test-ipv6",
                        "networkInterfaces": [{
                            "networkIP": "10.0.0.2",
                            "accessConfigs": [],
                            "ipv6AccessConfigs": [{"externalIpv6": "2600:1900:4000:318::"}]
                        }],
                        "disks": []
                    }]
                }
            }
        }"#;
        let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
        let inst = &resp.items["zones/us-central1-a"].instances[0];
        assert_eq!(inst.network_interfaces[0].ipv6_access_configs.len(), 1);
        assert_eq!(inst.network_interfaces[0].ipv6_access_configs[0].external_ipv6, "2600:1900:4000:318::");
    }

    #[test]
    fn test_parse_missing_ipv6_access_configs() {
        let json = r#"{
            "items": {
                "zones/us-central1-a": {
                    "instances": [{
                        "id": "123",
                        "name": "test-no-ipv6",
                        "networkInterfaces": [{
                            "networkIP": "10.0.0.2",
                            "accessConfigs": [{"natIP": "35.192.0.1"}]
                        }],
                        "disks": []
                    }]
                }
            }
        }"#;
        let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
        let inst = &resp.items["zones/us-central1-a"].instances[0];
        assert!(inst.network_interfaces[0].ipv6_access_configs.is_empty());
    }
}
