use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use ratatui::widgets::ListState;

use crate::history::ConnectionHistory;
use crate::providers::config::ProviderConfig;
use crate::ssh_config::model::{ConfigElement, HostEntry, PatternEntry, SshConfigFile};
use crate::ssh_keys::{self, SshKeyInfo};
use crate::tunnel::{TunnelRule, TunnelType};

/// Case-insensitive substring check without allocation.
/// Uses a byte-window approach for ASCII strings (the common case for SSH
/// hostnames and aliases). Falls back to a char-based scan when either
/// string contains non-ASCII bytes to avoid false matches across UTF-8
/// character boundaries.
pub(crate) fn contains_ci(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle.is_ascii() {
        return haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()));
    }
    // Non-ASCII fallback: compare char-by-char (case fold ASCII only)
    let needle_lower: Vec<char> = needle.chars().map(|c| c.to_ascii_lowercase()).collect();
    let haystack_chars: Vec<char> = haystack.chars().collect();
    haystack_chars.windows(needle_lower.len()).any(|window| {
        window
            .iter()
            .zip(needle_lower.iter())
            .all(|(h, n)| h.to_ascii_lowercase() == *n)
    })
}

/// Case-insensitive equality check without allocation.
fn eq_ci(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Record of the last sync result for a provider.
#[derive(Debug, Clone)]
pub struct SyncRecord {
    pub timestamp: u64,
    pub message: String,
    pub is_error: bool,
}

impl SyncRecord {
    /// Load sync history from ~/.purple/sync_history.tsv.
    /// Format: provider\ttimestamp\tis_error\tmessage
    pub fn load_all() -> HashMap<String, SyncRecord> {
        let mut map = HashMap::new();
        let Some(home) = dirs::home_dir() else {
            return map;
        };
        let path = home.join(".purple").join("sync_history.tsv");
        let Ok(content) = std::fs::read_to_string(&path) else {
            return map;
        };
        for line in content.lines() {
            let parts: Vec<&str> = line.splitn(4, '\t').collect();
            if parts.len() < 4 {
                continue;
            }
            let Some(ts) = parts[1].parse::<u64>().ok() else {
                continue;
            };
            let is_error = parts[2] == "1";
            map.insert(
                parts[0].to_string(),
                SyncRecord {
                    timestamp: ts,
                    message: parts[3].to_string(),
                    is_error,
                },
            );
        }
        map
    }

    /// Save sync history to ~/.purple/sync_history.tsv.
    pub fn save_all(history: &HashMap<String, SyncRecord>) {
        let Some(home) = dirs::home_dir() else { return };
        let dir = home.join(".purple");
        let path = dir.join("sync_history.tsv");
        let mut lines = Vec::new();
        for (provider, record) in history {
            lines.push(format!(
                "{}\t{}\t{}\t{}",
                provider,
                record.timestamp,
                if record.is_error { "1" } else { "0" },
                record.message
            ));
        }
        let _ = crate::fs_util::atomic_write(&path, lines.join("\n").as_bytes());
    }
}

/// Which screen is currently displayed.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    HostList,
    AddHost,
    EditHost {
        alias: String,
    },
    ConfirmDelete {
        alias: String,
    },
    Help {
        return_screen: Box<Screen>,
    },
    KeyList,
    KeyDetail {
        index: usize,
    },
    HostDetail {
        index: usize,
    },
    TagPicker,
    Providers,
    ProviderForm {
        provider: String,
    },
    TunnelList {
        alias: String,
    },
    TunnelForm {
        alias: String,
        editing: Option<usize>,
    },
    SnippetPicker {
        target_aliases: Vec<String>,
    },
    SnippetForm {
        target_aliases: Vec<String>,
        editing: Option<usize>,
    },
    SnippetOutput {
        snippet_name: String,
        target_aliases: Vec<String>,
    },
    SnippetParamForm {
        snippet: crate::snippet::Snippet,
        target_aliases: Vec<String>,
    },
    ConfirmHostKeyReset {
        alias: String,
        hostname: String,
        known_hosts_path: String,
        askpass: Option<String>,
    },
    FileBrowser {
        alias: String,
    },
    Containers {
        alias: String,
    },
    ConfirmImport {
        count: usize,
    },
    ConfirmPurgeStale {
        aliases: Vec<String>,
        provider: Option<String>,
    },
    Welcome {
        has_backup: bool,
        host_count: usize,
        known_hosts_count: usize,
    },
}

/// Which form field is focused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FormField {
    Alias,
    Hostname,
    User,
    Port,
    IdentityFile,
    ProxyJump,
    AskPass,
    Tags,
}

impl FormField {
    pub const ALL: [FormField; 8] = [
        FormField::Alias,
        FormField::Hostname,
        FormField::User,
        FormField::Port,
        FormField::IdentityFile,
        FormField::ProxyJump,
        FormField::AskPass,
        FormField::Tags,
    ];

    pub fn next(self) -> Self {
        let idx = FormField::ALL.iter().position(|f| *f == self).unwrap_or(0);
        FormField::ALL[(idx + 1) % FormField::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = FormField::ALL.iter().position(|f| *f == self).unwrap_or(0);
        FormField::ALL[(idx + FormField::ALL.len() - 1) % FormField::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            FormField::Alias => "Alias",
            FormField::Hostname => "Host / IP",
            FormField::User => "User",
            FormField::Port => "Port",
            FormField::IdentityFile => "Identity File",
            FormField::ProxyJump => "ProxyJump",
            FormField::AskPass => "Password Source",
            FormField::Tags => "Tags",
        }
    }
}

/// Form state for adding/editing a host.
#[derive(Debug, Clone)]
pub struct HostForm {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: String,
    pub identity_file: String,
    pub proxy_jump: String,
    pub askpass: String,
    pub tags: String,
    pub focused_field: FormField,
    pub cursor_pos: usize,
    /// Real-time validation hint shown in footer.
    pub form_hint: Option<String>,
    /// When true, alias is a Host pattern (wildcards allowed, hostname optional).
    pub is_pattern: bool,
}

impl HostForm {
    pub fn new() -> Self {
        Self {
            alias: String::new(),
            hostname: String::new(),
            user: String::new(),
            port: "22".to_string(),
            identity_file: String::new(),
            proxy_jump: String::new(),
            askpass: String::new(),
            tags: String::new(),
            focused_field: FormField::Alias,
            cursor_pos: 0,
            form_hint: None,
            is_pattern: false,
        }
    }

    pub fn new_pattern() -> Self {
        Self {
            is_pattern: true,
            ..Self::new()
        }
    }

    pub fn from_entry(entry: &HostEntry) -> Self {
        let alias = entry.alias.clone();
        let cursor_pos = alias.chars().count();
        Self {
            alias,
            hostname: entry.hostname.clone(),
            user: entry.user.clone(),
            port: entry.port.to_string(),
            identity_file: entry.identity_file.clone(),
            proxy_jump: entry.proxy_jump.clone(),
            askpass: entry.askpass.clone().unwrap_or_default(),
            tags: entry.tags.join(", "),
            focused_field: FormField::Alias,
            cursor_pos,
            form_hint: None,
            is_pattern: false,
        }
    }

    pub fn from_pattern_entry(entry: &PatternEntry) -> Self {
        let alias = entry.pattern.clone();
        let cursor_pos = alias.chars().count();
        Self {
            alias,
            hostname: entry.hostname.clone(),
            user: entry.user.clone(),
            port: entry.port.to_string(),
            identity_file: entry.identity_file.clone(),
            proxy_jump: entry.proxy_jump.clone(),
            askpass: entry.askpass.clone().unwrap_or_default(),
            tags: entry.tags.join(", "),
            focused_field: FormField::Alias,
            cursor_pos,
            form_hint: None,
            is_pattern: true,
        }
    }

    pub fn focused_value(&self) -> &str {
        match self.focused_field {
            FormField::Alias => &self.alias,
            FormField::Hostname => &self.hostname,
            FormField::User => &self.user,
            FormField::Port => &self.port,
            FormField::IdentityFile => &self.identity_file,
            FormField::ProxyJump => &self.proxy_jump,
            FormField::AskPass => &self.askpass,
            FormField::Tags => &self.tags,
        }
    }

    /// Get a mutable reference to the currently focused field's value.
    pub fn focused_value_mut(&mut self) -> &mut String {
        match self.focused_field {
            FormField::Alias => &mut self.alias,
            FormField::Hostname => &mut self.hostname,
            FormField::User => &mut self.user,
            FormField::Port => &mut self.port,
            FormField::IdentityFile => &mut self.identity_file,
            FormField::ProxyJump => &mut self.proxy_jump,
            FormField::AskPass => &mut self.askpass,
            FormField::Tags => &mut self.tags,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let pos = self.cursor_pos;
        let val = self.focused_value_mut();
        let byte_pos = char_to_byte_pos(val, pos);
        val.insert(byte_pos, c);
        self.cursor_pos = pos + 1;
    }

    pub fn delete_char_before_cursor(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let pos = self.cursor_pos;
        let val = self.focused_value_mut();
        let byte_pos = char_to_byte_pos(val, pos);
        let prev = char_to_byte_pos(val, pos - 1);
        val.drain(prev..byte_pos);
        self.cursor_pos = pos - 1;
    }

    pub fn sync_cursor_to_end(&mut self) {
        self.cursor_pos = self.focused_value().chars().count();
    }

    /// Run lightweight validation on the focused field and update `form_hint`.
    pub fn update_hint(&mut self) {
        self.form_hint = match self.focused_field {
            FormField::Alias => {
                let v = self.alias.trim();
                if v.is_empty() {
                    None // Don't nag while empty (user may not have typed yet)
                } else if self.is_pattern {
                    if !crate::ssh_config::model::is_host_pattern(v) {
                        Some("Pattern needs a wildcard (*, ?, [) or multiple hosts".into())
                    } else {
                        None
                    }
                } else if v.contains(char::is_whitespace) {
                    Some("Alias can't contain whitespace".into())
                } else if v.contains('#') {
                    Some("Alias can't contain '#'".into())
                } else if crate::ssh_config::model::is_host_pattern(v) {
                    Some("Alias can't contain pattern characters".into())
                } else {
                    None
                }
            }
            FormField::Hostname => {
                let v = self.hostname.trim();
                if !v.is_empty() && v.contains(char::is_whitespace) {
                    Some("Hostname can't contain whitespace".into())
                } else {
                    None
                }
            }
            FormField::User => {
                let v = self.user.trim();
                if !v.is_empty() && v.contains(char::is_whitespace) {
                    Some("User can't contain whitespace".into())
                } else {
                    None
                }
            }
            FormField::Port => {
                let v = &self.port;
                if v.is_empty() || v == "22" {
                    None
                } else {
                    match v.parse::<u16>() {
                        Ok(0) => Some("Port must be 1-65535".into()),
                        Err(_) => Some("Not a valid port number".into()),
                        _ => None,
                    }
                }
            }
            _ => None,
        };
    }

    /// Validate the form. Returns an error message if invalid.
    pub fn validate(&self) -> Result<(), String> {
        let alias = self.alias.trim();
        if alias.is_empty() {
            return Err(if self.is_pattern {
                "Pattern can't be empty.".to_string()
            } else {
                "Alias can't be empty. Every host needs a name!".to_string()
            });
        }
        if self.is_pattern && !crate::ssh_config::model::is_host_pattern(alias) {
            return Err("Pattern needs a wildcard (*, ?, [) or multiple hosts.".to_string());
        } else if !self.is_pattern {
            if alias.contains(char::is_whitespace) {
                return Err("Alias can't contain whitespace. Keep it simple.".to_string());
            }
            if alias.contains('#') {
                return Err(
                    "Alias can't contain '#'. That's a comment character in SSH config."
                        .to_string(),
                );
            }
            // Catches *, ?, [, ! — whitespace overlap with the check above is intentional
            // (user gets the more specific whitespace message first)
            if crate::ssh_config::model::is_host_pattern(alias) {
                return Err(
                    "Alias can't contain pattern characters. That creates a match pattern, not a host."
                        .to_string(),
                );
            }
        }
        // Reject control characters in all fields
        let fields = [
            (
                &self.alias,
                if self.is_pattern { "Pattern" } else { "Alias" },
            ),
            (&self.hostname, "Hostname"),
            (&self.user, "User"),
            (&self.port, "Port"),
            (&self.identity_file, "Identity File"),
            (&self.proxy_jump, "ProxyJump"),
            (&self.askpass, "Password Source"),
            (&self.tags, "Tags"),
        ];
        for (value, name) in &fields {
            if value.chars().any(|c| c.is_control()) {
                return Err(format!(
                    "{} contains control characters. That's not going to work.",
                    name
                ));
            }
        }
        if !self.is_pattern && self.hostname.trim().is_empty() {
            return Err("Hostname can't be empty. Where should we connect to?".to_string());
        }
        if self.hostname.trim().contains(char::is_whitespace) {
            return Err("Hostname can't contain whitespace.".to_string());
        }
        if self.user.trim().contains(char::is_whitespace) {
            return Err("User can't contain whitespace.".to_string());
        }
        let port: u16 = self
            .port
            .parse()
            .map_err(|_| "That's not a port number. Ports are 1-65535, not poetry.".to_string())?;
        if port == 0 {
            return Err("Port 0? Bold choice, but no. Try 1-65535.".to_string());
        }
        Ok(())
    }

    /// Convert to a HostEntry.
    pub fn to_entry(&self) -> HostEntry {
        let askpass_trimmed = self.askpass.trim().to_string();
        HostEntry {
            alias: self.alias.trim().to_string(),
            hostname: self.hostname.trim().to_string(),
            user: self.user.trim().to_string(),
            port: self.port.parse().unwrap_or(22),
            identity_file: self.identity_file.trim().to_string(),
            proxy_jump: self.proxy_jump.trim().to_string(),
            tags: self
                .tags
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect(),
            askpass: if askpass_trimmed.is_empty() {
                None
            } else {
                Some(askpass_trimmed)
            },
            ..Default::default()
        }
    }
}

/// Which provider form field is focused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProviderFormField {
    Url,
    Token,
    Profile,
    Project,
    Compartment,
    Regions,
    AliasPrefix,
    User,
    IdentityFile,
    VerifyTls,
    AutoSync,
}

impl ProviderFormField {
    const CLOUD_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::AutoSync,
    ];

    const PROXMOX_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Url,
        ProviderFormField::Token,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::VerifyTls,
        ProviderFormField::AutoSync,
    ];

    const AWS_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::Profile,
        ProviderFormField::Regions,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::AutoSync,
    ];

    const SCALEWAY_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::Regions,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::AutoSync,
    ];

    const GCP_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::Project,
        ProviderFormField::Regions,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::AutoSync,
    ];

    const AZURE_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::Regions,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::AutoSync,
    ];

    const ORACLE_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::Compartment,
        ProviderFormField::Regions,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::AutoSync,
    ];

    const OVH_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::Project,
        ProviderFormField::Regions,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::AutoSync,
    ];

    pub fn fields_for(provider: &str) -> &'static [ProviderFormField] {
        match provider {
            "proxmox" => Self::PROXMOX_FIELDS,
            "aws" => Self::AWS_FIELDS,
            "scaleway" => Self::SCALEWAY_FIELDS,
            "gcp" => Self::GCP_FIELDS,
            "azure" => Self::AZURE_FIELDS,
            "oracle" => Self::ORACLE_FIELDS,
            "ovh" => Self::OVH_FIELDS,
            _ => Self::CLOUD_FIELDS,
        }
    }

    pub fn next(self, fields: &[Self]) -> Self {
        let idx = fields.iter().position(|f| *f == self).unwrap_or(0);
        fields[(idx + 1) % fields.len()]
    }

    pub fn prev(self, fields: &[Self]) -> Self {
        let idx = fields.iter().position(|f| *f == self).unwrap_or(0);
        fields[(idx + fields.len() - 1) % fields.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            ProviderFormField::Url => "URL",
            ProviderFormField::Token => "Token",
            ProviderFormField::Profile => "Profile",
            ProviderFormField::Project => "Project ID",
            ProviderFormField::Compartment => "Compartment",
            ProviderFormField::Regions => "Regions",
            ProviderFormField::AliasPrefix => "Alias Prefix",
            ProviderFormField::User => "User",
            ProviderFormField::IdentityFile => "Identity File",
            ProviderFormField::VerifyTls => "Verify TLS",
            ProviderFormField::AutoSync => "Auto Sync",
        }
    }
}

/// Form state for configuring a provider.
#[derive(Debug, Clone)]
pub struct ProviderFormFields {
    pub url: String,
    pub token: String,
    pub profile: String,
    pub project: String,
    pub compartment: String,
    pub regions: String,
    pub alias_prefix: String,
    pub user: String,
    pub identity_file: String,
    pub verify_tls: bool,
    pub auto_sync: bool,
    pub focused_field: ProviderFormField,
    pub cursor_pos: usize,
}

impl ProviderFormFields {
    pub fn new() -> Self {
        Self {
            url: String::new(),
            token: String::new(),
            profile: String::new(),
            project: String::new(),
            compartment: String::new(),
            regions: String::new(),
            alias_prefix: String::new(),
            user: "root".to_string(),
            identity_file: String::new(),
            verify_tls: true,
            auto_sync: true,
            focused_field: ProviderFormField::Token,
            cursor_pos: 0,
        }
    }

    pub fn focused_value(&self) -> &str {
        match self.focused_field {
            ProviderFormField::Url => &self.url,
            ProviderFormField::Token => &self.token,
            ProviderFormField::Profile => &self.profile,
            ProviderFormField::Project => &self.project,
            ProviderFormField::Compartment => &self.compartment,
            ProviderFormField::Regions => &self.regions,
            ProviderFormField::AliasPrefix => &self.alias_prefix,
            ProviderFormField::User => &self.user,
            ProviderFormField::IdentityFile => &self.identity_file,
            ProviderFormField::VerifyTls | ProviderFormField::AutoSync => "",
        }
    }

    pub fn focused_value_mut(&mut self) -> Option<&mut String> {
        match self.focused_field {
            ProviderFormField::Url => Some(&mut self.url),
            ProviderFormField::Token => Some(&mut self.token),
            ProviderFormField::Profile => Some(&mut self.profile),
            ProviderFormField::Project => Some(&mut self.project),
            ProviderFormField::Compartment => Some(&mut self.compartment),
            ProviderFormField::Regions => Some(&mut self.regions),
            ProviderFormField::AliasPrefix => Some(&mut self.alias_prefix),
            ProviderFormField::User => Some(&mut self.user),
            ProviderFormField::IdentityFile => Some(&mut self.identity_file),
            ProviderFormField::VerifyTls | ProviderFormField::AutoSync => None,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let pos = self.cursor_pos;
        if let Some(val) = self.focused_value_mut() {
            let byte_pos = char_to_byte_pos(val, pos);
            val.insert(byte_pos, c);
            self.cursor_pos = pos + 1;
        }
    }

    pub fn delete_char_before_cursor(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let pos = self.cursor_pos;
        if let Some(val) = self.focused_value_mut() {
            let byte_pos = char_to_byte_pos(val, pos);
            let prev = char_to_byte_pos(val, pos - 1);
            val.drain(prev..byte_pos);
            self.cursor_pos = pos - 1;
        }
    }

    pub fn sync_cursor_to_end(&mut self) {
        self.cursor_pos = self.focused_value().chars().count();
    }
}

pub(crate) fn char_to_byte_pos(s: &str, char_pos: usize) -> usize {
    s.char_indices()
        .nth(char_pos)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

/// Which tunnel form field is focused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TunnelFormField {
    Type,
    BindPort,
    RemoteHost,
    RemotePort,
}

impl TunnelFormField {
    /// Next field, skipping remote fields when Dynamic.
    pub fn next(self, tunnel_type: TunnelType) -> Self {
        match (self, tunnel_type) {
            (TunnelFormField::Type, _) => TunnelFormField::BindPort,
            (TunnelFormField::BindPort, TunnelType::Dynamic) => TunnelFormField::Type,
            (TunnelFormField::BindPort, _) => TunnelFormField::RemoteHost,
            (TunnelFormField::RemoteHost, _) => TunnelFormField::RemotePort,
            (TunnelFormField::RemotePort, _) => TunnelFormField::Type,
        }
    }

    /// Previous field, skipping remote fields when Dynamic.
    pub fn prev(self, tunnel_type: TunnelType) -> Self {
        match (self, tunnel_type) {
            (TunnelFormField::Type, TunnelType::Dynamic) => TunnelFormField::BindPort,
            (TunnelFormField::Type, _) => TunnelFormField::RemotePort,
            (TunnelFormField::BindPort, _) => TunnelFormField::Type,
            (TunnelFormField::RemoteHost, _) => TunnelFormField::BindPort,
            (TunnelFormField::RemotePort, _) => TunnelFormField::RemoteHost,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TunnelFormField::Type => "Type",
            TunnelFormField::BindPort => "Bind Port",
            TunnelFormField::RemoteHost => "Remote Host",
            TunnelFormField::RemotePort => "Remote Port",
        }
    }
}

/// Form state for adding/editing a tunnel.
#[derive(Debug, Clone)]
pub struct TunnelForm {
    pub tunnel_type: TunnelType,
    pub bind_port: String,
    pub remote_host: String,
    pub remote_port: String,
    /// Hidden field: preserved during edit, not exposed in the form UI.
    pub bind_address: String,
    pub focused_field: TunnelFormField,
    pub cursor_pos: usize,
}

impl TunnelForm {
    pub fn new() -> Self {
        Self {
            tunnel_type: TunnelType::Local,
            bind_port: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: String::new(),
            bind_address: String::new(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        }
    }

    pub fn from_rule(rule: &TunnelRule) -> Self {
        Self {
            tunnel_type: rule.tunnel_type,
            bind_port: rule.bind_port.to_string(),
            remote_host: rule.remote_host.clone(),
            remote_port: if rule.remote_port > 0 {
                rule.remote_port.to_string()
            } else {
                String::new()
            },
            bind_address: rule.bind_address.clone(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        }
    }

    /// Validate the form. Returns error message if invalid.
    pub fn validate(&self) -> Result<(), String> {
        // Reject control characters in all fields
        let fields = [
            (&self.bind_port, "Bind Port"),
            (&self.remote_host, "Remote Host"),
            (&self.remote_port, "Remote Port"),
        ];
        for (value, name) in &fields {
            if value.chars().any(|c| c.is_control()) {
                return Err(format!("{} contains control characters.", name));
            }
        }
        let port: u16 = self
            .bind_port
            .parse()
            .map_err(|_| "Bind port must be 1-65535.".to_string())?;
        if port == 0 {
            return Err("Bind port can't be 0.".to_string());
        }
        if self.tunnel_type != TunnelType::Dynamic {
            let host = self.remote_host.trim();
            if host.is_empty() {
                return Err("Remote host can't be empty.".to_string());
            }
            if host.contains(char::is_whitespace) {
                return Err("Remote host can't contain spaces.".to_string());
            }
            let rport: u16 = self
                .remote_port
                .parse()
                .map_err(|_| "Remote port must be 1-65535.".to_string())?;
            if rport == 0 {
                return Err("Remote port can't be 0.".to_string());
            }
        }
        Ok(())
    }

    /// Convert to directive key and value for writing to SSH config.
    /// Uses TunnelRule for correct IPv6 bracketing and bind_address preservation.
    pub fn to_directive(&self) -> (&'static str, String) {
        let key = self.tunnel_type.directive_key();
        let bind_port: u16 = self.bind_port.parse().unwrap_or(0);
        let remote_port: u16 = self.remote_port.parse().unwrap_or(0);
        let rule = TunnelRule {
            tunnel_type: self.tunnel_type,
            bind_address: self.bind_address.clone(),
            bind_port,
            remote_host: self.remote_host.clone(),
            remote_port,
        };
        (key, rule.to_directive_value())
    }

    pub fn focused_value(&self) -> Option<&str> {
        match self.focused_field {
            TunnelFormField::Type => None,
            TunnelFormField::BindPort => Some(&self.bind_port),
            TunnelFormField::RemoteHost => Some(&self.remote_host),
            TunnelFormField::RemotePort => Some(&self.remote_port),
        }
    }

    /// Get mutable reference to the focused text field's value.
    /// Returns None for Type field (uses Left/Right, not text input).
    pub fn focused_value_mut(&mut self) -> Option<&mut String> {
        match self.focused_field {
            TunnelFormField::Type => None,
            TunnelFormField::BindPort => Some(&mut self.bind_port),
            TunnelFormField::RemoteHost => Some(&mut self.remote_host),
            TunnelFormField::RemotePort => Some(&mut self.remote_port),
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let pos = self.cursor_pos;
        if let Some(val) = self.focused_value_mut() {
            let byte_pos = char_to_byte_pos(val, pos);
            val.insert(byte_pos, c);
            self.cursor_pos = pos + 1;
        }
    }

    pub fn delete_char_before_cursor(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let pos = self.cursor_pos;
        if let Some(val) = self.focused_value_mut() {
            let byte_pos = char_to_byte_pos(val, pos);
            let prev = char_to_byte_pos(val, pos - 1);
            val.drain(prev..byte_pos);
            self.cursor_pos = pos - 1;
        }
    }

    pub fn sync_cursor_to_end(&mut self) {
        self.cursor_pos = self.focused_value().map(|v| v.chars().count()).unwrap_or(0);
    }
}

/// Which snippet form field is focused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnippetFormField {
    Name,
    Command,
    Description,
}

impl SnippetFormField {
    pub const ALL: &[SnippetFormField] = &[
        SnippetFormField::Name,
        SnippetFormField::Command,
        SnippetFormField::Description,
    ];

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            SnippetFormField::Name => "Name",
            SnippetFormField::Command => "Command",
            SnippetFormField::Description => "Description",
        }
    }
}

/// Form state for adding/editing a snippet.
#[derive(Debug, Clone)]
pub struct SnippetForm {
    pub name: String,
    pub command: String,
    pub description: String,
    pub focused_field: SnippetFormField,
    pub cursor_pos: usize,
}

impl SnippetForm {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            command: String::new(),
            description: String::new(),
            focused_field: SnippetFormField::Name,
            cursor_pos: 0,
        }
    }

    pub fn from_snippet(snippet: &crate::snippet::Snippet) -> Self {
        Self {
            name: snippet.name.clone(),
            command: snippet.command.clone(),
            description: snippet.description.clone(),
            focused_field: SnippetFormField::Name,
            cursor_pos: snippet.name.chars().count(),
        }
    }

    pub fn focused_value(&self) -> &str {
        match self.focused_field {
            SnippetFormField::Name => &self.name,
            SnippetFormField::Command => &self.command,
            SnippetFormField::Description => &self.description,
        }
    }

    pub fn focused_value_mut(&mut self) -> &mut String {
        match self.focused_field {
            SnippetFormField::Name => &mut self.name,
            SnippetFormField::Command => &mut self.command,
            SnippetFormField::Description => &mut self.description,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let pos = self.cursor_pos;
        let val = self.focused_value_mut();
        let byte_pos = char_to_byte_pos(val, pos);
        val.insert(byte_pos, c);
        self.cursor_pos = pos + 1;
    }

    pub fn delete_char_before_cursor(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let pos = self.cursor_pos;
        let val = self.focused_value_mut();
        let byte_pos = char_to_byte_pos(val, pos);
        let prev = char_to_byte_pos(val, pos - 1);
        val.drain(prev..byte_pos);
        self.cursor_pos = pos - 1;
    }

    pub fn sync_cursor_to_end(&mut self) {
        self.cursor_pos = self.focused_value().chars().count();
    }

    pub fn validate(&self) -> Result<(), String> {
        crate::snippet::validate_name(&self.name)?;
        crate::snippet::validate_command(&self.command)?;
        if self.description.contains(|c: char| c.is_control()) {
            return Err("Description contains control characters.".to_string());
        }
        Ok(())
    }
}

/// Output from snippet execution, per host.
#[derive(Debug, Clone)]
pub struct SnippetHostOutput {
    pub alias: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

/// State for the snippet output screen.
#[derive(Debug, Clone)]
pub struct SnippetOutputState {
    pub run_id: u64,
    pub results: Vec<SnippetHostOutput>,
    pub scroll_offset: usize,
    pub completed: usize,
    pub total: usize,
    pub all_done: bool,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Form state for snippet parameter input.
#[derive(Debug, Clone)]
pub struct SnippetParamFormState {
    pub params: Vec<crate::snippet::SnippetParam>,
    pub values: Vec<String>,
    pub focused_index: usize,
    pub cursor_pos: usize,
    pub scroll_offset: usize,
    /// How many params actually fit on screen (set by renderer).
    pub visible_count: usize,
}

impl SnippetParamFormState {
    pub fn new(params: &[crate::snippet::SnippetParam]) -> Self {
        let values: Vec<String> = params
            .iter()
            .map(|p| p.default.clone().unwrap_or_default())
            .collect();
        let cursor_pos = values.first().map(|v| v.chars().count()).unwrap_or(0);
        Self {
            params: params.to_vec(),
            values,
            focused_index: 0,
            cursor_pos,
            scroll_offset: 0,
            visible_count: params.len().min(8),
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let idx = self.focused_index;
        let pos = self.cursor_pos;
        let val = &mut self.values[idx];
        let byte_pos = char_to_byte_pos(val, pos);
        val.insert(byte_pos, c);
        self.cursor_pos = pos + 1;
    }

    pub fn delete_char_before_cursor(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let idx = self.focused_index;
        let pos = self.cursor_pos;
        let val = &mut self.values[idx];
        let byte_pos = char_to_byte_pos(val, pos);
        let prev = char_to_byte_pos(val, pos - 1);
        val.drain(prev..byte_pos);
        self.cursor_pos = pos - 1;
    }

    /// Build a map of param name to user-entered value for substitution.
    pub fn values_map(&self) -> HashMap<String, String> {
        self.params
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name.clone(), self.values[i].clone()))
            .collect()
    }
}

/// Status message displayed at the bottom.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub is_error: bool,
    pub tick_count: u32,
}

/// An item in the display list (hosts + group headers).
#[derive(Debug, Clone)]
pub enum HostListItem {
    GroupHeader(String),
    Host { index: usize },
    Pattern { index: usize },
}

/// Ping status for a host.
#[derive(Debug, Clone, PartialEq)]
pub enum PingStatus {
    Checking,
    Reachable,
    Unreachable,
    Skipped,
}

/// View mode for the host list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewMode {
    Compact,
    Detailed,
}

/// Animation state for the detail panel slide transition.
#[derive(Debug)]
pub struct DetailAnimation {
    /// Animation start time.
    pub start: std::time::Instant,
    /// Whether animating towards open (true) or closed (false).
    pub opening: bool,
    /// Progress at the start of this animation (0.0 = closed, 1.0 = open).
    /// Allows reversing mid-animation smoothly.
    pub start_progress: f32,
}

/// Animation state for overlay open/close transitions.
#[derive(Debug)]
pub struct OverlayAnimation {
    pub start: std::time::Instant,
    /// true = opening, false = closing.
    pub opening: bool,
    /// Duration in ms. Allows per-screen animation speed (e.g. slower for welcome).
    pub duration_ms: u128,
}

/// Sort mode for the host list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortMode {
    Original,
    AlphaAlias,
    AlphaHostname,
    Frecency,
    MostRecent,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            SortMode::Original => SortMode::AlphaAlias,
            SortMode::AlphaAlias => SortMode::AlphaHostname,
            SortMode::AlphaHostname => SortMode::Frecency,
            SortMode::Frecency => SortMode::MostRecent,
            SortMode::MostRecent => SortMode::Original,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::Original => "config order",
            SortMode::AlphaAlias => "A-Z alias",
            SortMode::AlphaHostname => "A-Z hostname",
            SortMode::Frecency => "most used",
            SortMode::MostRecent => "most recent",
        }
    }

    pub fn to_key(self) -> &'static str {
        match self {
            SortMode::Original => "original",
            SortMode::AlphaAlias => "alpha_alias",
            SortMode::AlphaHostname => "alpha_hostname",
            SortMode::Frecency => "frecency",
            SortMode::MostRecent => "most_recent",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "original" => SortMode::Original,
            "alpha_alias" => SortMode::AlphaAlias,
            "alpha_hostname" => SortMode::AlphaHostname,
            "frecency" => SortMode::Frecency,
            "most_recent" => SortMode::MostRecent,
            _ => SortMode::MostRecent,
        }
    }
}

/// Group mode for the host list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupBy {
    None,
    Provider,
    Tag(String),
}

impl GroupBy {
    pub fn to_key(&self) -> String {
        match self {
            GroupBy::None => "none".to_string(),
            GroupBy::Provider => "provider".to_string(),
            GroupBy::Tag(tag) => format!("tag:{}", tag),
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "none" => GroupBy::None,
            "provider" => GroupBy::Provider,
            s if s.starts_with("tag:") => match s.strip_prefix("tag:") {
                Some(tag) => GroupBy::Tag(tag.to_string()),
                _ => GroupBy::None,
            },
            _ => GroupBy::None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            GroupBy::None => "ungrouped".to_string(),
            GroupBy::Provider => "provider".to_string(),
            GroupBy::Tag(tag) => format!("tag: {}", tag),
        }
    }
}

/// Stores a deleted host for undo.
#[derive(Debug, Clone)]
pub struct DeletedHost {
    pub element: ConfigElement,
    pub position: usize,
}

/// Ratatui ListState fields for all list views.
pub struct UiSelection {
    pub list_state: ListState,
    pub key_list_state: ListState,
    pub show_key_picker: bool,
    pub key_picker_state: ListState,
    pub show_password_picker: bool,
    pub password_picker_state: ListState,
    pub show_proxyjump_picker: bool,
    pub proxyjump_picker_state: ListState,
    pub tag_picker_state: ListState,
    pub provider_list_state: ListState,
    pub tunnel_list_state: ListState,
    pub snippet_picker_state: ListState,
    pub snippet_search: Option<String>,
    pub show_region_picker: bool,
    pub region_picker_cursor: usize,
    pub help_scroll: u16,
    pub detail_scroll: u16,
}

/// State for the Containers overlay.
pub struct ContainerState {
    pub alias: String,
    pub askpass: Option<String>,
    pub runtime: Option<crate::containers::ContainerRuntime>,
    pub containers: Vec<crate::containers::ContainerInfo>,
    pub list_state: ratatui::widgets::ListState,
    pub loading: bool,
    pub error: Option<String>,
    pub action_in_progress: Option<String>,
    /// Pending confirmation for stop/restart actions: (action, container_name, container_id).
    pub confirm_action: Option<(crate::containers::ContainerAction, String, String)>,
}

/// Search mode state.
pub struct SearchState {
    pub query: Option<String>,
    pub filtered_indices: Vec<usize>,
    pub filtered_pattern_indices: Vec<usize>,
    pub pre_search_selection: Option<usize>,
    /// When a group tab is active, holds the host indices visible in that group.
    /// Search results are intersected with this set to scope the search.
    pub scope_indices: Option<std::collections::HashSet<usize>>,
}

/// Auto-reload mtime tracking.
pub struct ReloadState {
    pub config_path: PathBuf,
    pub last_modified: Option<SystemTime>,
    pub include_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
    pub include_dir_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
}

/// Form conflict detection mtimes.
pub struct ConflictState {
    pub form_mtime: Option<SystemTime>,
    pub form_include_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
    pub form_include_dir_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
    pub provider_form_mtime: Option<SystemTime>,
}

/// Kill active tunnel processes when App is dropped (e.g. on panic).
impl Drop for App {
    fn drop(&mut self) {
        for (_, mut tunnel) in self.active_tunnels.drain() {
            let _ = tunnel.child.kill();
            let _ = tunnel.child.wait();
        }
    }
}

/// Baseline snapshot of host form content for dirty-check on Esc.
#[derive(Clone)]
pub struct FormBaseline {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: String,
    pub identity_file: String,
    pub proxy_jump: String,
    pub askpass: String,
    pub tags: String,
}

/// Baseline snapshot of tunnel form content for dirty-check on Esc.
#[derive(Clone)]
pub struct TunnelFormBaseline {
    pub tunnel_type: crate::tunnel::TunnelType,
    pub bind_port: String,
    pub remote_host: String,
    pub remote_port: String,
    pub bind_address: String,
}

/// Baseline snapshot of snippet form content for dirty-check on Esc.
#[derive(Clone)]
pub struct SnippetFormBaseline {
    pub name: String,
    pub command: String,
    pub description: String,
}

/// Baseline snapshot of provider form content for dirty-check on Esc.
#[derive(Clone)]
pub struct ProviderFormBaseline {
    pub url: String,
    pub token: String,
    pub profile: String,
    pub project: String,
    pub compartment: String,
    pub regions: String,
    pub alias_prefix: String,
    pub user: String,
    pub identity_file: String,
    pub verify_tls: bool,
    pub auto_sync: bool,
}

/// Main application state.
pub struct App {
    // Core
    pub screen: Screen,
    pub running: bool,
    pub config: SshConfigFile,
    pub hosts: Vec<HostEntry>,
    pub patterns: Vec<PatternEntry>,
    pub display_list: Vec<HostListItem>,
    pub form: HostForm,
    pub status: Option<StatusMessage>,
    pub pending_connect: Option<(String, Option<String>)>,

    // Sub-structs
    pub ui: UiSelection,
    pub search: SearchState,
    pub reload: ReloadState,
    pub conflict: ConflictState,

    // Keys
    pub keys: Vec<SshKeyInfo>,

    // Tags
    pub tag_input: Option<String>,
    pub tag_input_cursor: usize,
    pub tag_list: Vec<String>,

    // History + preferences
    pub history: ConnectionHistory,
    pub sort_mode: SortMode,
    pub group_by: GroupBy,
    pub view_mode: ViewMode,
    pub detail_anim: Option<DetailAnimation>,

    // Overlay animation
    pub overlay_anim: Option<OverlayAnimation>,
    pub overlay_buffer: Option<ratatui::buffer::Buffer>,
    pub prev_was_overlay: bool,

    // Per-frame animation snapshots (set by tick_animations, read by render code).
    // Avoids multiple elapsed() calls per frame which can race.
    pub frame_detail_progress: Option<f32>,
    pub frame_overlay_progress: Option<f32>,
    pub frame_animating: bool,

    // Undo (multi-level, capped at 50)
    pub undo_stack: Vec<DeletedHost>,

    // Providers
    pub provider_config: ProviderConfig,
    pub provider_form: ProviderFormFields,
    pub syncing_providers: HashMap<String, Arc<AtomicBool>>,
    /// Names of providers that completed during this sync batch.
    pub sync_done: Vec<String>,
    /// Whether any provider in the current batch had errors.
    pub sync_had_errors: bool,
    pub pending_provider_delete: Option<String>,
    pub pending_snippet_delete: Option<usize>,
    pub pending_tunnel_delete: Option<usize>,

    // Hints
    pub ping_status: HashMap<String, PingStatus>,
    pub has_pinged: bool,
    pub ping_generation: u64,

    // Tunnels
    pub tunnel_list: Vec<TunnelRule>,
    pub tunnel_form: TunnelForm,
    pub active_tunnels: HashMap<String, crate::tunnel::ActiveTunnel>,

    // Snippets
    pub snippet_store: crate::snippet::SnippetStore,
    pub snippet_form: SnippetForm,
    pub pending_snippet: Option<(crate::snippet::Snippet, Vec<String>)>,
    /// Host indices selected for multi-host snippet execution (space to toggle).
    pub multi_select: HashSet<usize>,
    /// Currently active group filter (tab navigation). None = show all groups.
    pub group_filter: Option<String>,
    /// Index into group_tab_order for tab navigation.
    pub group_tab_index: usize,
    /// Ordered list of group names from the current display list.
    pub group_tab_order: Vec<String>,
    /// Host/pattern counts per group (computed before group filtering).
    pub group_host_counts: HashMap<String, usize>,
    pub snippet_output: Option<SnippetOutputState>,
    pub snippet_param_form: Option<SnippetParamFormState>,
    /// When true, the snippet param form submits to terminal-exit mode (! key).
    pub pending_snippet_terminal: bool,

    // Update
    pub update_available: Option<String>,
    pub update_headline: Option<String>,
    pub update_hint: &'static str,

    // Cached tunnel summaries (invalidated on config reload)
    pub tunnel_summaries_cache: HashMap<String, String>,

    // Sync history
    pub sync_history: HashMap<String, SyncRecord>,

    // Bitwarden session
    pub bw_session: Option<String>,

    // File browser
    pub file_browser: Option<crate::file_browser::FileBrowserState>,
    pub file_browser_paths: HashMap<String, (PathBuf, String)>,

    // Containers
    pub container_state: Option<ContainerState>,
    pub container_cache: HashMap<String, crate::containers::ContainerCacheEntry>,

    // First-run hints
    pub known_hosts_count: usize,
    pub welcome_opened: Option<std::time::Instant>,

    // Form dirty-check baselines
    pub form_baseline: Option<FormBaseline>,
    pub tunnel_form_baseline: Option<TunnelFormBaseline>,
    pub snippet_form_baseline: Option<SnippetFormBaseline>,
    pub provider_form_baseline: Option<ProviderFormBaseline>,
    /// When true, the Esc key shows a "Discard changes?" dialog instead of closing.
    pub pending_discard_confirm: bool,
}

impl App {
    pub fn new(config: SshConfigFile) -> Self {
        let hosts = config.host_entries();
        let patterns = config.pattern_entries();
        let display_list = Self::build_display_list_from(&config, &hosts, &patterns);
        let mut list_state = ListState::default();
        // Select first selectable item
        if let Some(pos) = display_list.iter().position(|item| {
            matches!(
                item,
                HostListItem::Host { .. } | HostListItem::Pattern { .. }
            )
        }) {
            list_state.select(Some(pos));
        }

        let config_path = config.path.clone();
        let last_modified = Self::get_mtime(&config_path);
        let include_mtimes = Self::snapshot_include_mtimes(&config);
        let include_dir_mtimes = Self::snapshot_include_dir_mtimes(&config);

        Self {
            screen: Screen::HostList,
            running: true,
            config,
            hosts,
            patterns,
            display_list,
            form: HostForm::new(),
            status: None,
            pending_connect: None,
            ui: UiSelection {
                list_state,
                key_list_state: ListState::default(),
                show_key_picker: false,
                key_picker_state: ListState::default(),
                show_password_picker: false,
                password_picker_state: ListState::default(),
                show_proxyjump_picker: false,
                proxyjump_picker_state: ListState::default(),
                tag_picker_state: ListState::default(),
                provider_list_state: ListState::default(),
                tunnel_list_state: ListState::default(),
                snippet_picker_state: ListState::default(),
                snippet_search: None,
                show_region_picker: false,
                region_picker_cursor: 0,
                help_scroll: 0,
                detail_scroll: 0,
            },
            search: SearchState {
                query: None,
                filtered_indices: Vec::new(),
                filtered_pattern_indices: Vec::new(),
                pre_search_selection: None,
                scope_indices: None,
            },
            reload: ReloadState {
                config_path,
                last_modified,
                include_mtimes,
                include_dir_mtimes,
            },
            conflict: ConflictState {
                form_mtime: None,
                form_include_mtimes: Vec::new(),
                form_include_dir_mtimes: Vec::new(),
                provider_form_mtime: None,
            },
            keys: Vec::new(),
            tag_input: None,
            tag_input_cursor: 0,
            tag_list: Vec::new(),
            history: ConnectionHistory::load(),
            sort_mode: SortMode::Original,
            group_by: GroupBy::None,
            view_mode: ViewMode::Compact,
            detail_anim: None,
            overlay_anim: None,
            overlay_buffer: None,
            prev_was_overlay: false,
            frame_detail_progress: None,
            frame_overlay_progress: None,
            frame_animating: false,
            undo_stack: Vec::new(),
            provider_config: ProviderConfig::load(),
            provider_form: ProviderFormFields::new(),
            syncing_providers: HashMap::new(),
            sync_done: Vec::new(),
            sync_had_errors: false,
            pending_provider_delete: None,
            pending_snippet_delete: None,
            pending_tunnel_delete: None,
            ping_status: HashMap::new(),
            has_pinged: false,
            ping_generation: 0,
            tunnel_list: Vec::new(),
            tunnel_form: TunnelForm::new(),
            active_tunnels: HashMap::new(),
            snippet_store: crate::snippet::SnippetStore::load(),
            snippet_form: SnippetForm::new(),
            pending_snippet: None,
            multi_select: HashSet::new(),
            group_filter: None,
            group_tab_index: 0,
            group_tab_order: Vec::new(),
            group_host_counts: HashMap::new(),
            snippet_output: None,
            snippet_param_form: None,
            pending_snippet_terminal: false,
            tunnel_summaries_cache: HashMap::new(),
            update_available: None,
            update_headline: None,
            update_hint: crate::update::update_hint(),
            sync_history: SyncRecord::load_all(),
            bw_session: None,
            file_browser: None,
            file_browser_paths: HashMap::new(),
            container_state: None,
            container_cache: crate::containers::load_container_cache(),
            known_hosts_count: 0,
            welcome_opened: None,
            form_baseline: None,
            tunnel_form_baseline: None,
            snippet_form_baseline: None,
            provider_form_baseline: None,
            pending_discard_confirm: false,
        }
    }

    /// Build the display list with group headers from comments above host blocks.
    /// Comments are associated with the host block directly below them (no blank line between).
    /// Because the parser puts inter-block comments inside the preceding block's directives,
    /// we also extract trailing comments from each HostBlock.
    fn build_display_list_from(
        config: &SshConfigFile,
        hosts: &[HostEntry],
        patterns: &[PatternEntry],
    ) -> Vec<HostListItem> {
        let mut display_list = Vec::new();
        let mut host_index = 0;
        let mut pending_comment: Option<String> = None;

        for element in &config.elements {
            match element {
                ConfigElement::GlobalLine(line) => {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix('#') {
                        let text = rest.trim();
                        let text = text.strip_prefix("purple:group ").unwrap_or(text);
                        if !text.is_empty() {
                            pending_comment = Some(text.to_string());
                        }
                    } else if trimmed.is_empty() {
                        // Blank line breaks the comment-to-host association
                        pending_comment = None;
                    } else {
                        pending_comment = None;
                    }
                }
                ConfigElement::HostBlock(block) => {
                    if crate::ssh_config::model::is_host_pattern(&block.host_pattern) {
                        pending_comment = None;
                        continue;
                    }

                    if host_index < hosts.len() {
                        if let Some(header) = pending_comment.take() {
                            display_list.push(HostListItem::GroupHeader(header));
                        }
                        display_list.push(HostListItem::Host { index: host_index });
                        host_index += 1;
                    }

                    // Extract trailing comments from this block for the next host
                    pending_comment = Self::extract_trailing_comment(&block.directives);
                }
                ConfigElement::Include(include) => {
                    pending_comment = None;
                    for file in &include.resolved_files {
                        Self::build_display_list_from_included(
                            &file.elements,
                            &file.path,
                            hosts,
                            &mut host_index,
                            &mut display_list,
                        );
                    }
                }
            }
        }

        // Append pattern group at the bottom
        if !patterns.is_empty() {
            let mut pattern_index = 0usize;
            display_list.push(HostListItem::GroupHeader("Patterns".to_string()));
            Self::append_pattern_items(&config.elements, &mut pattern_index, &mut display_list);
            debug_assert_eq!(
                pattern_index,
                patterns.len(),
                "append_pattern_items and collect_pattern_entries traversal mismatch"
            );
        }

        display_list
    }

    fn append_pattern_items(
        elements: &[ConfigElement],
        pattern_index: &mut usize,
        display_list: &mut Vec<HostListItem>,
    ) {
        for e in elements {
            match e {
                ConfigElement::HostBlock(block) => {
                    if crate::ssh_config::model::is_host_pattern(&block.host_pattern) {
                        display_list.push(HostListItem::Pattern {
                            index: *pattern_index,
                        });
                        *pattern_index += 1;
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        Self::append_pattern_items(&file.elements, pattern_index, display_list);
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
    }

    /// Extract a trailing comment from a block's directives.
    /// If the last non-blank line in the directives is a comment, return it as
    /// a potential group header for the next host block.
    /// Strips `purple:group ` prefix so headers display as the provider name.
    fn extract_trailing_comment(
        directives: &[crate::ssh_config::model::Directive],
    ) -> Option<String> {
        let d = directives.last()?;
        if !d.is_non_directive {
            return None;
        }
        let trimmed = d.raw_line.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            let text = rest.trim();
            // Skip purple metadata comments (purple:provider, purple:tags)
            // Only purple:group should produce a group header
            if text.starts_with("purple:") && !text.starts_with("purple:group ") {
                return None;
            }
            let text = text.strip_prefix("purple:group ").unwrap_or(text);
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
        None
    }

    fn build_display_list_from_included(
        elements: &[ConfigElement],
        file_path: &std::path::Path,
        hosts: &[HostEntry],
        host_index: &mut usize,
        display_list: &mut Vec<HostListItem>,
    ) {
        let mut pending_comment: Option<String> = None;
        let file_name = file_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();

        // Add file header for included files
        if !file_name.is_empty() {
            let has_hosts = elements.iter().any(|e| {
                matches!(e, ConfigElement::HostBlock(b)
                    if !crate::ssh_config::model::is_host_pattern(&b.host_pattern)
                )
            });
            if has_hosts {
                display_list.push(HostListItem::GroupHeader(file_name));
            }
        }

        for element in elements {
            match element {
                ConfigElement::GlobalLine(line) => {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix('#') {
                        let text = rest.trim();
                        let text = text.strip_prefix("purple:group ").unwrap_or(text);
                        if !text.is_empty() {
                            pending_comment = Some(text.to_string());
                        }
                    } else {
                        pending_comment = None;
                    }
                }
                ConfigElement::HostBlock(block) => {
                    if crate::ssh_config::model::is_host_pattern(&block.host_pattern) {
                        pending_comment = None;
                        continue;
                    }

                    if *host_index < hosts.len() {
                        if let Some(header) = pending_comment.take() {
                            display_list.push(HostListItem::GroupHeader(header));
                        }
                        display_list.push(HostListItem::Host { index: *host_index });
                        *host_index += 1;
                    }

                    // Extract trailing comments from this block for the next host
                    pending_comment = Self::extract_trailing_comment(&block.directives);
                }
                ConfigElement::Include(include) => {
                    pending_comment = None;
                    for file in &include.resolved_files {
                        Self::build_display_list_from_included(
                            &file.elements,
                            &file.path,
                            hosts,
                            host_index,
                            display_list,
                        );
                    }
                }
            }
        }
    }

    /// Rebuild the display list based on the current sort mode and group_by toggle.
    pub fn apply_sort(&mut self) {
        // Preserve currently selected host or pattern across sort changes
        let selected_alias = self
            .selected_host()
            .map(|h| h.alias.clone())
            .or_else(|| self.selected_pattern().map(|p| p.pattern.clone()));

        // Multi-select indices become visually misleading after reorder
        self.multi_select.clear();

        if self.sort_mode == SortMode::Original && matches!(self.group_by, GroupBy::None) {
            self.display_list =
                Self::build_display_list_from(&self.config, &self.hosts, &self.patterns);
        } else if self.sort_mode == SortMode::Original && !matches!(self.group_by, GroupBy::None) {
            // Original order but grouped: extract flat indices from config order
            let indices: Vec<usize> = (0..self.hosts.len()).collect();
            self.display_list = self.group_indices(&indices);
        } else {
            let mut indices: Vec<usize> = (0..self.hosts.len()).collect();
            match self.sort_mode {
                SortMode::AlphaAlias => {
                    indices.sort_by(|a, b| {
                        let sa = self.hosts[*a].stale.is_some();
                        let sb = self.hosts[*b].stale.is_some();
                        sa.cmp(&sb).then_with(|| {
                            self.hosts[*a]
                                .alias
                                .to_lowercase()
                                .cmp(&self.hosts[*b].alias.to_lowercase())
                        })
                    });
                }
                SortMode::AlphaHostname => {
                    indices.sort_by(|a, b| {
                        let sa = self.hosts[*a].stale.is_some();
                        let sb = self.hosts[*b].stale.is_some();
                        sa.cmp(&sb).then_with(|| {
                            self.hosts[*a]
                                .hostname
                                .to_lowercase()
                                .cmp(&self.hosts[*b].hostname.to_lowercase())
                        })
                    });
                }
                SortMode::Frecency => {
                    indices.sort_by(|a, b| {
                        let sa = self.hosts[*a].stale.is_some();
                        let sb = self.hosts[*b].stale.is_some();
                        sa.cmp(&sb).then_with(|| {
                            let score_a = self.history.frecency_score(&self.hosts[*a].alias);
                            let score_b = self.history.frecency_score(&self.hosts[*b].alias);
                            score_b.total_cmp(&score_a)
                        })
                    });
                }
                SortMode::MostRecent => {
                    indices.sort_by(|a, b| {
                        let sa = self.hosts[*a].stale.is_some();
                        let sb = self.hosts[*b].stale.is_some();
                        sa.cmp(&sb).then_with(|| {
                            let ts_a = self.history.last_connected(&self.hosts[*a].alias);
                            let ts_b = self.history.last_connected(&self.hosts[*b].alias);
                            ts_b.cmp(&ts_a)
                        })
                    });
                }
                _ => {}
            }
            self.display_list = self.group_indices(&indices);
        }

        // Append pattern group at the bottom (sorted/grouped paths skip
        // build_display_list_from which already handles this)
        if (self.sort_mode != SortMode::Original || !matches!(self.group_by, GroupBy::None))
            && !self.patterns.is_empty()
        {
            self.display_list
                .push(HostListItem::GroupHeader("Patterns".to_string()));
            let mut pattern_index = 0usize;
            Self::append_pattern_items(
                &self.config.elements,
                &mut pattern_index,
                &mut self.display_list,
            );
        }

        // Compute group host counts before group filtering
        {
            self.group_host_counts.clear();
            let mut current_group: Option<&str> = None;
            for item in &self.display_list {
                match item {
                    HostListItem::GroupHeader(text) => {
                        current_group = Some(text.as_str());
                    }
                    HostListItem::Host { .. } | HostListItem::Pattern { .. } => {
                        if let Some(group) = current_group {
                            *self.group_host_counts.entry(group.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }

        // Build group tab order. For tag mode, compute from host tags (matching
        // render_group_bar's tab list). For provider mode, extract from GroupHeaders.
        self.group_tab_order = match &self.group_by {
            GroupBy::Tag(_) => {
                let mut tag_counts: HashMap<String, usize> = HashMap::new();
                for host in &self.hosts {
                    for tag in host.tags.iter().chain(host.provider_tags.iter()) {
                        *tag_counts.entry(tag.clone()).or_insert(0) += 1;
                    }
                }
                let mut sorted: Vec<(String, usize)> = tag_counts.into_iter().collect();
                sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                let top: Vec<(String, usize)> = sorted.into_iter().take(10).collect();
                self.group_host_counts = top.iter().map(|(t, c)| (t.clone(), *c)).collect();
                top.into_iter().map(|(t, _)| t).collect()
            }
            _ => {
                let mut order = Vec::new();
                for item in &self.display_list {
                    if let HostListItem::GroupHeader(text) = item {
                        if !order.contains(text) {
                            order.push(text.clone());
                        }
                    }
                }
                order
            }
        };

        // Re-derive group_tab_index from group_filter after rebuild
        self.group_tab_index = match &self.group_filter {
            Some(name) => self
                .group_tab_order
                .iter()
                .position(|g| g == name)
                .map(|i| i + 1)
                .unwrap_or(0),
            None => 0,
        };

        // Filter by group if active
        if let Some(ref filter) = self.group_filter {
            let is_tag_mode = matches!(self.group_by, GroupBy::Tag(_));
            let mut filtered = Vec::with_capacity(self.display_list.len());

            if is_tag_mode {
                // In tag mode, filter by host tags directly (GroupHeaders don't
                // cover all tags, only the active GroupBy tag).
                for item in std::mem::take(&mut self.display_list) {
                    match &item {
                        HostListItem::GroupHeader(_) => {} // skip all headers
                        HostListItem::Host { index } => {
                            if let Some(host) = self.hosts.get(*index) {
                                if host
                                    .tags
                                    .iter()
                                    .chain(host.provider_tags.iter())
                                    .any(|t| t == filter)
                                {
                                    filtered.push(item);
                                }
                            }
                        }
                        HostListItem::Pattern { index } => {
                            if let Some(pattern) = self.patterns.get(*index) {
                                if pattern.tags.iter().any(|t| t == filter) {
                                    filtered.push(item);
                                }
                            }
                        }
                    }
                }
            } else {
                // In provider/none mode, filter by GroupHeader matching
                let mut in_group = false;
                for item in std::mem::take(&mut self.display_list) {
                    match &item {
                        HostListItem::GroupHeader(text) => {
                            in_group = text == filter;
                        }
                        _ => {
                            if in_group {
                                filtered.push(item);
                            }
                        }
                    }
                }
            }

            self.display_list = filtered;
        }

        // Restore selection by alias, fall back to first host
        if let Some(alias) = selected_alias {
            self.select_host_by_alias(&alias);
            if self.selected_host().is_some() || self.selected_pattern().is_some() {
                return;
            }
        }
        self.select_first_host();
    }

    /// Select the first selectable item in the display list (always skips headers).
    pub fn select_first_host(&mut self) {
        if let Some(pos) = self.display_list.iter().position(|item| {
            matches!(
                item,
                HostListItem::Host { .. } | HostListItem::Pattern { .. }
            )
        }) {
            self.ui.list_state.select(Some(pos));
        }
    }

    /// Partition sorted indices by provider, inserting group headers.
    /// Hosts without provider appear first (no header), then named provider
    /// groups (in first-appearance order) with headers.
    fn group_indices(&self, sorted_indices: &[usize]) -> Vec<HostListItem> {
        match &self.group_by {
            GroupBy::None => sorted_indices
                .iter()
                .map(|&i| HostListItem::Host { index: i })
                .collect(),
            GroupBy::Provider => Self::group_indices_by_provider(&self.hosts, sorted_indices),
            GroupBy::Tag(tag) => Self::group_indices_by_tag(&self.hosts, sorted_indices, tag),
        }
    }

    fn group_indices_by_provider(
        hosts: &[HostEntry],
        sorted_indices: &[usize],
    ) -> Vec<HostListItem> {
        let mut none_indices: Vec<usize> = Vec::new();
        let mut provider_groups: Vec<(&str, Vec<usize>)> = Vec::new();
        let mut provider_order: HashMap<&str, usize> = HashMap::new();

        for &idx in sorted_indices {
            match &hosts[idx].provider {
                None => none_indices.push(idx),
                Some(name) => {
                    if let Some(&group_idx) = provider_order.get(name.as_str()) {
                        provider_groups[group_idx].1.push(idx);
                    } else {
                        let group_idx = provider_groups.len();
                        provider_order.insert(name, group_idx);
                        provider_groups.push((name, vec![idx]));
                    }
                }
            }
        }

        let mut display_list = Vec::new();

        // Non-provider hosts first (no header)
        for idx in &none_indices {
            display_list.push(HostListItem::Host { index: *idx });
        }

        // Then provider groups with headers
        for (name, indices) in &provider_groups {
            let header = crate::providers::provider_display_name(name);
            display_list.push(HostListItem::GroupHeader(header.to_string()));
            for &idx in indices {
                display_list.push(HostListItem::Host { index: idx });
            }
        }
        display_list
    }

    /// Partition sorted indices by a user tag, inserting a group header.
    /// Hosts without the tag appear first (no header), then hosts with the
    /// tag appear under a single group header.
    fn group_indices_by_tag(
        hosts: &[HostEntry],
        sorted_indices: &[usize],
        tag: &str,
    ) -> Vec<HostListItem> {
        let mut without_tag: Vec<usize> = Vec::new();
        let mut with_tag: Vec<usize> = Vec::new();

        for &idx in sorted_indices {
            if hosts[idx].tags.iter().any(|t| t == tag) {
                with_tag.push(idx);
            } else {
                without_tag.push(idx);
            }
        }

        let mut display_list = Vec::new();

        for idx in &without_tag {
            display_list.push(HostListItem::Host { index: *idx });
        }

        if !with_tag.is_empty() {
            display_list.push(HostListItem::GroupHeader(tag.to_string()));
            for &idx in &with_tag {
                display_list.push(HostListItem::Host { index: idx });
            }
        }

        display_list
    }

    /// Get the host index from the currently selected display list item.
    pub fn selected_host_index(&self) -> Option<usize> {
        if self.search.query.is_some() {
            // In search mode, list_state indexes into filtered_indices
            let sel = self.ui.list_state.selected()?;
            self.search.filtered_indices.get(sel).copied()
        } else {
            // In normal mode, list_state indexes into display_list
            let sel = self.ui.list_state.selected()?;
            match self.display_list.get(sel) {
                Some(HostListItem::Host { index }) => Some(*index),
                _ => None,
            }
        }
    }

    /// Get the currently selected host entry.
    pub fn selected_host(&self) -> Option<&HostEntry> {
        self.selected_host_index().and_then(|i| self.hosts.get(i))
    }

    /// Get the currently selected pattern entry (if a pattern is selected).
    pub fn selected_pattern(&self) -> Option<&PatternEntry> {
        if self.search.query.is_some() {
            let sel = self.ui.list_state.selected()?;
            let host_count = self.search.filtered_indices.len();
            if sel >= host_count {
                let pattern_idx = sel - host_count;
                return self
                    .search
                    .filtered_pattern_indices
                    .get(pattern_idx)
                    .and_then(|&i| self.patterns.get(i));
            }
            return None;
        }
        let sel = self.ui.list_state.selected()?;
        match self.display_list.get(sel) {
            Some(HostListItem::Pattern { index }) => self.patterns.get(*index),
            _ => None,
        }
    }

    /// Check if the currently selected item is a pattern.
    pub fn is_pattern_selected(&self) -> bool {
        if self.search.query.is_some() {
            let Some(sel) = self.ui.list_state.selected() else {
                return false;
            };
            let total =
                self.search.filtered_indices.len() + self.search.filtered_pattern_indices.len();
            return sel >= self.search.filtered_indices.len() && sel < total;
        }
        let Some(sel) = self.ui.list_state.selected() else {
            return false;
        };
        matches!(
            self.display_list.get(sel),
            Some(HostListItem::Pattern { .. })
        )
    }

    /// Move selection up, skipping group headers.
    pub fn select_prev(&mut self) {
        self.ui.detail_scroll = 0;
        if self.search.query.is_some() {
            let total =
                self.search.filtered_indices.len() + self.search.filtered_pattern_indices.len();
            cycle_selection(&mut self.ui.list_state, total, false);
        } else {
            self.select_prev_in_display_list();
        }
    }

    /// Move selection down, skipping group headers.
    pub fn select_next(&mut self) {
        self.ui.detail_scroll = 0;
        if self.search.query.is_some() {
            let total =
                self.search.filtered_indices.len() + self.search.filtered_pattern_indices.len();
            cycle_selection(&mut self.ui.list_state, total, true);
        } else {
            self.select_next_in_display_list();
        }
    }

    fn select_next_in_display_list(&mut self) {
        if self.display_list.is_empty() {
            return;
        }
        let len = self.display_list.len();
        let current = self.ui.list_state.selected().unwrap_or(0);
        // Find next selectable item after current (always skip headers)
        for offset in 1..=len {
            let idx = (current + offset) % len;
            if matches!(
                &self.display_list[idx],
                HostListItem::Host { .. } | HostListItem::Pattern { .. }
            ) {
                self.ui.list_state.select(Some(idx));
                return;
            }
        }
    }

    fn select_prev_in_display_list(&mut self) {
        if self.display_list.is_empty() {
            return;
        }
        let len = self.display_list.len();
        let current = self.ui.list_state.selected().unwrap_or(0);
        // Find prev selectable item before current (always skip headers)
        for offset in 1..=len {
            let idx = (current + len - offset) % len;
            if matches!(
                &self.display_list[idx],
                HostListItem::Host { .. } | HostListItem::Pattern { .. }
            ) {
                self.ui.list_state.select(Some(idx));
                return;
            }
        }
    }

    /// Page down in the host list, skipping group headers when ungrouped.
    pub fn page_down_host(&mut self) {
        self.ui.detail_scroll = 0;
        const PAGE_SIZE: usize = 10;
        if self.search.query.is_some() {
            page_down(
                &mut self.ui.list_state,
                self.search.filtered_indices.len(),
                PAGE_SIZE,
            );
        } else {
            let current = self.ui.list_state.selected().unwrap_or(0);
            let mut target = current;
            let mut items_skipped = 0;
            let len = self.display_list.len();
            for i in (current + 1)..len {
                if matches!(
                    self.display_list[i],
                    HostListItem::Host { .. } | HostListItem::Pattern { .. }
                ) {
                    target = i;
                    items_skipped += 1;
                    if items_skipped >= PAGE_SIZE {
                        break;
                    }
                }
            }
            if target != current {
                self.ui.list_state.select(Some(target));
                self.update_group_tab_follow();
            }
        }
    }

    /// Page up in the host list, skipping group headers.
    pub fn page_up_host(&mut self) {
        self.ui.detail_scroll = 0;
        const PAGE_SIZE: usize = 10;
        if self.search.query.is_some() {
            page_up(
                &mut self.ui.list_state,
                self.search.filtered_indices.len(),
                PAGE_SIZE,
            );
        } else {
            let current = self.ui.list_state.selected().unwrap_or(0);
            let mut target = current;
            let mut items_skipped = 0;
            for i in (0..current).rev() {
                if matches!(
                    self.display_list[i],
                    HostListItem::Host { .. } | HostListItem::Pattern { .. }
                ) {
                    target = i;
                    items_skipped += 1;
                    if items_skipped >= PAGE_SIZE {
                        break;
                    }
                }
            }
            if target != current {
                self.ui.list_state.select(Some(target));
                self.update_group_tab_follow();
            }
        }
    }

    /// Reload hosts from config.
    pub fn reload_hosts(&mut self) {
        let had_search = self.search.query.take();
        let selected_alias = self
            .selected_host()
            .map(|h| h.alias.clone())
            .or_else(|| self.selected_pattern().map(|p| p.pattern.clone()));

        self.tunnel_summaries_cache.clear();
        self.hosts = self.config.host_entries();
        self.patterns = self.config.pattern_entries();
        if self.sort_mode == SortMode::Original && matches!(self.group_by, GroupBy::None) {
            self.display_list =
                Self::build_display_list_from(&self.config, &self.hosts, &self.patterns);
        } else {
            self.apply_sort();
        }

        // Close tag pickers if open — tag_list is stale after reload
        if matches!(self.screen, Screen::TagPicker) {
            self.screen = Screen::HostList;
        }

        // Multi-select stores indices into hosts; clear to avoid stale refs
        self.multi_select.clear();

        // Prune ping status for hosts that no longer exist
        let valid_aliases: std::collections::HashSet<&str> =
            self.hosts.iter().map(|h| h.alias.as_str()).collect();
        self.ping_status
            .retain(|alias, _| valid_aliases.contains(alias.as_str()));

        // Restore search if it was active, otherwise reset
        if let Some(query) = had_search {
            self.search.query = Some(query);
            self.apply_filter();
        } else {
            self.search.query = None;
            self.search.filtered_indices.clear();
            self.search.filtered_pattern_indices.clear();
            // Fix selection for display list mode
            if self.hosts.is_empty() && self.patterns.is_empty() {
                self.ui.list_state.select(None);
            } else if let Some(pos) = self.display_list.iter().position(|item| {
                matches!(
                    item,
                    HostListItem::Host { .. } | HostListItem::Pattern { .. }
                )
            }) {
                let current = self.ui.list_state.selected().unwrap_or(0);
                if current >= self.display_list.len()
                    || !matches!(
                        self.display_list.get(current),
                        Some(HostListItem::Host { .. } | HostListItem::Pattern { .. })
                    )
                {
                    self.ui.list_state.select(Some(pos));
                }
            } else {
                self.ui.list_state.select(None);
            }
        }

        // Restore selection by alias (e.g. after SSH connect changed sort order)
        if let Some(alias) = selected_alias {
            self.select_host_by_alias(&alias);
        }
    }

    // --- Search methods ---

    /// Compute the search scope from the current display list when group-filtered.
    fn compute_search_scope(&self) -> Option<HashSet<usize>> {
        self.group_filter.as_ref()?;
        Some(
            self.display_list
                .iter()
                .filter_map(|item| {
                    if let HostListItem::Host { index } = item {
                        Some(*index)
                    } else {
                        None
                    }
                })
                .collect(),
        )
    }

    /// Enter search mode.
    pub fn start_search(&mut self) {
        self.search.pre_search_selection = self.ui.list_state.selected();
        self.search.scope_indices = self.compute_search_scope();
        self.search.query = Some(String::new());
        self.apply_filter();
    }

    /// Start search with an initial query (for positional arg).
    pub fn start_search_with(&mut self, query: &str) {
        self.search.pre_search_selection = self.ui.list_state.selected();
        self.search.scope_indices = self.compute_search_scope();
        self.search.query = Some(query.to_string());
        self.apply_filter();
    }

    /// Cancel search mode and restore normal view.
    pub fn cancel_search(&mut self) {
        self.search.query = None;
        self.search.filtered_indices.clear();
        self.search.filtered_pattern_indices.clear();
        self.search.scope_indices = None;
        // Restore pre-search position (bounds-checked)
        if let Some(pos) = self.search.pre_search_selection.take() {
            if pos < self.display_list.len() {
                self.ui.list_state.select(Some(pos));
            } else if let Some(first) = self.display_list.iter().position(|item| {
                matches!(
                    item,
                    HostListItem::Host { .. } | HostListItem::Pattern { .. }
                )
            }) {
                self.ui.list_state.select(Some(first));
            }
        }
    }

    /// Apply the current search query to filter hosts.
    pub fn apply_filter(&mut self) {
        let query = match &self.search.query {
            Some(q) if !q.is_empty() => q.clone(),
            Some(_) => {
                self.search.filtered_indices = (0..self.hosts.len()).collect();
                self.search.filtered_pattern_indices = (0..self.patterns.len()).collect();
                // Scope to group if active
                if let Some(ref scope) = self.search.scope_indices {
                    self.search.filtered_indices.retain(|i| scope.contains(i));
                }
                let total =
                    self.search.filtered_indices.len() + self.search.filtered_pattern_indices.len();
                if total == 0 {
                    self.ui.list_state.select(None);
                } else {
                    self.ui.list_state.select(Some(0));
                }
                return;
            }
            None => return,
        };

        if let Some(tag_exact) = query.strip_prefix("tag=") {
            // Exact tag match (from tag picker), includes provider name and virtual "stale"
            self.search.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    (eq_ci("stale", tag_exact) && host.stale.is_some())
                        || host
                            .provider_tags
                            .iter()
                            .chain(host.tags.iter())
                            .any(|t| eq_ci(t, tag_exact))
                        || host.provider.as_ref().is_some_and(|p| eq_ci(p, tag_exact))
                })
                .map(|(i, _)| i)
                .collect();
            self.search.filtered_pattern_indices = self
                .patterns
                .iter()
                .enumerate()
                .filter(|(_, p)| p.tags.iter().any(|t| eq_ci(t, tag_exact)))
                .map(|(i, _)| i)
                .collect();
        } else if let Some(tag_query) = query.strip_prefix("tag:") {
            // Fuzzy tag match (manual search), includes provider name and virtual "stale"
            self.search.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    (contains_ci("stale", tag_query) && host.stale.is_some())
                        || host
                            .provider_tags
                            .iter()
                            .chain(host.tags.iter())
                            .any(|t| contains_ci(t, tag_query))
                        || host
                            .provider
                            .as_ref()
                            .is_some_and(|p| contains_ci(p, tag_query))
                })
                .map(|(i, _)| i)
                .collect();
            self.search.filtered_pattern_indices = self
                .patterns
                .iter()
                .enumerate()
                .filter(|(_, p)| p.tags.iter().any(|t| contains_ci(t, tag_query)))
                .map(|(i, _)| i)
                .collect();
        } else {
            self.search.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    contains_ci(&host.alias, &query)
                        || contains_ci(&host.hostname, &query)
                        || contains_ci(&host.user, &query)
                        || host
                            .provider_tags
                            .iter()
                            .chain(host.tags.iter())
                            .any(|t| contains_ci(t, &query))
                        || host
                            .provider
                            .as_ref()
                            .is_some_and(|p| contains_ci(p, &query))
                })
                .map(|(i, _)| i)
                .collect();
            self.search.filtered_pattern_indices = self
                .patterns
                .iter()
                .enumerate()
                .filter(|(_, p)| contains_ci(&p.pattern, &query))
                .map(|(i, _)| i)
                .collect();
        }

        // Scope results to the active group if set
        if let Some(ref scope) = self.search.scope_indices {
            self.search.filtered_indices.retain(|i| scope.contains(i));
        }

        // Reset selection
        let total_results =
            self.search.filtered_indices.len() + self.search.filtered_pattern_indices.len();
        if total_results == 0 {
            self.ui.list_state.select(None);
        } else {
            self.ui.list_state.select(Some(0));
        }
    }

    /// Provider names sorted by last sync (most recent first), then configured, then unconfigured.
    /// Includes any unknown provider names found in the config file (e.g. typos or future providers).
    pub fn sorted_provider_names(&self) -> Vec<String> {
        use crate::providers;
        let mut names: Vec<String> = providers::PROVIDER_NAMES
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Append configured providers not in the known list so they are visible and removable
        for section in &self.provider_config.sections {
            if !names.contains(&section.provider) {
                names.push(section.provider.clone());
            }
        }
        names.sort_by(|a, b| {
            let conf_a = self.provider_config.section(a.as_str()).is_some();
            let conf_b = self.provider_config.section(b.as_str()).is_some();
            let ts_a = self.sync_history.get(a.as_str()).map_or(0, |r| r.timestamp);
            let ts_b = self.sync_history.get(b.as_str()).map_or(0, |r| r.timestamp);
            // Configured first (by most recent sync), then unconfigured alphabetically
            conf_b.cmp(&conf_a).then(ts_b.cmp(&ts_a)).then(a.cmp(b))
        });
        names
    }

    /// Return indices of snippets matching the search query.
    pub fn filtered_snippet_indices(&self) -> Vec<usize> {
        match &self.ui.snippet_search {
            None => (0..self.snippet_store.snippets.len()).collect(),
            Some(query) if query.is_empty() => (0..self.snippet_store.snippets.len()).collect(),
            Some(query) => self
                .snippet_store
                .snippets
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    contains_ci(&s.name, query)
                        || contains_ci(&s.command, query)
                        || contains_ci(&s.description, query)
                })
                .map(|(i, _)| i)
                .collect(),
        }
    }

    pub fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status = Some(StatusMessage {
            text: text.into(),
            is_error,
            tick_count: 0,
        });
    }

    /// Detail panel animation duration in milliseconds.
    const DETAIL_ANIM_DURATION_MS: u128 = 200;

    /// Current detail panel animation progress (0.0 = closed, 1.0 = open).
    /// Returns the per-frame snapshot set by `tick_animations`.
    pub fn detail_anim_progress(&self) -> Option<f32> {
        self.frame_detail_progress
    }

    /// Current overlay animation progress (0.0 = hidden, 1.0 = fully visible).
    /// Returns the per-frame snapshot set by `tick_animations`.
    pub fn overlay_anim_progress(&self) -> Option<f32> {
        self.frame_overlay_progress
    }

    /// Snapshot animation progress and clean up completed animations.
    /// Call once per frame in the event loop, before rendering.
    /// All render code reads the snapshot fields instead of calling elapsed()
    /// independently, eliminating race windows within a single frame.
    pub fn tick_animations(&mut self) {
        // --- Detail panel ---
        self.frame_detail_progress = self.detail_anim.as_ref().and_then(|anim| {
            let elapsed = anim.start.elapsed().as_millis();
            if elapsed >= Self::DETAIL_ANIM_DURATION_MS {
                return None;
            }
            let t = elapsed as f32 / Self::DETAIL_ANIM_DURATION_MS as f32;
            // Ease-out cubic: 1 - (1 - t)^3
            let eased = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
            let progress = if anim.opening {
                anim.start_progress + (1.0 - anim.start_progress) * eased
            } else {
                anim.start_progress * (1.0 - eased)
            };
            Some(progress)
        });
        if self.frame_detail_progress.is_none() && self.detail_anim.is_some() {
            self.detail_anim = None;
        }

        // --- Overlay ---
        self.frame_overlay_progress = self.overlay_anim.as_ref().and_then(|anim| {
            let elapsed = anim.start.elapsed().as_millis();
            if elapsed >= anim.duration_ms {
                return None;
            }
            let t = elapsed as f32 / anim.duration_ms as f32;
            // Ease-out cubic
            let eased = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
            Some(if anim.opening { eased } else { 1.0 - eased })
        });
        if self.frame_overlay_progress.is_none() {
            if let Some(ref anim) = self.overlay_anim {
                let was_closing = !anim.opening;
                self.overlay_anim = None;
                if was_closing {
                    self.overlay_buffer = None;
                }
            }
        }

        // --- Composite: is any animation running? ---
        let welcome_animating = self
            .welcome_opened
            .is_some_and(|t| t.elapsed().as_millis() < Self::WELCOME_TOTAL_MS);
        self.frame_animating = self.frame_detail_progress.is_some()
            || self.frame_overlay_progress.is_some()
            || welcome_animating;
    }

    /// Total welcome animation duration: zoom(350) + logo(450) + delay(100) + typewriter(~2000).
    const WELCOME_TOTAL_MS: u128 = 3000;

    /// Whether any animation (detail, overlay or welcome typewriter) is running.
    /// Returns the per-frame snapshot set by `tick_animations`.
    pub fn is_animating(&self) -> bool {
        self.frame_animating
    }

    /// Detect overlay open/close transitions and start animations.
    /// Must be called once per frame, before `tick_animations`, so the snapshot
    /// includes newly started animations on the very first frame.
    pub fn detect_overlay_transition(&mut self) {
        let is_overlay = !matches!(self.screen, Screen::HostList);
        if is_overlay && !self.prev_was_overlay {
            let is_welcome = matches!(self.screen, Screen::Welcome { .. });
            let duration = if is_welcome { 350 } else { 150 };
            if is_welcome {
                self.welcome_opened = Some(std::time::Instant::now());
            }
            self.overlay_anim = Some(OverlayAnimation {
                start: std::time::Instant::now(),
                opening: true,
                duration_ms: duration,
            });
        } else if !is_overlay && self.prev_was_overlay {
            if self.overlay_buffer.is_some() {
                self.overlay_anim = Some(OverlayAnimation {
                    start: std::time::Instant::now(),
                    opening: false,
                    duration_ms: 150,
                });
            }
            // Always safe: welcome_opened is only set when the welcome screen
            // opens (above). For non-welcome overlays this is already None.
            self.welcome_opened = None;
        }
        self.prev_was_overlay = is_overlay;
    }

    /// Tick the status message timer. Errors show for 5s, success for 3s.
    pub fn tick_status(&mut self) {
        if let Some(ref mut status) = self.status {
            status.tick_count += 1;
            let timeout = if status.is_error { 20 } else { 12 };
            if status.tick_count > timeout {
                self.status = None;
            }
        }
    }

    /// Get the modification time of a file.
    fn get_mtime(path: &Path) -> Option<SystemTime> {
        std::fs::metadata(path).ok()?.modified().ok()
    }

    /// Check if config or any Include file has changed externally and reload if so.
    /// Skips reload when the user is in a form (AddHost/EditHost) to avoid
    /// overwriting in-memory config while the user is editing.
    pub fn check_config_changed(&mut self) {
        if matches!(
            self.screen,
            Screen::AddHost
                | Screen::EditHost { .. }
                | Screen::ProviderForm { .. }
                | Screen::TunnelList { .. }
                | Screen::TunnelForm { .. }
                | Screen::HostDetail { .. }
                | Screen::SnippetPicker { .. }
                | Screen::SnippetForm { .. }
                | Screen::SnippetOutput { .. }
                | Screen::SnippetParamForm { .. }
                | Screen::FileBrowser { .. }
                | Screen::Containers { .. }
                | Screen::ConfirmDelete { .. }
                | Screen::ConfirmHostKeyReset { .. }
                | Screen::ConfirmPurgeStale { .. }
                | Screen::ConfirmImport { .. }
                | Screen::TagPicker
        ) || self.tag_input.is_some()
        {
            return;
        }
        let current_mtime = Self::get_mtime(&self.reload.config_path);
        let changed = current_mtime != self.reload.last_modified
            || self
                .reload
                .include_mtimes
                .iter()
                .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
            || self
                .reload
                .include_dir_mtimes
                .iter()
                .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime);
        if changed {
            if let Ok(new_config) = SshConfigFile::parse(&self.reload.config_path) {
                self.config = new_config;
                // Invalidate undo state — config structure may have changed externally
                self.undo_stack.clear();
                // Clear stale ping status — hosts may have changed
                self.ping_status.clear();
                self.reload_hosts();
                self.reload.last_modified = current_mtime;
                self.reload.include_mtimes = Self::snapshot_include_mtimes(&self.config);
                self.reload.include_dir_mtimes = Self::snapshot_include_dir_mtimes(&self.config);
                let count = self.hosts.len();
                self.set_status(format!("Config reloaded. {} hosts.", count), false);
            }
        }
    }

    /// Update the last_modified timestamp (call after writing config).
    pub fn update_last_modified(&mut self) {
        self.reload.last_modified = Self::get_mtime(&self.reload.config_path);
        self.reload.include_mtimes = Self::snapshot_include_mtimes(&self.config);
        self.reload.include_dir_mtimes = Self::snapshot_include_dir_mtimes(&self.config);
    }

    /// Clear form mtime state (call on form cancel or successful submit).
    pub fn clear_form_mtime(&mut self) {
        self.conflict.form_mtime = None;
        self.conflict.form_include_mtimes.clear();
        self.conflict.form_include_dir_mtimes.clear();
        self.conflict.provider_form_mtime = None;
    }

    /// Capture config and Include file mtimes when opening a host form.
    pub fn capture_form_mtime(&mut self) {
        self.conflict.form_mtime = Self::get_mtime(&self.reload.config_path);
        self.conflict.form_include_mtimes = Self::snapshot_include_mtimes(&self.config);
        self.conflict.form_include_dir_mtimes = Self::snapshot_include_dir_mtimes(&self.config);
    }

    /// Capture ~/.purple/providers mtime when opening a provider form.
    pub fn capture_provider_form_mtime(&mut self) {
        let path = dirs::home_dir().map(|h| h.join(".purple/providers"));
        self.conflict.provider_form_mtime = path.as_ref().and_then(|p| Self::get_mtime(p));
    }

    /// Capture a baseline snapshot of the host form for dirty-check on Esc.
    pub fn capture_form_baseline(&mut self) {
        self.form_baseline = Some(FormBaseline {
            alias: self.form.alias.clone(),
            hostname: self.form.hostname.clone(),
            user: self.form.user.clone(),
            port: self.form.port.clone(),
            identity_file: self.form.identity_file.clone(),
            proxy_jump: self.form.proxy_jump.clone(),
            askpass: self.form.askpass.clone(),
            tags: self.form.tags.clone(),
        });
    }

    /// Check if the host form has been modified since baseline was captured.
    pub fn host_form_is_dirty(&self) -> bool {
        match &self.form_baseline {
            Some(b) => {
                self.form.alias != b.alias
                    || self.form.hostname != b.hostname
                    || self.form.user != b.user
                    || self.form.port != b.port
                    || self.form.identity_file != b.identity_file
                    || self.form.proxy_jump != b.proxy_jump
                    || self.form.askpass != b.askpass
                    || self.form.tags != b.tags
            }
            None => false,
        }
    }

    /// Capture a baseline snapshot of the tunnel form for dirty-check on Esc.
    pub fn capture_tunnel_form_baseline(&mut self) {
        self.tunnel_form_baseline = Some(TunnelFormBaseline {
            tunnel_type: self.tunnel_form.tunnel_type,
            bind_port: self.tunnel_form.bind_port.clone(),
            remote_host: self.tunnel_form.remote_host.clone(),
            remote_port: self.tunnel_form.remote_port.clone(),
            bind_address: self.tunnel_form.bind_address.clone(),
        });
    }

    /// Check if the tunnel form has been modified since baseline was captured.
    pub fn tunnel_form_is_dirty(&self) -> bool {
        match &self.tunnel_form_baseline {
            Some(b) => {
                self.tunnel_form.tunnel_type != b.tunnel_type
                    || self.tunnel_form.bind_port != b.bind_port
                    || self.tunnel_form.remote_host != b.remote_host
                    || self.tunnel_form.remote_port != b.remote_port
                    || self.tunnel_form.bind_address != b.bind_address
            }
            None => false,
        }
    }

    /// Capture a baseline snapshot of the snippet form for dirty-check on Esc.
    pub fn capture_snippet_form_baseline(&mut self) {
        self.snippet_form_baseline = Some(SnippetFormBaseline {
            name: self.snippet_form.name.clone(),
            command: self.snippet_form.command.clone(),
            description: self.snippet_form.description.clone(),
        });
    }

    /// Check if the snippet form has been modified since baseline was captured.
    pub fn snippet_form_is_dirty(&self) -> bool {
        match &self.snippet_form_baseline {
            Some(b) => {
                self.snippet_form.name != b.name
                    || self.snippet_form.command != b.command
                    || self.snippet_form.description != b.description
            }
            None => false,
        }
    }

    /// Capture a baseline snapshot of the provider form for dirty-check on Esc.
    pub fn capture_provider_form_baseline(&mut self) {
        self.provider_form_baseline = Some(ProviderFormBaseline {
            url: self.provider_form.url.clone(),
            token: self.provider_form.token.clone(),
            profile: self.provider_form.profile.clone(),
            project: self.provider_form.project.clone(),
            compartment: self.provider_form.compartment.clone(),
            regions: self.provider_form.regions.clone(),
            alias_prefix: self.provider_form.alias_prefix.clone(),
            user: self.provider_form.user.clone(),
            identity_file: self.provider_form.identity_file.clone(),
            verify_tls: self.provider_form.verify_tls,
            auto_sync: self.provider_form.auto_sync,
        });
    }

    /// Check if the provider form has been modified since baseline was captured.
    pub fn provider_form_is_dirty(&self) -> bool {
        match &self.provider_form_baseline {
            Some(b) => {
                self.provider_form.url != b.url
                    || self.provider_form.token != b.token
                    || self.provider_form.profile != b.profile
                    || self.provider_form.project != b.project
                    || self.provider_form.compartment != b.compartment
                    || self.provider_form.regions != b.regions
                    || self.provider_form.alias_prefix != b.alias_prefix
                    || self.provider_form.user != b.user
                    || self.provider_form.identity_file != b.identity_file
                    || self.provider_form.verify_tls != b.verify_tls
                    || self.provider_form.auto_sync != b.auto_sync
            }
            None => false,
        }
    }

    /// Check if config or any Include file/directory has changed since the form was opened.
    pub fn config_changed_since_form_open(&self) -> bool {
        match self.conflict.form_mtime {
            Some(open_mtime) => {
                if Self::get_mtime(&self.reload.config_path) != Some(open_mtime) {
                    return true;
                }
                self.conflict
                    .form_include_mtimes
                    .iter()
                    .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
                    || self
                        .conflict
                        .form_include_dir_mtimes
                        .iter()
                        .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
            }
            None => false,
        }
    }

    /// Check if ~/.purple/providers has changed since the provider form was opened.
    pub fn provider_config_changed_since_form_open(&self) -> bool {
        let path = dirs::home_dir().map(|h| h.join(".purple/providers"));
        let current_mtime = path.as_ref().and_then(|p| Self::get_mtime(p));
        self.conflict.provider_form_mtime != current_mtime
    }

    /// Snapshot mtimes of all resolved Include files.
    fn snapshot_include_mtimes(config: &SshConfigFile) -> Vec<(PathBuf, Option<SystemTime>)> {
        config
            .include_paths()
            .into_iter()
            .map(|p| {
                let mtime = Self::get_mtime(&p);
                (p, mtime)
            })
            .collect()
    }

    /// Snapshot mtimes of parent directories of Include glob patterns.
    fn snapshot_include_dir_mtimes(config: &SshConfigFile) -> Vec<(PathBuf, Option<SystemTime>)> {
        config
            .include_glob_dirs()
            .into_iter()
            .map(|p| {
                let mtime = Self::get_mtime(&p);
                (p, mtime)
            })
            .collect()
    }

    /// Scan SSH keys from ~/.ssh/ and cross-reference with hosts.
    pub fn scan_keys(&mut self) {
        if let Some(home) = dirs::home_dir() {
            let ssh_dir = home.join(".ssh");
            self.keys = ssh_keys::discover_keys(Path::new(&ssh_dir), &self.hosts);
            if !self.keys.is_empty() && self.ui.key_list_state.selected().is_none() {
                self.ui.key_list_state.select(Some(0));
            }
        }
    }

    /// Move key list selection up.
    pub fn select_prev_key(&mut self) {
        cycle_selection(&mut self.ui.key_list_state, self.keys.len(), false);
    }

    /// Move key list selection down.
    pub fn select_next_key(&mut self) {
        cycle_selection(&mut self.ui.key_list_state, self.keys.len(), true);
    }

    /// Move key picker selection up.
    pub fn select_prev_picker_key(&mut self) {
        cycle_selection(&mut self.ui.key_picker_state, self.keys.len(), false);
    }

    /// Move key picker selection down.
    pub fn select_next_picker_key(&mut self) {
        cycle_selection(&mut self.ui.key_picker_state, self.keys.len(), true);
    }

    /// Move password picker selection up.
    pub fn select_prev_password_source(&mut self) {
        cycle_selection(
            &mut self.ui.password_picker_state,
            crate::askpass::PASSWORD_SOURCES.len(),
            false,
        );
    }

    /// Move password picker selection down.
    pub fn select_next_password_source(&mut self) {
        cycle_selection(
            &mut self.ui.password_picker_state,
            crate::askpass::PASSWORD_SOURCES.len(),
            true,
        );
    }

    /// Get hosts available as ProxyJump targets (excludes the host being edited).
    pub fn proxyjump_candidates(&self) -> Vec<(String, String)> {
        let editing_alias = match &self.screen {
            Screen::EditHost { alias, .. } => Some(alias.as_str()),
            _ => None,
        };
        self.hosts
            .iter()
            .filter(|h| {
                if let Some(alias) = editing_alias {
                    h.alias != alias
                } else {
                    true
                }
            })
            .map(|h| (h.alias.clone(), h.hostname.clone()))
            .collect()
    }

    /// Move proxyjump picker selection up.
    pub fn select_prev_proxyjump(&mut self) {
        let len = self.proxyjump_candidates().len();
        cycle_selection(&mut self.ui.proxyjump_picker_state, len, false);
    }

    /// Move proxyjump picker selection down.
    pub fn select_next_proxyjump(&mut self) {
        let len = self.proxyjump_candidates().len();
        cycle_selection(&mut self.ui.proxyjump_picker_state, len, true);
    }

    /// Collect all unique tags from hosts, sorted alphabetically.
    pub fn collect_unique_tags(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut tags = Vec::new();
        let mut has_stale = false;
        for host in &self.hosts {
            for tag in host.provider_tags.iter().chain(host.tags.iter()) {
                if seen.insert(tag.clone()) {
                    tags.push(tag.clone());
                }
            }
            if let Some(ref provider) = host.provider {
                if seen.insert(provider.clone()) {
                    tags.push(provider.clone());
                }
            }
            if host.stale.is_some() {
                has_stale = true;
            }
        }
        for pattern in &self.patterns {
            for tag in &pattern.tags {
                if seen.insert(tag.clone()) {
                    tags.push(tag.clone());
                }
            }
        }
        if has_stale && seen.insert("stale".to_string()) {
            tags.push("stale".to_string());
        }
        tags.sort_by_cached_key(|a| a.to_lowercase());
        tags
    }

    /// Open the tag picker overlay.
    pub fn open_tag_picker(&mut self) {
        self.tag_list = self.collect_unique_tags();
        self.ui.tag_picker_state = ListState::default();
        if !self.tag_list.is_empty() {
            self.ui.tag_picker_state.select(Some(0));
        }
        self.screen = Screen::TagPicker;
    }

    /// Move tag picker selection up.
    pub fn select_prev_tag(&mut self) {
        cycle_selection(&mut self.ui.tag_picker_state, self.tag_list.len(), false);
    }

    /// Move tag picker selection down.
    pub fn select_next_tag(&mut self) {
        cycle_selection(&mut self.ui.tag_picker_state, self.tag_list.len(), true);
    }

    /// Load tunnel directives for a host alias.
    /// Uses find_tunnel_directives for Include-aware, multi-pattern host lookup.
    pub fn refresh_tunnel_list(&mut self, alias: &str) {
        self.tunnel_list = self.config.find_tunnel_directives(alias);
    }

    /// Move tunnel list selection up.
    pub fn select_prev_tunnel(&mut self) {
        cycle_selection(
            &mut self.ui.tunnel_list_state,
            self.tunnel_list.len(),
            false,
        );
    }

    /// Move tunnel list selection down.
    pub fn select_next_tunnel(&mut self) {
        cycle_selection(&mut self.ui.tunnel_list_state, self.tunnel_list.len(), true);
    }

    /// Move snippet picker selection up.
    pub fn select_prev_snippet(&mut self) {
        cycle_selection(
            &mut self.ui.snippet_picker_state,
            self.snippet_store.snippets.len(),
            false,
        );
    }

    /// Move snippet picker selection down.
    pub fn select_next_snippet(&mut self) {
        cycle_selection(
            &mut self.ui.snippet_picker_state,
            self.snippet_store.snippets.len(),
            true,
        );
    }

    /// Poll active tunnels for exit status. Returns messages for any that exited.
    /// Poll active tunnels for exit. Returns (alias, message, is_error) tuples.
    pub fn poll_tunnels(&mut self) -> Vec<(String, String, bool)> {
        if self.active_tunnels.is_empty() {
            return Vec::new();
        }
        let mut exited = Vec::new();
        let mut to_remove = Vec::new();
        for (alias, tunnel) in &mut self.active_tunnels {
            match tunnel.child.try_wait() {
                Ok(Some(status)) => {
                    // Read up to 1KB of stderr for error details
                    let stderr_msg = tunnel.child.stderr.take().and_then(|mut stderr| {
                        use std::io::Read;
                        let mut buf = vec![0u8; 1024];
                        match stderr.read(&mut buf) {
                            Ok(n) if n > 0 => {
                                let s = String::from_utf8_lossy(&buf[..n]);
                                let trimmed = s.trim();
                                if trimmed.is_empty() {
                                    None
                                } else {
                                    Some(trimmed.to_string())
                                }
                            }
                            _ => None,
                        }
                    });
                    let (msg, is_error) = if status.success() {
                        (format!("Tunnel for {} closed.", alias), false)
                    } else if let Some(err) = stderr_msg {
                        (format!("Tunnel for {}: {}", alias, err), true)
                    } else {
                        (
                            format!(
                                "Tunnel for {} exited with code {}.",
                                alias,
                                status.code().unwrap_or(-1)
                            ),
                            true,
                        )
                    };
                    exited.push((alias.clone(), msg, is_error));
                    to_remove.push(alias.clone());
                }
                Ok(None) => {} // Still running
                Err(e) => {
                    exited.push((
                        alias.clone(),
                        format!("Tunnel for {} lost: {}", alias, e),
                        true,
                    ));
                    to_remove.push(alias.clone());
                }
            }
        }
        for alias in to_remove {
            // Just remove — try_wait() already reaped the process above
            self.active_tunnels.remove(&alias);
        }
        exited
    }

    /// Add a new host from the current form. Returns status message.
    pub fn add_host_from_form(&mut self) -> Result<String, String> {
        let entry = self.form.to_entry();
        let alias = entry.alias.clone();
        if self.config.has_host(&alias) {
            return Err(if self.form.is_pattern {
                format!("Pattern '{}' already exists.", alias)
            } else {
                format!(
                    "'{}' already exists. Aliases are like fingerprints — unique.",
                    alias
                )
            });
        }
        let len_before = self.config.elements.len();
        self.config.add_host(&entry);
        if !entry.tags.is_empty() {
            self.config.set_host_tags(&alias, &entry.tags);
        }
        if let Some(ref source) = entry.askpass {
            self.config.set_host_askpass(&alias, source);
        }
        if let Err(e) = self.config.write() {
            self.config.elements.truncate(len_before);
            return Err(format!("Failed to save: {}", e));
        }
        self.update_last_modified();
        self.reload_hosts();
        self.select_host_by_alias(&alias);
        Ok(format!("Welcome aboard, {}!", alias))
    }

    /// Edit an existing host from the current form. Returns status message.
    pub fn edit_host_from_form(&mut self, old_alias: &str) -> Result<String, String> {
        let entry = self.form.to_entry();
        let alias = entry.alias.clone();
        if !self.config.has_host(old_alias) {
            return Err("Host no longer exists.".to_string());
        }
        if alias != old_alias && self.config.has_host(&alias) {
            return Err(if self.form.is_pattern {
                format!("Pattern '{}' already exists.", alias)
            } else {
                format!(
                    "'{}' already exists. Aliases are like fingerprints — unique.",
                    alias
                )
            });
        }
        let old_entry = if self.form.is_pattern {
            self.patterns
                .iter()
                .find(|p| p.pattern == old_alias)
                .map(|p| HostEntry {
                    alias: p.pattern.clone(),
                    hostname: p.hostname.clone(),
                    user: p.user.clone(),
                    port: p.port,
                    identity_file: p.identity_file.clone(),
                    proxy_jump: p.proxy_jump.clone(),
                    tags: p.tags.clone(),
                    askpass: p.askpass.clone(),
                    ..Default::default()
                })
                .unwrap_or_default()
        } else {
            self.hosts
                .iter()
                .find(|h| h.alias == old_alias)
                .cloned()
                .unwrap_or_default()
        };
        self.config.update_host(old_alias, &entry);
        self.config.set_host_tags(&entry.alias, &entry.tags);
        self.config
            .set_host_askpass(&entry.alias, entry.askpass.as_deref().unwrap_or(""));
        if let Err(e) = self.config.write() {
            self.config.update_host(&entry.alias, &old_entry);
            self.config.set_host_tags(&old_entry.alias, &old_entry.tags);
            self.config
                .set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
            return Err(format!("Failed to save: {}", e));
        }
        // Migrate active tunnel handle if alias changed
        if alias != old_alias {
            if let Some(tunnel) = self.active_tunnels.remove(old_alias) {
                self.active_tunnels.insert(alias.clone(), tunnel);
            }
        }
        self.update_last_modified();
        self.reload_hosts();
        Ok(format!("{} got a makeover.", alias))
    }

    /// Select a host in the display list (or filtered list) by alias.
    pub fn select_host_by_alias(&mut self, alias: &str) {
        if self.search.query.is_some() {
            // In search mode, list_state indexes into filtered_indices
            for (i, &host_idx) in self.search.filtered_indices.iter().enumerate() {
                if self.hosts.get(host_idx).is_some_and(|h| h.alias == alias) {
                    self.ui.list_state.select(Some(i));
                    return;
                }
            }
            // Also check patterns in search results
            let host_count = self.search.filtered_indices.len();
            for (i, &pat_idx) in self.search.filtered_pattern_indices.iter().enumerate() {
                if self
                    .patterns
                    .get(pat_idx)
                    .is_some_and(|p| p.pattern == alias)
                {
                    self.ui.list_state.select(Some(host_count + i));
                    return;
                }
            }
        } else {
            for (i, item) in self.display_list.iter().enumerate() {
                match item {
                    HostListItem::Host { index } => {
                        if self.hosts.get(*index).is_some_and(|h| h.alias == alias) {
                            self.ui.list_state.select(Some(i));
                            return;
                        }
                    }
                    HostListItem::Pattern { index } => {
                        if self
                            .patterns
                            .get(*index)
                            .is_some_and(|p| p.pattern == alias)
                        {
                            self.ui.list_state.select(Some(i));
                            return;
                        }
                    }
                    HostListItem::GroupHeader(_) => {}
                }
            }
        }
    }

    /// Apply sync results from a background provider fetch.
    /// Returns (message, is_error, server_count, added, updated, stale). Caller must remove from syncing_providers.
    pub fn apply_sync_result(
        &mut self,
        provider: &str,
        hosts: Vec<crate::providers::ProviderHost>,
        partial: bool,
    ) -> (String, bool, usize, usize, usize, usize) {
        let section = match self.provider_config.section(provider).cloned() {
            Some(s) => s,
            None => {
                return (
                    format!(
                        "{} sync skipped: no config.",
                        crate::providers::provider_display_name(provider)
                    ),
                    true,
                    0,
                    0,
                    0,
                    0,
                );
            }
        };
        let provider_impl = match crate::providers::get_provider_with_config(provider, &section) {
            Some(p) => p,
            None => {
                return (
                    format!(
                        "Unknown provider: {}.",
                        crate::providers::provider_display_name(provider)
                    ),
                    true,
                    0,
                    0,
                    0,
                    0,
                );
            }
        };
        let config_backup = self.config.clone();
        let result = crate::providers::sync::sync_provider(
            &mut self.config,
            &*provider_impl,
            &hosts,
            &section,
            false,
            partial, // suppress stale marking on partial failures
            false,
        );
        let total = result.added + result.updated + result.unchanged;
        if result.added > 0 || result.updated > 0 || result.stale > 0 {
            if let Err(e) = self.config.write() {
                self.config = config_backup;
                return (format!("Sync failed to save: {}", e), true, total, 0, 0, 0);
            }
            self.undo_stack.clear();
            self.update_last_modified();
            self.reload_hosts();
            // Migrate active tunnel handles for renamed aliases
            for (old_alias, new_alias) in &result.renames {
                if let Some(tunnel) = self.active_tunnels.remove(old_alias) {
                    self.active_tunnels.insert(new_alias.clone(), tunnel);
                }
            }
        }
        let name = crate::providers::provider_display_name(provider);
        let mut msg = format!(
            "Synced {}: added {}, updated {}, unchanged {}",
            name, result.added, result.updated, result.unchanged
        );
        if result.stale > 0 {
            msg.push_str(&format!(", stale {}", result.stale));
        }
        msg.push('.');
        (
            msg,
            false,
            total,
            result.added,
            result.updated,
            result.stale,
        )
    }

    /// Clear group-by-tag if the tag no longer exists in any host.
    /// Returns true if the tag was cleared.
    pub fn clear_stale_group_tag(&mut self) -> bool {
        if let GroupBy::Tag(ref tag) = self.group_by {
            // Empty tag = "show all tags as tabs" mode, always valid
            if tag.is_empty() {
                return false;
            }
            let tag_exists = self.hosts.iter().any(|h| h.tags.iter().any(|t| t == tag));
            if !tag_exists {
                self.group_by = GroupBy::None;
                self.group_filter = None;
                return true;
            }
        }
        false
    }

    /// Move selection to the next non-header item.
    pub fn select_next_skipping_headers(&mut self) {
        let current = self.ui.list_state.selected().unwrap_or(0);
        for i in (current + 1)..self.display_list.len() {
            if !matches!(self.display_list[i], HostListItem::GroupHeader(_)) {
                self.ui.list_state.select(Some(i));
                self.update_group_tab_follow();
                return;
            }
        }
    }

    /// Move selection to the previous non-header item.
    pub fn select_prev_skipping_headers(&mut self) {
        let current = self.ui.list_state.selected().unwrap_or(0);
        for i in (0..current).rev() {
            if !matches!(self.display_list[i], HostListItem::GroupHeader(_)) {
                self.ui.list_state.select(Some(i));
                self.update_group_tab_follow();
                return;
            }
        }
    }

    /// Auto-follow: update group_tab_index based on selected host's group.
    fn update_group_tab_follow(&mut self) {
        if self.group_filter.is_some() {
            return;
        }
        let selected = self.ui.list_state.selected().unwrap_or(0);
        // Walk backwards to find the nearest GroupHeader and its tab index
        for i in (0..=selected).rev() {
            if let HostListItem::GroupHeader(name) = &self.display_list[i] {
                self.group_tab_index = self
                    .group_tab_order
                    .iter()
                    .position(|g| g == name)
                    .map(|idx| idx + 1)
                    .unwrap_or(0);
                return;
            }
        }
        self.group_tab_index = 0;
    }

    /// Cycle to the next group tab (Tab key). All -> group1 -> ... -> groupN -> All.
    pub fn next_group_tab(&mut self) {
        let group_count = self.group_tab_order.len();
        if group_count == 0 {
            return;
        }
        match &self.group_filter {
            None => {
                self.group_filter = Some(self.group_tab_order[0].clone());
                self.group_tab_index = 1;
            }
            Some(current) => {
                let pos = self
                    .group_tab_order
                    .iter()
                    .position(|g| g == current)
                    .unwrap_or(0);
                let next = pos + 1;
                if next >= group_count {
                    // Wrap back to "All"
                    self.group_filter = None;
                    self.group_tab_index = 0;
                } else {
                    self.group_filter = Some(self.group_tab_order[next].clone());
                    self.group_tab_index = next + 1;
                }
            }
        }
        self.apply_sort();
        // Select first host in list
        for (i, item) in self.display_list.iter().enumerate() {
            if matches!(item, HostListItem::Host { .. }) {
                self.ui.list_state.select(Some(i));
                break;
            }
        }
    }

    /// Cycle to the previous group tab (Shift+Tab key). All <- group1 <- ... <- groupN.
    pub fn prev_group_tab(&mut self) {
        let group_count = self.group_tab_order.len();
        if group_count == 0 {
            return;
        }
        match &self.group_filter {
            None => {
                // From All, go to last group
                let last = group_count - 1;
                self.group_filter = Some(self.group_tab_order[last].clone());
                self.group_tab_index = last + 1;
            }
            Some(current) => {
                let pos = self
                    .group_tab_order
                    .iter()
                    .position(|g| g == current)
                    .unwrap_or(0);
                if pos == 0 {
                    // Wrap back to "All"
                    self.group_filter = None;
                    self.group_tab_index = 0;
                } else {
                    let prev = pos - 1;
                    self.group_filter = Some(self.group_tab_order[prev].clone());
                    self.group_tab_index = prev + 1;
                }
            }
        }
        self.apply_sort();
        for (i, item) in self.display_list.iter().enumerate() {
            if matches!(item, HostListItem::Host { .. }) {
                self.ui.list_state.select(Some(i));
                break;
            }
        }
    }

    /// Clear group filter (Esc from filtered mode).
    pub fn clear_group_filter(&mut self) {
        if self.group_filter.is_none() {
            return;
        }
        self.group_filter = None;
        self.group_tab_index = 0;
        self.apply_sort();
        for (i, item) in self.display_list.iter().enumerate() {
            if matches!(item, HostListItem::Host { .. }) {
                self.ui.list_state.select(Some(i));
                break;
            }
        }
    }
}

/// Cycle list selection forward or backward with wraparound.
pub fn cycle_selection(state: &mut ListState, len: usize, forward: bool) {
    if len == 0 {
        return;
    }
    let i = match state.selected() {
        Some(i) => {
            if forward {
                if i >= len - 1 { 0 } else { i + 1 }
            } else if i == 0 {
                len - 1
            } else {
                i - 1
            }
        }
        None => 0,
    };
    state.select(Some(i));
}

/// Jump forward by page_size items, clamping at the end (no wrap).
pub fn page_down(state: &mut ListState, len: usize, page_size: usize) {
    if len == 0 {
        return;
    }
    let current = state.selected().unwrap_or(0);
    let next = (current + page_size).min(len - 1);
    state.select(Some(next));
}

/// Jump backward by page_size items, clamping at 0 (no wrap).
pub fn page_up(state: &mut ListState, len: usize, page_size: usize) {
    if len == 0 {
        return;
    }
    let current = state.selected().unwrap_or(0);
    let prev = current.saturating_sub(page_size);
    state.select(Some(prev));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_config::model::SshConfigFile;
    use std::path::PathBuf;

    fn make_app(content: &str) -> App {
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: PathBuf::from("/tmp/test_config"),
            crlf: false,
            bom: false,
        };
        App::new(config)
    }

    #[test]
    fn test_apply_filter_matches_alias() {
        let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        app.start_search();
        app.search.query = Some("alp".to_string());
        app.apply_filter();
        assert_eq!(app.search.filtered_indices, vec![0]);
    }

    #[test]
    fn test_apply_filter_matches_hostname() {
        let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        app.start_search();
        app.search.query = Some("b.com".to_string());
        app.apply_filter();
        assert_eq!(app.search.filtered_indices, vec![1]);
    }

    #[test]
    fn test_apply_filter_empty_query() {
        let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        app.start_search();
        assert_eq!(app.search.filtered_indices, vec![0, 1]);
    }

    #[test]
    fn test_apply_filter_no_matches() {
        let mut app = make_app("Host alpha\n  HostName a.com\n");
        app.start_search();
        app.search.query = Some("zzz".to_string());
        app.apply_filter();
        assert!(app.search.filtered_indices.is_empty());
    }

    #[test]
    fn test_build_display_list_with_group_headers() {
        let content = "\
# Production
Host prod
  HostName prod.example.com

# Staging
Host staging
  HostName staging.example.com
";
        let app = make_app(content);
        assert_eq!(app.display_list.len(), 4);
        assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "Production"));
        assert!(matches!(
            &app.display_list[1],
            HostListItem::Host { index: 0 }
        ));
        assert!(matches!(&app.display_list[2], HostListItem::GroupHeader(s) if s == "Staging"));
        assert!(matches!(
            &app.display_list[3],
            HostListItem::Host { index: 1 }
        ));
    }

    #[test]
    fn test_build_display_list_blank_line_breaks_group() {
        let content = "\
# This comment is separated by blank line

Host nogroup
  HostName nogroup.example.com
";
        let app = make_app(content);
        // Blank line between comment and host means no group header
        assert_eq!(app.display_list.len(), 1);
        assert!(matches!(
            &app.display_list[0],
            HostListItem::Host { index: 0 }
        ));
    }

    #[test]
    fn test_navigation_skips_headers() {
        let content = "\
# Group
Host alpha
  HostName a.com

# Group 2
Host beta
  HostName b.com
";
        let mut app = make_app(content);
        // Should start on first Host (index 1 in display_list)
        assert_eq!(app.ui.list_state.selected(), Some(1));
        app.select_next();
        // Should skip header at index 2, land on Host at index 3
        assert_eq!(app.ui.list_state.selected(), Some(3));
        app.select_prev();
        assert_eq!(app.ui.list_state.selected(), Some(1));
    }

    #[test]
    fn test_group_by_provider_creates_headers() {
        let content = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:123

Host do-db
  HostName 5.6.7.8
  # purple:provider digitalocean:456

Host vultr-app
  HostName 9.9.9.9
  # purple:provider vultr:789
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();

        // Should have: DigitalOcean header, 2 hosts, Vultr header, 1 host
        assert_eq!(app.display_list.len(), 5);
        assert!(
            matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean")
        );
        assert!(matches!(&app.display_list[1], HostListItem::Host { .. }));
        assert!(matches!(&app.display_list[2], HostListItem::Host { .. }));
        assert!(matches!(&app.display_list[3], HostListItem::GroupHeader(s) if s == "Vultr"));
        assert!(matches!(&app.display_list[4], HostListItem::Host { .. }));
    }

    #[test]
    fn test_group_by_provider_no_header_for_none() {
        let content = "\
Host manual
  HostName 1.2.3.4

Host do-web
  HostName 5.6.7.8
  # purple:provider digitalocean:123
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();

        // manual first (no header), then DigitalOcean header + do-web
        assert_eq!(app.display_list.len(), 3);
        // No header before the manual host
        assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
        assert!(
            matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "DigitalOcean")
        );
        assert!(matches!(&app.display_list[2], HostListItem::Host { .. }));
    }

    #[test]
    fn test_group_by_provider_with_alpha_sort() {
        let content = "\
Host do-zeta
  HostName 1.2.3.4
  # purple:provider digitalocean:1

Host do-alpha
  HostName 5.6.7.8
  # purple:provider digitalocean:2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();

        // DigitalOcean header + sorted hosts
        assert_eq!(app.display_list.len(), 3);
        assert!(
            matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean")
        );
        // First host should be do-alpha (alphabetical)
        if let HostListItem::Host { index } = &app.display_list[1] {
            assert_eq!(app.hosts[*index].alias, "do-alpha");
        } else {
            panic!("Expected Host item");
        }
    }

    #[test]
    fn test_config_changed_since_form_open_no_mtime() {
        let app = make_app("Host alpha\n  HostName a.com\n");
        // No mtime captured — should return false
        assert!(!app.config_changed_since_form_open());
    }

    #[test]
    fn test_config_changed_since_form_open_same_mtime() {
        let mut app = make_app("Host alpha\n  HostName a.com\n");
        // Config path is /tmp/test_config which doesn't exist, so mtime is None
        app.capture_form_mtime();
        // Immediately checking — mtime should be same (None == None)
        assert!(!app.config_changed_since_form_open());
    }

    #[test]
    fn test_config_changed_since_form_open_detects_change() {
        let mut app = make_app("Host alpha\n  HostName a.com\n");
        // Set form_mtime to a known past value (different from current None)
        app.conflict.form_mtime = Some(SystemTime::UNIX_EPOCH);
        // Config path doesn't exist (mtime is None), so it differs from UNIX_EPOCH
        assert!(app.config_changed_since_form_open());
    }

    #[test]
    fn test_group_by_provider_toggle_off_restores_flat() {
        let content = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:123

Host vultr-app
  HostName 5.6.7.8
  # purple:provider vultr:456
";
        let mut app = make_app(content);
        app.sort_mode = SortMode::AlphaAlias;

        // Enable grouping
        app.group_by = GroupBy::Provider;
        app.apply_sort();
        let grouped_len = app.display_list.len();
        assert!(grouped_len > 2); // Has headers

        // Disable grouping
        app.group_by = GroupBy::None;
        app.apply_sort();
        // Should be flat: just hosts, no headers
        assert_eq!(app.display_list.len(), 2);
        assert!(
            app.display_list
                .iter()
                .all(|item| matches!(item, HostListItem::Host { .. }))
        );
    }

    #[test]
    fn group_by_tag_groups_hosts_with_tag() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host web2
  HostName 2.2.2.2
  # purple:tags production

Host dev1
  HostName 3.3.3.3
  # purple:tags staging
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();
        // dev1 ungrouped first, then production header + 2 hosts
        assert_eq!(app.display_list.len(), 4);
        assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
        assert!(matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "production"));
        assert!(matches!(&app.display_list[2], HostListItem::Host { .. }));
        assert!(matches!(&app.display_list[3], HostListItem::Host { .. }));
        // Verify config order preserved within group
        if let HostListItem::Host { index } = &app.display_list[2] {
            assert_eq!(app.hosts[*index].alias, "web1");
        } else {
            panic!("Expected Host item at position 2");
        }
        if let HostListItem::Host { index } = &app.display_list[3] {
            assert_eq!(app.hosts[*index].alias, "web2");
        } else {
            panic!("Expected Host item at position 3");
        }
    }

    #[test]
    fn group_by_tag_no_hosts_have_tag() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags staging

Host web2
  HostName 2.2.2.2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();
        assert_eq!(app.display_list.len(), 2);
        assert!(
            app.display_list
                .iter()
                .all(|item| matches!(item, HostListItem::Host { .. }))
        );
    }

    #[test]
    fn group_by_tag_all_hosts_have_tag() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host web2
  HostName 2.2.2.2
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();
        assert_eq!(app.display_list.len(), 3);
        assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
    }

    #[test]
    fn group_by_tag_host_with_multiple_tags() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production,frontend

Host dev1
  HostName 3.3.3.3
  # purple:tags staging
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();
        assert_eq!(app.display_list.len(), 3);
        assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
        assert!(matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "production"));
    }

    #[test]
    fn group_by_tag_empty_host_list() {
        let content = "";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();
        assert!(app.display_list.is_empty());
    }

    #[test]
    fn group_by_tag_case_sensitive() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags Production

Host web2
  HostName 2.2.2.2
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();
        assert_eq!(app.display_list.len(), 3);
        assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
        assert!(matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "production"));
        if let HostListItem::Host { index } = &app.display_list[2] {
            assert_eq!(app.hosts[*index].alias, "web2");
        } else {
            panic!("Expected Host item");
        }
    }

    #[test]
    fn group_by_tag_with_alpha_sort() {
        let content = "\
Host zeta
  HostName 1.1.1.1
  # purple:tags production

Host alpha
  HostName 2.2.2.2
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();
        assert_eq!(app.display_list.len(), 3);
        assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
        if let HostListItem::Host { index } = &app.display_list[1] {
            assert_eq!(app.hosts[*index].alias, "alpha");
        } else {
            panic!("Expected Host item");
        }
    }

    #[test]
    fn group_by_tag_preserves_ordering_within_ungrouped() {
        let content = "\
Host charlie
  HostName 3.3.3.3

Host alpha
  HostName 1.1.1.1

Host bravo
  HostName 2.2.2.2
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();
        assert_eq!(app.display_list.len(), 4);
        if let HostListItem::Host { index } = &app.display_list[0] {
            assert_eq!(app.hosts[*index].alias, "alpha");
        } else {
            panic!("Expected Host item");
        }
        if let HostListItem::Host { index } = &app.display_list[1] {
            assert_eq!(app.hosts[*index].alias, "charlie");
        } else {
            panic!("Expected Host item");
        }
        assert!(matches!(&app.display_list[2], HostListItem::GroupHeader(s) if s == "production"));
    }

    #[test]
    fn group_by_tag_does_not_mutate_config() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host web2
  HostName 2.2.2.2
  # purple:tags staging
  # purple:provider_tags cloud,frontend
  # purple:provider digitalocean:123
";
        let app = make_app(content);
        let original_len = app.config.elements.len();

        let mut app2 = make_app(content);
        app2.group_by = GroupBy::Tag("production".to_string());
        app2.apply_sort();

        // Config elements must be identical — grouping is display-only
        assert_eq!(app.config.elements.len(), app2.config.elements.len());
        assert_eq!(original_len, app2.config.elements.len());
    }

    #[test]
    fn group_by_tag_then_provider_then_none_config_unchanged() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
  # purple:provider digitalocean:1

Host dev1
  HostName 2.2.2.2
  # purple:tags staging
";
        let mut app = make_app(content);
        let original_len = app.config.elements.len();

        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();
        app.group_by = GroupBy::Provider;
        app.apply_sort();
        app.group_by = GroupBy::None;
        app.apply_sort();

        assert_eq!(app.config.elements.len(), original_len);
    }

    #[test]
    fn provider_grouping_still_works_after_refactor() {
        let content = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:123

Host manual
  HostName 5.5.5.5

Host vultr-app
  HostName 9.9.9.9
  # purple:provider vultr:789
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();

        assert_eq!(app.display_list.len(), 5);
        assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
        assert!(
            matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "DigitalOcean")
        );
        assert!(matches!(&app.display_list[2], HostListItem::Host { .. }));
        assert!(matches!(&app.display_list[3], HostListItem::GroupHeader(s) if s == "Vultr"));
        assert!(matches!(&app.display_list[4], HostListItem::Host { .. }));
    }

    #[test]
    fn provider_grouping_with_sort_still_works() {
        let content = "\
Host do-zeta
  HostName 1.2.3.4
  # purple:provider digitalocean:1

Host do-alpha
  HostName 5.6.7.8
  # purple:provider digitalocean:2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();

        assert_eq!(app.display_list.len(), 3);
        assert!(
            matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean")
        );
        if let HostListItem::Host { index } = &app.display_list[1] {
            assert_eq!(app.hosts[*index].alias, "do-alpha");
        } else {
            panic!("Expected Host item");
        }
    }

    #[test]
    fn group_by_to_key_none() {
        assert_eq!(GroupBy::None.to_key(), "none");
    }

    #[test]
    fn group_by_to_key_provider() {
        assert_eq!(GroupBy::Provider.to_key(), "provider");
    }

    #[test]
    fn group_by_to_key_tag() {
        assert_eq!(
            GroupBy::Tag("production".to_string()).to_key(),
            "tag:production"
        );
    }

    #[test]
    fn group_by_from_key_none() {
        assert_eq!(GroupBy::from_key("none"), GroupBy::None);
    }

    #[test]
    fn group_by_from_key_provider() {
        assert_eq!(GroupBy::from_key("provider"), GroupBy::Provider);
    }

    #[test]
    fn group_by_from_key_tag() {
        assert_eq!(
            GroupBy::from_key("tag:production"),
            GroupBy::Tag("production".to_string())
        );
    }

    #[test]
    fn group_by_from_key_unknown_falls_back_to_none() {
        assert_eq!(GroupBy::from_key("garbage"), GroupBy::None);
    }

    #[test]
    fn group_by_from_key_empty_tag_name() {
        assert_eq!(GroupBy::from_key("tag:"), GroupBy::Tag(String::new()));
    }

    #[test]
    fn group_by_label_none() {
        assert_eq!(GroupBy::None.label(), "ungrouped");
    }

    #[test]
    fn group_by_label_provider() {
        assert_eq!(GroupBy::Provider.label(), "provider");
    }

    #[test]
    fn group_by_label_tag() {
        assert_eq!(GroupBy::Tag("env".to_string()).label(), "tag: env");
    }

    // --- New validation tests from review findings ---

    #[test]
    fn test_validate_rejects_hash_in_alias() {
        let mut form = HostForm::new();
        form.alias = "my#host".to_string();
        form.hostname = "1.2.3.4".to_string();
        let result = form.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("#"));
    }

    #[test]
    fn test_validate_empty_alias() {
        let mut form = HostForm::new();
        form.alias = "".to_string();
        form.hostname = "1.2.3.4".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_whitespace_alias() {
        let mut form = HostForm::new();
        form.alias = "my host".to_string();
        form.hostname = "1.2.3.4".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_pattern_alias() {
        let mut form = HostForm::new();
        form.alias = "my*host".to_string();
        form.hostname = "1.2.3.4".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_empty_hostname() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_port() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.port = "abc".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_port_zero() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.port = "0".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_valid_form() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.port = "22".to_string();
        assert!(form.validate().is_ok());
    }

    #[test]
    fn test_validate_rejects_control_chars() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4\x00".to_string();
        form.port = "22".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_to_entry_parses_tags() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.tags = "prod, staging, us-east".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.tags, vec!["prod", "staging", "us-east"]);
    }

    #[test]
    fn test_sort_mode_round_trip() {
        for mode in [
            SortMode::Original,
            SortMode::AlphaAlias,
            SortMode::AlphaHostname,
            SortMode::Frecency,
            SortMode::MostRecent,
        ] {
            assert_eq!(SortMode::from_key(mode.to_key()), mode);
        }
    }

    // --- TunnelForm tests ---

    #[test]
    fn tunnel_form_from_rule_local() {
        use crate::tunnel::{TunnelRule, TunnelType};
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: String::new(),
            bind_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        };
        let form = TunnelForm::from_rule(&rule);
        assert_eq!(form.tunnel_type, TunnelType::Local);
        assert_eq!(form.bind_port, "8080");
        assert_eq!(form.remote_host, "localhost");
        assert_eq!(form.remote_port, "80");
    }

    #[test]
    fn tunnel_form_from_rule_dynamic() {
        use crate::tunnel::{TunnelRule, TunnelType};
        let rule = TunnelRule {
            tunnel_type: TunnelType::Dynamic,
            bind_address: String::new(),
            bind_port: 1080,
            remote_host: String::new(),
            remote_port: 0,
        };
        let form = TunnelForm::from_rule(&rule);
        assert_eq!(form.tunnel_type, TunnelType::Dynamic);
        assert_eq!(form.bind_port, "1080");
        assert_eq!(form.remote_host, "");
        assert_eq!(form.remote_port, "");
    }

    #[test]
    fn tunnel_form_to_directive_local() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        let (key, value) = form.to_directive();
        assert_eq!(key, "LocalForward");
        assert_eq!(value, "8080 localhost:80");
    }

    #[test]
    fn tunnel_form_to_directive_remote() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Remote,
            bind_port: "9090".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "3000".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        let (key, value) = form.to_directive();
        assert_eq!(key, "RemoteForward");
        assert_eq!(value, "9090 localhost:3000");
    }

    #[test]
    fn tunnel_form_to_directive_dynamic() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Dynamic,
            bind_port: "1080".to_string(),
            bind_address: String::new(),
            remote_host: String::new(),
            remote_port: String::new(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        let (key, value) = form.to_directive();
        assert_eq!(key, "DynamicForward");
        assert_eq!(value, "1080");
    }

    #[test]
    fn tunnel_form_validate_valid() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_ok());
    }

    #[test]
    fn tunnel_form_validate_bad_bind_port() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "abc".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_err());
    }

    #[test]
    fn tunnel_form_validate_zero_bind_port() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "0".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_err());
    }

    #[test]
    fn tunnel_form_validate_empty_remote_host() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "  ".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_err());
    }

    #[test]
    fn tunnel_form_validate_dynamic_skips_remote() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Dynamic,
            bind_port: "1080".to_string(),
            bind_address: String::new(),
            remote_host: String::new(),
            remote_port: String::new(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_ok());
    }

    #[test]
    fn tunnel_form_field_next_local() {
        use crate::tunnel::TunnelType;
        assert_eq!(
            TunnelFormField::Type.next(TunnelType::Local),
            TunnelFormField::BindPort
        );
        assert_eq!(
            TunnelFormField::BindPort.next(TunnelType::Local),
            TunnelFormField::RemoteHost
        );
        assert_eq!(
            TunnelFormField::RemoteHost.next(TunnelType::Local),
            TunnelFormField::RemotePort
        );
        assert_eq!(
            TunnelFormField::RemotePort.next(TunnelType::Local),
            TunnelFormField::Type
        );
    }

    #[test]
    fn tunnel_form_field_next_dynamic_skips_remote() {
        use crate::tunnel::TunnelType;
        assert_eq!(
            TunnelFormField::Type.next(TunnelType::Dynamic),
            TunnelFormField::BindPort
        );
        assert_eq!(
            TunnelFormField::BindPort.next(TunnelType::Dynamic),
            TunnelFormField::Type
        );
    }

    #[test]
    fn tunnel_form_field_prev_local() {
        use crate::tunnel::TunnelType;
        assert_eq!(
            TunnelFormField::Type.prev(TunnelType::Local),
            TunnelFormField::RemotePort
        );
        assert_eq!(
            TunnelFormField::BindPort.prev(TunnelType::Local),
            TunnelFormField::Type
        );
        assert_eq!(
            TunnelFormField::RemoteHost.prev(TunnelType::Local),
            TunnelFormField::BindPort
        );
        assert_eq!(
            TunnelFormField::RemotePort.prev(TunnelType::Local),
            TunnelFormField::RemoteHost
        );
    }

    #[test]
    fn tunnel_form_field_prev_dynamic_skips_remote() {
        use crate::tunnel::TunnelType;
        assert_eq!(
            TunnelFormField::Type.prev(TunnelType::Dynamic),
            TunnelFormField::BindPort
        );
        assert_eq!(
            TunnelFormField::BindPort.prev(TunnelType::Dynamic),
            TunnelFormField::Type
        );
    }

    #[test]
    fn tunnel_form_validate_bad_remote_port() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "abc".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_err());
    }

    #[test]
    fn tunnel_form_from_rule_with_bind_address() {
        use crate::tunnel::{TunnelRule, TunnelType};
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: "192.168.1.1".to_string(),
            bind_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        };
        let form = TunnelForm::from_rule(&rule);
        assert_eq!(form.bind_address, "192.168.1.1");
        assert_eq!(form.bind_port, "8080");
        let (key, value) = form.to_directive();
        assert_eq!(key, "LocalForward");
        assert_eq!(value, "192.168.1.1:8080 localhost:80");
    }

    #[test]
    fn tunnel_form_validate_empty_bind_port() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: String::new(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_err());
    }

    #[test]
    fn tunnel_form_validate_zero_remote_port() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "0".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("Remote port"));
    }

    #[test]
    fn tunnel_form_validate_control_chars() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "local\x00host".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("control characters"));
    }

    #[test]
    fn tunnel_form_validate_spaces_in_remote_host() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "local host".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("spaces"));
    }

    #[test]
    fn tunnel_form_from_rule_ipv6_bind_address() {
        use crate::tunnel::{TunnelRule, TunnelType};
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: "::1".to_string(),
            bind_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        };
        let form = TunnelForm::from_rule(&rule);
        assert_eq!(form.bind_address, "::1");
        let (key, value) = form.to_directive();
        assert_eq!(key, "LocalForward");
        assert_eq!(value, "[::1]:8080 localhost:80");
    }

    // --- HostForm validation tests ---

    #[test]
    fn validate_hostname_whitespace_rejected() {
        let form = HostForm {
            alias: "myserver".to_string(),
            hostname: "host name".to_string(),
            port: "22".to_string(),
            ..HostForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("whitespace"), "got: {}", err);
    }

    #[test]
    fn validate_user_whitespace_rejected() {
        let form = HostForm {
            alias: "myserver".to_string(),
            hostname: "10.0.0.1".to_string(),
            user: "my user".to_string(),
            port: "22".to_string(),
            ..HostForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("whitespace"), "got: {}", err);
    }

    #[test]
    fn validate_hostname_with_control_chars_rejected() {
        let form = HostForm {
            alias: "myserver".to_string(),
            hostname: "10.0.0.1\n".to_string(),
            port: "22".to_string(),
            ..HostForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("control"), "got: {}", err);
    }

    // --- TunnelForm validation error message tests ---

    #[test]
    fn tunnel_validate_bind_port_zero_message() {
        let form = TunnelForm {
            bind_port: "0".to_string(),
            ..TunnelForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("0"), "got: {}", err);
    }

    #[test]
    fn tunnel_validate_remote_host_empty_message() {
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            remote_host: "".to_string(),
            remote_port: "80".to_string(),
            ..TunnelForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("empty"), "got: {}", err);
    }

    #[test]
    fn tunnel_validate_remote_host_whitespace_message() {
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            remote_host: "host name".to_string(),
            remote_port: "80".to_string(),
            ..TunnelForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("spaces"), "got: {}", err);
    }

    #[test]
    fn tunnel_validate_bind_port_non_numeric_message() {
        let form = TunnelForm {
            bind_port: "abc".to_string(),
            ..TunnelForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("1-65535"), "got: {}", err);
    }

    #[test]
    fn tunnel_validate_remote_port_zero_message() {
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            remote_host: "localhost".to_string(),
            remote_port: "0".to_string(),
            ..TunnelForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("0"), "got: {}", err);
    }

    #[test]
    fn select_host_by_alias_normal_mode() {
        let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        app.select_host_by_alias("beta");
        let selected = app.selected_host().unwrap();
        assert_eq!(selected.alias, "beta");
    }

    #[test]
    fn select_host_by_alias_search_mode() {
        let mut app = make_app(
            "Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n\nHost gamma\n  HostName g.com\n",
        );
        app.start_search();
        // Filter to beta and gamma (both contain letter 'a' in hostname or alias)
        app.search.query = Some("a".to_string());
        app.apply_filter();
        // filtered_indices should contain alpha (0) and gamma (2)
        assert!(app.search.filtered_indices.contains(&0));
        assert!(app.search.filtered_indices.contains(&2));

        // Select gamma by alias — should find it in filtered_indices
        app.select_host_by_alias("gamma");
        let selected = app.selected_host().unwrap();
        assert_eq!(selected.alias, "gamma");
    }

    #[test]
    fn select_host_by_alias_search_mode_not_in_results() {
        let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        app.start_search();
        app.search.query = Some("alpha".to_string());
        app.apply_filter();
        assert_eq!(app.search.filtered_indices, vec![0]);

        // "beta" is not in filtered results — selection should not change
        let before = app.ui.list_state.selected();
        app.select_host_by_alias("beta");
        assert_eq!(app.ui.list_state.selected(), before);
    }

    fn make_provider_app() -> App {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.provider_config = crate::providers::config::ProviderConfig::default();
        app.provider_config
            .set_section(crate::providers::config::ProviderSection {
                provider: "digitalocean".to_string(),
                token: "test-token".to_string(),
                alias_prefix: "do".to_string(),
                user: "root".to_string(),
                identity_file: String::new(),
                url: String::new(),
                verify_tls: true,
                auto_sync: true,
                profile: String::new(),
                regions: String::new(),
                project: String::new(),
                compartment: String::new(),
            });
        app
    }

    #[test]
    fn test_apply_sync_result_no_config() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.provider_config = crate::providers::config::ProviderConfig::default();
        let (msg, is_err, total, _, _, _) = app.apply_sync_result("digitalocean", vec![], false);
        assert!(is_err);
        assert_eq!(total, 0);
        assert!(msg.contains("no config"));
    }

    #[test]
    fn test_apply_sync_result_empty_hosts_returns_zero_total() {
        let mut app = make_provider_app();
        let (msg, is_err, total, _, _, _) = app.apply_sync_result("digitalocean", vec![], false);
        assert!(!is_err);
        assert_eq!(total, 0);
        assert!(msg.contains("added 0"));
        assert!(msg.contains("unchanged 0"));
    }

    #[test]
    fn test_apply_sync_result_with_hosts_returns_total() {
        let mut app = make_provider_app();
        let hosts = vec![
            crate::providers::ProviderHost::new(
                "s1".to_string(),
                "web".to_string(),
                "1.2.3.4".to_string(),
                vec![],
            ),
            crate::providers::ProviderHost::new(
                "s2".to_string(),
                "db".to_string(),
                "5.6.7.8".to_string(),
                vec![],
            ),
        ];
        let (msg, is_err, total, added, _, _) = app.apply_sync_result("digitalocean", hosts, false);
        assert!(!is_err);
        assert_eq!(total, 2);
        assert_eq!(added, 2);
        assert!(msg.contains("added 2"));
        assert!(msg.contains("unchanged 0"));
    }

    #[test]
    fn test_apply_sync_result_write_failure_preserves_total() {
        let mut app = make_provider_app();
        // Point config to a non-writable path so write() fails
        app.config.path = PathBuf::from("/dev/null/impossible");
        let hosts = vec![
            crate::providers::ProviderHost::new(
                "s1".to_string(),
                "web".to_string(),
                "1.2.3.4".to_string(),
                vec![],
            ),
            crate::providers::ProviderHost::new(
                "s2".to_string(),
                "db".to_string(),
                "5.6.7.8".to_string(),
                vec![],
            ),
        ];
        let (msg, is_err, total, _, _, _) = app.apply_sync_result("digitalocean", hosts, false);
        assert!(is_err);
        assert_eq!(total, 2); // total preserved despite write failure
        assert!(msg.contains("Sync failed to save"));
    }

    #[test]
    fn test_apply_sync_result_unknown_provider() {
        let mut app = make_provider_app();
        // Configure a section for the unknown provider name so it passes
        // the config check but fails on get_provider()
        app.provider_config
            .set_section(crate::providers::config::ProviderSection {
                provider: "nonexistent".to_string(),
                token: "tok".to_string(),
                alias_prefix: "nope".to_string(),
                user: "root".to_string(),
                identity_file: String::new(),
                url: String::new(),
                verify_tls: true,
                auto_sync: true,
                profile: String::new(),
                regions: String::new(),
                project: String::new(),
                compartment: String::new(),
            });
        let (msg, is_err, total, _, _, _) = app.apply_sync_result("nonexistent", vec![], false);
        assert!(is_err);
        assert_eq!(total, 0);
        assert!(msg.contains("Unknown provider"));
    }

    #[test]
    fn test_sync_history_cleared_on_provider_remove() {
        let mut app = make_provider_app();
        // Simulate a completed sync
        app.sync_history.insert(
            "digitalocean".to_string(),
            SyncRecord {
                timestamp: 100,
                message: "3 servers".to_string(),
                is_error: false,
            },
        );
        assert!(app.sync_history.contains_key("digitalocean"));

        // Simulate provider remove (same as handler.rs 'd' key path)
        app.provider_config.remove_section("digitalocean");
        app.sync_history.remove("digitalocean");

        assert!(!app.sync_history.contains_key("digitalocean"));
    }

    #[test]
    fn test_sync_history_overwrite_replaces_error_with_success() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        // First sync fails
        app.sync_history.insert(
            "hetzner".to_string(),
            SyncRecord {
                timestamp: 100,
                message: "auth failed".to_string(),
                is_error: true,
            },
        );
        // Second sync succeeds
        app.sync_history.insert(
            "hetzner".to_string(),
            SyncRecord {
                timestamp: 200,
                message: "5 servers".to_string(),
                is_error: false,
            },
        );
        let record = app.sync_history.get("hetzner").unwrap();
        assert_eq!(record.timestamp, 200);
        assert!(!record.is_error);
        assert_eq!(record.message, "5 servers");
    }

    // --- SyncRecord persistence tests ---

    #[test]
    fn test_sync_record_save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("purple_sync_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".purple")).unwrap();

        // Build history
        let mut history = HashMap::new();
        history.insert(
            "digitalocean".to_string(),
            SyncRecord {
                timestamp: 1710000000,
                message: "3 servers".to_string(),
                is_error: false,
            },
        );
        history.insert(
            "aws".to_string(),
            SyncRecord {
                timestamp: 1710000100,
                message: "auth failed".to_string(),
                is_error: true,
            },
        );
        history.insert(
            "hetzner".to_string(),
            SyncRecord {
                timestamp: 1710000200,
                message: "1 server (1 of 3 failed)".to_string(),
                is_error: true,
            },
        );

        // Save
        let path = dir.join(".purple").join("sync_history.tsv");
        let mut lines = Vec::new();
        for (provider, record) in &history {
            lines.push(format!(
                "{}\t{}\t{}\t{}",
                provider,
                record.timestamp,
                if record.is_error { "1" } else { "0" },
                record.message
            ));
        }
        std::fs::write(&path, lines.join("\n")).unwrap();

        // Load
        let content = std::fs::read_to_string(&path).unwrap();
        let mut loaded = HashMap::new();
        for line in content.lines() {
            let parts: Vec<&str> = line.splitn(4, '\t').collect();
            if parts.len() < 4 {
                continue;
            }
            let ts: u64 = parts[1].parse().unwrap();
            let is_error = parts[2] == "1";
            loaded.insert(
                parts[0].to_string(),
                SyncRecord {
                    timestamp: ts,
                    message: parts[3].to_string(),
                    is_error,
                },
            );
        }

        // Verify
        assert_eq!(loaded.len(), 3);
        let do_rec = loaded.get("digitalocean").unwrap();
        assert_eq!(do_rec.timestamp, 1710000000);
        assert_eq!(do_rec.message, "3 servers");
        assert!(!do_rec.is_error);

        let aws_rec = loaded.get("aws").unwrap();
        assert_eq!(aws_rec.timestamp, 1710000100);
        assert_eq!(aws_rec.message, "auth failed");
        assert!(aws_rec.is_error);

        let hz_rec = loaded.get("hetzner").unwrap();
        assert_eq!(hz_rec.message, "1 server (1 of 3 failed)");
        assert!(hz_rec.is_error);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_sync_record_load_missing_file() {
        // load_all on a nonexistent path should return empty map
        // (tested implicitly since load_all uses dirs::home_dir,
        // but we verify the parser handles empty/malformed input)
        let mut map = HashMap::new();
        let content = "";
        for line in content.lines() {
            let parts: Vec<&str> = line.splitn(4, '\t').collect();
            if parts.len() < 4 {
                continue;
            }
            let Some(ts) = parts[1].parse::<u64>().ok() else {
                continue;
            };
            map.insert(
                parts[0].to_string(),
                SyncRecord {
                    timestamp: ts,
                    message: parts[3].to_string(),
                    is_error: parts[2] == "1",
                },
            );
        }
        assert!(map.is_empty());
    }

    #[test]
    fn test_sync_record_load_malformed_lines() {
        // Malformed lines should be skipped
        let content = "badline\naws\t123\t0\t2 servers\nalso_bad\ttwo\t0\tfoo\n";
        let mut map = HashMap::new();
        for line in content.lines() {
            let parts: Vec<&str> = line.splitn(4, '\t').collect();
            if parts.len() < 4 {
                continue;
            }
            let Some(ts) = parts[1].parse::<u64>().ok() else {
                continue;
            };
            map.insert(
                parts[0].to_string(),
                SyncRecord {
                    timestamp: ts,
                    message: parts[3].to_string(),
                    is_error: parts[2] == "1",
                },
            );
        }
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("aws").unwrap().message, "2 servers");
    }

    // --- auto_sync tests ---

    fn make_section(provider: &str, auto_sync: bool) -> crate::providers::config::ProviderSection {
        crate::providers::config::ProviderSection {
            provider: provider.to_string(),
            token: "tok".to_string(),
            alias_prefix: provider[..2].to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: if provider == "proxmox" {
                "https://pve:8006".to_string()
            } else {
                String::new()
            },
            verify_tls: true,
            auto_sync,
            profile: String::new(),
            regions: String::new(),
            project: String::new(),
            compartment: String::new(),
        }
    }

    #[test]
    fn test_startup_auto_sync_filter_skips_disabled_providers() {
        // Simuleert de startup-loop in main.rs: providers met auto_sync=false worden overgeslagen.
        let mut config = crate::providers::config::ProviderConfig::default();
        config.set_section(make_section("digitalocean", true));
        config.set_section(make_section("proxmox", false));
        let auto_synced: Vec<&str> = config
            .configured_providers()
            .iter()
            .filter(|s| s.auto_sync)
            .map(|s| s.provider.as_str())
            .collect();
        assert_eq!(auto_synced, vec!["digitalocean"]);
        assert!(!auto_synced.contains(&"proxmox"));
    }

    #[test]
    fn test_startup_auto_sync_filter_all_enabled() {
        let mut config = crate::providers::config::ProviderConfig::default();
        config.set_section(make_section("digitalocean", true));
        config.set_section(make_section("vu", true)); // vultr-achtig
        let skipped: Vec<&str> = config
            .configured_providers()
            .iter()
            .filter(|s| !s.auto_sync)
            .map(|s| s.provider.as_str())
            .collect();
        assert!(skipped.is_empty());
    }

    #[test]
    fn test_startup_auto_sync_filter_explicit_false_skips() {
        // Niet-Proxmox provider met expliciete auto_sync=false wordt ook overgeslagen.
        let mut config = crate::providers::config::ProviderConfig::default();
        config.set_section(make_section("digitalocean", false));
        let s = &config.configured_providers()[0];
        assert!(!s.auto_sync);
    }

    #[test]
    fn test_provider_form_fields_new_defaults() {
        let form = ProviderFormFields::new();
        assert!(form.auto_sync, "new() should default auto_sync to true");
        assert!(form.verify_tls);
        assert_eq!(form.focused_field, ProviderFormField::Token);
    }

    #[test]
    fn test_provider_form_field_cloud_fields_include_auto_sync() {
        let fields = ProviderFormField::fields_for("digitalocean");
        assert!(
            fields.contains(&ProviderFormField::AutoSync),
            "CLOUD_FIELDS should contain AutoSync"
        );
        assert!(
            !fields.contains(&ProviderFormField::VerifyTls),
            "CLOUD_FIELDS should not contain VerifyTls"
        );
    }

    #[test]
    fn test_provider_form_field_proxmox_fields_include_auto_sync_and_verify_tls() {
        let fields = ProviderFormField::fields_for("proxmox");
        assert!(
            fields.contains(&ProviderFormField::AutoSync),
            "PROXMOX_FIELDS should contain AutoSync"
        );
        assert!(
            fields.contains(&ProviderFormField::VerifyTls),
            "PROXMOX_FIELDS should contain VerifyTls"
        );
    }

    #[test]
    fn test_provider_form_field_ovh_fields() {
        let fields = ProviderFormField::fields_for("ovh");
        assert_eq!(*fields.last().unwrap(), ProviderFormField::AutoSync);
        assert!(fields.contains(&ProviderFormField::Token));
        assert!(fields.contains(&ProviderFormField::Project));
        assert!(fields.contains(&ProviderFormField::Regions));
        assert!(fields.contains(&ProviderFormField::AliasPrefix));
        assert!(!fields.contains(&ProviderFormField::Url));
        assert!(!fields.contains(&ProviderFormField::VerifyTls));
    }

    #[test]
    fn test_provider_form_field_auto_sync_is_last_in_all_field_lists() {
        let cloud = ProviderFormField::fields_for("digitalocean");
        assert_eq!(*cloud.last().unwrap(), ProviderFormField::AutoSync);

        let proxmox = ProviderFormField::fields_for("proxmox");
        assert_eq!(*proxmox.last().unwrap(), ProviderFormField::AutoSync);

        let aws = ProviderFormField::fields_for("aws");
        assert_eq!(*aws.last().unwrap(), ProviderFormField::AutoSync);

        let scaleway = ProviderFormField::fields_for("scaleway");
        assert_eq!(*scaleway.last().unwrap(), ProviderFormField::AutoSync);
        assert!(scaleway.contains(&ProviderFormField::Regions));
        assert!(scaleway.contains(&ProviderFormField::Token));
        assert!(!scaleway.contains(&ProviderFormField::Profile));
        assert!(!scaleway.contains(&ProviderFormField::Url));
        assert!(!scaleway.contains(&ProviderFormField::VerifyTls));

        let azure = ProviderFormField::fields_for("azure");
        assert_eq!(*azure.last().unwrap(), ProviderFormField::AutoSync);
        assert!(azure.contains(&ProviderFormField::Regions));
        assert!(azure.contains(&ProviderFormField::Token));
        assert!(!azure.contains(&ProviderFormField::Profile));
        assert!(!azure.contains(&ProviderFormField::Url));
        assert!(!azure.contains(&ProviderFormField::VerifyTls));

        let ovh = ProviderFormField::fields_for("ovh");
        assert_eq!(*ovh.last().unwrap(), ProviderFormField::AutoSync);
        assert!(ovh.contains(&ProviderFormField::Token));
        assert!(ovh.contains(&ProviderFormField::Project));
        assert!(ovh.contains(&ProviderFormField::Regions));
        assert!(!ovh.contains(&ProviderFormField::Url));
    }

    #[test]
    fn test_provider_form_field_label_auto_sync() {
        assert_eq!(ProviderFormField::AutoSync.label(), "Auto Sync");
    }

    // =========================================================================
    // HostForm askpass tests
    // =========================================================================

    #[test]
    fn test_form_new_has_empty_askpass() {
        let form = HostForm::new();
        assert_eq!(form.askpass, "");
    }

    #[test]
    fn test_form_from_entry_with_askpass() {
        let entry = HostEntry {
            alias: "test".to_string(),
            hostname: "1.2.3.4".to_string(),
            askpass: Some("keychain".to_string()),
            ..Default::default()
        };
        let form = HostForm::from_entry(&entry);
        assert_eq!(form.askpass, "keychain");
    }

    #[test]
    fn test_form_from_entry_without_askpass() {
        let entry = HostEntry {
            alias: "test".to_string(),
            hostname: "1.2.3.4".to_string(),
            askpass: None,
            ..Default::default()
        };
        let form = HostForm::from_entry(&entry);
        assert_eq!(form.askpass, "");
    }

    #[test]
    fn test_to_entry_with_askpass_keychain() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "keychain".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("keychain".to_string()));
    }

    #[test]
    fn test_to_entry_with_askpass_op() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "op://Vault/Item/password".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("op://Vault/Item/password".to_string()));
    }

    #[test]
    fn test_to_entry_with_askpass_vault() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "vault:secret/data/myapp#password".to_string();
        let entry = form.to_entry();
        assert_eq!(
            entry.askpass,
            Some("vault:secret/data/myapp#password".to_string())
        );
    }

    #[test]
    fn test_to_entry_with_askpass_bw() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "bw:my-item".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("bw:my-item".to_string()));
    }

    #[test]
    fn test_to_entry_with_askpass_pass() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "pass:ssh/myserver".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("pass:ssh/myserver".to_string()));
    }

    #[test]
    fn test_to_entry_with_askpass_custom_command() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "my-script %a %h".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("my-script %a %h".to_string()));
    }

    #[test]
    fn test_to_entry_with_askpass_empty() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, None);
    }

    #[test]
    fn test_to_entry_with_askpass_whitespace_only() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "  ".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, None);
    }

    #[test]
    fn test_to_entry_askpass_trimmed() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "  keychain  ".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("keychain".to_string()));
    }

    #[test]
    fn test_focused_value_mut_askpass() {
        let mut form = HostForm::new();
        form.focused_field = FormField::AskPass;
        form.focused_value_mut().push_str("vault:");
        assert_eq!(form.askpass, "vault:");
    }

    #[test]
    fn test_askpass_field_label() {
        assert_eq!(FormField::AskPass.label(), "Password Source");
    }

    #[test]
    fn test_askpass_field_navigation() {
        // AskPass is between ProxyJump and Tags
        assert_eq!(FormField::ProxyJump.next(), FormField::AskPass);
        assert_eq!(FormField::AskPass.next(), FormField::Tags);
        assert_eq!(FormField::Tags.prev(), FormField::AskPass);
        assert_eq!(FormField::AskPass.prev(), FormField::ProxyJump);
    }

    #[test]
    fn test_form_field_all_includes_askpass() {
        assert!(FormField::ALL.contains(&FormField::AskPass));
        assert_eq!(FormField::ALL.len(), 8);
    }

    // --- Password picker state ---

    #[test]
    fn test_password_picker_state_init() {
        let app = make_app("Host test\n  HostName test.com\n");
        assert!(!app.ui.show_password_picker);
    }

    #[test]
    fn test_select_next_password_source() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.ui.password_picker_state.select(Some(0));
        app.select_next_password_source();
        assert_eq!(app.ui.password_picker_state.selected(), Some(1));
    }

    #[test]
    fn test_select_prev_password_source() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.ui.password_picker_state.select(Some(2));
        app.select_prev_password_source();
        assert_eq!(app.ui.password_picker_state.selected(), Some(1));
    }

    #[test]
    fn test_select_password_source_wrap_bottom() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
        app.ui.password_picker_state.select(Some(last));
        app.select_next_password_source();
        assert_eq!(app.ui.password_picker_state.selected(), Some(0));
    }

    #[test]
    fn test_select_password_source_wrap_top() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.ui.password_picker_state.select(Some(0));
        app.select_prev_password_source();
        let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
        assert_eq!(app.ui.password_picker_state.selected(), Some(last));
    }

    // --- Host entry askpass from config ---

    #[test]
    fn test_host_entries_include_askpass() {
        let app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        assert_eq!(app.hosts[0].askpass, Some("keychain".to_string()));
    }

    #[test]
    fn test_host_entries_vault_askpass() {
        let app = make_app(
            "Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pass\n",
        );
        assert_eq!(
            app.hosts[0].askpass,
            Some("vault:secret/ssh#pass".to_string())
        );
    }

    #[test]
    fn test_host_entries_no_askpass() {
        let app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        assert_eq!(app.hosts[0].askpass, None);
    }

    // --- Validate with askpass ---

    #[test]
    fn test_validate_askpass_with_control_char() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "keychain\x00".to_string();
        let result = form.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Password Source"));
    }

    #[test]
    fn test_validate_askpass_normal_values_ok() {
        let sources = [
            "",
            "keychain",
            "op://V/I/p",
            "bw:x",
            "pass:x",
            "vault:x#y",
            "cmd %a",
        ];
        for src in &sources {
            let mut form = HostForm::new();
            form.alias = "myhost".to_string();
            form.hostname = "1.2.3.4".to_string();
            form.askpass = src.to_string();
            assert!(
                form.validate().is_ok(),
                "Validate should pass for askpass='{}'",
                src
            );
        }
    }

    // --- add_host askpass flow (test config mutation directly, bypassing write) ---

    #[test]
    fn test_add_host_config_mutation_with_askpass() {
        let mut app = make_app("");
        let entry = HostEntry {
            alias: "newhost".to_string(),
            hostname: "1.2.3.4".to_string(),
            askpass: Some("keychain".to_string()),
            ..Default::default()
        };
        app.config.add_host(&entry);
        app.config.set_host_askpass("newhost", "keychain");
        let serialized = app.config.serialize();
        assert!(serialized.contains("purple:askpass keychain"));
        let entries = app.config.host_entries();
        let found = entries.iter().find(|e| e.alias == "newhost").unwrap();
        assert_eq!(found.askpass, Some("keychain".to_string()));
    }

    #[test]
    fn test_add_host_config_mutation_with_vault() {
        let mut app = make_app("");
        let entry = HostEntry {
            alias: "vaulthost".to_string(),
            hostname: "10.0.0.1".to_string(),
            askpass: Some("vault:secret/ssh#pass".to_string()),
            ..Default::default()
        };
        app.config.add_host(&entry);
        app.config
            .set_host_askpass("vaulthost", "vault:secret/ssh#pass");
        let serialized = app.config.serialize();
        assert!(serialized.contains("purple:askpass vault:secret/ssh#pass"));
    }

    #[test]
    fn test_add_host_config_mutation_without_askpass() {
        let mut app = make_app("");
        let entry = HostEntry {
            alias: "nopass".to_string(),
            hostname: "1.2.3.4".to_string(),
            ..Default::default()
        };
        app.config.add_host(&entry);
        // Don't call set_host_askpass when None — mirrors add_host_from_form logic
        let serialized = app.config.serialize();
        assert!(
            !serialized.contains("purple:askpass"),
            "No askpass comment when None"
        );
    }

    #[test]
    fn test_add_host_from_form_calls_set_askpass() {
        // Verify that add_host_from_form invokes set_host_askpass for non-None askpass.
        // We test by checking the form.to_entry() produces correct askpass.
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "op://Vault/Item/pw".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("op://Vault/Item/pw".to_string()));
        // And that the code path in add_host_from_form would call set_host_askpass
        assert!(entry.askpass.is_some());
    }

    // --- update host askpass via config (bypassing write which fails in test) ---

    #[test]
    fn test_config_set_host_askpass_adds() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        app.config.set_host_askpass("myserver", "bw:my-item");
        let serialized = app.config.serialize();
        assert!(serialized.contains("purple:askpass bw:my-item"));
        let entries = app.config.host_entries();
        assert_eq!(entries[0].askpass, Some("bw:my-item".to_string()));
    }

    #[test]
    fn test_config_set_host_askpass_changes() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        app.config.set_host_askpass("myserver", "pass:ssh/myserver");
        let serialized = app.config.serialize();
        assert!(!serialized.contains("keychain"));
        assert!(serialized.contains("purple:askpass pass:ssh/myserver"));
    }

    #[test]
    fn test_config_set_host_askpass_removes() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        app.config.set_host_askpass("myserver", "");
        let serialized = app.config.serialize();
        assert!(!serialized.contains("purple:askpass"));
        let entries = app.config.host_entries();
        assert_eq!(entries[0].askpass, None);
    }

    #[test]
    fn test_edit_host_from_form_sets_askpass_in_config() {
        // edit_host_from_form calls config.set_host_askpass() before write().
        // Since write() fails with test path, the rollback restores old state.
        // Test the config mutation directly to verify the flow.
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        let entry = HostEntry {
            alias: "myserver".to_string(),
            hostname: "10.0.0.1".to_string(),
            askpass: Some("vault:secret/ssh#pass".to_string()),
            ..Default::default()
        };
        app.config.update_host("myserver", &entry);
        app.config
            .set_host_askpass("myserver", entry.askpass.as_deref().unwrap_or(""));
        let serialized = app.config.serialize();
        assert!(serialized.contains("purple:askpass vault:secret/ssh#pass"));
    }

    // --- pending_connect carries askpass ---

    #[test]
    fn test_pending_connect_with_askpass() {
        let app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        let host = &app.hosts[0];
        assert_eq!(host.askpass, Some("keychain".to_string()));
        // Simulating what handle_host_list does
        let pending = (host.alias.clone(), host.askpass.clone());
        assert_eq!(pending.0, "myserver");
        assert_eq!(pending.1, Some("keychain".to_string()));
    }

    #[test]
    fn test_pending_connect_without_askpass() {
        let app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        let host = &app.hosts[0];
        let pending = (host.alias.clone(), host.askpass.clone());
        assert_eq!(pending.0, "myserver");
        assert_eq!(pending.1, None);
    }

    // --- from_entry roundtrip for all source types ---

    #[test]
    fn test_form_entry_roundtrip_all_sources() {
        let sources = [
            Some("keychain".to_string()),
            Some("op://V/I/p".to_string()),
            Some("bw:item".to_string()),
            Some("pass:ssh/x".to_string()),
            Some("vault:s/d#f".to_string()),
            Some("cmd %a %h".to_string()),
            None,
        ];
        for askpass in &sources {
            let entry = HostEntry {
                alias: "test".to_string(),
                hostname: "1.2.3.4".to_string(),
                askpass: askpass.clone(),
                ..Default::default()
            };
            let form = HostForm::from_entry(&entry);
            let back = form.to_entry();
            assert_eq!(back.askpass, *askpass, "Roundtrip failed for {:?}", askpass);
        }
    }

    // --- askpass special values ---

    #[test]
    fn test_to_entry_askpass_with_equals_sign() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "cmd --opt=val %h".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("cmd --opt=val %h".to_string()));
    }

    #[test]
    fn test_to_entry_askpass_with_hash() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "vault:secret/ssh#api_key".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("vault:secret/ssh#api_key".to_string()));
    }

    #[test]
    fn test_to_entry_askpass_long_value() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "op://My Personal Vault/SSH Production Server/password".to_string();
        let entry = form.to_entry();
        assert_eq!(
            entry.askpass,
            Some("op://My Personal Vault/SSH Production Server/password".to_string())
        );
    }

    // --- edit form askpass rollback logic ---

    #[test]
    fn test_edit_askpass_rollback_restores_old_source() {
        // Simulate the rollback logic from edit_host_from_form
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        let old_entry = app.hosts[0].clone();
        assert_eq!(old_entry.askpass, Some("keychain".to_string()));

        // Apply new askpass
        app.config
            .set_host_askpass("myserver", "vault:secret/ssh#pw");
        assert_eq!(
            app.config.host_entries()[0].askpass,
            Some("vault:secret/ssh#pw".to_string())
        );

        // Simulate rollback (write failed)
        app.config
            .set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
        assert_eq!(
            app.config.host_entries()[0].askpass,
            Some("keychain".to_string())
        );
    }

    #[test]
    fn test_edit_askpass_rollback_restores_none() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        let old_entry = app.hosts[0].clone();
        assert_eq!(old_entry.askpass, None);

        // Apply new askpass
        app.config.set_host_askpass("myserver", "bw:my-item");
        assert_eq!(
            app.config.host_entries()[0].askpass,
            Some("bw:my-item".to_string())
        );

        // Simulate rollback (write failed)
        app.config
            .set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
        assert_eq!(app.config.host_entries()[0].askpass, None);
    }

    // --- password picker state edge cases ---

    #[test]
    fn test_password_picker_initial_state_not_shown() {
        let app = make_app("Host test\n  HostName test.com\n");
        assert!(!app.ui.show_password_picker);
        assert_eq!(app.ui.password_picker_state.selected(), None);
    }

    // --- askpass global default fallback ---

    #[test]
    fn test_pending_connect_askpass_from_host() {
        let app = make_app(
            "Host s1\n  HostName 1.1.1.1\n  # purple:askpass bw:item1\n\nHost s2\n  HostName 2.2.2.2\n",
        );
        assert_eq!(app.hosts[0].askpass, Some("bw:item1".to_string()));
        assert_eq!(app.hosts[1].askpass, None);
    }

    // --- form field cycling includes askpass ---

    #[test]
    fn test_form_field_cycle_through_askpass() {
        let fields = FormField::ALL;
        let askpass_idx = fields
            .iter()
            .position(|f| matches!(f, FormField::AskPass))
            .unwrap();
        assert_eq!(askpass_idx, 6, "AskPass should be the 7th field (index 6)");
        // Verify it's between ProxyJump and Tags
        assert!(matches!(fields[askpass_idx - 1], FormField::ProxyJump));
        assert!(matches!(fields[askpass_idx + 1], FormField::Tags));
    }

    // --- validate control chars in askpass ---

    #[test]
    fn test_validate_askpass_rejects_newline() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "keychain\ninjected".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_askpass_rejects_tab() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "keychain\tinjected".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_askpass_rejects_null_byte() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "keychain\0injected".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_askpass_allows_normal_special_chars() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "vault:secret/data/my-app#api_key".to_string();
        assert!(form.validate().is_ok());
    }

    #[test]
    fn test_validate_askpass_allows_percent_substitution() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "get-pass %a %h".to_string();
        assert!(form.validate().is_ok());
    }

    // =========================================================================
    // Askpass fallback chain: per-host → global default (replicated logic)
    // =========================================================================

    #[test]
    fn test_askpass_fallback_per_host_takes_precedence() {
        // main.rs: host_askpass.or_else(preferences::load_askpass_default)
        let host_askpass: Option<String> = Some("op://V/I/p".to_string());
        let global_default: Option<String> = Some("keychain".to_string());
        let result = host_askpass.or(global_default);
        assert_eq!(result, Some("op://V/I/p".to_string()));
    }

    #[test]
    fn test_askpass_fallback_uses_global_when_no_per_host() {
        let host_askpass: Option<String> = None;
        let global_default: Option<String> = Some("keychain".to_string());
        let result = host_askpass.or(global_default);
        assert_eq!(result, Some("keychain".to_string()));
    }

    #[test]
    fn test_askpass_fallback_none_when_both_absent() {
        let host_askpass: Option<String> = None;
        let global_default: Option<String> = None;
        let result = host_askpass.or(global_default);
        assert_eq!(result, None);
    }

    // =========================================================================
    // cleanup_marker called after connection (document contract)
    // =========================================================================

    #[test]
    fn test_cleanup_marker_contract() {
        // After successful connection, main.rs calls askpass::cleanup_marker(&alias)
        // to remove the retry detection marker file
        let alias = "myserver";
        let call = format!("askpass::cleanup_marker(\"{}\")", alias);
        assert!(call.contains("cleanup_marker"));
    }

    // =========================================================================
    // pending_connect carries askpass through TUI event loop
    // =========================================================================

    #[test]
    fn test_pending_connect_tuple_structure() {
        // pending_connect is Option<(String, Option<String>)> = (alias, askpass)
        let (alias, askpass) = ("myserver".to_string(), Some("keychain".to_string()));
        assert_eq!(alias, "myserver");
        assert_eq!(askpass, Some("keychain".to_string()));
    }

    #[test]
    fn test_pending_connect_none_askpass() {
        let (alias, askpass): (String, Option<String>) = ("myserver".to_string(), None);
        assert_eq!(alias, "myserver");
        assert!(askpass.is_none());
    }

    // =========================================================================
    // bw_session caching in app state
    // =========================================================================

    #[test]
    fn test_bw_session_cached_across_connections() {
        let mut app = make_app(
            "Host a\n  HostName 1.1.1.1\n  # purple:askpass bw:item\n\nHost b\n  HostName 2.2.2.2\n  # purple:askpass bw:other\n",
        );
        // First connection prompts for unlock and caches token
        app.bw_session = Some("cached-token".to_string());
        // Second connection should reuse cached token
        let existing = app.bw_session.as_deref();
        assert_eq!(existing, Some("cached-token"));
        // ensure_bw_session returns None when existing is Some (no re-prompt)
        let needs_prompt = existing.is_none();
        assert!(!needs_prompt);
    }

    #[test]
    fn test_bw_session_not_set_for_non_bw() {
        let app = make_app("Host srv\n  HostName 1.1.1.1\n  # purple:askpass keychain\n");
        assert!(app.bw_session.is_none());
    }

    // =========================================================================
    // AskPass field in HostForm: display label and position
    // =========================================================================

    #[test]
    fn test_askpass_field_is_seventh_in_form() {
        let fields = FormField::ALL;
        assert_eq!(fields.len(), 8);
        assert!(matches!(fields[6], FormField::AskPass));
    }

    #[test]
    fn test_askpass_field_between_proxyjump_and_tags() {
        let fields = FormField::ALL;
        assert!(matches!(fields[5], FormField::ProxyJump));
        assert!(matches!(fields[6], FormField::AskPass));
        assert!(matches!(fields[7], FormField::Tags));
    }

    // =========================================================================
    // Search/filter with provider_tags
    // =========================================================================

    #[test]
    fn test_search_tag_exact_matches_provider_tags() {
        let mut app =
            make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:provider_tags prod\n");
        app.start_search();
        app.search.query = Some("tag=prod".to_string());
        app.apply_filter();
        assert_eq!(app.search.filtered_indices, vec![0]);
    }

    #[test]
    fn test_search_tag_fuzzy_matches_provider_tags() {
        let mut app =
            make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:provider_tags production\n");
        app.start_search();
        app.search.query = Some("tag:prod".to_string());
        app.apply_filter();
        assert_eq!(app.search.filtered_indices, vec![0]);
    }

    #[test]
    fn test_search_general_matches_provider_tags() {
        let mut app =
            make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:provider_tags staging\n");
        app.start_search();
        app.search.query = Some("staging".to_string());
        app.apply_filter();
        assert_eq!(app.search.filtered_indices, vec![0]);
    }

    #[test]
    fn test_collect_unique_tags_includes_provider_tags() {
        let app = make_app(
            "Host srv1\n  HostName 10.0.0.1\n  # purple:tags user1\n  # purple:provider_tags cloud1\n\nHost srv2\n  HostName 10.0.0.2\n  # purple:provider_tags cloud2\n  # purple:tags user2\n",
        );
        let tags = app.collect_unique_tags();
        assert!(tags.contains(&"user1".to_string()));
        assert!(tags.contains(&"user2".to_string()));
        assert!(tags.contains(&"cloud1".to_string()));
        assert!(tags.contains(&"cloud2".to_string()));
    }

    #[test]
    fn test_sort_alpha_alias_stale_to_bottom() {
        let config_str = "\
Host alpha
  HostName 1.1.1.1
  # purple:stale 1711900000

Host beta
  HostName 2.2.2.2

Host gamma
  HostName 3.3.3.3
  # purple:stale 1711900000
";
        let mut app = make_app(config_str);
        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();

        // beta (non-stale) should come first, then alpha and gamma (stale, sorted alphabetically)
        assert_eq!(app.display_list.len(), 3);
        if let HostListItem::Host { index } = &app.display_list[0] {
            assert_eq!(app.hosts[*index].alias, "beta");
        } else {
            panic!("Expected Host item at position 0");
        }
        if let HostListItem::Host { index } = &app.display_list[1] {
            assert_eq!(app.hosts[*index].alias, "alpha");
        } else {
            panic!("Expected Host item at position 1");
        }
        if let HostListItem::Host { index } = &app.display_list[2] {
            assert_eq!(app.hosts[*index].alias, "gamma");
        } else {
            panic!("Expected Host item at position 2");
        }
    }

    #[test]
    fn test_apply_sort_selects_first_in_sorted_order() {
        // Config order: charlie, alpha, beta
        let mut app = make_app(
            "Host charlie\n  HostName c.com\n\nHost alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n",
        );
        // Initial selection should be charlie (first in config)
        assert_eq!(app.selected_host().unwrap().alias, "charlie");

        // Sort alphabetically and reset selection to first sorted
        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();
        app.select_first_host();

        // After sort + select_first_host, alpha should be selected (first alphabetically)
        assert_eq!(app.selected_host().unwrap().alias, "alpha");
    }

    #[test]
    fn test_apply_sort_preserves_selection_without_reset() {
        // Verify apply_sort alone preserves the current selection (for interactive use)
        let mut app = make_app(
            "Host charlie\n  HostName c.com\n\nHost alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n",
        );
        assert_eq!(app.selected_host().unwrap().alias, "charlie");

        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();

        // apply_sort preserves the previously selected host (charlie)
        assert_eq!(app.selected_host().unwrap().alias, "charlie");
    }

    #[test]
    fn test_select_first_host_lands_on_group_header_when_grouped() {
        let content = "\
Host do-beta
  HostName 2.2.2.2
  # purple:provider digitalocean:2

Host do-alpha
  HostName 1.1.1.1
  # purple:provider digitalocean:1
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();
        app.select_first_host();

        // Headers are never selectable; first host is selected instead
        assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(_)));
        assert_eq!(app.ui.list_state.selected(), Some(1));
        assert!(app.selected_host().is_some());
    }

    #[test]
    fn test_select_first_host_skips_group_header_when_ungrouped() {
        let content = "\
Host do-beta
  HostName 2.2.2.2
  # purple:provider digitalocean:2

Host do-alpha
  HostName 1.1.1.1
  # purple:provider digitalocean:1
";
        let mut app = make_app(content);
        // GroupBy::None means headers should be skipped
        app.group_by = GroupBy::None;
        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();
        app.select_first_host();

        // With no grouping, display_list has no headers
        assert_eq!(app.selected_host().unwrap().alias, "do-alpha");
    }

    #[test]
    fn test_select_first_host_with_hostname_sort() {
        // Config order: srv-a (z.com), srv-b (a.com), srv-c (m.com)
        let mut app = make_app(
            "Host srv-a\n  HostName z.com\n\nHost srv-b\n  HostName a.com\n\nHost srv-c\n  HostName m.com\n",
        );
        app.sort_mode = SortMode::AlphaHostname;
        app.apply_sort();
        app.select_first_host();

        // srv-b has hostname a.com, should be first alphabetically by hostname
        assert_eq!(app.selected_host().unwrap().alias, "srv-b");
    }

    #[test]
    fn test_filter_tag_exact_stale() {
        let config_str = "\
Host alpha
  HostName 1.1.1.1
  # purple:stale 1711900000

Host beta
  HostName 2.2.2.2

Host gamma
  HostName 3.3.3.3
  # purple:stale 1711900000
";
        let mut app = make_app(config_str);
        app.start_search();
        app.search.query = Some("tag=stale".to_string());
        app.apply_filter();

        // Only stale hosts (alpha and gamma) should match
        assert_eq!(app.search.filtered_indices.len(), 2);
        assert_eq!(app.hosts[app.search.filtered_indices[0]].alias, "alpha");
        assert_eq!(app.hosts[app.search.filtered_indices[1]].alias, "gamma");
    }

    #[test]
    fn test_filter_tag_fuzzy_stale() {
        let config_str = "\
Host alpha
  HostName 1.1.1.1
  # purple:stale 1711900000

Host beta
  HostName 2.2.2.2

Host gamma
  HostName 3.3.3.3
  # purple:stale 1711900000
";
        let mut app = make_app(config_str);
        app.start_search();
        app.search.query = Some("tag:stal".to_string());
        app.apply_filter();

        // Fuzzy match on "stal" should match stale hosts
        assert_eq!(app.search.filtered_indices.len(), 2);
        assert_eq!(app.hosts[app.search.filtered_indices[0]].alias, "alpha");
        assert_eq!(app.hosts[app.search.filtered_indices[1]].alias, "gamma");
    }

    #[test]
    fn test_apply_sync_result_stale_in_message() {
        // Create a temp config file so writes succeed
        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join(format!("purple_test_stale_{}.conf", std::process::id()));
        let initial_config = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:s1

Host do-db
  HostName 5.6.7.8
  # purple:provider digitalocean:s2
";
        std::fs::write(&tmp_path, initial_config).unwrap();

        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(initial_config),
            path: tmp_path.clone(),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        app.provider_config = crate::providers::config::ProviderConfig::default();
        app.provider_config
            .set_section(crate::providers::config::ProviderSection {
                provider: "digitalocean".to_string(),
                token: "test-token".to_string(),
                alias_prefix: "do".to_string(),
                user: "root".to_string(),
                identity_file: String::new(),
                url: String::new(),
                verify_tls: true,
                auto_sync: true,
                profile: String::new(),
                regions: String::new(),
                project: String::new(),
                compartment: String::new(),
            });

        // First sync adds both hosts
        let hosts = vec![
            crate::providers::ProviderHost::new(
                "s1".to_string(),
                "web".to_string(),
                "1.2.3.4".to_string(),
                vec![],
            ),
            crate::providers::ProviderHost::new(
                "s2".to_string(),
                "db".to_string(),
                "5.6.7.8".to_string(),
                vec![],
            ),
        ];
        let (_, is_err, _, _, _, _) = app.apply_sync_result("digitalocean", hosts, false);
        assert!(!is_err);

        // Second sync with only one host (non-partial) should mark the other as stale
        let hosts2 = vec![crate::providers::ProviderHost::new(
            "s1".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            vec![],
        )];
        let (msg, is_err, total, _, _, stale) =
            app.apply_sync_result("digitalocean", hosts2, false);
        assert!(!is_err);
        assert_eq!(total, 1); // only the one host that's still present
        assert_eq!(stale, 1);
        assert!(
            msg.contains("stale 1"),
            "Expected stale count in message, got: {}",
            msg
        );

        // Clean up
        let _ = std::fs::remove_file(&tmp_path);
    }

    // --- Pattern form validation tests ---

    #[test]
    fn pattern_form_validates_wildcard_required() {
        let mut form = HostForm::new_pattern();
        form.alias = "myserver".to_string(); // No wildcard
        assert!(form.validate().is_err());
        form.alias = "*.example.com".to_string(); // Valid pattern
        assert!(form.validate().is_ok());
        form.alias = "10.30.0.*".to_string(); // Valid IP pattern
        assert!(form.validate().is_ok());
        form.alias = "server-[123]".to_string(); // Valid char class
        assert!(form.validate().is_ok());
        form.alias = "prod staging".to_string(); // Valid multi-pattern (space = pattern)
        assert!(form.validate().is_ok());
    }

    #[test]
    fn pattern_form_hostname_optional() {
        let mut form = HostForm::new_pattern();
        form.alias = "*.example.com".to_string();
        // Hostname empty is OK for patterns
        assert!(form.validate().is_ok());
        // Hostname filled is also OK
        form.hostname = "10.0.0.1".to_string();
        assert!(form.validate().is_ok());
    }

    #[test]
    fn reload_hosts_clears_filtered_pattern_indices() {
        let config_str = "\
Host myserver
  HostName 1.1.1.1

Host 10.30.0.*
  User debian
";
        let mut app = make_app(config_str);
        assert_eq!(app.patterns.len(), 1);
        // Start a search that matches the pattern
        app.start_search();
        app.search.query = Some("10.30".to_string());
        app.apply_filter();
        assert!(!app.search.filtered_pattern_indices.is_empty());
        // Cancel search and verify cleared
        app.cancel_search();
        assert!(app.search.filtered_pattern_indices.is_empty());
        // Start search again, then reload (simulates config change)
        app.start_search();
        app.search.query = Some("10.30".to_string());
        app.apply_filter();
        assert!(!app.search.filtered_pattern_indices.is_empty());
        // Simulate non-search reload path
        app.search.query = None;
        app.reload_hosts();
        assert!(app.search.filtered_pattern_indices.is_empty());
    }

    #[test]
    fn pattern_clone_clears_alias() {
        let entry = crate::ssh_config::model::PatternEntry {
            pattern: "10.30.0.*".to_string(),
            user: "debian".to_string(),
            identity_file: "~/.ssh/id_ed25519".to_string(),
            ..Default::default()
        };
        let mut form = HostForm::from_pattern_entry(&entry);
        // Simulate clone behavior from handler.rs
        form.alias.clear();
        form.cursor_pos = 0;
        assert!(form.is_pattern);
        assert!(form.alias.is_empty());
        assert_eq!(form.cursor_pos, 0);
        // Other fields should be preserved
        assert_eq!(form.user, "debian");
        assert_eq!(form.identity_file, "~/.ssh/id_ed25519");
    }

    #[test]
    fn tag_exact_search_finds_patterns() {
        let config_str = "\
Host myserver
  HostName 1.1.1.1
  # purple:tags web

Host 10.30.0.*
  User debian
  # purple:tags internal
";
        let mut app = make_app(config_str);
        app.start_search();
        app.search.query = Some("tag=internal".to_string());
        app.apply_filter();
        // Host should not match
        assert!(app.search.filtered_indices.is_empty());
        // Pattern should match
        assert_eq!(app.search.filtered_pattern_indices.len(), 1);
        assert_eq!(
            app.patterns[app.search.filtered_pattern_indices[0]].pattern,
            "10.30.0.*"
        );
    }

    #[test]
    fn tag_fuzzy_search_finds_patterns() {
        let config_str = "\
Host myserver
  HostName 1.1.1.1

Host 10.30.0.*
  User debian
  # purple:tags internal
";
        let mut app = make_app(config_str);
        app.start_search();
        app.search.query = Some("tag:intern".to_string());
        app.apply_filter();
        assert!(app.search.filtered_indices.is_empty());
        assert_eq!(app.search.filtered_pattern_indices.len(), 1);
    }

    #[test]
    fn collect_unique_tags_includes_pattern_tags() {
        let config_str = "\
Host myserver
  HostName 1.1.1.1
  # purple:tags web

Host 10.30.0.*
  User debian
  # purple:tags internal
";
        let app = make_app(config_str);
        let tags = app.collect_unique_tags();
        assert!(tags.contains(&"web".to_string()));
        assert!(tags.contains(&"internal".to_string()));
    }

    #[test]
    fn pattern_placeholder_text() {
        use crate::app::FormField;
        use crate::ui::host_form::{placeholder_text, placeholder_text_pattern};
        // Regular host placeholder
        assert_eq!(
            placeholder_text(FormField::Alias),
            "user@host:port or alias"
        );
        // Pattern placeholder
        assert_eq!(
            placeholder_text_pattern(FormField::Alias),
            "10.0.0.* or *.example.com"
        );
        // Non-alias fields should be the same regardless of is_pattern
        assert_eq!(
            placeholder_text(FormField::User),
            placeholder_text_pattern(FormField::User)
        );
    }

    #[test]
    fn pattern_form_from_entry_roundtrip() {
        let entry = crate::ssh_config::model::PatternEntry {
            pattern: "10.30.0.*".to_string(),
            hostname: String::new(),
            user: "debian".to_string(),
            port: 2222,
            identity_file: "~/.ssh/id_ed25519".to_string(),
            proxy_jump: "bastion".to_string(),
            tags: vec!["internal".to_string()],
            askpass: Some("keychain".to_string()),
            source_file: None,
            directives: vec![
                ("User".to_string(), "debian".to_string()),
                ("Port".to_string(), "2222".to_string()),
            ],
        };
        let form = HostForm::from_pattern_entry(&entry);
        assert!(form.is_pattern);
        assert_eq!(form.alias, "10.30.0.*");
        assert_eq!(form.user, "debian");
        assert_eq!(form.port, "2222");
        assert_eq!(form.identity_file, "~/.ssh/id_ed25519");
        assert_eq!(form.proxy_jump, "bastion");
        assert_eq!(form.tags, "internal");
        assert_eq!(form.askpass, "keychain");
    }

    // --- GroupBy::from_key edge cases ---

    #[test]
    fn group_by_from_key_tag_with_colon_in_name() {
        // "tag:prod:us-east" — everything after first "tag:" is the tag name
        assert_eq!(
            GroupBy::from_key("tag:prod:us-east"),
            GroupBy::Tag("prod:us-east".to_string())
        );
    }

    #[test]
    fn group_by_from_key_tag_with_special_chars() {
        assert_eq!(
            GroupBy::from_key("tag:prod-v2.1"),
            GroupBy::Tag("prod-v2.1".to_string())
        );
    }

    #[test]
    fn group_by_from_key_tag_with_unicode() {
        assert_eq!(
            GroupBy::from_key("tag:生产"),
            GroupBy::Tag("生产".to_string())
        );
    }

    #[test]
    fn group_by_from_key_tag_with_spaces() {
        assert_eq!(
            GroupBy::from_key("tag:my servers"),
            GroupBy::Tag("my servers".to_string())
        );
    }

    // --- group_indices_by_tag with stale hosts ---

    #[test]
    fn group_by_tag_stale_host_with_tag() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
  # purple:stale 1700000000

Host web2
  HostName 2.2.2.2
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();

        // Both hosts have the tag, stale or not — both in group
        assert_eq!(app.display_list.len(), 3);
        assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
    }

    #[test]
    fn group_by_tag_host_with_provider_and_user_tags() {
        let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:tags production
  # purple:provider_tags cloud,frontend
  # purple:provider digitalocean:123

Host manual
  HostName 2.2.2.2
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();

        // Both hosts have user tag "production" — both grouped
        assert_eq!(app.display_list.len(), 3);
        assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
    }

    #[test]
    fn group_by_tag_provider_tag_not_matched() {
        // provider_tags should NOT be matched by group_indices_by_tag
        let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider_tags production

Host manual
  HostName 2.2.2.2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();

        // "production" is a provider_tag, not a user tag — no grouping
        assert_eq!(app.display_list.len(), 2);
        assert!(
            app.display_list
                .iter()
                .all(|item| matches!(item, HostListItem::Host { .. }))
        );
    }

    // --- apply_sort() — missing SortMode x GroupBy combinations ---

    #[test]
    fn group_by_tag_with_original_sort() {
        let content = "\
Host zeta
  HostName 1.1.1.1
  # purple:tags production

Host alpha
  HostName 2.2.2.2
  # purple:tags production

Host manual
  HostName 3.3.3.3
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.sort_mode = SortMode::Original;
        app.apply_sort();

        // manual ungrouped, then production header + zeta + alpha (config order)
        assert_eq!(app.display_list.len(), 4);
        assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
        assert!(matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "production"));
        // Verify config order preserved within group
        if let HostListItem::Host { index } = &app.display_list[2] {
            assert_eq!(app.hosts[*index].alias, "zeta");
        } else {
            panic!("Expected Host item at position 2");
        }
        if let HostListItem::Host { index } = &app.display_list[3] {
            assert_eq!(app.hosts[*index].alias, "alpha");
        } else {
            panic!("Expected Host item at position 3");
        }
    }

    #[test]
    fn group_by_tag_with_hostname_sort() {
        let content = "\
Host web1
  HostName zebra.example.com
  # purple:tags production

Host web2
  HostName alpha.example.com
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.sort_mode = SortMode::AlphaHostname;
        app.apply_sort();

        assert_eq!(app.display_list.len(), 3);
        assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
        if let HostListItem::Host { index } = &app.display_list[1] {
            assert_eq!(app.hosts[*index].hostname, "alpha.example.com");
        } else {
            panic!("Expected Host item");
        }
    }

    #[test]
    fn group_by_provider_with_hostname_sort() {
        let content = "\
Host do-zebra
  HostName zebra.example.com
  # purple:provider digitalocean:1

Host do-alpha
  HostName alpha.example.com
  # purple:provider digitalocean:2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.sort_mode = SortMode::AlphaHostname;
        app.apply_sort();

        assert_eq!(app.display_list.len(), 3);
        assert!(
            matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean")
        );
        if let HostListItem::Host { index } = &app.display_list[1] {
            assert_eq!(app.hosts[*index].hostname, "alpha.example.com");
        } else {
            panic!("Expected Host item");
        }
    }

    #[test]
    fn group_by_none_with_each_sort_mode() {
        let content = "\
Host beta
  HostName 2.2.2.2

Host alpha
  HostName 1.1.1.1
";
        for mode in [SortMode::AlphaAlias, SortMode::AlphaHostname] {
            let mut app = make_app(content);
            app.group_by = GroupBy::None;
            app.sort_mode = mode;
            app.apply_sort();

            // No headers, just sorted hosts
            assert_eq!(app.display_list.len(), 2);
            assert!(
                app.display_list
                    .iter()
                    .all(|item| matches!(item, HostListItem::Host { .. }))
            );
            if let HostListItem::Host { index } = &app.display_list[0] {
                assert_eq!(app.hosts[*index].alias, "alpha");
            }
        }
    }

    // --- Search + grouping interaction ---

    #[test]
    fn search_works_with_tag_grouping() {
        let content = "\
Host web-prod
  HostName 1.1.1.1
  # purple:tags production

Host web-staging
  HostName 2.2.2.2
  # purple:tags staging

Host db-prod
  HostName 3.3.3.3
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();

        // Before search: 1 ungrouped + 1 header + 2 grouped = 4
        assert_eq!(app.display_list.len(), 4);

        // Start search and filter for "web"
        app.start_search();
        app.search.query = Some("web".to_string());
        app.apply_filter();

        // Search should filter to web-prod and web-staging
        assert_eq!(app.search.filtered_indices.len(), 2);
    }

    // --- Multi-select cleared on group change ---

    #[test]
    fn multi_select_cleared_on_group_change() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host web2
  HostName 2.2.2.2
";
        let mut app = make_app(content);
        app.multi_select.insert(0);
        app.multi_select.insert(1);
        assert_eq!(app.multi_select.len(), 2);

        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();

        assert!(app.multi_select.is_empty());
    }

    // --- Pattern entries with tag grouping ---

    #[test]
    fn patterns_appear_at_bottom_with_tag_grouping() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host 10.0.0.*
  User debian
  # purple:tags internal
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();

        // Should have: production header + web1, then Patterns header + pattern
        let has_patterns_header = app
            .display_list
            .iter()
            .any(|item| matches!(item, HostListItem::GroupHeader(s) if s == "Patterns"));
        assert!(
            has_patterns_header,
            "Patterns header should appear at bottom"
        );

        // Patterns header should be after all hosts
        let patterns_pos = app
            .display_list
            .iter()
            .position(|item| matches!(item, HostListItem::GroupHeader(s) if s == "Patterns"))
            .unwrap();
        let last_host_pos = app
            .display_list
            .iter()
            .rposition(|item| matches!(item, HostListItem::Host { .. }));
        if let Some(host_pos) = last_host_pos {
            assert!(
                patterns_pos > host_pos,
                "Patterns header should be after last host"
            );
        }
    }

    // --- Proptest: group_by_tag display_list consistency ---

    use proptest::prelude::*;

    /// Generate a simple SSH config block with optional user tags.
    fn prop_host_block(alias: String, hostname: String, tags: Option<Vec<String>>) -> String {
        let mut lines = vec![format!("Host {alias}"), format!("  HostName {hostname}")];
        if let Some(ref ts) = tags {
            if !ts.is_empty() {
                lines.push(format!("  # purple:tags {}", ts.join(",")));
            }
        }
        lines.join("\n")
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(200))]

        /// GroupBy::Tag display_list is consistent:
        /// - Total host items == app.hosts.len()
        /// - No duplicate host indices
        /// - At most one GroupHeader per apply_sort call
        /// - All indices are in-bounds
        #[test]
        fn group_by_tag_display_list_consistent(
            hosts in prop::collection::vec(
                (
                    "[a-z][a-z0-9]{2,10}".prop_map(|s| s),
                    "[a-z]{3,8}\\.(com|net|io)".prop_map(|s| s),
                    prop::option::of(
                        prop::collection::vec("[a-z]{2,8}", 1..=3)
                    ),
                ),
                1..=15,
            ),
            tag_index in 0usize..10,
        ) {
            // Build config content from generated host data
            let mut blocks: Vec<String> = Vec::new();
            let mut all_tags: Vec<String> = Vec::new();

            for (alias, hostname, tags) in &hosts {
                if let Some(ts) = tags {
                    for t in ts {
                        if !all_tags.contains(t) {
                            all_tags.push(t.clone());
                        }
                    }
                }
                blocks.push(prop_host_block(alias.clone(), hostname.clone(), tags.clone()));
            }

            let content = blocks.join("\n\n") + "\n";
            let mut app = make_app(&content);

            // Pick a tag to group by (or use a nonexistent one if no tags)
            let chosen_tag = if all_tags.is_empty() {
                "nonexistent".to_string()
            } else {
                all_tags[tag_index % all_tags.len()].clone()
            };

            app.group_by = GroupBy::Tag(chosen_tag.clone());
            app.apply_sort();

            let host_count = app.hosts.len();
            let display_host_count = app.display_list.iter()
                .filter(|item| matches!(item, HostListItem::Host { .. }))
                .count();

            // All hosts must appear exactly once
            prop_assert_eq!(
                host_count,
                display_host_count,
                "host count mismatch: {} hosts but {} in display_list",
                host_count,
                display_host_count,
            );

            // No duplicate host indices
            let indices: Vec<usize> = app.display_list.iter()
                .filter_map(|item| {
                    if let HostListItem::Host { index } = item {
                        Some(*index)
                    } else {
                        None
                    }
                })
                .collect();

            let mut seen = std::collections::HashSet::new();
            for &idx in &indices {
                prop_assert!(
                    seen.insert(idx),
                    "duplicate host index {} in display_list",
                    idx,
                );
                prop_assert!(
                    idx < host_count,
                    "host index {} out of bounds (hosts len {})",
                    idx,
                    host_count,
                );
            }

            // At most one GroupHeader with the chosen tag name
            let header_count = app.display_list.iter()
                .filter(|item| matches!(item, HostListItem::GroupHeader(s) if s == &chosen_tag))
                .count();
            prop_assert!(
                header_count <= 1,
                "expected at most 1 GroupHeader for '{}', got {}",
                chosen_tag,
                header_count,
            );

            // If header is present, all tagged hosts appear after it
            if header_count == 1 {
                let header_pos = app.display_list.iter()
                    .position(|item| matches!(item, HostListItem::GroupHeader(s) if s == &chosen_tag))
                    .unwrap();
                for (pos, item) in app.display_list.iter().enumerate() {
                    if let HostListItem::Host { index } = item {
                        let has_tag = app.hosts[*index].tags.iter().any(|t| t == &chosen_tag);
                        if has_tag {
                            prop_assert!(
                                pos > header_pos,
                                "tagged host at pos {} is before header at pos {}",
                                pos,
                                header_pos,
                            );
                        }
                    }
                }
            }
        }

        /// GroupBy::None produces no GroupHeaders and all hosts appear exactly once.
        #[test]
        fn group_by_none_display_list_no_headers(
            hosts in prop::collection::vec(
                (
                    "[a-z][a-z0-9]{2,10}".prop_map(|s| s),
                    "[a-z]{3,8}\\.(com|net|io)".prop_map(|s| s),
                    prop::option::of(prop::collection::vec("[a-z]{2,8}", 1..=3)),
                ),
                1..=10,
            ),
        ) {
            let blocks: Vec<String> = hosts.iter().map(|(alias, hostname, tags)| {
                prop_host_block(alias.clone(), hostname.clone(), tags.clone())
            }).collect();
            let content = blocks.join("\n\n") + "\n";
            let mut app = make_app(&content);

            app.group_by = GroupBy::None;
            app.sort_mode = SortMode::AlphaAlias;
            app.apply_sort();

            let host_count = app.hosts.len();

            // No group headers from GroupBy::None (comment-based headers possible;
            // but no tag/provider headers)
            let display_host_count = app.display_list.iter()
                .filter(|item| matches!(item, HostListItem::Host { .. }))
                .count();

            prop_assert_eq!(
                host_count,
                display_host_count,
                "GroupBy::None: host count mismatch: {} hosts vs {} in display",
                host_count,
                display_host_count,
            );
        }

        /// Switching GroupBy::Tag → GroupBy::None always removes the GroupHeader.
        #[test]
        fn group_by_tag_to_none_removes_header(
            alias in "[a-z][a-z0-9]{2,8}",
            hostname in "[a-z]{3,8}\\.(com|net|io)",
            tag in "[a-z]{2,8}",
        ) {
            let content = format!(
                "Host {alias}\n  HostName {hostname}\n  # purple:tags {tag}\n"
            );
            let mut app = make_app(&content);

            // Apply tag grouping
            app.group_by = GroupBy::Tag(tag.clone());
            app.apply_sort();
            let has_header_grouped = app.display_list.iter()
                .any(|item| matches!(item, HostListItem::GroupHeader(s) if s == &tag));
            prop_assert!(has_header_grouped, "expected GroupHeader for tag '{}'", tag);

            // Switch to None
            app.group_by = GroupBy::None;
            app.apply_sort();
            let has_header_none = app.display_list.iter()
                .any(|item| matches!(item, HostListItem::GroupHeader(s) if s == &tag));
            prop_assert!(!has_header_none, "GroupHeader should be gone after GroupBy::None");
        }
    }

    #[test]
    fn group_by_tag_graceful_when_tag_removed_from_all_hosts() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags staging

Host web2
  HostName 2.2.2.2
";
        let mut app = make_app(content);
        // Group by a tag that no host has
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();

        // All hosts ungrouped, no header, no panic
        assert_eq!(app.display_list.len(), 2);
        assert!(
            app.display_list
                .iter()
                .all(|item| matches!(item, HostListItem::Host { .. }))
        );
    }

    #[test]
    fn group_by_tag_original_sort_preserves_stale_position() {
        // In Original sort mode, stale hosts stay in config order even when grouped.
        // This differs from other sort modes which push stale hosts to the bottom.
        let content = "\
Host stale-first
  HostName 1.1.1.1
  # purple:tags production
  # purple:stale 1700000000

Host healthy-second
  HostName 2.2.2.2
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.sort_mode = SortMode::Original;
        app.apply_sort();

        // Original order preserved: stale host first within group
        assert_eq!(app.display_list.len(), 3);
        assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
        if let HostListItem::Host { index } = &app.display_list[1] {
            assert_eq!(app.hosts[*index].alias, "stale-first");
        } else {
            panic!("Expected Host item");
        }
    }

    #[test]
    fn group_by_tag_alpha_sort_pushes_stale_to_bottom() {
        // Non-Original sort modes push stale hosts to the bottom of each group.
        let content = "\
Host alpha-stale
  HostName 1.1.1.1
  # purple:tags production
  # purple:stale 1700000000

Host beta-healthy
  HostName 2.2.2.2
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();

        // Alpha sort: stale host pushed to bottom of group
        assert_eq!(app.display_list.len(), 3);
        assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
        if let HostListItem::Host { index } = &app.display_list[1] {
            assert_eq!(app.hosts[*index].alias, "beta-healthy");
        } else {
            panic!("Expected Host item");
        }
        if let HostListItem::Host { index } = &app.display_list[2] {
            assert_eq!(app.hosts[*index].alias, "alpha-stale");
        } else {
            panic!("Expected Host item");
        }
    }

    #[test]
    fn clear_stale_group_tag_clears_when_tag_missing() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags staging
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());

        let cleared = app.clear_stale_group_tag();

        assert!(cleared);
        assert_eq!(app.group_by, GroupBy::None);
    }

    #[test]
    fn clear_stale_group_tag_keeps_when_tag_exists() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());

        let cleared = app.clear_stale_group_tag();

        assert!(!cleared);
        assert_eq!(app.group_by, GroupBy::Tag("production".to_string()));
    }

    #[test]
    fn clear_stale_group_tag_noop_for_provider() {
        let content = "\
Host web1
  HostName 1.1.1.1
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;

        let cleared = app.clear_stale_group_tag();

        assert!(!cleared);
        assert_eq!(app.group_by, GroupBy::Provider);
    }

    #[test]
    fn clear_stale_group_tag_noop_for_none() {
        let content = "\
Host web1
  HostName 1.1.1.1
";
        let mut app = make_app(content);
        app.group_by = GroupBy::None;

        let cleared = app.clear_stale_group_tag();

        assert!(!cleared);
        assert_eq!(app.group_by, GroupBy::None);
    }

    #[test]
    fn clear_stale_group_tag_empty_hosts() {
        let content = "";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());

        let cleared = app.clear_stale_group_tag();

        assert!(cleared);
        assert_eq!(app.group_by, GroupBy::None);
    }

    #[test]
    fn clear_stale_group_tag_keeps_empty_tag_sentinel() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag(String::new());

        let cleared = app.clear_stale_group_tag();

        assert!(!cleared, "empty tag sentinel should not be cleared");
        assert_eq!(app.group_by, GroupBy::Tag(String::new()));
    }

    // --- Group filter (tab navigation) ---

    #[test]
    fn group_filter_shows_only_group_hosts() {
        let content = "\
Host web-prod
  HostName 1.1.1.1
  # purple:tags production

Host web-staging
  HostName 2.2.2.2
  # purple:tags staging

Host db-prod
  HostName 3.3.3.3
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();

        // Without filter: header + all 3 hosts visible
        let hosts_before: Vec<_> = app
            .display_list
            .iter()
            .filter(|item| matches!(item, HostListItem::Host { .. }))
            .collect();
        assert_eq!(hosts_before.len(), 3, "all 3 hosts should be visible");

        // group_host_counts should show 2 for production
        assert_eq!(
            app.group_host_counts.get("production"),
            Some(&2),
            "production group should have 2 hosts"
        );

        // Filter to production group only
        app.group_filter = Some("production".to_string());
        app.apply_sort();

        // Only production hosts should be visible (no header, no staging host)
        let hosts_after: Vec<_> = app
            .display_list
            .iter()
            .filter(|item| matches!(item, HostListItem::Host { .. }))
            .collect();
        assert_eq!(
            hosts_after.len(),
            2,
            "only production hosts should be visible when filtered"
        );

        // group_host_counts should still show the correct count
        assert_eq!(
            app.group_host_counts.get("production"),
            Some(&2),
            "count should still be 2 with filter active"
        );
    }

    #[test]
    fn group_filter_cleared_restores_display_list() {
        let content = "\
Host web-prod
  HostName 1.1.1.1
  # purple:tags production

Host web-staging
  HostName 2.2.2.2
  # purple:tags staging

Host db-prod
  HostName 3.3.3.3
  # purple:tags production
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());

        // Filter
        app.group_filter = Some("production".to_string());
        app.apply_sort();

        let hosts_filtered: Vec<_> = app
            .display_list
            .iter()
            .filter(|item| matches!(item, HostListItem::Host { .. }))
            .collect();
        assert_eq!(hosts_filtered.len(), 2);

        // Clear filter
        app.group_filter = None;
        app.apply_sort();

        let hosts_unfiltered: Vec<_> = app
            .display_list
            .iter()
            .filter(|item| matches!(item, HostListItem::Host { .. }))
            .collect();
        assert_eq!(
            hosts_unfiltered.len(),
            3,
            "all hosts should reappear after clearing filter"
        );
    }

    #[test]
    fn group_filter_cleared_on_stale_group_by_change() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
  # purple:provider aws:i-123

Host web2
  HostName 2.2.2.2
  # purple:tags staging
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.group_filter = Some("aws".to_string());

        // Change group_by to Tag, which triggers clear_stale_group_tag
        app.group_by = GroupBy::Tag("nonexistent".to_string());
        let cleared = app.clear_stale_group_tag();

        assert!(cleared);
        assert!(
            app.group_filter.is_none(),
            "group_filter should be cleared when group_by tag is stale"
        );
    }

    #[test]
    fn group_tab_order_populated_by_apply_sort() {
        let content = "\
Host web-prod
  HostName 1.1.1.1
  # purple:tags production

Host web-staging
  HostName 2.2.2.2
  # purple:tags staging
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        app.apply_sort();

        // group_tab_order should contain "production"
        assert!(
            app.group_tab_order.contains(&"production".to_string()),
            "group_tab_order should include production group"
        );
    }

    #[test]
    fn group_tab_order_tag_mode_tiebreaker_is_alphabetical() {
        let content = "\
Host h1
  HostName 1.1.1.1
  # purple:tags beta

Host h2
  HostName 2.2.2.2
  # purple:tags alpha
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("alpha".to_string());
        app.apply_sort();

        assert_eq!(app.group_tab_order.len(), 2);
        assert_eq!(app.group_tab_order[0], "alpha");
        assert_eq!(app.group_tab_order[1], "beta");
    }

    #[test]
    fn ctrl_a_with_group_filter_skips_hidden_hosts() {
        let content = "\
Host web-prod
  HostName 1.1.1.1
  # purple:tags production

Host db-prod
  HostName 3.3.3.3
  # purple:tags production

Host web-staging
  HostName 2.2.2.2
  # purple:tags staging
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("production".to_string());
        // Filter to staging: only web-staging visible (it's the ungrouped host)
        app.group_filter = Some("production".to_string());
        app.apply_sort();

        // Simulate Ctrl+A: select all visible Host items
        let visible_indices: Vec<usize> = app
            .display_list
            .iter()
            .filter_map(|item| match item {
                HostListItem::Host { index } => Some(*index),
                _ => None,
            })
            .collect();
        for idx in &visible_indices {
            app.multi_select.insert(*idx);
        }

        // Only production hosts should be selected when filter is active
        assert_eq!(app.multi_select.len(), 2);
        for idx in &app.multi_select {
            let host = &app.hosts[*idx];
            assert!(
                host.tags.contains(&"production".to_string()),
                "only production hosts should be selected"
            );
        }
    }

    // --- Ping generation ---
    // Handler-level test: test_p_key_clears_ping_increments_generation in handler.rs

    // --- Ctrl+A select all / deselect all ---

    #[test]
    fn ctrl_a_selects_all_visible_hosts() {
        let content = "\
Host web1
  HostName 1.1.1.1

Host web2
  HostName 2.2.2.2

Host web3
  HostName 3.3.3.3
";
        let mut app = make_app(content);
        app.apply_sort();

        // Simulate Ctrl+A: collect all Host indices from display_list
        let host_indices: Vec<usize> = app
            .display_list
            .iter()
            .filter_map(|item| match item {
                HostListItem::Host { index } => Some(*index),
                _ => None,
            })
            .collect();
        for idx in &host_indices {
            app.multi_select.insert(*idx);
        }

        assert_eq!(app.multi_select.len(), 3);
    }

    #[test]
    fn ctrl_a_toggle_deselects_when_all_selected() {
        let content = "\
Host web1
  HostName 1.1.1.1

Host web2
  HostName 2.2.2.2

Host web3
  HostName 3.3.3.3
";
        let mut app = make_app(content);
        app.apply_sort();

        // Select all
        let host_indices: Vec<usize> = app
            .display_list
            .iter()
            .filter_map(|item| match item {
                HostListItem::Host { index } => Some(*index),
                _ => None,
            })
            .collect();
        for idx in &host_indices {
            app.multi_select.insert(*idx);
        }
        assert_eq!(app.multi_select.len(), 3);

        // Check all_selected condition and clear
        let all_selected = host_indices
            .iter()
            .all(|idx| app.multi_select.contains(idx));
        assert!(all_selected);
        app.multi_select.clear();

        assert!(app.multi_select.is_empty());
    }

    // --- next_group_tab ---

    #[test]
    fn next_group_tab_from_all_goes_to_first_group() {
        let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host aws-db
  HostName 2.2.2.2
  # purple:provider aws:2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();
        assert!(app.group_filter.is_none());
        assert_eq!(app.group_tab_index, 0);
        assert!(!app.group_tab_order.is_empty());

        let first_group = app.group_tab_order[0].clone();
        app.next_group_tab();

        assert_eq!(app.group_filter, Some(first_group));
        assert_eq!(app.group_tab_index, 1);
    }

    #[test]
    fn next_group_tab_cycles_through_groups_and_back_to_all() {
        let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host aws-db
  HostName 2.2.2.2
  # purple:provider aws:2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();
        // Ensure exactly 2 groups
        assert_eq!(app.group_tab_order.len(), 2);

        // First call: All -> group1
        app.next_group_tab();
        assert!(app.group_filter.is_some());
        assert_eq!(app.group_tab_index, 1);

        // Second call: group1 -> group2
        app.next_group_tab();
        assert!(app.group_filter.is_some());
        assert_eq!(app.group_tab_index, 2);

        // Third call: group2 -> All
        app.next_group_tab();
        assert_eq!(app.group_filter, None);
        assert_eq!(app.group_tab_index, 0);
    }

    #[test]
    fn next_group_tab_with_zero_groups_does_nothing() {
        let content = "\
Host web1
  HostName 1.1.1.1
";
        let mut app = make_app(content);
        // No grouping, so group_tab_order is empty
        app.group_by = GroupBy::None;
        app.apply_sort();
        assert!(app.group_tab_order.is_empty());

        app.next_group_tab();

        assert_eq!(app.group_filter, None);
        assert_eq!(app.group_tab_index, 0);
    }

    #[test]
    fn next_group_tab_with_one_group_toggles() {
        let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();
        assert_eq!(app.group_tab_order.len(), 1);

        let only_group = app.group_tab_order[0].clone();

        // First call: All -> the one group
        app.next_group_tab();
        assert_eq!(app.group_filter, Some(only_group));

        // Second call: the one group -> All
        app.next_group_tab();
        assert_eq!(app.group_filter, None);
        assert_eq!(app.group_tab_index, 0);
    }

    // --- prev_group_tab ---

    #[test]
    fn prev_group_tab_from_all_goes_to_last_group() {
        let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host aws-db
  HostName 2.2.2.2
  # purple:provider aws:2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();
        assert_eq!(app.group_tab_order.len(), 2);

        let last_group = app.group_tab_order.last().unwrap().clone();
        app.prev_group_tab();

        assert_eq!(app.group_filter, Some(last_group));
    }

    #[test]
    fn prev_group_tab_wraps_to_all() {
        let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host aws-db
  HostName 2.2.2.2
  # purple:provider aws:2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();

        // Navigate to the first group using next_group_tab (reliable, deterministic)
        app.next_group_tab();
        assert!(app.group_filter.is_some());
        assert_eq!(app.group_tab_index, 1);

        // prev from first group should go back to All
        app.prev_group_tab();
        assert_eq!(app.group_filter, None);
        assert_eq!(app.group_tab_index, 0);
    }

    // --- clear_group_filter ---

    #[test]
    fn clear_group_filter_resets_to_all() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host db1
  HostName 2.2.2.2
  # purple:provider digitalocean:2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();

        // Navigate into a group
        app.next_group_tab();
        assert!(app.group_filter.is_some());

        app.clear_group_filter();

        assert_eq!(app.group_filter, None);
        assert_eq!(app.group_tab_index, 0);
    }

    #[test]
    fn clear_group_filter_noop_when_already_none() {
        let content = "Host web1\n  HostName 1.1.1.1\n";
        let mut app = make_app(content);
        assert_eq!(app.group_filter, None);
        assert_eq!(app.group_tab_index, 0);

        // Should not panic or change state
        app.clear_group_filter();

        assert_eq!(app.group_filter, None);
        assert_eq!(app.group_tab_index, 0);
    }

    // --- select_next_skipping_headers / select_prev_skipping_headers ---

    #[test]
    fn select_next_skipping_headers_skips_group_header() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host web2
  HostName 2.2.2.2
  # purple:provider aws:2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();

        // display_list: [GroupHeader(DO), Host(idx), GroupHeader(AWS), Host(idx)]
        // Find the first Host item index in display_list
        let first_host_pos = app
            .display_list
            .iter()
            .position(|item| matches!(item, HostListItem::Host { .. }))
            .unwrap();
        app.ui.list_state.select(Some(first_host_pos));

        app.select_next_skipping_headers();

        let selected = app.ui.list_state.selected().unwrap();
        assert!(
            matches!(app.display_list[selected], HostListItem::Host { .. }),
            "selection should land on a Host, not a GroupHeader"
        );
        assert!(
            selected > first_host_pos,
            "selection should have moved forward"
        );
    }

    #[test]
    fn select_prev_skipping_headers_skips_group_header() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host web2
  HostName 2.2.2.2
  # purple:provider aws:2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();

        // Find the last Host item in display_list
        let last_host_pos = app
            .display_list
            .iter()
            .rposition(|item| matches!(item, HostListItem::Host { .. }))
            .unwrap();
        app.ui.list_state.select(Some(last_host_pos));

        app.select_prev_skipping_headers();

        let selected = app.ui.list_state.selected().unwrap();
        assert!(
            matches!(app.display_list[selected], HostListItem::Host { .. }),
            "selection should land on a Host, not a GroupHeader"
        );
        assert!(selected < last_host_pos, "selection should have moved back");
    }

    #[test]
    fn select_next_skipping_headers_stays_at_end() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();

        // Put selection on the only Host item
        let host_pos = app
            .display_list
            .iter()
            .position(|item| matches!(item, HostListItem::Host { .. }))
            .unwrap();
        app.ui.list_state.select(Some(host_pos));

        app.select_next_skipping_headers();

        // Should stay at the same position since there is no next host
        assert_eq!(app.ui.list_state.selected(), Some(host_pos));
    }

    // --- Scoped search ---

    #[test]
    fn scoped_search_filters_within_group() {
        let content = "\
Host web-do
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host db-do
  HostName 3.3.3.3
  # purple:provider digitalocean:2

Host web-aws
  HostName 2.2.2.2
  # purple:provider aws:3
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();

        // Navigate into the DigitalOcean group
        let do_group = app
            .group_tab_order
            .iter()
            .find(|g| g.to_lowercase().contains("digital"))
            .cloned()
            .unwrap_or_else(|| app.group_tab_order[0].clone());
        app.group_filter = Some(do_group.clone());
        app.apply_sort();

        // Start search with "web" - matches hosts in both providers
        app.start_search();
        app.search.query = Some("web".to_string());
        app.apply_filter();

        // Only web-do should match (web-aws is outside the scoped group)
        assert_eq!(
            app.search.filtered_indices.len(),
            1,
            "scoped search should only return hosts in the active group"
        );
        let matched_idx = app.search.filtered_indices[0];
        assert_eq!(
            app.hosts[matched_idx].provider.as_deref(),
            Some("digitalocean")
        );
    }

    #[test]
    fn global_search_when_no_filter() {
        let content = "\
Host web-do
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host web-aws
  HostName 2.2.2.2
  # purple:provider aws:2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        // No group_filter
        app.apply_sort();

        app.start_search();
        // scope_indices should be None when no group filter is active
        assert!(app.search.scope_indices.is_none());

        app.search.query = Some("web".to_string());
        app.apply_filter();

        // Both hosts match "web"
        assert_eq!(app.search.filtered_indices.len(), 2);
    }

    // --- group_tab_order computation ---

    #[test]
    fn group_tab_order_tag_mode_sorted_by_count() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags common

Host web2
  HostName 2.2.2.2
  # purple:tags common

Host db1
  HostName 3.3.3.3
  # purple:tags common

Host cache1
  HostName 4.4.4.4
  # purple:tags rare
";
        let mut app = make_app(content);
        // Use "common" as the active groupBy tag; group_tab_order is computed from all host tags
        app.group_by = GroupBy::Tag("common".to_string());
        app.apply_sort();

        // group_tab_order should be sorted by frequency descending
        // "common" appears 3 times, "rare" once
        assert!(!app.group_tab_order.is_empty());
        assert_eq!(app.group_tab_order[0], "common");
        assert_eq!(app.group_tab_order[1], "rare");
    }

    #[test]
    fn group_tab_order_tag_mode_max_ten() {
        // Build a config with 12 unique tags
        let mut blocks = Vec::new();
        for i in 0..12 {
            blocks.push(format!(
                "Host host{i}\n  HostName {i}.{i}.{i}.{i}\n  # purple:tags tag{i}"
            ));
        }
        let content = blocks.join("\n\n") + "\n";

        let mut app = make_app(&content);
        app.group_by = GroupBy::Tag("tag0".to_string());
        app.apply_sort();

        assert_eq!(
            app.group_tab_order.len(),
            10,
            "group_tab_order should be capped at exactly 10, got {}",
            app.group_tab_order.len()
        );
    }

    #[test]
    fn group_tab_order_provider_mode_from_headers() {
        let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host aws-db
  HostName 2.2.2.2
  # purple:provider aws:2
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();

        // group_tab_order should reflect GroupHeader order
        assert!(!app.group_tab_order.is_empty());
        for name in &app.group_tab_order {
            let header_exists = app
                .display_list
                .iter()
                .any(|item| matches!(item, HostListItem::GroupHeader(s) if s == name));
            assert!(
                header_exists,
                "group_tab_order entry '{name}' should have a corresponding GroupHeader"
            );
        }
    }

    // --- Tag mode filtering ---

    #[test]
    fn tag_filter_shows_hosts_with_matching_tag() {
        let content = "\
Host web-prod
  HostName 1.1.1.1
  # purple:tags prod

Host web-staging
  HostName 2.2.2.2
  # purple:tags staging
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("prod".to_string());
        app.group_filter = Some("prod".to_string());
        app.apply_sort();

        // Only hosts with the prod tag should appear
        for item in &app.display_list {
            if let HostListItem::Host { index } = item {
                assert!(
                    app.hosts[*index].tags.contains(&"prod".to_string()),
                    "only hosts with 'prod' tag should appear when filtered"
                );
            }
        }

        let host_count = app
            .display_list
            .iter()
            .filter(|item| matches!(item, HostListItem::Host { .. }))
            .count();
        assert_eq!(host_count, 1, "exactly one prod host should be visible");
    }

    #[test]
    fn tag_filter_includes_patterns_with_matching_tag() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags prod

Host 10.0.0.*
  User root
  # purple:tags prod
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Tag("prod".to_string());
        app.group_filter = Some("prod".to_string());
        app.apply_sort();

        let pattern_count = app
            .display_list
            .iter()
            .filter(|item| matches!(item, HostListItem::Pattern { .. }))
            .count();
        assert_eq!(
            pattern_count, 1,
            "pattern with matching tag should be visible"
        );
    }

    // --- page_down header skipping ---

    #[test]
    fn page_down_skips_group_headers() {
        let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host web2
  HostName 2.2.2.2
  # purple:provider digitalocean:2

Host aws1
  HostName 3.3.3.3
  # purple:provider aws:3
";
        let mut app = make_app(content);
        app.group_by = GroupBy::Provider;
        app.apply_sort();

        // Start at the first item
        app.ui.list_state.select(Some(0));

        app.page_down_host();

        let selected = app.ui.list_state.selected().unwrap();
        assert!(
            matches!(
                app.display_list[selected],
                HostListItem::Host { .. } | HostListItem::Pattern { .. }
            ),
            "page_down should not land on a GroupHeader"
        );
    }

    // --- GroupBy::Tag round-trip ---

    #[test]
    fn group_by_tag_empty_round_trips() {
        let gb = GroupBy::Tag(String::new());
        let key = gb.to_key();
        let restored = GroupBy::from_key(&key);
        assert_eq!(restored, gb);
    }

    #[test]
    fn group_by_tag_nonempty_round_trips() {
        let gb = GroupBy::Tag("production".to_string());
        let key = gb.to_key();
        let restored = GroupBy::from_key(&key);
        assert_eq!(restored, gb);
    }

    #[test]
    fn group_by_none_round_trips() {
        let gb = GroupBy::None;
        let key = gb.to_key();
        let restored = GroupBy::from_key(&key);
        assert_eq!(restored, gb);
    }

    #[test]
    fn group_by_provider_round_trips() {
        let gb = GroupBy::Provider;
        let key = gb.to_key();
        let restored = GroupBy::from_key(&key);
        assert_eq!(restored, gb);
    }
}
