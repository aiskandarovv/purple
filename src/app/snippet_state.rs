use crate::app::SnippetFormBaseline;
use crate::app::forms::{SnippetForm, SnippetOutputState, SnippetParamFormState};
use crate::snippet::{Snippet, SnippetStore};

/// Snippet-owned state grouped off the `App` god-struct. Holds the on-disk
/// snippet store, the edit form, the pending execution payload, the output
/// screen state, the param form, the terminal-submit flag, the dirty-check
/// baseline and the pending-delete index. Pure state container.
pub struct SnippetState {
    pub store: SnippetStore,
    pub form: SnippetForm,
    pub pending: Option<(Snippet, Vec<String>)>,
    pub output: Option<SnippetOutputState>,
    pub param_form: Option<SnippetParamFormState>,
    pub pending_terminal: bool,
    pub form_baseline: Option<SnippetFormBaseline>,
    pub pending_delete: Option<usize>,
}

impl Default for SnippetState {
    fn default() -> Self {
        Self {
            store: SnippetStore::default(),
            form: SnippetForm::new(),
            pending: None,
            output: None,
            param_form: None,
            pending_terminal: false,
            form_baseline: None,
            pending_delete: None,
        }
    }
}

impl SnippetState {
    /// Construct with snippet store loaded from disk.
    pub fn with_store_loaded() -> Self {
        Self {
            store: crate::snippet::SnippetStore::load(),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        let s = SnippetState::default();
        assert!(s.pending.is_none());
        assert!(s.output.is_none());
        assert!(s.param_form.is_none());
        assert!(!s.pending_terminal);
        assert!(s.form_baseline.is_none());
        assert!(s.pending_delete.is_none());
    }
}
