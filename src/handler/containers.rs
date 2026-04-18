use std::sync::mpsc;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Screen};
use crate::event::AppEvent;

pub(super) fn handle_containers(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    // Block all keys except the confirm-dialog contract (y/Y/n/N/Esc) and
    // `?` (help) when a confirmation is pending. Uniform with every other
    // confirm dialog in purple. `q` is intentionally NOT in the allowlist:
    // it belongs to browse-context cancel, not confirm-context.
    if let Some(ref state) = app.container_state {
        if state.confirm_action.is_some() {
            match key.code {
                KeyCode::Char('y')
                | KeyCode::Char('Y')
                | KeyCode::Char('n')
                | KeyCode::Char('N')
                | KeyCode::Esc
                | KeyCode::Char('?') => {}
                _ => return Ok(()),
            }
        }
    }

    // When a confirm is pending, n/N/Esc all cancel it (uniform with other
    // confirm dialogs). `q` was deliberately removed from the confirm-context
    // allowlist above, so it can never reach this point during a confirm.
    if let Some(ref mut state) = app.container_state {
        if state.confirm_action.is_some()
            && matches!(
                key.code,
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc
            )
        {
            state.confirm_action = None;
            return Ok(());
        }
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            // No confirm pending (the early-return above handles that case):
            // close the overlay.
            app.container_state = None;
            app.set_screen(Screen::HostList);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(ref mut state) = app.container_state {
                let len = state.containers.len();
                if len > 0 {
                    let i = state.list_state.selected().unwrap_or(0);
                    state
                        .list_state
                        .select(Some(if i == 0 { len - 1 } else { i - 1 }));
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(ref mut state) = app.container_state {
                let len = state.containers.len();
                if len > 0 {
                    let i = state.list_state.selected().unwrap_or(0);
                    state
                        .list_state
                        .select(Some(if i + 1 >= len { 0 } else { i + 1 }));
                }
            }
        }
        KeyCode::PageDown => {
            if let Some(ref mut state) = app.container_state {
                let len = state.containers.len();
                if len > 0 {
                    let i = state.list_state.selected().unwrap_or(0);
                    state.list_state.select(Some((i + 10).min(len - 1)));
                }
            }
        }
        KeyCode::PageUp => {
            if let Some(ref mut state) = app.container_state {
                let len = state.containers.len();
                if len > 0 {
                    let i = state.list_state.selected().unwrap_or(0);
                    state.list_state.select(Some(i.saturating_sub(10)));
                }
            }
        }
        KeyCode::Char('s') => {
            container_action(app, events_tx, crate::containers::ContainerAction::Start);
        }
        KeyCode::Char('x') => {
            // Stop requires confirmation
            if let Some(ref mut state) = app.container_state {
                if state.action_in_progress.is_some() || state.confirm_action.is_some() {
                    return Ok(());
                }
                if let Some(idx) = state.list_state.selected() {
                    if let Some(container) = state.containers.get(idx) {
                        state.confirm_action = Some((
                            crate::containers::ContainerAction::Stop,
                            container.names.clone(),
                            container.id.clone(),
                        ));
                    }
                }
            }
        }
        KeyCode::Char('r') => {
            // Restart requires confirmation
            if let Some(ref mut state) = app.container_state {
                if state.action_in_progress.is_some() || state.confirm_action.is_some() {
                    return Ok(());
                }
                if let Some(idx) = state.list_state.selected() {
                    if let Some(container) = state.containers.get(idx) {
                        state.confirm_action = Some((
                            crate::containers::ContainerAction::Restart,
                            container.names.clone(),
                            container.id.clone(),
                        ));
                    }
                }
            }
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            // Confirm pending action
            if let Some(ref mut state) = app.container_state {
                if let Some((action, _name, _id)) = state.confirm_action.take() {
                    container_action(app, events_tx, action);
                }
            }
        }
        KeyCode::Char('R') => {
            // Refresh container list
            if app.demo_mode {
                app.notify(crate::messages::DEMO_CONTAINER_REFRESH_DISABLED);
                return Ok(());
            }
            if let Some(ref mut state) = app.container_state {
                if state.action_in_progress.is_some() {
                    return Ok(());
                }
                state.loading = true;
                state.error = None;
                let alias = state.alias.clone();
                let cached_runtime = state.runtime;
                let ctx = crate::ssh_context::OwnedSshContext {
                    alias: alias.clone(),
                    config_path: app.reload.config_path.clone(),
                    askpass: state.askpass.clone(),
                    bw_session: app.bw_session.clone(),
                    has_tunnel: app.tunnels.active.contains_key(&alias),
                };
                let tx = events_tx.clone();
                crate::containers::spawn_container_listing(
                    ctx,
                    cached_runtime,
                    move |a, result| {
                        let _ = tx.send(AppEvent::ContainerListing { alias: a, result });
                    },
                );
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
    Ok(())
}

fn container_action(
    app: &mut App,
    events_tx: &mpsc::Sender<AppEvent>,
    action: crate::containers::ContainerAction,
) {
    let Some(ref mut state) = app.container_state else {
        return;
    };
    if state.action_in_progress.is_some() {
        return;
    }
    let Some(idx) = state.list_state.selected() else {
        return;
    };
    let Some(container) = state.containers.get(idx) else {
        return;
    };
    if crate::containers::validate_container_id(&container.id).is_err() {
        return;
    }
    if app.demo_mode {
        app.notify(crate::messages::DEMO_CONTAINER_ACTIONS_DISABLED);
        return;
    }
    let Some(runtime) = state.runtime else {
        return;
    };
    let container_id = container.id.clone();
    let container_name = container.names.clone();
    state.action_in_progress = Some(format!("{} {}...", action.as_str(), container_name));
    let alias = state.alias.clone();
    let ctx = crate::ssh_context::OwnedSshContext {
        alias: alias.clone(),
        config_path: app.reload.config_path.clone(),
        askpass: state.askpass.clone(),
        bw_session: app.bw_session.clone(),
        has_tunnel: app.tunnels.active.contains_key(&alias),
    };
    let tx = events_tx.clone();
    crate::containers::spawn_container_action(
        ctx,
        runtime,
        action,
        container_id,
        move |a, act, result| {
            let _ = tx.send(AppEvent::ContainerActionComplete {
                alias: a,
                action: act,
                result,
            });
        },
    );
}
