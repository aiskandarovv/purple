use std::collections::{HashMap, HashSet};

use crate::app::types::{
    DeletedHost, GroupBy, HostListItem, HostListRenderCache, SortMode, ViewMode,
};
use crate::ssh_config::model::{HostEntry, PatternEntry, SshConfigFile};

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
