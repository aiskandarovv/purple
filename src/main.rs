mod animation;
mod app;
mod askpass;
mod askpass_env;
mod changelog;
mod cli;
mod cli_args;
mod clipboard;
mod connection;
mod containers;
mod demo;
mod demo_flag;
mod event;
mod file_browser;
mod fs_util;
mod handler;
mod history;
mod import;
mod logging;
mod mcp;
mod messages;
mod onboarding;
mod ping;
mod preferences;
mod providers;
mod quick_add;
mod snippet;
mod ssh_config;
mod ssh_context;
mod ssh_keys;
mod tui;
mod tui_loop;
mod tunnel;
mod ui;
mod update;
mod vault_ssh;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use log::warn;

use app::App;
use cli_args::{Cli, Commands, VaultCommands};
use ssh_config::model::SshConfigFile;
use tui_loop::run_tui;
// Re-exported so `mod tests` below and external handler sites that used to
// live in this file keep working after the tui_loop extraction.
#[allow(unused_imports)]
use tui_loop::{cache_entry_is_stale, current_cert_mtime};

pub(crate) fn resolve_config_path(path: &str) -> Result<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(rest))
    } else {
        Ok(PathBuf::from(path))
    }
}

pub(crate) fn resolve_token(explicit: Option<String>, from_stdin: bool) -> Result<String> {
    if let Some(t) = explicit {
        return Ok(t);
    }
    if from_stdin {
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        return Ok(buf.trim().to_string());
    }
    if let Ok(t) = std::env::var("PURPLE_TOKEN") {
        return Ok(t);
    }
    anyhow::bail!("{}", crate::messages::cli::NO_TOKEN)
}

fn main() -> Result<()> {
    // Askpass mode: when invoked as SSH_ASKPASS, handle the request and exit.
    // Must run before theme init and CLI parse to avoid terminal interference.
    if std::env::var("PURPLE_ASKPASS_MODE").is_ok() {
        return askpass::handle();
    }

    ui::theme::init();
    let cli = Cli::parse();

    // Determine if this is a CLI subcommand (log to stderr too) or TUI (file only)
    let is_cli_subcommand = cli.command.is_some() || cli.list || cli.connect.is_some();
    logging::init(cli.verbose, is_cli_subcommand);

    if let Some(ref name) = cli.theme {
        if let Some(theme) = ui::theme::ThemeDef::find_builtin(name).or_else(|| {
            ui::theme::ThemeDef::load_custom()
                .into_iter()
                .find(|t| t.name.eq_ignore_ascii_case(name))
        }) {
            ui::theme::set_theme(theme);
        } else {
            anyhow::bail!("Unknown theme: {}", name);
        }
    }

    // Shell completions (no config file needed)
    if let Some(shell) = cli.completions {
        let mut cmd = Cli::command();
        generate(shell, &mut cmd, "purple", &mut std::io::stdout());
        return Ok(());
    }

    if cli.demo {
        let mut app = demo::build_demo_app();
        demo::seed_whats_new_toast(&mut app);
        return run_tui(app);
    }

    // Provider and Update subcommands don't need SSH config
    if let Some(Commands::Provider { command }) = cli.command {
        return cli::handle_provider_command(command);
    }
    if let Some(Commands::Update) = cli.command {
        return update::self_update();
    }
    if let Some(Commands::Password { command }) = cli.command {
        return cli::handle_password_command(command);
    }
    if let Some(Commands::Mcp) = cli.command {
        let config_path = resolve_config_path(&cli.config)?;
        return mcp::run(&config_path);
    }
    if let Some(Commands::Logs { tail, clear }) = cli.command {
        return cli::handle_logs_command(tail, clear);
    }
    if let Some(Commands::Theme { command }) = cli.command {
        return cli::handle_theme_command(command);
    }
    if let Some(Commands::WhatsNew { since }) = &cli.command {
        let output = cli::run_whats_new(since.as_deref())?;
        print!("{}", output);
        return Ok(());
    }

    let config_path = resolve_config_path(&cli.config)?;
    let mut config = SshConfigFile::parse(&config_path)?;
    let repaired_groups = config.repair_absorbed_group_comments();
    let orphaned_headers = config.remove_all_orphaned_group_headers();

    write_startup_banner(&config, &config_path, cli.verbose);

    // Handle subcommands that need SSH config
    match cli.command {
        Some(Commands::Add { target, alias, key }) => {
            return cli::handle_quick_add(config, &target, alias.as_deref(), key.as_deref());
        }
        Some(Commands::Import {
            file,
            known_hosts,
            group,
        }) => {
            return cli::handle_import(config, file.as_deref(), known_hosts, group.as_deref());
        }
        Some(Commands::Sync {
            provider,
            dry_run,
            remove,
        }) => {
            return cli::handle_sync(config, provider.as_deref(), dry_run, remove);
        }
        Some(Commands::Tunnel { command }) => {
            return cli::handle_tunnel_command(config, command);
        }
        Some(Commands::Snippet { command }) => {
            return cli::handle_snippet_command(config, command, &config_path);
        }
        Some(Commands::Vault {
            command:
                VaultCommands::Sign {
                    alias,
                    all,
                    vault_addr: cli_vault_addr,
                },
        }) => {
            return cli::handle_vault_sign_command(config, alias, all, cli_vault_addr);
        }
        Some(Commands::Provider { .. })
        | Some(Commands::Update)
        | Some(Commands::Password { .. })
        | Some(Commands::Mcp)
        | Some(Commands::Theme { .. })
        | Some(Commands::Logs { .. })
        | Some(Commands::WhatsNew { .. }) => unreachable!(),
        None => {}
    }

    // Direct connect mode (--connect)
    if let Some(alias) = cli.connect {
        run_direct_connect(alias, &mut config, &config_path)?;
    }

    // List mode
    if cli.list {
        print_host_list(&config);
        return Ok(());
    }

    // Positional argument: exact match → connect, otherwise → TUI with filter
    if let Some(ref alias) = cli.alias {
        return run_positional_alias(
            alias,
            config,
            &config_path,
            repaired_groups,
            orphaned_headers,
        );
    }

    // Interactive TUI mode
    let mut app = App::new(config);
    app.post_init();
    apply_saved_sort(&mut app);
    if repaired_groups > 0 || orphaned_headers > 0 {
        app.notify(crate::messages::config_repaired(
            repaired_groups,
            orphaned_headers,
        ));
    }
    run_tui(app)
}

/// Collect environment + config metadata and write a startup banner to the
/// log file. Runs once at process start so support bundles always show
/// the SSH config path, active providers, askpass sources and Vault
/// posture under which purple ran.
fn write_startup_banner(config: &SshConfigFile, config_path: &Path, verbose: bool) {
    let level_str = logging::level_name(verbose);
    let provider_config = providers::config::ProviderConfig::load();

    let provider_names: Vec<String> = provider_config
        .sections
        .iter()
        .map(|s| s.provider.clone())
        .collect();

    let askpass_sources: Vec<String> = config
        .host_entries()
        .iter()
        .filter_map(|h| h.askpass.as_ref())
        .map(|s| s.to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let vault_ssh_info = {
        let has_host_level = config.host_entries().iter().any(|h| h.vault_ssh.is_some());
        let has_provider_level = provider_config
            .sections
            .iter()
            .any(|s| !s.vault_role.is_empty());
        if has_host_level || has_provider_level {
            // Resolve addr from all sources: per-host > per-provider > env var.
            let addr = config
                .host_entries()
                .iter()
                .find_map(|h| h.vault_addr.clone())
                .or_else(|| {
                    provider_config
                        .sections
                        .iter()
                        .find(|s| !s.vault_addr.is_empty())
                        .map(|s| s.vault_addr.clone())
                })
                .or_else(|| std::env::var("VAULT_ADDR").ok())
                .unwrap_or_else(|| "not set".to_string());
            Some(format!("enabled (addr={addr})"))
        } else {
            None
        }
    };

    let ssh_version = logging::detect_ssh_version();
    let term = std::env::var("TERM").unwrap_or_else(|_| "unset".to_string());
    let colorterm = std::env::var("COLORTERM").unwrap_or_else(|_| "unset".to_string());

    logging::write_banner(&logging::BannerInfo {
        version: env!("CARGO_PKG_VERSION"),
        config_path: &config_path.display().to_string(),
        providers: &provider_names,
        askpass_sources: &askpass_sources,
        vault_ssh_info: vault_ssh_info.as_deref(),
        ssh_version: &ssh_version,
        term: &term,
        colorterm: &colorterm,
        level: &level_str,
    });
}

/// Direct-connect mode (`purple --connect <alias>`): resolve askpass and
/// Vault SSH, run `ssh` inline and exit with its status code. Never
/// returns on success — always calls `std::process::exit`.
fn run_direct_connect(alias: String, config: &mut SshConfigFile, config_path: &Path) -> Result<()> {
    let provider_config = providers::config::ProviderConfig::load();
    let host_entry = config.host_entries().into_iter().find(|h| h.alias == alias);
    if let Some(ref host) = host_entry {
        if let Some((msg, _is_error)) =
            ensure_vault_ssh_if_needed(&alias, host, &provider_config, config)
        {
            eprintln!("{}", msg);
        }
    }
    let askpass = host_entry
        .as_ref()
        .and_then(|h| h.askpass.clone())
        .or_else(preferences::load_askpass_default);
    let bw_session = ensure_bw_session(None, askpass.as_deref());
    ensure_keychain_password(&alias, askpass.as_deref());
    let result = connection::connect(
        &alias,
        config_path,
        askpass.as_deref(),
        bw_session.as_deref(),
        false,
    )?;
    let code = result.status.code().unwrap_or(1);
    if code != 255 {
        history::ConnectionHistory::load().record(&alias);
    }
    askpass::cleanup_marker(&alias);
    std::process::exit(code);
}

/// Positional-alias mode (`purple <alias>`): if the alias is an exact
/// match, connect directly. Otherwise open the TUI with the alias
/// pre-filled as a search filter.
fn run_positional_alias(
    alias: &str,
    mut config: SshConfigFile,
    config_path: &Path,
    repaired_groups: usize,
    orphaned_headers: usize,
) -> Result<()> {
    let host_opt = config
        .host_entries()
        .iter()
        .find(|h| h.alias == *alias)
        .cloned();
    if let Some(host) = host_opt {
        let provider_config = providers::config::ProviderConfig::load();
        if let Some((msg, _is_error)) =
            ensure_vault_ssh_if_needed(&host.alias, &host, &provider_config, &mut config)
        {
            eprintln!("{}", msg);
        }
        let alias = host.alias.clone();
        let askpass = host
            .askpass
            .clone()
            .or_else(preferences::load_askpass_default);
        let bw_session = ensure_bw_session(None, askpass.as_deref());
        ensure_keychain_password(&alias, askpass.as_deref());
        print!("{}", crate::messages::cli::beaming_up(&alias));
        let result = connection::connect(
            &alias,
            config_path,
            askpass.as_deref(),
            bw_session.as_deref(),
            false,
        )?;
        let code = result.status.code().unwrap_or(1);
        if code != 255 {
            history::ConnectionHistory::load().record(&alias);
        }
        askpass::cleanup_marker(&alias);
        std::process::exit(code);
    }

    // No exact match — open TUI with search pre-filled.
    let mut app = App::new(config);
    app.post_init();
    apply_saved_sort(&mut app);
    if repaired_groups > 0 || orphaned_headers > 0 {
        app.notify(crate::messages::config_repaired(
            repaired_groups,
            orphaned_headers,
        ));
    }
    app.start_search_with(alias);
    if app.search.filtered_indices.is_empty() {
        app.notify(crate::messages::no_exact_match(alias));
    }
    run_tui(app)
}

/// Plain-text host listing for `purple --list`. Prints `alias user@host:port`
/// rows or the NO_HOSTS marker when the config has no Host blocks.
fn print_host_list(config: &SshConfigFile) {
    let entries = config.host_entries();
    if entries.is_empty() {
        println!("{}", crate::messages::cli::NO_HOSTS);
        return;
    }
    for host in &entries {
        let user = if host.user.is_empty() {
            String::new()
        } else {
            format!("{}@", host.user)
        };
        let port = if host.port == 22 {
            String::new()
        } else {
            format!(":{}", host.port)
        };
        println!("{:<20} {}{}{}", host.alias, user, host.hostname, port);
    }
}

fn apply_saved_sort(app: &mut App) {
    let saved = preferences::load_sort_mode();
    let group = preferences::load_group_by();
    app.hosts_state.sort_mode = saved;
    app.hosts_state.group_by = group;
    app.hosts_state.view_mode = preferences::load_view_mode();
    // Clear stale tag preference if the tag no longer exists in any host
    if app.clear_stale_group_tag() {
        if let Err(e) = preferences::save_group_by(&app.hosts_state.group_by) {
            app.notify_error(crate::messages::group_pref_reset_failed(&e));
        }
    }
    if saved != app::SortMode::Original || !matches!(app.hosts_state.group_by, app::GroupBy::None) {
        app.apply_sort();
        // After startup sort, select the first host in the sorted order
        // rather than preserving the arbitrary first-in-config selection.
        app.select_first_host();
    }
}

/// Build a rolling sync summary from completed providers.
/// Format a sync diff summary like "(+3 ~1 -2)" from add/update/stale counts.
/// Returns empty string when all counts are zero.
/// Build the status-bar summary shown after a bulk Vault SSH signing run
/// completes. When `failed > 0` and `first_error` is present, the scrubbed
/// error is appended so the user sees the actual reason (missing role,
/// permission denied, connection refused, etc.) instead of a bare
/// "1 failed" count.
/// Replace the spinner frame prefix in a status text. Returns None if the
/// text does not start with a known spinner frame.
///
/// Animated statuses MUST start with a character from
/// [`crate::animation::SPINNER_FRAMES`] followed by a space, otherwise
/// `event_loop::handle_tick` cannot rotate the frame and the animation
/// silently stops. This is an implicit contract upheld by:
///
/// - `messages::synced_progress` (footer batch summary)
/// - `messages::provider_progress` (per-provider progress)
/// - vault signing progress text in `handler::event_loop::handle_tick`
///
/// Any new code path that posts a status meant to animate must seed the
/// text with `SPINNER_FRAMES[0]` followed by a single space, and pass
/// through the `replace_spinner_frame` + `created_at` refresh dance in
/// `handle_tick`. Returning `None` is the intentional no-op for
/// non-animated statuses.
pub(crate) fn replace_spinner_frame(text: &str, new_frame: &str) -> Option<String> {
    let starts_with_spinner = crate::animation::SPINNER_FRAMES
        .iter()
        .any(|f| text.starts_with(f));
    if !starts_with_spinner {
        return None;
    }
    text.split_once(' ')
        .map(|(_, rest)| format!("{} {}", new_frame, rest))
}

pub(crate) fn format_vault_sign_summary(
    signed: u32,
    failed: u32,
    skipped: u32,
    first_error: Option<&str>,
) -> String {
    let total = signed + failed + skipped;
    let cert_word = if total == 1 {
        "certificate"
    } else {
        "certificates"
    };
    if failed > 0 {
        if let Some(err) = first_error {
            if total == 1 {
                // Single host: just show the error, no stats prefix
                return err.to_string();
            }
            format!(
                "Signed {} of {} {}. {} failed: {}",
                signed, total, cert_word, failed, err
            )
        } else {
            format!(
                "Signed {} of {} {}. {} failed",
                signed, total, cert_word, failed
            )
        }
    } else if skipped > 0 && signed == 0 {
        format!(
            "All {} {} already valid. Nothing to sign.",
            total, cert_word
        )
    } else if skipped > 0 {
        format!(
            "Signed {} of {} {}. {} already valid.",
            signed, total, cert_word, skipped
        )
    } else {
        format!("Signed {} of {} {}.", signed, total, cert_word)
    }
}

pub(crate) fn format_sync_diff(added: usize, updated: usize, stale: usize) -> String {
    let diff_parts: Vec<String> = [(added, "+"), (updated, "~"), (stale, "-")]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, prefix)| format!("{}{}", prefix, n))
        .collect();
    if diff_parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", diff_parts.join(" "))
    }
}

/// Footer status that surfaces in-flight providers as the batch progresses.
/// While a sync is running the line is
/// `⠋ Syncing AWS, Hetzner · 1/3 (+12 ~3 -1)`, where the leading char is a
/// braille spinner frame rotated by `event_loop::handle_tick` and the names
/// are the providers that have not yet reported back. Once every provider in
/// the batch has resolved the line becomes
/// `Synced 5/5 · AWS, DO, Vultr, Hetzner, Linode (+12 ~3 -1)` and the batch
/// state resets. Persists `sync_history.tsv` on completion.
pub(crate) fn set_sync_summary(app: &mut App) {
    let still_syncing = !app.providers.syncing.is_empty();
    let done = app.providers.sync_done.len();
    let total = app
        .providers
        .batch_total
        .max(done + app.providers.syncing.len());
    let added = app.providers.batch_added;
    let updated = app.providers.batch_updated;
    let stale = app.providers.batch_stale;
    if still_syncing {
        // Active providers (still in flight) are what the user wants to see —
        // especially when one is slow. Sort for stable rendering across ticks
        // (HashMap iteration order would otherwise jitter the footer).
        let mut active: Vec<String> = app
            .providers
            .syncing
            .keys()
            .map(|name| crate::providers::provider_display_name(name).to_string())
            .collect();
        active.sort();
        let active_names = active.join(", ");
        // Seed with frame 0; handle_tick rotates the prefix every ~100ms while
        // sync is active. Using SPINNER_FRAMES[0] keeps replace_spinner_frame's
        // contract (text must start with a known frame) intact from tick zero.
        let spinner = crate::animation::SPINNER_FRAMES[0];
        let text = crate::messages::synced_progress(
            spinner,
            &active_names,
            done,
            total,
            added,
            updated,
            stale,
        );
        if app.providers.sync_had_errors {
            app.notify_background_error(text);
        } else {
            app.notify_background(text);
        }
    } else {
        let names = app.providers.sync_done.join(", ");
        let text = crate::messages::synced_done(done, total, &names, added, updated, stale);
        if app.providers.sync_had_errors {
            app.notify_background_error(text);
        } else {
            app.notify_background(text);
        }
        app.providers.sync_done.clear();
        app.providers.sync_had_errors = false;
        app.providers.batch_added = 0;
        app.providers.batch_updated = 0;
        app.providers.batch_stale = 0;
        app.providers.batch_total = 0;
        app::SyncRecord::save_all(&app.providers.sync_history);
    }
}

/// First-launch initialization: create ~/.purple/ and back up the original SSH config.
/// Returns `Some(has_backup)` if this was a first launch, or `None` if already initialized.
///
/// Detection is deliberately forgiving. The `~/.purple/` directory itself is
/// not a first-launch signal because `logging::init` creates it early to
/// place the log file. Instead, check a set of markers written by long-lived
/// purple features. Users who installed before `config.original` existed
/// still have preferences, history or a container cache, so we treat any of
/// those as "already initialized" and do not show Welcome again.
pub(crate) fn first_launch_init(purple_dir: &Path, config_path: &Path) -> Option<bool> {
    let markers = [
        "config.original",
        "preferences",
        "history.tsv",
        "container_cache.jsonl",
        "last_version_check",
        "providers",
        "snippets.toml",
        "themes",
    ];
    if markers.iter().any(|m| purple_dir.join(m).exists()) {
        return None;
    }
    if let Err(e) = std::fs::create_dir_all(purple_dir) {
        warn!("[config] Failed to create ~/.purple directory: {e}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(purple_dir, std::fs::Permissions::from_mode(0o700))
        {
            warn!("[config] Failed to set ~/.purple directory permissions: {e}");
        }
    }
    // One-time backup of the original SSH config before purple touches it.
    // Stored as config.original and never overwritten or pruned.
    let original_backup = purple_dir.join("config.original");
    if config_path.exists() {
        if let Err(e) = std::fs::copy(config_path, &original_backup) {
            warn!(
                "[config] Failed to backup SSH config to {}: {e}",
                original_backup.display()
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) =
                std::fs::set_permissions(&original_backup, std::fs::Permissions::from_mode(0o600))
            {
                warn!("[config] Failed to set backup permissions: {e}");
            }
        }
    }
    Some(original_backup.exists())
}

/// Check and renew Vault SSH certificate if the host has a vault role configured.
/// Writes the cert file to ~/.purple/certs/ AND sets CertificateFile on the host
/// block when it is empty, so `ssh` actually uses the freshly signed cert.
///
/// Returns `Some(message)` when a signing action was attempted (success or failure),
/// `None` when no vault role is configured or the cert is still valid.
pub(crate) fn ensure_vault_ssh_if_needed(
    alias: &str,
    host: &ssh_config::model::HostEntry,
    provider_config: &providers::config::ProviderConfig,
    config: &mut ssh_config::model::SshConfigFile,
) -> Option<(String, bool)> {
    let role = vault_ssh::resolve_vault_role(
        host.vault_ssh.as_deref(),
        host.provider.as_deref(),
        provider_config,
    )?;

    let pubkey = match vault_ssh::resolve_pubkey_path(&host.identity_file) {
        Ok(p) => p,
        Err(e) => return Some((format!("Vault SSH cert failed: {}", e), true)),
    };

    // Check if the cert needs renewal before calling ensure_cert, so we can
    // distinguish "renewed" from "already valid" for status feedback.
    let check_path = vault_ssh::resolve_cert_path(alias, &host.certificate_file).ok()?;
    let status = vault_ssh::check_cert_validity(&check_path);
    if !vault_ssh::needs_renewal(&status) {
        return None; // Cert valid, no action needed
    }

    // Resolve the Vault address at signing time (host override > provider
    // default > None). None lets the `vault` CLI use its own env resolution.
    let vault_addr = vault_ssh::resolve_vault_addr(
        host.vault_addr.as_deref(),
        host.provider.as_deref(),
        provider_config,
    );
    match vault_ssh::ensure_cert(
        &role,
        &pubkey,
        alias,
        &host.certificate_file,
        vault_addr.as_deref(),
    ) {
        Ok(cert_path) => {
            // If the host block did not already set CertificateFile, wire the
            // freshly signed cert into the SSH config so `ssh` actually uses it.
            // Otherwise the cert on disk is silently ignored.
            if should_write_certificate_file(&host.certificate_file) {
                let cert_str = cert_path.to_string_lossy().to_string();
                let updated = config.set_host_certificate_file(alias, &cert_str);
                if !updated {
                    eprintln!(
                        "Warning: Signed cert for {} but host block is no longer in ssh config; CertificateFile not written (cert saved to {})",
                        alias,
                        cert_path.display()
                    );
                } else if let Err(e) = config.write() {
                    eprintln!(
                        "Warning: Signed cert for {} but failed to update SSH config CertificateFile: {}",
                        alias, e
                    );
                }
            }
            Some((format!("Signed SSH certificate for {}.", alias), false))
        }
        Err(e) => {
            eprintln!("Warning: Vault SSH signing failed: {}", e);
            Some((format!("Vault SSH signing failed: {}", e), true))
        }
    }
}

/// Decide whether `ensure_vault_ssh_if_needed` (and the equivalent
/// `VaultSignResult` event handler, the `purple vault sign` CLI paths and
/// every host-form mutator) should write a `CertificateFile` directive after a
/// successful Vault SSH signing.
///
/// The rule is simple but load-bearing: only write when the host has no
/// existing `CertificateFile`. A user-set custom path must never be silently
/// overwritten with purple's default cert path. Whitespace-only values count
/// as empty so that a stray space typed in the form does not lock purple out
/// of writing the directive.
pub(crate) fn should_write_certificate_file(existing: &str) -> bool {
    existing.trim().is_empty()
}

/// Pre-flight check for Bitwarden vault. If the askpass source uses `bw:` and
/// no session token is cached, prompts the user to unlock the vault.
/// Returns Some(token) only when a new token was obtained. None means no action needed.
pub(crate) fn ensure_bw_session(existing: Option<&str>, askpass: Option<&str>) -> Option<String> {
    let askpass = askpass?;
    if !askpass.starts_with("bw:") || existing.is_some() {
        return None;
    }
    // Check vault status
    let status = askpass::bw_vault_status();
    match status {
        askpass::BwStatus::Unlocked => {
            // Vault already unlocked (e.g. BW_SESSION in environment). No action needed.
            None
        }
        askpass::BwStatus::NotInstalled => {
            eprintln!("{}", crate::messages::askpass::BW_NOT_FOUND);
            None
        }
        askpass::BwStatus::NotAuthenticated => {
            eprintln!("{}", crate::messages::askpass::BW_NOT_LOGGED_IN);
            None
        }
        askpass::BwStatus::Locked => {
            // Prompt for master password and unlock
            for attempt in 0..2 {
                let password = match cli::prompt_hidden_input("Bitwarden master password: ") {
                    Ok(Some(p)) if !p.is_empty() => p,
                    Ok(Some(_)) => {
                        eprintln!("{}", crate::messages::askpass::EMPTY_PASSWORD);
                        return None;
                    }
                    Ok(None) => {
                        // User pressed Esc
                        return None;
                    }
                    Err(e) => {
                        eprintln!("{}", crate::messages::askpass::read_failed(&e));
                        return None;
                    }
                };
                match askpass::bw_unlock(&password) {
                    Ok(token) => return Some(token),
                    Err(e) => {
                        if attempt == 0 {
                            eprintln!("{}", crate::messages::askpass::unlock_failed_retry(&e));
                        } else {
                            eprintln!("{}", crate::messages::askpass::unlock_failed_prompt(&e));
                        }
                    }
                }
            }
            None
        }
    }
}

/// Pre-flight check for keychain password. If the askpass source is `keychain` and
/// no password is stored yet, prompts the user to enter one and stores it.
pub(crate) fn ensure_keychain_password(alias: &str, askpass: Option<&str>) {
    if askpass != Some("keychain") {
        return;
    }
    // Check if password already exists
    if askpass::keychain_has_password(alias) {
        return;
    }
    // Prompt for password and store it
    let password =
        match cli::prompt_hidden_input(&format!("Password for {} (stored in keychain): ", alias)) {
            Ok(Some(p)) if !p.is_empty() => p,
            Ok(Some(_)) => {
                eprintln!("{}", crate::messages::askpass::EMPTY_PASSWORD);
                return;
            }
            Ok(None) => return, // Esc
            Err(_) => return,
        };
    match askpass::store_in_keychain(alias, &password) {
        Ok(()) => eprintln!("{}", crate::messages::askpass::PASSWORD_IN_KEYCHAIN),
        Err(e) => eprintln!(
            "Failed to store in keychain: {}. SSH will prompt for password.",
            e
        ),
    }
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "visual_regression_tests.rs"]
mod visual_regression_tests;
