use crate::app::forms::HostForm;
use crate::app::types::{BulkTagEditorState, FormBaseline};

/// Host-form and bulk-tag editor state grouped off the `App` god-struct.
/// Holds the add/edit host form, its dirty-check baseline, the bulk-tag
/// editor, the last-apply snapshot used by `u` to revert bulk-tag changes
/// and the pending-discard confirmation flag. Pure state container.
pub struct FormState {
    pub host: HostForm,
    pub host_baseline: Option<FormBaseline>,
    pub bulk_tag_editor: BulkTagEditorState,
    /// Snapshot of the last bulk tag apply, used by `u` to revert the
    /// operation even though `undo_stack` only holds deleted hosts. Holds
    /// `(alias, previous_tags)` pairs so restore is idempotent. Cleared
    /// after a successful undo or on the next mutation.
    pub bulk_tag_undo: Option<Vec<(String, Vec<String>)>>,
    /// When true, the Esc key shows a "Discard changes?" dialog instead of
    /// closing the open host form.
    pub pending_discard_confirm: bool,
}

impl Default for FormState {
    fn default() -> Self {
        Self {
            host: HostForm::new(),
            host_baseline: None,
            bulk_tag_editor: BulkTagEditorState::default(),
            bulk_tag_undo: None,
            pending_discard_confirm: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        let s = FormState::default();
        assert!(!s.pending_discard_confirm);
        assert!(s.bulk_tag_undo.is_none());
        assert!(s.host_baseline.is_none());
        assert!(s.bulk_tag_editor.rows.is_empty());
    }
}
