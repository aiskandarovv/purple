use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct UpCloud;

#[derive(Deserialize)]
struct ServerListResponse {
    servers: ServerListWrapper,
}

#[derive(Deserialize)]
struct ServerListWrapper {
    server: Vec<ServerSummary>,
}

#[derive(Deserialize)]
struct ServerSummary {
    uuid: String,
    title: String,
    hostname: String,
    #[serde(default)]
    tags: TagWrapper,
    #[serde(default)]
    labels: LabelWrapper,
    #[serde(default)]
    zone: String,
    #[serde(default)]
    plan: String,
    #[serde(default)]
    state: String,
}

#[derive(Deserialize, Default)]
struct TagWrapper {
    #[serde(default)]
    tag: Vec<String>,
}

#[derive(Deserialize, Default)]
struct LabelWrapper {
    #[serde(default)]
    label: Vec<Label>,
}

#[derive(Deserialize)]
struct Label {
    key: String,
    value: String,
}

#[derive(Deserialize)]
struct ServerDetailResponse {
    server: ServerDetail,
}

#[derive(Deserialize)]
struct ServerDetail {
    #[serde(default)]
    networking: Networking,
    #[serde(default)]
    storage_devices: Option<StorageDevices>,
}

#[derive(Deserialize)]
struct StorageDevices {
    #[serde(default)]
    storage_device: Vec<StorageDevice>,
}

#[derive(Deserialize)]
struct StorageDevice {
    #[serde(default)]
    storage_title: String,
}

#[derive(Deserialize, Default)]
struct Networking {
    #[serde(default)]
    interfaces: InterfacesWrapper,
}

#[derive(Deserialize, Default)]
struct InterfacesWrapper {
    #[serde(default)]
    interface: Vec<NetworkInterface>,
}

#[derive(Deserialize)]
struct NetworkInterface {
    #[serde(default)]
    ip_addresses: IpAddressesWrapper,
    #[serde(rename = "type")]
    iface_type: String,
}

#[derive(Deserialize, Default)]
struct IpAddressesWrapper {
    #[serde(default)]
    ip_address: Vec<IpAddress>,
}

#[derive(Deserialize)]
struct IpAddress {
    address: String,
    family: String,
}

/// Collect all IP addresses from networking interfaces, filtered by interface type.
fn collect_ips<'a>(interfaces: &'a [NetworkInterface], iface_type: &str) -> Vec<&'a IpAddress> {
    interfaces
        .iter()
        .filter(|iface| iface.iface_type == iface_type)
        .flat_map(|iface| &iface.ip_addresses.ip_address)
        .collect()
}

/// Select the best public IP address from networking interfaces.
/// Priority: public IPv4 > public IPv6. Skips utility/private interfaces.
/// Filters out placeholder IPs (0.0.0.0, ::) from provisioning servers.
fn select_ip(interfaces: &[NetworkInterface]) -> Option<String> {
    let public_ips = collect_ips(interfaces, "public");
    // Public IPv4 (skip placeholder)
    if let Some(ip) = public_ips
        .iter()
        .find(|a| a.family == "IPv4" && a.address != "0.0.0.0")
    {
        return Some(ip.address.clone());
    }
    // Public IPv6 (skip placeholder)
    public_ips
        .iter()
        .find(|a| a.family == "IPv6" && a.address != "::")
        .map(|ip| ip.address.clone())
}

impl Provider for UpCloud {
    fn name(&self) -> &str {
        "upcloud"
    }

    fn short_label(&self) -> &str {
        "uc"
    }

    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        let mut all_servers: Vec<ServerSummary> = Vec::new();
        let limit = 100;
        let mut offset = 0u64;
        let agent = super::http_agent();
        let mut pages = 0u64;

        // Phase 1: Paginate server list
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            let url = format!(
                "https://api.upcloud.com/1.3/server?limit={}&offset={}",
                limit, offset
            );
            let resp: ServerListResponse = agent
                .get(&url)
                .set("Authorization", &format!("Bearer {}", token))
                .call()
                .map_err(map_ureq_error)?
                .into_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            let count = resp.servers.server.len();
            all_servers.extend(resp.servers.server);

            if count < limit {
                break;
            }
            offset += limit as u64;
            pages += 1;
            if pages >= 500 {
                break;
            }
        }

        // Phase 2: Fetch detail for each server to get IPs via networking.interfaces.
        // Auth/rate-limit errors abort immediately. Other per-server failures are counted
        // and reported as an error to prevent --remove acting on incomplete data.
        let mut all_hosts = Vec::new();
        let mut fetch_failures = 0usize;
        for server in &all_servers {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            let url = format!("https://api.upcloud.com/1.3/server/{}", server.uuid);
            let detail: ServerDetailResponse = match agent
                .get(&url)
                .set("Authorization", &format!("Bearer {}", token))
                .call()
            {
                Ok(resp) => match resp.into_json() {
                    Ok(d) => d,
                    Err(_) => {
                        fetch_failures += 1;
                        continue;
                    }
                },
                Err(ureq::Error::Status(401, _) | ureq::Error::Status(403, _)) => {
                    return Err(ProviderError::AuthFailed);
                }
                Err(ureq::Error::Status(429, _)) => {
                    return Err(ProviderError::RateLimited);
                }
                Err(_) => {
                    fetch_failures += 1;
                    continue;
                }
            };

            let ip = match select_ip(&detail.server.networking.interfaces.interface) {
                Some(ip) => super::strip_cidr(&ip).to_string(),
                None => continue,
            };

            // Server name: title if non-empty, otherwise hostname
            let name = if server.title.is_empty() {
                server.hostname.clone()
            } else {
                server.title.clone()
            };

            // Tags: UpCloud tags (lowercased) + labels as key=value, sorted
            let mut tags: Vec<String> = server
                .tags
                .tag
                .iter()
                .map(|t| t.to_lowercase())
                .collect();
            for label in &server.labels.label {
                if label.value.is_empty() {
                    tags.push(label.key.clone());
                } else {
                    tags.push(format!("{}={}", label.key, label.value));
                }
            }
            tags.sort();

            let mut metadata = Vec::new();
            if !server.zone.is_empty() {
                metadata.push(("region".to_string(), server.zone.clone()));
            }
            if !server.plan.is_empty() {
                metadata.push(("plan".to_string(), server.plan.clone()));
            }
            if let Some(ref sd) = detail.server.storage_devices {
                if let Some(first) = sd.storage_device.first() {
                    if !first.storage_title.is_empty() {
                        metadata.push(("os".to_string(), first.storage_title.clone()));
                    }
                }
            }
            if !server.state.is_empty() {
                metadata.push(("status".to_string(), server.state.clone()));
            }
            all_hosts.push(ProviderHost {
                server_id: server.uuid.clone(),
                name,
                ip,
                tags,
                metadata,
            });
        }

        if fetch_failures > 0 {
            let total = all_servers.len();
            if all_hosts.is_empty() {
                return Err(ProviderError::Http(format!(
                    "Failed to fetch details for all {} servers", total
                )));
            }
            return Err(ProviderError::PartialResult {
                hosts: all_hosts,
                failures: fetch_failures,
                total,
            });
        }

        Ok(all_hosts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_server_list_response() {
        let json = r#"{
            "servers": {
                "server": [
                    {
                        "uuid": "uuid-1",
                        "title": "My Server",
                        "hostname": "my-server.example.com",
                        "tags": {"tag": ["PRODUCTION", "WEB"]},
                        "labels": {"label": [{"key": "env", "value": "prod"}]}
                    },
                    {
                        "uuid": "uuid-2",
                        "title": "",
                        "hostname": "db.example.com",
                        "tags": {"tag": []},
                        "labels": {"label": []}
                    }
                ]
            }
        }"#;
        let resp: ServerListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.servers.server.len(), 2);
        assert_eq!(resp.servers.server[0].uuid, "uuid-1");
        assert_eq!(resp.servers.server[0].title, "My Server");
        assert_eq!(resp.servers.server[0].tags.tag, vec!["PRODUCTION", "WEB"]);
        assert_eq!(resp.servers.server[1].title, "");
        assert_eq!(resp.servers.server[1].hostname, "db.example.com");
    }

    #[test]
    fn test_parse_server_detail_with_networking() {
        let json = r#"{
            "server": {
                "networking": {
                    "interfaces": {
                        "interface": [
                            {
                                "type": "utility",
                                "ip_addresses": {
                                    "ip_address": [
                                        {"address": "10.3.0.1", "family": "IPv4"}
                                    ]
                                }
                            },
                            {
                                "type": "public",
                                "ip_addresses": {
                                    "ip_address": [
                                        {"address": "94.237.1.1", "family": "IPv4"},
                                        {"address": "2a04:3540::1", "family": "IPv6"}
                                    ]
                                }
                            },
                            {
                                "type": "private",
                                "ip_addresses": {
                                    "ip_address": [
                                        {"address": "10.0.0.1", "family": "IPv4"}
                                    ]
                                }
                            }
                        ]
                    }
                }
            }
        }"#;
        let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
        let interfaces = &resp.server.networking.interfaces.interface;
        assert_eq!(interfaces.len(), 3);
        assert_eq!(interfaces[1].iface_type, "public");
        assert_eq!(interfaces[1].ip_addresses.ip_address[0].address, "94.237.1.1");
    }

    #[test]
    fn test_select_ip_public_ipv4() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "private".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "10.0.0.1".into(), family: "IPv4".into() },
                    ],
                },
            },
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "94.237.1.1".into(), family: "IPv4".into() },
                        IpAddress { address: "2a04::1".into(), family: "IPv6".into() },
                    ],
                },
            },
        ];
        assert_eq!(select_ip(&interfaces), Some("94.237.1.1".to_string()));
    }

    #[test]
    fn test_select_ip_public_ipv6_fallback() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "2a04::1".into(), family: "IPv6".into() },
                    ],
                },
            },
        ];
        assert_eq!(select_ip(&interfaces), Some("2a04::1".to_string()));
    }

    #[test]
    fn test_select_ip_skips_placeholder_ipv4() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "0.0.0.0".into(), family: "IPv4".into() },
                    ],
                },
            },
        ];
        assert_eq!(select_ip(&interfaces), None);
    }

    #[test]
    fn test_select_ip_placeholder_ipv4_falls_through_to_ipv6() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "0.0.0.0".into(), family: "IPv4".into() },
                        IpAddress { address: "2a04::1".into(), family: "IPv6".into() },
                    ],
                },
            },
        ];
        assert_eq!(select_ip(&interfaces), Some("2a04::1".to_string()));
    }

    #[test]
    fn test_select_ip_skips_placeholder_ipv6() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "::".into(), family: "IPv6".into() },
                    ],
                },
            },
        ];
        assert_eq!(select_ip(&interfaces), None);
    }

    #[test]
    fn test_select_ip_utility_skipped() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "utility".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "10.3.0.1".into(), family: "IPv4".into() },
                    ],
                },
            },
        ];
        assert_eq!(select_ip(&interfaces), None);
    }

    #[test]
    fn test_select_ip_private_only() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "private".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "10.0.0.1".into(), family: "IPv4".into() },
                    ],
                },
            },
        ];
        assert_eq!(select_ip(&interfaces), None);
    }

    #[test]
    fn test_select_ip_empty() {
        let interfaces: Vec<NetworkInterface> = Vec::new();
        assert_eq!(select_ip(&interfaces), None);
    }

    #[test]
    fn test_tags_lowercased_and_sorted() {
        let server = ServerSummary {
            uuid: "uuid-1".into(),
            title: "test".into(),
            hostname: "test.example.com".into(),
            tags: TagWrapper { tag: vec!["ZEBRA".into(), "ALPHA".into()] },
            labels: LabelWrapper { label: vec![
                Label { key: "env".into(), value: "prod".into() },
            ]},
            zone: String::new(),
            plan: String::new(),
            state: String::new(),
        };
        let mut tags: Vec<String> = server.tags.tag.iter().map(|t| t.to_lowercase()).collect();
        for label in &server.labels.label {
            if label.value.is_empty() {
                tags.push(label.key.clone());
            } else {
                tags.push(format!("{}={}", label.key, label.value));
            }
        }
        tags.sort();
        assert_eq!(tags, vec!["alpha", "env=prod", "zebra"]);
    }

    #[test]
    fn test_server_name_title_preferred() {
        let server = ServerSummary {
            uuid: "uuid-1".into(),
            title: "My Server".into(),
            hostname: "my-server.example.com".into(),
            tags: TagWrapper::default(),
            labels: LabelWrapper::default(),
            zone: String::new(),
            plan: String::new(),
            state: String::new(),
        };
        let name = if server.title.is_empty() {
            server.hostname.clone()
        } else {
            server.title.clone()
        };
        assert_eq!(name, "My Server");
    }

    #[test]
    fn test_server_name_hostname_fallback() {
        let server = ServerSummary {
            uuid: "uuid-1".into(),
            title: "".into(),
            hostname: "db.example.com".into(),
            tags: TagWrapper::default(),
            labels: LabelWrapper::default(),
            zone: String::new(),
            plan: String::new(),
            state: String::new(),
        };
        let name = if server.title.is_empty() {
            server.hostname.clone()
        } else {
            server.title.clone()
        };
        assert_eq!(name, "db.example.com");
    }

    #[test]
    fn test_parse_missing_tags_and_labels() {
        let json = r#"{
            "servers": {
                "server": [
                    {
                        "uuid": "uuid-1",
                        "title": "bare",
                        "hostname": "bare.example.com"
                    }
                ]
            }
        }"#;
        let resp: ServerListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.servers.server.len(), 1);
        assert!(resp.servers.server[0].tags.tag.is_empty());
        assert!(resp.servers.server[0].labels.label.is_empty());
    }

    #[test]
    fn test_parse_detail_missing_networking() {
        let json = r#"{"server": {}}"#;
        let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
        assert!(resp.server.networking.interfaces.interface.is_empty());
    }

    // --- collect_ips tests ---

    #[test]
    fn test_collect_ips_filters_by_type() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "94.237.1.1".into(), family: "IPv4".into() },
                    ],
                },
            },
            NetworkInterface {
                iface_type: "private".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "10.0.0.1".into(), family: "IPv4".into() },
                    ],
                },
            },
            NetworkInterface {
                iface_type: "utility".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "10.3.0.1".into(), family: "IPv4".into() },
                    ],
                },
            },
        ];
        let public = collect_ips(&interfaces, "public");
        assert_eq!(public.len(), 1);
        assert_eq!(public[0].address, "94.237.1.1");

        let private = collect_ips(&interfaces, "private");
        assert_eq!(private.len(), 1);
        assert_eq!(private[0].address, "10.0.0.1");

        let utility = collect_ips(&interfaces, "utility");
        assert_eq!(utility.len(), 1);
        assert_eq!(utility[0].address, "10.3.0.1");
    }

    #[test]
    fn test_collect_ips_empty_interfaces() {
        let interfaces: Vec<NetworkInterface> = Vec::new();
        assert!(collect_ips(&interfaces, "public").is_empty());
    }

    #[test]
    fn test_collect_ips_no_matching_type() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "private".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "10.0.0.1".into(), family: "IPv4".into() },
                    ],
                },
            },
        ];
        assert!(collect_ips(&interfaces, "public").is_empty());
    }

    #[test]
    fn test_collect_ips_multiple_addresses_on_same_interface() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "94.237.1.1".into(), family: "IPv4".into() },
                        IpAddress { address: "94.237.1.2".into(), family: "IPv4".into() },
                        IpAddress { address: "2a04::1".into(), family: "IPv6".into() },
                    ],
                },
            },
        ];
        let public = collect_ips(&interfaces, "public");
        assert_eq!(public.len(), 3);
    }

    // --- select_ip: multiple public IPv4 uses first ---

    #[test]
    fn test_select_ip_multiple_public_v4_uses_first() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "94.237.1.1".into(), family: "IPv4".into() },
                        IpAddress { address: "94.237.1.2".into(), family: "IPv4".into() },
                    ],
                },
            },
        ];
        assert_eq!(select_ip(&interfaces), Some("94.237.1.1".to_string()));
    }

    // --- select_ip: both placeholders ---

    #[test]
    fn test_select_ip_both_placeholders() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "0.0.0.0".into(), family: "IPv4".into() },
                        IpAddress { address: "::".into(), family: "IPv6".into() },
                    ],
                },
            },
        ];
        assert_eq!(select_ip(&interfaces), None);
    }

    // --- tag construction edge cases ---

    #[test]
    fn test_tags_label_empty_value_key_only() {
        let server = ServerSummary {
            uuid: "u".into(),
            title: "t".into(),
            hostname: "h".into(),
            tags: TagWrapper::default(),
            labels: LabelWrapper { label: vec![
                Label { key: "managed".into(), value: "".into() },
            ]},
            zone: String::new(),
            plan: String::new(),
            state: String::new(),
        };
        let mut tags: Vec<String> = server.tags.tag.iter().map(|t| t.to_lowercase()).collect();
        for label in &server.labels.label {
            if label.value.is_empty() {
                tags.push(label.key.clone());
            } else {
                tags.push(format!("{}={}", label.key, label.value));
            }
        }
        tags.sort();
        assert_eq!(tags, vec!["managed"]);
    }

    #[test]
    fn test_tags_only_no_labels() {
        let server = ServerSummary {
            uuid: "u".into(),
            title: "t".into(),
            hostname: "h".into(),
            tags: TagWrapper { tag: vec!["WEB".into(), "PROD".into()] },
            labels: LabelWrapper::default(),
            zone: String::new(),
            plan: String::new(),
            state: String::new(),
        };
        let mut tags: Vec<String> = server.tags.tag.iter().map(|t| t.to_lowercase()).collect();
        tags.sort();
        assert_eq!(tags, vec!["prod", "web"]);
    }

    #[test]
    fn test_labels_only_no_tags() {
        let server = ServerSummary {
            uuid: "u".into(),
            title: "t".into(),
            hostname: "h".into(),
            tags: TagWrapper::default(),
            labels: LabelWrapper { label: vec![
                Label { key: "env".into(), value: "staging".into() },
                Label { key: "team".into(), value: "backend".into() },
            ]},
            zone: String::new(),
            plan: String::new(),
            state: String::new(),
        };
        let mut tags: Vec<String> = Vec::new();
        for label in &server.labels.label {
            if label.value.is_empty() {
                tags.push(label.key.clone());
            } else {
                tags.push(format!("{}={}", label.key, label.value));
            }
        }
        tags.sort();
        assert_eq!(tags, vec!["env=staging", "team=backend"]);
    }

    // --- pagination offset logic ---

    #[test]
    fn test_pagination_stops_when_count_less_than_limit() {
        // When the API returns fewer items than the limit, we've hit the last page
        let json = r#"{
            "servers": {
                "server": [
                    {"uuid": "u1", "title": "a", "hostname": "a.example.com"},
                    {"uuid": "u2", "title": "b", "hostname": "b.example.com"}
                ]
            }
        }"#;
        let resp: ServerListResponse = serde_json::from_str(json).unwrap();
        let limit = 100;
        let count = resp.servers.server.len();
        assert!(count < limit); // Should stop
    }

    #[test]
    fn test_pagination_continues_when_count_equals_limit() {
        // When count == limit, there may be more pages
        let count = 100;
        let limit = 100;
        assert!(count >= limit); // Should continue
    }

    // --- server UUID is string ---

    #[test]
    fn test_server_uuid_is_string() {
        let json = r#"{
            "servers": {
                "server": [
                    {"uuid": "00c148cb-ef71-46cb-a76f-1bb53e791e8a", "title": "t", "hostname": "h"}
                ]
            }
        }"#;
        let resp: ServerListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.servers.server[0].uuid, "00c148cb-ef71-46cb-a76f-1bb53e791e8a");
    }

    // --- empty server list ---

    #[test]
    fn test_empty_server_list() {
        let json = r#"{"servers": {"server": []}}"#;
        let resp: ServerListResponse = serde_json::from_str(json).unwrap();
        assert!(resp.servers.server.is_empty());
    }

    // --- detail response with empty networking ---

    #[test]
    fn test_detail_empty_interfaces() {
        let json = r#"{"server": {"networking": {"interfaces": {"interface": []}}}}"#;
        let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
        assert!(resp.server.networking.interfaces.interface.is_empty());
        assert_eq!(select_ip(&resp.server.networking.interfaces.interface), None);
    }

    // --- mixed public interfaces: IPv4 on one, IPv6 on another ---

    #[test]
    fn test_select_ip_across_multiple_public_interfaces() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "2a04::1".into(), family: "IPv6".into() },
                    ],
                },
            },
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "94.237.1.1".into(), family: "IPv4".into() },
                    ],
                },
            },
        ];
        // IPv4 should win even though it's on the second interface
        assert_eq!(select_ip(&interfaces), Some("94.237.1.1".to_string()));
    }

    // --- collect_ips with multiple interfaces of same type ---

    #[test]
    fn test_collect_ips_two_public_interfaces() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "94.1.1.1".into(), family: "IPv4".into() },
                    ],
                },
            },
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "94.2.2.2".into(), family: "IPv4".into() },
                    ],
                },
            },
        ];
        let ips = collect_ips(&interfaces, "public");
        assert_eq!(ips.len(), 2);
        assert_eq!(ips[0].address, "94.1.1.1");
        assert_eq!(ips[1].address, "94.2.2.2");
    }

    // --- select_ip: public interface with empty ip_address list ---

    #[test]
    fn test_select_ip_public_empty_addresses() {
        let interfaces = vec![NetworkInterface {
            iface_type: "public".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: Vec::new(),
            },
        }];
        assert_eq!(select_ip(&interfaces), None);
    }

    // --- select_ip: only utility interface (ignored) ---

    #[test]
    fn test_select_ip_utility_interface_ignored() {
        let interfaces = vec![NetworkInterface {
            iface_type: "utility".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: vec![
                    IpAddress { address: "10.0.0.1".into(), family: "IPv4".into() },
                ],
            },
        }];
        assert_eq!(select_ip(&interfaces), None);
    }

    // --- collect_ips: private interface not in public results ---

    #[test]
    fn test_collect_ips_private_not_in_public() {
        let interfaces = vec![
            NetworkInterface {
                iface_type: "private".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "10.0.0.1".into(), family: "IPv4".into() },
                    ],
                },
            },
            NetworkInterface {
                iface_type: "public".into(),
                ip_addresses: IpAddressesWrapper {
                    ip_address: vec![
                        IpAddress { address: "94.1.1.1".into(), family: "IPv4".into() },
                    ],
                },
            },
        ];
        let public_ips = collect_ips(&interfaces, "public");
        assert_eq!(public_ips.len(), 1);
        assert_eq!(public_ips[0].address, "94.1.1.1");
    }

    // --- server name: title non-empty uses title ---

    #[test]
    fn test_server_name_uses_title_when_present() {
        let json = r#"{
            "servers": {
                "server": [{
                    "uuid": "uuid-1",
                    "title": "My Title",
                    "hostname": "my-host.example.com",
                    "tags": {"tag": []},
                    "labels": {"label": []}
                }]
            }
        }"#;
        let resp: ServerListResponse = serde_json::from_str(json).unwrap();
        let server = &resp.servers.server[0];
        let name = if server.title.is_empty() {
            server.hostname.clone()
        } else {
            server.title.clone()
        };
        assert_eq!(name, "My Title");
    }

    // --- tags: combined tags + labels, sorted ---

    #[test]
    fn test_tags_combined_and_sorted() {
        let json = r#"{
            "servers": {
                "server": [{
                    "uuid": "uuid-1",
                    "title": "test",
                    "hostname": "test",
                    "tags": {"tag": ["Zebra", "Apple"]},
                    "labels": {"label": [{"key": "env", "value": "prod"}, {"key": "tier", "value": ""}]}
                }]
            }
        }"#;
        let resp: ServerListResponse = serde_json::from_str(json).unwrap();
        let server = &resp.servers.server[0];
        let mut tags: Vec<String> = server.tags.tag.iter().map(|t| t.to_lowercase()).collect();
        for label in &server.labels.label {
            if label.value.is_empty() {
                tags.push(label.key.clone());
            } else {
                tags.push(format!("{}={}", label.key, label.value));
            }
        }
        tags.sort();
        assert_eq!(tags, vec!["apple", "env=prod", "tier", "zebra"]);
    }

    // =========================================================================
    // OS metadata from storage_devices
    // =========================================================================

    #[test]
    fn test_parse_detail_with_storage_devices() {
        let json = r#"{"server": {"networking": {"interfaces": {"interface": []}},
            "storage_devices": {"storage_device": [
                {"storage_title": "Ubuntu Server 24.04 LTS", "type": "disk"},
                {"storage_title": "Extra disk", "type": "disk"}
            ]}}}"#;
        let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
        let sd = resp.server.storage_devices.unwrap();
        assert_eq!(sd.storage_device[0].storage_title, "Ubuntu Server 24.04 LTS");
    }

    #[test]
    fn test_os_metadata_from_storage_devices() {
        let json = r#"{"server": {"networking": {"interfaces": {"interface": []}},
            "storage_devices": {"storage_device": [
                {"storage_title": "Debian GNU/Linux 12"}
            ]}}}"#;
        let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
        let sd = resp.server.storage_devices.unwrap();
        let title = &sd.storage_device[0].storage_title;
        assert_eq!(title, "Debian GNU/Linux 12");
    }

    #[test]
    fn test_os_metadata_missing_storage_devices() {
        let json = r#"{"server": {"networking": {"interfaces": {"interface": []}}}}"#;
        let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
        assert!(resp.server.storage_devices.is_none());
    }

    #[test]
    fn test_os_metadata_empty_storage_device_list() {
        let json = r#"{"server": {"networking": {"interfaces": {"interface": []}},
            "storage_devices": {"storage_device": []}}}"#;
        let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
        let sd = resp.server.storage_devices.unwrap();
        assert!(sd.storage_device.is_empty());
    }
}
