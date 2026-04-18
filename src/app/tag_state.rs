//! Tag domain state: per-host tag tracking and bulk-tag-editor model.

use crate::app::host_state::GroupBy;
use crate::ssh_config::model::HostEntry;

/// A display tag with its source (user-defined or provider-synced).
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayTag {
    pub name: String,
    pub is_user: bool,
}

/// Select up to 3 tags for display based on view mode and grouping.
/// Returns a Vec of up to 3 DisplayTags (user tags first, then provider tags).
pub fn select_display_tags(
    host: &HostEntry,
    group_by: &GroupBy,
    detail_mode: bool,
) -> Vec<DisplayTag> {
    let group_name = match group_by {
        GroupBy::Provider => host.provider.clone(),
        GroupBy::Tag(t) => Some(t.clone()),
        GroupBy::None => None,
    };

    let not_group = |t: &&str| {
        group_name
            .as_ref()
            .is_none_or(|g| !t.eq_ignore_ascii_case(g))
    };

    // Collect user tags, filtering out the group name
    let user_tags: Vec<DisplayTag> = host
        .tags
        .iter()
        .map(|t| t.as_str())
        .filter(not_group)
        .map(|t| DisplayTag {
            name: t.to_string(),
            is_user: true,
        })
        .collect();

    let limit = if detail_mode { 1 } else { 3 };
    let is_grouped = !matches!(group_by, GroupBy::None);

    // Grouped view: user tags only. Flat view: user tags + provider tags.
    if is_grouped {
        user_tags.into_iter().take(limit).collect()
    } else {
        let provider_tags = host
            .provider_tags
            .iter()
            .chain(host.provider.iter())
            .map(|t| DisplayTag {
                name: t.to_string(),
                is_user: false,
            });
        user_tags
            .into_iter()
            .chain(provider_tags)
            .take(limit)
            .collect()
    }
}

/// Tag editor state.
#[derive(Default)]
pub struct TagState {
    pub input: Option<String>,
    pub cursor: usize,
    pub list: Vec<String>,
}

/// User action per tag row in the bulk tag editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkTagAction {
    /// `[~]` Leave each host's state for this tag unchanged.
    Leave,
    /// `[x]` Ensure the tag is present on every selected host.
    AddToAll,
    /// `[ ]` Ensure the tag is absent from every selected host.
    RemoveFromAll,
}

impl BulkTagAction {
    /// 3-way cycle: `Leave` → `AddToAll` → `RemoveFromAll` → `Leave`.
    pub fn cycle(self) -> Self {
        match self {
            BulkTagAction::Leave => BulkTagAction::AddToAll,
            BulkTagAction::AddToAll => BulkTagAction::RemoveFromAll,
            BulkTagAction::RemoveFromAll => BulkTagAction::Leave,
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            BulkTagAction::Leave => "[~]",
            BulkTagAction::AddToAll => "[x]",
            BulkTagAction::RemoveFromAll => "[ ]",
        }
    }
}

/// A single row in the bulk tag editor.
#[derive(Debug, Clone)]
pub struct BulkTagRow {
    pub tag: String,
    /// Number of selected hosts that had this tag at editor open time.
    pub initial_count: usize,
    pub action: BulkTagAction,
}

/// Snapshot state for the bulk tag editor overlay.
#[derive(Debug, Default)]
pub struct BulkTagEditorState {
    pub rows: Vec<BulkTagRow>,
    /// Aliases being edited, snapshot at open time so selection changes
    /// during the flow do not affect the in-progress edit.
    pub aliases: Vec<String>,
    /// Aliases that live in an Include file and cannot be edited in place.
    /// Surfaced in the header so the user sees the blast radius.
    pub skipped_included: Vec<String>,
    /// Draft name for a brand-new tag being typed by the user. `None` when
    /// the input bar is inactive. Newly entered tags are appended to `rows`
    /// with `action = AddToAll`.
    pub new_tag_input: Option<String>,
    pub new_tag_cursor: usize,
    /// Snapshot of `rows[i].action` at editor open time. Used by `is_dirty`
    /// to detect pending changes on Esc and prompt the user before
    /// discarding. Captured by the opener (e.g. `App::open_bulk_tag_editor`)
    /// after `rows` is populated.
    ///
    /// Length-mismatch semantics: any extra row beyond the baseline length
    /// (i.e. a newly added tag via `+`) counts as dirty if its action is
    /// non-Leave. This matches the user's intuition that "I typed a new tag,
    /// closing now should warn me".
    pub initial_actions: Vec<BulkTagAction>,
}

impl BulkTagEditorState {
    /// Returns true if any row's action differs from the open-time baseline,
    /// or if rows have been added since open.
    ///
    /// Single source of truth for the dirty check. The handler consults this
    /// on Esc to decide between immediate exit and discard confirmation.
    /// Every editable surface gets a dirty-check so Esc never drops unsaved
    /// work.
    ///
    /// **Invariant**: rows is append-only after `open_bulk_tag_editor`
    /// captures the baseline. The `+ new tag` flow only appends to `rows`;
    /// no code path removes rows during the editor session. If a future
    /// change introduces row removal, the length-mismatch branch below will
    /// silently treat the missing baseline rows as clean (because `zip`
    /// stops at the shorter slice). At that point this method needs an
    /// explicit shrink branch; the assertion below guards the assumption.
    pub fn is_dirty(&self) -> bool {
        debug_assert!(
            self.rows.len() >= self.initial_actions.len(),
            "rows must be append-only after baseline capture; \
             shorter rows breaks the dirty-check"
        );
        if self.rows.len() != self.initial_actions.len() {
            // Tags added since open. New rows count as dirty unless still Leave.
            return self
                .rows
                .iter()
                .skip(self.initial_actions.len())
                .any(|r| r.action != BulkTagAction::Leave)
                || self
                    .rows
                    .iter()
                    .zip(self.initial_actions.iter())
                    .any(|(r, baseline)| r.action != *baseline);
        }
        self.rows
            .iter()
            .zip(self.initial_actions.iter())
            .any(|(r, baseline)| r.action != *baseline)
    }
}

/// Outcome of applying a bulk tag edit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BulkTagApplyResult {
    /// Hosts whose tag list actually changed.
    pub changed_hosts: usize,
    /// Total (host, tag) additions.
    pub added: usize,
    /// Total (host, tag) removals.
    pub removed: usize,
    /// Hosts skipped because they live in an Include file.
    pub skipped_included: usize,
}
