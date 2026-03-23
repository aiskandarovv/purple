#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;

fuzz_target!(|data: &[u8]| {
    // Only fuzz valid UTF-8 (SSH configs are text files).
    let Ok(content) = std::str::from_utf8(data) else {
        return;
    };

    // 1. Parse must not panic.
    let elements = purple_ssh::ssh_config::model::SshConfigFile::parse_content(content);

    let config = purple_ssh::ssh_config::model::SshConfigFile {
        elements,
        path: PathBuf::from("/tmp/fuzz_config"),
        crlf: content.contains("\r\n"),
        bom: content.starts_with('\u{FEFF}'),
    };

    // 2. Serialize must not panic.
    let serialized = config.serialize();

    // 3. host_entries must not panic.
    let _ = config.host_entries();

    // 4. Idempotency: serialize(parse(serialize(parse(input)))) == serialize(parse(input))
    let reparsed = purple_ssh::ssh_config::model::SshConfigFile {
        elements: purple_ssh::ssh_config::model::SshConfigFile::parse_content(&serialized),
        path: PathBuf::from("/tmp/fuzz_config"),
        crlf: serialized.contains("\r\n"),
        bom: serialized.starts_with('\u{FEFF}'),
    };
    let reserialized = reparsed.serialize();
    assert_eq!(
        serialized, reserialized,
        "Round-trip not idempotent for input of length {}",
        content.len()
    );

    // 5. Mutation smoke tests (must not panic).
    let entries = config.host_entries();
    if !entries.is_empty() {
        let alias = entries[0].alias.clone();

        // Delete
        let mut config_del = config.clone();
        config_del.delete_host(&alias);
        let _ = config_del.serialize();

        // Delete undoable + undo
        let mut config_undo = config.clone();
        if let Some((element, position)) = config_undo.delete_host_undoable(&alias) {
            config_undo.insert_host_at(element, position);
            let _ = config_undo.serialize();
        }

        // Update
        let mut config_upd = config.clone();
        config_upd.update_host(
            &alias,
            &purple_ssh::ssh_config::model::HostEntry {
                alias: alias.clone(),
                hostname: "10.0.0.1".to_string(),
                user: "fuzz".to_string(),
                port: 22,
                ..Default::default()
            },
        );
        let _ = config_upd.serialize();

        // Swap (if 2+ hosts)
        if entries.len() >= 2 {
            let mut config_swap = config.clone();
            config_swap.swap_hosts(&entries[0].alias, &entries[1].alias);
            let _ = config_swap.serialize();
        }
    }

    // 6. Add host
    let mut config_add = config.clone();
    config_add.add_host(&purple_ssh::ssh_config::model::HostEntry {
        alias: "fuzz-new-host".to_string(),
        hostname: "10.0.0.99".to_string(),
        user: "fuzzer".to_string(),
        port: 22,
        ..Default::default()
    });
    let _ = config_add.serialize();

    // 7. Tags, provider, askpass, meta must not panic.
    for entry in &entries {
        let _ = &entry.tags;
        let _ = &entry.provider;
        let _ = &entry.askpass;
        let _ = &entry.provider_meta;
    }
});
