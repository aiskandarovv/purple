//! UI selection substate: list cursors, picker overlays, scroll offsets.

use ratatui::widgets::ListState;

use crate::ui::theme::ThemeDef;

/// A picker overlay: open flag plus its list cursor.
#[derive(Debug, Default)]
pub struct PickerState {
    pub open: bool,
    pub list: ListState,
}

#[allow(dead_code)]
impl PickerState {
    /// Open the picker with the cursor positioned at `index`.
    pub fn open_at(&mut self, index: usize) {
        self.open = true;
        self.list.select(Some(index));
    }

    /// Close the picker and reset the cursor.
    pub fn close(&mut self) {
        self.open = false;
        self.list.select(None);
    }
}

/// Theme picker carries extra catalogue + preview state beyond a simple list.
#[derive(Debug, Default)]
pub struct ThemePickerState {
    pub list: ListState,
    pub builtins: Vec<ThemeDef>,
    pub custom: Vec<ThemeDef>,
    pub saved_name: String,
    pub original: Option<ThemeDef>,
}

/// Region picker uses a cursor index rather than a ListState because region
/// rows are a synthetic flat array (provider × region pairs) rather than a
/// ratatui-managed list.
#[derive(Debug, Default)]
pub struct RegionPickerState {
    pub open: bool,
    pub cursor: usize,
}

#[derive(Debug, Default)]
pub struct UiSelection {
    pub list_state: ListState,
    pub key_list_state: ListState,
    pub key_picker: PickerState,
    pub password_picker: PickerState,
    pub proxyjump_picker: PickerState,
    pub vault_role_picker: PickerState,
    pub tag_picker_state: ListState,
    pub bulk_tag_editor_state: ListState,
    pub theme_picker: ThemePickerState,
    pub provider_list_state: ListState,
    pub tunnel_list_state: ListState,
    pub snippet_picker_state: ListState,
    pub snippet_search: Option<String>,
    pub region_picker: RegionPickerState,
    pub help_scroll: u16,
    pub detail_scroll: u16,
}

impl UiSelection {
    /// Construct with all picker/list state defaulted and the host list
    /// selection pre-positioned at `initial` (the first selectable host or
    /// pattern in the display list).
    pub fn new_with_initial_selection(initial: Option<usize>) -> Self {
        let mut s = Self::default();
        if let Some(pos) = initial {
            s.list_state.select(Some(pos));
        }
        s
    }
}
