use std::collections::{HashMap, HashSet};

use ratatui::text::Span;

use crate::app::ping::PingStatus;
use crate::ssh_config::model::{ConfigElement, HostEntry, PatternEntry, SshConfigFile};
use crate::ui::theme;

/// Host, group, sort and view state grouped off the `App` god-struct. Holds
/// the parsed `~/.ssh/config`, the resolved host + pattern entries, the
/// display list built from them, the render cache, the undo stack for
/// deletions, the multi-select set for bulk snippet runs and all sort /
/// group / view UI-state. Pure state container.
pub struct HostState {
    pub ssh_config: SshConfigFile,
    pub list: Vec<HostEntry>,
    pub patterns: Vec<PatternEntry>,
    pub display_list: Vec<HostListItem>,
    pub render_cache: HostListRenderCache,
    pub undo_stack: Vec<DeletedHost>,
    /// Host indices selected for multi-host snippet execution (space to toggle).
    pub multi_select: HashSet<usize>,
    pub sort_mode: SortMode,
    pub group_by: GroupBy,
    pub view_mode: ViewMode,
    /// Currently active group filter (tab navigation). None = show all groups.
    pub group_filter: Option<String>,
    /// Index into group_tab_order for tab navigation.
    pub group_tab_index: usize,
    /// Ordered list of group names from the current display list.
    pub group_tab_order: Vec<String>,
    /// Host/pattern counts per group (computed before group filtering).
    pub group_host_counts: HashMap<String, usize>,
}

impl HostState {
    /// Construct from a loaded config and pre-resolved host/pattern lists.
    pub fn from_config(
        ssh_config: SshConfigFile,
        hosts: Vec<HostEntry>,
        patterns: Vec<PatternEntry>,
        display_list: Vec<HostListItem>,
    ) -> Self {
        Self {
            ssh_config,
            list: hosts,
            patterns,
            display_list,
            render_cache: HostListRenderCache::default(),
            undo_stack: Vec::new(),
            multi_select: HashSet::new(),
            sort_mode: SortMode::Original,
            group_by: GroupBy::None,
            view_mode: ViewMode::Compact,
            group_filter: None,
            group_tab_index: 0,
            group_tab_order: Vec::new(),
            group_host_counts: HashMap::new(),
        }
    }
}

#[cfg(test)]
impl Default for HostState {
    fn default() -> Self {
        Self {
            ssh_config: SshConfigFile {
                elements: Vec::new(),
                path: std::path::PathBuf::new(),
                crlf: false,
                bom: false,
            },
            list: Vec::new(),
            patterns: Vec::new(),
            display_list: Vec::new(),
            render_cache: HostListRenderCache::default(),
            undo_stack: Vec::new(),
            multi_select: HashSet::new(),
            sort_mode: SortMode::Original,
            group_by: GroupBy::None,
            view_mode: ViewMode::Compact,
            group_filter: None,
            group_tab_index: 0,
            group_tab_order: Vec::new(),
            group_host_counts: HashMap::new(),
        }
    }
}

/// An item in the display list (hosts + group headers).
#[derive(Debug, Clone)]
pub enum HostListItem {
    GroupHeader(String),
    Host { index: usize },
    Pattern { index: usize },
}

/// View mode for the host list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewMode {
    Compact,
    Detailed,
}

/// Sort mode for the host list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortMode {
    Original,
    AlphaAlias,
    AlphaHostname,
    Frecency,
    MostRecent,
    Status,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            SortMode::Original => SortMode::AlphaAlias,
            SortMode::AlphaAlias => SortMode::AlphaHostname,
            SortMode::AlphaHostname => SortMode::Frecency,
            SortMode::Frecency => SortMode::MostRecent,
            SortMode::MostRecent => SortMode::Status,
            SortMode::Status => SortMode::Original,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::Original => "config order",
            SortMode::AlphaAlias => "A-Z alias",
            SortMode::AlphaHostname => "A-Z hostname",
            SortMode::Frecency => "most used",
            SortMode::MostRecent => "most recent",
            SortMode::Status => "down first",
        }
    }

    pub fn to_key(self) -> &'static str {
        match self {
            SortMode::Original => "original",
            SortMode::AlphaAlias => "alpha_alias",
            SortMode::AlphaHostname => "alpha_hostname",
            SortMode::Frecency => "frecency",
            SortMode::MostRecent => "most_recent",
            SortMode::Status => "status",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "original" => SortMode::Original,
            "alpha_alias" => SortMode::AlphaAlias,
            "alpha_hostname" => SortMode::AlphaHostname,
            "frecency" => SortMode::Frecency,
            "most_recent" => SortMode::MostRecent,
            "status" => SortMode::Status,
            _ => SortMode::MostRecent,
        }
    }
}

/// Build health summary spans: ●23 ▲2 ✖1 ○1
/// Only includes states with count > 0. Returns empty vec if no pings.
pub fn health_summary_spans(
    ping_status: &HashMap<String, PingStatus>,
    hosts: &[HostEntry],
) -> Vec<Span<'static>> {
    health_summary_spans_for(ping_status, hosts.iter().map(|h| h.alias.as_str()))
}

/// Build health summary spans for a subset of host aliases.
/// Only includes states with count > 0. Returns empty vec if no pings.
pub fn health_summary_spans_for<'a>(
    ping_status: &HashMap<String, PingStatus>,
    aliases: impl Iterator<Item = &'a str>,
) -> Vec<Span<'static>> {
    if ping_status.is_empty() {
        return vec![];
    }
    let mut online = 0u32;
    let mut slow = 0u32;
    let mut down = 0u32;
    let mut unchecked = 0u32;
    for alias in aliases {
        match ping_status.get(alias) {
            Some(PingStatus::Reachable { .. }) => online += 1,
            Some(PingStatus::Slow { .. }) => slow += 1,
            Some(PingStatus::Unreachable) => down += 1,
            Some(PingStatus::Checking) | None => unchecked += 1,
            Some(PingStatus::Skipped) => {} // ProxyJump, excluded
        }
    }
    let mut spans = Vec::new();
    if online > 0 {
        spans.push(Span::styled(
            format!("\u{25CF}{online}"),
            theme::online_dot(),
        ));
    }
    if slow > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!("\u{25B2}{slow}"), theme::warning()));
    }
    if down > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!("\u{2716}{down}"), theme::error()));
    }
    if unchecked > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!("\u{25CB}{unchecked}"), theme::muted()));
    }
    spans
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

/// Item in the ProxyJump picker list. Scored hosts (used elsewhere as
/// ProxyJump, matching a jump-host name pattern, or sharing the editing
/// host's domain suffix) are promoted above a visual separator so the
/// likely pick is at the top and the rest stays alphabetical below.
/// `SectionLabel` renders a non-selectable heading (e.g. "Suggestions")
/// above the scored section. Navigation skips both `SectionLabel` and
/// `Separator`.
#[derive(Debug, Clone, PartialEq)]
pub enum ProxyJumpCandidate {
    Host {
        alias: String,
        hostname: String,
        suggested: bool,
    },
    SectionLabel(&'static str),
    Separator,
}

/// Lazily-computed derived state that feeds the host-list renderer.
///
/// The renderer runs on every keystroke and every animation tick. Rebuilding
/// these from `hosts`/`display_list`/`history` per frame allocates thousands
/// of short-lived `String`s on hosts lists in the 500+ range. Fields are
/// `None` when dirty; the renderer populates them on first use after an
/// invalidation and subsequent frames reuse the values until the next
/// mutation calls `invalidate()`.
#[derive(Default)]
pub struct HostListRenderCache {
    /// Max width of formatted "last connected" strings across all hosts.
    /// Caches the `format_time_ago` allocations.
    pub history_width: Option<usize>,
    /// Group-header text -> host aliases in that group. Built from
    /// `display_list`, so invalidates on every sort/filter/reload.
    pub group_alias_map: Option<HashMap<String, Vec<String>>>,
}

impl HostListRenderCache {
    pub fn invalidate(&mut self) {
        self.history_width = None;
        self.group_alias_map = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        let s = HostState::default();
        assert!(s.list.is_empty());
        assert!(s.patterns.is_empty());
        assert!(s.display_list.is_empty());
        assert!(s.undo_stack.is_empty());
        assert!(s.multi_select.is_empty());
        assert!(s.group_filter.is_none());
        assert!(s.group_tab_order.is_empty());
        assert!(s.group_host_counts.is_empty());
    }
}
