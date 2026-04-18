use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, BulkTagEditorState, Screen};

pub(super) fn handle_bulk_tag_editor_screen(app: &mut App, key: KeyEvent) {
    // When the "new tag" input bar is active, route character input there
    // first so users can type tag names without triggering the row-level
    // keybindings (j/k/Space/Enter). Esc cancels the input without closing
    // the editor. The new-tag-input early-return runs BEFORE the discard
    // confirm so typing-mode Esc does not trigger the dirty check.
    if app.forms.bulk_tag_editor.new_tag_input.is_some() {
        handle_new_tag_input(app, key);
        return;
    }

    // Discard confirmation: when the user pressed Esc on a dirty editor, the
    // main handler set `pending_discard_confirm` and re-rendered with the
    // discard footer. Route the next keypress through the central confirm
    // router (uniform with form discard prompts elsewhere).
    if app.forms.pending_discard_confirm {
        match super::route_confirm_key(key) {
            super::ConfirmAction::Yes => {
                app.forms.pending_discard_confirm = false;
                app.set_screen(Screen::HostList);
                app.forms.bulk_tag_editor = BulkTagEditorState::default();
            }
            super::ConfirmAction::No => {
                app.forms.pending_discard_confirm = false;
            }
            super::ConfirmAction::Ignored => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            // Stakes test: tag edits are non-trivial work (typing new tags,
            // deciding add/remove per row across N hosts). Warn before
            // discarding.
            if app.forms.bulk_tag_editor.is_dirty() {
                app.forms.pending_discard_confirm = true;
            } else {
                app.set_screen(Screen::HostList);
                app.forms.bulk_tag_editor = BulkTagEditorState::default();
            }
        }
        KeyCode::Char('?') => {
            let old = std::mem::replace(&mut app.screen, Screen::HostList);
            app.set_screen(Screen::Help {
                return_screen: Box::new(old),
            });
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.bulk_tag_editor_next();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.bulk_tag_editor_prev();
        }
        KeyCode::Char(' ') => {
            app.bulk_tag_editor_cycle_current();
        }
        KeyCode::Char('+') => {
            app.forms.bulk_tag_editor.new_tag_input = Some(String::new());
            app.forms.bulk_tag_editor.new_tag_cursor = 0;
        }
        KeyCode::Enter => match app.bulk_tag_apply() {
            Ok(result) => {
                app.set_screen(Screen::HostList);
                app.forms.bulk_tag_editor = BulkTagEditorState::default();
                let msg = format_apply_status(&result);
                if !msg.is_empty() {
                    app.notify(msg);
                }
            }
            Err(err) => {
                app.notify_error(err);
            }
        },
        _ => {}
    }
}

/// Status string shown after a successful bulk apply. Empty when nothing
/// was pending (no-op) and no included-host warning applies, so the caller
/// can skip setting a status.
pub(crate) fn format_apply_status(result: &crate::app::BulkTagApplyResult) -> String {
    let mut parts: Vec<String> = Vec::new();
    if result.changed_hosts > 0 {
        parts.push(format!(
            "Updated {} host{}",
            result.changed_hosts,
            if result.changed_hosts == 1 { "" } else { "s" }
        ));
        let mut delta = Vec::new();
        if result.added > 0 {
            delta.push(format!("+{}", result.added));
        }
        if result.removed > 0 {
            delta.push(format!("-{}", result.removed));
        }
        if !delta.is_empty() {
            let last = parts.pop().expect("just pushed host count");
            parts.push(format!("{} ({})", last, delta.join(" ")));
        }
    }
    if result.skipped_included > 0 {
        parts.push(format!(
            "skipped {} in include file{}",
            result.skipped_included,
            if result.skipped_included == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    parts.join(". ")
}

fn handle_new_tag_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            app.bulk_tag_editor_commit_new_tag();
        }
        KeyCode::Esc => {
            app.forms.bulk_tag_editor.new_tag_input = None;
            app.forms.bulk_tag_editor.new_tag_cursor = 0;
        }
        KeyCode::Left if app.forms.bulk_tag_editor.new_tag_cursor > 0 => {
            app.forms.bulk_tag_editor.new_tag_cursor -= 1;
        }
        KeyCode::Right => {
            if let Some(ref input) = app.forms.bulk_tag_editor.new_tag_input {
                if app.forms.bulk_tag_editor.new_tag_cursor < input.chars().count() {
                    app.forms.bulk_tag_editor.new_tag_cursor += 1;
                }
            }
        }
        KeyCode::Home => {
            app.forms.bulk_tag_editor.new_tag_cursor = 0;
        }
        KeyCode::End => {
            if let Some(ref input) = app.forms.bulk_tag_editor.new_tag_input {
                app.forms.bulk_tag_editor.new_tag_cursor = input.chars().count();
            }
        }
        KeyCode::Backspace if app.forms.bulk_tag_editor.new_tag_cursor > 0 => {
            if let Some(ref mut input) = app.forms.bulk_tag_editor.new_tag_input {
                let byte_pos =
                    crate::app::char_to_byte_pos(input, app.forms.bulk_tag_editor.new_tag_cursor);
                let prev = crate::app::char_to_byte_pos(
                    input,
                    app.forms.bulk_tag_editor.new_tag_cursor - 1,
                );
                input.drain(prev..byte_pos);
                app.forms.bulk_tag_editor.new_tag_cursor -= 1;
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut input) = app.forms.bulk_tag_editor.new_tag_input {
                let byte_pos =
                    crate::app::char_to_byte_pos(input, app.forms.bulk_tag_editor.new_tag_cursor);
                input.insert(byte_pos, c);
                app.forms.bulk_tag_editor.new_tag_cursor += 1;
            }
        }
        _ => {}
    }
}
