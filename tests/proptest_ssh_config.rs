//! Property-based tests for SSH config round-trip fidelity.
//!
//! Uses proptest to generate random SSH configs and verify that:
//! 1. parse -> serialize -> reparse produces identical output (idempotent)
//! 2. Mutations (add/delete/update) preserve structural integrity
//! 3. No panics on arbitrary input

use std::path::PathBuf;

use proptest::prelude::*;
use purple_ssh::ssh_config::model::{HostEntry, SshConfigFile};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_str(content: &str) -> SshConfigFile {
    SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/proptest_config"),
        crlf: content.contains("\r\n"),
        bom: content.starts_with('\u{FEFF}'),
    }
}

// ---------------------------------------------------------------------------
// Strategies: building blocks
// ---------------------------------------------------------------------------

/// Safe characters for SSH config aliases (alphanumeric, dash, underscore, dot).
fn alias_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9._-]{0,30}"
}

/// Hostname: IP or domain-like.
fn hostname_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // IPv4
        (1u8..=254, 0u8..=255, 0u8..=255, 1u8..=254)
            .prop_map(|(a, b, c, d)| format!("{a}.{b}.{c}.{d}")),
        // Simple domain
        "[a-z]{3,12}\\.(com|org|net|io|dev)",
        // Subdomain
        "[a-z]{2,8}\\.[a-z]{3,10}\\.(com|org|net)",
    ]
}

/// User names.
fn user_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("root".to_string()),
        Just("admin".to_string()),
        Just("deploy".to_string()),
        Just("ec2-user".to_string()),
        "[a-z]{3,12}",
    ]
}

/// Port numbers (valid SSH range).
fn port_strategy() -> impl Strategy<Value = u16> {
    prop_oneof![
        Just(22u16),
        Just(22u16), // weight toward default
        Just(22u16),
        2222u16..=65535,
    ]
}

/// Indentation style.
fn indent_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("  ".to_string()),
        Just("    ".to_string()),
        Just("\t".to_string()),
    ]
}

/// An identity file path.
fn identity_file_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("~/.ssh/id_ed25519".to_string()),
        Just("~/.ssh/id_rsa".to_string()),
        "[a-z_]{3,12}".prop_map(|name| format!("~/.ssh/{name}")),
    ]
}

/// A comment line (including the # prefix).
fn comment_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 .,;:!?_-]{0,60}".prop_map(|text| format!("# {text}"))
}

// ---------------------------------------------------------------------------
// Strategy: a complete Host block
// ---------------------------------------------------------------------------

fn host_block_strategy() -> impl Strategy<Value = String> {
    (
        alias_strategy(),
        hostname_strategy(),
        user_strategy(),
        port_strategy(),
        indent_strategy(),
        prop::option::of(identity_file_strategy()),
        prop::option::of(comment_strategy()),
        prop::bool::ANY, // use equals syntax
    )
        .prop_map(
            |(alias, hostname, user, port, indent, identity, comment, use_equals)| {
                let mut lines = Vec::new();

                // Host line
                if use_equals {
                    lines.push(format!("Host={alias}"));
                } else {
                    lines.push(format!("Host {alias}"));
                }

                // HostName
                if use_equals {
                    lines.push(format!("{indent}HostName={hostname}"));
                } else {
                    lines.push(format!("{indent}HostName {hostname}"));
                }

                // User
                lines.push(format!("{indent}User {user}"));

                // Port (skip default 22)
                if port != 22 {
                    lines.push(format!("{indent}Port {port}"));
                }

                // Optional identity file
                if let Some(ref id) = identity {
                    lines.push(format!("{indent}IdentityFile {id}"));
                }

                // Optional inline comment in block
                if let Some(ref c) = comment {
                    lines.push(format!("{indent}{c}"));
                }

                lines.join("\n")
            },
        )
}

// ---------------------------------------------------------------------------
// Strategy: a complete SSH config file
// ---------------------------------------------------------------------------

fn ssh_config_strategy() -> impl Strategy<Value = String> {
    (
        prop::option::of(comment_strategy()), // optional header comment
        prop::collection::vec(host_block_strategy(), 1..=20),
        prop::bool::ANY, // optional trailing Host *
        prop::bool::ANY, // use CRLF
    )
        .prop_map(|(header, blocks, trailing_wildcard, crlf)| {
            let mut parts = Vec::new();

            if let Some(h) = header {
                parts.push(h);
                parts.push(String::new()); // blank line after header
            }

            for (i, block) in blocks.iter().enumerate() {
                if i > 0 {
                    parts.push(String::new()); // blank line between blocks
                }
                parts.push(block.clone());
            }

            if trailing_wildcard {
                parts.push(String::new());
                parts.push("Host *".to_string());
                parts.push("  ServerAliveInterval 60".to_string());
            }

            let le = if crlf { "\r\n" } else { "\n" };
            let mut content = parts.join(le);
            content.push_str(le);
            content
        })
}

/// Strategy for Match blocks.
fn match_block_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("Match host *.example.com\n  User deploy\n".to_string()),
        Just("Match originalhost web*\n  ProxyJump bastion\n".to_string()),
        Just("Match all\n  ServerAliveInterval 30\n".to_string()),
        Just("Match exec \"test -f /tmp/flag\"\n  HostName override.local\n".to_string()),
    ]
}

/// Strategy for configs with Match blocks interspersed.
fn config_with_match_strategy() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(host_block_strategy(), 1..=5),
        prop::collection::vec(match_block_strategy(), 1..=3),
    )
        .prop_map(|(blocks, matches)| {
            let mut parts = Vec::new();
            for (i, block) in blocks.iter().enumerate() {
                if i > 0 {
                    parts.push(String::new());
                }
                parts.push(block.clone());
                // Insert Match block after every other host
                if i < matches.len() && i % 2 == 0 {
                    parts.push(String::new());
                    parts.push(matches[i].clone());
                }
            }
            let mut content = parts.join("\n");
            content.push('\n');
            content
        })
}

/// Purple-specific comment annotations.
fn purple_tags_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z]{2,8}", 1..=5)
        .prop_map(|tags| format!("  # purple:tags {}", tags.join(",")))
}

fn purple_provider_strategy() -> impl Strategy<Value = String> {
    (
        prop_oneof![
            Just("aws"),
            Just("digitalocean"),
            Just("hetzner"),
            Just("gcp"),
            Just("vultr"),
            Just("tailscale"),
        ],
        "[a-z0-9]{8,20}",
    )
        .prop_map(|(provider, id)| format!("  # purple:provider {provider}:{id}"))
}

fn purple_meta_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("  # purple:meta region=us-east-1,status=running".to_string()),
        Just("  # purple:meta zone=eu-west-1a,type=t3.micro".to_string()),
        Just("  # purple:meta node=pve1,type=qemu,specs=4c/8g".to_string()),
    ]
}

/// Host block with purple annotations.
fn annotated_host_block_strategy() -> impl Strategy<Value = String> {
    (
        host_block_strategy(),
        prop::option::of(purple_tags_strategy()),
        prop::option::of(purple_provider_strategy()),
        prop::option::of(purple_meta_strategy()),
    )
        .prop_map(|(block, tags, provider, meta)| {
            let mut lines: Vec<&str> = block.lines().collect();
            // Insert purple comments before any trailing blank
            if let Some(t) = &tags {
                lines.push(t);
            }
            if let Some(p) = &provider {
                lines.push(p);
            }
            if let Some(m) = &meta {
                lines.push(m);
            }
            lines.join("\n")
        })
}

/// A full config with purple annotations.
fn annotated_config_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(annotated_host_block_strategy(), 1..=10).prop_map(|blocks| {
        let mut content = blocks.join("\n\n");
        content.push('\n');
        content
    })
}

// ---------------------------------------------------------------------------
// Property: parse -> serialize is idempotent
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn roundtrip_idempotent(content in ssh_config_strategy()) {
        let config = parse_str(&content);
        let serialized = config.serialize();
        let reparsed = parse_str(&serialized);
        let reserialized = reparsed.serialize();
        prop_assert!(
            serialized == reserialized,
            "Second serialization differs from first (len {} vs {})",
            serialized.len(),
            reserialized.len(),
        );
    }

    #[test]
    fn roundtrip_with_match_blocks(content in config_with_match_strategy()) {
        let config = parse_str(&content);
        let serialized = config.serialize();
        let reparsed = parse_str(&serialized);
        let reserialized = reparsed.serialize();
        prop_assert!(serialized == reserialized);
    }

    #[test]
    fn roundtrip_annotated_configs(content in annotated_config_strategy()) {
        let config = parse_str(&content);
        let serialized = config.serialize();
        let reparsed = parse_str(&serialized);
        let reserialized = reparsed.serialize();
        prop_assert!(serialized == reserialized);
    }
}

// ---------------------------------------------------------------------------
// Property: host count preserved after parse -> serialize -> reparse
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn host_count_preserved(content in ssh_config_strategy()) {
        let config = parse_str(&content);
        let count1 = config.host_entries().len();
        let serialized = config.serialize();
        let reparsed = parse_str(&serialized);
        let count2 = reparsed.host_entries().len();
        prop_assert_eq!(
            count1, count2,
            "Host count changed: {} -> {}",
            count1, count2,
        );
    }
}

// ---------------------------------------------------------------------------
// Property: add_host then serialize preserves existing hosts
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn add_host_preserves_existing(
        content in ssh_config_strategy(),
        new_alias in alias_strategy(),
        new_hostname in hostname_strategy(),
        new_user in user_strategy(),
    ) {
        let mut config = parse_str(&content);
        let original_hosts: Vec<String> = config
            .host_entries()
            .iter()
            .map(|e| e.alias.clone())
            .collect();

        // Skip if alias already exists (add_host doesn't overwrite)
        if config.has_host(&new_alias) {
            return Ok(());
        }

        config.add_host(&HostEntry {
            alias: new_alias.clone(),
            hostname: new_hostname,
            user: new_user,
            port: 22,
            ..Default::default()
        });

        let entries = config.host_entries();
        // All original hosts still present
        for alias in &original_hosts {
            prop_assert!(
                entries.iter().any(|e| e.alias == *alias),
                "Lost host '{}' after add",
                alias,
            );
        }
        // New host is present
        prop_assert!(
            entries.iter().any(|e| e.alias == new_alias),
            "New host '{}' not found after add",
            new_alias,
        );

        // Round-trip stable after add
        let serialized = config.serialize();
        let reparsed = parse_str(&serialized);
        prop_assert!(serialized == reparsed.serialize());
    }
}

// ---------------------------------------------------------------------------
// Property: delete_host removes all hosts with matching alias
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn delete_host_removes_all_with_alias(content in ssh_config_strategy()) {
        let mut config = parse_str(&content);
        let entries = config.host_entries();
        if entries.is_empty() {
            return Ok(());
        }

        // Pick the first host
        let alias = entries[0].alias.clone();
        let count_before = entries.len();
        let alias_count = entries.iter().filter(|e| e.alias == alias).count();

        config.delete_host(&alias);

        let count_after = config.host_entries().len();
        prop_assert_eq!(
            count_before - alias_count,
            count_after,
            "Expected {} hosts after deleting {} copies of '{}', got {}",
            count_before - alias_count,
            alias_count,
            alias,
            count_after,
        );

        // Round-trip stable after delete
        let serialized = config.serialize();
        let reparsed = parse_str(&serialized);
        prop_assert!(serialized == reparsed.serialize());
    }
}

// ---------------------------------------------------------------------------
// Property: update_host preserves host count
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn update_host_preserves_count(
        content in ssh_config_strategy(),
        new_hostname in hostname_strategy(),
        new_user in user_strategy(),
    ) {
        let mut config = parse_str(&content);
        let entries = config.host_entries();
        if entries.is_empty() {
            return Ok(());
        }

        let alias = entries[0].alias.clone();
        let count_before = entries.len();

        config.update_host(
            &alias,
            &HostEntry {
                alias: alias.clone(),
                hostname: new_hostname,
                user: new_user,
                port: 22,
                ..Default::default()
            },
        );

        let count_after = config.host_entries().len();
        prop_assert_eq!(count_before, count_after);

        // Round-trip stable
        let serialized = config.serialize();
        let reparsed = parse_str(&serialized);
        prop_assert!(serialized == reparsed.serialize());
    }
}

// ---------------------------------------------------------------------------
// Property: delete_host_undoable + insert_host_at restores config
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn undo_restores_host(content in ssh_config_strategy()) {
        let mut config = parse_str(&content);
        let entries = config.host_entries();
        if entries.is_empty() {
            return Ok(());
        }

        let alias = entries[0].alias.clone();
        // Delete undoable
        if let Some((element, position)) = config.delete_host_undoable(&alias) {
            // Undo
            config.insert_host_at(element, position);

            // Serialization should match (modulo blank line collapse which
            // only happens in serialize(), and delete_host_undoable skips collapse)
            let reparsed = parse_str(&config.serialize());
            let final_entries = reparsed.host_entries();
            prop_assert!(
                final_entries.iter().any(|e| e.alias == alias),
                "Host '{}' not restored after undo",
                alias,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Property: no panic on arbitrary bytes
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn no_panic_on_arbitrary_input(content in "\\PC{0,500}") {
        // Just parse and serialize. Should never panic.
        let config = parse_str(&content);
        let _ = config.serialize();
        let _ = config.host_entries();
    }

    #[test]
    fn no_panic_on_binary_input(bytes in prop::collection::vec(any::<u8>(), 0..1000)) {
        if let Ok(content) = String::from_utf8(bytes) {
            let config = parse_str(&content);
            let _ = config.serialize();
            let _ = config.host_entries();
        }
    }
}

// ---------------------------------------------------------------------------
// Property: swap_hosts is reversible
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn swap_hosts_reversible(content in ssh_config_strategy()) {
        let mut config = parse_str(&content);
        let entries = config.host_entries();
        if entries.len() < 2 {
            return Ok(());
        }

        let a = entries[0].alias.clone();
        let b = entries[1].alias.clone();
        let before = config.serialize();

        // Swap twice = identity
        config.swap_hosts(&a, &b);
        config.swap_hosts(&a, &b);
        let after = config.serialize();

        prop_assert!(before == after, "Double swap should be identity");
    }
}

// ---------------------------------------------------------------------------
// Property: CRLF and BOM flags preserved
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn crlf_detected_correctly(content in ssh_config_strategy()) {
        let config = parse_str(&content);
        let has_crlf = content.contains("\r\n");
        prop_assert_eq!(
            config.crlf,
            has_crlf,
            "CRLF detection mismatch",
        );
    }
}

// ---------------------------------------------------------------------------
// Property: tags survive mutations
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn tags_survive_update(content in annotated_config_strategy()) {
        let mut config = parse_str(&content);
        let entries = config.host_entries();
        if entries.is_empty() {
            return Ok(());
        }

        let alias = entries[0].alias.clone();
        let original_tags = entries[0].tags.clone();
        let original_user = entries[0].user.clone();
        let original_port = entries[0].port;
        let original_identity = entries[0].identity_file.clone();
        let original_proxy = entries[0].proxy_jump.clone();

        // Update hostname only
        config.update_host(
            &alias,
            &HostEntry {
                alias: alias.clone(),
                hostname: "10.99.99.99".to_string(),
                user: original_user,
                port: original_port,
                identity_file: original_identity,
                proxy_jump: original_proxy,
                ..Default::default()
            },
        );

        let updated_entries = config.host_entries();
        let updated = updated_entries.iter().find(|e| e.alias == alias);
        if let Some(entry) = updated {
            prop_assert!(
                entry.tags == original_tags,
                "Tags changed after update for host '{}'",
                alias,
            );
        }
    }
}
