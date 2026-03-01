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
                all_hosts.push(ProviderHost {
                    server_id: instance.id.clone(),
                    name: instance.label.clone(),
                    ip,
                    tags: instance.tags.clone(),
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
}
