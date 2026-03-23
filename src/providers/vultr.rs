use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct Vultr;

#[derive(Deserialize)]
struct InstanceResponse {
    instances: Vec<Instance>,
    meta: VultrMeta,
}

#[derive(Deserialize)]
struct Instance {
    id: String,
    label: String,
    main_ip: String,
    #[serde(default)]
    v6_main_ip: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    region: String,
    #[serde(default)]
    plan: String,
    #[serde(default)]
    os: String,
    #[serde(default)]
    power_status: String,
}

#[derive(Deserialize)]
struct VultrMeta {
    links: VultrLinks,
}

#[derive(Deserialize)]
struct VultrLinks {
    #[serde(default)]
    next: String,
}

impl Provider for Vultr {
    fn name(&self) -> &str {
        "vultr"
    }

    fn short_label(&self) -> &str {
        "vultr"
    }

    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        let mut all_hosts = Vec::new();
        let mut cursor: Option<String> = None;
        let agent = super::http_agent();
        let mut pages = 0u64;

        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            let url = match &cursor {
                None => "https://api.vultr.com/v2/instances?per_page=500".to_string(),
                Some(c) => format!(
                    "https://api.vultr.com/v2/instances?per_page=500&cursor={}",
                    c
                ),
            };
            let resp: InstanceResponse = agent
                .get(&url)
                .set("Authorization", &format!("Bearer {}", token))
                .call()
                .map_err(map_ureq_error)?
                .into_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            if resp.instances.is_empty() {
                break;
            }

            for instance in &resp.instances {
                // Prefer public IPv4, fall back to public IPv6
                let ip = if !instance.main_ip.is_empty() && instance.main_ip != "0.0.0.0" {
                    instance.main_ip.clone()
                } else if !instance.v6_main_ip.is_empty() && instance.v6_main_ip != "::" {
                    instance.v6_main_ip.clone()
                } else {
                    continue;
                };
                let mut metadata = Vec::new();
                if !instance.region.is_empty() {
                    metadata.push(("region".to_string(), instance.region.clone()));
                }
                if !instance.plan.is_empty() {
                    metadata.push(("plan".to_string(), instance.plan.clone()));
                }
                if !instance.os.is_empty() {
                    metadata.push(("os".to_string(), instance.os.clone()));
                }
                if !instance.power_status.is_empty() {
                    metadata.push(("status".to_string(), instance.power_status.clone()));
                }
                all_hosts.push(ProviderHost {
                    server_id: instance.id.clone(),
                    name: instance.label.clone(),
                    ip,
                    tags: instance.tags.clone(),
                    metadata,
                });
            }

            if resp.meta.links.next.is_empty() {
                break;
            }
            cursor = Some(resp.meta.links.next.clone());
            pages += 1;
            if pages >= 500 {
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
    fn test_parse_instance_response() {
        let json = r#"{
            "instances": [
                {
                    "id": "abc-123",
                    "label": "my-server",
                    "main_ip": "5.6.7.8",
                    "tags": ["web"]
                },
                {
                    "id": "def-456",
                    "label": "pending-server",
                    "main_ip": "0.0.0.0",
                    "tags": []
                }
            ],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.instances.len(), 2);
        assert_eq!(resp.instances[0].label, "my-server");
        assert_eq!(resp.instances[0].main_ip, "5.6.7.8");
        // Second instance has 0.0.0.0 (should be skipped)
        assert_eq!(resp.instances[1].main_ip, "0.0.0.0");
    }

    #[test]
    fn test_vultr_empty_ip_skipped() {
        let json = r#"{
            "instances": [
                {
                    "id": "abc-123",
                    "label": "empty-ip",
                    "main_ip": "",
                    "tags": []
                }
            ],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.instances.len(), 1);
        assert!(resp.instances[0].main_ip.is_empty());
        // This instance should be skipped during fetch_hosts because main_ip is empty
    }

    #[test]
    fn test_vultr_v6_fallback() {
        let json = r#"{
            "instances": [
                {
                    "id": "v6-only",
                    "label": "v6-server",
                    "main_ip": "0.0.0.0",
                    "v6_main_ip": "2001:db8::1",
                    "tags": []
                }
            ],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        let instance = &resp.instances[0];
        assert_eq!(instance.main_ip, "0.0.0.0");
        assert_eq!(instance.v6_main_ip, "2001:db8::1");
    }

    // Helper: same IP selection logic as fetch_hosts_cancellable
    fn select_vultr_ip(instance: &Instance) -> Option<String> {
        if !instance.main_ip.is_empty() && instance.main_ip != "0.0.0.0" {
            Some(instance.main_ip.clone())
        } else if !instance.v6_main_ip.is_empty() && instance.v6_main_ip != "::" {
            Some(instance.v6_main_ip.clone())
        } else {
            None
        }
    }

    #[test]
    fn test_vultr_both_placeholder_skipped() {
        let json = r#"{
            "instances": [
                {"id": "xyz", "label": "both-zero", "main_ip": "0.0.0.0", "v6_main_ip": "::", "tags": []}
            ],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_vultr_ip(&resp.instances[0]), None);
    }

    #[test]
    fn test_vultr_prefers_v4_over_v6() {
        let json = r#"{
            "instances": [
                {"id": "a", "label": "dual", "main_ip": "5.6.7.8", "v6_main_ip": "2001:db8::1", "tags": []}
            ],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            select_vultr_ip(&resp.instances[0]),
            Some("5.6.7.8".to_string())
        );
    }

    #[test]
    fn test_vultr_tags_preserved() {
        let json = r#"{
            "instances": [
                {"id": "t", "label": "tagged", "main_ip": "1.2.3.4", "tags": ["web", "prod"]}
            ],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.instances[0].tags, vec!["web", "prod"]);
    }

    #[test]
    fn test_vultr_cursor_pagination_has_next() {
        let json = r#"{
            "instances": [{"id": "a", "label": "a", "main_ip": "1.2.3.4", "tags": []}],
            "meta": {"links": {"next": "bmV4dA=="}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.meta.links.next.is_empty());
    }

    #[test]
    fn test_vultr_default_v6_empty_string() {
        // v6_main_ip defaults to "" when not in JSON
        let json = r#"{
            "instances": [{"id": "a", "label": "a", "main_ip": "1.2.3.4", "tags": []}],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.instances[0].v6_main_ip, "");
    }

    #[test]
    fn test_vultr_default_tags_empty() {
        let json = r#"{
            "instances": [{"id": "a", "label": "a", "main_ip": "1.2.3.4"}],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert!(resp.instances[0].tags.is_empty());
    }

    #[test]
    fn test_vultr_empty_instance_list_stops() {
        let json = r#"{
            "instances": [],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert!(resp.instances.is_empty());
    }

    #[test]
    fn test_vultr_string_id_preserved() {
        // Vultr uses string UUIDs for instance IDs, unlike other providers
        let json = r#"{
            "instances": [{
                "id": "cb676a46-66fd-4dfb-b839-443f2e6c0b60",
                "label": "uuid-test",
                "main_ip": "1.2.3.4",
                "tags": []
            }],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.instances[0].id, "cb676a46-66fd-4dfb-b839-443f2e6c0b60");
    }

    #[test]
    fn test_vultr_valid_v4_ignores_placeholder_v6() {
        let json = r#"{
            "instances": [{
                "id": "a",
                "label": "v4-with-placeholder-v6",
                "main_ip": "5.6.7.8",
                "v6_main_ip": "::",
                "tags": []
            }],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            select_vultr_ip(&resp.instances[0]),
            Some("5.6.7.8".to_string())
        );
    }

    #[test]
    fn test_vultr_empty_v4_and_valid_v6() {
        let json = r#"{
            "instances": [{
                "id": "a",
                "label": "empty-v4-valid-v6",
                "main_ip": "",
                "v6_main_ip": "2001:db8::1",
                "tags": []
            }],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            select_vultr_ip(&resp.instances[0]),
            Some("2001:db8::1".to_string())
        );
    }

    #[test]
    fn test_vultr_empty_v4_and_empty_v6() {
        let json = r#"{
            "instances": [{
                "id": "a",
                "label": "both-empty",
                "main_ip": "",
                "v6_main_ip": "",
                "tags": []
            }],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_vultr_ip(&resp.instances[0]), None);
    }

    #[test]
    fn test_vultr_multiple_tags() {
        let json = r#"{
            "instances": [{
                "id": "a",
                "label": "multi-tag",
                "main_ip": "1.2.3.4",
                "tags": ["web", "production", "us-east", "team-a"]
            }],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.instances[0].tags.len(), 4);
    }

    #[test]
    fn test_vultr_missing_next_link() {
        // VultrLinks.next should default to empty string when missing
        let json = r#"{
            "instances": [],
            "meta": {"links": {}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert!(resp.meta.links.next.is_empty());
    }

    #[test]
    fn test_vultr_v6_placeholder_only() {
        // main_ip is 0.0.0.0 and v6 is :: → skipped
        let json = r#"{
            "instances": [{
                "id": "a",
                "label": "no-ip",
                "main_ip": "0.0.0.0",
                "v6_main_ip": "::",
                "tags": []
            }],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(select_vultr_ip(&resp.instances[0]), None);
    }

    #[test]
    fn test_vultr_label_with_special_chars() {
        let json = r#"{
            "instances": [{
                "id": "a",
                "label": "web-server (prod) #1",
                "main_ip": "1.2.3.4",
                "tags": []
            }],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.instances[0].label, "web-server (prod) #1");
    }

    #[test]
    fn test_vultr_v4_zero_not_empty() {
        // "0.0.0.0" is the placeholder, not empty string
        let json = r#"{
            "instances": [{
                "id": "a",
                "label": "pending",
                "main_ip": "0.0.0.0",
                "v6_main_ip": "2001:db8::1",
                "tags": []
            }],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        // 0.0.0.0 treated as placeholder, falls back to v6
        assert_eq!(
            select_vultr_ip(&resp.instances[0]),
            Some("2001:db8::1".to_string())
        );
    }

    #[test]
    fn test_vultr_cursor_pagination_empty_next_stops() {
        let json = r#"{
            "instances": [{"id": "a", "label": "a", "main_ip": "1.2.3.4", "tags": []}],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert!(resp.meta.links.next.is_empty());
    }

    #[test]
    fn test_vultr_multiple_instances_parsed() {
        let json = r#"{
            "instances": [
                {"id": "a", "label": "web-1", "main_ip": "1.1.1.1", "tags": ["web"]},
                {"id": "b", "label": "web-2", "main_ip": "2.2.2.2", "tags": ["web"]},
                {"id": "c", "label": "db-1", "main_ip": "3.3.3.3", "tags": ["db"]}
            ],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.instances.len(), 3);
        for inst in &resp.instances {
            assert!(select_vultr_ip(inst).is_some());
        }
    }

    // --- v4 is 0.0.0.0 with no v6 field at all → None ---

    #[test]
    fn test_vultr_placeholder_v4_no_v6_field() {
        let json = r#"{
            "instances": [{
                "id": "a",
                "label": "no-v6",
                "main_ip": "0.0.0.0",
                "tags": []
            }],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        // v6_main_ip defaults to "" which is also not usable
        assert_eq!(select_vultr_ip(&resp.instances[0]), None);
    }

    // --- valid v4 with empty label ---

    #[test]
    fn test_vultr_empty_label() {
        let json = r#"{
            "instances": [{
                "id": "x",
                "label": "",
                "main_ip": "1.2.3.4",
                "tags": []
            }],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.instances[0].label, "");
        assert_eq!(
            select_vultr_ip(&resp.instances[0]),
            Some("1.2.3.4".to_string())
        );
    }

    // --- Resilience: extra/unknown fields are ignored by serde ---

    #[test]
    fn test_vultr_extra_fields_ignored() {
        // Real Vultr API returns many more fields per instance
        let json = r#"{
            "instances": [{
                "id": "cb676a46-66fd-4dfb-b839-443f2e6c0b60",
                "os": "Ubuntu 22.04 LTS x64",
                "ram": 1024,
                "disk": 25,
                "main_ip": "45.76.1.1",
                "vcpu_count": 1,
                "region": "ewr",
                "plan": "vc2-1c-1gb",
                "date_created": "2024-01-01T00:00:00+00:00",
                "status": "active",
                "allowed_bandwidth": 1000,
                "netmask_v4": "255.255.254.0",
                "gateway_v4": "45.76.0.1",
                "power_status": "running",
                "server_status": "ok",
                "v6_main_ip": "2001:19f0::1",
                "v6_network": "2001:19f0::",
                "v6_network_size": 64,
                "label": "full-response",
                "internal_ip": "",
                "kvm": "https://my.vultr.com/subs/vps/novnc/...",
                "hostname": "full-response",
                "os_id": 1743,
                "app_id": 0,
                "image_id": "",
                "firewall_group_id": "",
                "features": ["auto_backups"],
                "tags": ["web", "prod"],
                "user_scheme": "root"
            }],
            "meta": {"links": {"next": "", "prev": ""}, "total": 1}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.instances[0].label, "full-response");
        assert_eq!(resp.instances[0].main_ip, "45.76.1.1");
        assert_eq!(resp.instances[0].v6_main_ip, "2001:19f0::1");
        assert_eq!(resp.instances[0].tags, vec!["web", "prod"]);
    }

    #[test]
    fn test_vultr_meta_extra_fields_ignored() {
        // Meta may contain additional fields like total and prev
        let json = r#"{
            "instances": [],
            "meta": {"links": {"next": "", "prev": ""}, "total": 0}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert!(resp.instances.is_empty());
        assert!(resp.meta.links.next.is_empty());
    }
}
