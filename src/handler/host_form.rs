use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, FormField, Screen};
use crate::quick_add;

pub(super) fn handle_form(app: &mut App, key: KeyEvent) {
    // Dispatch to password picker if it's open
    if app.ui.show_password_picker {
        super::picker::handle_password_picker(app, key);
        return;
    }

    // Dispatch to key picker if it's open
    if app.ui.show_key_picker {
        super::picker::handle_key_picker_shared(app, key, false);
        return;
    }

    // Dispatch to proxyjump picker if it's open
    if app.ui.show_proxyjump_picker {
        super::picker::handle_proxyjump_picker(app, key);
        return;
    }

    // Dispatch to vault role picker if it's open
    if app.ui.show_vault_role_picker {
        super::picker::handle_vault_role_picker(app, key);
        return;
    }

    // Handle discard confirmation dialog
    if app.forms.pending_discard_confirm {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.forms.pending_discard_confirm = false;
                app.clear_form_mtime();
                app.forms.host_baseline = None;
                app.set_screen(Screen::HostList);
                app.flush_pending_vault_write();
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.forms.pending_discard_confirm = false;
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            if app.host_form_is_dirty() {
                app.forms.pending_discard_confirm = true;
            } else {
                app.clear_form_mtime();
                app.forms.host_baseline = None;
                app.set_screen(Screen::HostList);
                app.flush_pending_vault_write();
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            // Smart paste detection: when leaving Alias field, check for user@host:port
            if app.forms.host.focused_field == FormField::Alias {
                maybe_smart_paste(app);
            }
            if !app.forms.host.expanded {
                // Collapsed mode: Tab/Down from last required field expands
                match app.forms.host.focused_field {
                    FormField::Alias => {
                        app.forms.host.focused_field = FormField::Hostname;
                    }
                    FormField::Hostname => {
                        app.forms.host.expanded = true;
                        app.forms.host.focused_field = FormField::User;
                    }
                    // Defensive: if focus is on an optional field while collapsed, reset
                    _ => {
                        app.forms.host.focused_field = FormField::Alias;
                    }
                }
            } else {
                // Progressive disclosure: advance through the visible field
                // subset so Tab skips over the hidden `VaultAddr` field when
                // no role is set.
                app.forms.host.focus_next_visible();
            }
            app.forms.host.sync_cursor_to_end();
            app.forms.host.update_hint();
        }
        KeyCode::BackTab | KeyCode::Up => {
            if !app.forms.host.expanded {
                // Collapsed: cycle within required fields only
                app.forms.host.focused_field = match app.forms.host.focused_field {
                    FormField::Alias => FormField::Hostname,
                    // Any other field (including Hostname): go to Alias
                    _ => FormField::Alias,
                };
            } else {
                app.forms.host.focus_prev_visible();
            }
            app.forms.host.sync_cursor_to_end();
            app.forms.host.update_hint();
        }
        KeyCode::Left if app.forms.host.cursor_pos > 0 => {
            app.forms.host.cursor_pos -= 1;
        }
        KeyCode::Right => {
            let len = app.forms.host.focused_value().chars().count();
            if app.forms.host.cursor_pos < len {
                app.forms.host.cursor_pos += 1;
            }
        }
        KeyCode::Home => {
            app.forms.host.cursor_pos = 0;
        }
        KeyCode::End => {
            app.forms.host.sync_cursor_to_end();
        }
        KeyCode::Enter => {
            // INVARIANT: Enter ALWAYS submits the form, regardless of focused
            // field. Pickers are reached via Space (see Char(' ') arm below).
            // Smart-paste detection runs before submit on the Alias field so
            // pasted user@host:port targets get split into the right fields.
            if app.forms.host.focused_field == FormField::Alias {
                maybe_smart_paste(app);
            }
            submit_form(app);
        }
        // SPACE GUARD MUST PRECEDE the generic Char(c) arm.
        // Rust matches arms top-to-bottom; reordering this arm below the
        // generic insert-char would let Space fall through as a literal
        // character and break picker activation.
        //
        // The "empty-field" gate preserves free-text editing: once the
        // user has typed anything, Space inserts a literal space (so paths
        // like `/home/me/My Keys/id_rsa` and custom askpass commands like
        // `my-script %h` work). On an empty picker field, Space opens the
        // picker — that is the affordance that makes pickers discoverable.
        //
        // Edge case: `VaultSsh` is `is_picker() == true` even when no role
        // candidates are configured (the role list is provider-derived).
        // In that case `open_picker_for_focused_field` short-circuits and
        // inserts a literal space — Space on empty VaultSsh with no
        // candidates degrades cleanly to "type the role yourself".
        KeyCode::Char(' ')
            if app.forms.host.focused_field.is_picker()
                && app.forms.host.focused_value().is_empty() =>
        {
            open_picker_for_focused_field(app);
        }
        KeyCode::Char(c) => {
            app.forms.host.insert_char(c);
            app.forms.host.update_hint();
        }
        KeyCode::Backspace => {
            app.forms.host.delete_char_before_cursor();
            app.forms.host.update_hint();
        }
        _ => {}
    }
}

/// If the alias field contains something like user@host:port, auto-parse and fill fields.
/// Also detects bare domains and IP addresses (e.g. "db.example.com", "192.168.1.1")
/// and moves them to the hostname field with a short alias derived from the first segment.
fn maybe_smart_paste(app: &mut App) {
    let alias_value = app.forms.host.alias.clone();
    if quick_add::looks_like_target(&alias_value) {
        if let Ok(parsed) = quick_add::parse_target(&alias_value) {
            // Only auto-fill if other fields are still at defaults
            if app.forms.host.hostname.is_empty() {
                app.forms.host.hostname = parsed.hostname.clone();
            }
            if app.forms.host.user.is_empty() && !parsed.user.is_empty() {
                app.forms.host.user = parsed.user;
            }
            if app.forms.host.port == "22" && parsed.port != 22 {
                app.forms.host.port = parsed.port.to_string();
            }
            // Generate a clean alias from the hostname
            let clean_alias = parsed
                .hostname
                .split('.')
                .next()
                .unwrap_or(&parsed.hostname)
                .to_string();
            app.forms.host.alias = clean_alias;
            app.notify(crate::messages::SMART_PARSED);
            log::debug!(
                "host_form: smart-paste parsed alias={} host={} user={} port={}",
                app.forms.host.alias,
                app.forms.host.hostname,
                app.forms.host.user,
                app.forms.host.port
            );
        }
        return;
    }

    // Detect bare domain or IP address in the alias field.
    // Must contain a dot, no interior whitespace, and only valid hostname
    // characters (alphanumeric, dot, hyphen, underscore). Colons are excluded
    // to avoid false positives on IPv6 notations like ::ffff:192.0.2.1.
    let trimmed = alias_value.trim();
    if trimmed.len() >= 4
        && trimmed.contains('.')
        && !trimmed.starts_with('.')
        && !trimmed.ends_with('.')
        && !trimmed.contains(char::is_whitespace)
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        && app.forms.host.hostname.is_empty()
    {
        // Copy the value to the Host field as a suggestion. The Name field
        // stays unchanged so the user keeps full control over the alias.
        app.forms.host.hostname = trimmed.to_string();
        app.notify(crate::messages::LOOKS_LIKE_ADDRESS);
        log::debug!("host_form: auto-suggest hostname={trimmed}");
    }
}

/// Open the picker overlay appropriate for the currently focused field.
///
/// Space activates picker fields. `VaultSsh` is special: when the host has
/// no role candidates (no provider configured a role) Space still inserts a
/// literal space so the user can type the role manually. Other picker
/// fields always open their picker.
fn open_picker_for_focused_field(app: &mut App) {
    use ratatui::widgets::ListState;
    match app.forms.host.focused_field {
        FormField::IdentityFile => {
            app.scan_keys();
            app.ui.show_key_picker = true;
            app.ui.key_picker_state = ListState::default();
            if !app.keys.is_empty() {
                app.ui.key_picker_state.select(Some(0));
            }
        }
        FormField::ProxyJump => {
            app.ui.show_proxyjump_picker = true;
            app.ui.proxyjump_picker_state = ListState::default();
            if let Some(idx) = app.proxyjump_first_host_index() {
                app.ui.proxyjump_picker_state.select(Some(idx));
            }
        }
        FormField::VaultSsh => {
            let candidates = app.vault_role_candidates();
            if candidates.is_empty() {
                // No candidates → fall through to literal-space insert so
                // the user can type the role manually. Picker opens only
                // when there is something to pick.
                app.forms.host.insert_char(' ');
                app.forms.host.update_hint();
            } else {
                app.ui.show_vault_role_picker = true;
                app.ui.vault_role_picker_state = ListState::default();
                app.ui.vault_role_picker_state.select(Some(0));
            }
        }
        FormField::AskPass => {
            app.ui.show_password_picker = true;
            app.ui.password_picker_state = ListState::default();
            app.ui.password_picker_state.select(Some(0));
        }
        // Defensive: only reached if `FormField::is_picker()` grows a new
        // variant without a matching arm here. Insert a literal space so
        // typing keeps working while the gap is fixed; debug builds panic
        // to surface the drift.
        other => {
            debug_assert!(
                false,
                "open_picker_for_focused_field has no arm for picker field {:?}",
                other
            );
            app.forms.host.insert_char(' ');
            app.forms.host.update_hint();
        }
    }
}

pub(super) fn submit_form(app: &mut App) {
    // Check for external config changes since form was opened
    if app.config_changed_since_form_open() {
        app.notify_warning(crate::messages::CONFIG_CHANGED_EXTERNALLY);
        return;
    }

    // Validate
    if let Err(msg) = app.forms.host.validate() {
        app.notify_error(msg);
        return;
    }

    // Track old askpass to detect keychain removal
    let old_askpass = match &app.screen {
        Screen::EditHost { alias } => app
            .hosts_state
            .list
            .iter()
            .find(|h| h.alias == *alias)
            .and_then(|h| h.askpass.clone()),
        _ => None,
    };

    let result = match &app.screen {
        Screen::AddHost => app.add_host_from_form(),
        Screen::EditHost { alias } => {
            let old = alias.clone();
            app.edit_host_from_form(&old)
        }
        _ => return,
    };
    match result {
        Ok(msg) => {
            // Clear undo buffer after successful write
            app.hosts_state.undo_stack.clear();
            // Handle keychain changes on edit
            let mut final_msg = msg;
            if old_askpass.as_deref() == Some("keychain") {
                if app.forms.host.askpass != "keychain" {
                    // Source changed away from keychain — remove old entry
                    if let Screen::EditHost { ref alias } = app.screen {
                        let _ = crate::askpass::remove_from_keychain(alias);
                    }
                    final_msg = format!("{}. Keychain entry removed.", final_msg);
                } else if let Screen::EditHost { ref alias } = app.screen {
                    // Alias renamed — migrate keychain entry
                    if *alias != app.forms.host.alias {
                        if let Ok(pw) = crate::askpass::retrieve_keychain_password(alias) {
                            if crate::askpass::store_in_keychain(&app.forms.host.alias, &pw).is_ok()
                            {
                                let _ = crate::askpass::remove_from_keychain(alias);
                            }
                        }
                    }
                }
            }
            // Drain any side-channel cleanup warning produced during the
            // mutation. When set, it overrides the success message because
            // the user needs to see that something on disk failed.
            if let Some(warning) = app.vault.cleanup_warning.take() {
                app.notify_error(warning);
            } else {
                app.notify(final_msg);
            }
        }
        Err(msg) => {
            app.notify_error(msg);
            return;
        }
    }

    let target_alias = app.forms.host.alias.trim().to_string();
    // Editing a stale host means the user asserts it is still wanted
    if let Screen::EditHost { ref alias } = app.screen {
        app.hosts_state.ssh_config.clear_host_stale(alias);
        // If alias was renamed, also clear on the new alias
        if *alias != target_alias {
            app.hosts_state.ssh_config.clear_host_stale(&target_alias);
        }
    }
    app.clear_form_mtime();
    app.forms.host_baseline = None;
    app.set_screen(Screen::HostList);
    app.select_host_by_alias(&target_alias);
    app.flush_pending_vault_write();
}
