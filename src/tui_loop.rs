//! TUI event loop and the per-iteration helpers that drive it.
//!
//! Everything that runs while the TUI is on the alternate screen lives
//! here: the main `run_tui` orchestrator, its six tick-scoped helpers
//! (startup tasks, event dispatch, lazy cert check, pending SSH connect,
//! pending snippet run, teardown), plus Vault cert-cache helpers used by
//! the dispatch logic.

use anyhow::Result;

use crate::app::{self, App};
use crate::event::{self, AppEvent, EventHandler};
use crate::ssh_config::model::SshConfigFile;
use crate::{
    animation, askpass, connection, ensure_bw_session, ensure_keychain_password,
    ensure_vault_ssh_if_needed, first_launch_init, handler, import, ping, preferences, snippet,
    tui, update, vault_ssh,
};

pub(crate) fn run_tui(mut app: App) -> Result<()> {
    // First-launch welcome hint (one-shot: creates .purple/ so it won't show again)
    if app.status.is_none() && !app.demo_mode {
        if let Some(home) = dirs::home_dir() {
            let purple_dir = home.join(".purple");
            if let Some(has_backup) = first_launch_init(&purple_dir, &app.reload.config_path) {
                let host_count = app.hosts.len();
                let known_hosts_count = if host_count == 0 {
                    import::count_known_hosts_candidates()
                } else {
                    0
                };
                app.known_hosts_count = known_hosts_count;
                app.screen = app::Screen::Welcome {
                    has_backup,
                    host_count,
                    known_hosts_count,
                };
            }
        }
    }

    let mut terminal = tui::Tui::new()?;
    terminal.enter()?;
    let events = EventHandler::new(50);
    let events_tx = events.sender();
    let mut last_config_check = std::time::Instant::now();

    // Skip background tasks in demo mode (ping status is pre-populated).
    if !app.demo_mode {
        spawn_startup_tasks(&mut app, &events_tx);
    }

    let mut anim = animation::AnimationState::new();

    while app.running {
        anim.detect_transitions(&mut app);
        terminal.draw(&mut app, &mut anim)?;

        // During animation, use a short timeout for smooth frames (~60fps).
        // During ping checking, use 80ms timeout for spinner.
        // Otherwise, block until the next event arrives.
        let vault_signing = app.vault.signing_cancel.is_some();
        let event = if anim.is_animating(&app) {
            events.next_timeout(std::time::Duration::from_millis(16))?
        } else if anim.has_checking_hosts(&app) || vault_signing {
            events.next_timeout(std::time::Duration::from_millis(80))?
        } else {
            Some(events.next()?)
        };

        if dispatch_event(
            &mut app,
            event,
            &mut anim,
            vault_signing,
            &events_tx,
            &mut terminal,
            &mut last_config_check,
        )?
        .is_break()
        {
            continue;
        }

        lazy_cert_check(&mut app, &events_tx);

        handle_pending_connect(&mut app, &mut terminal, &events, &mut last_config_check)?;

        handle_pending_snippet(&mut app, &mut terminal, &events, &mut last_config_check)?;
    }

    tui_teardown(&mut app, &mut terminal)
}

/// Spawn auto-sync, auto-ping and the background version check on TUI startup.
fn spawn_startup_tasks(app: &mut App, events_tx: &std::sync::mpsc::Sender<AppEvent>) {
    for section in app.provider_config.configured_providers().to_vec() {
        if !section.auto_sync {
            continue;
        }
        if !app.syncing_providers.contains_key(&section.provider) {
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            app.syncing_providers
                .insert(section.provider.clone(), cancel.clone());
            handler::spawn_provider_sync(&section, events_tx.clone(), cancel);
        }
    }

    if app.ping.auto_ping {
        let hosts_to_ping: Vec<(String, String, u16)> = app
            .hosts
            .iter()
            .filter(|h| !h.hostname.is_empty() && h.proxy_jump.is_empty())
            .map(|h| (h.alias.clone(), h.hostname.clone(), h.port))
            .collect();
        for h in &app.hosts {
            if !h.proxy_jump.is_empty() {
                app.ping
                    .status
                    .insert(h.alias.clone(), app::PingStatus::Skipped);
            }
        }
        if !hosts_to_ping.is_empty() {
            for (alias, _, _) in &hosts_to_ping {
                app.ping
                    .status
                    .insert(alias.clone(), app::PingStatus::Checking);
            }
            ping::ping_all(&hosts_to_ping, events_tx.clone(), app.ping.generation);
        }
    }

    update::spawn_version_check(events_tx.clone());
}

/// Dispatch a single tick's event. Returns `Break` when the outer loop
/// should `continue` without running the post-dispatch helpers.
#[allow(clippy::too_many_arguments)]
fn dispatch_event(
    app: &mut App,
    event: Option<AppEvent>,
    anim: &mut animation::AnimationState,
    vault_signing: bool,
    events_tx: &std::sync::mpsc::Sender<AppEvent>,
    terminal: &mut tui::Tui,
    last_config_check: &mut std::time::Instant,
) -> Result<std::ops::ControlFlow<()>> {
    match event {
        Some(AppEvent::Key(key)) => {
            handler::handle_key_event(app, key, events_tx)?;
        }
        Some(AppEvent::Tick) | None => {
            handler::event_loop::handle_tick(app, anim, vault_signing, last_config_check);
        }
        Some(AppEvent::PingResult {
            alias,
            rtt_ms,
            generation,
        }) => {
            handler::event_loop::handle_ping_result(app, alias, rtt_ms, generation);
        }
        Some(AppEvent::SyncProgress { provider, message }) => {
            handler::event_loop::handle_sync_progress(app, provider, message);
        }
        Some(AppEvent::SyncComplete { provider, hosts }) => {
            handler::event_loop::handle_sync_complete(app, provider, hosts, last_config_check);
        }
        Some(AppEvent::SyncPartial {
            provider,
            hosts,
            failures,
            total,
        }) => {
            handler::event_loop::handle_sync_partial(
                app,
                provider,
                hosts,
                failures,
                total,
                last_config_check,
            );
        }
        Some(AppEvent::SyncError { provider, message }) => {
            handler::event_loop::handle_sync_error(app, provider, message, last_config_check);
        }
        Some(AppEvent::UpdateAvailable { version, headline }) => {
            handler::event_loop::handle_update_available(app, version, headline);
        }
        Some(AppEvent::FileBrowserListing {
            alias,
            path,
            entries,
        }) => {
            handler::event_loop::handle_file_browser_listing(app, alias, path, entries, terminal);
        }
        Some(AppEvent::ScpComplete {
            alias,
            success,
            message,
        }) => {
            handler::event_loop::handle_scp_complete(
                app, alias, success, message, events_tx, terminal,
            );
        }
        Some(AppEvent::SnippetHostDone {
            run_id,
            alias,
            stdout,
            stderr,
            exit_code,
        }) => {
            handler::event_loop::handle_snippet_host_done(
                app, run_id, alias, stdout, stderr, exit_code,
            );
        }
        Some(AppEvent::SnippetProgress {
            run_id,
            completed,
            total,
        }) => {
            handler::event_loop::handle_snippet_progress(app, run_id, completed, total);
        }
        Some(AppEvent::SnippetAllDone { run_id }) => {
            handler::event_loop::handle_snippet_all_done(app, run_id);
        }
        Some(AppEvent::ContainerListing { alias, result }) => {
            handler::event_loop::handle_container_listing(app, alias, result);
        }
        Some(AppEvent::ContainerActionComplete {
            alias,
            action,
            result,
        }) => {
            handler::event_loop::handle_container_action_complete(
                app, alias, action, result, events_tx,
            );
        }
        Some(AppEvent::VaultSignResult {
            alias,
            certificate_file: existing_cert_file,
            success,
            message,
        }) => {
            handler::event_loop::handle_vault_sign_result(
                app,
                alias,
                existing_cert_file,
                success,
                message,
            );
        }
        Some(AppEvent::VaultSignProgress { alias, done, total }) => {
            handler::event_loop::handle_vault_sign_progress(
                app,
                alias,
                done,
                total,
                anim.spinner_tick,
            );
        }
        Some(AppEvent::VaultSignAllDone {
            signed,
            failed,
            skipped,
            cancelled,
            aborted_message,
            first_error,
        }) => {
            if handler::event_loop::handle_vault_sign_all_done(
                app,
                signed,
                failed,
                skipped,
                cancelled,
                aborted_message,
                first_error,
            )
            .is_break()
            {
                return Ok(std::ops::ControlFlow::Break(()));
            }
        }
        Some(AppEvent::CertCheckResult { alias, status }) => {
            handler::event_loop::handle_cert_check_result(app, alias, status);
        }
        Some(AppEvent::CertCheckError { alias, message }) => {
            handler::event_loop::handle_cert_check_error(app, alias, message);
        }
        Some(AppEvent::PollError) => {
            app.running = false;
        }
    }
    Ok(std::ops::ControlFlow::Continue(()))
}

/// When the selected host has a vault role and the cached cert status is
/// missing, stale or has been touched externally, spawn a background check.
fn lazy_cert_check(app: &mut App, events_tx: &std::sync::mpsc::Sender<AppEvent>) {
    if let Some(selected) = app.selected_host() {
        if vault_ssh::resolve_vault_role(
            selected.vault_ssh.as_deref(),
            selected.provider.as_deref(),
            &app.provider_config,
        )
        .is_some()
        {
            // Stat the cert file once per iteration to detect external writes
            // (CLI sign, another purple instance) within one frame. Compared
            // against the mtime recorded when the cache entry was populated;
            // any mismatch forces a re-check, no matter the TTL.
            let current_mtime =
                vault_ssh::resolve_cert_path(&selected.alias, &selected.certificate_file)
                    .ok()
                    .and_then(|p| std::fs::metadata(&p).ok())
                    .and_then(|m| m.modified().ok());
            let cache_stale = cache_entry_is_stale(
                app.vault.cert_cache.get(&selected.alias),
                current_mtime,
                |t| t.elapsed().as_secs(),
            );

            let sign_in_flight = app
                .vault
                .sign_in_flight
                .lock()
                .map(|g| g.contains(&selected.alias))
                .unwrap_or(false);
            if cache_stale
                && !app.vault.cert_checks_in_flight.contains(&selected.alias)
                && !sign_in_flight
            {
                let alias = selected.alias.clone();
                let cert_file = selected.certificate_file.clone();
                app.vault.cert_checks_in_flight.insert(alias.clone());
                let tx = events_tx.clone();
                std::thread::spawn(move || {
                    let check_path = match vault_ssh::resolve_cert_path(&alias, &cert_file) {
                        Ok(p) => p,
                        Err(e) => {
                            let _ = tx.send(event::AppEvent::CertCheckError {
                                alias,
                                message: e.to_string(),
                            });
                            return;
                        }
                    };
                    let status = vault_ssh::check_cert_validity(&check_path);
                    let _ = tx.send(event::AppEvent::CertCheckResult { alias, status });
                });
            }
        }
    }
}

/// Drain any queued SSH connection request. In tmux mode we open a new
/// window and leave the TUI alive; otherwise we suspend the TUI, run ssh
/// inline, then restore it. Vault SSH signing and askpass pre-flight
/// (Bitwarden, keychain) run on the bare terminal to allow prompts.
fn handle_pending_connect(
    app: &mut App,
    terminal: &mut tui::Tui,
    events: &EventHandler,
    last_config_check: &mut std::time::Instant,
) -> Result<()> {
    let Some((alias, host_askpass)) = app.pending_connect.take() else {
        return Ok(());
    };
    let vault_host = app.hosts.iter().find(|h| h.alias == alias).cloned();
    let askpass = host_askpass.or_else(preferences::load_askpass_default);
    let has_active_tunnel = app.active_tunnels.contains_key(&alias);
    let use_tmux = connection::is_in_tmux() && askpass.is_none();

    if use_tmux {
        // Tmux mode: open SSH in a new tmux window. TUI stays alive.
        // Vault SSH cert signing runs first (eprintln warnings are harmless
        // on the alternate screen — ratatui repaints over them on the next
        // draw cycle).
        let vault_msg = if let Some(ref host) = vault_host {
            let msg =
                ensure_vault_ssh_if_needed(&alias, host, &app.provider_config, &mut app.config);
            if msg.is_some() {
                app.reload_hosts();
                app.refresh_cert_cache(&alias);
            }
            msg
        } else {
            None
        };

        match connection::connect_tmux_window(&alias, &app.reload.config_path, has_active_tunnel) {
            Ok(()) => {
                if let Some((ref msg, is_error)) = vault_msg {
                    if is_error {
                        app.notify_error(msg.clone());
                    } else {
                        app.notify(msg.clone());
                    }
                } else {
                    app.notify(crate::messages::opened_in_tmux(&alias));
                }
            }
            Err(e) => {
                app.notify_error(crate::messages::tmux_error(&e));
            }
        }
        return Ok(());
    }

    // Standard mode: suspend TUI, run SSH inline, restore TUI.
    // Order preserved: pause events, exit TUI, THEN run vault signing and
    // password setup (which may eprintln or prompt for input on the real
    // terminal).
    events.pause();
    terminal.exit()?;
    let vault_msg = if let Some(ref host) = vault_host {
        let msg = ensure_vault_ssh_if_needed(&alias, host, &app.provider_config, &mut app.config);
        if msg.is_some() {
            app.reload_hosts();
            app.refresh_cert_cache(&alias);
        }
        msg
    } else {
        None
    };
    if let Some(token) = ensure_bw_session(app.bw_session.as_deref(), askpass.as_deref()) {
        app.bw_session = Some(token);
    }
    ensure_keychain_password(&alias, askpass.as_deref());
    print!("{}", crate::messages::cli::beaming_up(&alias));
    let result = connection::connect(
        &alias,
        &app.reload.config_path,
        askpass.as_deref(),
        app.bw_session.as_deref(),
        has_active_tunnel,
    );
    println!();
    match &result {
        Ok(cr) => {
            let code = cr.status.code().unwrap_or(1);
            if code != 255 {
                app.history.record(&alias);
                app.host_list_cache.invalidate();
            }
            if code != 0 {
                if let Some((hostname, known_hosts_path)) =
                    connection::parse_host_key_error(&cr.stderr_output)
                {
                    app.screen = app::Screen::ConfirmHostKeyReset {
                        alias: alias.clone(),
                        hostname,
                        known_hosts_path,
                        askpass,
                    };
                } else {
                    let reason = connection::stderr_summary(&cr.stderr_output);
                    let msg = if let Some(reason) = reason {
                        format!("SSH to {} failed. {}", alias, reason)
                    } else {
                        format!("SSH to {} exited with code {}.", alias, code)
                    };
                    app.notify_error(msg);
                }
            } else if let Some((ref msg, is_error)) = vault_msg {
                if is_error {
                    app.notify_error(msg.clone());
                } else {
                    app.notify(msg.clone());
                }
            }
        }
        Err(e) => {
            eprintln!("Connection failed: {}", e);
            app.notify_error(crate::messages::connection_failed(&alias));
        }
    }
    askpass::cleanup_marker(&alias);
    terminal.enter()?;
    events.resume();
    *last_config_check = std::time::Instant::now();
    app.config = SshConfigFile::parse(&app.reload.config_path)?;
    app.reload_hosts();
    app.update_last_modified();
    Ok(())
}

/// Drain any queued snippet-run request: suspend the TUI, run the command
/// across all selected hosts, record history on success, wait for Enter,
/// then restore the TUI and reload the SSH config.
fn handle_pending_snippet(
    app: &mut App,
    terminal: &mut tui::Tui,
    events: &EventHandler,
    last_config_check: &mut std::time::Instant,
) -> Result<()> {
    let Some((snip, aliases)) = app.pending_snippet.take() else {
        return Ok(());
    };
    events.pause();
    terminal.exit()?;

    let multi = aliases.len() > 1;
    for alias in &aliases {
        let askpass = app
            .hosts
            .iter()
            .find(|h| h.alias == *alias)
            .and_then(|h| h.askpass.clone())
            .or_else(preferences::load_askpass_default);
        if let Some(token) = ensure_bw_session(app.bw_session.as_deref(), askpass.as_deref()) {
            app.bw_session = Some(token);
        }
        ensure_keychain_password(alias, askpass.as_deref());

        if multi {
            println!("{}", crate::messages::cli::host_separator(alias));
        } else {
            print!(
                "{}",
                crate::messages::cli::running_snippet_on(&snip.name, alias)
            );
        }
        let has_tunnel = app.active_tunnels.contains_key(alias);
        match snippet::run_snippet(
            alias,
            &app.reload.config_path,
            &snip.command,
            askpass.as_deref(),
            app.bw_session.as_deref(),
            false,
            has_tunnel,
        ) {
            Ok(r) => {
                if r.status.success() {
                    app.history.record(alias);
                    app.host_list_cache.invalidate();
                } else if multi {
                    eprintln!(
                        "{}",
                        crate::messages::cli::exited_with_code(r.status.code().unwrap_or(1))
                    );
                } else {
                    println!(
                        "\n{}",
                        crate::messages::cli::exited_with_code(r.status.code().unwrap_or(1))
                    );
                }
            }
            Err(e) => eprintln!("{}", crate::messages::cli::host_failed(alias, &e)),
        }
        if multi {
            println!();
        }
    }

    if !multi {
        println!("\n{}", crate::messages::cli::DONE);
    } else {
        println!(
            "{}",
            crate::messages::cli::done_multi(&snip.name, aliases.len())
        );
    }
    println!("\n{}", crate::messages::cli::PRESS_ENTER);
    let _ = std::io::stdin().read_line(&mut String::new());
    terminal.enter()?;
    events.resume();
    *last_config_check = std::time::Instant::now();
    // Reload so sort order (e.g. most recent) reflects the new history.
    app.config = SshConfigFile::parse(&app.reload.config_path)?;
    app.reload_hosts();
    app.update_last_modified();
    Ok(())
}

/// Flush any deferred vault-config writes, join the background signing
/// thread and kill active tunnels before leaving the TUI.
fn tui_teardown(app: &mut App, terminal: &mut tui::Tui) -> Result<()> {
    app.flush_pending_vault_write();

    if let Some(ref cancel) = app.vault.signing_cancel {
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if let Some(handle) = app.vault.sign_thread.take() {
        let _ = handle.join();
    }

    for (_, mut tunnel) in app.active_tunnels.drain() {
        let _ = tunnel.child.kill();
        let _ = tunnel.child.wait();
    }

    terminal.exit()?;
    Ok(())
}

pub(crate) fn current_cert_mtime(alias: &str, app: &app::App) -> Option<std::time::SystemTime> {
    let host = app.hosts.iter().find(|h| h.alias == alias)?;
    let cert_path = vault_ssh::resolve_cert_path(alias, &host.certificate_file).ok()?;
    std::fs::metadata(&cert_path)
        .ok()
        .and_then(|m| m.modified().ok())
}

/// Decide whether a `vault.cert_cache` entry should be re-checked.
///
/// Returns true when:
/// - there is no cached entry at all, or
/// - the cert file's current mtime differs from the cached mtime
///   (an external actor signed or deleted the cert behind our back), or
/// - the entry's age exceeds its TTL. `CertStatus::Invalid` uses a shorter
///   backoff so transient errors recover quickly without hammering the
///   background check thread on every poll tick.
///
/// The `elapsed_secs` closure is taken as a parameter so tests can inject
/// deterministic elapsed times instead of calling the real clock.
pub(crate) fn cache_entry_is_stale<F>(
    entry: Option<&(
        std::time::Instant,
        vault_ssh::CertStatus,
        Option<std::time::SystemTime>,
    )>,
    current_mtime: Option<std::time::SystemTime>,
    elapsed_secs: F,
) -> bool
where
    F: FnOnce(std::time::Instant) -> u64,
{
    let Some((checked_at, status, cached_mtime)) = entry else {
        return true;
    };
    if current_mtime != *cached_mtime {
        return true;
    }
    let ttl = if matches!(status, vault_ssh::CertStatus::Invalid(_)) {
        vault_ssh::CERT_ERROR_BACKOFF_SECS
    } else {
        vault_ssh::CERT_STATUS_CACHE_TTL_SECS
    };
    elapsed_secs(*checked_at) > ttl
}
