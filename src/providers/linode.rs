use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct Linode;

#[derive(Deserialize)]
struct LinodeResponse {
    data: Vec<LinodeInstance>,
    page: u64,
    pages: u64,
}

#[derive(Deserialize)]
struct LinodeInstance {
    id: u64,
    label: String,
    #[serde(default)]
    ipv4: Vec<String>,
    #[serde(default)]
    ipv6: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

/// Check if an IP address is in a private/reserved range.
fn is_private_ip(ip: &str) -> bool {
    ip.starts_with("10.")
        || ip.starts_with("192.168.")
        || ip.starts_with("127.")
        || (ip.starts_with("172.")
            && ip
                .split('.')
                .nth(1)
                .and_then(|s| s.parse::<u8>().ok())
                .is_some_and(|n| (16..=31).contains(&n)))
        || (ip.starts_with("100.")
            && ip
                .split('.')
                .nth(1)
                .and_then(|s| s.parse::<u8>().ok())
                .is_some_and(|n| (64..=127).contains(&n)))
}

impl Provider for Linode {
    fn name(&self) -> &str {
        "linode"
    }

    fn short_label(&self) -> &str {
        "linode"
    }

    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        let mut all_hosts = Vec::new();
        let mut page = 1u64;
        let agent = super::http_agent();

        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            let url = format!(
                "https://api.linode.com/v4/linode/instances?page={}&page_size=500",
                page
            );
            let resp: LinodeResponse = agent
                .get(&url)
                .set("Authorization", &format!("Bearer {}", token))
                .call()
                .map_err(map_ureq_error)?
                .into_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            if resp.data.is_empty() {
                break;
            }

            for instance in &resp.data {
                // Prefer public IPv4; fall back to private IPv4, then IPv6
                let ip = instance
                    .ipv4
                    .iter()
                    .find(|ip| !is_private_ip(ip))
                    .or_else(|| instance.ipv4.first())
                    .cloned()
                    .or_else(|| {
                        instance
                            .ipv6
                            .as_ref()
                            .filter(|v| !v.is_empty())
                            .map(|v| super::strip_cidr(v).to_string())
                    });
                if let Some(ip) = ip {
                    if !ip.is_empty() {
                        all_hosts.push(ProviderHost {
                            server_id: instance.id.to_string(),
                            name: instance.label.clone(),
                            ip,
                            tags: instance.tags.clone(),
                        });
                    }
                }
            }

            if resp.page >= resp.pages {
                break;
            }
            page += 1;
            if page > 500 {
                break;
            }
        }

        Ok(all_hosts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_private_ip() {
        assert!(is_private_ip("10.0.0.1"));
        assert!(is_private_ip("192.168.1.1"));
        assert!(is_private_ip("172.16.0.1"));
        assert!(is_private_ip("172.31.255.255"));
        assert!(is_private_ip("100.64.0.1"));
        assert!(is_private_ip("127.0.0.1"));
        assert!(!is_private_ip("1.2.3.4"));
        assert!(!is_private_ip("172.15.0.1"));
        assert!(!is_private_ip("172.32.0.1"));
        assert!(!is_private_ip("100.63.0.1"));
    }

    #[test]
    fn test_parse_linode_prefers_public_ip() {
        let json = r#"{
            "data": [
                {
                    "id": 111,
                    "label": "mixed-ips",
                    "ipv4": ["192.168.1.1", "5.6.7.8"],
                    "tags": []
                }
            ],
            "page": 1,
            "pages": 1
        }"#;
        let resp: LinodeResponse = serde_json::from_str(json).unwrap();
        let instance = &resp.data[0];
        let ip = instance
            .ipv4
            .iter()
            .find(|ip| !is_private_ip(ip))
            .or_else(|| instance.ipv4.first());
        assert_eq!(ip.unwrap(), "5.6.7.8");
    }

    #[test]
    fn test_parse_linode_response() {
        let json = r#"{
            "data": [
                {
                    "id": 111,
                    "label": "app-server",
                    "ipv4": ["9.8.7.6", "192.168.1.1"],
                    "tags": ["production"]
                },
                {
                    "id": 222,
                    "label": "no-ip-server",
                    "ipv4": [],
                    "tags": []
                }
            ],
            "page": 1,
            "pages": 1
        }"#;
        let resp: LinodeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.data[0].label, "app-server");
        assert_eq!(resp.data[0].ipv4[0], "9.8.7.6");
        assert!(resp.data[1].ipv4.is_empty());
    }

    #[test]
    fn test_ipv6_only_instance_uses_v6() {
        let json = r#"{
            "data": [
                {
                    "id": 333,
                    "label": "v6-only",
                    "ipv4": [],
                    "ipv6": "2600:3c00::1/128",
                    "tags": []
                }
            ],
            "page": 1,
            "pages": 1
        }"#;
        let resp: LinodeResponse = serde_json::from_str(json).unwrap();
        let instance = &resp.data[0];
        let ip = instance
            .ipv4
            .iter()
            .find(|ip| !is_private_ip(ip))
            .or_else(|| instance.ipv4.first())
            .cloned()
            .or_else(|| {
                instance
                    .ipv6
                    .as_ref()
                    .filter(|v| !v.is_empty())
                    .map(|v| crate::providers::strip_cidr(v).to_string())
            });
        // CIDR suffix must be stripped for SSH compatibility
        assert_eq!(ip, Some("2600:3c00::1".to_string()));
    }
}
