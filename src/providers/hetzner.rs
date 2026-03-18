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
    #[serde(default)]
    server_type: Option<HetznerServerType>,
    #[serde(default)]
    datacenter: Option<HetznerDatacenter>,
    #[serde(default)]
    location: Option<HetznerLocation>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    image: Option<HetznerImage>,
}

#[derive(Deserialize)]
struct HetznerImage {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct HetznerServerType {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct HetznerDatacenter {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct HetznerLocation {
    #[serde(default)]
    name: String,
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
                    let mut metadata = Vec::new();
                    let region = server
                        .location
                        .as_ref()
                        .map(|l| &l.name)
                        .filter(|n| !n.is_empty())
                        .or_else(|| {
                            server
                                .datacenter
                                .as_ref()
                                .map(|d| &d.name)
                                .filter(|n| !n.is_empty())
                        });
                    if let Some(name) = region {
                        metadata.push(("region".to_string(), name.clone()));
                    }
                    if let Some(ref st) = server.server_type {
                        if !st.name.is_empty() {
                            metadata.push(("plan".to_string(), st.name.clone()));
                        }
                    }
                    if let Some(ref image) = server.image {
                        if let Some(ref name) = image.name {
                            if !name.is_empty() {
                                metadata.push(("os".to_string(), name.clone()));
                            }
                        }
                    }
                    if !server.status.is_empty() {
                        metadata.push(("status".to_string(), server.status.clone()));
                    }
                    all_hosts.push(ProviderHost {
                        server_id: server.id.to_string(),
                        name: server.name.clone(),
                        ip,
                        tags,
                        metadata,
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

    // Helper: same IP selection logic as fetch_hosts_cancellable
    fn select_hetzner_ip(server: &HetznerServer) -> Option<String> {
        server
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
            })
    }

    fn make_hetzner_tags(labels: &std::collections::HashMap<String, String>) -> Vec<String> {
        let mut tags: Vec<String> = labels
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
        tags
    }

    #[test]
    fn test_hetzner_no_ip_skipped() {
        let json = r#"{
            "servers": [{"id": 1, "name": "no-ip", "public_net": {"ipv4": null}, "labels": {}}],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_hetzner_ip(&resp.servers[0]), None);
    }

    #[test]
    fn test_hetzner_empty_ipv4_skipped() {
        let json = r#"{
            "servers": [{"id": 1, "name": "empty", "public_net": {"ipv4": {"ip": ""}}, "labels": {}}],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_hetzner_ip(&resp.servers[0]), None);
    }

    #[test]
    fn test_hetzner_prefers_v4_over_v6() {
        let json = r#"{
            "servers": [{
                "id": 1, "name": "dual",
                "public_net": {"ipv4": {"ip": "1.2.3.4"}, "ipv6": {"ip": "2a01::1/64"}},
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_hetzner_ip(&resp.servers[0]), Some("1.2.3.4".to_string()));
    }

    #[test]
    fn test_hetzner_labels_to_tags_key_value() {
        let mut labels = std::collections::HashMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        labels.insert("team".to_string(), "backend".to_string());
        let tags = make_hetzner_tags(&labels);
        assert!(tags.contains(&"env=prod".to_string()));
        assert!(tags.contains(&"team=backend".to_string()));
    }

    #[test]
    fn test_hetzner_labels_to_tags_empty_value() {
        let mut labels = std::collections::HashMap::new();
        labels.insert("managed".to_string(), String::new());
        let tags = make_hetzner_tags(&labels);
        assert_eq!(tags, vec!["managed"]);
    }

    #[test]
    fn test_hetzner_labels_sorted() {
        let mut labels = std::collections::HashMap::new();
        labels.insert("z-key".to_string(), "val".to_string());
        labels.insert("a-key".to_string(), "val".to_string());
        let tags = make_hetzner_tags(&labels);
        assert_eq!(tags[0], "a-key=val");
        assert_eq!(tags[1], "z-key=val");
    }

    #[test]
    fn test_hetzner_pagination_continues() {
        let json = r#"{
            "servers": [{"id": 1, "name": "a", "public_net": {"ipv4": {"ip": "1.1.1.1"}}, "labels": {}}],
            "meta": {"pagination": {"page": 1, "last_page": 3}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert!(resp.meta.pagination.page < resp.meta.pagination.last_page);
    }

    #[test]
    fn test_hetzner_pagination_stops() {
        let json = r#"{
            "servers": [{"id": 1, "name": "a", "public_net": {"ipv4": {"ip": "1.1.1.1"}}, "labels": {}}],
            "meta": {"pagination": {"page": 3, "last_page": 3}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert!(resp.meta.pagination.page >= resp.meta.pagination.last_page);
    }

    #[test]
    fn test_hetzner_empty_server_list() {
        let json = r#"{
            "servers": [],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert!(resp.servers.is_empty());
    }

    #[test]
    fn test_hetzner_default_labels_empty() {
        let json = r#"{
            "servers": [{"id": 1, "name": "no-labels", "public_net": {"ipv4": {"ip": "1.1.1.1"}}}],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert!(resp.servers[0].labels.is_empty());
    }

    #[test]
    fn test_hetzner_v6_only_fallback() {
        let json = r#"{
            "servers": [{
                "id": 1, "name": "v6-only",
                "public_net": {"ipv4": null, "ipv6": {"ip": "2a01::1/64"}},
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_hetzner_ip(&resp.servers[0]), Some("2a01::1".to_string()));
    }

    #[test]
    fn test_hetzner_v6_without_cidr() {
        let json = r#"{
            "servers": [{
                "id": 1, "name": "v6-bare",
                "public_net": {"ipv4": null, "ipv6": {"ip": "2a01::1"}},
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_hetzner_ip(&resp.servers[0]), Some("2a01::1".to_string()));
    }

    #[test]
    fn test_hetzner_empty_v6_skipped() {
        let json = r#"{
            "servers": [{
                "id": 1, "name": "empty-v6",
                "public_net": {"ipv4": null, "ipv6": {"ip": ""}},
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_hetzner_ip(&resp.servers[0]), None);
    }

    #[test]
    fn test_hetzner_multiple_labels() {
        let mut labels = std::collections::HashMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        labels.insert("app".to_string(), "web".to_string());
        labels.insert("managed".to_string(), String::new());
        let tags = make_hetzner_tags(&labels);
        assert_eq!(tags.len(), 3);
        // Sorted: app=web, env=prod, managed
        assert_eq!(tags[0], "app=web");
        assert_eq!(tags[1], "env=prod");
        assert_eq!(tags[2], "managed");
    }

    #[test]
    fn test_hetzner_null_v6_field() {
        // When ipv6 is completely absent from JSON
        let json = r#"{
            "servers": [{
                "id": 1, "name": "no-v6",
                "public_net": {"ipv4": {"ip": "1.2.3.4"}},
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert!(resp.servers[0].public_net.ipv6.is_none());
        assert_eq!(select_hetzner_ip(&resp.servers[0]), Some("1.2.3.4".to_string()));
    }

    #[test]
    fn test_hetzner_labels_with_special_chars() {
        // Labels can contain equals signs in values (handled by split format)
        let mut labels = std::collections::HashMap::new();
        labels.insert("config".to_string(), "key=val".to_string());
        let tags = make_hetzner_tags(&labels);
        assert_eq!(tags, vec!["config=key=val"]);
    }

    #[test]
    fn test_hetzner_large_id() {
        // Hetzner IDs are u64 and can be large
        let json = r#"{
            "servers": [{
                "id": 99999999999,
                "name": "big-id",
                "public_net": {"ipv4": {"ip": "1.2.3.4"}},
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.servers[0].id, 99999999999);
    }

    #[test]
    fn test_hetzner_empty_ipv4_with_v6_uses_v6() {
        // ipv4 exists but is empty, v6 available
        let json = r#"{
            "servers": [{
                "id": 1, "name": "empty-v4-has-v6",
                "public_net": {"ipv4": {"ip": ""}, "ipv6": {"ip": "2a01::1/64"}},
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_hetzner_ip(&resp.servers[0]), Some("2a01::1".to_string()));
    }

    #[test]
    fn test_hetzner_both_null_skipped() {
        let json = r#"{
            "servers": [{
                "id": 1, "name": "no-ips",
                "public_net": {"ipv4": null, "ipv6": null},
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_hetzner_ip(&resp.servers[0]), None);
    }

    #[test]
    fn test_hetzner_both_empty_skipped() {
        let json = r#"{
            "servers": [{
                "id": 1, "name": "empty-both",
                "public_net": {"ipv4": {"ip": ""}, "ipv6": {"ip": ""}},
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_hetzner_ip(&resp.servers[0]), None);
    }

    #[test]
    fn test_hetzner_many_labels_sorted() {
        let mut labels = std::collections::HashMap::new();
        labels.insert("zzz".to_string(), "last".to_string());
        labels.insert("aaa".to_string(), "first".to_string());
        labels.insert("mmm".to_string(), String::new());
        let tags = make_hetzner_tags(&labels);
        assert_eq!(tags, vec!["aaa=first", "mmm", "zzz=last"]);
    }

    #[test]
    fn test_hetzner_v6_cidr_128() {
        let json = r#"{
            "servers": [{
                "id": 1, "name": "v6-128",
                "public_net": {"ipv4": null, "ipv6": {"ip": "2a01:4f8::1/128"}},
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_hetzner_ip(&resp.servers[0]), Some("2a01:4f8::1".to_string()));
    }

    #[test]
    fn test_hetzner_multiple_servers_parsed() {
        let json = r#"{
            "servers": [
                {"id": 1, "name": "web-1", "public_net": {"ipv4": {"ip": "1.1.1.1"}}, "labels": {"env": "prod"}},
                {"id": 2, "name": "web-2", "public_net": {"ipv4": {"ip": "2.2.2.2"}}, "labels": {"env": "staging"}},
                {"id": 3, "name": "db", "public_net": {"ipv4": null}, "labels": {}}
            ],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.servers.len(), 3);
        assert_eq!(select_hetzner_ip(&resp.servers[0]), Some("1.1.1.1".to_string()));
        assert_eq!(select_hetzner_ip(&resp.servers[1]), Some("2.2.2.2".to_string()));
        assert_eq!(select_hetzner_ip(&resp.servers[2]), None);
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

    // --- label with empty value uses key only ---

    #[test]
    fn test_hetzner_label_empty_value_uses_key() {
        let json = r#"{
            "servers": [{
                "id": 50,
                "name": "label-test",
                "public_net": {"ipv4": {"ip": "1.2.3.4"}},
                "labels": {"env": "", "role": "web"}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        let server = &resp.servers[0];
        let mut tags: Vec<String> = server
            .labels
            .iter()
            .map(|(k, v)| {
                if v.is_empty() { k.clone() } else { format!("{}={}", k, v) }
            })
            .collect();
        tags.sort();
        assert_eq!(tags, vec!["env", "role=web"]);
    }

    // --- server with empty name ---

    #[test]
    fn test_hetzner_empty_name() {
        let json = r#"{
            "servers": [{
                "id": 51,
                "name": "",
                "public_net": {"ipv4": {"ip": "1.2.3.4"}},
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.servers[0].name, "");
    }

    // --- both ipv4 and ipv6 present: prefers ipv4 ---

    #[test]
    fn test_hetzner_dual_stack_prefers_v4() {
        let json = r#"{
            "servers": [{
                "id": 52,
                "name": "dual",
                "public_net": {
                    "ipv4": {"ip": "1.2.3.4"},
                    "ipv6": {"ip": "2a01::1/64"}
                },
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        let server = &resp.servers[0];
        let ip = server
            .public_net
            .ipv4
            .as_ref()
            .filter(|v| !v.ip.is_empty())
            .map(|v| v.ip.clone());
        assert_eq!(ip, Some("1.2.3.4".to_string()));
    }

    // --- Resilience: extra/unknown fields are ignored by serde ---

    #[test]
    fn test_hetzner_extra_fields_ignored() {
        // Real Hetzner API returns many more fields per server
        let json = r#"{
            "servers": [{
                "id": 60,
                "name": "full-response",
                "status": "running",
                "public_net": {
                    "ipv4": {"ip": "1.2.3.4", "blocked": false, "dns_ptr": "1.2.3.4.host.example.com"},
                    "ipv6": {"ip": "2a01:4f8::1/64", "blocked": false, "dns_ptr": []},
                    "floating_ips": [],
                    "firewalls": []
                },
                "server_type": {"id": 1, "name": "cx11", "description": "CX11"},
                "datacenter": {"id": 1, "name": "fsn1-dc14"},
                "image": {"id": 12345, "name": "ubuntu-22.04"},
                "iso": null,
                "rescue_enabled": false,
                "locked": false,
                "backup_window": "22-02",
                "outgoing_traffic": 123456,
                "ingoing_traffic": 654321,
                "included_traffic": 654321000000,
                "protection": {"delete": false, "rebuild": false},
                "labels": {"env": "prod"},
                "volumes": [],
                "load_balancers": [],
                "primary_disk_size": 20,
                "created": "2024-01-01T00:00:00+00:00"
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1, "per_page": 25, "total_entries": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.servers[0].name, "full-response");
        assert_eq!(select_hetzner_ip(&resp.servers[0]), Some("1.2.3.4".to_string()));
        assert_eq!(resp.servers[0].labels.get("env").unwrap(), "prod");
    }

    #[test]
    fn test_hetzner_ipinfo_extra_fields_ignored() {
        // IpInfo may contain blocked, dns_ptr
        let json = r#"{
            "servers": [{
                "id": 61,
                "name": "ip-extra",
                "public_net": {
                    "ipv4": {"ip": "5.6.7.8", "blocked": false, "dns_ptr": "host.example.com"}
                },
                "labels": {}
            }],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.servers[0].public_net.ipv4.as_ref().unwrap().ip, "5.6.7.8");
    }

    #[test]
    fn test_hetzner_pagination_extra_fields_ignored() {
        // Pagination may contain per_page and total_entries
        let json = r#"{
            "servers": [],
            "meta": {"pagination": {"page": 1, "last_page": 1, "per_page": 25, "total_entries": 0, "next_page": null, "previous_page": null}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.meta.pagination.page, 1);
        assert_eq!(resp.meta.pagination.last_page, 1);
    }

    // =========================================================================
    // location preferred over datacenter
    // =========================================================================

    fn select_region(server: &HetznerServer) -> Option<String> {
        server
            .location
            .as_ref()
            .map(|l| &l.name)
            .filter(|n| !n.is_empty())
            .or_else(|| {
                server
                    .datacenter
                    .as_ref()
                    .map(|d| &d.name)
                    .filter(|n| !n.is_empty())
            })
            .cloned()
    }

    #[test]
    fn test_location_preferred_over_datacenter() {
        let json = r#"{
            "servers": [{"id": 1, "name": "a", "public_net": {"ipv4": {"ip": "1.2.3.4"}},
                "labels": {}, "datacenter": {"name": "fsn1-dc14"}, "location": {"name": "fsn1"}}],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_region(&resp.servers[0]), Some("fsn1".to_string()));
    }

    #[test]
    fn test_datacenter_fallback_when_no_location() {
        let json = r#"{
            "servers": [{"id": 1, "name": "a", "public_net": {"ipv4": {"ip": "1.2.3.4"}},
                "labels": {}, "datacenter": {"name": "fsn1-dc14"}}],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_region(&resp.servers[0]), Some("fsn1-dc14".to_string()));
    }

    #[test]
    fn test_empty_location_falls_back_to_datacenter() {
        let json = r#"{
            "servers": [{"id": 1, "name": "a", "public_net": {"ipv4": {"ip": "1.2.3.4"}},
                "labels": {}, "datacenter": {"name": "fsn1-dc14"}, "location": {"name": ""}}],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_region(&resp.servers[0]), Some("fsn1-dc14".to_string()));
    }

    #[test]
    fn test_no_location_no_datacenter() {
        let json = r#"{
            "servers": [{"id": 1, "name": "a", "public_net": {"ipv4": {"ip": "1.2.3.4"}},
                "labels": {}}],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_region(&resp.servers[0]), None);
    }
}
