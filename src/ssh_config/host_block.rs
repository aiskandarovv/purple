//! Accessors, mutators and converters for a single `Host <pattern>` block.
//!
//! Everything that reads or writes `# purple:*` comment metadata, SSH
//! directives, or round-trip formatting for one block lives here. The
//! type definition itself and the rest of the file-level model stay in
//! [`super::model`].

use super::model::{Directive, HostBlock, HostEntry, PatternEntry};

impl HostBlock {
    /// Index of the first trailing blank line (for inserting content before separators).
    pub(super) fn content_end(&self) -> usize {
        let mut pos = self.directives.len();
        while pos > 0 {
            if self.directives[pos - 1].is_non_directive
                && self.directives[pos - 1].raw_line.trim().is_empty()
            {
                pos -= 1;
            } else {
                break;
            }
        }
        pos
    }

    /// Remove and return trailing blank lines.
    #[allow(dead_code)]
    pub(super) fn pop_trailing_blanks(&mut self) -> Vec<Directive> {
        let end = self.content_end();
        self.directives.drain(end..).collect()
    }

    /// Ensure exactly one trailing blank line.
    #[allow(dead_code)]
    pub(super) fn ensure_trailing_blank(&mut self) {
        self.pop_trailing_blanks();
        self.directives.push(Directive {
            key: String::new(),
            value: String::new(),
            raw_line: String::new(),
            is_non_directive: true,
        });
    }

    /// Detect indentation used by existing directives (falls back to "  ").
    pub(super) fn detect_indent(&self) -> String {
        for d in &self.directives {
            if !d.is_non_directive && !d.raw_line.is_empty() {
                let trimmed = d.raw_line.trim_start();
                let indent_len = d.raw_line.len() - trimmed.len();
                if indent_len > 0 {
                    return d.raw_line[..indent_len].to_string();
                }
            }
        }
        "  ".to_string()
    }

    /// Extract tags from purple:tags comment in directives.
    pub fn tags(&self) -> Vec<String> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:tags ") {
                    return rest
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect();
                }
            }
        }
        Vec::new()
    }

    /// Extract provider-synced tags from purple:provider_tags comment.
    pub fn provider_tags(&self) -> Vec<String> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:provider_tags ") {
                    return rest
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect();
                }
            }
        }
        Vec::new()
    }

    /// Check if a purple:provider_tags comment exists (even if empty).
    /// Used to distinguish "never migrated" from "migrated with no tags".
    pub fn has_provider_tags_comment(&self) -> bool {
        self.directives.iter().any(|d| {
            d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:provider_tags" || t.starts_with("# purple:provider_tags ")
            }
        })
    }

    /// Extract provider info from purple:provider comment in directives.
    /// Returns (provider_name, server_id), e.g. ("digitalocean", "412345678").
    pub fn provider(&self) -> Option<(String, String)> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:provider ") {
                    if let Some((name, id)) = rest.split_once(':') {
                        return Some((name.trim().to_string(), id.trim().to_string()));
                    }
                }
            }
        }
        None
    }

    /// Set provider on a host block. Replaces existing purple:provider comment or adds one.
    pub fn set_provider(&mut self, provider_name: &str, server_id: &str) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && d.raw_line.trim().starts_with("# purple:provider "))
        });
        let pos = self.content_end();
        self.directives.insert(
            pos,
            Directive {
                key: String::new(),
                value: String::new(),
                raw_line: format!(
                    "{}# purple:provider {}:{}",
                    indent, provider_name, server_id
                ),
                is_non_directive: true,
            },
        );
    }

    /// Extract askpass source from purple:askpass comment in directives.
    pub fn askpass(&self) -> Option<String> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:askpass ") {
                    let val = rest.trim();
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
        None
    }

    /// Extract vault-ssh role from purple:vault-ssh comment.
    pub fn vault_ssh(&self) -> Option<String> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:vault-ssh ") {
                    let val = rest.trim();
                    if !val.is_empty() && crate::vault_ssh::is_valid_role(val) {
                        return Some(val.to_string());
                    }
                }
            }
        }
        None
    }

    /// Set vault-ssh role. Replaces existing comment or adds one. Empty string removes.
    pub fn set_vault_ssh(&mut self, role: &str) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:vault-ssh" || t.starts_with("# purple:vault-ssh ")
            })
        });
        if !role.is_empty() {
            let pos = self.content_end();
            self.directives.insert(
                pos,
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: format!("{}# purple:vault-ssh {}", indent, role),
                    is_non_directive: true,
                },
            );
        }
    }

    /// Extract the Vault SSH endpoint from a `# purple:vault-addr` comment.
    /// Returns None when the comment is absent, blank or contains an invalid
    /// URL value. Validation is intentionally minimal: we reject empty,
    /// whitespace-containing and control-character values but otherwise let
    /// the Vault CLI surface its own error on typos.
    pub fn vault_addr(&self) -> Option<String> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:vault-addr ") {
                    let val = rest.trim();
                    if !val.is_empty() && crate::vault_ssh::is_valid_vault_addr(val) {
                        return Some(val.to_string());
                    }
                }
            }
        }
        None
    }

    /// Set vault-addr endpoint. Replaces existing comment or adds one. Empty
    /// string removes. Caller is expected to have validated the URL upstream
    /// (e.g. via `is_valid_vault_addr`) — this function does not re-validate.
    pub fn set_vault_addr(&mut self, url: &str) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:vault-addr" || t.starts_with("# purple:vault-addr ")
            })
        });
        if !url.is_empty() {
            let pos = self.content_end();
            self.directives.insert(
                pos,
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: format!("{}# purple:vault-addr {}", indent, url),
                    is_non_directive: true,
                },
            );
        }
    }

    /// Set askpass source on a host block. Replaces existing purple:askpass comment or adds one.
    /// Pass an empty string to remove the comment.
    pub fn set_askpass(&mut self, source: &str) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:askpass" || t.starts_with("# purple:askpass ")
            })
        });
        if !source.is_empty() {
            let pos = self.content_end();
            self.directives.insert(
                pos,
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: format!("{}# purple:askpass {}", indent, source),
                    is_non_directive: true,
                },
            );
        }
    }

    /// Extract provider metadata from purple:meta comment in directives.
    /// Format: `# purple:meta key=value,key=value`
    pub fn meta(&self) -> Vec<(String, String)> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:meta ") {
                    return rest
                        .split(',')
                        .filter_map(|pair| {
                            let (k, v) = pair.split_once('=')?;
                            let k = k.trim();
                            let v = v.trim();
                            if k.is_empty() {
                                None
                            } else {
                                Some((k.to_string(), v.to_string()))
                            }
                        })
                        .collect();
                }
            }
        }
        Vec::new()
    }

    /// Set provider metadata on a host block. Replaces existing purple:meta comment or adds one.
    /// Pass an empty slice to remove the comment.
    pub fn set_meta(&mut self, meta: &[(String, String)]) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:meta" || t.starts_with("# purple:meta ")
            })
        });
        if !meta.is_empty() {
            let encoded: Vec<String> = meta
                .iter()
                .map(|(k, v)| {
                    let clean_k = Self::sanitize_tag(&k.replace([',', '='], ""));
                    let clean_v = Self::sanitize_tag(&v.replace(',', ""));
                    format!("{}={}", clean_k, clean_v)
                })
                .collect();
            let pos = self.content_end();
            self.directives.insert(
                pos,
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: format!("{}# purple:meta {}", indent, encoded.join(",")),
                    is_non_directive: true,
                },
            );
        }
    }

    /// Extract stale timestamp from purple:stale comment in directives.
    /// Returns `None` if absent or malformed.
    pub fn stale(&self) -> Option<u64> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:stale ") {
                    return rest.trim().parse::<u64>().ok();
                }
            }
        }
        None
    }

    /// Mark a host block as stale with a unix timestamp.
    /// Replaces existing purple:stale comment or adds one.
    pub fn set_stale(&mut self, timestamp: u64) {
        let indent = self.detect_indent();
        self.clear_stale();
        let pos = self.content_end();
        self.directives.insert(
            pos,
            Directive {
                key: String::new(),
                value: String::new(),
                raw_line: format!("{}# purple:stale {}", indent, timestamp),
                is_non_directive: true,
            },
        );
    }

    /// Remove stale marking from a host block.
    pub fn clear_stale(&mut self) {
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:stale" || t.starts_with("# purple:stale ")
            })
        });
    }

    /// Sanitize a tag value: strip control characters, commas (delimiter),
    /// and Unicode format/bidi override characters. Truncate to 128 chars.
    pub(super) fn sanitize_tag(tag: &str) -> String {
        tag.chars()
            .filter(|c| {
                !c.is_control()
                    && *c != ','
                    && !('\u{200B}'..='\u{200F}').contains(c) // zero-width, bidi marks
                    && !('\u{202A}'..='\u{202E}').contains(c) // bidi embedding/override
                    && !('\u{2066}'..='\u{2069}').contains(c) // bidi isolate
                    && *c != '\u{FEFF}' // BOM/zero-width no-break space
            })
            .take(128)
            .collect()
    }

    /// Set user tags on a host block. Replaces existing purple:tags comment or adds one.
    pub fn set_tags(&mut self, tags: &[String]) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:tags" || t.starts_with("# purple:tags ")
            })
        });
        let sanitized: Vec<String> = tags
            .iter()
            .map(|t| Self::sanitize_tag(t))
            .filter(|t| !t.is_empty())
            .collect();
        if !sanitized.is_empty() {
            let pos = self.content_end();
            self.directives.insert(
                pos,
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: format!("{}# purple:tags {}", indent, sanitized.join(",")),
                    is_non_directive: true,
                },
            );
        }
    }

    /// Set provider-synced tags. Replaces existing purple:provider_tags comment.
    /// Always writes the comment (even when empty) as a migration sentinel.
    pub fn set_provider_tags(&mut self, tags: &[String]) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:provider_tags" || t.starts_with("# purple:provider_tags ")
            })
        });
        let sanitized: Vec<String> = tags
            .iter()
            .map(|t| Self::sanitize_tag(t))
            .filter(|t| !t.is_empty())
            .collect();
        let raw = if sanitized.is_empty() {
            format!("{}# purple:provider_tags", indent)
        } else {
            format!("{}# purple:provider_tags {}", indent, sanitized.join(","))
        };
        let pos = self.content_end();
        self.directives.insert(
            pos,
            Directive {
                key: String::new(),
                value: String::new(),
                raw_line: raw,
                is_non_directive: true,
            },
        );
    }

    /// Extract a convenience HostEntry view from this block.
    pub fn to_host_entry(&self) -> HostEntry {
        let mut entry = HostEntry {
            alias: self.host_pattern.clone(),
            port: 22,
            ..Default::default()
        };
        for d in &self.directives {
            if d.is_non_directive {
                continue;
            }
            if d.key.eq_ignore_ascii_case("hostname") {
                entry.hostname = d.value.clone();
            } else if d.key.eq_ignore_ascii_case("user") {
                entry.user = d.value.clone();
            } else if d.key.eq_ignore_ascii_case("port") {
                entry.port = d.value.parse().unwrap_or(22);
            } else if d.key.eq_ignore_ascii_case("identityfile") {
                if entry.identity_file.is_empty() {
                    entry.identity_file = d.value.clone();
                }
            } else if d.key.eq_ignore_ascii_case("proxyjump") {
                entry.proxy_jump = d.value.clone();
            } else if d.key.eq_ignore_ascii_case("certificatefile")
                && entry.certificate_file.is_empty()
            {
                entry.certificate_file = d.value.clone();
            }
        }
        entry.tags = self.tags();
        entry.provider_tags = self.provider_tags();
        entry.has_provider_tags = self.has_provider_tags_comment();
        entry.provider = self.provider().map(|(name, _)| name);
        entry.tunnel_count = self.tunnel_count();
        entry.askpass = self.askpass();
        entry.vault_ssh = self.vault_ssh();
        entry.vault_addr = self.vault_addr();
        entry.provider_meta = self.meta();
        entry.stale = self.stale();
        entry
    }

    /// Extract a convenience PatternEntry view from this block.
    pub fn to_pattern_entry(&self) -> PatternEntry {
        let mut entry = PatternEntry {
            pattern: self.host_pattern.clone(),
            hostname: String::new(),
            user: String::new(),
            port: 22,
            identity_file: String::new(),
            proxy_jump: String::new(),
            tags: self.tags(),
            askpass: self.askpass(),
            source_file: None,
            directives: Vec::new(),
        };
        for d in &self.directives {
            if d.is_non_directive {
                continue;
            }
            match d.key.to_ascii_lowercase().as_str() {
                "hostname" => entry.hostname = d.value.clone(),
                "user" => entry.user = d.value.clone(),
                "port" => entry.port = d.value.parse().unwrap_or(22),
                "identityfile" if entry.identity_file.is_empty() => {
                    entry.identity_file = d.value.clone();
                }
                "proxyjump" => entry.proxy_jump = d.value.clone(),
                _ => {}
            }
            entry.directives.push((d.key.clone(), d.value.clone()));
        }
        entry
    }

    /// Count forwarding directives (LocalForward, RemoteForward, DynamicForward).
    pub fn tunnel_count(&self) -> u16 {
        let count = self
            .directives
            .iter()
            .filter(|d| {
                !d.is_non_directive
                    && (d.key.eq_ignore_ascii_case("localforward")
                        || d.key.eq_ignore_ascii_case("remoteforward")
                        || d.key.eq_ignore_ascii_case("dynamicforward"))
            })
            .count();
        count.min(u16::MAX as usize) as u16
    }

    /// Check if this block has any tunnel forwarding directives.
    #[allow(dead_code)]
    pub fn has_tunnels(&self) -> bool {
        self.directives.iter().any(|d| {
            !d.is_non_directive
                && (d.key.eq_ignore_ascii_case("localforward")
                    || d.key.eq_ignore_ascii_case("remoteforward")
                    || d.key.eq_ignore_ascii_case("dynamicforward"))
        })
    }

    /// Extract tunnel rules from forwarding directives.
    pub fn tunnel_directives(&self) -> Vec<crate::tunnel::TunnelRule> {
        self.directives
            .iter()
            .filter(|d| !d.is_non_directive)
            .filter_map(|d| crate::tunnel::TunnelRule::parse_value(&d.key, &d.value))
            .collect()
    }
}
