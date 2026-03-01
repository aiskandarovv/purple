use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct Hetzner;

#[derive(Deserialize)]
struct HetznerResponse {
    servers: Vec<HetznerServer>,
    meta: HetznerMeta,
}

#[derive(Deserialize)]
struct HetznerServer {
    id: u64,
    name: String,
    public_net: PublicNet,
    #[serde(default)]
    labels: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct PublicNet {
    ipv4: Option<IpInfo>,
    #[serde(default)]
    ipv6: Option<IpInfo>,
}

#[derive(Deserialize)]
struct IpInfo {
    ip: String,
}

#[derive(Deserialize)]
struct HetznerMeta {
    pagination: Pagination,
}

#[derive(Deserialize)]
struct Pagination {
    page: u64,
    last_page: u64,
}

impl Provider for Hetzner {
    fn name(&self) -> &str {
        "hetzner"
    }

    fn short_label(&self) -> &str {
        "hetzner"
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
                "https://api.hetzner.cloud/v1/servers?page={}&per_page=50",
                page
            );
            let resp: HetznerResponse = agent
                .get(&url)
                .set("Authorization", &format!("Bearer {}", token))
                .call()
                .map_err(map_ureq_error)?
                .into_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            if resp.servers.is_empty() {
                break;
            }

            for server in &resp.servers {
                // Prefer public IPv4, fall back to public IPv6
                // IPv6 addresses may include CIDR suffix (e.g. "2a01:4f8::1/64")
                // which must be stripped for SSH compatibility.
                let ip_str = server
                    .public_net
                    .ipv4
                    .as_ref()
                    .filter(|v| !v.ip.is_empty())
                    .map(|v| v.ip.clone())
                    .or_else(|| {
                        server
                            .public_net
                            .ipv6
                            .as_ref()
                            .filter(|v| !v.ip.is_empty())
                            .map(|v| super::strip_cidr(&v.ip).to_string())
                    });
                if let Some(ip) = ip_str {
                    let mut tags: Vec<String> = server
                        .labels
                        .iter()
                        .map(|(k, v)| {
                            if v.is_empty() {
                                k.clone()
                            } else {
                                format!("{}={}", k, v)
                            }
                        })
                        .collect();
                    tags.sort();
                    all_hosts.push(ProviderHost {
                        server_id: server.id.to_string(),
                        name: server.name.clone(),
                        ip,
                        tags,
                    });
                }
            }

            if resp.meta.pagination.page >= resp.meta.pagination.last_page {
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
    fn test_parse_hetzner_response() {
        let json = r#"{
            "servers": [
                {
                    "id": 42,
                    "name": "my-server",
                    "public_net": {
                        "ipv4": {"ip": "1.2.3.4"}
                    },
                    "labels": {"env": "prod", "team": ""}
                },
                {
                    "id": 43,
                    "name": "no-ip",
                    "public_net": {
                        "ipv4": null
                    },
                    "labels": {}
                }
            ],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.servers.len(), 2);
        assert_eq!(resp.servers[0].name, "my-server");
        assert_eq!(resp.servers[0].public_net.ipv4.as_ref().unwrap().ip, "1.2.3.4");
        assert!(resp.servers[1].public_net.ipv4.is_none());
    }

    #[test]
    fn test_ipv6_only_server_uses_v6() {
        let json = r#"{
            "servers": [
                {
                    "id": 44,
                    "name": "v6-only",
                    "public_net": {
                        "ipv4": null,
                        "ipv6": {"ip": "2a01:4f8::1/64"}
                    },
                    "labels": {}
                }
            ],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        let server = &resp.servers[0];
        let ip = server
            .public_net
            .ipv4
            .as_ref()
            .filter(|v| !v.ip.is_empty())
            .map(|v| v.ip.clone())
            .or_else(|| {
                server
                    .public_net
                    .ipv6
                    .as_ref()
                    .filter(|v| !v.ip.is_empty())
                    .map(|v| crate::providers::strip_cidr(&v.ip).to_string())
            });
        // CIDR suffix must be stripped for SSH compatibility
        assert_eq!(ip, Some("2a01:4f8::1".to_string()));
    }
}
