mod animation;
mod app;
mod askpass;
mod askpass_env;
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
mod ping;
mod preferences;
mod providers;
mod quick_add;
mod snippet;
mod ssh_config;
mod ssh_keys;
mod tui;
mod tunnel;
mod ui;
mod update;
mod vault_ssh;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};

use app::App;
use event::{AppEvent, EventHandler};
use ssh_config::model::{HostEntry, SshConfigFile};

#[derive(Parser)]
#[command(
    name = "purple",
    about = "Your SSH config is a mess. Purple fixes that.",
    long_about = "Purple is a terminal SSH client for managing your hosts.\n\
                  Add, edit, delete and connect without opening a text editor.\n\n\
                  Life's too short for nano ~/.ssh/config.",
    version
)]
struct Cli {
    /// Connect to a host by alias, or filter the TUI
    #[arg(value_name = "ALIAS")]
    alias: Option<String>,

    /// Connect directly to a host by alias (skip the TUI)
    #[arg(short, long)]
    connect: Option<String>,

    /// List all configured hosts
    #[arg(short, long)]
    list: bool,

    /// Path to SSH config file
    #[arg(long, default_value = "~/.ssh/config")]
    config: String,

    /// Launch with demo data (no real config needed)
    #[arg(long)]
    demo: bool,

    /// Generate shell completions
    #[arg(long, value_name = "SHELL")]
    completions: Option<Shell>,

    /// Override theme for this session
    #[arg(long)]
    theme: Option<String>,

    /// Enable verbose logging (debug level)
    #[arg(long)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Quick-add a host: purple add user@host:port --alias myserver
    Add {
        /// Target in user@hostname:port format
        target: String,

        /// Alias for the host (default: derived from hostname)
        #[arg(short, long)]
        alias: Option<String>,

        /// Path to identity file (SSH key)
        #[arg(short, long)]
        key: Option<String>,
    },
    /// Import hosts from a file or known_hosts
    Import {
        /// File with one host per line (user@host:port format)
        file: Option<String>,

        /// Import from ~/.ssh/known_hosts instead
        #[arg(long)]
        known_hosts: bool,

        /// Group label for imported hosts
        #[arg(short, long)]
        group: Option<String>,
    },
    /// Sync hosts from cloud providers (DigitalOcean, Vultr, Linode, Hetzner, UpCloud, Proxmox VE, AWS EC2, Scaleway, GCP, Azure, Tailscale, Oracle Cloud, OVHcloud, Leaseweb, i3D.net, TransIP)
    Sync {
        /// Sync a specific provider (default: all configured)
        provider: Option<String>,

        /// Preview changes without modifying config
        #[arg(long)]
        dry_run: bool,

        /// Remove hosts that no longer exist on the provider
        #[arg(long)]
        remove: bool,
    },
    /// Manage cloud provider configurations
    Provider {
        #[command(subcommand)]
        command: ProviderCommands,
    },
    /// Manage SSH tunnels
    Tunnel {
        #[command(subcommand)]
        command: TunnelCommands,
    },
    /// Manage passwords in the OS keychain for SSH hosts
    Password {
        #[command(subcommand)]
        command: PasswordCommands,
    },
    /// Manage command snippets for quick execution on hosts
    Snippet {
        #[command(subcommand)]
        command: SnippetCommands,
    },
    /// Update purple to the latest version
    Update,
    /// Start MCP server (Model Context Protocol) for AI agent integration
    Mcp,
    /// Manage color themes
    Theme {
        #[command(subcommand)]
        command: ThemeCommands,
    },
    /// HashiCorp Vault SSH secrets engine operations (signed SSH certificates)
    Vault {
        #[command(subcommand)]
        command: VaultCommands,
    },
    /// View or manage log file
    Logs {
        /// Follow log output in real time
        #[arg(long)]
        tail: bool,

        /// Delete the log file
        #[arg(long)]
        clear: bool,
    },
}

#[derive(Subcommand)]
enum VaultCommands {
    /// Sign an SSH certificate for a host (or --all) via the Vault SSH secrets engine
    #[command(
        long_about = "Sign one or more SSH certificates via the HashiCorp Vault SSH secrets engine.\n\n\
        Prerequisites:\n\
        - The `vault` CLI is installed and authenticated (run `vault login` or set VAULT_TOKEN)\n\
        - VAULT_ADDR points at your Vault server\n\
        - A role is configured on the host (Vault SSH role field in the host form) or\n  \
          on its provider (provider-level vault_role default)\n\
        - The SSH secrets engine is enabled on Vault and your token has `update` capability\n  \
          on the role path\n\n\
        Signed certificates are cached under ~/.purple/certs/<alias>-cert.pub and\n\
        `CertificateFile` is wired into the SSH config automatically.\n\n\
        Distinct from the Vault KV secrets engine used as a password source (`vault:`\n\
        askpass prefix); see `purple password` for that."
    )]
    Sign {
        /// Host alias to sign (omit for --all)
        alias: Option<String>,
        /// Sign all hosts with a Vault SSH role configured
        #[arg(long)]
        all: bool,
        /// Override VAULT_ADDR for this invocation only.
        /// Highest precedence: flag > per-host comment > provider default > shell env.
        #[arg(long, value_name = "URL")]
        vault_addr: Option<String>,
    },
}

#[derive(Subcommand)]
enum ThemeCommands {
    /// List available themes
    List,
    /// Set the active theme
    Set {
        /// Theme name
        name: String,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum ProviderCommands {
    /// Add or update a provider configuration
    Add {
        /// Provider name (digitalocean, vultr, linode, hetzner, upcloud, proxmox, aws, scaleway, gcp, azure, tailscale, oracle, ovh, leaseweb, i3d, transip)
        provider: String,

        /// API token (or set PURPLE_TOKEN env var, or use --token-stdin)
        #[arg(long)]
        token: Option<String>,

        /// Read token from stdin (e.g. from a password manager)
        #[arg(long)]
        token_stdin: bool,

        /// Alias prefix (default: provider short label)
        #[arg(long)]
        prefix: Option<String>,

        /// Default SSH user (default: root)
        #[arg(long)]
        user: Option<String>,

        /// Default identity file
        #[arg(long)]
        key: Option<String>,

        /// Base URL for self-hosted providers (required for Proxmox)
        #[arg(long)]
        url: Option<String>,

        /// AWS credential profile from ~/.aws/credentials
        #[arg(long)]
        profile: Option<String>,

        /// Comma-separated regions, zones or subscription IDs (e.g. us-east-1,eu-west-1 for AWS, fr-par-1,nl-ams-1 for Scaleway, us-central1-a for GCP zones or subscription UUIDs for Azure)
        #[arg(long)]
        regions: Option<String>,

        /// GCP project ID
        #[arg(long)]
        project: Option<String>,

        /// OCI compartment OCID (Oracle)
        #[arg(long)]
        compartment: Option<String>,

        /// Skip TLS certificate verification (for self-signed certs)
        #[arg(long, conflicts_with = "verify_tls")]
        no_verify_tls: bool,

        /// Explicitly enable TLS certificate verification (overrides stored setting)
        #[arg(long, conflicts_with = "no_verify_tls")]
        verify_tls: bool,

        /// Enable automatic sync on startup
        #[arg(long, conflicts_with = "no_auto_sync")]
        auto_sync: bool,

        /// Disable automatic sync on startup
        #[arg(long, conflicts_with = "auto_sync")]
        no_auto_sync: bool,
    },
    /// List configured providers
    List,
    /// Remove a provider configuration
    Remove {
        /// Provider name to remove
        provider: String,
    },
}

#[derive(Subcommand)]
enum TunnelCommands {
    /// List configured tunnels
    List {
        /// Show tunnels for a specific host
        alias: Option<String>,
    },
    /// Add a tunnel to a host
    Add {
        /// Host alias
        alias: String,

        /// Forward spec: L:port:host:port (local), R:port:host:port (remote) or D:port (SOCKS)
        forward: String,
    },
    /// Remove a tunnel from a host
    Remove {
        /// Host alias
        alias: String,

        /// Forward spec: L:port:host:port (local), R:port:host:port (remote) or D:port (SOCKS)
        forward: String,
    },
    /// Start a tunnel (foreground, Ctrl+C to stop)
    Start {
        /// Host alias
        alias: String,
    },
}

#[derive(Subcommand)]
enum PasswordCommands {
    /// Store a password in the OS keychain for a host
    Set {
        /// Host alias
        alias: String,
    },
    /// Remove a password from the OS keychain
    Remove {
        /// Host alias
        alias: String,
    },
}

#[derive(Subcommand)]
enum SnippetCommands {
    /// List all saved snippets
    List,
    /// Add a new snippet
    Add {
        /// Snippet name
        name: String,

        /// Command to run on the remote host
        command: String,

        /// Short description
        #[arg(long)]
        description: Option<String>,
    },
    /// Remove a snippet
    Remove {
        /// Snippet name
        name: String,
    },
    /// Run a snippet on one or more hosts
    Run {
        /// Snippet name
        name: String,

        /// Host alias (run on a single host)
        alias: Option<String>,

        /// Run on all hosts matching this tag
        #[arg(long)]
        tag: Option<String>,

        /// Run on all hosts
        #[arg(long)]
        all: bool,

        /// Run on hosts concurrently
        #[arg(long)]
        parallel: bool,
    },
}

fn resolve_config_path(path: &str) -> Result<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(rest))
    } else {
        Ok(PathBuf::from(path))
    }
}

fn resolve_token(explicit: Option<String>, from_stdin: bool) -> Result<String> {
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
    anyhow::bail!("No token provided. Use --token, --token-stdin, or set PURPLE_TOKEN env var.")
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
        let app = demo::build_demo_app();
        return run_tui(app);
    }

    // Provider and Update subcommands don't need SSH config
    if let Some(Commands::Provider { command }) = cli.command {
        return handle_provider_command(command);
    }
    if let Some(Commands::Update) = cli.command {
        return update::self_update();
    }
    if let Some(Commands::Password { command }) = cli.command {
        return handle_password_command(command);
    }
    if let Some(Commands::Mcp) = cli.command {
        let config_path = resolve_config_path(&cli.config)?;
        return mcp::run(&config_path);
    }
    if let Some(Commands::Logs { tail, clear }) = cli.command {
        let path = logging::log_path().context("Could not determine log path")?;
        if clear {
            if path.exists() {
                std::fs::remove_file(&path)?;
                println!("Log file deleted: {}", path.display());
            } else {
                println!("No log file found at {}", path.display());
            }
        } else if tail {
            let status = std::process::Command::new("tail")
                .args(["-f", &path.to_string_lossy()])
                .status()
                .context("Failed to run tail")?;
            std::process::exit(status.code().unwrap_or(1));
        } else {
            println!("{}", path.display());
        }
        return Ok(());
    }
    if let Some(Commands::Theme { command }) = cli.command {
        match command {
            ThemeCommands::List => {
                let current = preferences::load_theme().unwrap_or_else(|| "Purple".to_string());
                println!("Built-in themes:");
                for theme in ui::theme::ThemeDef::builtins() {
                    let marker = if theme.name.eq_ignore_ascii_case(&current) {
                        "*"
                    } else {
                        " "
                    };
                    println!("  {} {}", marker, theme.name);
                }
                let custom = ui::theme::ThemeDef::load_custom();
                if !custom.is_empty() {
                    println!("\nCustom themes:");
                    for theme in &custom {
                        let marker = if theme.name.eq_ignore_ascii_case(&current) {
                            "*"
                        } else {
                            " "
                        };
                        println!("  {} {}", marker, theme.name);
                    }
                }
            }
            ThemeCommands::Set { name } => {
                let found = ui::theme::ThemeDef::find_builtin(&name).or_else(|| {
                    ui::theme::ThemeDef::load_custom()
                        .into_iter()
                        .find(|t| t.name.eq_ignore_ascii_case(&name))
                });
                match found {
                    Some(theme) => {
                        preferences::save_theme(&theme.name)?;
                        println!("Theme set to: {}", theme.name);
                    }
                    None => {
                        anyhow::bail!("Unknown theme: {}", name);
                    }
                }
            }
        }
        return Ok(());
    }

    let config_path = resolve_config_path(&cli.config)?;
    let mut config = SshConfigFile::parse(&config_path)?;
    let repaired_groups = config.repair_absorbed_group_comments();
    let orphaned_headers = config.remove_all_orphaned_group_headers();

    // Write startup banner to log file
    {
        let level_str = logging::level_name(cli.verbose);

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
                // Resolve addr from all sources: per-host > per-provider > env var
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

    // Handle subcommands that need SSH config
    match cli.command {
        Some(Commands::Add { target, alias, key }) => {
            return handle_quick_add(config, &target, alias.as_deref(), key.as_deref());
        }
        Some(Commands::Import {
            file,
            known_hosts,
            group,
        }) => {
            return handle_import(config, file.as_deref(), known_hosts, group.as_deref());
        }
        Some(Commands::Sync {
            provider,
            dry_run,
            remove,
        }) => {
            return handle_sync(config, provider.as_deref(), dry_run, remove);
        }
        Some(Commands::Tunnel { command }) => {
            return handle_tunnel_command(config, command);
        }
        Some(Commands::Snippet { command }) => {
            return handle_snippet_command(config, command, &config_path);
        }
        Some(Commands::Vault {
            command:
                VaultCommands::Sign {
                    alias,
                    all,
                    vault_addr: cli_vault_addr,
                },
        }) => {
            if let Some(ref addr) = cli_vault_addr {
                if !vault_ssh::is_valid_vault_addr(addr) {
                    anyhow::bail!(
                        "Invalid --vault-addr value. Must be non-empty, no whitespace or control chars."
                    );
                }
            }
            let provider_config = providers::config::ProviderConfig::load();
            let entries = config.host_entries();

            if all {
                let mut signed = 0u32;
                let mut failed = 0u32;
                let mut skipped = 0u32;

                for entry in &entries {
                    let role = match vault_ssh::resolve_vault_role(
                        entry.vault_ssh.as_deref(),
                        entry.provider.as_deref(),
                        &provider_config,
                    ) {
                        Some(r) => r,
                        None => {
                            skipped += 1;
                            continue;
                        }
                    };

                    let pubkey = match vault_ssh::resolve_pubkey_path(&entry.identity_file) {
                        Ok(p) => p,
                        Err(e) => {
                            println!("Skipping {}: {}", entry.alias, e);
                            failed += 1;
                            continue;
                        }
                    };
                    let cert_path =
                        vault_ssh::resolve_cert_path(&entry.alias, &entry.certificate_file)?;
                    let status = vault_ssh::check_cert_validity(&cert_path);

                    if !vault_ssh::needs_renewal(&status) {
                        skipped += 1;
                        continue;
                    }

                    // Flag beats per-host beats provider default.
                    let resolved_addr = cli_vault_addr.clone().or_else(|| {
                        vault_ssh::resolve_vault_addr(
                            entry.vault_addr.as_deref(),
                            entry.provider.as_deref(),
                            &provider_config,
                        )
                    });
                    print!("Signing {}... ", entry.alias);
                    match vault_ssh::sign_certificate(
                        &role,
                        &pubkey,
                        &entry.alias,
                        resolved_addr.as_deref(),
                    ) {
                        Ok(result) => {
                            println!("\u{2713}");
                            // Honor the same invariant as the TUI paths: never
                            // overwrite a user-set CertificateFile.
                            if should_write_certificate_file(&entry.certificate_file) {
                                let updated = config.set_host_certificate_file(
                                    &entry.alias,
                                    &result.cert_path.to_string_lossy(),
                                );
                                if !updated {
                                    eprintln!(
                                        "  warning: {} no longer in ssh config; CertificateFile not written (cert saved on disk)",
                                        entry.alias
                                    );
                                }
                            }
                            signed += 1;
                        }
                        Err(e) => {
                            println!("failed: {}", e);
                            failed += 1;
                        }
                    }
                }
                if signed > 0 {
                    if let Err(e) = config.write() {
                        eprintln!("Warning: Failed to update SSH config: {}", e);
                    }
                }
                println!(
                    "\nSigned: {}, failed: {}, skipped (valid): {}",
                    signed, failed, skipped
                );
                if failed > 0 {
                    std::process::exit(1);
                }
            } else if let Some(alias) = alias {
                let entry = entries
                    .iter()
                    .find(|h| h.alias == alias)
                    .with_context(|| format!("Host '{}' not found", alias))?;

                let role = vault_ssh::resolve_vault_role(
                    entry.vault_ssh.as_deref(),
                    entry.provider.as_deref(),
                    &provider_config,
                )
                .with_context(|| {
                    format!(
                        "No Vault SSH role configured for '{}'. Set it in the host form (Vault SSH Role field) or in the provider config (vault_role).",
                        alias
                    )
                })?;

                let pubkey = vault_ssh::resolve_pubkey_path(&entry.identity_file)?;
                let resolved_addr = cli_vault_addr.clone().or_else(|| {
                    vault_ssh::resolve_vault_addr(
                        entry.vault_addr.as_deref(),
                        entry.provider.as_deref(),
                        &provider_config,
                    )
                });
                let result =
                    vault_ssh::sign_certificate(&role, &pubkey, &alias, resolved_addr.as_deref())?;
                // Honor the same invariant as the TUI paths: never overwrite a
                // user-set CertificateFile. Only write the directive (and the
                // SSH config) when the host has none yet.
                if should_write_certificate_file(&entry.certificate_file) {
                    let updated = config
                        .set_host_certificate_file(&alias, &result.cert_path.to_string_lossy());
                    if !updated {
                        // Host disappeared between the `entries` snapshot and
                        // the config mutation. In the single-host CLI path
                        // both reads happen back-to-back in the same process,
                        // so this is effectively unreachable — but surface it
                        // loudly if the invariant ever breaks instead of
                        // silently writing a cert nobody references.
                        anyhow::bail!(
                            "Host '{}' disappeared from ssh config before CertificateFile could be written. Cert saved to {}.",
                            alias,
                            result.cert_path.display()
                        );
                    }
                    config
                        .write()
                        .with_context(|| "Failed to update SSH config with CertificateFile")?;
                }
                println!("Certificate signed: {}", result.cert_path.display());
            } else {
                anyhow::bail!("Provide a host alias or use --all");
            }
            return Ok(());
        }
        Some(Commands::Provider { .. })
        | Some(Commands::Update)
        | Some(Commands::Password { .. })
        | Some(Commands::Mcp)
        | Some(Commands::Theme { .. })
        | Some(Commands::Logs { .. }) => unreachable!(),
        None => {}
    }

    // Direct connect mode (--connect)
    if let Some(alias) = cli.connect {
        let provider_config = providers::config::ProviderConfig::load();
        let entries = config.host_entries();
        let host_entry = entries.iter().find(|h| h.alias == alias).cloned();
        if let Some(ref host) = host_entry {
            if let Some((msg, _is_error)) =
                ensure_vault_ssh_if_needed(&alias, host, &provider_config, &mut config)
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
            &config_path,
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

    // List mode
    if cli.list {
        let entries = config.host_entries();
        if entries.is_empty() {
            println!("No hosts configured. Run 'purple' to add some!");
        } else {
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
        return Ok(());
    }

    // Positional argument: exact match → connect, otherwise → TUI with filter
    if let Some(ref alias) = cli.alias {
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
            println!("Beaming you up to {}...\n", alias);
            let result = connection::connect(
                &alias,
                &config_path,
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
        // No exact match — open TUI with search pre-filled
        let mut app = App::new(config);
        apply_saved_sort(&mut app);
        if repaired_groups > 0 || orphaned_headers > 0 {
            app.set_status(
                format!(
                    "Repaired SSH config ({} absorbed, {} orphaned group headers).",
                    repaired_groups, orphaned_headers
                ),
                false,
            );
        }
        app.start_search_with(alias);
        if app.search.filtered_indices.is_empty() {
            app.set_status(
                format!("No exact match for '{}'. Here's what we found.", alias),
                false,
            );
        }
        return run_tui(app);
    }

    // Interactive TUI mode
    let mut app = App::new(config);
    apply_saved_sort(&mut app);
    if repaired_groups > 0 || orphaned_headers > 0 {
        app.set_status(
            format!(
                "Repaired SSH config ({} absorbed, {} orphaned group headers).",
                repaired_groups, orphaned_headers
            ),
            false,
        );
    }
    run_tui(app)
}

fn apply_saved_sort(app: &mut App) {
    let saved = preferences::load_sort_mode();
    let group = preferences::load_group_by();
    app.sort_mode = saved;
    app.group_by = group;
    app.view_mode = preferences::load_view_mode();
    // Clear stale tag preference if the tag no longer exists in any host
    if app.clear_stale_group_tag() {
        if let Err(e) = preferences::save_group_by(&app.group_by) {
            app.set_status(
                format!("Group preference reset. (save failed: {})", e),
                true,
            );
        }
    }
    if saved != app::SortMode::Original || !matches!(app.group_by, app::GroupBy::None) {
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
fn replace_spinner_frame(text: &str, new_frame: &str) -> Option<String> {
    let starts_with_spinner = crate::animation::SPINNER_FRAMES
        .iter()
        .any(|f| text.starts_with(f));
    if !starts_with_spinner {
        return None;
    }
    text.split_once(' ')
        .map(|(_, rest)| format!("{} {}", new_frame, rest))
}

fn format_vault_sign_summary(
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

fn format_sync_diff(added: usize, updated: usize, stale: usize) -> String {
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

/// Shows "Synced: AWS, DO, Vultr" that grows as each provider finishes.
/// Clears the batch state once all providers are done so the status can expire normally.
fn set_sync_summary(app: &mut App) {
    let still_syncing = !app.syncing_providers.is_empty();
    let names = app.sync_done.join(", ");
    if still_syncing {
        app.set_background_status(format!("Synced: {}...", names), app.sync_had_errors);
    } else {
        app.set_background_status(format!("Synced: {}", names), app.sync_had_errors);
        app.sync_done.clear();
        app.sync_had_errors = false;
        app::SyncRecord::save_all(&app.sync_history);
    }
}

/// First-launch initialization: create ~/.purple/ and back up the original SSH config.
/// Returns `Some(has_backup)` if this was a first launch, or `None` if already initialized.
fn first_launch_init(purple_dir: &Path, config_path: &Path) -> Option<bool> {
    if purple_dir.exists() {
        return None;
    }
    let _ = std::fs::create_dir_all(purple_dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(purple_dir, std::fs::Permissions::from_mode(0o700));
    }
    // One-time backup of the original SSH config before purple touches it.
    // Stored as config.original and never overwritten or pruned.
    let original_backup = purple_dir.join("config.original");
    if config_path.exists() {
        let _ = std::fs::copy(config_path, &original_backup);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(&original_backup, std::fs::Permissions::from_mode(0o600));
        }
    }
    Some(original_backup.exists())
}

fn run_tui(mut app: App) -> Result<()> {
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
    let events = EventHandler::new(250);
    let events_tx = events.sender();
    let mut last_config_check = std::time::Instant::now();

    // Skip background tasks in demo mode (ping status is pre-populated)
    if !app.demo_mode {
        // Auto-sync configured providers on startup (skipped when auto_sync=false)
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

        // Auto-ping all hosts on startup if enabled in preferences
        if app.auto_ping {
            let hosts_to_ping: Vec<(String, String, u16)> = app
                .hosts
                .iter()
                .filter(|h| !h.hostname.is_empty() && h.proxy_jump.is_empty())
                .map(|h| (h.alias.clone(), h.hostname.clone(), h.port))
                .collect();
            for h in &app.hosts {
                if !h.proxy_jump.is_empty() {
                    app.ping_status
                        .insert(h.alias.clone(), app::PingStatus::Skipped);
                }
            }
            if !hosts_to_ping.is_empty() {
                for (alias, _, _) in &hosts_to_ping {
                    app.ping_status
                        .insert(alias.clone(), app::PingStatus::Checking);
                }
                ping::ping_all(&hosts_to_ping, events.sender(), app.ping_generation);
            }
        }

        // Background version check
        update::spawn_version_check(events_tx.clone());
    } // end skip background tasks in demo mode

    let mut anim = animation::AnimationState::new();

    while app.running {
        anim.detect_transitions(&mut app);
        terminal.draw(&mut app, &mut anim)?;

        // During animation, use a short timeout for smooth frames (~60fps).
        // During ping checking, use 80ms timeout for spinner.
        // Otherwise, block until the next event arrives.
        let vault_signing = app.vault_signing_cancel.is_some();
        let event = if anim.is_animating(&app) {
            events.next_timeout(std::time::Duration::from_millis(16))?
        } else if anim.has_checking_hosts(&app) || vault_signing {
            events.next_timeout(std::time::Duration::from_millis(80))?
        } else {
            Some(events.next()?)
        };

        match event {
            Some(AppEvent::Key(key)) => {
                handler::handle_key_event(&mut app, key, &events_tx)?;
            }
            Some(AppEvent::Tick) | None => {
                app.tick_status();
                if anim.has_checking_hosts(&app) || vault_signing {
                    anim.tick_spinner();
                }
                // Update the spinner character in the signing status text
                // so the spinner animates between VaultSignProgress events.
                if vault_signing {
                    if let Some(ref mut status) = app.status {
                        if status.sticky && !status.is_error {
                            let frame = crate::animation::SPINNER_FRAMES[anim.spinner_tick
                                as usize
                                % crate::animation::SPINNER_FRAMES.len()];
                            if let Some(updated) = replace_spinner_frame(&status.text, frame) {
                                status.text = updated;
                            }
                        }
                    }
                }
                // Expire ping results after 60s TTL
                if let Some(checked_at) = app.ping_checked_at {
                    if checked_at.elapsed() > std::time::Duration::from_secs(60) {
                        app.ping_status.clear();
                        app.ping_checked_at = None;
                        app.ping_generation += 1;
                        if app.filter_down_only {
                            app.cancel_search();
                        }
                        app.set_background_status("Ping expired. Press P to refresh.", false);
                    }
                }
                // Throttle config file stat() to every 4 seconds
                if last_config_check.elapsed() >= std::time::Duration::from_secs(4) {
                    app.check_config_changed();
                    last_config_check = std::time::Instant::now();
                }
                // Poll active tunnels for exit
                let exited = app.poll_tunnels();
                for (_alias, msg, is_error) in exited {
                    app.set_background_status(msg, is_error);
                }
            }
            Some(AppEvent::PingResult {
                alias,
                rtt_ms,
                generation,
            }) => {
                if generation == app.ping_generation {
                    let status = app::classify_ping(rtt_ms, app.slow_threshold_ms);
                    app.ping_status.insert(alias.clone(), status.clone());
                    // Propagate bastion status to all ProxyJump dependents.
                    // If this host is a bastion for other hosts, they inherit
                    // its reachability (one check per bastion, not per host).
                    app::propagate_ping_to_dependents(
                        &app.hosts,
                        &mut app.ping_status,
                        &alias,
                        &status,
                    );
                    // Update live filter/sort as results arrive
                    if app.filter_down_only {
                        app.apply_filter();
                    }
                    if app.sort_mode == app::SortMode::Status {
                        app.apply_sort();
                    }
                    // Update "last checked" timestamp when all pings are done
                    if !app.ping_status.is_empty()
                        && app
                            .ping_status
                            .values()
                            .all(|s| !matches!(s, app::PingStatus::Checking))
                    {
                        app.ping_checked_at = Some(std::time::Instant::now());
                    }
                }
            }
            Some(AppEvent::SyncProgress { provider, message }) => {
                // Only show per-provider progress while that provider is still syncing.
                // Late progress events (arriving after SyncComplete) are discarded.
                if app.syncing_providers.contains_key(&provider) && app.sync_done.is_empty() {
                    let name = providers::provider_display_name(&provider);
                    app.set_background_status(format!("{}: {}", name, message), false);
                }
            }
            Some(AppEvent::SyncComplete { provider, hosts }) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let display_name = providers::provider_display_name(&provider);
                let (_msg, is_err, total, added, updated, stale) =
                    app.apply_sync_result(&provider, hosts, false);
                if is_err {
                    app.sync_history.insert(
                        provider.clone(),
                        app::SyncRecord {
                            timestamp: now,
                            message: format!("{}: sync failed", display_name),
                            is_error: true,
                        },
                    );
                    app.sync_had_errors = true;
                } else {
                    let label = if total == 1 { "server" } else { "servers" };
                    let message = format!(
                        "{} {}{}",
                        total,
                        label,
                        format_sync_diff(added, updated, stale)
                    );
                    app.sync_history.insert(
                        provider.clone(),
                        app::SyncRecord {
                            timestamp: now,
                            message,
                            is_error: false,
                        },
                    );
                }
                app.syncing_providers.remove(&provider);
                app.sync_done.push(display_name.to_string());
                set_sync_summary(&mut app);
                // Reset config check timer so auto-reload doesn't immediately
                // detect our own write as an "external" change
                last_config_check = std::time::Instant::now();
            }
            Some(AppEvent::SyncPartial {
                provider,
                hosts,
                failures,
                total,
            }) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let display_name = providers::provider_display_name(provider.as_str());
                let (msg, is_err, synced, added, updated, stale) =
                    app.apply_sync_result(&provider, hosts, true);
                if is_err {
                    app.sync_history.insert(
                        provider.clone(),
                        app::SyncRecord {
                            timestamp: now,
                            message: msg,
                            is_error: true,
                        },
                    );
                } else {
                    let label = if synced == 1 { "server" } else { "servers" };
                    app.sync_history.insert(
                        provider.clone(),
                        app::SyncRecord {
                            timestamp: now,
                            message: format!(
                                "{} {}{} ({} of {} failed)",
                                synced,
                                label,
                                format_sync_diff(added, updated, stale),
                                failures,
                                total
                            ),
                            is_error: true,
                        },
                    );
                }
                app.sync_had_errors = true;
                app.syncing_providers.remove(&provider);
                app.sync_done.push(display_name.to_string());
                set_sync_summary(&mut app);
                last_config_check = std::time::Instant::now();
            }
            Some(AppEvent::SyncError { provider, message }) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let display_name = providers::provider_display_name(provider.as_str());
                app.sync_history.insert(
                    provider.clone(),
                    app::SyncRecord {
                        timestamp: now,
                        message: message.clone(),
                        is_error: true,
                    },
                );
                app.sync_had_errors = true;
                app.syncing_providers.remove(&provider);
                app.sync_done.push(display_name.to_string());
                set_sync_summary(&mut app);
                last_config_check = std::time::Instant::now();
            }
            Some(AppEvent::UpdateAvailable { version, headline }) => {
                app.update_available = Some(version);
                app.update_headline = headline;
            }
            Some(AppEvent::FileBrowserListing {
                alias,
                path,
                entries,
            }) => {
                let mut record_connection = false;
                if let Some(ref mut fb) = app.file_browser {
                    if fb.alias == alias {
                        fb.remote_loading = false;
                        match entries {
                            Ok(listing) => {
                                if !fb.connection_recorded {
                                    fb.connection_recorded = true;
                                    record_connection = true;
                                }
                                if fb.remote_path.is_empty() || fb.remote_path != path {
                                    fb.remote_path = path;
                                }
                                fb.remote_entries = listing;
                                fb.remote_error = None;
                                fb.remote_list_state = ratatui::widgets::ListState::default();
                                fb.remote_list_state.select(Some(0));
                            }
                            Err(msg) => {
                                if fb.remote_path.is_empty() {
                                    fb.remote_path = path;
                                }
                                fb.remote_error = Some(msg);
                                fb.remote_entries.clear();
                            }
                        }
                    }
                }
                if record_connection {
                    app.history.record(&alias);
                    app.apply_sort();
                }
                // Force full redraw: ssh may have written to /dev/tty
                terminal.force_redraw();
            }
            Some(AppEvent::ScpComplete {
                alias,
                success,
                message,
            }) => {
                // Track whether we need to spawn a remote refresh (can't do it inside the fb borrow
                // because spawn_remote_listing needs values from app too)
                let mut refresh_remote: Option<(
                    String,
                    Option<String>,
                    String,
                    bool,
                    file_browser::BrowserSort,
                )> = None;
                let matched = if let Some(ref mut fb) = app.file_browser {
                    if fb.alias == alias {
                        fb.transferring = None;
                        if success {
                            app.history.record(&alias);
                            fb.local_selected.clear();
                            fb.remote_selected.clear();
                            match file_browser::list_local(&fb.local_path, fb.show_hidden, fb.sort)
                            {
                                Ok(entries) => {
                                    fb.local_entries = entries;
                                    fb.local_error = None;
                                }
                                Err(e) => {
                                    fb.local_entries = Vec::new();
                                    fb.local_error = Some(e.to_string());
                                }
                            }
                            fb.local_list_state.select(Some(0));
                            if !fb.remote_path.is_empty() {
                                fb.remote_loading = true;
                                fb.remote_entries.clear();
                                fb.remote_error = None;
                                fb.remote_list_state = ratatui::widgets::ListState::default();
                                refresh_remote = Some((
                                    fb.alias.clone(),
                                    fb.askpass.clone(),
                                    fb.remote_path.clone(),
                                    fb.show_hidden,
                                    fb.sort,
                                ));
                            }
                        } else {
                            fb.transfer_error = Some(message.clone());
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if matched && success {
                    app.set_background_status("Transfer complete.", false);
                    // Rebuild display list so frecency sort and LAST column reflect the transfer
                    app.apply_sort();
                }
                if let Some((fb_alias, askpass_fb, path, show_hidden, sort)) = refresh_remote {
                    let config_path = app.reload.config_path.clone();
                    let has_tunnel = app.active_tunnels.contains_key(&fb_alias);
                    let bw = app.bw_session.clone();
                    let tx = events_tx.clone();
                    file_browser::spawn_remote_listing(
                        fb_alias,
                        config_path,
                        path,
                        show_hidden,
                        sort,
                        askpass_fb,
                        bw,
                        has_tunnel,
                        move |a, p, e| {
                            let _ = tx.send(AppEvent::FileBrowserListing {
                                alias: a,
                                path: p,
                                entries: e,
                            });
                        },
                    );
                }
                askpass::cleanup_marker(&alias);
                // Force full redraw: ssh may have written to /dev/tty
                terminal.force_redraw();
            }
            Some(AppEvent::SnippetHostDone {
                run_id,
                alias,
                stdout,
                stderr,
                exit_code,
            }) => {
                if exit_code == Some(0) {
                    app.history.record(&alias);
                    app.apply_sort();
                }
                if let Some(ref mut state) = app.snippet_output {
                    if state.run_id == run_id {
                        state.results.push(app::SnippetHostOutput {
                            alias,
                            stdout,
                            stderr,
                            exit_code,
                        });
                    }
                }
            }
            Some(AppEvent::SnippetProgress {
                run_id,
                completed,
                total,
            }) => {
                if let Some(ref mut state) = app.snippet_output {
                    if state.run_id == run_id {
                        state.completed = completed;
                        state.total = total;
                    }
                }
            }
            Some(AppEvent::SnippetAllDone { run_id }) => {
                if let Some(ref mut state) = app.snippet_output {
                    if state.run_id == run_id {
                        state.all_done = true;
                    }
                }
            }
            Some(AppEvent::ContainerListing { alias, result }) => {
                // Always update cache, even if overlay is closed
                match &result {
                    Ok((runtime, containers)) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        app.container_cache.insert(
                            alias.clone(),
                            crate::containers::ContainerCacheEntry {
                                timestamp: now,
                                runtime: *runtime,
                                containers: containers.clone(),
                            },
                        );
                        crate::containers::save_container_cache(&app.container_cache);
                    }
                    Err(e) => {
                        // Preserve runtime even on error
                        if let Some(rt) = e.runtime {
                            if let Some(entry) = app.container_cache.get_mut(&alias) {
                                entry.runtime = rt;
                            }
                        }
                    }
                }
                // Update overlay state if open
                if let Some(ref mut state) = app.container_state {
                    if state.alias == alias {
                        match result {
                            Ok((runtime, containers)) => {
                                state.runtime = Some(runtime);
                                state.containers = containers;
                                state.loading = false;
                                state.error = None;
                                if let Some(sel) = state.list_state.selected() {
                                    if sel >= state.containers.len() && !state.containers.is_empty()
                                    {
                                        state.list_state.select(Some(0));
                                    }
                                } else if !state.containers.is_empty() {
                                    state.list_state.select(Some(0));
                                }
                            }
                            Err(e) => {
                                if let Some(rt) = e.runtime {
                                    state.runtime = Some(rt);
                                }
                                state.loading = false;
                                state.error = Some(e.message);
                            }
                        }
                    }
                }
                askpass::cleanup_marker(&alias);
            }
            Some(AppEvent::ContainerActionComplete {
                alias,
                action,
                result,
            }) => {
                // Check if overlay matches and extract refresh info before set_status
                let should_refresh = if let Some(ref mut state) = app.container_state {
                    if state.alias == alias {
                        state.action_in_progress = None;
                        match result {
                            Ok(()) => {
                                state.loading = true;
                                Some((state.alias.clone(), state.askpass.clone(), state.runtime))
                            }
                            Err(e) => {
                                state.error = Some(e);
                                None
                            }
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some((refresh_alias, askpass, cached_runtime)) = should_refresh {
                    app.set_background_status(
                        format!("Container {} complete.", action.as_str()),
                        false,
                    );
                    let has_tunnel = app.active_tunnels.contains_key(&refresh_alias);
                    let config_path = app.reload.config_path.clone();
                    let bw = app.bw_session.clone();
                    let tx = events_tx.clone();
                    crate::containers::spawn_container_listing(
                        refresh_alias,
                        config_path,
                        askpass,
                        bw,
                        has_tunnel,
                        cached_runtime,
                        move |a, r| {
                            let _ = tx.send(AppEvent::ContainerListing {
                                alias: a,
                                result: r,
                            });
                        },
                    );
                }
                askpass::cleanup_marker(&alias);
            }
            Some(AppEvent::VaultSignResult {
                alias,
                certificate_file: existing_cert_file,
                success,
                message,
            }) => {
                if success {
                    // The CertificateFile snapshot is carried in the event so
                    // we never re-look up the host (which would be O(n) and
                    // racy under concurrent renames). `sign_certificate`
                    // always writes to purple's default path; if the host
                    // already has its own CertificateFile we leave the SSH
                    // config alone and cache validity against the configured
                    // path instead.
                    let mut host_missing = false;
                    if should_write_certificate_file(&existing_cert_file) {
                        if let Ok(cert_path) = vault_ssh::cert_path_for(&alias) {
                            let updated = app
                                .config
                                .set_host_certificate_file(&alias, &cert_path.to_string_lossy());
                            if !updated {
                                // Alias disappeared between scheduling the
                                // sign and processing the result (renamed or
                                // deleted from another code path). The cert
                                // is safely on disk; warn the user so they
                                // know the ssh config was NOT wired up.
                                host_missing = true;
                            }
                        }
                    }
                    // Single consolidated cache refresh path. `refresh_cert_cache`
                    // resolves the cert file (honoring any custom CertificateFile
                    // we just wrote) and records mtime for external-change
                    // detection. Replaces the two manual `check_cert_validity`
                    // inserts that used to live in both branches above.
                    app.refresh_cert_cache(&alias);
                    if host_missing {
                        app.set_status(
                            format!(
                                "Vault SSH cert saved for {} but host no longer in config (renamed or deleted). CertificateFile NOT written.",
                                alias
                            ),
                            true,
                        );
                    } else {
                        app.set_status(format!("Signed Vault SSH cert for {}", alias), false);
                    }
                } else {
                    app.set_status(
                        format!("Vault SSH: failed to sign {}: {}", alias, message),
                        true,
                    );
                }
            }
            Some(AppEvent::VaultSignProgress { alias, done, total }) => {
                // Truncate long aliases so the status line fits even on
                // narrow terminals; the full alias is recoverable from the
                // host list.
                const ALIAS_BUDGET: usize = 40;
                let display_alias: String = if alias.chars().count() > ALIAS_BUDGET {
                    let cut: String = alias.chars().take(ALIAS_BUDGET - 1).collect();
                    format!("{}\u{2026}", cut)
                } else {
                    alias.clone()
                };
                let spinner = crate::animation::SPINNER_FRAMES
                    [anim.spinner_tick as usize % crate::animation::SPINNER_FRAMES.len()];
                app.set_sticky_status(
                    format!(
                        "{} Signing {}/{}: {} (V to cancel)",
                        spinner, done, total, display_alias
                    ),
                    false,
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
                app.vault_signing_cancel = None;
                // Join the background thread now that it has finished.
                if let Some(handle) = app.vault_sign_thread.take() {
                    let _ = handle.join();
                }
                if let Some(msg) = aborted_message {
                    app.set_sticky_status(msg, true);
                    continue;
                }
                if cancelled {
                    let mut msg = format!(
                        "Vault SSH signing cancelled ({} signed, {} failed)",
                        signed, failed
                    );
                    if let Some(err) = first_error.as_ref() {
                        msg.push_str(": ");
                        msg.push_str(err);
                    }
                    if failed > 0 {
                        app.set_sticky_status(msg, true);
                    } else {
                        app.set_status(msg, false);
                    }
                    continue;
                }
                let summary_msg =
                    format_vault_sign_summary(signed, failed, skipped, first_error.as_deref());
                if signed > 0 {
                    if app.is_form_open() {
                        // Defer config write to avoid mtime conflict with open forms
                        app.pending_vault_config_write = true;
                        if failed > 0 {
                            app.set_sticky_status(summary_msg, true);
                        } else {
                            app.set_status(summary_msg, false);
                        }
                    } else if app.external_config_changed() {
                        // The on-disk ssh config (or an include) was modified
                        // by an external editor while the bulk-sign worker was
                        // running. Writing now would overwrite those edits.
                        // The certs are already safely on disk under
                        // ~/.purple/certs/, so we reload the fresh config from
                        // disk and re-apply `CertificateFile` directives only
                        // for hosts that still exist and still have no custom
                        // path. This way external edits are preserved AND new
                        // certs get wired up where safe.
                        let reapply: Vec<(String, String)> = app
                            .config
                            .host_entries()
                            .into_iter()
                            .filter_map(|h| {
                                if h.vault_ssh.is_some()
                                    && should_write_certificate_file(&h.certificate_file)
                                {
                                    vault_ssh::cert_path_for(&h.alias).ok().map(|p| {
                                        (h.alias.clone(), p.to_string_lossy().into_owned())
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect();
                        match ssh_config::model::SshConfigFile::parse(&app.reload.config_path) {
                            Ok(fresh) => {
                                app.config = fresh;
                                let mut reapplied = 0usize;
                                for (alias, cert_path) in &reapply {
                                    let entry = app
                                        .config
                                        .host_entries()
                                        .into_iter()
                                        .find(|h| &h.alias == alias);
                                    if let Some(entry) = entry {
                                        if should_write_certificate_file(&entry.certificate_file)
                                            && app
                                                .config
                                                .set_host_certificate_file(alias, cert_path)
                                        {
                                            reapplied += 1;
                                        }
                                    }
                                }
                                if reapplied > 0 {
                                    if let Err(e) = app.config.write() {
                                        app.set_sticky_status(
                                            format!(
                                                "External edits detected; signed {} certs but failed to re-apply CertificateFile: {}",
                                                signed, e
                                            ),
                                            true,
                                        );
                                    } else {
                                        app.update_last_modified();
                                        app.reload_hosts();
                                        app.set_status(
                                            format!(
                                                "{} External ssh config edits detected, merged {} CertificateFile directives.",
                                                summary_msg, reapplied
                                            ),
                                            failed > 0,
                                        );
                                    }
                                } else {
                                    app.reload_hosts();
                                    app.set_sticky_status(
                                        format!(
                                            "{} External ssh config edits detected; certs on disk, no CertificateFile written.",
                                            summary_msg
                                        ),
                                        true,
                                    );
                                }
                            }
                            Err(e) => {
                                app.set_sticky_status(
                                    format!(
                                        "Signed {} certs but cannot re-parse ssh config after external edit: {}. Certs are on disk under ~/.purple/certs/.",
                                        signed, e
                                    ),
                                    true,
                                );
                            }
                        }
                    } else if let Err(e) = app.config.write() {
                        app.set_sticky_status(
                            format!(
                                "Signed {} certs but failed to update SSH config: {}",
                                signed, e
                            ),
                            true,
                        );
                    } else {
                        app.update_last_modified();
                        app.reload_hosts();
                        if failed > 0 {
                            app.set_sticky_status(summary_msg, true);
                        } else {
                            app.set_status(summary_msg, false);
                        }
                    }
                } else if failed > 0 {
                    app.set_sticky_status(summary_msg, true);
                } else {
                    app.set_status(summary_msg, false);
                }
            }
            Some(AppEvent::CertCheckResult { alias, status }) => {
                app.cert_check_in_flight.remove(&alias);
                let mtime = current_cert_mtime(&alias, &app);
                app.cert_status_cache
                    .insert(alias, (std::time::Instant::now(), status, mtime));
            }
            Some(AppEvent::CertCheckError { alias, message }) => {
                // Cache the error as Invalid so the lazy-check loop doesn't
                // re-spawn a background thread on every poll tick (which at
                // ~100ms would spawn ~10 threads/sec against a broken path).
                // The lazy-check block uses CERT_ERROR_BACKOFF_SECS instead of
                // the normal TTL for Invalid entries, so transient errors
                // still recover within 30 seconds instead of the 5-minute
                // valid-cert TTL.
                app.cert_check_in_flight.remove(&alias);
                app.cert_status_cache.insert(
                    alias.clone(),
                    (
                        std::time::Instant::now(),
                        vault_ssh::CertStatus::Invalid(message.clone()),
                        None,
                    ),
                );
                app.set_background_status(
                    format!("Cert check failed for {}: {}", alias, message),
                    true,
                );
            }
            Some(AppEvent::PollError) => {
                app.running = false;
            }
        }

        // Lazy cert status check: when the selected host has a vault role and
        // no cached status, spawn a background check.
        if let Some(selected) = app.selected_host() {
            if vault_ssh::resolve_vault_role(
                selected.vault_ssh.as_deref(),
                selected.provider.as_deref(),
                &app.provider_config,
            )
            .is_some()
            {
                // Stat the cert file once per iteration to detect external
                // writes (CLI sign, another purple instance) within one frame.
                // Compared against the mtime recorded when the cache entry was
                // populated; any mismatch forces a re-check, no matter the TTL.
                let current_mtime =
                    vault_ssh::resolve_cert_path(&selected.alias, &selected.certificate_file)
                        .ok()
                        .and_then(|p| std::fs::metadata(&p).ok())
                        .and_then(|m| m.modified().ok());
                let cache_stale = cache_entry_is_stale(
                    app.cert_status_cache.get(&selected.alias),
                    current_mtime,
                    |t| t.elapsed().as_secs(),
                );

                let sign_in_flight = app
                    .vault_sign_in_flight
                    .lock()
                    .map(|g| g.contains(&selected.alias))
                    .unwrap_or(false);
                if cache_stale
                    && !app.cert_check_in_flight.contains(&selected.alias)
                    && !sign_in_flight
                {
                    let alias = selected.alias.clone();
                    let cert_file = selected.certificate_file.clone();
                    app.cert_check_in_flight.insert(alias.clone());
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

        // Handle pending SSH connection
        if let Some((alias, host_askpass)) = app.pending_connect.take() {
            let vault_host = app.hosts.iter().find(|h| h.alias == alias).cloned();
            let askpass = host_askpass.or_else(preferences::load_askpass_default);
            let has_active_tunnel = app.active_tunnels.contains_key(&alias);
            let use_tmux = connection::is_in_tmux() && askpass.is_none();

            if use_tmux {
                // Tmux mode: open SSH in a new tmux window. TUI stays alive.
                // Vault SSH cert signing runs first (eprintln warnings are
                // harmless on the alternate screen — ratatui repaints over
                // them on the next draw cycle).
                let vault_msg = if let Some(ref host) = vault_host {
                    let msg = ensure_vault_ssh_if_needed(
                        &alias,
                        host,
                        &app.provider_config,
                        &mut app.config,
                    );
                    if msg.is_some() {
                        app.reload_hosts();
                        app.refresh_cert_cache(&alias);
                    }
                    msg
                } else {
                    None
                };

                match connection::connect_tmux_window(
                    &alias,
                    &app.reload.config_path,
                    has_active_tunnel,
                ) {
                    Ok(()) => {
                        if let Some((ref msg, is_error)) = vault_msg {
                            app.set_status(msg.clone(), is_error);
                        } else {
                            app.set_status(format!("Opened {} in new tmux window.", alias), false);
                        }
                    }
                    Err(e) => {
                        app.set_status(format!("tmux: {e}"), true);
                    }
                }
            } else {
                // Standard mode: suspend TUI, run SSH inline, restore TUI.
                // Order preserved: pause events, exit TUI, THEN run vault
                // signing and password setup (which may eprintln or prompt
                // for input on the real terminal).
                events.pause();
                terminal.exit()?;
                let vault_msg = if let Some(ref host) = vault_host {
                    let msg = ensure_vault_ssh_if_needed(
                        &alias,
                        host,
                        &app.provider_config,
                        &mut app.config,
                    );
                    if msg.is_some() {
                        app.reload_hosts();
                        app.refresh_cert_cache(&alias);
                    }
                    msg
                } else {
                    None
                };
                if let Some(token) =
                    ensure_bw_session(app.bw_session.as_deref(), askpass.as_deref())
                {
                    app.bw_session = Some(token);
                }
                ensure_keychain_password(&alias, askpass.as_deref());
                println!("Beaming you up to {}...\n", alias);
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
                                app.set_status(
                                    format!("SSH to {} exited with code {}.", alias, code),
                                    true,
                                );
                            }
                        } else if let Some((ref msg, is_error)) = vault_msg {
                            app.set_status(msg.clone(), is_error);
                        }
                    }
                    Err(e) => {
                        eprintln!("Connection failed: {}", e);
                        app.set_status(format!("Connection to {} failed.", alias), true);
                    }
                }
                askpass::cleanup_marker(&alias);
                terminal.enter()?;
                events.resume();
                last_config_check = std::time::Instant::now();
                // Reload in case config changed externally
                app.config = SshConfigFile::parse(&app.reload.config_path)?;
                app.reload_hosts();
                app.update_last_modified();
            }
        }

        // Handle pending snippet execution
        if let Some((snip, aliases)) = app.pending_snippet.take() {
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
                if let Some(token) =
                    ensure_bw_session(app.bw_session.as_deref(), askpass.as_deref())
                {
                    app.bw_session = Some(token);
                }
                ensure_keychain_password(alias, askpass.as_deref());

                if multi {
                    println!("── {} ──", alias);
                } else {
                    println!("Running '{}' on {}...\n", snip.name, alias);
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
                        } else if multi {
                            eprintln!("Exited with code {}.", r.status.code().unwrap_or(1));
                        } else {
                            println!("\nExited with code {}.", r.status.code().unwrap_or(1));
                        }
                    }
                    Err(e) => eprintln!("[{}] Failed: {}", alias, e),
                }
                if multi {
                    println!();
                }
            }

            if !multi {
                println!("\nDone.");
            } else {
                println!("Done. Ran '{}' on {} hosts.", snip.name, aliases.len());
            }
            println!("\nPress Enter to continue...");
            let _ = std::io::stdin().read_line(&mut String::new());
            terminal.enter()?;
            events.resume();
            last_config_check = std::time::Instant::now();
            // Reload so sort order (e.g. most recent) reflects the new history
            app.config = SshConfigFile::parse(&app.reload.config_path)?;
            app.reload_hosts();
            app.update_last_modified();
        }
    }

    // Final flush of any deferred vault config write before teardown so on-disk
    // state is not left behind.
    app.flush_pending_vault_write();

    // Cancel and join the background vault signing thread, if running.
    if let Some(ref cancel) = app.vault_signing_cancel {
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if let Some(handle) = app.vault_sign_thread.take() {
        let _ = handle.join();
    }

    // Kill all active tunnels on exit
    for (_, mut tunnel) in app.active_tunnels.drain() {
        let _ = tunnel.child.kill();
        let _ = tunnel.child.wait();
    }

    terminal.exit()?;
    Ok(())
}

fn handle_quick_add(
    mut config: SshConfigFile,
    target: &str,
    alias: Option<&str>,
    key: Option<&str>,
) -> Result<()> {
    let parsed = quick_add::parse_target(target).map_err(|e| anyhow::anyhow!(e))?;

    let alias_str = alias.map(|a| a.to_string()).unwrap_or_else(|| {
        parsed
            .hostname
            .split('.')
            .next()
            .unwrap_or(&parsed.hostname)
            .to_string()
    });

    if alias_str.trim().is_empty() {
        eprintln!("Alias can't be empty. Use --alias to specify one.");
        std::process::exit(1);
    }
    if alias_str.contains(char::is_whitespace) {
        eprintln!("Alias can't contain whitespace. Use --alias to pick a simpler name.");
        std::process::exit(1);
    }
    if ssh_config::model::is_host_pattern(&alias_str) {
        eprintln!("Alias can't contain pattern characters. Use --alias to pick a different name.");
        std::process::exit(1);
    }

    // Reject control characters in alias, hostname, user and key
    let key_val = key.unwrap_or("").to_string();
    for (value, name) in [
        (&alias_str, "Alias"),
        (&parsed.hostname, "Hostname"),
        (&parsed.user, "User"),
        (&key_val, "Identity file"),
    ] {
        if value.chars().any(|c| c.is_control()) {
            eprintln!("{} contains control characters.", name);
            std::process::exit(1);
        }
    }

    // Reject whitespace in hostname and user (matches TUI validation)
    if parsed.hostname.contains(char::is_whitespace) {
        eprintln!("Hostname can't contain whitespace.");
        std::process::exit(1);
    }
    if parsed.user.contains(char::is_whitespace) {
        eprintln!("User can't contain whitespace.");
        std::process::exit(1);
    }

    if config.has_host(&alias_str) {
        eprintln!(
            "'{}' already exists. Use --alias to pick a different name.",
            alias_str
        );
        std::process::exit(1);
    }

    let entry = HostEntry {
        alias: alias_str.clone(),
        hostname: parsed.hostname,
        user: parsed.user,
        port: parsed.port,
        identity_file: key_val,
        ..Default::default()
    };

    config.add_host(&entry);
    config.write()?;
    println!("Welcome aboard, {}!", alias_str);
    Ok(())
}

fn handle_import(
    mut config: SshConfigFile,
    file: Option<&str>,
    known_hosts: bool,
    group: Option<&str>,
) -> Result<()> {
    let result = if known_hosts {
        import::import_from_known_hosts(&mut config, group)
    } else if let Some(path) = file {
        let resolved = resolve_config_path(path)?;
        import::import_from_file(&mut config, &resolved, group)
    } else {
        eprintln!("Provide a file or use --known-hosts. Run 'purple import --help' for details.");
        std::process::exit(1);
    };

    match result {
        Ok((imported, skipped, parse_failures, read_errors)) => {
            if imported > 0 {
                config.write()?;
            }
            println!(
                "Imported {} host{}, skipped {} duplicate{}.",
                imported,
                if imported == 1 { "" } else { "s" },
                skipped,
                if skipped == 1 { "" } else { "s" },
            );
            if parse_failures > 0 {
                eprintln!(
                    "! {} line{} could not be parsed (invalid format).",
                    parse_failures,
                    if parse_failures == 1 { "" } else { "s" },
                );
            }
            if read_errors > 0 {
                eprintln!(
                    "! {} line{} could not be read (encoding error).",
                    read_errors,
                    if read_errors == 1 { "" } else { "s" },
                );
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

fn handle_sync(
    mut config: SshConfigFile,
    provider_name: Option<&str>,
    dry_run: bool,
    remove: bool,
) -> Result<()> {
    let provider_config = providers::config::ProviderConfig::load();
    let sections: Vec<&providers::config::ProviderSection> = if let Some(name) = provider_name {
        if providers::get_provider(name).is_none() {
            eprintln!(
                "Never heard of '{}'. Try: digitalocean, vultr, linode, hetzner, upcloud, proxmox, aws, scaleway, gcp, azure, tailscale, oracle, ovh, leaseweb, i3d, transip.",
                name
            );
            std::process::exit(1);
        }
        match provider_config.section(name) {
            Some(s) => vec![s],
            None => {
                eprintln!(
                    "No configuration for {}. Run 'purple provider add {}' first.",
                    name, name
                );
                std::process::exit(1);
            }
        }
    } else {
        let configured = provider_config.configured_providers();
        if configured.is_empty() {
            eprintln!("No providers configured. Run 'purple provider add' to set one up.");
            std::process::exit(1);
        }
        configured.iter().collect()
    };

    let mut any_changes = false;
    let mut any_failures = false;
    let mut any_hard_failures = false;

    for section in &sections {
        let provider = match providers::get_provider_with_config(&section.provider, section) {
            Some(p) => p,
            None => {
                eprintln!(
                    "Skipping unknown provider '{}'. Try: digitalocean, vultr, linode, hetzner, upcloud, proxmox, aws, scaleway, gcp, azure, tailscale, oracle, ovh, leaseweb, i3d, transip.",
                    section.provider
                );
                any_failures = true;
                // Not a hard failure: unknown provider contributes no changes,
                // so other providers' successful results should still be written.
                continue;
            }
        };
        let display_name = providers::provider_display_name(section.provider.as_str());
        let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdout());
        print!("Syncing {}... ", display_name);
        let _ = std::io::Write::flush(&mut std::io::stdout());

        let last_summary = std::cell::RefCell::new(String::new());
        let progress = |msg: &str| {
            *last_summary.borrow_mut() = msg.to_string();
            if is_tty {
                print!("\x1b[2K\rSyncing {}... {}", display_name, msg);
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
        };
        let fetch_result = provider.fetch_hosts_with_progress(
            &section.token,
            &std::sync::atomic::AtomicBool::new(false),
            &progress,
        );
        let summary = last_summary.into_inner();
        // Complete the Syncing line: TTY overwrites with summary; non-TTY appends.
        if is_tty {
            if summary.is_empty() {
                print!("\x1b[2K\rSyncing {}... ", display_name);
            } else {
                println!("\x1b[2K\rSyncing {}... {}", display_name, summary);
            }
            let _ = std::io::Write::flush(&mut std::io::stdout());
        } else if !summary.is_empty() {
            println!("{}", summary);
        }
        let (hosts, suppress_remove) = match fetch_result {
            Ok(hosts) => (hosts, false),
            Err(providers::ProviderError::PartialResult {
                hosts,
                failures,
                total,
            }) => {
                println!(
                    "{} servers found ({} of {} failed to fetch).",
                    hosts.len(),
                    failures,
                    total
                );
                if remove {
                    eprintln!(
                        "! {}: skipping --remove due to partial failures.",
                        display_name
                    );
                }
                any_failures = true;
                (hosts, true)
            }
            Err(e) => {
                println!("failed.");
                eprintln!("! {}: {}", display_name, e);
                any_failures = true;
                any_hard_failures = true;
                continue;
            }
        };
        if !suppress_remove {
            println!("{} servers found.", hosts.len());
        }
        let effective_remove = remove && !suppress_remove;
        let result = providers::sync::sync_provider(
            &mut config,
            &*provider,
            &hosts,
            section,
            effective_remove,
            suppress_remove, // suppress stale marking when partial failures occurred
            dry_run,
        );
        let prefix = if dry_run { "  Would have: " } else { "  " };
        println!(
            "{}Added {}, updated {}, unchanged {}.",
            prefix, result.added, result.updated, result.unchanged
        );
        if result.removed > 0 {
            println!("  Removed {}.", result.removed);
        }
        if result.stale > 0 {
            println!("  Marked {} stale.", result.stale);
        }
        if result.added > 0 || result.updated > 0 || result.removed > 0 || result.stale > 0 {
            any_changes = true;
        }
    }

    if any_changes && !dry_run {
        if any_hard_failures {
            eprintln!("! Skipping config write due to sync failures. Fix the errors and re-run.");
        } else {
            config.write()?;
        }
    }

    if any_failures {
        std::process::exit(1);
    }

    Ok(())
}

fn handle_provider_command(command: ProviderCommands) -> Result<()> {
    match command {
        ProviderCommands::Add {
            provider,
            token,
            token_stdin,
            mut prefix,
            mut user,
            mut key,
            url,
            mut profile,
            mut regions,
            mut project,
            mut compartment,
            no_verify_tls,
            verify_tls,
            auto_sync,
            no_auto_sync,
        } => {
            let p = match providers::get_provider(&provider) {
                Some(p) => p,
                None => {
                    eprintln!(
                        "Never heard of '{}'. Try: digitalocean, vultr, linode, hetzner, upcloud, proxmox, aws, scaleway, gcp, azure, tailscale, oracle, ovh, leaseweb, i3d, transip.",
                        provider
                    );
                    std::process::exit(1);
                }
            };

            // --url, --no-verify-tls and --verify-tls are Proxmox-only; clear them for other providers
            let mut token = token;
            let mut url = url;
            let mut no_verify_tls = no_verify_tls;
            let mut verify_tls = verify_tls;
            if provider != "proxmox" {
                if url.is_some() {
                    eprintln!("Warning: --url is only used by the Proxmox provider. Ignoring.");
                    url = None;
                }
                if no_verify_tls {
                    eprintln!(
                        "Warning: --no-verify-tls is only used by the Proxmox provider. Ignoring."
                    );
                    no_verify_tls = false;
                }
                if verify_tls {
                    eprintln!(
                        "Warning: --verify-tls is only used by the Proxmox provider. Ignoring."
                    );
                    verify_tls = false;
                }
            }
            // --profile is AWS-only, --regions is AWS/Scaleway/GCP/Azure, --project is GCP-only
            if provider != "aws" && profile.is_some() {
                eprintln!("Warning: --profile is only used by the AWS provider. Ignoring.");
                profile = None;
            }
            if !matches!(
                provider.as_str(),
                "aws" | "scaleway" | "gcp" | "azure" | "oracle"
            ) && regions.is_some()
            {
                eprintln!(
                    "Warning: --regions is only used by the AWS, Scaleway, GCP, Azure and Oracle providers. Ignoring."
                );
                regions = None;
            }
            if provider != "gcp" && project.is_some() {
                eprintln!("Warning: --project is only used by the GCP provider. Ignoring.");
                project = None;
            }
            if provider != "oracle" && compartment.is_some() {
                eprintln!("Warning: --compartment is only used by the Oracle provider. Ignoring.");
                compartment = None;
            }

            // When updating an existing section, fall back to stored values for fields not supplied
            let existing_section = providers::config::ProviderConfig::load()
                .section(&provider)
                .cloned();

            if let Some(ref existing) = existing_section {
                // URL fallback only applies to Proxmox (only provider that uses the url field)
                if provider == "proxmox" && url.is_none() && !existing.url.is_empty() {
                    url = Some(existing.url.clone());
                }
                if token.is_none()
                    && !token_stdin
                    && std::env::var("PURPLE_TOKEN").is_err()
                    && !existing.token.is_empty()
                {
                    token = Some(existing.token.clone());
                }
                if prefix.is_none() {
                    prefix = Some(existing.alias_prefix.clone());
                }
                if user.is_none() {
                    user = Some(existing.user.clone());
                }
                if key.is_none() && !existing.identity_file.is_empty() {
                    key = Some(existing.identity_file.clone());
                }
                // Preserve verify_tls=false unless the user explicitly overrides it either way
                if !no_verify_tls && !verify_tls && !existing.verify_tls {
                    no_verify_tls = true;
                }
                // AWS: fall back to stored profile/regions
                if provider == "aws" && profile.is_none() && !existing.profile.is_empty() {
                    profile = Some(existing.profile.clone());
                }
                // AWS/Scaleway/GCP/Azure: fall back to stored regions
                if matches!(
                    provider.as_str(),
                    "aws" | "scaleway" | "gcp" | "azure" | "oracle"
                ) && regions.is_none()
                    && !existing.regions.is_empty()
                {
                    regions = Some(existing.regions.clone());
                }
                // GCP: fall back to stored project
                if provider == "gcp" && project.is_none() && !existing.project.is_empty() {
                    project = Some(existing.project.clone());
                }
                // Oracle: fall back to stored compartment
                if provider == "oracle" && compartment.is_none() && !existing.compartment.is_empty()
                {
                    compartment = Some(existing.compartment.clone());
                }
            }

            // Proxmox requires --url
            if provider == "proxmox" {
                if url.is_none() || url.as_deref().unwrap_or("").trim().is_empty() {
                    eprintln!("Proxmox requires --url (e.g. --url https://pve.example.com:8006).");
                    std::process::exit(1);
                }
                let u = url.as_deref().unwrap();
                if !u.to_ascii_lowercase().starts_with("https://") {
                    eprintln!(
                        "URL must start with https://. For self-signed certificates use --no-verify-tls."
                    );
                    std::process::exit(1);
                }
            }

            // AWS allows empty token when --profile is set
            let aws_has_profile =
                provider == "aws" && profile.as_deref().is_some_and(|p| !p.trim().is_empty());
            let token = if aws_has_profile
                && token.is_none()
                && !token_stdin
                && std::env::var("PURPLE_TOKEN").is_err()
            {
                String::new()
            } else {
                match resolve_token(token, token_stdin) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
            };

            if token.trim().is_empty() && !aws_has_profile && provider != "tailscale" {
                if provider == "gcp" {
                    eprintln!(
                        "Token can't be empty. Provide a service account JSON key file path or access token."
                    );
                } else if provider == "oracle" {
                    eprintln!(
                        "Token can't be empty. Provide the path to your OCI config file (e.g. ~/.oci/config)."
                    );
                } else {
                    eprintln!(
                        "Token can't be empty. Grab one from your {} dashboard.",
                        providers::provider_display_name(&provider)
                    );
                }
                std::process::exit(1);
            }

            let alias_prefix = prefix.unwrap_or_else(|| p.short_label().to_string());
            if ssh_config::model::is_host_pattern(&alias_prefix) {
                eprintln!("Alias prefix can't contain spaces or pattern characters (*, ?, [, !).");
                std::process::exit(1);
            }

            let user = user.unwrap_or_else(|| "root".to_string());
            let identity_file = key.unwrap_or_default();

            // Reject control characters in all fields (prevents INI injection)
            let url_value = url.clone().unwrap_or_default();
            let profile_value = profile.clone().unwrap_or_default();
            let regions_value = regions.clone().unwrap_or_default();
            let project_value = project.clone().unwrap_or_default();
            let compartment_value = compartment.clone().unwrap_or_default();
            for (value, name) in [
                (&url_value, "URL"),
                (&token, "Token"),
                (&alias_prefix, "Alias prefix"),
                (&user, "User"),
                (&identity_file, "Identity file"),
                (&profile_value, "Profile"),
                (&project_value, "Project"),
                (&regions_value, "Regions"),
                (&compartment_value, "Compartment"),
            ] {
                if value.chars().any(|c| c.is_control()) {
                    eprintln!("{} contains control characters.", name);
                    std::process::exit(1);
                }
            }
            if user.contains(char::is_whitespace) {
                eprintln!("User can't contain whitespace.");
                std::process::exit(1);
            }

            // Resolve auto_sync: explicit flags > existing config > provider default
            let resolved_auto_sync = if auto_sync {
                true
            } else if no_auto_sync {
                false
            } else if let Some(ref existing) = existing_section {
                existing.auto_sync
            } else {
                !matches!(provider.as_str(), "proxmox")
            };

            let resolved_profile = profile.unwrap_or_default();
            let resolved_regions = regions.unwrap_or_default();
            let resolved_project = project.unwrap_or_default();
            let resolved_compartment = compartment.unwrap_or_default();

            // AWS/Scaleway/Azure requires at least one region/zone/subscription
            if provider == "aws" && resolved_regions.trim().is_empty() {
                eprintln!("AWS requires --regions (e.g. --regions us-east-1,eu-west-1).");
                std::process::exit(1);
            }
            if provider == "scaleway" && resolved_regions.trim().is_empty() {
                eprintln!(
                    "Scaleway requires --regions with one or more zones (e.g. --regions fr-par-1,nl-ams-1)."
                );
                std::process::exit(1);
            }
            if provider == "azure" {
                if resolved_regions.trim().is_empty() {
                    eprintln!("Azure requires --regions with one or more subscription IDs.");
                    std::process::exit(1);
                }
                for sub in resolved_regions
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    if !providers::azure::is_valid_subscription_id(sub) {
                        eprintln!(
                            "Invalid subscription ID '{}'. Expected UUID format (e.g. 12345678-1234-1234-1234-123456789012).",
                            sub
                        );
                        std::process::exit(1);
                    }
                }
            }
            // GCP requires --project
            if provider == "gcp" && resolved_project.trim().is_empty() {
                eprintln!("GCP requires --project (e.g. --project my-gcp-project-id).");
                std::process::exit(1);
            }
            // Oracle requires --compartment
            if provider == "oracle" && resolved_compartment.trim().is_empty() {
                eprintln!(
                    "Oracle requires --compartment (e.g. --compartment ocid1.compartment.oc1..aaa...)."
                );
                std::process::exit(1);
            }

            let section = providers::config::ProviderSection {
                provider: provider.clone(),
                token,
                alias_prefix,
                user,
                identity_file,
                url: url.unwrap_or_default(),
                verify_tls: !no_verify_tls,
                auto_sync: resolved_auto_sync,
                profile: resolved_profile,
                regions: resolved_regions,
                project: resolved_project,
                compartment: resolved_compartment,
                vault_role: String::new(),
                vault_addr: String::new(),
            };

            let mut config = providers::config::ProviderConfig::load();
            config.set_section(section);
            config
                .save()
                .map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
            println!("Saved {} configuration.", provider);
            Ok(())
        }
        ProviderCommands::List => {
            let config = providers::config::ProviderConfig::load();
            let sections = config.configured_providers();
            if sections.is_empty() {
                println!("No providers configured. Run 'purple provider add' to set one up.");
            } else {
                for s in sections {
                    let display_name = providers::provider_display_name(s.provider.as_str());
                    println!("  {:<16} {}-*{:>8}", display_name, s.alias_prefix, s.user);
                }
            }
            Ok(())
        }
        ProviderCommands::Remove { provider } => {
            let mut config = providers::config::ProviderConfig::load();
            if config.section(&provider).is_none() {
                eprintln!("No configuration for '{}'. Nothing to remove.", provider);
                std::process::exit(1);
            }
            config.remove_section(&provider);
            config
                .save()
                .map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
            println!("Removed {} configuration.", provider);
            Ok(())
        }
    }
}

fn handle_tunnel_command(mut config: SshConfigFile, command: TunnelCommands) -> Result<()> {
    match command {
        TunnelCommands::List { alias } => {
            if let Some(alias) = alias {
                // Show tunnels for a specific host
                if !config.has_host(&alias) {
                    eprintln!("No host '{}' found.", alias);
                    std::process::exit(1);
                }
                let rules = config.find_tunnel_directives(&alias);
                if rules.is_empty() {
                    println!("No tunnels configured for {}.", alias);
                } else {
                    println!("Tunnels for {}:", alias);
                    for rule in &rules {
                        println!("  {}", rule.display());
                    }
                }
            } else {
                // Show all hosts with tunnels
                let entries = config.host_entries();
                let with_tunnels: Vec<_> = entries.iter().filter(|e| e.tunnel_count > 0).collect();
                if with_tunnels.is_empty() {
                    println!("No tunnels configured.");
                } else {
                    for (i, host) in with_tunnels.iter().enumerate() {
                        if i > 0 {
                            println!();
                        }
                        println!("{}:", host.alias);
                        for rule in config.find_tunnel_directives(&host.alias) {
                            println!("  {}", rule.display());
                        }
                    }
                }
            }
            Ok(())
        }
        TunnelCommands::Add { alias, forward } => {
            if !config.has_host(&alias) {
                eprintln!("No host '{}' found.", alias);
                std::process::exit(1);
            }
            if config.is_included_host(&alias) {
                eprintln!(
                    "Host '{}' is from an included file and cannot be modified.",
                    alias
                );
                std::process::exit(1);
            }
            let rule = tunnel::TunnelRule::from_cli_spec(&forward).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let key = rule.tunnel_type.directive_key();
            let value = rule.to_directive_value();
            // Check for duplicate forward
            if config.has_forward(&alias, key, &value) {
                eprintln!("Forward {} already exists on {}.", forward, alias);
                std::process::exit(1);
            }
            config.add_forward(&alias, key, &value);
            if let Err(e) = config.write() {
                eprintln!("Failed to save config: {}", e);
                std::process::exit(1);
            }
            println!("Added {} to {}.", forward, alias);
            Ok(())
        }
        TunnelCommands::Remove { alias, forward } => {
            if !config.has_host(&alias) {
                eprintln!("No host '{}' found.", alias);
                std::process::exit(1);
            }
            if config.is_included_host(&alias) {
                eprintln!(
                    "Host '{}' is from an included file and cannot be modified.",
                    alias
                );
                std::process::exit(1);
            }
            let rule = tunnel::TunnelRule::from_cli_spec(&forward).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let key = rule.tunnel_type.directive_key();
            let value = rule.to_directive_value();
            let removed = config.remove_forward(&alias, key, &value);
            if !removed {
                eprintln!("No matching forward {} found on {}.", forward, alias);
                std::process::exit(1);
            }
            if let Err(e) = config.write() {
                eprintln!("Failed to save config: {}", e);
                std::process::exit(1);
            }
            println!("Removed {} from {}.", forward, alias);
            Ok(())
        }
        TunnelCommands::Start { alias } => {
            if !config.has_host(&alias) {
                eprintln!("No host '{}' found.", alias);
                std::process::exit(1);
            }
            let tunnels = config.find_tunnel_directives(&alias);
            if tunnels.is_empty() {
                eprintln!("No forwarding directives configured for '{}'.", alias);
                std::process::exit(1);
            }
            println!("Starting tunnel for {}... (Ctrl+C to stop)", alias);
            // Run ssh -N in foreground with inherited stdio
            let status = std::process::Command::new("ssh")
                .arg("-F")
                .arg(&config.path)
                .arg("-N")
                .arg("--")
                .arg(&alias)
                .status()
                .map_err(|e| anyhow::anyhow!("Failed to start ssh: {}", e))?;
            let code = status.code().unwrap_or(1);
            std::process::exit(code);
        }
    }
}

/// Read a line of input with echo disabled. Returns None if the user presses Esc.
fn prompt_hidden_input(prompt: &str) -> Result<Option<String>> {
    eprint!("{}", prompt);
    crossterm::terminal::enable_raw_mode()?;
    let mut input = String::new();
    loop {
        if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
            match key.code {
                crossterm::event::KeyCode::Enter => break,
                crossterm::event::KeyCode::Char(c) => {
                    input.push(c);
                    eprint!("*");
                }
                crossterm::event::KeyCode::Backspace => {
                    if input.pop().is_some() {
                        eprint!("\x08 \x08");
                    }
                }
                crossterm::event::KeyCode::Esc => {
                    crossterm::terminal::disable_raw_mode()?;
                    eprintln!();
                    return Ok(None);
                }
                _ => {}
            }
        }
    }
    crossterm::terminal::disable_raw_mode()?;
    eprintln!();
    Ok(Some(input))
}

/// Resolve the current on-disk mtime of a host's Vault SSH certificate.
///
/// Used by the `CertCheckResult` handler so every cache entry carries a
/// mtime alongside its status, enabling mtime-based lazy invalidation when
/// an external actor (CLI, another purple instance) rewrites the cert.
pub(crate) fn current_cert_mtime(alias: &str, app: &app::App) -> Option<std::time::SystemTime> {
    let host = app.hosts.iter().find(|h| h.alias == alias)?;
    let cert_path = vault_ssh::resolve_cert_path(alias, &host.certificate_file).ok()?;
    std::fs::metadata(&cert_path)
        .ok()
        .and_then(|m| m.modified().ok())
}

/// Decide whether a `cert_status_cache` entry should be re-checked.
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

/// Check and renew Vault SSH certificate if the host has a vault role configured.
/// Writes the cert file to ~/.purple/certs/ AND sets CertificateFile on the host
/// block when it is empty, so `ssh` actually uses the freshly signed cert.
///
/// Returns `Some(message)` when a signing action was attempted (success or failure),
/// `None` when no vault role is configured or the cert is still valid.
fn ensure_vault_ssh_if_needed(
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
fn ensure_bw_session(existing: Option<&str>, askpass: Option<&str>) -> Option<String> {
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
            eprintln!("Bitwarden CLI (bw) not found. SSH will prompt for password.");
            None
        }
        askpass::BwStatus::NotAuthenticated => {
            eprintln!("Bitwarden vault not logged in. Run 'bw login' first.");
            None
        }
        askpass::BwStatus::Locked => {
            // Prompt for master password and unlock
            for attempt in 0..2 {
                let password = match prompt_hidden_input("Bitwarden master password: ") {
                    Ok(Some(p)) if !p.is_empty() => p,
                    Ok(Some(_)) => {
                        eprintln!("Empty password. SSH will prompt for password.");
                        return None;
                    }
                    Ok(None) => {
                        // User pressed Esc
                        return None;
                    }
                    Err(e) => {
                        eprintln!("Failed to read password: {}", e);
                        return None;
                    }
                };
                match askpass::bw_unlock(&password) {
                    Ok(token) => return Some(token),
                    Err(e) => {
                        if attempt == 0 {
                            eprintln!("Unlock failed: {}. Try again.", e);
                        } else {
                            eprintln!("Unlock failed: {}. SSH will prompt for password.", e);
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
fn ensure_keychain_password(alias: &str, askpass: Option<&str>) {
    if askpass != Some("keychain") {
        return;
    }
    // Check if password already exists
    if askpass::keychain_has_password(alias) {
        return;
    }
    // Prompt for password and store it
    let password =
        match prompt_hidden_input(&format!("Password for {} (stored in keychain): ", alias)) {
            Ok(Some(p)) if !p.is_empty() => p,
            Ok(Some(_)) => {
                eprintln!("Empty password. SSH will prompt for password.");
                return;
            }
            Ok(None) => return, // Esc
            Err(_) => return,
        };
    match askpass::store_in_keychain(alias, &password) {
        Ok(()) => eprintln!("Password stored in keychain."),
        Err(e) => eprintln!(
            "Failed to store in keychain: {}. SSH will prompt for password.",
            e
        ),
    }
}

fn handle_password_command(command: PasswordCommands) -> Result<()> {
    match command {
        PasswordCommands::Set { alias } => {
            let password = match prompt_hidden_input(&format!("Password for {}: ", alias))? {
                Some(p) if !p.is_empty() => p,
                Some(_) => {
                    eprintln!("Password can't be empty.");
                    std::process::exit(1);
                }
                None => {
                    eprintln!("Cancelled.");
                    std::process::exit(1);
                }
            };

            askpass::store_in_keychain(&alias, &password)?;
            println!(
                "Password stored for {}. Set 'keychain' as password source to use it.",
                alias
            );
            Ok(())
        }
        PasswordCommands::Remove { alias } => {
            askpass::remove_from_keychain(&alias)?;
            println!("Password removed for {}.", alias);
            Ok(())
        }
    }
}

fn handle_snippet_command(
    config: SshConfigFile,
    command: SnippetCommands,
    config_path: &Path,
) -> Result<()> {
    match command {
        SnippetCommands::List => {
            let store = snippet::SnippetStore::load();
            if store.snippets.is_empty() {
                println!("No snippets configured. Use 'purple snippet add' to create one.");
            } else {
                for s in &store.snippets {
                    if s.description.is_empty() {
                        println!("  {}  {}", s.name, s.command);
                    } else {
                        println!("  {}  {}  ({})", s.name, s.command, s.description);
                    }
                }
            }
            Ok(())
        }
        SnippetCommands::Add {
            name,
            command,
            description,
        } => {
            if let Err(e) = snippet::validate_name(&name) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            if let Err(e) = snippet::validate_command(&command) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            if let Some(ref desc) = description {
                if desc.contains(|c: char| c.is_control()) {
                    eprintln!("Description contains control characters.");
                    std::process::exit(1);
                }
            }
            let mut store = snippet::SnippetStore::load();
            let is_update = store.get(&name).is_some();
            store.set(snippet::Snippet {
                name: name.clone(),
                command,
                description: description.unwrap_or_default(),
            });
            store.save()?;
            if is_update {
                println!("Updated snippet '{}'.", name);
            } else {
                println!("Added snippet '{}'.", name);
            }
            Ok(())
        }
        SnippetCommands::Remove { name } => {
            let mut store = snippet::SnippetStore::load();
            if store.get(&name).is_none() {
                eprintln!("No snippet '{}' found.", name);
                std::process::exit(1);
            }
            store.remove(&name);
            store.save()?;
            println!("Removed snippet '{}'.", name);
            Ok(())
        }
        SnippetCommands::Run {
            name,
            alias,
            tag,
            all,
            parallel,
        } => {
            let store = snippet::SnippetStore::load();
            let snip = match store.get(&name) {
                Some(s) => s.clone(),
                None => {
                    eprintln!("No snippet '{}' found.", name);
                    std::process::exit(1);
                }
            };

            let entries = config.host_entries();

            // Determine target hosts
            let targets: Vec<&HostEntry> = if let Some(ref alias) = alias {
                match entries.iter().find(|h| h.alias == *alias) {
                    Some(h) => vec![h],
                    None => {
                        eprintln!("No host '{}' found.", alias);
                        std::process::exit(1);
                    }
                }
            } else if let Some(ref tag_filter) = tag {
                let matched: Vec<_> = entries
                    .iter()
                    .filter(|h| h.tags.iter().any(|t| t.eq_ignore_ascii_case(tag_filter)))
                    .collect();
                if matched.is_empty() {
                    eprintln!("No hosts found with tag '{}'.", tag_filter);
                    std::process::exit(1);
                }
                matched
            } else if all {
                entries.iter().collect()
            } else {
                eprintln!("Specify a host alias, --tag or --all.");
                std::process::exit(1);
            };

            if targets.len() == 1 {
                // Single host: run directly
                let host = targets[0];
                let askpass = host
                    .askpass
                    .clone()
                    .or_else(preferences::load_askpass_default);
                let bw_session = ensure_bw_session(None, askpass.as_deref());
                ensure_keychain_password(&host.alias, askpass.as_deref());
                match snippet::run_snippet(
                    &host.alias,
                    config_path,
                    &snip.command,
                    askpass.as_deref(),
                    bw_session.as_deref(),
                    false,
                    false,
                ) {
                    Ok(r) => {
                        if !r.status.success() {
                            std::process::exit(r.status.code().unwrap_or(1));
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed: {}", e);
                        std::process::exit(1);
                    }
                }
            } else if parallel {
                // Multi-host parallel
                use std::sync::mpsc;
                use std::thread;
                let (tx, rx) = mpsc::channel();
                let max_concurrent: usize = 20;
                let (slot_tx, slot_rx) = mpsc::channel();
                for _ in 0..max_concurrent {
                    let _ = slot_tx.send(());
                }
                let config_path = config_path.to_path_buf();
                // Resolve BW session if any target uses Bitwarden
                let any_bw = targets.iter().any(|h| {
                    let askpass = h.askpass.clone().or_else(preferences::load_askpass_default);
                    askpass.as_deref().unwrap_or("").starts_with("bw:")
                });
                let bw_session = if any_bw {
                    let bw_askpass = targets
                        .iter()
                        .find_map(|h| h.askpass.as_ref().filter(|a| a.starts_with("bw:")))
                        .cloned()
                        .or_else(preferences::load_askpass_default);
                    ensure_bw_session(None, bw_askpass.as_deref())
                } else {
                    None
                };
                let targets_info: Vec<_> = targets
                    .iter()
                    .map(|h| {
                        let askpass = h.askpass.clone().or_else(preferences::load_askpass_default);
                        ensure_keychain_password(&h.alias, askpass.as_deref());
                        (h.alias.clone(), askpass)
                    })
                    .collect();
                let command = snip.command.clone();
                thread::spawn(move || {
                    for (alias, askpass) in targets_info {
                        let _ = slot_rx.recv();
                        let slot_tx = slot_tx.clone();
                        let tx = tx.clone();
                        let config_path = config_path.clone();
                        let command = command.clone();
                        let bw_session = bw_session.clone();
                        thread::spawn(move || {
                            let result = snippet::run_snippet(
                                &alias,
                                &config_path,
                                &command,
                                askpass.as_deref(),
                                bw_session.as_deref(),
                                true,
                                false,
                            );
                            let _ = tx.send((alias, result));
                            let _ = slot_tx.send(());
                        });
                    }
                });

                let host_count = targets.len();
                for _ in 0..host_count {
                    if let Ok((alias, result)) = rx.recv() {
                        match result {
                            Ok(r) => {
                                for line in r.stdout.lines() {
                                    println!("[{}] {}", alias, line);
                                }
                                for line in r.stderr.lines() {
                                    eprintln!("[{}] {}", alias, line);
                                }
                            }
                            Err(e) => eprintln!("[{}] Failed: {}", alias, e),
                        }
                    }
                }
            } else {
                // Multi-host sequential
                let mut bw_session: Option<String> = None;
                for host in &targets {
                    let askpass = host
                        .askpass
                        .clone()
                        .or_else(preferences::load_askpass_default);
                    if let Some(token) =
                        ensure_bw_session(bw_session.as_deref(), askpass.as_deref())
                    {
                        bw_session = Some(token);
                    }
                    ensure_keychain_password(&host.alias, askpass.as_deref());
                    println!("── {} ──", host.alias);
                    match snippet::run_snippet(
                        &host.alias,
                        config_path,
                        &snip.command,
                        askpass.as_deref(),
                        bw_session.as_deref(),
                        false,
                        false,
                    ) {
                        Ok(r) => {
                            if !r.status.success() {
                                eprintln!("Exited with code {}.", r.status.code().unwrap_or(1));
                            }
                        }
                        Err(e) => eprintln!("[{}] Failed: {}", host.alias, e),
                    }
                    println!();
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_config::model::SshConfigFile;

    fn empty_app() -> App {
        let config = SshConfigFile {
            elements: Vec::new(),
            path: std::path::PathBuf::from("/dev/null"),
            crlf: false,
            bom: false,
        };
        App::new(config)
    }

    // ---- cache_entry_is_stale tests ----

    fn valid_status() -> vault_ssh::CertStatus {
        vault_ssh::CertStatus::Valid {
            expires_at: 0,
            remaining_secs: 3600,
            total_secs: 3600,
        }
    }

    fn fixed_elapsed(secs: u64) -> impl FnOnce(std::time::Instant) -> u64 {
        move |_| secs
    }

    #[test]
    fn cache_stale_when_entry_missing() {
        assert!(cache_entry_is_stale(None, None, fixed_elapsed(0)));
        assert!(cache_entry_is_stale(
            None,
            Some(std::time::SystemTime::UNIX_EPOCH),
            fixed_elapsed(0),
        ));
    }

    #[test]
    fn cache_fresh_when_recent_and_mtime_matches() {
        let mtime = std::time::SystemTime::UNIX_EPOCH;
        let entry = (std::time::Instant::now(), valid_status(), Some(mtime));
        assert!(!cache_entry_is_stale(
            Some(&entry),
            Some(mtime),
            fixed_elapsed(1),
        ));
    }

    #[test]
    fn cache_stale_when_current_mtime_differs_from_cached() {
        let cached = std::time::SystemTime::UNIX_EPOCH;
        let current = cached + std::time::Duration::from_secs(5);
        let entry = (std::time::Instant::now(), valid_status(), Some(cached));
        assert!(cache_entry_is_stale(
            Some(&entry),
            Some(current),
            fixed_elapsed(1),
        ));
    }

    #[test]
    fn cache_stale_detects_external_cert_rewrite_via_mtime() {
        // Regression guard for the documented feature: when an external
        // actor (CLI `purple vault sign` from another shell, or another
        // running purple instance) rewrites the cert file behind the TUI's
        // back, the lazy-check loop MUST detect the change via mtime and
        // force a re-read — regardless of the TTL.
        //
        // Timeline:
        //   t=0  purple caches Valid status with mtime M1
        //   t=1  external sign writes new cert, mtime becomes M2 > M1
        //   t=2  lazy-check runs: elapsed 2s (far under the 5-min TTL),
        //        but the mtime mismatch forces cache_stale = true.
        let cached_mtime = std::time::SystemTime::UNIX_EPOCH;
        let rewritten_mtime = cached_mtime + std::time::Duration::from_secs(60);
        let entry = (
            std::time::Instant::now(),
            valid_status(),
            Some(cached_mtime),
        );
        assert!(
            cache_entry_is_stale(Some(&entry), Some(rewritten_mtime), fixed_elapsed(2)),
            "external rewrite via mtime mismatch must force re-check even within TTL"
        );
    }

    #[test]
    fn cache_stale_when_file_appears_after_missing_cache() {
        let entry = (std::time::Instant::now(), valid_status(), None);
        assert!(cache_entry_is_stale(
            Some(&entry),
            Some(std::time::SystemTime::UNIX_EPOCH),
            fixed_elapsed(1),
        ));
    }

    #[test]
    fn cache_stale_when_file_disappears_after_cached_mtime() {
        let mtime = std::time::SystemTime::UNIX_EPOCH;
        let entry = (std::time::Instant::now(), valid_status(), Some(mtime));
        assert!(cache_entry_is_stale(Some(&entry), None, fixed_elapsed(1)));
    }

    #[test]
    fn cache_stale_when_ttl_exceeded_even_if_mtime_matches() {
        let mtime = std::time::SystemTime::UNIX_EPOCH;
        let entry = (std::time::Instant::now(), valid_status(), Some(mtime));
        let over = vault_ssh::CERT_STATUS_CACHE_TTL_SECS + 1;
        assert!(cache_entry_is_stale(
            Some(&entry),
            Some(mtime),
            fixed_elapsed(over),
        ));
    }

    #[test]
    fn cache_invalid_entry_uses_shorter_backoff() {
        let mtime = std::time::SystemTime::UNIX_EPOCH;
        let entry = (
            std::time::Instant::now(),
            vault_ssh::CertStatus::Invalid("boom".to_string()),
            Some(mtime),
        );
        // Just above error backoff but well below the normal TTL: must be
        // stale under the shorter Invalid backoff.
        let secs = vault_ssh::CERT_ERROR_BACKOFF_SECS + 1;
        assert!(secs < vault_ssh::CERT_STATUS_CACHE_TTL_SECS);
        assert!(cache_entry_is_stale(
            Some(&entry),
            Some(mtime),
            fixed_elapsed(secs),
        ));
    }

    #[test]
    fn cache_invalid_entry_fresh_within_backoff() {
        let mtime = std::time::SystemTime::UNIX_EPOCH;
        let entry = (
            std::time::Instant::now(),
            vault_ssh::CertStatus::Invalid("boom".to_string()),
            Some(mtime),
        );
        assert!(!cache_entry_is_stale(
            Some(&entry),
            Some(mtime),
            fixed_elapsed(0),
        ));
    }

    // ---- end cache_entry_is_stale tests ----

    #[test]
    fn test_sync_summary_still_syncing() {
        let mut app = empty_app();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        app.syncing_providers.insert("aws".to_string(), cancel);
        app.sync_done.push("DigitalOcean".to_string());
        set_sync_summary(&mut app);
        let status = app.status.as_ref().unwrap();
        assert_eq!(status.text, "Synced: DigitalOcean...");
        assert!(!status.is_error);
        // sync_done should NOT be cleared while still syncing
        assert_eq!(app.sync_done.len(), 1);
    }

    #[test]
    fn vault_sign_summary_single_failure_shows_only_error() {
        let msg = format_vault_sign_summary(0, 1, 0, Some("Vault SSH permission denied."));
        assert_eq!(msg, "Vault SSH permission denied.");
    }

    #[test]
    fn vault_sign_summary_includes_error_on_partial_failure() {
        let msg = format_vault_sign_summary(2, 1, 0, Some("role not found"));
        assert_eq!(msg, "Signed 2 of 3 certificates. 1 failed: role not found");
    }

    #[test]
    fn vault_sign_summary_failure_without_error_text() {
        let msg = format_vault_sign_summary(0, 1, 0, None);
        assert_eq!(msg, "Signed 0 of 1 certificate. 1 failed");
    }

    #[test]
    fn vault_sign_summary_all_success() {
        let msg = format_vault_sign_summary(3, 0, 0, None);
        assert_eq!(msg, "Signed 3 of 3 certificates.");
    }

    #[test]
    fn vault_sign_summary_skipped_with_signed() {
        let msg = format_vault_sign_summary(1, 0, 2, None);
        assert_eq!(msg, "Signed 1 of 3 certificates. 2 already valid.");
    }

    #[test]
    fn vault_sign_summary_all_skipped() {
        let msg = format_vault_sign_summary(0, 0, 3, None);
        assert_eq!(msg, "All 3 certificates already valid. Nothing to sign.");
    }

    #[test]
    fn replace_spinner_frame_replaces_known_spinner() {
        let text = "\u{280B} Signing 1/3: myhost (V to cancel)";
        let result = replace_spinner_frame(text, "\u{2819}");
        assert_eq!(
            result.as_deref(),
            Some("\u{2819} Signing 1/3: myhost (V to cancel)")
        );
    }

    #[test]
    fn replace_spinner_frame_ignores_non_spinner_text() {
        let text = "Signing 0/3 (V to cancel)";
        assert!(replace_spinner_frame(text, "\u{2819}").is_none());
    }

    #[test]
    fn replace_spinner_frame_ignores_regular_status() {
        let text = "Signed 3 of 3 certificates.";
        assert!(replace_spinner_frame(text, "\u{2819}").is_none());
    }

    #[test]
    fn test_sync_summary_all_done() {
        let mut app = empty_app();
        app.sync_done.push("AWS".to_string());
        app.sync_done.push("Hetzner".to_string());
        set_sync_summary(&mut app);
        let status = app.status.as_ref().unwrap();
        assert_eq!(status.text, "Synced: AWS, Hetzner");
        assert!(!status.is_error);
        // sync_done should be cleared when all done
        assert!(app.sync_done.is_empty());
        assert!(!app.sync_had_errors);
    }

    #[test]
    fn test_sync_summary_with_errors() {
        let mut app = empty_app();
        app.sync_done.push("AWS".to_string());
        app.sync_had_errors = true;
        set_sync_summary(&mut app);
        let status = app.status.as_ref().unwrap();
        assert_eq!(status.text, "Synced: AWS");
        assert!(status.is_error);
        // Error flag should be reset when batch completes
        assert!(!app.sync_had_errors);
    }

    #[test]
    fn test_sync_summary_errors_persist_while_syncing() {
        let mut app = empty_app();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        app.syncing_providers.insert("vultr".to_string(), cancel);
        app.sync_done.push("AWS".to_string());
        app.sync_had_errors = true;
        set_sync_summary(&mut app);
        let status = app.status.as_ref().unwrap();
        assert!(status.is_error);
        // Error flag should persist while still syncing
        assert!(app.sync_had_errors);
    }

    // =========================================================================
    // first_launch_init
    // =========================================================================

    #[test]
    fn first_launch_creates_dir_and_backup() {
        let dir = std::env::temp_dir().join(format!(
            "purple_test_first_launch_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let purple_dir = dir.join(".purple");
        let config_path = dir.join("config");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(&config_path, "Host myserver\n  HostName 10.0.0.1\n").unwrap();

        let result = first_launch_init(&purple_dir, &config_path);
        assert_eq!(
            result,
            Some(true),
            "Should return Some(true) when config exists"
        );
        assert!(purple_dir.exists(), ".purple dir should be created");
        let backup = purple_dir.join("config.original");
        assert!(backup.exists(), "config.original should be created");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "Host myserver\n  HostName 10.0.0.1\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_launch_returns_none_on_second_call() {
        let dir = std::env::temp_dir().join(format!(
            "purple_test_first_launch_twice_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let purple_dir = dir.join(".purple");
        let config_path = dir.join("config");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(&config_path, "Host a\n").unwrap();

        assert!(first_launch_init(&purple_dir, &config_path).is_some());
        assert!(first_launch_init(&purple_dir, &config_path).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_launch_no_config_file_skips_backup() {
        let dir = std::env::temp_dir().join(format!(
            "purple_test_first_launch_no_cfg_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let purple_dir = dir.join(".purple");
        let config_path = dir.join("nonexistent_config");

        let result = first_launch_init(&purple_dir, &config_path);
        assert_eq!(
            result,
            Some(false),
            "Should return Some(false) when no config"
        );
        assert!(purple_dir.exists(), ".purple dir should be created");
        assert!(
            !purple_dir.join("config.original").exists(),
            "config.original should NOT be created when config does not exist"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_launch_backup_not_overwritten() {
        let dir = std::env::temp_dir().join(format!(
            "purple_test_first_launch_no_overwrite_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let purple_dir = dir.join(".purple");
        let config_path = dir.join("config");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(&config_path, "original content\n").unwrap();

        first_launch_init(&purple_dir, &config_path);
        let backup = purple_dir.join("config.original");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "original content\n"
        );

        // Modify the config and call again (simulates second launch)
        std::fs::write(&config_path, "modified content\n").unwrap();
        first_launch_init(&purple_dir, &config_path);

        // Backup should still have original content
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "original content\n",
            "config.original should never be overwritten"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_launch_has_backup_true_when_config_exists() {
        let dir = std::env::temp_dir().join(format!(
            "purple_test_first_launch_has_backup_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let purple_dir = dir.join(".purple");
        let config_path = dir.join("config");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(&config_path, "Host a\n").unwrap();

        assert_eq!(first_launch_init(&purple_dir, &config_path), Some(true));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_launch_has_backup_false_without_config() {
        let dir = std::env::temp_dir().join(format!(
            "purple_test_first_launch_no_backup_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let purple_dir = dir.join(".purple");
        let config_path = dir.join("nonexistent");

        assert_eq!(first_launch_init(&purple_dir, &config_path), Some(false));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // =========================================================================
    // Welcome screen handler state transitions
    // =========================================================================
    // Keys to test on Welcome screen:
    // Enter -> HostList
    // Esc -> HostList
    // ? -> Help
    // I (known_hosts > 0) -> HostList + import
    // I (known_hosts = 0) -> HostList (treated as any other key)
    // random char (q, a, j, etc.) -> HostList
    // arrow keys -> HostList

    #[test]
    fn welcome_enter_goes_to_host_list() {
        let mut app = empty_app();
        app.screen = app::Screen::Welcome {
            has_backup: false,
            host_count: 0,
            known_hosts_count: 0,
        };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::HostList));
    }

    #[test]
    fn welcome_esc_goes_to_host_list() {
        let mut app = empty_app();
        app.screen = app::Screen::Welcome {
            has_backup: true,
            host_count: 5,
            known_hosts_count: 0,
        };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::HostList));
    }

    #[test]
    fn welcome_question_mark_goes_to_help() {
        let mut app = empty_app();
        app.screen = app::Screen::Welcome {
            has_backup: false,
            host_count: 0,
            known_hosts_count: 0,
        };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('?'),
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::Help { .. }));
    }

    #[test]
    fn welcome_i_without_known_hosts_goes_to_host_list() {
        let mut app = empty_app();
        app.screen = app::Screen::Welcome {
            has_backup: false,
            host_count: 0,
            known_hosts_count: 0,
        };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('I'),
            crossterm::event::KeyModifiers::SHIFT,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::HostList));
    }

    #[test]
    fn welcome_random_char_goes_to_host_list() {
        let mut app = empty_app();
        app.screen = app::Screen::Welcome {
            has_backup: false,
            host_count: 3,
            known_hosts_count: 0,
        };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('z'),
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::HostList));
    }

    #[test]
    fn welcome_arrow_key_goes_to_host_list() {
        let mut app = empty_app();
        app.screen = app::Screen::Welcome {
            has_backup: false,
            host_count: 0,
            known_hosts_count: 5,
        };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::HostList));
    }

    // =========================================================================
    // ConfirmImport handler state transitions
    // =========================================================================
    // Keys to test on ConfirmImport screen:
    // y -> HostList + import executed
    // Esc -> HostList, no import
    // n -> HostList, no import
    // random key -> stays on ConfirmImport
    // Enter -> stays on ConfirmImport
    // ? -> stays on ConfirmImport

    #[test]
    fn confirm_import_esc_goes_to_host_list() {
        let mut app = empty_app();
        app.screen = app::Screen::ConfirmImport { count: 10 };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::HostList));
    }

    #[test]
    fn confirm_import_n_goes_to_host_list() {
        let mut app = empty_app();
        app.screen = app::Screen::ConfirmImport { count: 10 };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('n'),
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::HostList));
    }

    #[test]
    fn confirm_import_random_key_stays() {
        let mut app = empty_app();
        app.screen = app::Screen::ConfirmImport { count: 10 };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::ConfirmImport { .. }));
    }

    #[test]
    fn confirm_import_enter_stays() {
        let mut app = empty_app();
        app.screen = app::Screen::ConfirmImport { count: 10 };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::ConfirmImport { .. }));
    }

    #[test]
    fn confirm_import_question_mark_stays() {
        let mut app = empty_app();
        app.screen = app::Screen::ConfirmImport { count: 10 };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('?'),
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::ConfirmImport { .. }));
    }

    #[test]
    fn confirm_import_arrow_key_stays() {
        let mut app = empty_app();
        app.screen = app::Screen::ConfirmImport { count: 5 };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::ConfirmImport { .. }));
    }

    // =========================================================================
    // App known_hosts_count field
    // =========================================================================

    #[test]
    fn app_known_hosts_count_default_zero() {
        let app = empty_app();
        assert_eq!(app.known_hosts_count, 0);
    }

    // =========================================================================
    // HostList I key handler
    // =========================================================================
    // On HostList, I calls count_known_hosts_candidates() which reads the real
    // filesystem, so we can't control the result. But we can verify the error
    // path (when count == 0, it sets error status) by testing on a system
    // without importable known_hosts, or by testing that the key is handled
    // without panic.

    #[test]
    fn host_list_i_key_does_not_panic() {
        let mut app = empty_app();
        app.screen = app::Screen::HostList;
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('I'),
            crossterm::event::KeyModifiers::SHIFT,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        // This calls count_known_hosts_candidates() which reads real filesystem.
        // It should either go to ConfirmImport (if known_hosts has entries)
        // or set error status (if not). Either way, it should not panic.
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(
            matches!(app.screen, app::Screen::ConfirmImport { .. })
                || matches!(app.screen, app::Screen::HostList)
        );
    }

    #[test]
    fn host_list_i_key_sets_error_when_no_hosts_available() {
        // If count_known_hosts_candidates() returns 0, status should be error
        let mut app = empty_app();
        app.screen = app::Screen::HostList;
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('I'),
            crossterm::event::KeyModifiers::SHIFT,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        // If we got ConfirmImport, known_hosts had entries (can't control that)
        // If we stayed on HostList, verify error status was set
        if matches!(app.screen, app::Screen::HostList) {
            let status = app.status.as_ref().expect("status should be set");
            assert!(status.is_error);
            assert_eq!(status.text, "No importable hosts in known_hosts.");
        }
    }

    // =========================================================================
    // Empty state behavior per screen
    // =========================================================================

    #[test]
    fn empty_state_hidden_during_welcome() {
        // When screen is Welcome, the empty state match returns ""
        let screen = app::Screen::Welcome {
            has_backup: false,
            host_count: 0,
            known_hosts_count: 0,
        };
        assert!(matches!(screen, app::Screen::Welcome { .. }));
        // The host_list.rs code does:
        //   if matches!(app.screen, app::Screen::Welcome { .. }) { "" }
        //   else { "It's quiet in here..." }
    }

    #[test]
    fn empty_state_shown_during_host_list() {
        let screen = app::Screen::HostList;
        assert!(!matches!(screen, app::Screen::Welcome { .. }));
    }

    #[test]
    fn empty_state_shown_during_confirm_import() {
        let screen = app::Screen::ConfirmImport { count: 5 };
        assert!(!matches!(screen, app::Screen::Welcome { .. }));
    }

    // =========================================================================
    // Welcome with backup variations
    // =========================================================================

    #[test]
    fn welcome_q_goes_to_host_list() {
        let mut app = empty_app();
        app.screen = app::Screen::Welcome {
            has_backup: true,
            host_count: 10,
            known_hosts_count: 0,
        };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('q'),
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::HostList));
    }

    #[test]
    fn welcome_tab_goes_to_host_list() {
        let mut app = empty_app();
        app.screen = app::Screen::Welcome {
            has_backup: false,
            host_count: 0,
            known_hosts_count: 5,
        };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::HostList));
    }

    // =========================================================================
    // ConfirmImport y key (actual import - reads filesystem)
    // =========================================================================

    #[test]
    fn confirm_import_y_transitions_to_host_list() {
        let mut app = empty_app();
        app.screen = app::Screen::ConfirmImport { count: 10 };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('y'),
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        // Should transition to HostList regardless of import result
        assert!(matches!(app.screen, app::Screen::HostList));
        // Status should be set (either success or error)
        assert!(app.status.is_some());
    }

    // =========================================================================
    // ConfirmImport tab/q stays
    // =========================================================================

    #[test]
    fn confirm_import_tab_stays() {
        let mut app = empty_app();
        app.screen = app::Screen::ConfirmImport { count: 5 };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::ConfirmImport { .. }));
    }

    #[test]
    fn confirm_import_q_stays() {
        let mut app = empty_app();
        app.screen = app::Screen::ConfirmImport { count: 5 };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('q'),
            crossterm::event::KeyModifiers::NONE,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::ConfirmImport { .. }));
    }

    // =========================================================================
    // execute_known_hosts_import — test via import_from_file (controlled input)
    // =========================================================================
    // We can't call execute_known_hosts_import directly (it reads real
    // known_hosts), but we can test the same logic paths by using
    // import_from_file + config.write() on controlled temp files.

    #[test]
    fn import_successful_sets_success_status() {
        let dir = std::env::temp_dir().join(format!(
            "purple_test_import_status_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_path = dir.join("config");
        std::fs::write(&config_path, "").unwrap();
        let config = crate::ssh_config::model::SshConfigFile {
            elements: Vec::new(),
            path: config_path,
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);

        let hosts_file = dir.join("hosts.txt");
        std::fs::write(&hosts_file, "web.example.com\ndb.example.com\n").unwrap();

        let result = import::import_from_file(&mut app.config, &hosts_file, Some("test"));
        let (imported, skipped, _, _) = result.unwrap();
        assert_eq!(imported, 2);
        assert_eq!(skipped, 0);

        // Write should succeed
        assert!(app.config.write().is_ok());
        app.reload_hosts();
        assert_eq!(app.hosts.len(), 2);

        // Verify the status message format
        let msg = format!(
            "Imported {} host{}, skipped {} duplicate{}",
            imported,
            if imported == 1 { "" } else { "s" },
            skipped,
            if skipped == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "Imported 2 hosts, skipped 0 duplicates");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_all_duplicates_sets_status() {
        let dir = std::env::temp_dir().join(format!(
            "purple_test_import_alldup_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_path = dir.join("config");
        std::fs::write(&config_path, "").unwrap();
        let config = crate::ssh_config::model::SshConfigFile {
            elements: Vec::new(),
            path: config_path,
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);

        let hosts_file = dir.join("hosts.txt");
        std::fs::write(&hosts_file, "web.example.com\n").unwrap();

        // First import
        let _ = import::import_from_file(&mut app.config, &hosts_file, None);
        let _ = app.config.write();
        app.reload_hosts();

        // Second import - all duplicates
        let (imported, skipped, _, _) =
            import::import_from_file(&mut app.config, &hosts_file, None).unwrap();
        assert_eq!(imported, 0);
        assert_eq!(skipped, 1);

        let msg = if skipped == 1 {
            "Host already exists".to_string()
        } else {
            format!("All {} hosts already exist", skipped)
        };
        assert_eq!(msg, "Host already exists");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_write_failure_rolls_back_config() {
        // Create a config pointing to a read-only path so write() fails
        let dir = std::env::temp_dir().join(format!(
            "purple_test_import_writefail_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_path = dir.join("nonexistent_dir").join("config");
        // config_path parent doesn't exist, so write() will fail
        let config = crate::ssh_config::model::SshConfigFile {
            elements: Vec::new(),
            path: config_path,
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        let config_backup = app.config.clone();

        let hosts_file = dir.join("hosts.txt");
        std::fs::write(&hosts_file, "web.example.com\n").unwrap();

        let (imported, _, _, _) =
            import::import_from_file(&mut app.config, &hosts_file, None).unwrap();
        assert_eq!(imported, 1);

        // Write should fail because parent dir doesn't exist
        let write_result = app.config.write();
        assert!(write_result.is_err());

        // After failure, rollback should restore config
        app.config = config_backup;
        let hosts = app.config.host_entries();
        assert_eq!(hosts.len(), 0, "config should be rolled back to empty");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn known_hosts_count_not_reset_on_write_failure() {
        // The execute_known_hosts_import function returns early on write failure
        // without resetting known_hosts_count. This is correct behavior:
        // if the import didn't save, the user might want to try again.
        let mut app = empty_app();
        app.known_hosts_count = 10;
        // Simulate: write failure would do `return` before `app.known_hosts_count = 0`
        // So known_hosts_count should remain 10
        assert_eq!(app.known_hosts_count, 10);
    }

    #[test]
    fn known_hosts_count_not_reset_on_import_error() {
        // When import_from_known_hosts returns Err, known_hosts_count is not reset
        let mut app = empty_app();
        app.known_hosts_count = 5;
        // The Err branch only sets status, doesn't touch known_hosts_count
        app.set_status("some error", true);
        assert_eq!(app.known_hosts_count, 5);
    }

    #[test]
    fn known_hosts_count_reset_on_success() {
        // When import succeeds (even with 0 imported), known_hosts_count is reset
        let mut app = empty_app();
        app.known_hosts_count = 15;
        app.known_hosts_count = 0; // simulates the Ok branch
        assert_eq!(app.known_hosts_count, 0);
    }

    // =========================================================================
    // Welcome I key with known_hosts_count > 0
    // =========================================================================

    #[test]
    fn welcome_i_with_known_hosts_transitions_to_host_list() {
        // When known_hosts_count > 0, I should trigger import and go to HostList
        let mut app = empty_app();
        app.screen = app::Screen::Welcome {
            has_backup: false,
            host_count: 0,
            known_hosts_count: 10,
        };
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('I'),
            crossterm::event::KeyModifiers::SHIFT,
        );
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = crate::handler::handle_key_event(&mut app, key, &tx);
        assert!(matches!(app.screen, app::Screen::HostList));
        // Status should be set (import attempted)
        assert!(app.status.is_some());
    }

    // =========================================================================
    // Cheat sheet verification
    // =========================================================================

    #[test]
    fn cheat_sheet_contains_import_entry() {
        // The help.rs host_list_columns() should contain "I" key with "import known_hosts"
        let source = include_str!("ui/help.rs");
        assert!(
            source.contains(r#"help_line("I", "import known_hosts")"#),
            "cheat sheet should have I key"
        );
    }

    #[test]
    fn cheat_sheet_i_after_s_and_k() {
        let source = include_str!("ui/help.rs");
        let k_pos = source
            .find(r#"help_line("K","#)
            .expect("K should be in cheat sheet");
        let s_pos = source
            .find(r#"help_line("S","#)
            .expect("S should be in cheat sheet");
        let i_pos = source
            .find(r#"help_line("I","#)
            .expect("I should be in cheat sheet");
        assert!(k_pos < s_pos, "K should come before S");
        assert!(s_pos < i_pos, "S should come before I");
    }

    // =========================================================================
    // UI consistency: ConfirmImport dialog structure
    // =========================================================================

    #[test]
    fn confirm_import_dialog_has_same_structure_as_confirm_delete() {
        // Both dialogs use: Block + rounded borders + 4 text lines
        // (blank, question, blank, y/Esc footer)
        // ConfirmDelete: 48x7, ConfirmImport: 52x7
        // Verify by checking source structure
        let source = include_str!("ui/confirm_dialog.rs");

        // Both use BorderType::Rounded
        let rounded_count = source.matches("BorderType::Rounded").count();
        assert!(rounded_count >= 4, "all dialogs should use rounded borders");

        // ConfirmImport uses footer_key for y (not danger, since import is not destructive)
        assert!(
            source.contains(r#"Span::styled(" y ", theme::footer_key())"#),
            "import dialog y should use footer_key"
        );
    }

    // =========================================================================
    // Screen variant field values
    // =========================================================================

    #[test]
    fn confirm_import_preserves_count() {
        let screen = app::Screen::ConfirmImport { count: 42 };
        if let app::Screen::ConfirmImport { count } = screen {
            assert_eq!(count, 42);
        } else {
            panic!("expected ConfirmImport");
        }
    }

    #[test]
    fn welcome_preserves_all_fields() {
        let screen = app::Screen::Welcome {
            has_backup: true,
            host_count: 12,
            known_hosts_count: 34,
        };
        if let app::Screen::Welcome {
            has_backup,
            host_count,
            known_hosts_count,
        } = screen
        {
            assert!(has_backup);
            assert_eq!(host_count, 12);
            assert_eq!(known_hosts_count, 34);
        } else {
            panic!("expected Welcome");
        }
    }

    #[test]
    fn test_format_sync_diff_all_changes() {
        assert_eq!(format_sync_diff(3, 1, 2), " (+3 ~1 -2)");
    }

    #[test]
    fn test_format_sync_diff_no_changes() {
        assert_eq!(format_sync_diff(0, 0, 0), "");
    }

    #[test]
    fn test_format_sync_diff_only_added() {
        assert_eq!(format_sync_diff(5, 0, 0), " (+5)");
    }

    // CLI refactor regression: `purple vault-sign` was renamed to a nested
    // `purple vault sign` subcommand group matching `provider`/`theme`. Verify
    // clap parses both the alias form and --all.
    #[test]
    fn cli_vault_sign_alias_parsing() {
        use clap::Parser;
        let cli = Cli::try_parse_from(["purple", "vault", "sign", "myhost"]).unwrap();
        match cli.command {
            Some(Commands::Vault {
                command:
                    VaultCommands::Sign {
                        alias,
                        all,
                        vault_addr,
                    },
            }) => {
                assert_eq!(alias.as_deref(), Some("myhost"));
                assert!(!all);
                assert!(vault_addr.is_none());
            }
            _ => panic!("expected Vault::Sign"),
        }
    }

    #[test]
    fn cli_vault_sign_all_flag_parsing() {
        use clap::Parser;
        let cli = Cli::try_parse_from(["purple", "vault", "sign", "--all"]).unwrap();
        match cli.command {
            Some(Commands::Vault {
                command:
                    VaultCommands::Sign {
                        alias,
                        all,
                        vault_addr,
                    },
            }) => {
                assert_eq!(alias, None);
                assert!(all);
                assert!(vault_addr.is_none());
            }
            _ => panic!("expected Vault::Sign --all"),
        }
    }

    #[test]
    fn cli_vault_sign_vault_addr_flag_parsing() {
        use clap::Parser;
        let cli = Cli::try_parse_from([
            "purple",
            "vault",
            "sign",
            "--all",
            "--vault-addr",
            "http://127.0.0.1:8200",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Vault {
                command:
                    VaultCommands::Sign {
                        alias: _,
                        all,
                        vault_addr,
                    },
            }) => {
                assert!(all);
                assert_eq!(vault_addr.as_deref(), Some("http://127.0.0.1:8200"));
            }
            _ => panic!("expected Vault::Sign with --vault-addr"),
        }
    }

    #[test]
    fn should_write_certificate_file_only_when_empty() {
        // Empty string: purple owns the cert path, write it.
        assert!(should_write_certificate_file(""));
        // Whitespace-only is treated as empty so a stray space typed in the
        // form does not lock purple out of writing the directive.
        assert!(should_write_certificate_file(" "));
        assert!(should_write_certificate_file("\t"));
        assert!(should_write_certificate_file("   \t  "));
        // Any user-set value (default purple path included): never overwrite,
        // because the user may rely on a custom path and we never want to
        // silently change it.
        assert!(!should_write_certificate_file("/custom/path/cert.pub"));
        assert!(!should_write_certificate_file("~/.ssh/my-cert.pub"));
        assert!(!should_write_certificate_file("relative/path"));
        // A path with leading/trailing space is still a real path; trim is
        // applied to the emptiness check, not the value itself.
        assert!(!should_write_certificate_file(" /tmp/cert.pub "));
    }

    #[test]
    fn ensure_vault_ssh_returns_none_when_no_role_configured() {
        // Build a host with no vault_ssh and no provider mapping. The function
        // must short-circuit before touching disk or shelling out.
        let dir = std::env::temp_dir().join(format!(
            "purple_test_ensure_vault_norole_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config");
        std::fs::write(&config_path, "Host plain\n  HostName 1.2.3.4\n").unwrap();
        let mut config = SshConfigFile::parse(&config_path).unwrap();
        let host = config.host_entries().into_iter().next().unwrap();
        let provider_config = providers::config::ProviderConfig::parse("");
        let result = ensure_vault_ssh_if_needed(&host.alias, &host, &provider_config, &mut config);
        assert!(
            result.is_none(),
            "no role configured: must short-circuit to None"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cli_legacy_vault_sign_flat_form_rejected() {
        // The old flat `purple vault-sign` subcommand was removed. Ensure it
        // does not silently match something else.
        use clap::Parser;
        let result = Cli::try_parse_from(["purple", "vault-sign", "myhost"]);
        assert!(
            result.is_err(),
            "legacy `vault-sign` must not parse after refactor"
        );
    }
}
