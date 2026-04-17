//! Centralized user-facing messages.
//!
//! Every string the user can see (toasts, CLI output, error messages) lives
//! here. Handler, CLI and UI code reference these constants and functions
//! instead of inlining string literals. This makes copy consistent, auditable
//! and future-proof for i18n.

// ── General / shared ────────────────────────────────────────────────

pub const FAILED_TO_SAVE: &str = "Failed to save";
pub fn failed_to_save(e: &impl std::fmt::Display) -> String {
    format!("{}: {}", FAILED_TO_SAVE, e)
}

pub const CONFIG_CHANGED_EXTERNALLY: &str =
    "Config changed externally. Press Esc and re-open to pick up changes.";

// ── Demo mode ───────────────────────────────────────────────────────

pub const DEMO_CONNECTION_DISABLED: &str = "Demo mode. Connection disabled.";
pub const DEMO_SYNC_DISABLED: &str = "Demo mode. Sync disabled.";
pub const DEMO_TUNNELS_DISABLED: &str = "Demo mode. Tunnels disabled.";
pub const DEMO_VAULT_SIGNING_DISABLED: &str = "Demo mode. Vault SSH signing disabled.";
pub const DEMO_FILE_BROWSER_DISABLED: &str = "Demo mode. File browser disabled.";
pub const DEMO_CONTAINER_REFRESH_DISABLED: &str = "Demo mode. Container refresh disabled.";
pub const DEMO_CONTAINER_ACTIONS_DISABLED: &str = "Demo mode. Container actions disabled.";
pub const DEMO_EXECUTION_DISABLED: &str = "Demo mode. Execution disabled.";
pub const DEMO_PROVIDER_CHANGES_DISABLED: &str = "Demo mode. Provider config changes disabled.";

// ── Stale host ──────────────────────────────────────────────────────

pub fn stale_host(hint: &str) -> String {
    format!("Stale host.{}", hint)
}

// ── Host list ───────────────────────────────────────────────────────

pub fn copied_ssh_command(alias: &str) -> String {
    format!("Copied SSH command for {}.", alias)
}

pub fn copied_config_block(alias: &str) -> String {
    format!("Copied config block for {}.", alias)
}

pub fn showing_unreachable(count: usize) -> String {
    format!(
        "Showing {} unreachable host{}.",
        count,
        if count == 1 { "" } else { "s" }
    )
}

pub fn sorted_by(label: &str) -> String {
    format!("Sorted by {}.", label)
}

pub fn sorted_by_save_failed(label: &str, e: &impl std::fmt::Display) -> String {
    format!("Sorted by {}. (save failed: {})", label, e)
}

pub fn grouped_by(label: &str) -> String {
    format!("Grouped by {}.", label)
}

pub fn grouped_by_save_failed(label: &str, e: &impl std::fmt::Display) -> String {
    format!("Grouped by {}. (save failed: {})", label, e)
}

pub const UNGROUPED: &str = "Ungrouped.";

pub fn ungrouped_save_failed(e: &impl std::fmt::Display) -> String {
    format!("Ungrouped. (save failed: {})", e)
}

pub const GROUPED_BY_TAG: &str = "Grouped by tag.";

pub fn grouped_by_tag_save_failed(e: &impl std::fmt::Display) -> String {
    format!("Grouped by tag. (save failed: {})", e)
}

pub fn host_restored(alias: &str) -> String {
    format!("{} is back from the dead.", alias)
}

pub fn restored_tags(count: usize) -> String {
    format!(
        "Restored tags on {} host{}.",
        count,
        if count == 1 { "" } else { "s" }
    )
}

pub const NOTHING_TO_UNDO: &str = "Nothing to undo.";
pub const NO_IMPORTABLE_HOSTS: &str = "No importable hosts in known_hosts.";
pub const NO_STALE_HOSTS: &str = "No stale hosts.";
pub const NO_HOST_SELECTED: &str = "No host selected.";
pub const NO_HOSTS_TO_RUN: &str = "No hosts to run on.";
pub const NO_HOSTS_TO_TAG: &str = "No hosts to tag.";
pub const PING_FIRST: &str = "Ping first (p/P), then filter with !.";
pub const PINGING_ALL: &str = "Pinging all the things...";

pub fn included_file_edit(name: &str) -> String {
    format!("{} is in an included file. Edit it there.", name)
}

pub fn included_file_delete(name: &str) -> String {
    format!("{} is in an included file. Delete it there.", name)
}

pub fn included_file_clone(name: &str) -> String {
    format!("{} is in an included file. Clone it there.", name)
}

pub fn included_host_lives_in(alias: &str, path: &impl std::fmt::Display) -> String {
    format!("{} lives in {}. Edit it there.", alias, path)
}

pub fn included_host_clone_there(alias: &str, path: &impl std::fmt::Display) -> String {
    format!("{} lives in {}. Clone it there.", alias, path)
}

pub fn included_host_tag_there(alias: &str, path: &impl std::fmt::Display) -> String {
    format!("{} is included from {}. Tag it there.", alias, path)
}

pub const HOST_NOT_FOUND_IN_CONFIG: &str = "Host not found in config.";

// ── Host form ───────────────────────────────────────────────────────

pub const SMART_PARSED: &str = "Smart-parsed that for you. Check the fields.";
pub const LOOKS_LIKE_ADDRESS: &str = "Looks like an address. Suggested as Host.";

// ── Confirm delete ──────────────────────────────────────────────────

pub fn goodbye_host(alias: &str) -> String {
    format!("Goodbye, {}. We barely knew ye. (u to undo)", alias)
}

pub fn host_not_found(alias: &str) -> String {
    format!("Host '{}' not found.", alias)
}

pub fn cert_cleanup_warning(path: &impl std::fmt::Display, e: &impl std::fmt::Display) -> String {
    format!("Warning: failed to clean up Vault SSH cert {}: {}", path, e)
}

// ── Clone ───────────────────────────────────────────────────────────

pub const CLONED_VAULT_CLEARED: &str = "Cloned. Vault SSH role cleared on copy.";

// ── Tunnels ─────────────────────────────────────────────────────────

pub const TUNNEL_REMOVED: &str = "Tunnel removed.";
pub const TUNNEL_SAVED: &str = "Tunnel saved.";
pub const TUNNEL_NOT_FOUND: &str = "Tunnel not found in config.";
pub const TUNNEL_INCLUDED_READ_ONLY: &str = "Included host. Tunnels are read-only.";
pub const TUNNEL_ORIGINAL_NOT_FOUND: &str = "Original tunnel not found in config.";
pub const TUNNEL_LIST_CHANGED: &str = "Tunnel list changed externally. Press Esc and re-open.";
pub const TUNNEL_DUPLICATE: &str = "Duplicate tunnel already configured.";

pub fn tunnel_stopped(alias: &str) -> String {
    format!("Tunnel for {} stopped.", alias)
}

pub fn tunnel_started(alias: &str) -> String {
    format!("Tunnel for {} started.", alias)
}

pub fn tunnel_start_failed(e: &impl std::fmt::Display) -> String {
    format!("Failed to start tunnel: {}", e)
}

// ── Ping ────────────────────────────────────────────────────────────

pub fn pinging_host(alias: &str, show_hint: bool) -> String {
    if show_hint {
        format!("Pinging {}... (Shift+P pings all)", alias)
    } else {
        format!("Pinging {}...", alias)
    }
}

pub fn bastion_not_found(alias: &str) -> String {
    format!("Bastion {} not found in config.", alias)
}

// ── Providers ───────────────────────────────────────────────────────

pub fn provider_removed(display_name: &str) -> String {
    format!(
        "Removed {} configuration. Synced hosts remain in your SSH config.",
        display_name
    )
}

pub fn provider_not_configured(display_name: &str) -> String {
    format!("{} is not configured. Nothing to remove.", display_name)
}

pub fn provider_configure_first(display_name: &str) -> String {
    format!("Configure {} first. Press Enter to set up.", display_name)
}

pub fn provider_saved_syncing(display_name: &str) -> String {
    format!("Saved {} configuration. Syncing...", display_name)
}

pub fn provider_saved(display_name: &str) -> String {
    format!("Saved {} configuration.", display_name)
}

pub fn syncing_provider(display_name: &str) -> String {
    format!("Syncing {}...", display_name)
}

pub fn no_stale_hosts_for(display_name: &str) -> String {
    format!("No stale hosts for {}.", display_name)
}

pub fn contains_control_chars(name: &str) -> String {
    format!("{} contains control characters.", name)
}

pub const TOKEN_FORMAT_AWS: &str = "Token format: AccessKeyId:SecretAccessKey";
pub const URL_REQUIRED_PROXMOX: &str = "URL is required for Proxmox VE.";
pub const PROJECT_REQUIRED_GCP: &str = "Project ID can't be empty. Set your GCP project ID.";
pub const COMPARTMENT_REQUIRED_OCI: &str =
    "Compartment can't be empty. Set your OCI compartment OCID.";
pub const REGIONS_REQUIRED_AWS: &str = "Select at least one AWS region.";
pub const ZONES_REQUIRED_SCALEWAY: &str = "Select at least one Scaleway zone.";
pub const SUBSCRIPTIONS_REQUIRED_AZURE: &str = "Enter at least one Azure subscription ID.";
pub const ALIAS_PREFIX_INVALID: &str =
    "Alias prefix can't contain spaces or pattern characters (*, ?, [, !).";
pub const USER_NO_WHITESPACE: &str = "User can't contain whitespace.";
pub const VAULT_ROLE_FORMAT: &str = "Vault SSH role must be in the form <mount>/sign/<role>.";

// ── Vault SSH ───────────────────────────────────────────────────────

pub const VAULT_SIGNING_CANCELLED: &str = "Vault SSH signing cancelled.";
pub const VAULT_NO_ROLE_CONFIGURED: &str = "No Vault SSH role configured. Set one in the host form \
     (Vault SSH role field) or on a provider for shared defaults.";
pub const VAULT_NO_HOSTS_WITH_ROLE: &str = "No hosts with a Vault SSH role configured.";
pub const VAULT_ALL_CERTS_VALID: &str = "All Vault SSH certificates are still valid.";
pub const VAULT_NO_ADDRESS: &str = "No Vault address set. Edit the host (e) or provider \
     and fill in the Vault SSH Address field.";

pub fn vault_error(msg: &str) -> String {
    format!("Vault SSH: {}", msg)
}

pub fn vault_signed(alias: &str) -> String {
    format!("Signed Vault SSH cert for {}", alias)
}

pub fn vault_sign_failed(alias: &str, message: &str) -> String {
    format!("Vault SSH: failed to sign {}: {}", alias, message)
}

pub fn vault_signing_progress(spinner: &str, done: usize, total: usize, alias: &str) -> String {
    format!(
        "{} Signing {}/{}: {} (V to cancel)",
        spinner, done, total, alias
    )
}

pub fn vault_cert_saved_host_gone(alias: &str) -> String {
    format!(
        "Vault SSH cert saved for {} but host no longer in config \
         (renamed or deleted). CertificateFile NOT written.",
        alias
    )
}

pub fn vault_spawn_failed(e: &impl std::fmt::Display) -> String {
    format!("Vault SSH: failed to spawn signing thread: {}", e)
}

pub fn vault_cert_check_failed(alias: &str, message: &str) -> String {
    format!("Cert check failed for {}: {}", alias, message)
}

pub fn vault_role_set(role: &str) -> String {
    format!("Vault SSH role set to {}.", role)
}

// ── Snippets ────────────────────────────────────────────────────────

pub fn snippet_removed(name: &str) -> String {
    format!("Removed snippet '{}'.", name)
}

pub fn snippet_added(name: &str) -> String {
    format!("Added snippet '{}'.", name)
}

pub fn snippet_updated(name: &str) -> String {
    format!("Updated snippet '{}'.", name)
}

pub fn snippet_exists(name: &str) -> String {
    format!("'{}' already exists.", name)
}

pub const OUTPUT_COPIED: &str = "Output copied.";

pub fn copy_failed(e: &impl std::fmt::Display) -> String {
    format!("Copy failed: {}", e)
}

// ── Picker (password source, key, proxy) ────────────────────────────

pub const GLOBAL_DEFAULT_CLEARED: &str = "Global default cleared.";
pub const PASSWORD_SOURCE_CLEARED: &str = "Password source cleared.";

pub fn global_default_set(label: &str) -> String {
    format!("Global default set to {}.", label)
}

pub fn password_source_set(label: &str) -> String {
    format!("Password source set to {}.", label)
}

pub fn complete_path(label: &str) -> String {
    format!("Complete the {} path.", label)
}

pub fn key_selected(name: &str) -> String {
    format!("Locked and loaded with {}.", name)
}

pub fn proxy_jump_set(alias: &str) -> String {
    format!("Jumping through {}.", alias)
}

pub fn save_default_failed(e: &impl std::fmt::Display) -> String {
    format!("Failed to save default: {}", e)
}

// ── Containers ──────────────────────────────────────────────────────

pub fn container_action_complete(action: &str) -> String {
    format!("Container {} complete.", action)
}

// ── Import ──────────────────────────────────────────────────────────

pub fn imported_hosts(imported: usize, skipped: usize) -> String {
    format!(
        "Imported {} host{}, skipped {} duplicate{}.",
        imported,
        if imported == 1 { "" } else { "s" },
        skipped,
        if skipped == 1 { "" } else { "s" }
    )
}

pub fn all_hosts_exist(skipped: usize) -> String {
    if skipped == 1 {
        "Host already exists.".to_string()
    } else {
        format!("All {} hosts already exist.", skipped)
    }
}

// ── SSH config repair ───────────────────────────────────────────────

pub fn config_repaired(groups: usize, orphaned: usize) -> String {
    format!(
        "Repaired SSH config ({} absorbed, {} orphaned group headers).",
        groups, orphaned
    )
}

pub fn no_exact_match(alias: &str) -> String {
    format!("No exact match for '{}'. Here's what we found.", alias)
}

pub fn group_pref_reset_failed(e: &impl std::fmt::Display) -> String {
    format!("Group preference reset. (save failed: {})", e)
}

// ── Connection ──────────────────────────────────────────────────────

pub fn opened_in_tmux(alias: &str) -> String {
    format!("Opened {} in new tmux window.", alias)
}

pub fn tmux_error(e: &impl std::fmt::Display) -> String {
    format!("tmux: {}", e)
}

pub fn connection_failed(alias: &str) -> String {
    format!("Connection to {} failed.", alias)
}

// ── Host key reset ──────────────────────────────────────────────────

pub fn host_key_remove_failed(stderr: &str) -> String {
    format!("Failed to remove host key: {}", stderr)
}

pub fn ssh_keygen_failed(e: &impl std::fmt::Display) -> String {
    format!("Failed to run ssh-keygen: {}", e)
}

// ── Transfer ────────────────────────────────────────────────────────

pub const TRANSFER_COMPLETE: &str = "Transfer complete.";

// ── Background / event loop ─────────────────────────────────────────

pub const PING_EXPIRED: &str = "Ping expired. Press P to refresh.";

pub fn provider_event(name: &str, message: &str) -> String {
    format!("{}: {}", name, message)
}

// ── Vault SSH bulk signing summaries (event_loop.rs) ────────────────

pub fn vault_config_reapply_failed(signed: usize, e: &impl std::fmt::Display) -> String {
    format!(
        "External edits detected; signed {} certs but failed to re-apply CertificateFile: {}",
        signed, e
    )
}

pub fn vault_external_edits_merged(summary: &str, reapplied: usize) -> String {
    format!(
        "{} External ssh config edits detected, merged {} CertificateFile directives.",
        summary, reapplied
    )
}

pub fn vault_external_edits_no_write(summary: &str) -> String {
    format!(
        "{} External ssh config edits detected; certs on disk, no CertificateFile written.",
        summary
    )
}

pub fn vault_reparse_failed(signed: usize, e: &impl std::fmt::Display) -> String {
    format!(
        "Signed {} certs but cannot re-parse ssh config after external edit: {}. \
         Certs are on disk under ~/.purple/certs/.",
        signed, e
    )
}

pub fn vault_config_update_failed(signed: usize, e: &impl std::fmt::Display) -> String {
    format!(
        "Signed {} certs but failed to update SSH config: {}",
        signed, e
    )
}

pub fn vault_config_write_after_sign(e: &impl std::fmt::Display) -> String {
    format!("Failed to update config after vault signing: {}", e)
}

// ── File browser ────────────────────────────────────────────────────

// ── Confirm / host key ──────────────────────────────────────────────

pub fn removed_host_key(hostname: &str) -> String {
    format!("Removed host key for {}. Reconnecting...", hostname)
}

// ── Host detail (tags) ──────────────────────────────────────────────

pub fn tagged_host(alias: &str, count: usize) -> String {
    format!(
        "Tagged {} with {} label{}.",
        alias,
        count,
        if count == 1 { "" } else { "s" }
    )
}

// ── Config reload ───────────────────────────────────────────────────

pub fn config_reloaded(count: usize) -> String {
    format!("Config reloaded. {} hosts.", count)
}

// ── Sync background ─────────────────────────────────────────────────

pub fn synced_progress(names: &str) -> String {
    format!("Synced: {}...", names)
}

pub fn synced_done(names: &str) -> String {
    format!("Synced: {}", names)
}

// ── Vault signing cancelled summary ─────────────────────────────────

pub fn vault_signing_cancelled_summary(
    signed: u32,
    failed: u32,
    first_error: Option<&str>,
) -> String {
    let mut msg = format!(
        "Vault SSH signing cancelled ({} signed, {} failed)",
        signed, failed
    );
    if let Some(err) = first_error {
        msg.push_str(": ");
        msg.push_str(err);
    }
    msg
}

// ── Region picker ───────────────────────────────────────────────────

pub fn regions_selected_count(count: usize, label: &str) -> String {
    let s = if count == 1 { "" } else { "s" };
    format!("{} {}{} selected.", count, label, s)
}

// ── Purge stale ─────────────────────────────────────────────────────

// ── Clipboard ───────────────────────────────────────────────────────

pub const NO_CLIPBOARD_TOOL: &str =
    "No clipboard tool found. Install pbcopy (macOS), wl-copy (Wayland), or xclip/xsel (X11).";

// ── CLI messages ────────────────────────────────────────────────────

#[path = "messages/cli.rs"]
pub mod cli;

// ── Update messages ─────────────────────────────────────────────────

pub mod update {
    pub const WHATS_NEW_HINT: &str = "Press n inside purple to see what's new.";

    pub fn already_on(current: &str) -> String {
        format!("already on v{} (latest).", current)
    }

    pub fn available(latest: &str, current: &str) -> String {
        format!("v{} available (current: v{}).", latest, current)
    }
}

// ── Askpass / password prompts ───────────────────────────────────────

pub mod askpass {
    pub const BW_NOT_FOUND: &str = "Bitwarden CLI (bw) not found. SSH will prompt for password.";
    pub const BW_NOT_LOGGED_IN: &str = "Bitwarden vault not logged in. Run 'bw login' first.";
    pub const EMPTY_PASSWORD: &str = "Empty password. SSH will prompt for password.";
    pub const PASSWORD_IN_KEYCHAIN: &str = "Password stored in keychain.";

    pub fn read_failed(e: &impl std::fmt::Display) -> String {
        format!("Failed to read password: {}", e)
    }

    pub fn unlock_failed_retry(e: &impl std::fmt::Display) -> String {
        format!("Unlock failed: {}. Try again.", e)
    }

    pub fn unlock_failed_prompt(e: &impl std::fmt::Display) -> String {
        format!("Unlock failed: {}. SSH will prompt for password.", e)
    }
}

// ── Logging ─────────────────────────────────────────────────────────

pub mod logging {
    pub fn init_failed(e: &impl std::fmt::Display) -> String {
        format!("[purple] Failed to initialize logger: {}", e)
    }

    pub const SSH_VERSION_FAILED: &str = "[purple] Failed to detect SSH version. Is ssh installed?";
}

// ── Form field hints / placeholders ─────────────────────────────────
//
// Dimmed placeholder text shown in empty form fields. Centralized here
// so every user-visible string lives in one place and is auditable.

pub mod hints {
    // ── Shared ──────────────────────────────────────────────────────
    // Picker hints mention "Space" because per the design system keyboard
    // invariants, Enter always submits a form; pickers open on Space.
    // Keep these strings in sync with scripts/check-keybindings.sh.
    pub const IDENTITY_FILE_PICK: &str = "Space to pick a key";
    pub const DEFAULT_SSH_USER: &str = "root";

    // ── Host form ───────────────────────────────────────────────────
    pub const HOST_ALIAS: &str = "e.g. prod or db-01";
    pub const HOST_ALIAS_PATTERN: &str = "10.0.0.* or *.example.com";
    pub const HOST_HOSTNAME: &str = "192.168.1.1 or example.com";
    pub const HOST_PORT: &str = "22";
    pub const HOST_PROXY_JUMP: &str = "Space to pick a host";
    pub const HOST_VAULT_SSH: &str = "e.g. ssh-client-signer/sign/my-role (auth via vault login)";
    pub const HOST_VAULT_SSH_PICKER: &str = "Space to pick a role or type one";
    pub const HOST_VAULT_ADDR: &str =
        "e.g. http://127.0.0.1:8200 (inherits from provider or env when empty)";
    pub const HOST_TAGS: &str = "e.g. prod, staging, us-east (comma-separated)";
    pub const HOST_ASKPASS_PICK: &str = "Space to pick a source";

    pub fn askpass_default(default: &str) -> String {
        format!("default: {}", default)
    }

    pub fn inherits_from(value: &str, provider: &str) -> String {
        format!("inherits {} from {}", value, provider)
    }

    // ── Tunnel form ─────────────────────────────────────────────────
    pub const TUNNEL_BIND_PORT: &str = "8080";
    pub const TUNNEL_REMOTE_HOST: &str = "localhost";
    pub const TUNNEL_REMOTE_PORT: &str = "80";

    // ── Snippet form ────────────────────────────────────────────────
    pub const SNIPPET_NAME: &str = "check-disk";
    pub const SNIPPET_COMMAND: &str = "df -h";
    pub const SNIPPET_OPTIONAL: &str = "(optional)";

    // ── Provider form ───────────────────────────────────────────────
    pub const PROVIDER_URL: &str = "https://pve.example.com:8006";
    pub const PROVIDER_TOKEN_DEFAULT: &str = "your-api-token";
    pub const PROVIDER_TOKEN_PROXMOX: &str = "user@pam!token=secret";
    pub const PROVIDER_TOKEN_AWS: &str = "AccessKeyId:Secret (or use Profile)";
    pub const PROVIDER_TOKEN_GCP: &str = "/path/to/service-account.json (or access token)";
    pub const PROVIDER_TOKEN_AZURE: &str = "/path/to/service-principal.json (or access token)";
    pub const PROVIDER_TOKEN_TAILSCALE: &str = "API key (leave empty for local CLI)";
    pub const PROVIDER_TOKEN_ORACLE: &str = "~/.oci/config";
    pub const PROVIDER_TOKEN_OVH: &str = "app_key:app_secret:consumer_key";
    pub const PROVIDER_PROFILE: &str = "Name from ~/.aws/credentials (or use Token)";
    pub const PROVIDER_PROJECT_DEFAULT: &str = "my-gcp-project-id";
    pub const PROVIDER_PROJECT_OVH: &str = "Public Cloud project ID";
    pub const PROVIDER_COMPARTMENT: &str = "ocid1.compartment.oc1..aaaa...";
    pub const PROVIDER_REGIONS_DEFAULT: &str = "Space to select regions";
    pub const PROVIDER_REGIONS_GCP: &str = "Space to select zones (empty = all)";
    pub const PROVIDER_REGIONS_SCALEWAY: &str = "Space to select zones";
    // Azure regions is a text input (not a picker), so no key is mentioned.
    pub const PROVIDER_REGIONS_AZURE: &str = "comma-separated subscription IDs";
    pub const PROVIDER_REGIONS_OVH: &str = "Space to select endpoint (default: EU)";
    pub const PROVIDER_USER_AWS: &str = "ec2-user";
    pub const PROVIDER_USER_GCP: &str = "ubuntu";
    pub const PROVIDER_USER_AZURE: &str = "azureuser";
    pub const PROVIDER_USER_ORACLE: &str = "opc";
    pub const PROVIDER_USER_OVH: &str = "ubuntu";
    pub const PROVIDER_VAULT_ROLE: &str =
        "e.g. ssh-client-signer/sign/my-role (vault login; inherited)";
    pub const PROVIDER_VAULT_ADDR: &str = "e.g. http://127.0.0.1:8200 (inherited by all hosts)";
    pub const PROVIDER_ALIAS_PREFIX_DEFAULT: &str = "prefix";
}

#[cfg(test)]
mod hints_tests {
    use super::hints;

    #[test]
    fn askpass_default_formats() {
        assert_eq!(hints::askpass_default("keychain"), "default: keychain");
    }

    #[test]
    fn askpass_default_formats_empty() {
        assert_eq!(hints::askpass_default(""), "default: ");
    }

    #[test]
    fn inherits_from_formats() {
        assert_eq!(
            hints::inherits_from("role/x", "aws"),
            "inherits role/x from aws"
        );
    }

    #[test]
    fn picker_hints_mention_space_not_enter() {
        // Per the keyboard invariants, pickers open on Space.
        // If these assertions fail, audit scripts/check-keybindings.sh too.
        for s in [
            hints::IDENTITY_FILE_PICK,
            hints::HOST_PROXY_JUMP,
            hints::HOST_VAULT_SSH_PICKER,
            hints::HOST_ASKPASS_PICK,
            hints::PROVIDER_REGIONS_DEFAULT,
            hints::PROVIDER_REGIONS_GCP,
            hints::PROVIDER_REGIONS_SCALEWAY,
            hints::PROVIDER_REGIONS_OVH,
        ] {
            assert!(
                s.starts_with("Space "),
                "picker hint must mention Space: {s}"
            );
            assert!(!s.contains("Enter "), "picker hint must not say Enter: {s}");
        }
    }
}

#[path = "messages/whats_new.rs"]
pub mod whats_new;

#[path = "messages/whats_new_toast.rs"]
pub mod whats_new_toast;
