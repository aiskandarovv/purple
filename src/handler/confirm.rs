use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use crossterm::event::KeyEvent;

use crate::app::{App, Screen};
use crate::event::AppEvent;

pub(super) fn handle_confirm_delete(app: &mut App, key: KeyEvent) {
    // Use the central confirm-key router so the y/n/Esc contract is uniform
    // across all confirm dialogs.
    match super::route_confirm_key(key) {
        super::ConfirmAction::Yes => {
            if let Screen::ConfirmDelete { ref alias } = app.screen {
                let alias = alias.clone();
                if let Some((element, position)) = app.config.delete_host_undoable(&alias) {
                    if let Err(e) = app.config.write() {
                        // Restore the element on write failure
                        app.config.insert_host_at(element, position);
                        app.notify_error(crate::messages::failed_to_save(&e));
                    } else {
                        // Stop active tunnel for the deleted host
                        if let Some(mut tunnel) = app.active_tunnels.remove(&alias) {
                            let _ = tunnel.child.kill();
                            let _ = tunnel.child.wait();
                        }
                        // Clean up cert file if it exists. NotFound is the
                        // expected case for hosts that never had a cert. Other
                        // errors are surfaced via the status bar (never via
                        // eprintln, which would corrupt the ratatui screen).
                        let mut cert_cleanup_warning: Option<String> = None;
                        if !crate::demo_flag::is_demo() {
                            if let Ok(cert_path) = crate::vault_ssh::cert_path_for(&alias) {
                                match std::fs::remove_file(&cert_path) {
                                    Ok(()) => {}
                                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                                    Err(e) => {
                                        cert_cleanup_warning =
                                            Some(crate::messages::cert_cleanup_warning(
                                                &cert_path.display(),
                                                &e,
                                            ));
                                    }
                                }
                            }
                        }
                        app.undo_stack
                            .push(crate::app::DeletedHost { element, position });
                        if app.undo_stack.len() > 50 {
                            app.undo_stack.remove(0);
                        }
                        app.update_last_modified();
                        app.reload_hosts();
                        if let Some(warning) = cert_cleanup_warning {
                            app.notify_error(warning);
                        } else {
                            app.notify(crate::messages::goodbye_host(&alias));
                        }
                    }
                } else {
                    app.notify_warning(crate::messages::host_not_found(&alias));
                }
            }
            app.set_screen(Screen::HostList);
        }
        super::ConfirmAction::No => {
            app.set_screen(Screen::HostList);
        }
        super::ConfirmAction::Ignored => {}
    }
}

pub(super) fn handle_confirm_vault_sign(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    // Vault Sign is a destructive/material action: signing N certificates
    // hits Vault, may take time and is hard to reverse. Stray keys must NOT
    // cancel — use `route_confirm_key` so only y/Y/n/N/Esc are honored.
    // History: an earlier `_ => app.screen = Screen::HostList` catch-all
    // could be triggered by any keypress next to `y` (e.g. fat-fingered
    // `t` or `u`), silently aborting a bulk sign.
    match super::route_confirm_key(key) {
        super::ConfirmAction::Yes => {
            // Extract the precomputed signable list, then transition back to
            // the host list and kick off the background signing loop.
            let signable = if let Screen::ConfirmVaultSign { signable } = &app.screen {
                signable.clone()
            } else {
                return;
            };
            app.set_screen(Screen::HostList);
            start_vault_bulk_sign(app, signable, events_tx);
        }
        super::ConfirmAction::No => {
            app.set_screen(Screen::HostList);
        }
        super::ConfirmAction::Ignored => {}
    }
}

/// Start the background vault bulk sign loop with fast-fail, progress, TOCTOU
/// coordination and cancellation. Stores the JoinHandle on App for clean exit.
fn start_vault_bulk_sign(
    app: &mut App,
    signable: Vec<(String, String, String, std::path::PathBuf, Option<String>)>,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let total = signable.len();
    if total == 0 {
        return;
    }
    app.notify_progress(crate::messages::vault_signing_progress(
        crate::animation::SPINNER_FRAMES[0],
        0,
        total,
        "",
    ));

    let cancel = Arc::new(AtomicBool::new(false));
    app.vault.signing_cancel = Some(cancel.clone());

    let in_flight = app.vault.sign_in_flight.clone();
    let tx = events_tx.clone();
    let spawn_result = std::thread::Builder::new()
        .name("vault-bulk-sign".into())
        .spawn(move || {
            let mut signed = 0u32;
            let mut failed = 0u32;
            let mut skipped = 0u32;
            let mut consecutive_failures = 0usize;
            let mut first_error: Option<String> = None;
            let mut aborted_message: Option<String> = None;

            for (idx, (alias, role, cert_file, pubkey, vault_addr)) in signable.iter().enumerate()
            {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let done = idx + 1;

                // TOCTOU: skip host if another thread already has it in-flight.
                // Otherwise mark it in-flight for the duration of this iteration.
                {
                    // If the mutex is poisoned a worker thread panicked while holding
                    // the lock. Recover the inner value without clearing — clearing
                    // the whole set would make every in-flight alias simultaneously
                    // eligible for re-signing, risking duplicate cert writes.
                    let mut set = match in_flight.lock() {
                        Ok(g) => g,
                        Err(p) => p.into_inner(),
                    };
                    if !set.insert(alias.clone()) {
                        skipped += 1;
                        let _ = tx.send(AppEvent::VaultSignProgress {
                            alias: alias.clone(),
                            done,
                            total,
                        });
                        continue;
                    }
                }

                let _ = tx.send(AppEvent::VaultSignProgress {
                    alias: alias.clone(),
                    done,
                    total,
                });

                let cert_path = match crate::vault_ssh::resolve_cert_path(alias, cert_file) {
                    Ok(p) => p,
                    Err(e) => {
                        failed += 1;
                        consecutive_failures += 1;
                        let scrubbed = crate::vault_ssh::scrub_vault_stderr(&e.to_string());
                        if first_error.is_none() {
                            first_error = Some(scrubbed);
                        }
                        remove_in_flight(&in_flight, alias);
                        if consecutive_failures >= 3 {
                            aborted_message = Some(format!(
                                "Vault SSH signing aborted after {} consecutive failures. Press V to retry. Last error: {}",
                                failed,
                                first_error.clone().unwrap_or_else(|| "unknown".into())
                            ));
                            break;
                        }
                        continue;
                    }
                };
                let status = crate::vault_ssh::check_cert_validity(&cert_path);
                if !crate::vault_ssh::needs_renewal(&status) {
                    skipped += 1;
                    consecutive_failures = 0;
                    remove_in_flight(&in_flight, alias);
                    continue;
                }

                let sign_result =
                    crate::vault_ssh::sign_certificate(role, pubkey, alias, vault_addr.as_deref());
                // Always clean up in_flight for this alias before handling the
                // result. Using a single cleanup point (rather than per-arm)
                // prevents orphaned aliases when new control flow is added.
                remove_in_flight(&in_flight, alias);
                match sign_result {
                    Ok(_) => {
                        let _ = tx.send(AppEvent::VaultSignResult {
                            alias: alias.clone(),
                            certificate_file: cert_file.clone(),
                            success: true,
                            message: String::new(),
                        });
                        signed += 1;
                        consecutive_failures = 0;
                    }
                    Err(e) => {
                        let raw = e.to_string();
                        let scrubbed = crate::vault_ssh::scrub_vault_stderr(&raw);
                        if first_error.is_none() {
                            first_error = Some(scrubbed.clone());
                        }
                        let _ = tx.send(AppEvent::VaultSignResult {
                            alias: alias.clone(),
                            certificate_file: cert_file.clone(),
                            success: false,
                            message: scrubbed,
                        });
                        failed += 1;
                        consecutive_failures += 1;
                        if consecutive_failures >= 3 {
                            aborted_message = Some(format!(
                                "Vault SSH signing aborted after {} consecutive failures. Press V to retry. Last error: {}",
                                failed,
                                first_error.clone().unwrap_or_else(|| "unknown".into())
                            ));
                            break;
                        }
                    }
                }
            }

            let cancelled = cancel.load(Ordering::Relaxed);
            let _ = tx.send(AppEvent::VaultSignAllDone {
                signed,
                failed,
                skipped,
                cancelled,
                aborted_message,
                first_error,
            });
        });
    match spawn_result {
        Ok(handle) => {
            app.vault.sign_thread = Some(handle);
        }
        Err(e) => {
            // Spawn failed (e.g. OS thread limit). Clear the cancel flag and
            // surface the error — otherwise the status bar is stuck at
            // "Signing 0/N" with no way for the user to recover.
            app.vault.signing_cancel = None;
            app.vault.sign_thread = None;
            app.notify_error(crate::messages::vault_spawn_failed(&e));
        }
    }
}

pub(super) fn remove_in_flight(
    set: &std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    alias: &str,
) {
    // On mutex poison, recover the inner value and remove only the target alias.
    // Do NOT clear the entire set — other in-flight aliases are still owned by
    // live worker iterations and clearing them would allow duplicate signs.
    let mut guard = match set.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.remove(alias);
}

pub(super) fn handle_confirm_host_key_reset(app: &mut App, key: KeyEvent) {
    // Host key reset wipes the host's known_hosts entry — uniform y/n/Esc
    // contract via the central router so stray keys cannot trigger it.
    match super::route_confirm_key(key) {
        super::ConfirmAction::Yes => {
            if let Screen::ConfirmHostKeyReset {
                ref alias,
                ref hostname,
                ref known_hosts_path,
                ref askpass,
            } = app.screen
            {
                let alias = alias.clone();
                let hostname = hostname.clone();
                let known_hosts_path = known_hosts_path.clone();
                let askpass = askpass.clone();

                let output = std::process::Command::new("ssh-keygen")
                    .arg("-R")
                    .arg(&hostname)
                    .arg("-f")
                    .arg(&known_hosts_path)
                    .output();

                match output {
                    Ok(result) if result.status.success() => {
                        app.notify(crate::messages::removed_host_key(&hostname));
                        if app.demo_mode {
                            app.notify(crate::messages::DEMO_CONNECTION_DISABLED);
                        } else {
                            app.pending_connect = Some((alias, askpass));
                        }
                    }
                    Ok(result) => {
                        let stderr = String::from_utf8_lossy(&result.stderr);
                        app.notify_error(crate::messages::host_key_remove_failed(stderr.trim()));
                    }
                    Err(e) => {
                        app.notify_error(crate::messages::ssh_keygen_failed(&e));
                    }
                }
            }
            app.set_screen(Screen::HostList);
        }
        super::ConfirmAction::No => {
            app.set_screen(Screen::HostList);
        }
        super::ConfirmAction::Ignored => {}
    }
}
