use std::path::PathBuf;

/// Represents the entire SSH config file as a sequence of elements.
/// Preserves the original structure for round-trip fidelity.
#[derive(Debug, Clone)]
pub struct SshConfigFile {
    pub elements: Vec<ConfigElement>,
    pub path: PathBuf,
    /// Whether the original file used CRLF line endings.
    pub crlf: bool,
}

/// An Include directive that references other config files.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IncludeDirective {
    pub raw_line: String,
    pub pattern: String,
    pub resolved_files: Vec<IncludedFile>,
}

/// A file resolved from an Include directive.
#[derive(Debug, Clone)]
pub struct IncludedFile {
    pub path: PathBuf,
    pub elements: Vec<ConfigElement>,
}

/// A single element in the config file.
#[derive(Debug, Clone)]
pub enum ConfigElement {
    /// A Host block: the "Host <pattern>" line plus all indented directives.
    HostBlock(HostBlock),
    /// A comment, blank line, or global directive not inside a Host block.
    GlobalLine(String),
    /// An Include directive referencing other config files (read-only).
    Include(IncludeDirective),
}

/// A parsed Host block with its directives.
#[derive(Debug, Clone)]
pub struct HostBlock {
    /// The host alias/pattern (the value after "Host").
    pub host_pattern: String,
    /// The original raw "Host ..." line for faithful reproduction.
    pub raw_host_line: String,
    /// Parsed directives inside this block.
    pub directives: Vec<Directive>,
}

/// A directive line inside a Host block.
#[derive(Debug, Clone)]
pub struct Directive {
    /// The directive key (e.g., "HostName", "User", "Port").
    pub key: String,
    /// The directive value.
    pub value: String,
    /// The original raw line (preserves indentation, inline comments).
    pub raw_line: String,
    /// Whether this is a comment-only or blank line inside a host block.
    pub is_non_directive: bool,
}

/// Convenience view for the TUI — extracted from a HostBlock.
#[derive(Debug, Clone)]
pub struct HostEntry {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub identity_file: String,
    pub proxy_jump: String,
    /// If this host comes from an included file, the file path.
    pub source_file: Option<PathBuf>,
    /// Tags from purple:tags comment.
    pub tags: Vec<String>,
    /// Cloud provider label from purple:provider comment (e.g. "do", "vultr").
    pub provider: Option<String>,
    /// Number of tunnel forwarding directives.
    pub tunnel_count: u16,
    /// Password source from purple:askpass comment (e.g. "keychain", "op://...", "pass:...").
    pub askpass: Option<String>,
}

impl Default for HostEntry {
    fn default() -> Self {
        Self {
            alias: String::new(),
            hostname: String::new(),
            user: String::new(),
            port: 22,
            identity_file: String::new(),
            proxy_jump: String::new(),
            source_file: None,
            tags: Vec::new(),
            provider: None,
            tunnel_count: 0,
            askpass: None,
        }
    }
}

impl HostEntry {
    /// Build the SSH command string for this host (e.g. "ssh -- 'myserver'").
    /// Shell-quotes the alias to prevent injection when pasted into a terminal.
    pub fn ssh_command(&self) -> String {
        let escaped = self.alias.replace('\'', "'\\''");
        format!("ssh -- '{}'", escaped)
    }
}

/// Returns true if the host pattern contains wildcards, character classes,
/// negation or whitespace-separated multi-patterns (*, ?, [], !, space/tab).
/// These are SSH match patterns, not concrete hosts.
pub fn is_host_pattern(pattern: &str) -> bool {
    pattern.contains('*')
        || pattern.contains('?')
        || pattern.contains('[')
        || pattern.starts_with('!')
        || pattern.contains(' ')
        || pattern.contains('\t')
}

impl HostBlock {
    /// Index of the first trailing blank line (for inserting content before separators).
    fn content_end(&self) -> usize {
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
    fn pop_trailing_blanks(&mut self) -> Vec<Directive> {
        let end = self.content_end();
        self.directives.drain(end..).collect()
    }

    /// Ensure exactly one trailing blank line.
    fn ensure_trailing_blank(&mut self) {
        self.pop_trailing_blanks();
        self.directives.push(Directive {
            key: String::new(),
            value: String::new(),
            raw_line: String::new(),
            is_non_directive: true,
        });
    }

    /// Detect indentation used by existing directives (falls back to "  ").
    fn detect_indent(&self) -> String {
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
            !(d.is_non_directive && d.raw_line.trim().starts_with("# purple:provider"))
        });
        let pos = self.content_end();
        self.directives.insert(
            pos,
            Directive {
                key: String::new(),
                value: String::new(),
                raw_line: format!("{}# purple:provider {}:{}", indent, provider_name, server_id),
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

    /// Set askpass source on a host block. Replaces existing purple:askpass comment or adds one.
    /// Pass an empty string to remove the comment.
    pub fn set_askpass(&mut self, source: &str) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && d.raw_line.trim().starts_with("# purple:askpass"))
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

    /// Set tags on a host block. Replaces existing purple:tags comment or adds one.
    pub fn set_tags(&mut self, tags: &[String]) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && d.raw_line.trim().starts_with("# purple:tags"))
        });
        if !tags.is_empty() {
            let pos = self.content_end();
            self.directives.insert(
                pos,
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: format!("{}# purple:tags {}", indent, tags.join(",")),
                    is_non_directive: true,
                },
            );
        }
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
            }
        }
        entry.tags = self.tags();
        entry.provider = self.provider().map(|(name, _)| name);
        entry.tunnel_count = self.tunnel_count();
        entry.askpass = self.askpass();
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

impl SshConfigFile {
    /// Get all host entries as convenience views (including from Include files).
    pub fn host_entries(&self) -> Vec<HostEntry> {
        let mut entries = Vec::new();
        Self::collect_host_entries(&self.elements, &mut entries);
        entries
    }

    /// Collect all resolved Include file paths (recursively).
    pub fn include_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        Self::collect_include_paths(&self.elements, &mut paths);
        paths
    }

    fn collect_include_paths(elements: &[ConfigElement], paths: &mut Vec<PathBuf>) {
        for e in elements {
            if let ConfigElement::Include(include) = e {
                for file in &include.resolved_files {
                    paths.push(file.path.clone());
                    Self::collect_include_paths(&file.elements, paths);
                }
            }
        }
    }

    /// Collect parent directories of Include glob patterns.
    /// When a file is added/removed under a glob dir, the directory's mtime changes.
    pub fn include_glob_dirs(&self) -> Vec<PathBuf> {
        let config_dir = self.path.parent();
        let mut seen = std::collections::HashSet::new();
        let mut dirs = Vec::new();
        Self::collect_include_glob_dirs(&self.elements, config_dir, &mut seen, &mut dirs);
        dirs
    }

    fn collect_include_glob_dirs(
        elements: &[ConfigElement],
        config_dir: Option<&std::path::Path>,
        seen: &mut std::collections::HashSet<PathBuf>,
        dirs: &mut Vec<PathBuf>,
    ) {
        for e in elements {
            if let ConfigElement::Include(include) = e {
                // Split on whitespace to handle multi-pattern Includes
                // (same as resolve_include does)
                for single in include.pattern.split_whitespace() {
                    let expanded = Self::expand_tilde(single);
                    let resolved = if expanded.starts_with('/') {
                        PathBuf::from(&expanded)
                    } else if let Some(dir) = config_dir {
                        dir.join(&expanded)
                    } else {
                        continue;
                    };
                    if let Some(parent) = resolved.parent() {
                        let parent = parent.to_path_buf();
                        if seen.insert(parent.clone()) {
                            dirs.push(parent);
                        }
                    }
                }
                // Recurse into resolved files
                for file in &include.resolved_files {
                    Self::collect_include_glob_dirs(
                        &file.elements,
                        file.path.parent(),
                        seen,
                        dirs,
                    );
                }
            }
        }
    }


    /// Recursively collect host entries from a list of elements.
    fn collect_host_entries(elements: &[ConfigElement], entries: &mut Vec<HostEntry>) {
        for e in elements {
            match e {
                ConfigElement::HostBlock(block) => {
                    if is_host_pattern(&block.host_pattern) {
                        continue;
                    }
                    entries.push(block.to_host_entry());
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        let start = entries.len();
                        Self::collect_host_entries(&file.elements, entries);
                        for entry in &mut entries[start..] {
                            if entry.source_file.is_none() {
                                entry.source_file = Some(file.path.clone());
                            }
                        }
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
    }

    /// Check if a host alias already exists (including in Include files).
    /// Walks the element tree directly without building HostEntry structs.
    pub fn has_host(&self, alias: &str) -> bool {
        Self::has_host_in_elements(&self.elements, alias)
    }

    fn has_host_in_elements(elements: &[ConfigElement], alias: &str) -> bool {
        for e in elements {
            match e {
                ConfigElement::HostBlock(block) => {
                    if block.host_pattern.split_whitespace().any(|p| p == alias) {
                        return true;
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        if Self::has_host_in_elements(&file.elements, alias) {
                            return true;
                        }
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
        false
    }

    /// Check if a host alias is from an included file (read-only).
    /// Handles multi-pattern Host lines by splitting on whitespace.
    pub fn is_included_host(&self, alias: &str) -> bool {
        // Not in top-level elements → must be in an Include
        for e in &self.elements {
            match e {
                ConfigElement::HostBlock(block) => {
                    if block.host_pattern.split_whitespace().any(|p| p == alias) {
                        return false;
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        if Self::has_host_in_elements(&file.elements, alias) {
                            return true;
                        }
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
        false
    }

    /// Add a new host entry to the config.
    pub fn add_host(&mut self, entry: &HostEntry) {
        let block = Self::entry_to_block(entry);
        // Add a blank line separator if the file isn't empty and doesn't already end with one
        if !self.elements.is_empty() && !self.last_element_has_trailing_blank() {
            self.elements
                .push(ConfigElement::GlobalLine(String::new()));
        }
        self.elements.push(ConfigElement::HostBlock(block));
    }

    /// Check if the last element already ends with a blank line.
    pub fn last_element_has_trailing_blank(&self) -> bool {
        match self.elements.last() {
            Some(ConfigElement::HostBlock(block)) => block
                .directives
                .last()
                .is_some_and(|d| d.is_non_directive && d.raw_line.trim().is_empty()),
            Some(ConfigElement::GlobalLine(line)) => line.trim().is_empty(),
            _ => false,
        }
    }

    /// Update an existing host entry by alias.
    /// Merges changes into the existing block, preserving unknown directives.
    pub fn update_host(&mut self, old_alias: &str, entry: &HostEntry) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == old_alias {
                    // Update host pattern (preserve raw_host_line when alias unchanged)
                    if entry.alias != block.host_pattern {
                        block.host_pattern = entry.alias.clone();
                        block.raw_host_line = format!("Host {}", entry.alias);
                    }

                    // Merge known directives (update existing, add missing, remove empty)
                    Self::upsert_directive(block, "HostName", &entry.hostname);
                    Self::upsert_directive(block, "User", &entry.user);
                    if entry.port != 22 {
                        Self::upsert_directive(block, "Port", &entry.port.to_string());
                    } else {
                        // Remove explicit Port 22 (it's the default)
                        block
                            .directives
                            .retain(|d| d.is_non_directive || !d.key.eq_ignore_ascii_case("port"));
                    }
                    Self::upsert_directive(block, "IdentityFile", &entry.identity_file);
                    Self::upsert_directive(block, "ProxyJump", &entry.proxy_jump);
                    return;
                }
            }
        }
    }

    /// Update a directive in-place, add it if missing, or remove it if value is empty.
    fn upsert_directive(block: &mut HostBlock, key: &str, value: &str) {
        if value.is_empty() {
            block
                .directives
                .retain(|d| d.is_non_directive || !d.key.eq_ignore_ascii_case(key));
            return;
        }
        let indent = block.detect_indent();
        for d in &mut block.directives {
            if !d.is_non_directive && d.key.eq_ignore_ascii_case(key) {
                // Only rebuild raw_line when value actually changed (preserves inline comments)
                if d.value != value {
                    d.value = value.to_string();
                    // Detect separator style from original raw_line and preserve it.
                    // Handles: "Key value", "Key=value", "Key = value", "Key =value"
                    // Only considers '=' as separator if it appears before any
                    // non-whitespace content (avoids matching '=' inside values
                    // like "IdentityFile ~/.ssh/id=prod").
                    let trimmed = d.raw_line.trim_start();
                    let after_key = &trimmed[d.key.len()..];
                    let sep = if after_key.trim_start().starts_with('=') {
                        let eq_pos = after_key.find('=').unwrap();
                        let after_eq = &after_key[eq_pos + 1..];
                        let trailing_ws = after_eq.len() - after_eq.trim_start().len();
                        after_key[..eq_pos + 1 + trailing_ws].to_string()
                    } else {
                        " ".to_string()
                    };
                    d.raw_line = format!("{}{}{}{}", indent, d.key, sep, value);
                }
                return;
            }
        }
        // Not found — insert before trailing blanks
        let pos = block.content_end();
        block.directives.insert(
            pos,
            Directive {
                key: key.to_string(),
                value: value.to_string(),
                raw_line: format!("{}{} {}", indent, key, value),
                is_non_directive: false,
            },
        );
    }

    /// Set provider on a host block by alias.
    pub fn set_host_provider(&mut self, alias: &str, provider_name: &str, server_id: &str) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    block.set_provider(provider_name, server_id);
                    return;
                }
            }
        }
    }

    /// Find all hosts with a specific provider, returning (alias, server_id) pairs.
    /// Searches both top-level elements and Include files so that provider hosts
    /// in included configs are recognized during sync (prevents duplicate additions).
    pub fn find_hosts_by_provider(&self, provider_name: &str) -> Vec<(String, String)> {
        let mut results = Vec::new();
        Self::collect_provider_hosts(&self.elements, provider_name, &mut results);
        results
    }

    fn collect_provider_hosts(
        elements: &[ConfigElement],
        provider_name: &str,
        results: &mut Vec<(String, String)>,
    ) {
        for element in elements {
            match element {
                ConfigElement::HostBlock(block) => {
                    if let Some((name, id)) = block.provider() {
                        if name == provider_name {
                            results.push((block.host_pattern.clone(), id));
                        }
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        Self::collect_provider_hosts(&file.elements, provider_name, results);
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
    }

    /// Compare two directive values with whitespace normalization.
    /// Handles hand-edited configs with tabs or multiple spaces.
    fn values_match(a: &str, b: &str) -> bool {
        a.split_whitespace().eq(b.split_whitespace())
    }

    /// Add a forwarding directive to a host block.
    /// Inserts at `content_end()` (before trailing blanks), using detected indentation.
    /// Uses split_whitespace matching for multi-pattern Host lines.
    pub fn add_forward(&mut self, alias: &str, directive_key: &str, value: &str) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern.split_whitespace().any(|p| p == alias) {
                    let indent = block.detect_indent();
                    let pos = block.content_end();
                    block.directives.insert(
                        pos,
                        Directive {
                            key: directive_key.to_string(),
                            value: value.to_string(),
                            raw_line: format!("{}{} {}", indent, directive_key, value),
                            is_non_directive: false,
                        },
                    );
                    return;
                }
            }
        }
    }

    /// Remove a specific forwarding directive from a host block.
    /// Matches key (case-insensitive) and value (whitespace-normalized).
    /// Uses split_whitespace matching for multi-pattern Host lines.
    /// Returns true if a directive was actually removed.
    pub fn remove_forward(&mut self, alias: &str, directive_key: &str, value: &str) -> bool {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern.split_whitespace().any(|p| p == alias) {
                    if let Some(pos) = block.directives.iter().position(|d| {
                        !d.is_non_directive
                            && d.key.eq_ignore_ascii_case(directive_key)
                            && Self::values_match(&d.value, value)
                    }) {
                        block.directives.remove(pos);
                        return true;
                    }
                    return false;
                }
            }
        }
        false
    }

    /// Check if a host block has a specific forwarding directive.
    /// Uses whitespace-normalized value comparison and split_whitespace host matching.
    pub fn has_forward(&self, alias: &str, directive_key: &str, value: &str) -> bool {
        for element in &self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern.split_whitespace().any(|p| p == alias) {
                    return block.directives.iter().any(|d| {
                        !d.is_non_directive
                            && d.key.eq_ignore_ascii_case(directive_key)
                            && Self::values_match(&d.value, value)
                    });
                }
            }
        }
        false
    }

    /// Find tunnel directives for a host alias, searching all elements including
    /// Include files. Uses split_whitespace matching like has_host() for multi-pattern
    /// Host lines.
    pub fn find_tunnel_directives(&self, alias: &str) -> Vec<crate::tunnel::TunnelRule> {
        Self::find_tunnel_directives_in(&self.elements, alias)
    }

    fn find_tunnel_directives_in(
        elements: &[ConfigElement],
        alias: &str,
    ) -> Vec<crate::tunnel::TunnelRule> {
        for element in elements {
            match element {
                ConfigElement::HostBlock(block) => {
                    if block.host_pattern.split_whitespace().any(|p| p == alias) {
                        return block.tunnel_directives();
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        let rules = Self::find_tunnel_directives_in(&file.elements, alias);
                        if !rules.is_empty() {
                            return rules;
                        }
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
        Vec::new()
    }

    /// Generate a unique alias by appending -2, -3, etc. if the base alias is taken.
    pub fn deduplicate_alias(&self, base: &str) -> String {
        self.deduplicate_alias_excluding(base, None)
    }

    /// Generate a unique alias, optionally excluding one alias from collision detection.
    /// Used during rename so the host being renamed doesn't collide with itself.
    pub fn deduplicate_alias_excluding(&self, base: &str, exclude: Option<&str>) -> String {
        let is_taken = |alias: &str| {
            if exclude == Some(alias) {
                return false;
            }
            self.has_host(alias)
        };
        if !is_taken(base) {
            return base.to_string();
        }
        for n in 2..=9999 {
            let candidate = format!("{}-{}", base, n);
            if !is_taken(&candidate) {
                return candidate;
            }
        }
        // Practically unreachable: fall back to PID-based suffix
        format!("{}-{}", base, std::process::id())
    }

    /// Set tags on a host block by alias.
    pub fn set_host_tags(&mut self, alias: &str, tags: &[String]) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    block.set_tags(tags);
                    return;
                }
            }
        }
    }

    /// Set askpass source on a host block by alias.
    pub fn set_host_askpass(&mut self, alias: &str, source: &str) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    block.set_askpass(source);
                    return;
                }
            }
        }
    }

    /// Delete a host entry by alias.
    #[allow(dead_code)]
    pub fn delete_host(&mut self, alias: &str) {
        self.elements.retain(|e| match e {
            ConfigElement::HostBlock(block) => block.host_pattern != alias,
            _ => true,
        });
        // Collapse consecutive blank lines left by deletion
        self.elements.dedup_by(|a, b| {
            matches!(
                (&*a, &*b),
                (ConfigElement::GlobalLine(x), ConfigElement::GlobalLine(y))
                if x.trim().is_empty() && y.trim().is_empty()
            )
        });
    }

    /// Delete a host and return the removed element and its position for undo.
    /// Does NOT collapse blank lines so the position stays valid for re-insertion.
    pub fn delete_host_undoable(&mut self, alias: &str) -> Option<(ConfigElement, usize)> {
        let pos = self.elements.iter().position(|e| {
            matches!(e, ConfigElement::HostBlock(b) if b.host_pattern == alias)
        })?;
        let element = self.elements.remove(pos);
        Some((element, pos))
    }

    /// Insert a host block at a specific position (for undo).
    pub fn insert_host_at(&mut self, element: ConfigElement, position: usize) {
        let pos = position.min(self.elements.len());
        self.elements.insert(pos, element);
    }

    /// Swap two host blocks in the config by alias. Returns true if swap was performed.
    #[allow(dead_code)]
    pub fn swap_hosts(&mut self, alias_a: &str, alias_b: &str) -> bool {
        let pos_a = self.elements.iter().position(|e| {
            matches!(e, ConfigElement::HostBlock(b) if b.host_pattern == alias_a)
        });
        let pos_b = self.elements.iter().position(|e| {
            matches!(e, ConfigElement::HostBlock(b) if b.host_pattern == alias_b)
        });
        if let (Some(a), Some(b)) = (pos_a, pos_b) {
            if a == b {
                return false;
            }
            let (first, second) = (a.min(b), a.max(b));

            // Strip trailing blanks from both blocks before swap
            if let ConfigElement::HostBlock(block) = &mut self.elements[first] {
                block.pop_trailing_blanks();
            }
            if let ConfigElement::HostBlock(block) = &mut self.elements[second] {
                block.pop_trailing_blanks();
            }

            // Swap
            self.elements.swap(first, second);

            // Add trailing blank to first block (separator between the two)
            if let ConfigElement::HostBlock(block) = &mut self.elements[first] {
                block.ensure_trailing_blank();
            }

            // Add trailing blank to second only if not the last element
            if second < self.elements.len() - 1 {
                if let ConfigElement::HostBlock(block) = &mut self.elements[second] {
                    block.ensure_trailing_blank();
                }
            }

            return true;
        }
        false
    }

    /// Convert a HostEntry into a new HostBlock with clean formatting.
    pub(crate) fn entry_to_block(entry: &HostEntry) -> HostBlock {
        let mut directives = Vec::new();

        if !entry.hostname.is_empty() {
            directives.push(Directive {
                key: "HostName".to_string(),
                value: entry.hostname.clone(),
                raw_line: format!("  HostName {}", entry.hostname),
                is_non_directive: false,
            });
        }
        if !entry.user.is_empty() {
            directives.push(Directive {
                key: "User".to_string(),
                value: entry.user.clone(),
                raw_line: format!("  User {}", entry.user),
                is_non_directive: false,
            });
        }
        if entry.port != 22 {
            directives.push(Directive {
                key: "Port".to_string(),
                value: entry.port.to_string(),
                raw_line: format!("  Port {}", entry.port),
                is_non_directive: false,
            });
        }
        if !entry.identity_file.is_empty() {
            directives.push(Directive {
                key: "IdentityFile".to_string(),
                value: entry.identity_file.clone(),
                raw_line: format!("  IdentityFile {}", entry.identity_file),
                is_non_directive: false,
            });
        }
        if !entry.proxy_jump.is_empty() {
            directives.push(Directive {
                key: "ProxyJump".to_string(),
                value: entry.proxy_jump.clone(),
                raw_line: format!("  ProxyJump {}", entry.proxy_jump),
                is_non_directive: false,
            });
        }

        HostBlock {
            host_pattern: entry.alias.clone(),
            raw_host_line: format!("Host {}", entry.alias),
            directives,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(content: &str) -> SshConfigFile {
        SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: PathBuf::from("/tmp/test_config"),
            crlf: false,
        }
    }

    #[test]
    fn tunnel_directives_extracts_forwards() {
        let config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n  RemoteForward 9090 localhost:3000\n  DynamicForward 1080\n",
        );
        if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
            let rules = block.tunnel_directives();
            assert_eq!(rules.len(), 3);
            assert_eq!(rules[0].tunnel_type, crate::tunnel::TunnelType::Local);
            assert_eq!(rules[0].bind_port, 8080);
            assert_eq!(rules[1].tunnel_type, crate::tunnel::TunnelType::Remote);
            assert_eq!(rules[2].tunnel_type, crate::tunnel::TunnelType::Dynamic);
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn tunnel_count_counts_forwards() {
        let config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n  RemoteForward 9090 localhost:3000\n",
        );
        if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
            assert_eq!(block.tunnel_count(), 2);
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn tunnel_count_zero_for_no_forwards() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  User admin\n");
        if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
            assert_eq!(block.tunnel_count(), 0);
            assert!(!block.has_tunnels());
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn has_tunnels_true_with_forward() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  DynamicForward 1080\n");
        if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
            assert!(block.has_tunnels());
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn add_forward_inserts_directive() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  User admin\n");
        config.add_forward("myserver", "LocalForward", "8080 localhost:80");
        let output = config.serialize();
        assert!(output.contains("LocalForward 8080 localhost:80"));
        // Existing directives preserved
        assert!(output.contains("HostName 10.0.0.1"));
        assert!(output.contains("User admin"));
    }

    #[test]
    fn add_forward_preserves_indentation() {
        let mut config = parse_str("Host myserver\n\tHostName 10.0.0.1\n");
        config.add_forward("myserver", "LocalForward", "8080 localhost:80");
        let output = config.serialize();
        assert!(output.contains("\tLocalForward 8080 localhost:80"));
    }

    #[test]
    fn add_multiple_forwards_same_type() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        config.add_forward("myserver", "LocalForward", "8080 localhost:80");
        config.add_forward("myserver", "LocalForward", "9090 localhost:90");
        let output = config.serialize();
        assert!(output.contains("LocalForward 8080 localhost:80"));
        assert!(output.contains("LocalForward 9090 localhost:90"));
    }

    #[test]
    fn remove_forward_removes_exact_match() {
        let mut config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n  LocalForward 9090 localhost:90\n",
        );
        config.remove_forward("myserver", "LocalForward", "8080 localhost:80");
        let output = config.serialize();
        assert!(!output.contains("8080 localhost:80"));
        assert!(output.contains("9090 localhost:90"));
    }

    #[test]
    fn remove_forward_leaves_other_directives() {
        let mut config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n  User admin\n",
        );
        config.remove_forward("myserver", "LocalForward", "8080 localhost:80");
        let output = config.serialize();
        assert!(!output.contains("LocalForward"));
        assert!(output.contains("HostName 10.0.0.1"));
        assert!(output.contains("User admin"));
    }

    #[test]
    fn remove_forward_no_match_is_noop() {
        let original = "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n";
        let mut config = parse_str(original);
        config.remove_forward("myserver", "LocalForward", "9999 localhost:99");
        assert_eq!(config.serialize(), original);
    }

    #[test]
    fn host_entry_tunnel_count_populated() {
        let config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n  DynamicForward 1080\n",
        );
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tunnel_count, 2);
    }

    #[test]
    fn remove_forward_returns_true_on_match() {
        let mut config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n",
        );
        assert!(config.remove_forward("myserver", "LocalForward", "8080 localhost:80"));
    }

    #[test]
    fn remove_forward_returns_false_on_no_match() {
        let mut config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n",
        );
        assert!(!config.remove_forward("myserver", "LocalForward", "9999 localhost:99"));
    }

    #[test]
    fn remove_forward_returns_false_for_unknown_host() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        assert!(!config.remove_forward("nohost", "LocalForward", "8080 localhost:80"));
    }

    #[test]
    fn has_forward_finds_match() {
        let config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n",
        );
        assert!(config.has_forward("myserver", "LocalForward", "8080 localhost:80"));
    }

    #[test]
    fn has_forward_no_match() {
        let config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n",
        );
        assert!(!config.has_forward("myserver", "LocalForward", "9999 localhost:99"));
        assert!(!config.has_forward("nohost", "LocalForward", "8080 localhost:80"));
    }

    #[test]
    fn has_forward_case_insensitive_key() {
        let config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n  localforward 8080 localhost:80\n",
        );
        assert!(config.has_forward("myserver", "LocalForward", "8080 localhost:80"));
    }

    #[test]
    fn add_forward_to_empty_block() {
        let mut config = parse_str("Host myserver\n");
        config.add_forward("myserver", "LocalForward", "8080 localhost:80");
        let output = config.serialize();
        assert!(output.contains("LocalForward 8080 localhost:80"));
    }

    #[test]
    fn remove_forward_case_insensitive_key_match() {
        let mut config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n  localforward 8080 localhost:80\n",
        );
        assert!(config.remove_forward("myserver", "LocalForward", "8080 localhost:80"));
        assert!(!config.serialize().contains("localforward"));
    }

    #[test]
    fn tunnel_count_case_insensitive() {
        let config = parse_str(
            "Host myserver\n  localforward 8080 localhost:80\n  REMOTEFORWARD 9090 localhost:90\n  dynamicforward 1080\n",
        );
        if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
            assert_eq!(block.tunnel_count(), 3);
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn tunnel_directives_extracts_all_types() {
        let config = parse_str(
            "Host myserver\n  LocalForward 8080 localhost:80\n  RemoteForward 9090 localhost:3000\n  DynamicForward 1080\n",
        );
        if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
            let rules = block.tunnel_directives();
            assert_eq!(rules.len(), 3);
            assert_eq!(rules[0].tunnel_type, crate::tunnel::TunnelType::Local);
            assert_eq!(rules[1].tunnel_type, crate::tunnel::TunnelType::Remote);
            assert_eq!(rules[2].tunnel_type, crate::tunnel::TunnelType::Dynamic);
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn tunnel_directives_skips_malformed() {
        let config = parse_str(
            "Host myserver\n  LocalForward not_valid\n  DynamicForward 1080\n",
        );
        if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
            let rules = block.tunnel_directives();
            assert_eq!(rules.len(), 1);
            assert_eq!(rules[0].bind_port, 1080);
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn find_tunnel_directives_multi_pattern_host() {
        let config = parse_str(
            "Host prod staging\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n",
        );
        let rules = config.find_tunnel_directives("prod");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].bind_port, 8080);
        let rules2 = config.find_tunnel_directives("staging");
        assert_eq!(rules2.len(), 1);
    }

    #[test]
    fn find_tunnel_directives_no_match() {
        let config = parse_str(
            "Host myserver\n  LocalForward 8080 localhost:80\n",
        );
        let rules = config.find_tunnel_directives("nohost");
        assert!(rules.is_empty());
    }

    #[test]
    fn has_forward_exact_match() {
        let config = parse_str(
            "Host myserver\n  LocalForward 8080 localhost:80\n",
        );
        assert!(config.has_forward("myserver", "LocalForward", "8080 localhost:80"));
        assert!(!config.has_forward("myserver", "LocalForward", "9090 localhost:80"));
        assert!(!config.has_forward("myserver", "RemoteForward", "8080 localhost:80"));
        assert!(!config.has_forward("nohost", "LocalForward", "8080 localhost:80"));
    }

    #[test]
    fn has_forward_whitespace_normalized() {
        let config = parse_str(
            "Host myserver\n  LocalForward 8080  localhost:80\n",
        );
        // Extra space in config value vs single space in query — should still match
        assert!(config.has_forward("myserver", "LocalForward", "8080 localhost:80"));
    }

    #[test]
    fn has_forward_multi_pattern_host() {
        let config = parse_str(
            "Host prod staging\n  LocalForward 8080 localhost:80\n",
        );
        assert!(config.has_forward("prod", "LocalForward", "8080 localhost:80"));
        assert!(config.has_forward("staging", "LocalForward", "8080 localhost:80"));
    }

    #[test]
    fn add_forward_multi_pattern_host() {
        let mut config = parse_str(
            "Host prod staging\n  HostName 10.0.0.1\n",
        );
        config.add_forward("prod", "LocalForward", "8080 localhost:80");
        assert!(config.has_forward("prod", "LocalForward", "8080 localhost:80"));
        assert!(config.has_forward("staging", "LocalForward", "8080 localhost:80"));
    }

    #[test]
    fn remove_forward_multi_pattern_host() {
        let mut config = parse_str(
            "Host prod staging\n  LocalForward 8080 localhost:80\n  LocalForward 9090 localhost:90\n",
        );
        assert!(config.remove_forward("staging", "LocalForward", "8080 localhost:80"));
        assert!(!config.has_forward("staging", "LocalForward", "8080 localhost:80"));
        // Other forward should remain
        assert!(config.has_forward("staging", "LocalForward", "9090 localhost:90"));
    }

    #[test]
    fn edit_tunnel_detects_duplicate_after_remove() {
        // Simulates edit flow: remove old, then check if new value already exists
        let mut config = parse_str(
            "Host myserver\n  LocalForward 8080 localhost:80\n  LocalForward 9090 localhost:90\n",
        );
        // Edit rule A (8080) toward rule B (9090): remove A first
        assert!(config.remove_forward("myserver", "LocalForward", "8080 localhost:80"));
        // Now check if the target value already exists — should detect duplicate
        assert!(config.has_forward("myserver", "LocalForward", "9090 localhost:90"));
    }

    #[test]
    fn has_forward_tab_whitespace_normalized() {
        let config = parse_str(
            "Host myserver\n  LocalForward 8080\tlocalhost:80\n",
        );
        // Tab in config value vs space in query — should match via values_match
        assert!(config.has_forward("myserver", "LocalForward", "8080 localhost:80"));
    }

    #[test]
    fn remove_forward_tab_whitespace_normalized() {
        let mut config = parse_str(
            "Host myserver\n  LocalForward 8080\tlocalhost:80\n",
        );
        // Remove with single space should match tab-separated value
        assert!(config.remove_forward("myserver", "LocalForward", "8080 localhost:80"));
        assert!(!config.has_forward("myserver", "LocalForward", "8080 localhost:80"));
    }

    #[test]
    fn upsert_preserves_space_separator_when_value_contains_equals() {
        let mut config = parse_str(
            "Host myserver\n  IdentityFile ~/.ssh/id=prod\n",
        );
        let entry = HostEntry {
            alias: "myserver".to_string(),
            hostname: "10.0.0.1".to_string(),
            identity_file: "~/.ssh/id=staging".to_string(),
            port: 22,
            ..Default::default()
        };
        config.update_host("myserver", &entry);
        let output = config.serialize();
        // Separator should remain a space, not pick up the = from the value
        assert!(output.contains("  IdentityFile ~/.ssh/id=staging"), "got: {}", output);
        assert!(!output.contains("IdentityFile="), "got: {}", output);
    }

    #[test]
    fn upsert_preserves_equals_separator() {
        let mut config = parse_str(
            "Host myserver\n  IdentityFile=~/.ssh/id_rsa\n",
        );
        let entry = HostEntry {
            alias: "myserver".to_string(),
            hostname: "10.0.0.1".to_string(),
            identity_file: "~/.ssh/id_ed25519".to_string(),
            port: 22,
            ..Default::default()
        };
        config.update_host("myserver", &entry);
        let output = config.serialize();
        assert!(output.contains("IdentityFile=~/.ssh/id_ed25519"), "got: {}", output);
    }

    #[test]
    fn upsert_preserves_spaced_equals_separator() {
        let mut config = parse_str(
            "Host myserver\n  IdentityFile = ~/.ssh/id_rsa\n",
        );
        let entry = HostEntry {
            alias: "myserver".to_string(),
            hostname: "10.0.0.1".to_string(),
            identity_file: "~/.ssh/id_ed25519".to_string(),
            port: 22,
            ..Default::default()
        };
        config.update_host("myserver", &entry);
        let output = config.serialize();
        assert!(output.contains("IdentityFile = ~/.ssh/id_ed25519"), "got: {}", output);
    }

    #[test]
    fn is_included_host_false_for_main_config() {
        let config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n",
        );
        assert!(!config.is_included_host("myserver"));
    }

    #[test]
    fn is_included_host_false_for_nonexistent() {
        let config = parse_str(
            "Host myserver\n  HostName 10.0.0.1\n",
        );
        assert!(!config.is_included_host("nohost"));
    }

    #[test]
    fn is_included_host_multi_pattern_main_config() {
        let config = parse_str(
            "Host prod staging\n  HostName 10.0.0.1\n",
        );
        assert!(!config.is_included_host("prod"));
        assert!(!config.is_included_host("staging"));
    }

    // =========================================================================
    // HostBlock::askpass() and set_askpass() tests
    // =========================================================================

    fn first_block(config: &SshConfigFile) -> &HostBlock {
        match config.elements.first().unwrap() {
            ConfigElement::HostBlock(b) => b,
            _ => panic!("Expected HostBlock"),
        }
    }

    fn block_by_index(config: &SshConfigFile, idx: usize) -> &HostBlock {
        let mut count = 0;
        for el in &config.elements {
            if let ConfigElement::HostBlock(b) = el {
                if count == idx {
                    return b;
                }
                count += 1;
            }
        }
        panic!("No HostBlock at index {}", idx);
    }

    #[test]
    fn askpass_returns_none_when_absent() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        assert_eq!(first_block(&config).askpass(), None);
    }

    #[test]
    fn askpass_returns_keychain() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        assert_eq!(first_block(&config).askpass(), Some("keychain".to_string()));
    }

    #[test]
    fn askpass_returns_op_uri() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://Vault/Item/field\n");
        assert_eq!(first_block(&config).askpass(), Some("op://Vault/Item/field".to_string()));
    }

    #[test]
    fn askpass_returns_vault_with_field() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#password\n");
        assert_eq!(first_block(&config).askpass(), Some("vault:secret/ssh#password".to_string()));
    }

    #[test]
    fn askpass_returns_bw_source() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass bw:my-item\n");
        assert_eq!(first_block(&config).askpass(), Some("bw:my-item".to_string()));
    }

    #[test]
    fn askpass_returns_pass_source() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass pass:ssh/prod\n");
        assert_eq!(first_block(&config).askpass(), Some("pass:ssh/prod".to_string()));
    }

    #[test]
    fn askpass_returns_custom_command() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass get-pass %a %h\n");
        assert_eq!(first_block(&config).askpass(), Some("get-pass %a %h".to_string()));
    }

    #[test]
    fn askpass_ignores_empty_value() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass \n");
        assert_eq!(first_block(&config).askpass(), None);
    }

    #[test]
    fn askpass_ignores_non_askpass_purple_comments() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:tags prod\n");
        assert_eq!(first_block(&config).askpass(), None);
    }

    #[test]
    fn set_askpass_adds_comment() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        config.set_host_askpass("myserver", "keychain");
        assert_eq!(first_block(&config).askpass(), Some("keychain".to_string()));
    }

    #[test]
    fn set_askpass_replaces_existing() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        config.set_host_askpass("myserver", "op://V/I/p");
        assert_eq!(first_block(&config).askpass(), Some("op://V/I/p".to_string()));
    }

    #[test]
    fn set_askpass_empty_removes_comment() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        config.set_host_askpass("myserver", "");
        assert_eq!(first_block(&config).askpass(), None);
    }

    #[test]
    fn set_askpass_preserves_other_directives() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  User admin\n  # purple:tags prod\n");
        config.set_host_askpass("myserver", "vault:secret/ssh");
        assert_eq!(first_block(&config).askpass(), Some("vault:secret/ssh".to_string()));
        let entry = first_block(&config).to_host_entry();
        assert_eq!(entry.user, "admin");
        assert!(entry.tags.contains(&"prod".to_string()));
    }

    #[test]
    fn set_askpass_preserves_indent() {
        let mut config = parse_str("Host myserver\n    HostName 10.0.0.1\n");
        config.set_host_askpass("myserver", "keychain");
        let raw = first_block(&config).directives.iter()
            .find(|d| d.raw_line.contains("purple:askpass"))
            .unwrap();
        assert!(raw.raw_line.starts_with("    "), "Expected 4-space indent, got: {:?}", raw.raw_line);
    }

    #[test]
    fn set_askpass_on_nonexistent_host() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        config.set_host_askpass("nohost", "keychain");
        assert_eq!(first_block(&config).askpass(), None);
    }

    #[test]
    fn to_entry_includes_askpass() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass bw:item\n");
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].askpass, Some("bw:item".to_string()));
    }

    #[test]
    fn to_entry_askpass_none_when_absent() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].askpass, None);
    }

    #[test]
    fn set_askpass_vault_with_hash_field() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        config.set_host_askpass("myserver", "vault:secret/data/team#api_key");
        assert_eq!(first_block(&config).askpass(), Some("vault:secret/data/team#api_key".to_string()));
    }

    #[test]
    fn set_askpass_custom_command_with_percent() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        config.set_host_askpass("myserver", "get-pass %a %h");
        assert_eq!(first_block(&config).askpass(), Some("get-pass %a %h".to_string()));
    }

    #[test]
    fn multiple_hosts_independent_askpass() {
        let mut config = parse_str("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        config.set_host_askpass("alpha", "keychain");
        config.set_host_askpass("beta", "vault:secret/ssh");
        assert_eq!(block_by_index(&config, 0).askpass(), Some("keychain".to_string()));
        assert_eq!(block_by_index(&config, 1).askpass(), Some("vault:secret/ssh".to_string()));
    }

    #[test]
    fn set_askpass_then_clear_then_set_again() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        config.set_host_askpass("myserver", "keychain");
        assert_eq!(first_block(&config).askpass(), Some("keychain".to_string()));
        config.set_host_askpass("myserver", "");
        assert_eq!(first_block(&config).askpass(), None);
        config.set_host_askpass("myserver", "op://V/I/p");
        assert_eq!(first_block(&config).askpass(), Some("op://V/I/p".to_string()));
    }

    #[test]
    fn askpass_tab_indent_preserved() {
        let mut config = parse_str("Host myserver\n\tHostName 10.0.0.1\n");
        config.set_host_askpass("myserver", "pass:ssh/prod");
        let raw = first_block(&config).directives.iter()
            .find(|d| d.raw_line.contains("purple:askpass"))
            .unwrap();
        assert!(raw.raw_line.starts_with("\t"), "Expected tab indent, got: {:?}", raw.raw_line);
    }

    #[test]
    fn askpass_coexists_with_provider_comment() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:provider do:123\n  # purple:askpass keychain\n");
        let block = first_block(&config);
        assert_eq!(block.askpass(), Some("keychain".to_string()));
        assert!(block.provider().is_some());
    }

    #[test]
    fn set_askpass_does_not_remove_tags() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:tags prod,staging\n");
        config.set_host_askpass("myserver", "keychain");
        let entry = first_block(&config).to_host_entry();
        assert_eq!(entry.askpass, Some("keychain".to_string()));
        assert!(entry.tags.contains(&"prod".to_string()));
        assert!(entry.tags.contains(&"staging".to_string()));
    }

    #[test]
    fn askpass_idempotent_set_same_value() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        config.set_host_askpass("myserver", "keychain");
        assert_eq!(first_block(&config).askpass(), Some("keychain".to_string()));
        let serialized = config.serialize();
        assert_eq!(serialized.matches("purple:askpass").count(), 1, "Should have exactly one askpass comment");
    }

    #[test]
    fn askpass_with_value_containing_equals() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        config.set_host_askpass("myserver", "cmd --opt=val %h");
        assert_eq!(first_block(&config).askpass(), Some("cmd --opt=val %h".to_string()));
    }

    #[test]
    fn askpass_with_value_containing_hash() {
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:a/b#c\n");
        assert_eq!(first_block(&config).askpass(), Some("vault:a/b#c".to_string()));
    }

    #[test]
    fn askpass_with_long_op_uri() {
        let uri = "op://My Personal Vault/SSH Production Server/password";
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        config.set_host_askpass("myserver", uri);
        assert_eq!(first_block(&config).askpass(), Some(uri.to_string()));
    }

    #[test]
    fn askpass_does_not_interfere_with_host_matching() {
        // askpass is stored as a non-directive comment; it shouldn't affect SSH matching
        let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  User root\n  # purple:askpass keychain\n");
        let entry = first_block(&config).to_host_entry();
        assert_eq!(entry.user, "root");
        assert_eq!(entry.hostname, "10.0.0.1");
        assert_eq!(entry.askpass, Some("keychain".to_string()));
    }

    #[test]
    fn set_askpass_on_host_with_many_directives() {
        let config_str = "\
Host myserver
  HostName 10.0.0.1
  User admin
  Port 2222
  IdentityFile ~/.ssh/id_ed25519
  ProxyJump bastion
  # purple:tags prod,us-east
";
        let mut config = parse_str(config_str);
        config.set_host_askpass("myserver", "pass:ssh/prod");
        let entry = first_block(&config).to_host_entry();
        assert_eq!(entry.askpass, Some("pass:ssh/prod".to_string()));
        assert_eq!(entry.user, "admin");
        assert_eq!(entry.port, 2222);
        assert!(entry.tags.contains(&"prod".to_string()));
    }

    #[test]
    fn askpass_with_crlf_line_endings() {
        let config = parse_str("Host myserver\r\n  HostName 10.0.0.1\r\n  # purple:askpass keychain\r\n");
        assert_eq!(first_block(&config).askpass(), Some("keychain".to_string()));
    }

    #[test]
    fn askpass_only_on_first_matching_host() {
        // If two Host blocks have the same alias (unusual), askpass comes from first
        let config = parse_str("Host dup\n  HostName a.com\n  # purple:askpass keychain\n\nHost dup\n  HostName b.com\n  # purple:askpass vault:x\n");
        let entries = config.host_entries();
        // First match
        assert_eq!(entries[0].askpass, Some("keychain".to_string()));
    }

    #[test]
    fn set_askpass_preserves_other_non_directive_comments() {
        let config_str = "Host myserver\n  HostName 10.0.0.1\n  # This is a user comment\n  # purple:askpass old\n  # Another comment\n";
        let mut config = parse_str(config_str);
        config.set_host_askpass("myserver", "new-source");
        let serialized = config.serialize();
        assert!(serialized.contains("# This is a user comment"));
        assert!(serialized.contains("# Another comment"));
        assert!(serialized.contains("# purple:askpass new-source"));
        assert!(!serialized.contains("# purple:askpass old"));
    }

    #[test]
    fn askpass_mixed_with_tunnel_directives() {
        let config_str = "\
Host myserver
  HostName 10.0.0.1
  LocalForward 8080 localhost:80
  # purple:askpass bw:item
  RemoteForward 9090 localhost:9090
";
        let config = parse_str(config_str);
        let entry = first_block(&config).to_host_entry();
        assert_eq!(entry.askpass, Some("bw:item".to_string()));
        assert_eq!(entry.tunnel_count, 2);
    }

    // =========================================================================
    // askpass: set_askpass idempotent (same value)
    // =========================================================================

    #[test]
    fn set_askpass_idempotent_same_value() {
        let config_str = "Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n";
        let mut config = parse_str(config_str);
        config.set_host_askpass("myserver", "keychain");
        let output = config.serialize();
        // Should still have exactly one askpass comment
        assert_eq!(output.matches("purple:askpass").count(), 1);
        assert!(output.contains("# purple:askpass keychain"));
    }

    #[test]
    fn set_askpass_with_equals_in_value() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        config.set_host_askpass("myserver", "cmd --opt=val");
        let entries = config.host_entries();
        assert_eq!(entries[0].askpass, Some("cmd --opt=val".to_string()));
    }

    #[test]
    fn set_askpass_with_hash_in_value() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        config.set_host_askpass("myserver", "vault:secret/data#field");
        let entries = config.host_entries();
        assert_eq!(entries[0].askpass, Some("vault:secret/data#field".to_string()));
    }

    #[test]
    fn set_askpass_long_op_uri() {
        let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
        let long_uri = "op://My Personal Vault/SSH Production Server Key/password";
        config.set_host_askpass("myserver", long_uri);
        assert_eq!(config.host_entries()[0].askpass, Some(long_uri.to_string()));
    }

    #[test]
    fn askpass_host_with_multi_pattern_is_skipped() {
        // Multi-pattern host blocks ("Host prod staging") are treated as patterns
        // and are not included in host_entries(), so set_askpass is a no-op
        let config_str = "Host prod staging\n  HostName 10.0.0.1\n";
        let mut config = parse_str(config_str);
        config.set_host_askpass("prod", "keychain");
        // No entries because multi-pattern hosts are pattern hosts
        assert!(config.host_entries().is_empty());
    }

    #[test]
    fn askpass_survives_directive_reorder() {
        // askpass should survive even when directives are in unusual order
        let config_str = "\
Host myserver
  # purple:askpass op://V/I/p
  HostName 10.0.0.1
  User root
";
        let config = parse_str(config_str);
        let entry = first_block(&config).to_host_entry();
        assert_eq!(entry.askpass, Some("op://V/I/p".to_string()));
        assert_eq!(entry.hostname, "10.0.0.1");
    }

    #[test]
    fn askpass_among_many_purple_comments() {
        let config_str = "\
Host myserver
  HostName 10.0.0.1
  # purple:tags prod,us-east
  # purple:provider do:12345
  # purple:askpass pass:ssh/prod
";
        let config = parse_str(config_str);
        let entry = first_block(&config).to_host_entry();
        assert_eq!(entry.askpass, Some("pass:ssh/prod".to_string()));
        assert!(entry.tags.contains(&"prod".to_string()));
    }
}
