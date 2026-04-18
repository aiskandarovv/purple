//! Repair utilities for `# purple:group` comments in an SSH config file.
//!
//! Two problems this module fixes:
//!
//! 1. Provider group headers (`# purple:group <DisplayName>`) that end up
//!    absorbed as non-directive lines inside a preceding Host block.
//!    `repair_absorbed_group_comments` promotes them back to top-level
//!    `GlobalLine` elements so they render correctly.
//! 2. Orphaned group headers for providers that no longer have any hosts
//!    configured. `remove_all_orphaned_group_headers` and
//!    `remove_orphaned_group_header` drop those stale lines.

use super::model::{ConfigElement, SshConfigFile};

/// Display name for a provider used in `# purple:group` headers.
/// Mirrors `providers::provider_display_name()` without a cross-module
/// dependency.
pub(super) fn provider_group_display_name(name: &str) -> &str {
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
        other => other,
    }
}

impl SshConfigFile {
    /// Remove all `# purple:group <DisplayName>` GlobalLines that point at a
    /// provider with no remaining Host blocks. Returns the count removed.
    pub fn remove_all_orphaned_group_headers(&mut self) -> usize {
        let active_providers: std::collections::HashSet<String> = self
            .elements
            .iter()
            .filter_map(|e| {
                if let ConfigElement::HostBlock(block) = e {
                    block
                        .provider()
                        .map(|(name, _)| provider_group_display_name(&name).to_string())
                } else {
                    None
                }
            })
            .collect();

        let mut removed = 0;
        self.elements.retain(|e| {
            if let ConfigElement::GlobalLine(line) = e {
                if let Some(rest) = line.trim().strip_prefix("# purple:group ") {
                    if !active_providers.contains(rest.trim()) {
                        removed += 1;
                        return false;
                    }
                }
            }
            true
        });
        removed
    }

    /// Repair configs where `# purple:group` comments were absorbed into the
    /// preceding host block's directives instead of being stored as
    /// GlobalLines. Returns the number of blocks that were repaired.
    pub fn repair_absorbed_group_comments(&mut self) -> usize {
        let mut repaired = 0;
        let mut idx = 0;
        while idx < self.elements.len() {
            let needs_repair = if let ConfigElement::HostBlock(block) = &self.elements[idx] {
                block
                    .directives
                    .iter()
                    .any(|d| d.is_non_directive && d.raw_line.trim().starts_with("# purple:group "))
            } else {
                false
            };

            if !needs_repair {
                idx += 1;
                continue;
            }

            let block = if let ConfigElement::HostBlock(block) = &mut self.elements[idx] {
                block
            } else {
                unreachable!()
            };

            let group_idx = block
                .directives
                .iter()
                .position(|d| {
                    d.is_non_directive && d.raw_line.trim().starts_with("# purple:group ")
                })
                .unwrap();

            let mut keep_end = group_idx;
            while keep_end > 0
                && block.directives[keep_end - 1].is_non_directive
                && block.directives[keep_end - 1].raw_line.trim().is_empty()
            {
                keep_end -= 1;
            }

            let extracted: Vec<ConfigElement> = block
                .directives
                .drain(keep_end..)
                .map(|d| ConfigElement::GlobalLine(d.raw_line))
                .collect();

            let insert_at = idx + 1;
            for (i, elem) in extracted.into_iter().enumerate() {
                self.elements.insert(insert_at + i, elem);
            }

            repaired += 1;
            idx = insert_at;
            while idx < self.elements.len() {
                if let ConfigElement::HostBlock(_) = &self.elements[idx] {
                    break;
                }
                idx += 1;
            }
        }
        repaired
    }

    /// Remove the `# purple:group <DisplayName>` GlobalLine for a single
    /// provider if no remaining HostBlock has that provider.
    pub(super) fn remove_orphaned_group_header(&mut self, provider_name: &str) {
        if self.find_hosts_by_provider(provider_name).is_empty() {
            let display = provider_group_display_name(provider_name);
            let header = format!("# purple:group {}", display);
            self.elements
                .retain(|e| !matches!(e, ConfigElement::GlobalLine(line) if *line == header));
        }
    }
}
