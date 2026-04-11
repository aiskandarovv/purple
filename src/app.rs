use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use log::{debug, error};

use ratatui::widgets::ListState;

use crate::history::ConnectionHistory;
use crate::providers::config::ProviderConfig;
use crate::ssh_config::model::{ConfigElement, HostEntry, PatternEntry, SshConfigFile};
use crate::ssh_keys::{self, SshKeyInfo};
use crate::tunnel::TunnelRule;

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

mod forms;
mod types;

pub(crate) use forms::char_to_byte_pos;
pub use forms::{
    FormField, HostForm, ProviderFormField, ProviderFormFields, SnippetForm, SnippetFormField,
    SnippetHostOutput, SnippetOutputState, SnippetParamFormState, TunnelForm, TunnelFormField,
};
pub use types::{
    ConflictState, ContainerState, DeletedHost, FormBaseline, GroupBy, HostListItem, PingStatus,
    ProviderFormBaseline, ReloadState, Screen, SearchState, SnippetFormBaseline, SortMode,
    StatusMessage, SyncRecord, TunnelFormBaseline, UiSelection, ViewMode, classify_ping,
    health_summary_spans, health_summary_spans_for, ping_sort_key, propagate_ping_to_dependents,
    select_display_tags, status_glyph,
};

/// Kill active tunnel processes when App is dropped (e.g. on panic).
impl Drop for App {
    fn drop(&mut self) {
        for (_, mut tunnel) in self.active_tunnels.drain() {
            let _ = tunnel.child.kill();
            let _ = tunnel.child.wait();
        }
        // Cancel and join any in-flight Vault SSH bulk-sign worker so it
        // cannot keep writing to ~/.purple/certs/ after teardown (panic
        // unwind, normal exit, etc.).
        if let Some(ref cancel) = self.vault_signing_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(handle) = self.vault_sign_thread.take() {
            let _ = handle.join();
        }
    }
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

    /// Signal for animation layer: detail panel toggle requested.
    /// Set by handler, consumed by AnimationState.detect_transitions().
    pub detail_toggle_pending: bool,

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
    pub slow_threshold_ms: u16,
    pub auto_ping: bool,
    /// When true, only show hosts with PingStatus::Unreachable.
    pub filter_down_only: bool,
    /// Timestamp of last ping completion (for TTL display). None if no pings done.
    pub ping_checked_at: Option<std::time::Instant>,

    /// Cached vault certificate status per host alias.
    /// Tuple: (check timestamp, status, cert file mtime at check time).
    /// The mtime is used to detect external changes (e.g. another purple
    /// instance or the CLI signing a cert) so the detail panel refreshes
    /// within one frame instead of waiting for the TTL.
    pub cert_status_cache: HashMap<
        String,
        (
            std::time::Instant,
            crate::vault_ssh::CertStatus,
            Option<std::time::SystemTime>,
        ),
    >,
    /// Aliases currently being checked for cert status (prevent duplicate checks).
    pub cert_check_in_flight: HashSet<String>,

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

    /// Demo mode: all mutations are in-memory only, no disk writes.
    pub demo_mode: bool,

    // Form dirty-check baselines
    pub form_baseline: Option<FormBaseline>,
    pub tunnel_form_baseline: Option<TunnelFormBaseline>,
    pub snippet_form_baseline: Option<SnippetFormBaseline>,
    pub provider_form_baseline: Option<ProviderFormBaseline>,
    /// When true, the Esc key shows a "Discard changes?" dialog instead of closing.
    pub pending_discard_confirm: bool,

    /// Deferred config write from VaultSignAllDone (guarded while forms are open).
    pub pending_vault_config_write: bool,

    /// Side-channel warning from cert-cache cleanup. Set by mutators that
    /// cannot themselves call `set_status` because they return a Result the
    /// caller turns into a status. The handler caller drains this and
    /// overrides the success message when present.
    pub cert_cleanup_warning: Option<String>,

    /// Cancel flag for the V-key vault signing background thread.
    pub vault_signing_cancel: Option<Arc<AtomicBool>>,

    /// JoinHandle for the V-key vault signing background thread (for clean exit).
    pub vault_sign_thread: Option<std::thread::JoinHandle<()>>,

    /// Aliases currently being signed by the bulk V-key loop. Shared with the
    /// background thread so the main-thread cert-check spawner can skip any
    /// host that is mid-signing (TOCTOU guard).
    pub vault_sign_in_flight: Arc<std::sync::Mutex<HashSet<String>>>,
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
                theme_picker_state: ListState::default(),
                theme_picker_builtins: Vec::new(),
                theme_picker_custom: Vec::new(),
                theme_picker_saved_name: String::new(),
                theme_picker_original: None,
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
            detail_toggle_pending: false,
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
            slow_threshold_ms: crate::preferences::load_slow_threshold(),
            auto_ping: crate::preferences::load_auto_ping(),
            filter_down_only: false,
            ping_checked_at: None,
            cert_status_cache: HashMap::new(),
            cert_check_in_flight: HashSet::new(),
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
            demo_mode: false,
            form_baseline: None,
            tunnel_form_baseline: None,
            snippet_form_baseline: None,
            provider_form_baseline: None,
            pending_discard_confirm: false,
            pending_vault_config_write: false,
            cert_cleanup_warning: None,
            vault_signing_cancel: None,
            vault_sign_thread: None,
            vault_sign_in_flight: Arc::new(std::sync::Mutex::new(HashSet::new())),
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
                    indices.sort_by_cached_key(|&i| {
                        let stale = self.hosts[i].stale.is_some();
                        (stale, self.hosts[i].alias.to_ascii_lowercase())
                    });
                }
                SortMode::AlphaHostname => {
                    indices.sort_by_cached_key(|&i| {
                        let stale = self.hosts[i].stale.is_some();
                        (stale, self.hosts[i].hostname.to_ascii_lowercase())
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
                SortMode::Status => {
                    indices.sort_by(|a, b| {
                        let sa = self.hosts[*a].stale.is_some();
                        let sb = self.hosts[*b].stale.is_some();
                        sa.cmp(&sb).then_with(|| {
                            let pa = self.ping_status.get(&self.hosts[*a].alias);
                            let pb = self.ping_status.get(&self.hosts[*b].alias);
                            ping_sort_key(pa).cmp(&ping_sort_key(pb))
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
                    for tag in host.tags.iter() {
                        *tag_counts.entry(tag.clone()).or_insert(0) += 1;
                    }
                }
                for pattern in &self.patterns {
                    for tag in &pattern.tags {
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
                let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
                for item in &self.display_list {
                    if let HostListItem::GroupHeader(text) = item {
                        if seen.insert(text.as_str()) {
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
        // Synchronously flush any deferred vault config write before reloading,
        // so on-disk state matches in-memory state (no TOCTOU with auto-reload).
        // Skip when a form is open (flush handler would bail anyway) and do not
        // call flush_pending_vault_write() itself to avoid recursion.
        if self.pending_vault_config_write && !self.is_form_open() {
            if let Err(e) = self.config.write() {
                self.set_status(
                    format!("Failed to update config after vault signing: {}", e),
                    true,
                );
            }
        }
        // Always clear the flag: either we flushed, or the form-submit path
        // has already written the full config.
        self.pending_vault_config_write = false;
        let had_search = self.search.query.take();
        let selected_alias = self
            .selected_host()
            .map(|h| h.alias.clone())
            .or_else(|| self.selected_pattern().map(|p| p.pattern.clone()));

        self.tunnel_summaries_cache.clear();
        self.hosts = self.config.host_entries();
        self.patterns = self.config.pattern_entries();
        // Prune cert status cache and in-flight set: retain only entries whose
        // host alias still exists after the reload.
        let valid_for_certs: std::collections::HashSet<&str> =
            self.hosts.iter().map(|h| h.alias.as_str()).collect();
        self.cert_status_cache
            .retain(|alias, _| valid_for_certs.contains(alias.as_str()));
        self.cert_check_in_flight
            .retain(|alias| valid_for_certs.contains(alias.as_str()));
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

    /// Synchronously re-check a host's Vault SSH certificate and update
    /// `cert_status_cache` with fresh status + on-disk mtime.
    ///
    /// Every sign path (V-key bulk sign, host form submit, connect-time
    /// `ensure_vault_ssh_if_needed`, CLI) funnels through this helper so the
    /// detail panel never lies about cert state after a successful sign.
    ///
    /// No-op in demo mode. If the host is missing, has no resolvable vault
    /// role, or the cert path cannot be resolved, any stale entry for the
    /// alias is removed to avoid showing ghost status.
    pub fn refresh_cert_cache(&mut self, alias: &str) {
        if crate::demo_flag::is_demo() {
            return;
        }
        let Some(host) = self.hosts.iter().find(|h| h.alias == alias) else {
            self.cert_status_cache.remove(alias);
            return;
        };
        let role_some = crate::vault_ssh::resolve_vault_role(
            host.vault_ssh.as_deref(),
            host.provider.as_deref(),
            &self.provider_config,
        )
        .is_some();
        if !role_some {
            self.cert_status_cache.remove(alias);
            return;
        }
        let cert_path = match crate::vault_ssh::resolve_cert_path(alias, &host.certificate_file) {
            Ok(p) => p,
            Err(_) => {
                self.cert_status_cache.remove(alias);
                return;
            }
        };
        let status = crate::vault_ssh::check_cert_validity(&cert_path);
        let mtime = std::fs::metadata(&cert_path)
            .ok()
            .and_then(|m| m.modified().ok());
        self.cert_status_cache.insert(
            alias.to_string(),
            (std::time::Instant::now(), status, mtime),
        );
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
        self.filter_down_only = false;
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
                if !self.filter_down_only {
                    let total = self.search.filtered_indices.len()
                        + self.search.filtered_pattern_indices.len();
                    if total == 0 {
                        self.ui.list_state.select(None);
                    } else {
                        self.ui.list_state.select(Some(0));
                    }
                    return;
                }
                // Fall through to down-only filtering below
                String::new()
            }
            None => {
                if !self.filter_down_only {
                    return;
                }
                // No search query but down-only is active: start with all hosts
                self.search.filtered_indices = (0..self.hosts.len()).collect();
                self.search.filtered_pattern_indices = Vec::new();
                // Scope to group if active
                if let Some(ref scope) = self.search.scope_indices {
                    self.search.filtered_indices.retain(|i| scope.contains(i));
                }
                // Fall through to down-only filtering below
                String::new()
            }
        };

        if let Some(tag_exact) = query.strip_prefix("tag=") {
            // Exact tag match (from tag picker), includes provider name and virtual "stale"/"vault"
            let provider_config = &self.provider_config;
            self.search.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    (eq_ci("stale", tag_exact) && host.stale.is_some())
                        || (eq_ci("vault-ssh", tag_exact)
                            && crate::vault_ssh::resolve_vault_role(
                                host.vault_ssh.as_deref(),
                                host.provider.as_deref(),
                                provider_config,
                            )
                            .is_some())
                        || (eq_ci("vault-kv", tag_exact)
                            && host
                                .askpass
                                .as_deref()
                                .map(|s| s.starts_with("vault:"))
                                .unwrap_or(false))
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
            // Fuzzy tag match (manual search), includes provider name and virtual "stale"/"vault"
            let provider_config = &self.provider_config;
            self.search.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    (contains_ci("stale", tag_query) && host.stale.is_some())
                        || (contains_ci("vault-ssh", tag_query)
                            && crate::vault_ssh::resolve_vault_role(
                                host.vault_ssh.as_deref(),
                                host.provider.as_deref(),
                                provider_config,
                            )
                            .is_some())
                        || (contains_ci("vault-kv", tag_query)
                            && host
                                .askpass
                                .as_deref()
                                .map(|s| s.starts_with("vault:"))
                                .unwrap_or(false))
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
                .filter(|(_, p)| {
                    contains_ci(&p.pattern, &query) || p.tags.iter().any(|t| contains_ci(t, &query))
                })
                .map(|(i, _)| i)
                .collect();
        }

        // Scope results to the active group if set
        if let Some(ref scope) = self.search.scope_indices {
            self.search.filtered_indices.retain(|i| scope.contains(i));
        }

        // Post-filter: keep only unreachable hosts when down-only mode is active
        if self.filter_down_only {
            self.search.filtered_indices.retain(|&idx| {
                let alias = &self.hosts[idx].alias;
                matches!(self.ping_status.get(alias), Some(PingStatus::Unreachable))
            });
            // Patterns can't be pinged, so hide them in down-only mode
            self.search.filtered_pattern_indices.clear();
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

    /// Check whether a form screen is currently open (host or provider forms).
    pub fn is_form_open(&self) -> bool {
        matches!(
            self.screen,
            Screen::AddHost | Screen::EditHost { .. } | Screen::ProviderForm { .. }
        )
    }

    /// Flush a deferred vault config write if one is pending and no form is open.
    /// Returns true if a write was performed.
    pub fn flush_pending_vault_write(&mut self) -> bool {
        if !self.pending_vault_config_write || self.is_form_open() {
            return false;
        }
        // reload_hosts() performs the write and clears the flag.
        self.reload_hosts();
        true
    }

    pub fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status = Some(StatusMessage {
            text: text.into(),
            is_error,
            tick_count: 0,
            sticky: false,
        });
    }

    /// Like `set_status` but skips the write when a sticky message is active.
    /// Use for background/timer events (ping expiry, sync ticks) that must
    /// not clobber an in-progress or critical sticky message.
    pub fn set_background_status(&mut self, text: impl Into<String>, is_error: bool) {
        if self.status.as_ref().is_some_and(|s| s.sticky) {
            return;
        }
        self.set_status(text, is_error);
    }

    pub fn set_sticky_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status = Some(StatusMessage {
            text: text.into(),
            is_error,
            tick_count: 0,
            sticky: true,
        });
    }

    /// Tick the status message timer. Errors show for 5s, success for 3s.
    /// Sticky messages never expire automatically.
    pub fn tick_status(&mut self) {
        // Don't expire status while providers are still syncing
        if !self.syncing_providers.is_empty() {
            return;
        }
        if let Some(ref mut status) = self.status {
            if status.sticky {
                return;
            }
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
                | Screen::ConfirmVaultSign { .. }
                | Screen::TagPicker
                | Screen::ThemePicker
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
                self.filter_down_only = false;
                self.ping_checked_at = None;
                self.reload_hosts();
                self.reload.last_modified = current_mtime;
                self.reload.include_mtimes = Self::snapshot_include_mtimes(&self.config);
                self.reload.include_dir_mtimes = Self::snapshot_include_dir_mtimes(&self.config);
                let count = self.hosts.len();
                self.set_background_status(format!("Config reloaded. {} hosts.", count), false);
            }
        }
    }

    /// Non-mutating check: has the on-disk config (or any tracked Include)
    /// been modified since `self.reload.last_modified` was captured? Used by
    /// async write paths (e.g. the Vault SSH bulk-sign completion handler)
    /// to refuse writing when an external editor changed the file underneath
    /// us — overwriting those edits would silently discard user work. The
    /// backup-on-write mechanism in `SshConfigFile::write()` would still
    /// recover them, but detecting the conflict BEFORE writing is strictly
    /// better than after.
    pub fn external_config_changed(&self) -> bool {
        let current_mtime = Self::get_mtime(&self.reload.config_path);
        current_mtime != self.reload.last_modified
            || self
                .reload
                .include_mtimes
                .iter()
                .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
            || self
                .reload
                .include_dir_mtimes
                .iter()
                .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
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
            vault_ssh: self.form.vault_ssh.clone(),
            vault_addr: self.form.vault_addr.clone(),
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
                    || self.form.vault_ssh != b.vault_ssh
                    || self.form.vault_addr != b.vault_addr
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
            vault_role: self.provider_form.vault_role.clone(),
            vault_addr: self.provider_form.vault_addr.clone(),
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
                    || self.provider_form.vault_role != b.vault_role
                    || self.provider_form.vault_addr != b.vault_addr
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

    /// Returns true if any host or provider has a vault role configured.
    pub fn has_any_vault_role(&self) -> bool {
        for host in &self.hosts {
            if host.vault_ssh.is_some() {
                return true;
            }
        }
        for section in &self.provider_config.sections {
            if !section.vault_role.is_empty() {
                return true;
            }
        }
        false
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
        let mut has_vault_ssh = false;
        let mut has_vault_kv = false;
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
            if crate::vault_ssh::resolve_vault_role(
                host.vault_ssh.as_deref(),
                host.provider.as_deref(),
                &self.provider_config,
            )
            .is_some()
            {
                has_vault_ssh = true;
            }
            if host
                .askpass
                .as_deref()
                .map(|s| s.starts_with("vault:"))
                .unwrap_or(false)
            {
                has_vault_kv = true;
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
        if !has_vault_ssh {
            for section in &self.provider_config.sections {
                if !section.vault_role.is_empty() {
                    has_vault_ssh = true;
                    break;
                }
            }
        }
        if has_vault_ssh && seen.insert("vault-ssh".to_string()) {
            tags.push("vault-ssh".to_string());
        }
        if has_vault_kv && seen.insert("vault-kv".to_string()) {
            tags.push("vault-kv".to_string());
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
                    let exit_code = status.code().unwrap_or(-1);
                    if !status.success() {
                        error!(
                            "[external] Tunnel exited unexpectedly: alias={alias} exit={exit_code}"
                        );
                        if let Some(ref err) = stderr_msg {
                            debug!("[external] Tunnel stderr: {}", err.trim());
                        }
                    }
                    let (msg, is_error) = if status.success() {
                        (format!("Tunnel for {} closed.", alias), false)
                    } else if let Some(err) = stderr_msg {
                        (format!("Tunnel for {}: {}", alias, err), true)
                    } else {
                        (
                            format!("Tunnel for {} exited with code {}.", alias, exit_code),
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
        let duplicate = if self.form.is_pattern {
            self.config.has_host_block(&alias)
        } else {
            self.config.has_host(&alias)
        };
        if duplicate {
            return Err(if self.form.is_pattern {
                format!("Pattern '{}' already exists.", alias)
            } else {
                format!("'{}' already exists. Aliases must be unique.", alias)
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
        if let Some(ref role) = entry.vault_ssh {
            self.config.set_host_vault_ssh(&alias, role);
            // Persist the optional Vault address next to the role. `set_host_vault_addr`
            // is `#[must_use]` but the alias was just upserted above so we only
            // debug-assert the return value here (matches the CertificateFile pattern).
            let addr = entry.vault_addr.as_deref().unwrap_or("");
            let addr_wired = self.config.set_host_vault_addr(&alias, addr);
            debug_assert!(
                addr_wired,
                "add_host_from_form: alias '{}' missing immediately after upsert (set_host_vault_addr)",
                alias
            );
            // For a brand-new host the only existing CertificateFile value can
            // come from the form itself (a power user pasting one in). Honor
            // the same invariant as edit_host_from_form: never overwrite a
            // user-set custom path.
            if crate::should_write_certificate_file(&entry.certificate_file) {
                let cert_path = crate::vault_ssh::cert_path_for(&alias)
                    .map_err(|e| format!("Failed to resolve cert path: {}", e))?;
                // The host block was just upserted above, so the alias MUST
                // exist. Assert the invariant to catch regressions early.
                let wired = self
                    .config
                    .set_host_certificate_file(&alias, &cert_path.to_string_lossy());
                debug_assert!(
                    wired,
                    "add_host_from_form: alias '{}' missing immediately after upsert",
                    alias
                );
            }
        }
        if let Err(e) = self.config.write() {
            self.config.elements.truncate(len_before);
            return Err(format!("Failed to save: {}", e));
        }
        // Form submit writes the full config including any pending vault mutations
        self.pending_vault_config_write = false;
        self.update_last_modified();
        self.reload_hosts();
        self.select_host_by_alias(&alias);
        // Refresh the cert cache so the detail panel reflects reality
        // immediately. No-op when the new host has no vault role or when
        // running in demo mode.
        self.refresh_cert_cache(&alias);
        Ok(format!("Welcome aboard, {}!", alias))
    }

    /// Edit an existing host from the current form. Returns status message.
    pub fn edit_host_from_form(&mut self, old_alias: &str) -> Result<String, String> {
        let entry = self.form.to_entry();
        let alias = entry.alias.clone();
        let exists = if self.form.is_pattern {
            self.config.has_host_block(old_alias)
        } else {
            self.config.has_host(old_alias)
        };
        if !exists {
            return Err(if self.form.is_pattern {
                "Pattern no longer exists.".to_string()
            } else {
                "Host no longer exists.".to_string()
            });
        }
        let duplicate = if self.form.is_pattern {
            alias != old_alias && self.config.has_host_block(&alias)
        } else {
            alias != old_alias && self.config.has_host(&alias)
        };
        if duplicate {
            return Err(if self.form.is_pattern {
                format!("Pattern '{}' already exists.", alias)
            } else {
                format!("'{}' already exists. Aliases must be unique.", alias)
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
        self.config
            .set_host_vault_ssh(&entry.alias, entry.vault_ssh.as_deref().unwrap_or(""));
        // Persist vault address comment. `set_host_vault_addr` refuses
        // wildcard aliases (mirroring the CertificateFile invariant), so we
        // skip it entirely for Host pattern entries — patterns never carry a
        // vault address. For concrete hosts the alias was just upserted so
        // the #[must_use] return is asserted in debug builds.
        if !self.form.is_pattern {
            let addr_wired = self
                .config
                .set_host_vault_addr(&entry.alias, entry.vault_addr.as_deref().unwrap_or(""));
            debug_assert!(
                addr_wired,
                "edit_host_from_form: alias '{}' missing immediately after update_host (set_host_vault_addr)",
                entry.alias
            );
        }
        // HostForm does not track CertificateFile, so the source of truth for
        // the host's existing CertificateFile is `old_entry` (loaded from
        // disk), not `entry` (rebuilt from the form, which always has it
        // empty). Both branches below honor that distinction so a user-set
        // custom CertificateFile is preserved across an edit.
        if entry.vault_ssh.is_some() {
            if crate::should_write_certificate_file(&old_entry.certificate_file) {
                let cert_path = crate::vault_ssh::cert_path_for(&entry.alias)
                    .map_err(|e| format!("Failed to resolve cert path: {}", e))?;
                // Synchronous mutation: the host block was just updated, so
                // the alias MUST exist. Assert the invariant.
                let wired = self
                    .config
                    .set_host_certificate_file(&entry.alias, &cert_path.to_string_lossy());
                debug_assert!(
                    wired,
                    "edit_host_from_form: alias '{}' missing immediately after update_host",
                    entry.alias
                );
            }
        } else {
            // Vault SSH role removed: clear the CertificateFile only if it
            // points at purple's managed cert path. A user-set custom path is
            // left alone. Compare the expanded form on both sides so a
            // tilde-relative directive (`~/.purple/certs/...`) and the
            // absolute path produced by `cert_path_for` match.
            let purple_managed = crate::vault_ssh::cert_path_for(&entry.alias).ok();
            let existing_resolved = if old_entry.certificate_file.is_empty() {
                None
            } else {
                crate::vault_ssh::resolve_cert_path(&entry.alias, &old_entry.certificate_file).ok()
            };
            if purple_managed.is_some() && purple_managed == existing_resolved {
                let _ = self.config.set_host_certificate_file(&entry.alias, "");
            }
        }
        if let Err(e) = self.config.write() {
            self.config.update_host(&entry.alias, &old_entry);
            self.config.set_host_tags(&old_entry.alias, &old_entry.tags);
            self.config
                .set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
            self.config.set_host_vault_ssh(
                &old_entry.alias,
                old_entry.vault_ssh.as_deref().unwrap_or(""),
            );
            if !self.form.is_pattern {
                let _ = self.config.set_host_vault_addr(
                    &old_entry.alias,
                    old_entry.vault_addr.as_deref().unwrap_or(""),
                );
            }
            if old_entry.vault_ssh.is_some() {
                // Rollback restores the old host's actual CertificateFile
                // value (which may be a user-set custom path), not purple's
                // default. Falling back to the default would silently rewrite
                // the directive on a write failure.
                let _ = self
                    .config
                    .set_host_certificate_file(&old_entry.alias, &old_entry.certificate_file);
            } else {
                let _ = self.config.set_host_certificate_file(&old_entry.alias, "");
            }
            return Err(format!("Failed to save: {}", e));
        }
        // Form submit writes the full config including any pending vault mutations
        self.pending_vault_config_write = false;
        // Migrate active tunnel handle if alias changed
        if alias != old_alias {
            if let Some(tunnel) = self.active_tunnels.remove(old_alias) {
                self.active_tunnels.insert(alias.clone(), tunnel);
            }
            // Clean up old cert file on rename. Best-effort: a missing file is
            // fine (NotFound is expected when no cert was ever signed) but any
            // other error is surfaced via the status bar (never via eprintln,
            // which would corrupt the ratatui screen in raw mode).
            if !crate::demo_flag::is_demo() {
                if let Ok(old_cert) = crate::vault_ssh::cert_path_for(old_alias) {
                    match std::fs::remove_file(&old_cert) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => {
                            self.cert_cleanup_warning = Some(format!(
                                "Warning: failed to clean up old Vault SSH cert {}: {}",
                                old_cert.display(),
                                e
                            ));
                        }
                    }
                }
            }
        }
        self.update_last_modified();
        self.reload_hosts();
        // Refresh the cert cache so the detail panel reflects reality
        // immediately after an edit (e.g. a newly set vault role, a custom
        // CertificateFile path change, or role removal). When the alias
        // itself changed, also clear the stale entry under the old alias.
        if alias != old_alias {
            self.cert_status_cache.remove(old_alias);
        }
        self.refresh_cert_cache(&alias);
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
            let tag_exists = self.hosts.iter().any(|h| h.tags.iter().any(|t| t == tag))
                || self
                    .patterns
                    .iter()
                    .any(|p| p.tags.iter().any(|t| t == tag));
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

        // In tag mode, match the selected host's tags against the tab order
        // directly, because the display list only has one GroupHeader (the
        // active GroupBy tag) while the tab bar shows the top-10 tags.
        if matches!(self.group_by, GroupBy::Tag(_)) {
            let tags: Option<&[String]> = match self.display_list.get(selected) {
                Some(HostListItem::Host { index }) => {
                    self.hosts.get(*index).map(|h| h.tags.as_slice())
                }
                Some(HostListItem::Pattern { index }) => {
                    self.patterns.get(*index).map(|p| p.tags.as_slice())
                }
                _ => None,
            };
            if let Some(item_tags) = tags {
                for (idx, tab_tag) in self.group_tab_order.iter().enumerate() {
                    if item_tags.iter().any(|t| t == tab_tag) {
                        self.group_tab_index = idx + 1;
                        return;
                    }
                }
            }
            self.group_tab_index = 0;
            return;
        }

        // Provider/none mode: walk backwards to find the nearest GroupHeader
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
mod tests;
