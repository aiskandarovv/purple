//! CLI subcommand handlers. Each function handles one clap subcommand
//! (provider, tunnel, password, snippet, add, import, sync, logs, theme,
//! vault sign) and runs outside the TUI in a non-interactive terminal context.

use anyhow::{Context, Result};
use std::path::Path;

use crate::providers;
use crate::snippet;
use crate::ssh_config::model::{HostEntry, SshConfigFile};
use crate::vault_ssh;

use super::cli_args::{
    PasswordCommands, ProviderCommands, SnippetCommands, ThemeCommands, TunnelCommands,
};
use super::{askpass, import, logging, preferences, quick_add, should_write_certificate_file, ui};

pub(super) fn handle_quick_add(
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
        eprintln!("{}", crate::messages::cli::ALIAS_EMPTY);
        std::process::exit(1);
    }
    if alias_str.contains(char::is_whitespace) {
        eprintln!("{}", crate::messages::cli::ALIAS_WHITESPACE);
        std::process::exit(1);
    }
    if crate::ssh_config::model::is_host_pattern(&alias_str) {
        eprintln!("{}", crate::messages::cli::ALIAS_PATTERN_CHARS);
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
            eprintln!("{}", crate::messages::cli::control_chars(name));
            std::process::exit(1);
        }
    }

    // Reject whitespace in hostname and user (matches TUI validation)
    if parsed.hostname.contains(char::is_whitespace) {
        eprintln!("{}", crate::messages::cli::HOSTNAME_WHITESPACE);
        std::process::exit(1);
    }
    if parsed.user.contains(char::is_whitespace) {
        eprintln!("{}", crate::messages::cli::USER_WHITESPACE);
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
    println!("{}", crate::messages::cli::welcome(&alias_str));
    Ok(())
}

pub(super) fn handle_import(
    mut config: SshConfigFile,
    file: Option<&str>,
    known_hosts: bool,
    group: Option<&str>,
) -> Result<()> {
    let result = if known_hosts {
        import::import_from_known_hosts(&mut config, group)
    } else if let Some(path) = file {
        let resolved = super::resolve_config_path(path)?;
        import::import_from_file(&mut config, &resolved, group)
    } else {
        eprintln!("{}", crate::messages::cli::IMPORT_NO_FILE);
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

pub(super) fn handle_sync(
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
                eprintln!("{}", crate::messages::cli::no_config_for(name));
                std::process::exit(1);
            }
        }
    } else {
        let configured = provider_config.configured_providers();
        if configured.is_empty() {
            eprintln!("{}", crate::messages::cli::NO_PROVIDERS);
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
                print!("{}", crate::messages::cli::syncing(display_name, msg));
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
                print!("{}", crate::messages::cli::syncing(display_name, ""));
            } else {
                println!("{}", crate::messages::cli::syncing(display_name, &summary));
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
                    "{}",
                    crate::messages::cli::servers_found_with_failures(hosts.len(), failures, total)
                );
                if remove {
                    eprintln!("{}", crate::messages::cli::sync_skip_remove(display_name));
                }
                any_failures = true;
                (hosts, true)
            }
            Err(e) => {
                println!("failed.");
                eprintln!("{}", crate::messages::cli::sync_error(display_name, &e));
                any_failures = true;
                any_hard_failures = true;
                continue;
            }
        };
        if !suppress_remove {
            println!("{}", crate::messages::cli::servers_found(hosts.len()));
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
            "{}",
            crate::messages::cli::sync_result(
                prefix,
                result.added,
                result.updated,
                result.unchanged
            )
        );
        if result.removed > 0 {
            println!("{}", crate::messages::cli::sync_removed(result.removed));
        }
        if result.stale > 0 {
            println!("{}", crate::messages::cli::sync_stale(result.stale));
        }
        if result.added > 0 || result.updated > 0 || result.removed > 0 || result.stale > 0 {
            any_changes = true;
        }
    }

    if any_changes && !dry_run {
        if any_hard_failures {
            eprintln!("{}", crate::messages::cli::SYNC_SKIP_WRITE);
        } else {
            config.write()?;
        }
    }

    if any_failures {
        std::process::exit(1);
    }

    Ok(())
}

pub(super) fn handle_provider_command(command: ProviderCommands) -> Result<()> {
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
                    eprintln!("{}", crate::messages::cli::WARN_URL_NOT_USED);
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
                eprintln!("{}", crate::messages::cli::WARN_PROFILE_NOT_USED);
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
                eprintln!("{}", crate::messages::cli::WARN_PROJECT_NOT_USED);
                project = None;
            }
            if provider != "oracle" && compartment.is_some() {
                eprintln!("{}", crate::messages::cli::WARN_COMPARTMENT_NOT_USED);
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
                    eprintln!("{}", crate::messages::cli::PROXMOX_URL_REQUIRED);
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
                match super::resolve_token(token, token_stdin) {
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
            if crate::ssh_config::model::is_host_pattern(&alias_prefix) {
                eprintln!("{}", crate::messages::cli::ALIAS_PREFIX_INVALID);
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
                    eprintln!("{}", crate::messages::cli::control_chars(name));
                    std::process::exit(1);
                }
            }
            if user.contains(char::is_whitespace) {
                eprintln!("{}", crate::messages::cli::USER_WHITESPACE);
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
                eprintln!("{}", crate::messages::cli::AWS_REGIONS_REQUIRED);
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
                    eprintln!("{}", crate::messages::cli::AZURE_REGIONS_REQUIRED);
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
                eprintln!("{}", crate::messages::cli::GCP_PROJECT_REQUIRED);
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
            println!("{}", crate::messages::cli::saved_config(&provider));
            Ok(())
        }
        ProviderCommands::List => {
            let config = providers::config::ProviderConfig::load();
            let sections = config.configured_providers();
            if sections.is_empty() {
                println!("{}", crate::messages::cli::NO_PROVIDERS);
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
                eprintln!("{}", crate::messages::cli::no_config_to_remove(&provider));
                std::process::exit(1);
            }
            config.remove_section(&provider);
            config
                .save()
                .map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
            println!("{}", crate::messages::cli::removed_config(&provider));
            Ok(())
        }
    }
}

pub(super) fn handle_tunnel_command(
    mut config: SshConfigFile,
    command: TunnelCommands,
) -> Result<()> {
    match command {
        TunnelCommands::List { alias } => {
            if let Some(alias) = alias {
                // Show tunnels for a specific host
                if !config.has_host(&alias) {
                    eprintln!("{}", crate::messages::cli::host_not_found(&alias));
                    std::process::exit(1);
                }
                let rules = config.find_tunnel_directives(&alias);
                if rules.is_empty() {
                    println!("{}", crate::messages::cli::no_tunnels_for(&alias));
                } else {
                    println!("{}", crate::messages::cli::tunnels_for(&alias));
                    for rule in &rules {
                        println!("  {}", rule.display());
                    }
                }
            } else {
                // Show all hosts with tunnels
                let entries = config.host_entries();
                let with_tunnels: Vec<_> = entries.iter().filter(|e| e.tunnel_count > 0).collect();
                if with_tunnels.is_empty() {
                    println!("{}", crate::messages::cli::NO_TUNNELS);
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
                eprintln!("{}", crate::messages::cli::host_not_found(&alias));
                std::process::exit(1);
            }
            if config.is_included_host(&alias) {
                eprintln!("{}", crate::messages::cli::included_host_read_only(&alias));
                std::process::exit(1);
            }
            let rule = crate::tunnel::TunnelRule::from_cli_spec(&forward).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let key = rule.tunnel_type.directive_key();
            let value = rule.to_directive_value();
            // Check for duplicate forward
            if config.has_forward(&alias, key, &value) {
                eprintln!("{}", crate::messages::cli::forward_exists(&forward, &alias));
                std::process::exit(1);
            }
            config.add_forward(&alias, key, &value);
            if let Err(e) = config.write() {
                eprintln!("{}", crate::messages::cli::save_config_failed(&e));
                std::process::exit(1);
            }
            println!("{}", crate::messages::cli::added_forward(&forward, &alias));
            Ok(())
        }
        TunnelCommands::Remove { alias, forward } => {
            if !config.has_host(&alias) {
                eprintln!("{}", crate::messages::cli::host_not_found(&alias));
                std::process::exit(1);
            }
            if config.is_included_host(&alias) {
                eprintln!("{}", crate::messages::cli::included_host_read_only(&alias));
                std::process::exit(1);
            }
            let rule = crate::tunnel::TunnelRule::from_cli_spec(&forward).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let key = rule.tunnel_type.directive_key();
            let value = rule.to_directive_value();
            let removed = config.remove_forward(&alias, key, &value);
            if !removed {
                eprintln!(
                    "{}",
                    crate::messages::cli::forward_not_found(&forward, &alias)
                );
                std::process::exit(1);
            }
            if let Err(e) = config.write() {
                eprintln!("{}", crate::messages::cli::save_config_failed(&e));
                std::process::exit(1);
            }
            println!(
                "{}",
                crate::messages::cli::removed_forward(&forward, &alias)
            );
            Ok(())
        }
        TunnelCommands::Start { alias } => {
            if !config.has_host(&alias) {
                eprintln!("{}", crate::messages::cli::host_not_found(&alias));
                std::process::exit(1);
            }
            let tunnels = config.find_tunnel_directives(&alias);
            if tunnels.is_empty() {
                eprintln!("{}", crate::messages::cli::no_forwards(&alias));
                std::process::exit(1);
            }
            println!("{}", crate::messages::cli::starting_tunnel(&alias));
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
pub(super) fn prompt_hidden_input(prompt: &str) -> Result<Option<String>> {
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
                crossterm::event::KeyCode::Backspace if input.pop().is_some() => {
                    eprint!("\x08 \x08");
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
pub(super) fn handle_password_command(command: PasswordCommands) -> Result<()> {
    match command {
        PasswordCommands::Set { alias } => {
            let password = match prompt_hidden_input(&format!("Password for {}: ", alias))? {
                Some(p) if !p.is_empty() => p,
                Some(_) => {
                    eprintln!("{}", crate::messages::cli::PASSWORD_EMPTY);
                    std::process::exit(1);
                }
                None => {
                    eprintln!("{}", crate::messages::cli::CANCELLED);
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
            println!("{}", crate::messages::cli::password_removed(&alias));
            Ok(())
        }
    }
}

pub(super) fn handle_snippet_command(
    config: SshConfigFile,
    command: SnippetCommands,
    config_path: &Path,
) -> Result<()> {
    match command {
        SnippetCommands::List => {
            let store = snippet::SnippetStore::load();
            if store.snippets.is_empty() {
                println!("{}", crate::messages::cli::NO_SNIPPETS);
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
                    eprintln!("{}", crate::messages::cli::DESCRIPTION_CONTROL_CHARS);
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
                println!("{}", crate::messages::cli::snippet_updated(&name));
            } else {
                println!("{}", crate::messages::cli::snippet_added(&name));
            }
            Ok(())
        }
        SnippetCommands::Remove { name } => {
            let mut store = snippet::SnippetStore::load();
            if store.get(&name).is_none() {
                eprintln!("{}", crate::messages::cli::snippet_not_found(&name));
                std::process::exit(1);
            }
            store.remove(&name);
            store.save()?;
            println!("{}", crate::messages::cli::snippet_removed(&name));
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
                    eprintln!("{}", crate::messages::cli::snippet_not_found(&name));
                    std::process::exit(1);
                }
            };

            let entries = config.host_entries();

            // Determine target hosts
            let targets: Vec<&HostEntry> = if let Some(ref alias) = alias {
                match entries.iter().find(|h| h.alias == *alias) {
                    Some(h) => vec![h],
                    None => {
                        eprintln!("{}", crate::messages::cli::host_not_found(alias));
                        std::process::exit(1);
                    }
                }
            } else if let Some(ref tag_filter) = tag {
                let matched: Vec<_> = entries
                    .iter()
                    .filter(|h| h.tags.iter().any(|t| t.eq_ignore_ascii_case(tag_filter)))
                    .collect();
                if matched.is_empty() {
                    eprintln!("{}", crate::messages::cli::no_hosts_with_tag(tag_filter));
                    std::process::exit(1);
                }
                matched
            } else if all {
                entries.iter().collect()
            } else {
                eprintln!("{}", crate::messages::cli::SPECIFY_TARGET);
                std::process::exit(1);
            };

            if targets.len() == 1 {
                // Single host: run directly
                let host = targets[0];
                let askpass = host
                    .askpass
                    .clone()
                    .or_else(preferences::load_askpass_default);
                let bw_session = super::ensure_bw_session(None, askpass.as_deref());
                super::ensure_keychain_password(&host.alias, askpass.as_deref());
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
                        eprintln!("{}", crate::messages::cli::operation_failed(&e));
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
                    super::ensure_bw_session(None, bw_askpass.as_deref())
                } else {
                    None
                };
                let targets_info: Vec<_> = targets
                    .iter()
                    .map(|h| {
                        let askpass = h.askpass.clone().or_else(preferences::load_askpass_default);
                        super::ensure_keychain_password(&h.alias, askpass.as_deref());
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
                            Err(e) => {
                                eprintln!("{}", crate::messages::cli::host_failed(&alias, &e))
                            }
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
                        super::ensure_bw_session(bw_session.as_deref(), askpass.as_deref())
                    {
                        bw_session = Some(token);
                    }
                    super::ensure_keychain_password(&host.alias, askpass.as_deref());
                    println!("{}", crate::messages::cli::host_separator(&host.alias));
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
                                eprintln!(
                                    "{}",
                                    crate::messages::cli::exited_with_code(
                                        r.status.code().unwrap_or(1)
                                    )
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!("{}", crate::messages::cli::host_failed(&host.alias, &e))
                        }
                    }
                    println!();
                }
            }
            Ok(())
        }
    }
}

pub(super) fn handle_logs_command(tail: bool, clear: bool) -> Result<()> {
    let path = logging::log_path().context("Could not determine log path")?;
    if clear {
        if path.exists() {
            std::fs::remove_file(&path)?;
            println!("{}", crate::messages::cli::log_deleted(&path.display()));
        } else {
            println!("{}", crate::messages::cli::no_log_file(&path.display()));
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
    Ok(())
}

pub(super) fn handle_theme_command(command: ThemeCommands) -> Result<()> {
    match command {
        ThemeCommands::List => {
            let current = preferences::load_theme().unwrap_or_else(|| "Purple".to_string());
            println!("{}", crate::messages::cli::BUILTIN_THEMES);
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
                println!("{}", crate::messages::cli::CUSTOM_THEMES);
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
                    println!("{}", crate::messages::cli::theme_set(&theme.name));
                }
                None => {
                    anyhow::bail!("Unknown theme: {}", name);
                }
            }
        }
    }
    Ok(())
}

pub(super) fn handle_vault_sign_command(
    mut config: SshConfigFile,
    alias: Option<String>,
    all: bool,
    cli_vault_addr: Option<String>,
) -> Result<()> {
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
                    println!("{}", crate::messages::cli::skipping_host(&entry.alias, &e));
                    failed += 1;
                    continue;
                }
            };
            let cert_path = vault_ssh::resolve_cert_path(&entry.alias, &entry.certificate_file)?;
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
                    println!("{}", crate::messages::cli::vault_sign_failed(&e));
                    failed += 1;
                }
            }
        }
        if signed > 0 {
            if let Err(e) = config.write() {
                eprintln!("{}", crate::messages::cli::vault_config_update_warning(&e));
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
        .with_context(|| crate::messages::cli::vault_no_role(&alias))?;

        let pubkey = vault_ssh::resolve_pubkey_path(&entry.identity_file)?;
        let resolved_addr = cli_vault_addr.clone().or_else(|| {
            vault_ssh::resolve_vault_addr(
                entry.vault_addr.as_deref(),
                entry.provider.as_deref(),
                &provider_config,
            )
        });
        let result = vault_ssh::sign_certificate(&role, &pubkey, &alias, resolved_addr.as_deref())?;
        // Honor the same invariant as the TUI paths: never overwrite a
        // user-set CertificateFile. Only write the directive (and the
        // SSH config) when the host has none yet.
        if should_write_certificate_file(&entry.certificate_file) {
            let updated =
                config.set_host_certificate_file(&alias, &result.cert_path.to_string_lossy());
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
        println!(
            "{}",
            crate::messages::cli::vault_cert_signed(&result.cert_path.display())
        );
    } else {
        anyhow::bail!("Provide a host alias or use --all");
    }
    Ok(())
}

pub(super) fn run_whats_new(since: Option<&str>) -> Result<String> {
    use crate::changelog::{self, EntryKind};
    use semver::Version;

    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .with_context(|| "failed to parse current version")?;
    let last = match since {
        Some(s) => Some(Version::parse(s).with_context(|| format!("invalid --since version {s}"))?),
        None => None,
    };

    let sections = changelog::cached();
    let shown = changelog::versions_to_show(sections, last.as_ref(), &current, sections.len());

    let mut out = String::new();
    out.push_str(crate::messages::cli::whats_new::HEADER);
    out.push_str("\n\n");
    for section in shown {
        out.push_str(&format!("## {}", section.version));
        if let Some(date) = &section.date {
            out.push_str(&format!(" - {}", date));
        }
        out.push('\n');
        for entry in &section.entries {
            let prefix = match entry.kind {
                EntryKind::Feature => "+ ",
                EntryKind::Change => "~ ",
                EntryKind::Fix => "! ",
            };
            out.push_str(prefix);
            out.push_str(&entry.text);
            out.push('\n');
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod whats_new_tests {
    use super::*;

    #[test]
    fn whats_new_cli_outputs_header() {
        let output = run_whats_new(None).unwrap();
        assert!(output.contains("purple release notes"));
    }

    #[test]
    fn whats_new_cli_filters_by_since() {
        let output = run_whats_new(Some("999.0.0")).unwrap();
        assert!(!output.contains("## "));
    }

    #[test]
    fn whats_new_cli_returns_error_on_bad_version() {
        let result = run_whats_new(Some("not-a-version"));
        assert!(result.is_err());
    }
}
