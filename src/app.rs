use std::collections::HashMap;
use std::path::PathBuf;

use ratatui::widgets::ListState;

use crate::history::ConnectionHistory;
use crate::ssh_config::model::SshConfigFile;
use crate::ssh_keys::SshKeyInfo;

/// Case-insensitive substring check without allocation.
/// Uses a byte-window approach for ASCII strings (the common case for SSH
/// hostnames and aliases). Falls back to a char-based scan when either
/// string contains non-ASCII bytes to avoid false matches across UTF-8
/// character boundaries.
pub(super) fn contains_ci(haystack: &str, needle: &str) -> bool {
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
pub(super) fn eq_ci(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

mod baselines;
mod container_state;
mod display_list;
mod form_state;
mod forms;
mod groups;
mod host_state;
mod hosts;
mod ping;
mod provider_state;
mod reload_state;
mod screen;
mod search;
mod selection;
mod snippet_state;
mod status_state;
mod tag_state;
mod tunnel_state;
mod ui_state;
mod update;
mod vault;

pub use baselines::{FormBaseline, ProviderFormBaseline, SnippetFormBaseline, TunnelFormBaseline};
pub use container_state::ContainerState;
pub use form_state::FormState;
pub(crate) use forms::char_to_byte_pos;
pub use forms::{
    FormField, HostForm, ProviderFormField, ProviderFormFields, SnippetForm, SnippetFormField,
    SnippetHostOutput, SnippetOutputState, SnippetParamFormState, TunnelForm, TunnelFormField,
};
pub use host_state::{
    DeletedHost, GroupBy, HostListItem, HostState, ProxyJumpCandidate, SortMode, ViewMode,
    health_summary_spans, health_summary_spans_for,
};
pub use ping::{
    PingState, PingStatus, classify_ping, ping_sort_key, propagate_ping_to_dependents, status_glyph,
};
pub use provider_state::{ProviderState, SyncRecord};
pub use reload_state::{ConflictState, ReloadState};
pub use screen::{Screen, WhatsNewState};
pub use search::SearchState;
pub use snippet_state::SnippetState;
pub use status_state::{MessageClass, StatusCenter, StatusMessage};
pub use tag_state::{
    BulkTagAction, BulkTagApplyResult, BulkTagEditorState, BulkTagRow, TagState,
    select_display_tags,
};
pub use tunnel_state::TunnelState;
pub use ui_state::UiSelection;
pub use update::UpdateState;
pub use vault::VaultState;

/// Kill active tunnel processes when App is dropped (e.g. on panic).
impl Drop for App {
    fn drop(&mut self) {
        for (alias, mut tunnel) in self.tunnels.active.drain() {
            if let Err(e) = tunnel.child.kill() {
                log::debug!("[external] Failed to kill tunnel for {alias} on shutdown: {e}");
            }
            let _ = tunnel.child.wait();
        }
        // Cancel and join any in-flight Vault SSH bulk-sign worker so it
        // cannot keep writing to ~/.purple/certs/ after teardown (panic
        // unwind, normal exit, etc.).
        if let Some(ref cancel) = self.vault.signing_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(handle) = self.vault.sign_thread.take() {
            let _ = handle.join();
        }
    }
}

/// Main application state.
pub struct App {
    // Core
    pub screen: Screen,
    pub running: bool,
    pub hosts_state: HostState,
    pub pending_connect: Option<(String, Option<String>)>,

    // Sub-structs
    pub status_center: StatusCenter,
    pub ui: UiSelection,
    pub search: SearchState,
    pub reload: ReloadState,
    pub conflict: ConflictState,

    // Keys
    pub keys: Vec<SshKeyInfo>,

    // Tags
    pub tags: TagState,

    // Host form + bulk tag editor
    pub forms: FormState,

    // History + preferences
    pub history: ConnectionHistory,

    /// Signal for animation layer: detail panel toggle requested.
    /// Set by handler, consumed by AnimationState.detect_transitions().
    pub detail_toggle_pending: bool,

    // Providers
    pub providers: ProviderState,

    // Ping / health-check
    pub ping: PingState,

    // Vault SSH certificate and signing state
    pub vault: VaultState,

    // Tunnels
    pub tunnels: TunnelState,

    // Snippets
    pub snippets: SnippetState,

    // Update
    pub update: UpdateState,

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

    /// Deferred config write from VaultSignAllDone (guarded while forms are open).
    pub pending_vault_config_write: bool,

    /// Command palette state. Some when palette is open.
    pub palette: Option<CommandPaletteState>,
}

impl App {
    pub fn new(config: SshConfigFile) -> Self {
        let hosts = config.host_entries();
        let patterns = config.pattern_entries();
        let display_list = Self::build_display_list_from(&config, &hosts, &patterns);

        let initial_selection = display_list.iter().position(|item| {
            matches!(
                item,
                HostListItem::Host { .. } | HostListItem::Pattern { .. }
            )
        });

        let reload = ReloadState::from_config(&config);
        let hosts_state = HostState::from_config(config, hosts, patterns, display_list);

        Self {
            screen: Screen::HostList,
            running: true,
            hosts_state,
            pending_connect: None,
            status_center: StatusCenter::default(),
            ui: UiSelection::new_with_initial_selection(initial_selection),
            search: SearchState::default(),
            reload,
            conflict: ConflictState::default(),
            keys: Vec::new(),
            tags: TagState::default(),
            forms: FormState::default(),
            history: ConnectionHistory::load(),
            detail_toggle_pending: false,
            providers: ProviderState::load(),
            ping: PingState::from_preferences(),
            vault: VaultState::default(),
            tunnels: TunnelState::default(),
            snippets: SnippetState::with_store_loaded(),
            update: UpdateState::with_current_hint(),
            bw_session: None,
            file_browser: None,
            file_browser_paths: HashMap::new(),
            container_state: None,
            container_cache: crate::containers::load_container_cache(),
            known_hosts_count: 0,
            welcome_opened: None,
            demo_mode: false,
            pending_vault_config_write: false,
            palette: None,
        }
    }

    /// Reload hosts from config.
    pub fn reload_hosts(&mut self) {
        let had_pending_vault_write = self.pending_vault_config_write;
        // Synchronously flush any deferred vault config write before reloading,
        // so on-disk state matches in-memory state (no TOCTOU with auto-reload).
        // Skip when a form is open (flush handler would bail anyway) and do not
        // call flush_pending_vault_write() itself to avoid recursion.
        let mut flushed_vault_write = false;
        if self.pending_vault_config_write && !self.is_form_open() {
            match self.hosts_state.ssh_config.write() {
                Ok(()) => flushed_vault_write = true,
                Err(e) => self.notify_error(crate::messages::vault_config_write_after_sign(&e)),
            }
        }
        // Always clear the flag: either we flushed, or the form-submit path
        // has already written the full config.
        self.pending_vault_config_write = false;
        log::debug!(
            "[config] reload_hosts: pending_vault_write={had_pending_vault_write} flushed={flushed_vault_write}"
        );
        let had_search = self.search.query.take();
        let selected_alias = self
            .selected_host()
            .map(|h| h.alias.clone())
            .or_else(|| self.selected_pattern().map(|p| p.pattern.clone()));

        self.tunnels.summaries_cache.clear();
        self.hosts_state.render_cache.invalidate();
        self.hosts_state.list = self.hosts_state.ssh_config.host_entries();
        self.hosts_state.patterns = self.hosts_state.ssh_config.pattern_entries();
        // Prune cert status cache and in-flight set: retain only entries whose
        // host alias still exists after the reload.
        let valid_for_certs: std::collections::HashSet<&str> = self
            .hosts_state
            .list
            .iter()
            .map(|h| h.alias.as_str())
            .collect();
        self.vault
            .cert_cache
            .retain(|alias, _| valid_for_certs.contains(alias.as_str()));
        self.vault
            .cert_checks_in_flight
            .retain(|alias| valid_for_certs.contains(alias.as_str()));
        if self.hosts_state.sort_mode == SortMode::Original
            && matches!(self.hosts_state.group_by, GroupBy::None)
        {
            self.hosts_state.display_list = Self::build_display_list_from(
                &self.hosts_state.ssh_config,
                &self.hosts_state.list,
                &self.hosts_state.patterns,
            );
        } else {
            self.apply_sort();
        }

        // Close tag pickers if open — tags.list is stale after reload
        if matches!(self.screen, Screen::TagPicker | Screen::BulkTagEditor) {
            self.screen = Screen::HostList;
            self.forms.bulk_tag_editor = BulkTagEditorState::default();
        }

        // Multi-select stores indices into hosts; clear to avoid stale refs
        self.hosts_state.multi_select.clear();

        // Prune ping status for hosts that no longer exist
        let valid_aliases: std::collections::HashSet<&str> = self
            .hosts_state
            .list
            .iter()
            .map(|h| h.alias.as_str())
            .collect();
        self.ping
            .status
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
            if self.hosts_state.list.is_empty() && self.hosts_state.patterns.is_empty() {
                self.ui.list_state.select(None);
            } else if let Some(pos) = self.hosts_state.display_list.iter().position(|item| {
                matches!(
                    item,
                    HostListItem::Host { .. } | HostListItem::Pattern { .. }
                )
            }) {
                let current = self.ui.list_state.selected().unwrap_or(0);
                if current >= self.hosts_state.display_list.len()
                    || !matches!(
                        self.hosts_state.display_list.get(current),
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

        log::debug!(
            "[config] reload_hosts: hosts={} patterns={} display_items={}",
            self.hosts_state.list.len(),
            self.hosts_state.patterns.len(),
            self.hosts_state.display_list.len(),
        );
    }

    /// Synchronously re-check a host's Vault SSH certificate and update
    /// `vault.cert_cache` with fresh status + on-disk mtime.
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
        let Some(host) = self.hosts_state.list.iter().find(|h| h.alias == alias) else {
            self.vault.cert_cache.remove(alias);
            return;
        };
        let role_some = crate::vault_ssh::resolve_vault_role(
            host.vault_ssh.as_deref(),
            host.provider.as_deref(),
            &self.providers.config,
        )
        .is_some();
        if !role_some {
            self.vault.cert_cache.remove(alias);
            return;
        }
        let cert_path = match crate::vault_ssh::resolve_cert_path(alias, &host.certificate_file) {
            Ok(p) => p,
            Err(_) => {
                self.vault.cert_cache.remove(alias);
                return;
            }
        };
        let status = crate::vault_ssh::check_cert_validity(&cert_path);
        let mtime = std::fs::metadata(&cert_path)
            .ok()
            .and_then(|m| m.modified().ok());
        self.vault.cert_cache.insert(
            alias.to_string(),
            (std::time::Instant::now(), status, mtime),
        );
    }

    // --- Search methods ---

    /// Shim. Routes to `ProviderState::sorted_names`.
    pub fn sorted_provider_names(&self) -> Vec<String> {
        self.providers.sorted_names()
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

    /// Shim. Routes to `StatusCenter::set_status`. 174+ call-sites via
    /// `notify_*` wrappers depend on this signature.
    #[deprecated(note = "use notify() / notify_error() instead")]
    #[allow(deprecated)]
    pub fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status_center.set_status(text, is_error);
    }

    /// Run once after App::new: queue the upgrade toast if the user just
    /// upgraded past their last-seen version, otherwise seed the preference
    /// so the next launch is silent.
    pub fn post_init(&mut self) {
        let outcome = crate::onboarding::evaluate();
        if let Some(text) = outcome.upgrade_toast {
            self.enqueue_sticky_toast(text);
        }
    }

    fn enqueue_sticky_toast(&mut self, text: String) {
        log::debug!("[purple] enqueue sticky toast: {}", text);
        let msg = StatusMessage {
            text,
            class: MessageClass::Success,
            tick_count: 0,
            sticky: true,
            created_at: std::time::Instant::now(),
        };
        self.status_center.toast = Some(msg);
    }

    /// Shim. Routes to `StatusCenter::set_info_status`.
    #[deprecated(note = "use notify_info() instead")]
    #[allow(deprecated)]
    pub fn set_info_status(&mut self, text: impl Into<String>) {
        self.status_center.set_info_status(text);
    }

    /// Shim. Routes to `StatusCenter::set_background_status`.
    #[deprecated(note = "use notify_background() / notify_background_error() instead")]
    #[allow(deprecated)]
    pub fn set_background_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status_center.set_background_status(text, is_error);
    }

    /// Shim. Routes to `StatusCenter::set_sticky_status`.
    #[deprecated(note = "use notify_progress() / notify_sticky_error() instead")]
    #[allow(deprecated)]
    pub fn set_sticky_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status_center.set_sticky_status(text, is_error);
    }

    /// User action feedback → Success toast (length-proportional timeout,
    /// last-write-wins). For: copy, sort, delete, save, demo mode messages.
    #[allow(deprecated)]
    pub fn notify(&mut self, text: impl Into<String>) {
        self.set_status(text, false);
    }

    /// User action error → Error toast (sticky by default, queued).
    /// Errors require user acknowledgement; they do not auto-expire.
    #[allow(deprecated)]
    pub fn notify_error(&mut self, text: impl Into<String>) {
        self.set_status(text, true);
    }

    /// Background event → Info footer (length-proportional timeout,
    /// suppressed if sticky active). For: ping expiry, sync progress,
    /// tunnel exit.
    #[allow(deprecated)]
    pub fn notify_background(&mut self, text: impl Into<String>) {
        self.set_background_status(text, false);
    }

    /// Background error → Error toast (sticky, queued, bypasses sticky
    /// suppression). Same semantics as `notify_error` but for events that
    /// arise from background workers rather than direct user actions.
    #[allow(deprecated)]
    pub fn notify_background_error(&mut self, text: impl Into<String>) {
        self.set_background_status(text, true);
    }

    /// Caution / degraded state → Warning toast (length-proportional
    /// timeout, queued). For: precondition violations ("Nothing to undo."),
    /// validation hints ("Project ID can't be empty."), empty-state
    /// notices ("No stale hosts."), stale-host warnings, deprecated
    /// config detected, partial sync results. Warnings are NOT sticky;
    /// the user acknowledges them by continuing to interact.
    ///
    /// Use `notify_error` only for system-level failures (I/O, network,
    /// subprocess) that require explicit acknowledgement. Use
    /// `notify_warning` for everything that is "this can't happen given
    /// current state" or "you forgot something".
    pub fn notify_warning(&mut self, text: impl Into<String>) {
        let msg = StatusMessage {
            text: text.into(),
            class: MessageClass::Warning,
            tick_count: 0,
            sticky: false,
            created_at: std::time::Instant::now(),
        };
        log::debug!("toast <- Warning: {}", msg.text);
        self.status_center.push_toast(msg);
    }

    /// Long-running progress → footer sticky (never expires).
    /// For: Vault SSH signing, multi-step operations.
    #[allow(deprecated)]
    pub fn notify_progress(&mut self, text: impl Into<String>) {
        self.set_sticky_status(text, false);
    }

    /// Sticky error → footer sticky.
    #[allow(deprecated)]
    pub fn notify_sticky_error(&mut self, text: impl Into<String>) {
        self.set_sticky_status(text, true);
    }

    /// Explicit info → footer (4s, not suppressed).
    /// For: config reload, sync complete.
    #[allow(deprecated)]
    pub fn notify_info(&mut self, text: impl Into<String>) {
        self.set_info_status(text);
    }

    /// Tick the footer status message timer. Uses wall-clock time.
    /// Sticky/Progress messages never expire automatically.
    ///
    /// Stays on `App` (not moved to `StatusCenter`) because expiry is
    /// suppressed while any provider sync is in flight, which requires
    /// reading `self.providers.syncing`.
    pub fn tick_status(&mut self) {
        // Don't expire status while providers are still syncing
        if !self.providers.syncing.is_empty() {
            return;
        }
        if let Some(ref status) = self.status_center.status {
            if status.sticky {
                return;
            }
            let timeout_ms = status.timeout_ms();
            if timeout_ms != u64::MAX && status.created_at.elapsed().as_millis() as u64 > timeout_ms
            {
                log::debug!("footer status expired: {}", status.text);
                self.status_center.status = None;
            }
        }
    }

    /// Shim. Routes to `StatusCenter::tick_toast`.
    pub fn tick_toast(&mut self) {
        self.status_center.tick_toast();
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
                | Screen::BulkTagEditor
                | Screen::ThemePicker
                | Screen::WhatsNew(_)
        ) || self.tags.input.is_some()
        {
            return;
        }
        let current_mtime = reload_state::get_mtime(&self.reload.config_path);
        let changed = current_mtime != self.reload.last_modified
            || self
                .reload
                .include_mtimes
                .iter()
                .any(|(path, old_mtime)| reload_state::get_mtime(path) != *old_mtime)
            || self
                .reload
                .include_dir_mtimes
                .iter()
                .any(|(path, old_mtime)| reload_state::get_mtime(path) != *old_mtime);
        if changed {
            if let Ok(new_config) = SshConfigFile::parse(&self.reload.config_path) {
                self.hosts_state.ssh_config = new_config;
                // Invalidate undo state — config structure may have changed externally
                self.hosts_state.undo_stack.clear();
                // Clear stale ping status — hosts may have changed
                self.ping.status.clear();
                self.ping.filter_down_only = false;
                self.ping.checked_at = None;
                self.reload_hosts();
                self.reload.last_modified = current_mtime;
                self.reload.include_mtimes =
                    reload_state::snapshot_include_mtimes(&self.hosts_state.ssh_config);
                self.reload.include_dir_mtimes =
                    reload_state::snapshot_include_dir_mtimes(&self.hosts_state.ssh_config);
                let count = self.hosts_state.list.len();
                self.notify_background(crate::messages::config_reloaded(count));
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
        let current_mtime = reload_state::get_mtime(&self.reload.config_path);
        current_mtime != self.reload.last_modified
            || self
                .reload
                .include_mtimes
                .iter()
                .any(|(path, old_mtime)| reload_state::get_mtime(path) != *old_mtime)
            || self
                .reload
                .include_dir_mtimes
                .iter()
                .any(|(path, old_mtime)| reload_state::get_mtime(path) != *old_mtime)
    }

    /// Update the last_modified timestamp (call after writing config).
    pub fn update_last_modified(&mut self) {
        self.reload.last_modified = reload_state::get_mtime(&self.reload.config_path);
        self.reload.include_mtimes =
            reload_state::snapshot_include_mtimes(&self.hosts_state.ssh_config);
        self.reload.include_dir_mtimes =
            reload_state::snapshot_include_dir_mtimes(&self.hosts_state.ssh_config);
    }

    /// Returns true if any host or provider has a vault role configured.
    pub fn has_any_vault_role(&self) -> bool {
        for host in &self.hosts_state.list {
            if host.vault_ssh.is_some() {
                return true;
            }
        }
        for section in &self.providers.config.sections {
            if !section.vault_role.is_empty() {
                return true;
            }
        }
        false
    }

    /// Poll active tunnels for exit. Returns (alias, message, is_error) tuples.
    pub fn poll_tunnels(&mut self) -> Vec<(String, String, bool)> {
        self.tunnels.poll()
    }
}

/// Cycle list selection forward or backward with wraparound.
pub(crate) fn cycle_selection(state: &mut ListState, len: usize, forward: bool) {
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
pub(crate) fn page_down(state: &mut ListState, len: usize, page_size: usize) {
    if len == 0 {
        return;
    }
    let current = state.selected().unwrap_or(0);
    let next = (current + page_size).min(len - 1);
    state.select(Some(next));
}

/// Jump backward by page_size items, clamping at 0 (no wrap).
pub(crate) fn page_up(state: &mut ListState, len: usize, page_size: usize) {
    if len == 0 {
        return;
    }
    let current = state.selected().unwrap_or(0);
    let prev = current.saturating_sub(page_size);
    state.select(Some(prev));
}

/// A command that can be executed from the command palette.
#[derive(Debug, Clone, Copy)]
pub struct PaletteCommand {
    pub key: char,
    pub label: &'static str,
    /// Section for future grouped display. Not yet used by the renderer.
    #[allow(dead_code)]
    pub section: &'static str,
}

static ALL_PALETTE_COMMANDS: &[PaletteCommand] = &[
    PaletteCommand {
        key: 'a',
        label: "add host",
        section: "manage",
    },
    PaletteCommand {
        key: 'A',
        label: "add pattern",
        section: "manage",
    },
    PaletteCommand {
        key: 'e',
        label: "edit",
        section: "manage",
    },
    PaletteCommand {
        key: 'd',
        label: "del",
        section: "manage",
    },
    PaletteCommand {
        key: 'c',
        label: "clone",
        section: "manage",
    },
    PaletteCommand {
        key: 'u',
        label: "undo del",
        section: "manage",
    },
    PaletteCommand {
        key: 't',
        label: "tag (inline)",
        section: "manage",
    },
    PaletteCommand {
        key: 'i',
        label: "all directives",
        section: "manage",
    },
    PaletteCommand {
        key: 'y',
        label: "copy ssh command",
        section: "clipboard",
    },
    PaletteCommand {
        key: 'x',
        label: "copy config block",
        section: "clipboard",
    },
    PaletteCommand {
        key: 'X',
        label: "purge stale",
        section: "clipboard",
    },
    PaletteCommand {
        key: 'F',
        label: "file explorer",
        section: "tools",
    },
    PaletteCommand {
        key: 'T',
        label: "tunnels",
        section: "tools",
    },
    PaletteCommand {
        key: 'C',
        label: "containers",
        section: "tools",
    },
    PaletteCommand {
        key: 'K',
        label: "SSH keys",
        section: "tools",
    },
    PaletteCommand {
        key: 'S',
        label: "providers",
        section: "tools",
    },
    PaletteCommand {
        key: 'V',
        label: "vault sign",
        section: "tools",
    },
    PaletteCommand {
        key: 'I',
        label: "import known_hosts",
        section: "tools",
    },
    PaletteCommand {
        key: 'm',
        label: "theme",
        section: "tools",
    },
    PaletteCommand {
        key: 'n',
        label: "what's new",
        section: "tools",
    },
    PaletteCommand {
        key: 'r',
        label: "run snippet",
        section: "connect",
    },
    PaletteCommand {
        key: 'R',
        label: "run on all visible",
        section: "connect",
    },
    PaletteCommand {
        key: 'p',
        label: "ping",
        section: "connect",
    },
    PaletteCommand {
        key: 'P',
        label: "ping all",
        section: "connect",
    },
    PaletteCommand {
        key: '!',
        label: "down-only filter",
        section: "connect",
    },
];

impl PaletteCommand {
    pub fn all() -> &'static [PaletteCommand] {
        ALL_PALETTE_COMMANDS
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommandPaletteState {
    pub query: String,
    pub selected: usize,
}

impl CommandPaletteState {
    pub fn push_query(&mut self, c: char) {
        if self.query.len() < 64 {
            self.query.push(c);
        }
        self.selected = 0;
    }

    pub fn pop_query(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    /// Return commands filtered by the current query (substring match on label).
    /// Returns a borrowed static slice when the query is empty (no allocation).
    pub fn filtered_commands(&self) -> std::borrow::Cow<'static, [PaletteCommand]> {
        let all = PaletteCommand::all();
        if self.query.is_empty() {
            return std::borrow::Cow::Borrowed(all);
        }
        let q = self.query.to_lowercase();
        std::borrow::Cow::Owned(
            all.iter()
                .filter(|cmd| cmd.label.to_lowercase().contains(&q))
                .copied()
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests;
