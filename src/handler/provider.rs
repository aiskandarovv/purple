use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, ProviderFormFields, Screen};
use crate::event::AppEvent;
use crate::providers;

mod region;

// Test-only: region_picker_rows is pub(crate) in region.rs but only re-exported
// here for handler::tests which validates the OVH endpoint picker row count.
// Production code calls handle_region_picker directly; it never needs the raw rows.
#[cfg(test)]
pub(super) use region::region_picker_rows;
pub(crate) use region::zone_data_for;

pub(super) fn handle_provider_list(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    // Handle pending provider delete confirmation first
    if app.pending_provider_delete.is_some() && key.code != KeyCode::Char('?') {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let Some(name) = app.pending_provider_delete.take() else {
                    return;
                };
                if let Some(old_section) = app.provider_config.section(name.as_str()).cloned() {
                    app.provider_config.remove_section(name.as_str());
                    if let Err(e) = app.provider_config.save() {
                        app.provider_config.set_section(old_section);
                        app.notify_error(crate::messages::failed_to_save(&e));
                    } else {
                        app.sync_history.remove(name.as_str());
                        crate::app::SyncRecord::save_all(&app.sync_history);
                        let display_name = crate::providers::provider_display_name(name.as_str());
                        app.notify(crate::messages::provider_removed(display_name));
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.pending_provider_delete = None;
            }
            _ => {}
        }
        return;
    }

    let provider_count = app.sorted_provider_names().len();
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            // Cancel all running syncs
            for cancel_flag in app.syncing_providers.values() {
                cancel_flag.store(true, Ordering::Relaxed);
            }
            app.screen = Screen::HostList;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            crate::app::cycle_selection(&mut app.ui.provider_list_state, provider_count, true);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            crate::app::cycle_selection(&mut app.ui.provider_list_state, provider_count, false);
        }
        KeyCode::PageDown => {
            crate::app::page_down(&mut app.ui.provider_list_state, provider_count, 10);
        }
        KeyCode::PageUp => {
            crate::app::page_up(&mut app.ui.provider_list_state, provider_count, 10);
        }
        KeyCode::Enter => {
            if let Some(index) = app.ui.provider_list_state.selected() {
                let sorted = app.sorted_provider_names();
                if let Some(name) = sorted.get(index) {
                    let provider_impl = providers::get_provider(name.as_str());
                    let short_label = provider_impl
                        .as_ref()
                        .map(|p| p.short_label().to_string())
                        .unwrap_or_else(|| name.clone());

                    // Pre-fill form from existing config or defaults
                    let first_field = crate::app::ProviderFormField::fields_for(name.as_str())[0];
                    app.provider_form = if let Some(section) =
                        app.provider_config.section(name.as_str())
                    {
                        let cursor_pos = match first_field {
                            crate::app::ProviderFormField::Url => section.url.chars().count(),
                            crate::app::ProviderFormField::Token => section.token.chars().count(),
                            _ => 0,
                        };
                        ProviderFormFields {
                            url: section.url.clone(),
                            token: section.token.clone(),
                            profile: section.profile.clone(),
                            project: section.project.clone(),
                            compartment: section.compartment.clone(),
                            regions: section.regions.clone(),
                            alias_prefix: section.alias_prefix.clone(),
                            user: section.user.clone(),
                            identity_file: section.identity_file.clone(),
                            verify_tls: section.verify_tls,
                            auto_sync: section.auto_sync,
                            vault_role: section.vault_role.clone(),
                            vault_addr: section.vault_addr.clone(),
                            focused_field: first_field,
                            cursor_pos,
                            expanded: true,
                        }
                    } else {
                        ProviderFormFields {
                            url: String::new(),
                            token: String::new(),
                            profile: String::new(),
                            project: String::new(),
                            compartment: String::new(),
                            regions: String::new(),
                            alias_prefix: short_label,
                            user: "root".to_string(),
                            identity_file: String::new(),
                            verify_tls: true,
                            auto_sync: !matches!(name.as_str(), "proxmox"),
                            vault_role: String::new(),
                            vault_addr: String::new(),
                            focused_field: first_field,
                            cursor_pos: 0,
                            expanded: false,
                        }
                    };
                    app.screen = Screen::ProviderForm {
                        provider: name.clone(),
                    };
                    app.capture_provider_form_mtime();
                    app.capture_provider_form_baseline();
                }
            }
        }
        KeyCode::Char('s') => {
            if app.demo_mode {
                app.notify(crate::messages::DEMO_SYNC_DISABLED);
                return;
            }
            if let Some(index) = app.ui.provider_list_state.selected() {
                let sorted = app.sorted_provider_names();
                if let Some(name) = sorted.get(index) {
                    if let Some(section) = app.provider_config.section(name.as_str()).cloned() {
                        if !app.syncing_providers.contains_key(name.as_str()) {
                            let cancel = Arc::new(AtomicBool::new(false));
                            app.syncing_providers.insert(name.clone(), cancel.clone());
                            let display_name =
                                crate::providers::provider_display_name(name.as_str());
                            app.notify_info(crate::messages::syncing_provider(display_name));
                            super::sync::spawn_provider_sync(&section, events_tx.clone(), cancel);
                        }
                    } else {
                        let display_name = crate::providers::provider_display_name(name.as_str());
                        app.notify_error(crate::messages::provider_configure_first(display_name));
                    }
                }
            }
        }
        KeyCode::Char('d') => {
            if let Some(index) = app.ui.provider_list_state.selected() {
                let sorted = app.sorted_provider_names();
                if let Some(name) = sorted.get(index) {
                    if app.provider_config.section(name.as_str()).is_some() {
                        app.pending_provider_delete = Some(name.clone());
                    } else {
                        let display_name = crate::providers::provider_display_name(name.as_str());
                        app.notify(crate::messages::provider_not_configured(display_name));
                    }
                }
            }
        }
        KeyCode::Char('?') => {
            let old = std::mem::replace(&mut app.screen, Screen::HostList);
            app.screen = Screen::Help {
                return_screen: Box::new(old),
            };
        }
        KeyCode::Char('X') => {
            if let Some(index) = app.ui.provider_list_state.selected() {
                let sorted = app.sorted_provider_names();
                if let Some(name) = sorted.get(index) {
                    let stale = app.config.stale_hosts();
                    let provider_stale: Vec<_> = stale
                        .iter()
                        .filter(|(alias, _)| {
                            app.config.host_entries().iter().any(|e| {
                                e.alias == *alias && e.provider.as_deref() == Some(name.as_str())
                            })
                        })
                        .collect();
                    if provider_stale.is_empty() {
                        let display = crate::providers::provider_display_name(name);
                        app.notify_warning(crate::messages::no_stale_hosts_for(display));
                    } else {
                        let aliases: Vec<String> =
                            provider_stale.into_iter().map(|(a, _)| a.clone()).collect();
                        app.screen = Screen::ConfirmPurgeStale {
                            aliases,
                            provider: Some(name.clone()),
                        };
                    }
                }
            }
        }
        _ => {}
    }
}

/// Show a non-blocking warning when leaving the Token field with an invalid format.
fn warn_aws_token_format(app: &mut App, provider_name: &str) {
    if provider_name != "aws" {
        return;
    }
    if app.provider_form.focused_field != crate::app::ProviderFormField::Token {
        return;
    }
    let token = app.provider_form.token.trim();
    if token.is_empty() {
        return;
    }
    if !token.contains(':') {
        app.notify_warning(crate::messages::TOKEN_FORMAT_AWS);
    }
}

pub(super) fn handle_provider_form(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    // Dispatch to key picker if open
    if app.ui.show_key_picker {
        super::picker::handle_key_picker_shared(app, key, true);
        return;
    }

    // Dispatch to region picker if open
    if app.ui.show_region_picker {
        region::handle_region_picker(app, key);
        return;
    }

    let provider_name = match &app.screen {
        Screen::ProviderForm { provider } => provider.clone(),
        _ => return,
    };
    // Progressive disclosure: hide `VaultAddr` when no role is set so Tab
    // navigation skips the hidden field. `visible_fields` is a filtered
    // snapshot of `fields_for(provider)` taken once per key press.
    let visible = app.provider_form.visible_fields(&provider_name);
    let fields: &[crate::app::ProviderFormField] = &visible;
    // Field-kind predicates live on `ProviderFormField` so the rule is
    // enforced in one place. Note: `is_picker` here matches the full set
    // (aws/scaleway/gcp/oracle/ovh) -- the previous local closure only
    // matched aws/scaleway/gcp which was a bug; oracle/ovh need the picker
    // too because their `Regions` Space-handler at the bottom of this match
    // expects to open the picker. `is_picker` on the type is the source of
    // truth.
    let is_toggle = |f: crate::app::ProviderFormField| f.is_toggle();
    let is_picker = |f: crate::app::ProviderFormField| f.is_picker(&provider_name);

    // Handle discard confirmation dialog
    if app.pending_discard_confirm {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.pending_discard_confirm = false;
                app.clear_form_mtime();
                app.provider_form_baseline = None;
                app.screen = Screen::Providers;
                app.flush_pending_vault_write();
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.pending_discard_confirm = false;
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            if app.provider_form_is_dirty() {
                app.pending_discard_confirm = true;
            } else {
                app.clear_form_mtime();
                app.provider_form_baseline = None;
                app.screen = Screen::Providers;
                app.flush_pending_vault_write();
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            warn_aws_token_format(app, &provider_name);
            if !app.provider_form.expanded {
                let all = crate::app::ProviderFormField::fields_for(&provider_name);
                let req_count = all
                    .iter()
                    .filter(|f| {
                        crate::app::ProviderFormField::is_required_field(**f, &provider_name)
                    })
                    .count();
                let required = &all[..req_count];
                if required.is_empty() {
                    // Fallback: no required fields, use full field list
                    app.provider_form.focused_field = app.provider_form.focused_field.next(fields);
                } else {
                    let pos = required
                        .iter()
                        .position(|f| *f == app.provider_form.focused_field);
                    if let Some(idx) = pos {
                        if idx + 1 < required.len() {
                            app.provider_form.focused_field = required[idx + 1];
                        } else if req_count < all.len() {
                            // Last required field: expand and focus first optional
                            app.provider_form.expanded = true;
                            app.provider_form.focused_field = all[req_count];
                        } else {
                            // No optional fields, wrap
                            app.provider_form.focused_field = required[0];
                        }
                    } else {
                        app.provider_form.focused_field =
                            app.provider_form.focused_field.next(fields);
                    }
                }
            } else {
                app.provider_form.focused_field = app.provider_form.focused_field.next(fields);
            }
            app.provider_form.sync_cursor_to_end();
        }
        KeyCode::BackTab | KeyCode::Up => {
            warn_aws_token_format(app, &provider_name);
            if !app.provider_form.expanded {
                let all = crate::app::ProviderFormField::fields_for(&provider_name);
                let req_count = all
                    .iter()
                    .filter(|f| {
                        crate::app::ProviderFormField::is_required_field(**f, &provider_name)
                    })
                    .count();
                let required = &all[..req_count];
                if required.is_empty() {
                    // Fallback: no required fields, use full field list
                    app.provider_form.focused_field = app.provider_form.focused_field.prev(fields);
                } else {
                    let pos = required
                        .iter()
                        .position(|f| *f == app.provider_form.focused_field);
                    if let Some(idx) = pos {
                        let prev_idx = if idx > 0 { idx - 1 } else { required.len() - 1 };
                        app.provider_form.focused_field = required[prev_idx];
                    } else {
                        // Focus is on a non-required field while collapsed; go to last required
                        app.provider_form.focused_field = required[required.len() - 1];
                    }
                }
            } else {
                app.provider_form.focused_field = app.provider_form.focused_field.prev(fields);
            }
            app.provider_form.sync_cursor_to_end();
        }
        KeyCode::Left => {
            if app.provider_form.cursor_pos > 0 {
                app.provider_form.cursor_pos -= 1;
            }
        }
        KeyCode::Right => {
            let len = app.provider_form.focused_value().chars().count();
            if app.provider_form.cursor_pos < len {
                app.provider_form.cursor_pos += 1;
            }
        }
        KeyCode::Home => {
            app.provider_form.cursor_pos = 0;
        }
        KeyCode::End => {
            app.provider_form.sync_cursor_to_end();
        }
        KeyCode::Enter => {
            // INVARIANT: Enter ALWAYS submits the form, regardless of focused
            // field. Pickers/toggles are reached via Space (see arms below).
            // Documented in CLAUDE.md "Keyboard interaction rules".
            submit_provider_form(app, events_tx);
        }
        // SPACE GUARDS MUST PRECEDE the generic Char(c) arm.
        // Order: toggle first, picker second (no overlap, but explicit
        // ordering protects against future ProviderFormField additions).
        KeyCode::Char(' ')
            if app.provider_form.focused_field == crate::app::ProviderFormField::VerifyTls =>
        {
            app.provider_form.verify_tls = !app.provider_form.verify_tls;
        }
        KeyCode::Char(' ')
            if app.provider_form.focused_field == crate::app::ProviderFormField::AutoSync =>
        {
            app.provider_form.auto_sync = !app.provider_form.auto_sync;
        }
        // Empty-field gate: same rationale as host_form — once the user
        // has typed anything, Space inserts a literal space so custom
        // identity paths (e.g. `~/My Keys/id_rsa`) and free-form region
        // lists work. On an empty picker field, Space opens the picker.
        KeyCode::Char(' ')
            if is_picker(app.provider_form.focused_field)
                && app.provider_form.focused_value().is_empty() =>
        {
            let f = app.provider_form.focused_field;
            if f == crate::app::ProviderFormField::IdentityFile {
                app.scan_keys();
                app.ui.show_key_picker = true;
                app.ui.key_picker_state = ratatui::widgets::ListState::default();
                if !app.keys.is_empty() {
                    app.ui.key_picker_state.select(Some(0));
                }
            } else if f == crate::app::ProviderFormField::Regions {
                app.ui.show_region_picker = true;
                app.ui.region_picker_cursor = 0;
            }
        }
        KeyCode::Char(c) => {
            // Toggle fields (VerifyTls/AutoSync) have no text value to mutate;
            // every other field — including picker fields — accepts free-text
            // typing so users can supply custom paths or region values not
            // surfaced by the picker. Matches the host form's Char arm.
            let f = app.provider_form.focused_field;
            if !is_toggle(f) {
                app.provider_form.insert_char(c);
            }
        }
        KeyCode::Backspace => {
            let f = app.provider_form.focused_field;
            if !is_toggle(f) {
                app.provider_form.delete_char_before_cursor();
            }
        }
        _ => {}
    }
}

fn submit_provider_form(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.demo_mode {
        app.notify(crate::messages::DEMO_PROVIDER_CHANGES_DISABLED);
        app.screen = Screen::Providers;
        return;
    }
    let provider_name = match &app.screen {
        Screen::ProviderForm { provider } => provider.clone(),
        _ => return,
    };

    // Check for external provider config changes since form was opened
    if app.provider_config_changed_since_form_open() {
        app.notify_error(
            "Provider config changed externally. Press Esc and re-open to pick up changes.",
        );
        return;
    }

    // Reject control characters in all fields (prevents INI injection)
    let pf_fields = [
        (&app.provider_form.url, "URL"),
        (&app.provider_form.token, "Token"),
        (&app.provider_form.alias_prefix, "Alias Prefix"),
        (&app.provider_form.user, "User"),
        (&app.provider_form.identity_file, "Identity File"),
        (&app.provider_form.profile, "Profile"),
        (&app.provider_form.project, "Project ID"),
        (&app.provider_form.regions, "Regions"),
    ];
    for (value, name) in &pf_fields {
        if value.chars().any(|c| c.is_control()) {
            app.notify_warning(crate::messages::contains_control_chars(name));
            return;
        }
    }

    // Proxmox requires a URL
    if provider_name == "proxmox" {
        let url = app.provider_form.url.trim();
        if url.is_empty() {
            app.notify_warning(crate::messages::URL_REQUIRED_PROXMOX);
            return;
        }
        if !url.to_ascii_lowercase().starts_with("https://") {
            app.notify_error(
                "URL must start with https://. Toggle Verify TLS off for self-signed certificates.",
            );
            return;
        }
    }

    // AWS allows empty token when profile is set (credentials from ~/.aws/credentials)
    if app.provider_form.token.trim().is_empty()
        && provider_name != "tailscale"
        && (provider_name != "aws" || app.provider_form.profile.trim().is_empty())
    {
        let hint = if provider_name == "gcp" {
            "Token can't be empty. Provide a service account JSON key file path or access token."
                .to_string()
        } else if provider_name == "oracle" {
            "Token can't be empty. Provide the path to your OCI config file (e.g. ~/.oci/config)."
                .to_string()
        } else {
            let display_name = crate::providers::provider_display_name(provider_name.as_str());
            format!(
                "Token can't be empty. Grab one from your {} dashboard.",
                display_name
            )
        };
        app.notify_error(hint);
        return;
    }

    // GCP requires a project ID
    if provider_name == "gcp" && app.provider_form.project.trim().is_empty() {
        app.notify_warning(crate::messages::PROJECT_REQUIRED_GCP);
        return;
    }

    // Oracle requires a compartment OCID
    if provider_name == "oracle" && app.provider_form.compartment.trim().is_empty() {
        app.notify_warning(crate::messages::COMPARTMENT_REQUIRED_OCI);
        return;
    }

    // AWS/Scaleway require at least one region/zone
    if provider_name == "aws" && app.provider_form.regions.trim().is_empty() {
        app.notify_warning(crate::messages::REGIONS_REQUIRED_AWS);
        return;
    }
    if provider_name == "scaleway" && app.provider_form.regions.trim().is_empty() {
        app.notify_warning(crate::messages::ZONES_REQUIRED_SCALEWAY);
        return;
    }
    if provider_name == "azure" {
        let subs = app.provider_form.regions.trim();
        if subs.is_empty() {
            app.notify_warning(crate::messages::SUBSCRIPTIONS_REQUIRED_AZURE);
            return;
        }
        for sub in subs.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if !crate::providers::azure::is_valid_subscription_id(sub) {
                app.notify_error(
                    format!("Invalid subscription ID '{}'. Expected UUID format (e.g. 12345678-1234-1234-1234-123456789012).", sub));
                return;
            }
        }
    }

    let token = app.provider_form.token.trim().to_string();
    let alias_prefix = app.provider_form.alias_prefix.trim().to_string();
    if crate::ssh_config::model::is_host_pattern(&alias_prefix) {
        app.notify_warning(crate::messages::ALIAS_PREFIX_INVALID);
        return;
    }

    let user = {
        let u = app.provider_form.user.trim();
        if u.is_empty() {
            "root".to_string()
        } else {
            u.to_string()
        }
    };
    if user.contains(char::is_whitespace) {
        app.notify_warning(crate::messages::USER_NO_WHITESPACE);
        return;
    }

    let vault_role_trimmed = app.provider_form.vault_role.trim();
    if !vault_role_trimmed.is_empty() && !crate::vault_ssh::is_valid_role(vault_role_trimmed) {
        app.notify_warning(crate::messages::VAULT_ROLE_FORMAT);
        return;
    }

    let section = providers::config::ProviderSection {
        provider: provider_name.clone(),
        token: token.clone(),
        alias_prefix,
        user,
        identity_file: app.provider_form.identity_file.trim().to_string(),
        url: app.provider_form.url.trim().to_string(),
        verify_tls: app.provider_form.verify_tls,
        auto_sync: app.provider_form.auto_sync,
        profile: app.provider_form.profile.trim().to_string(),
        regions: app.provider_form.regions.trim().to_string(),
        project: app.provider_form.project.trim().to_string(),
        compartment: app.provider_form.compartment.trim().to_string(),
        vault_role: app.provider_form.vault_role.trim().to_string(),
        vault_addr: app.provider_form.vault_addr.trim().to_string(),
    };

    let old_section = app.provider_config.section(&provider_name).cloned();
    app.provider_config.set_section(section);
    if let Err(e) = app.provider_config.save() {
        // Rollback: restore previous state
        match old_section {
            Some(old) => app.provider_config.set_section(old),
            None => app.provider_config.remove_section(&provider_name),
        }
        app.notify_error(crate::messages::failed_to_save(&e));
        return;
    }

    let display_name = crate::providers::provider_display_name(provider_name.as_str());

    if !app.syncing_providers.contains_key(&provider_name) {
        let sync_section = app.provider_config.section(&provider_name).cloned();
        if let Some(sync_section) = sync_section {
            let cancel = Arc::new(AtomicBool::new(false));
            app.syncing_providers
                .insert(provider_name.clone(), cancel.clone());
            app.notify(crate::messages::provider_saved_syncing(display_name));
            super::sync::spawn_provider_sync(&sync_section, events_tx.clone(), cancel);
        }
    } else {
        app.notify(crate::messages::provider_saved(display_name));
    }
    app.clear_form_mtime();
    app.provider_form_baseline = None;
    app.screen = Screen::Providers;
    app.flush_pending_vault_write();
}
