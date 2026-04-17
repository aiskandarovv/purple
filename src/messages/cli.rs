// ── Add host validation ─────────────────────────────────────────

pub const ALIAS_EMPTY: &str = "Alias can't be empty. Use --alias to specify one.";
pub const ALIAS_WHITESPACE: &str =
    "Alias can't contain whitespace. Use --alias to pick a simpler name.";
pub const ALIAS_PATTERN_CHARS: &str =
    "Alias can't contain pattern characters. Use --alias to pick a different name.";
pub const HOSTNAME_WHITESPACE: &str = "Hostname can't contain whitespace.";
pub const USER_WHITESPACE: &str = "User can't contain whitespace.";
pub const PASSWORD_EMPTY: &str = "Password can't be empty.";
pub const CANCELLED: &str = "Cancelled.";
pub const DESCRIPTION_CONTROL_CHARS: &str = "Description contains control characters.";

pub use super::contains_control_chars as control_chars;

pub fn welcome(alias: &str) -> String {
    format!("Welcome aboard, {}!", alias)
}

// ── Import ──────────────────────────────────────────────────────

pub const IMPORT_NO_FILE: &str =
    "Provide a file or use --known-hosts. Run 'purple import --help' for details.";

// ── Provider CLI ────────────────────────────────────────────────

pub const NO_PROVIDERS: &str = "No providers configured. Run 'purple provider add' to set one up.";

pub fn no_config_for(provider: &str) -> String {
    format!(
        "No configuration for {}. Run 'purple provider add {}' first.",
        provider, provider
    )
}

pub fn saved_config(provider: &str) -> String {
    format!("Saved {} configuration.", provider)
}

pub fn no_config_to_remove(provider: &str) -> String {
    format!("No configuration for '{}'. Nothing to remove.", provider)
}

pub fn removed_config(provider: &str) -> String {
    format!("Removed {} configuration.", provider)
}

// ── Tunnel CLI ──────────────────────────────────────────────────

pub fn no_tunnels_for(alias: &str) -> String {
    format!("No tunnels configured for {}.", alias)
}

pub fn tunnels_for(alias: &str) -> String {
    format!("Tunnels for {}:", alias)
}

pub const NO_TUNNELS: &str = "No tunnels configured.";

pub fn starting_tunnel(alias: &str) -> String {
    format!("Starting tunnel for {}... (Ctrl+C to stop)", alias)
}

pub fn host_not_found(alias: &str) -> String {
    format!("No host '{}' found.", alias)
}

pub fn added_forward(forward: &str, alias: &str) -> String {
    format!("Added {} to {}.", forward, alias)
}

pub fn forward_exists(forward: &str, alias: &str) -> String {
    format!("Forward {} already exists on {}.", forward, alias)
}

pub fn forward_not_found(forward: &str, alias: &str) -> String {
    format!("No matching forward {} found on {}.", forward, alias)
}

pub fn removed_forward(forward: &str, alias: &str) -> String {
    format!("Removed {} from {}.", forward, alias)
}

pub fn no_forwards(alias: &str) -> String {
    format!("No forwarding directives configured for '{}'.", alias)
}

// ── Snippet CLI ─────────────────────────────────────────────────

pub const NO_SNIPPETS: &str = "No snippets configured. Use 'purple snippet add' to create one.";

pub use super::snippet_added;
pub use super::snippet_removed;
pub use super::snippet_updated;

pub fn snippet_not_found(name: &str) -> String {
    format!("No snippet '{}' found.", name)
}

pub fn no_hosts_with_tag(tag: &str) -> String {
    format!("No hosts found with tag '{}'.", tag)
}

pub const SPECIFY_TARGET: &str = "Specify a host alias, --tag or --all.";

// ── Run/exec output ─────────────────────────────────────────────

pub fn beaming_up(alias: &str) -> String {
    format!("Beaming you up to {}...\n", alias)
}

pub fn running_snippet_on(name: &str, alias: &str) -> String {
    format!("Running '{}' on {}...\n", name, alias)
}

pub fn host_separator(alias: &str) -> String {
    format!("── {} ──", alias)
}

pub fn exited_with_code(code: i32) -> String {
    format!("Exited with code {}.", code)
}

pub const DONE: &str = "Done.";

pub fn done_multi(name: &str, count: usize) -> String {
    format!("Done. Ran '{}' on {} hosts.", name, count)
}

pub const PRESS_ENTER: &str = "Press Enter to continue...";

pub fn host_failed(alias: &str, e: &impl std::fmt::Display) -> String {
    format!("[{}] Failed: {}", alias, e)
}

pub fn skipping_host(alias: &str, e: &impl std::fmt::Display) -> String {
    format!("Skipping {}: {}", alias, e)
}

// ── Password CLI ────────────────────────────────────────────────

pub fn password_removed(alias: &str) -> String {
    format!("Password removed for {}.", alias)
}

// ── Log CLI ─────────────────────────────────────────────────────

pub fn log_deleted(path: &impl std::fmt::Display) -> String {
    format!("Log file deleted: {}", path)
}

pub fn no_log_file(path: &impl std::fmt::Display) -> String {
    format!("No log file found at {}", path)
}

// ── Theme CLI ───────────────────────────────────────────────────

pub const BUILTIN_THEMES: &str = "Built-in themes:";
pub const CUSTOM_THEMES: &str = "\nCustom themes:";

pub fn theme_set(name: &str) -> String {
    format!("Theme set to: {}", name)
}

// ── Sync output ─────────────────────────────────────────────────

pub fn syncing(name: &str, summary: &str) -> String {
    format!("\x1b[2K\rSyncing {}... {}", name, summary)
}

pub fn servers_found_with_failures(count: usize, failures: usize, total: usize) -> String {
    format!(
        "{} servers found ({} of {} failed to fetch).",
        count, failures, total
    )
}

pub fn servers_found(count: usize) -> String {
    format!("{} servers found.", count)
}

pub fn sync_result(prefix: &str, added: usize, updated: usize, unchanged: usize) -> String {
    format!(
        "{}Added {}, updated {}, unchanged {}.",
        prefix, added, updated, unchanged
    )
}

pub fn sync_removed(count: usize) -> String {
    format!("  Removed {}.", count)
}

pub fn sync_stale(count: usize) -> String {
    format!("  Marked {} stale.", count)
}

pub fn sync_skip_remove(display_name: &str) -> String {
    format!(
        "! {}: skipping --remove due to partial failures.",
        display_name
    )
}

pub fn sync_error(display_name: &str, e: &impl std::fmt::Display) -> String {
    format!("! {}: {}", display_name, e)
}

pub const SYNC_SKIP_WRITE: &str =
    "! Skipping config write due to sync failures. Fix the errors and re-run.";

// ── Provider validation (CLI) ───────────────────────────────────

pub const PROXMOX_URL_REQUIRED: &str =
    "Proxmox requires --url (e.g. --url https://pve.example.com:8006).";
pub const AWS_REGIONS_REQUIRED: &str =
    "AWS requires --regions (e.g. --regions us-east-1,eu-west-1).";
pub const AZURE_REGIONS_REQUIRED: &str =
    "Azure requires --regions with one or more subscription IDs.";
pub const GCP_PROJECT_REQUIRED: &str = "GCP requires --project (e.g. --project my-gcp-project-id).";
pub use super::ALIAS_PREFIX_INVALID;

pub const WARN_URL_NOT_USED: &str =
    "Warning: --url is only used by the Proxmox provider. Ignoring.";
pub const WARN_PROFILE_NOT_USED: &str =
    "Warning: --profile is only used by the AWS provider. Ignoring.";
pub const WARN_PROJECT_NOT_USED: &str =
    "Warning: --project is only used by the GCP provider. Ignoring.";
pub const WARN_COMPARTMENT_NOT_USED: &str =
    "Warning: --compartment is only used by the Oracle provider. Ignoring.";

// ── Vault CLI ───────────────────────────────────────────────────

pub fn vault_no_role(alias: &str) -> String {
    format!(
        "No Vault SSH role configured for '{}'. Set it in the host form \
         (Vault SSH Role field) or in the provider config (vault_role).",
        alias
    )
}

pub fn vault_cert_signed(path: &impl std::fmt::Display) -> String {
    format!("Certificate signed: {}", path)
}

pub fn vault_sign_failed(e: &impl std::fmt::Display) -> String {
    format!("failed: {}", e)
}

pub fn vault_config_update_warning(e: &impl std::fmt::Display) -> String {
    format!("Warning: Failed to update SSH config: {}", e)
}

// ── List hosts ──────────────────────────────────────────────────

pub const NO_HOSTS: &str = "No hosts configured. Run 'purple' to add some!";

// ── Token ───────────────────────────────────────────────────────

pub const NO_TOKEN: &str =
    "No token provided. Use --token, --token-stdin, or set PURPLE_TOKEN env var.";

// ── What's new (CLI) ────────────────────────────────────────────

pub mod whats_new {
    pub const HEADER: &str = "purple release notes";
}
