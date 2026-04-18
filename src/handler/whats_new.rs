use crossterm::event::{KeyCode, KeyEvent};
use log::debug;

use crate::app::{App, Screen};

pub(super) fn handle_whats_new(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('n') => close_and_mark_seen(app),
        KeyCode::Char('j') | KeyCode::Down => {
            if let Screen::WhatsNew(ref mut state) = app.screen {
                state.scroll = state.scroll.saturating_add(1);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Screen::WhatsNew(ref mut state) = app.screen {
                state.scroll = state.scroll.saturating_sub(1);
            }
        }
        KeyCode::PageDown => {
            if let Screen::WhatsNew(ref mut state) = app.screen {
                state.scroll = state.scroll.saturating_add(10);
            }
        }
        KeyCode::PageUp => {
            if let Screen::WhatsNew(ref mut state) = app.screen {
                state.scroll = state.scroll.saturating_sub(10);
            }
        }
        KeyCode::Char('g') | KeyCode::Home => {
            if let Screen::WhatsNew(ref mut state) = app.screen {
                state.scroll = 0;
            }
        }
        KeyCode::Char('G') | KeyCode::End => {
            if let Screen::WhatsNew(ref mut state) = app.screen {
                state.scroll = u16::MAX;
            }
        }
        _ => {}
    }
}

fn close_and_mark_seen(app: &mut App) {
    let version = env!("CARGO_PKG_VERSION");
    if let Err(e) = crate::preferences::save_last_seen_version(version) {
        log::warn!("[purple] failed to persist last_seen_version: {}", e);
    }
    dismiss_whats_new_toast(app);
    debug!("[purple] whats-new closed, marked seen={}", version);
    app.set_screen(Screen::HostList);
}

pub(super) fn dismiss_whats_new_toast(app: &mut App) {
    let fragment = crate::messages::whats_new_toast::INVITE_FRAGMENT;
    if let Some(ref t) = app.toast {
        if t.text.contains(fragment) {
            app.toast = app.toast_queue.pop_front();
        }
    }
    app.toast_queue.retain(|t| !t.text.contains(fragment));
}
