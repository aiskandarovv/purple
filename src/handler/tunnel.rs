use crossterm::event::{KeyCode, KeyEvent};
use log::{debug, info};

use crate::app::{App, Screen};

pub(super) fn handle_tunnel_list(app: &mut App, key: KeyEvent) {
    let alias = match &app.screen {
        Screen::TunnelList { alias } => alias.clone(),
        _ => return,
    };

    // Handle pending tunnel delete confirmation first
    if app.tunnels.pending_delete.is_some() && key.code != KeyCode::Char('?') {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let Some(sel) = app.tunnels.pending_delete.take() else {
                    return;
                };
                if let Some(rule) = app.tunnels.list.get(sel) {
                    let key = rule.tunnel_type.directive_key().to_string();
                    let value = rule.to_directive_value();
                    let config_backup = app.hosts_state.ssh_config.clone();
                    if !app
                        .hosts_state
                        .ssh_config
                        .remove_forward(&alias, &key, &value)
                    {
                        app.notify_warning(crate::messages::TUNNEL_NOT_FOUND);
                        return;
                    }
                    if let Err(e) = app.hosts_state.ssh_config.write() {
                        app.hosts_state.ssh_config = config_backup;
                        app.notify_error(crate::messages::failed_to_save(&e));
                    } else {
                        app.update_last_modified();
                        app.refresh_tunnel_list(&alias);
                        app.reload_hosts();
                        if app.tunnels.list.is_empty() {
                            app.ui.tunnel_list_state.select(None);
                        } else if sel >= app.tunnels.list.len() {
                            app.ui
                                .tunnel_list_state
                                .select(Some(app.tunnels.list.len() - 1));
                        }
                        app.notify(crate::messages::TUNNEL_REMOVED);
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.tunnels.pending_delete = None;
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.set_screen(Screen::HostList);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_tunnel();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_tunnel();
        }
        KeyCode::PageDown => {
            crate::app::page_down(&mut app.ui.tunnel_list_state, app.tunnels.list.len(), 10);
        }
        KeyCode::PageUp => {
            crate::app::page_up(&mut app.ui.tunnel_list_state, app.tunnels.list.len(), 10);
        }
        KeyCode::Char('a') => {
            // Check if host is from an included file (read-only)
            if let Some(host) = app.hosts_state.list.iter().find(|h| h.alias == alias) {
                if host.source_file.is_some() {
                    app.notify_warning(crate::messages::TUNNEL_INCLUDED_READ_ONLY);
                    return;
                }
            }
            app.tunnels.form = crate::app::TunnelForm::new();
            app.set_screen(Screen::TunnelForm {
                alias: alias.clone(),
                editing: None,
            });
            app.capture_form_mtime();
            app.capture_tunnel_form_baseline();
        }
        KeyCode::Char('e') => {
            // Check if host is from an included file (read-only)
            if let Some(host) = app.hosts_state.list.iter().find(|h| h.alias == alias) {
                if host.source_file.is_some() {
                    app.notify_warning(crate::messages::TUNNEL_INCLUDED_READ_ONLY);
                    return;
                }
            }
            if let Some(sel) = app.ui.tunnel_list_state.selected() {
                if let Some(rule) = app.tunnels.list.get(sel) {
                    app.tunnels.form = crate::app::TunnelForm::from_rule(rule);
                    app.set_screen(Screen::TunnelForm {
                        alias: alias.clone(),
                        editing: Some(sel),
                    });
                    app.capture_form_mtime();
                    app.capture_tunnel_form_baseline();
                }
            }
        }
        KeyCode::Char('d') => {
            // Check if host is from an included file (read-only)
            if let Some(host) = app.hosts_state.list.iter().find(|h| h.alias == alias) {
                if host.source_file.is_some() {
                    app.notify_warning(crate::messages::TUNNEL_INCLUDED_READ_ONLY);
                    return;
                }
            }
            if let Some(sel) = app.ui.tunnel_list_state.selected() {
                if sel < app.tunnels.list.len() {
                    app.tunnels.pending_delete = Some(sel);
                }
            }
        }
        KeyCode::Enter => {
            // Start/stop tunnel
            if app.tunnels.active.contains_key(&alias) {
                // Stop
                if let Some(mut tunnel) = app.tunnels.active.remove(&alias) {
                    if let Err(e) = tunnel.child.kill() {
                        debug!("[external] Failed to kill tunnel process for {alias}: {e}");
                    }
                    let _ = tunnel.child.wait();
                    app.notify(crate::messages::tunnel_stopped(&alias));
                }
            } else if !app.tunnels.list.is_empty() {
                // Start
                if app.demo_mode {
                    app.notify(crate::messages::DEMO_TUNNELS_DISABLED);
                    return;
                }
                let askpass = app
                    .hosts_state
                    .list
                    .iter()
                    .find(|h| h.alias == alias)
                    .and_then(|h| h.askpass.clone());
                match crate::tunnel::start_tunnel(
                    &alias,
                    &app.reload.config_path,
                    askpass.as_deref(),
                    app.bw_session.as_deref(),
                ) {
                    Ok(child) => {
                        for rule in &app.tunnels.list {
                            info!(
                                "Tunnel started: type={} local={} remote={}:{} alias={alias}",
                                rule.tunnel_type.label(),
                                rule.bind_port,
                                rule.remote_host,
                                rule.remote_port
                            );
                        }
                        app.tunnels
                            .active
                            .insert(alias.clone(), crate::tunnel::ActiveTunnel { child });
                        app.notify(crate::messages::tunnel_started(&alias));
                    }
                    Err(e) => {
                        app.notify_error(crate::messages::tunnel_start_failed(&e));
                    }
                }
            }
        }
        KeyCode::Char('?') => {
            let old = std::mem::replace(&mut app.screen, Screen::HostList);
            app.set_screen(Screen::Help {
                return_screen: Box::new(old),
            });
        }
        _ => {}
    }
}

pub(super) fn handle_tunnel_form(app: &mut App, key: KeyEvent) {
    let (alias, editing) = match &app.screen {
        Screen::TunnelForm { alias, editing } => (alias.clone(), *editing),
        _ => return,
    };

    // Handle discard confirmation dialog
    if app.forms.pending_discard_confirm {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.forms.pending_discard_confirm = false;
                app.clear_form_mtime();
                app.tunnels.form_baseline = None;
                app.set_screen(Screen::TunnelList { alias });
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
            if app.tunnel_form_is_dirty() {
                app.forms.pending_discard_confirm = true;
            } else {
                app.clear_form_mtime();
                app.tunnels.form_baseline = None;
                app.set_screen(Screen::TunnelList { alias });
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            app.tunnels.form.focused_field = app
                .tunnels
                .form
                .focused_field
                .next(app.tunnels.form.tunnel_type);
            app.tunnels.form.sync_cursor_to_end();
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.tunnels.form.focused_field = app
                .tunnels
                .form
                .focused_field
                .prev(app.tunnels.form.tunnel_type);
            app.tunnels.form.sync_cursor_to_end();
        }
        KeyCode::Left if app.tunnels.form.cursor_pos > 0 => {
            app.tunnels.form.cursor_pos -= 1;
        }
        KeyCode::Right => {
            let len = app
                .tunnels
                .form
                .focused_value()
                .map(|v| v.chars().count())
                .unwrap_or(0);
            if app.tunnels.form.cursor_pos < len {
                app.tunnels.form.cursor_pos += 1;
            }
        }
        KeyCode::Home => {
            app.tunnels.form.cursor_pos = 0;
        }
        KeyCode::End => {
            app.tunnels.form.sync_cursor_to_end();
        }
        KeyCode::Enter => {
            submit_tunnel_form(app, &alias, editing);
        }
        KeyCode::Char(' ')
            if app.tunnels.form.focused_field == crate::app::TunnelFormField::Type =>
        {
            app.tunnels.form.tunnel_type = app.tunnels.form.tunnel_type.next();
        }
        KeyCode::Char(c) => {
            app.tunnels.form.insert_char(c);
        }
        KeyCode::Backspace => {
            app.tunnels.form.delete_char_before_cursor();
        }
        _ => {}
    }
}

fn submit_tunnel_form(app: &mut App, alias: &str, editing: Option<usize>) {
    // Check for external config changes since form was opened
    if app.config_changed_since_form_open() {
        app.notify_warning(crate::messages::CONFIG_CHANGED_EXTERNALLY);
        return;
    }

    if let Err(msg) = app.tunnels.form.validate() {
        app.notify_error(msg);
        return;
    }

    let (directive_key, directive_value) = app.tunnels.form.to_directive();
    let config_backup = app.hosts_state.ssh_config.clone();

    // If editing, remove the old directive first
    if let Some(idx) = editing {
        if let Some(old_rule) = app.tunnels.list.get(idx) {
            let old_key = old_rule.tunnel_type.directive_key().to_string();
            let old_value = old_rule.to_directive_value();
            if !app
                .hosts_state
                .ssh_config
                .remove_forward(alias, &old_key, &old_value)
            {
                app.hosts_state.ssh_config = config_backup;
                app.notify_warning(crate::messages::TUNNEL_ORIGINAL_NOT_FOUND);
                return;
            }
        } else {
            // Index out of bounds (external config change) — abort
            app.notify_warning(crate::messages::TUNNEL_LIST_CHANGED);
            return;
        }
    }

    // Duplicate detection (runs after old directive removal for edits)
    if app
        .hosts_state
        .ssh_config
        .has_forward(alias, directive_key, &directive_value)
    {
        app.hosts_state.ssh_config = config_backup;
        app.notify_warning(crate::messages::TUNNEL_DUPLICATE);
        return;
    }

    app.hosts_state
        .ssh_config
        .add_forward(alias, directive_key, &directive_value);
    if let Err(e) = app.hosts_state.ssh_config.write() {
        app.hosts_state.ssh_config = config_backup;
        app.notify_error(crate::messages::failed_to_save(&e));
        return;
    }

    app.hosts_state.undo_stack.clear(); // Clear undo buffer — positions may have shifted
    app.update_last_modified();
    app.refresh_tunnel_list(alias);
    app.reload_hosts();
    // Fix selection after list change
    if app.tunnels.list.is_empty() {
        app.ui.tunnel_list_state.select(None);
    } else if let Some(sel) = app.ui.tunnel_list_state.selected() {
        if sel >= app.tunnels.list.len() {
            app.ui
                .tunnel_list_state
                .select(Some(app.tunnels.list.len() - 1));
        }
    } else {
        // First tunnel added to empty list — initialize selection
        app.ui.tunnel_list_state.select(Some(0));
    }
    app.clear_form_mtime();
    app.tunnels.form_baseline = None;
    app.notify(crate::messages::TUNNEL_SAVED);
    app.set_screen(Screen::TunnelList {
        alias: alias.to_string(),
    });
}
