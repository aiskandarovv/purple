//! Sub-handlers for the largest key actions in `handle_host_list`.
//!
//! Extracted from the main key dispatcher so the parent function stays below
//! the project file-size limit. Each function corresponds to one key press
//! and owns the full side-effect flow (status updates, state transitions,
//! thread spawning).

use std::sync::mpsc;

use crate::app::{App, HostForm, Screen};
use crate::event::AppEvent;

/// `c` — duplicate the selected host or pattern into a new AddHost form.
pub(super) fn clone_selected(app: &mut App) {
    if let Some(pattern) = app.selected_pattern() {
        if pattern.source_file.is_some() {
            app.notify_error(crate::messages::included_file_clone(&pattern.pattern));
            return;
        }
        let mut form = HostForm::from_pattern_entry(pattern);
        form.alias.clear();
        form.cursor_pos = 0;
        app.forms.host = form;
        app.set_screen(Screen::AddHost);
        app.capture_form_mtime();
        app.capture_form_baseline();
        return;
    }

    if let Some(host) = app.selected_host() {
        if let Some(ref source) = host.source_file {
            let alias = host.alias.clone();
            let path = source.display();
            app.notify_warning(crate::messages::included_host_clone_there(&alias, &path));
            return;
        }
        let stale_hint = if host.stale.is_some() {
            Some(crate::handler::stale_provider_hint(host))
        } else {
            None
        };
        let copy_alias = format!("{}-copy", host.alias);
        // Clone uses the enriched entry (with inheritance) so the copy is
        // self-contained. from_entry_duplicate clears vault_ssh so the copy
        // does not inherit a per-host override tied to the original alias's
        // certificate.
        let (mut form, vault_cleared) = HostForm::from_entry_duplicate(host, Default::default());
        form.alias = copy_alias;
        form.cursor_pos = form.alias.chars().count();
        if let Some(hint) = stale_hint {
            app.notify_warning(crate::messages::stale_host(&hint));
        } else if vault_cleared {
            app.notify(crate::messages::CLONED_VAULT_CLEARED);
        }
        app.forms.host = form;
        app.set_screen(Screen::AddHost);
        app.capture_form_mtime();
        app.capture_form_baseline();
    }
}

/// `V` — collect all hosts with a Vault SSH role, filter the ones that need
/// renewal, and transition to the bulk-sign confirmation screen. Cancels an
/// in-progress signing thread if one is already running.
pub(super) fn initiate_bulk_vault_sign(app: &mut App) {
    if !app.has_any_vault_role() {
        app.notify(crate::messages::VAULT_NO_ROLE_CONFIGURED);
        return;
    }
    if app.demo_mode {
        app.notify(crate::messages::DEMO_VAULT_SIGNING_DISABLED);
        return;
    }
    // Cancel any in-progress vault signing thread
    if let Some(ref cancel) = app.vault.signing_cancel {
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        app.vault.signing_cancel = None;
        app.notify(crate::messages::VAULT_SIGNING_CANCELLED);
        return;
    }
    let provider_config = crate::providers::config::ProviderConfig::load();
    let entries = app.hosts_state.ssh_config.host_entries();
    let mut signable: Vec<(String, String, String, std::path::PathBuf, Option<String>)> =
        Vec::new();
    let mut pubkey_error: Option<String> = None;
    for e in &entries {
        let Some(role) = crate::vault_ssh::resolve_vault_role(
            e.vault_ssh.as_deref(),
            e.provider.as_deref(),
            &provider_config,
        ) else {
            continue;
        };
        let vault_addr = crate::vault_ssh::resolve_vault_addr(
            e.vault_addr.as_deref(),
            e.provider.as_deref(),
            &provider_config,
        );
        match crate::vault_ssh::resolve_pubkey_path(&e.identity_file) {
            Ok(pubkey) => signable.push((
                e.alias.clone(),
                role,
                e.certificate_file.clone(),
                pubkey,
                vault_addr,
            )),
            Err(err) => {
                if pubkey_error.is_none() {
                    pubkey_error = Some(err.to_string());
                }
            }
        }
    }
    if let Some(msg) = pubkey_error {
        app.notify_error(crate::messages::vault_error(&msg));
        return;
    }

    if signable.is_empty() {
        app.notify(crate::messages::VAULT_NO_HOSTS_WITH_ROLE);
        return;
    }

    // Pre-check: if any signable host has no resolved VAULT_ADDR and the
    // process env also has none, the vault CLI will fail with a cryptic
    // error only after the user confirms the dialog. Surface this upfront
    // with a clear, actionable message.
    let env_vault_addr = std::env::var("VAULT_ADDR").ok();
    let host_addrs: Vec<Option<&str>> = signable
        .iter()
        .map(|(_, _, _, _, a)| a.as_deref())
        .collect();
    if crate::handler::vault_addr_missing(&host_addrs, env_vault_addr.as_deref()) {
        app.notify_error(crate::messages::VAULT_NO_ADDRESS);
        return;
    }

    // Pre-filter to hosts that actually need renewal, so the confirm
    // dialog count matches what will actually be signed. Hosts with a
    // valid cached cert are skipped silently.
    let mut needs_signing: Vec<(String, String, String, std::path::PathBuf, Option<String>)> =
        Vec::with_capacity(signable.len());
    for entry in &signable {
        let (alias, _role, cert_file, _pubkey, _vault_addr) = entry;
        let check_path = match crate::vault_ssh::resolve_cert_path(alias, cert_file) {
            Ok(p) => p,
            Err(_) => {
                needs_signing.push(entry.clone());
                continue;
            }
        };
        let status = crate::vault_ssh::check_cert_validity(&check_path);
        if crate::vault_ssh::needs_renewal(&status) {
            needs_signing.push(entry.clone());
        }
    }

    if needs_signing.is_empty() {
        app.notify(crate::messages::VAULT_ALL_CERTS_VALID);
        return;
    }

    app.set_screen(Screen::ConfirmVaultSign {
        signable: needs_signing,
    });
}

/// `F` — open the file browser overlay for the selected host. Spawns a
/// background thread to fetch the remote home directory.
pub(super) fn open_file_browser(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.is_pattern_selected() {
        return;
    }
    if app.demo_mode {
        app.notify(crate::messages::DEMO_FILE_BROWSER_DISABLED);
        return;
    }
    let Some(host) = app.selected_host() else {
        return;
    };
    let stale_hint = if host.stale.is_some() {
        Some(crate::handler::stale_provider_hint(host))
    } else {
        None
    };
    let alias = host.alias.clone();
    let askpass = host.askpass.clone();
    if let Some(hint) = stale_hint {
        app.notify_warning(crate::messages::stale_host(&hint));
    }
    let has_tunnel = app.tunnels.active.contains_key(&alias);
    let (local_path, remote_path) =
        app.file_browser_paths
            .get(&alias)
            .cloned()
            .unwrap_or_else(|| {
                (
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/")),
                    String::new(),
                )
            });
    let (local_entries, local_error) = match crate::file_browser::list_local(
        &local_path,
        false,
        crate::file_browser::BrowserSort::Name,
    ) {
        Ok(entries) => (entries, None),
        Err(e) => (Vec::new(), Some(e.to_string())),
    };
    let mut local_list_state = ratatui::widgets::ListState::default();
    local_list_state.select(Some(0)); // Always select ".." entry
    let fb = crate::file_browser::FileBrowserState {
        alias: alias.clone(),
        askpass: askpass.clone(),
        active_pane: crate::file_browser::BrowserPane::Local,
        local_path,
        local_entries,
        local_list_state,
        local_selected: std::collections::HashSet::new(),
        local_error,
        remote_path: String::new(),
        remote_entries: Vec::new(),
        remote_list_state: ratatui::widgets::ListState::default(),
        remote_selected: std::collections::HashSet::new(),
        remote_error: None,
        remote_loading: true,
        show_hidden: false,
        sort: crate::file_browser::BrowserSort::Name,
        confirm_copy: None,
        transferring: None,
        transfer_error: None,
        connection_recorded: false,
    };
    app.file_browser = Some(fb);
    app.set_screen(Screen::FileBrowser {
        alias: alias.clone(),
    });
    // Fetch remote home dir in background
    let tx = events_tx.clone();
    let remote = remote_path;
    let ctx = crate::ssh_context::OwnedSshContext {
        alias: alias.clone(),
        config_path: app.reload.config_path.clone(),
        askpass,
        bw_session: app.bw_session.clone(),
        has_tunnel,
    };
    std::thread::spawn(move || {
        let home = if remote.is_empty() {
            match crate::file_browser::get_remote_home(
                &ctx.alias,
                &ctx.config_path,
                ctx.askpass.as_deref(),
                ctx.bw_session.as_deref(),
                ctx.has_tunnel,
            ) {
                Ok(h) => h,
                Err(e) => {
                    let _ = tx.send(crate::event::AppEvent::FileBrowserListing {
                        alias: ctx.alias,
                        path: String::new(),
                        entries: Err(e.to_string()),
                    });
                    return;
                }
            }
        } else {
            remote
        };
        crate::file_browser::spawn_remote_listing(
            ctx,
            home,
            false,
            crate::file_browser::BrowserSort::Name,
            super::super::file_browser::fb_send(tx),
        );
    });
}

/// `C` — open the container overlay for the selected host. Spawns a
/// background listing thread unless the app is in demo mode.
pub(super) fn open_container_overlay(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.is_pattern_selected() {
        return;
    }
    let Some(host) = app.selected_host() else {
        return;
    };
    let stale_hint = if host.stale.is_some() {
        Some(crate::handler::stale_provider_hint(host))
    } else {
        None
    };
    let alias = host.alias.clone();
    let askpass = host.askpass.clone();
    if let Some(hint) = stale_hint {
        app.notify_warning(crate::messages::stale_host(&hint));
    }
    let (cached_runtime, cached_containers) = if let Some(entry) = app.container_cache.get(&alias) {
        (Some(entry.runtime), entry.containers.clone())
    } else {
        (None, Vec::new())
    };
    let mut list_state = ratatui::widgets::ListState::default();
    if !cached_containers.is_empty() {
        list_state.select(Some(0));
    }
    app.container_state = Some(crate::app::ContainerState {
        alias: alias.clone(),
        askpass: askpass.clone(),
        runtime: cached_runtime,
        containers: cached_containers,
        list_state,
        loading: !app.demo_mode,
        error: None,
        action_in_progress: None,
        confirm_action: None,
    });
    app.set_screen(Screen::Containers {
        alias: alias.clone(),
    });
    if !app.demo_mode {
        let has_tunnel = app.tunnels.active.contains_key(&alias);
        let ctx = crate::ssh_context::OwnedSshContext {
            alias,
            config_path: app.reload.config_path.clone(),
            askpass,
            bw_session: app.bw_session.clone(),
            has_tunnel,
        };
        let tx = events_tx.clone();
        crate::containers::spawn_container_listing(ctx, cached_runtime, move |a, result| {
            let _ = tx.send(AppEvent::ContainerListing { alias: a, result });
        });
    }
}
