//! OpenSSH ground-truth cross-validation.
//!
//! These tests use the real `ssh` binary as the authoritative oracle: for
//! any config purple ingests, the directives OpenSSH resolves via
//! `ssh -G <alias>` must be identical before and after purple parses and
//! re-serializes the file. If the interpretation ever changes, purple is
//! silently corrupting user configs and we want to know immediately.
//!
//! All tests skip cleanly when `/usr/bin/ssh` is not available so CI on
//! non-unix builders does not fail spuriously.

use purple_ssh::ssh_config::model::SshConfigFile;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn ssh_binary_available() -> bool {
    Command::new("ssh")
        .arg("-V")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty())
        .unwrap_or(false)
}

/// Run `ssh -F <config> -G <alias>` and parse the `key value` output into a
/// map. `ssh -G` emits ALL resolved options, including noisy defaults like
/// `compression`, `forwardx11`, `controlmaster`. Callers filter to the
/// subset they care about via [`key_fields`].
fn ssh_resolve(config_path: &Path, alias: &str) -> HashMap<String, String> {
    let output = Command::new("ssh")
        .arg("-F")
        .arg(config_path)
        .arg("-G")
        .arg(alias)
        .output()
        .expect("ssh -G must run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut map = HashMap::new();
    for line in stdout.lines() {
        if let Some((k, v)) = line.split_once(' ') {
            // Multi-valued options like `identityfile` may repeat. Collect
            // all values into a newline-joined string so equality is
            // preserved (order-sensitive, which is correct for ssh).
            map.entry(k.to_string())
                .and_modify(|existing: &mut String| {
                    existing.push('\n');
                    existing.push_str(v);
                })
                .or_insert_with(|| v.to_string());
        }
    }
    map
}

/// Subset of `ssh -G` keys that are user-specified and therefore sensitive
/// to parser/writer round-trip. Keys like `compression` default to `no`
/// regardless of config content so they are excluded.
const CARE_KEYS: &[&str] = &[
    "hostname",
    "user",
    "port",
    "identityfile",
    "proxyjump",
    "certificatefile",
    "proxycommand",
    "localforward",
    "remoteforward",
    "dynamicforward",
    "forwardagent",
    "serveraliveinterval",
    "serveralivecountmax",
    "connecttimeout",
    "preferredauthentications",
    "pubkeyauthentication",
    "stricthostkeychecking",
    "userknownhostsfile",
    "controlmaster",
    "controlpath",
    "controlpersist",
    "addkeystoagent",
    "canonicalizehostname",
    "canonicaldomains",
];

fn key_fields(map: &HashMap<String, String>) -> HashMap<String, String> {
    CARE_KEYS
        .iter()
        .filter_map(|k| map.get(*k).map(|v| ((*k).to_string(), v.clone())))
        .collect()
}

fn write_temp(content: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config");
    std::fs::write(&path, content).expect("write config");
    (dir, path)
}

/// Parse content through purple and re-serialize. This is the operation
/// whose semantic idempotence we want to verify against OpenSSH.
fn roundtrip(content: &str) -> String {
    let elements = SshConfigFile::parse_content(content);
    let config = SshConfigFile {
        elements,
        path: PathBuf::from("/tmp/test"),
        crlf: content.contains("\r\n"),
        bom: content.starts_with('\u{FEFF}'),
    };
    config.serialize()
}

/// Curated corpus of configs plus aliases to probe with `ssh -G`. Covers
/// the parsing surfaces purple touches: multi-alias blocks, `=` separators,
/// tabs, CRLF, inline comments, pattern inheritance, negation, ProxyJump,
/// tilde expansion, wildcard defaults, and multi-valued directives.
fn corpus() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "Host web\n  HostName 10.0.0.1\n  User alice\n  Port 2222\n",
            vec!["web"],
        ),
        (
            "Host web-01 web-01.prod 10.0.1.5\n  HostName 10.0.1.5\n  User deploy\n",
            vec!["web-01", "web-01.prod", "10.0.1.5"],
        ),
        (
            "Host *.prod\n  User prod-admin\n  IdentityFile ~/.ssh/prod_key\n\nHost foo.prod\n  HostName 1.2.3.4\n",
            vec!["foo.prod", "bar.prod"],
        ),
        (
            "# header\n\n# section\nHost gamma\n  HostName 2.2.2.2\n  User gu\n",
            vec!["gamma"],
        ),
        (
            "Host eq\n  HostName=1.2.3.4\n  User=bob\n  Port=443\n",
            vec!["eq"],
        ),
        (
            "Host tab\n\tHostName\t3.3.3.3\n\tUser\tcarol\n\tPort\t8022\n",
            vec!["tab"],
        ),
        (
            "Host crlf\r\n  HostName 4.4.4.4\r\n  User dave\r\n  Port 2200\r\n",
            vec!["crlf"],
        ),
        (
            "Host bastion\n  HostName 1.1.1.1\n  User jump\n\nHost inside\n  HostName 10.0.0.2\n  ProxyJump bastion\n",
            vec!["inside"],
        ),
        (
            "Host tilde\n  HostName 5.5.5.5\n  IdentityFile ~/.ssh/id_rsa\n",
            vec!["tilde"],
        ),
        (
            "Host multikey\n  HostName 6.6.6.6\n  IdentityFile ~/.ssh/a\n  IdentityFile ~/.ssh/b\n",
            vec!["multikey"],
        ),
        (
            "# top\nHost mix\n  # inline\n  HostName 7.7.7.7 # trailing comment\n  User eve\n  Port 7777\n",
            vec!["mix"],
        ),
        (
            "Host *\n  ServerAliveInterval 60\n  User defaultu\n\nHost specific\n  HostName 8.8.8.8\n",
            vec!["specific", "anythinghere"],
        ),
        (
            "Host fwd\n  HostName 9.9.9.9\n  LocalForward 8080 localhost:80\n  RemoteForward 9090 localhost:90\n",
            vec!["fwd"],
        ),
        (
            "Host quoted\n  HostName \"10.10.10.10\"\n  User \"quoted-user\"\n",
            vec!["quoted"],
        ),
        (
            "Host !admin.prod *.prod\n  User worker\n\nHost admin.prod\n  User root\n  HostName adm.host\n",
            vec!["app.prod", "admin.prod"],
        ),
        (
            "Host stream-a stream-b\n  HostName stream.host\n  User streamer\n  IdentityFile ~/.ssh/stream\n",
            vec!["stream-a", "stream-b"],
        ),
    ]
}

/// Top-level guarantee: parse(content) serialized back must be interpreted
/// identically by OpenSSH for every alias we resolve. This is the
/// strongest round-trip fidelity check purple has because it uses the
/// real reference implementation of SSH as the judge.
#[test]
fn openssh_semantic_roundtrip_is_identity() {
    if !ssh_binary_available() {
        eprintln!("skipping: ssh binary unavailable");
        return;
    }
    for (idx, (content, aliases)) in corpus().into_iter().enumerate() {
        let roundtripped = roundtrip(content);
        let (_orig_dir, orig_path) = write_temp(content);
        let (_rt_dir, rt_path) = write_temp(&roundtripped);
        for alias in aliases {
            let orig = key_fields(&ssh_resolve(&orig_path, alias));
            let rt = key_fields(&ssh_resolve(&rt_path, alias));
            assert_eq!(
                orig, rt,
                "OpenSSH interpretation changed after purple round-trip.\n\
                 Corpus index: {idx}\nAlias: {alias}\n\n--- Original ---\n{content}\n\
                 --- Roundtripped ---\n{roundtripped}\n\n--- Original -G subset ---\n{orig:#?}\n\
                 --- Roundtripped -G subset ---\n{rt:#?}"
            );
        }
    }
}

/// Double round-trip: parse → serialize → parse → serialize. OpenSSH must
/// interpret both serialization rounds identically. Catches any drift that
/// only manifests on second application of the serializer.
#[test]
fn openssh_double_roundtrip_is_stable() {
    if !ssh_binary_available() {
        eprintln!("skipping: ssh binary unavailable");
        return;
    }
    for (idx, (content, aliases)) in corpus().into_iter().enumerate() {
        let once = roundtrip(content);
        let twice = roundtrip(&once);
        let (_d1, p1) = write_temp(&once);
        let (_d2, p2) = write_temp(&twice);
        for alias in aliases {
            let a = key_fields(&ssh_resolve(&p1, alias));
            let b = key_fields(&ssh_resolve(&p2, alias));
            assert_eq!(
                a, b,
                "Double round-trip drift at corpus[{idx}] alias={alias}:\n\
                 once:\n{once}\ntwice:\n{twice}"
            );
        }
    }
}

/// For single-alias non-inheriting hosts, purple's `host_entries()`
/// extraction must agree with `ssh -G` on the core directives. This
/// validates that the `HostEntry` convenience view is a faithful
/// projection of what SSH would actually use.
#[test]
fn purple_host_entries_match_openssh_core_directives() {
    if !ssh_binary_available() {
        eprintln!("skipping: ssh binary unavailable");
        return;
    }
    let cases: &[(&str, &str)] = &[
        ("Host a\n  HostName 1.1.1.1\n  User u1\n  Port 10\n", "a"),
        ("Host b\n  HostName=2.2.2.2\n  User=u2\n", "b"),
        (
            "Host c\n\tHostName\t3.3.3.3\n\tUser\tu3\n\tIdentityFile\t~/.ssh/key\n",
            "c",
        ),
        ("Host d\n  HostName 4.4.4.4\n  ProxyJump bastion\n", "d"),
    ];
    for (content, alias) in cases {
        let (_dir, path) = write_temp(content);
        let ssh_map = ssh_resolve(&path, alias);

        let elements = SshConfigFile::parse_content(content);
        let config = SshConfigFile {
            elements,
            path: PathBuf::from("/tmp/t"),
            crlf: false,
            bom: false,
        };
        let entries = config.host_entries();
        let entry = entries
            .iter()
            .find(|e| e.alias == *alias)
            .unwrap_or_else(|| panic!("alias {alias} not found: {content}"));

        // `ssh -G` fills in defaults that purple's HostEntry leaves empty
        // (User defaults to the current OS user, HostName defaults to the
        // alias). We therefore only assert equality when the field was
        // explicitly set in the source config — which is the only
        // contract purple's extractor actually owns.
        let ssh_hostname = ssh_map.get("hostname").cloned().unwrap_or_default();
        let ssh_user = ssh_map.get("user").cloned().unwrap_or_default();
        let ssh_port: u16 = ssh_map
            .get("port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(22);

        if !entry.hostname.is_empty() {
            assert_eq!(entry.hostname, ssh_hostname, "hostname for {alias}");
        }
        if !entry.user.is_empty() {
            assert_eq!(entry.user, ssh_user, "user for {alias}");
        }
        assert_eq!(entry.port, ssh_port, "port for {alias}");
    }
}

/// Pattern inheritance cross-validation: purple applies
/// `apply_pattern_inheritance` on top of `host_entries()` to merge User,
/// ProxyJump and IdentityFile from matching wildcard `Host *.foo` /
/// `Host *` blocks. Whatever purple fills into the `HostEntry` must
/// agree with what OpenSSH itself resolves for the same alias — otherwise
/// indicators like `↗` (proxy present) and any command derived from
/// `HostEntry` diverge from how `ssh <alias>` will actually behave.
///
/// The test uses configs where the inherited field IS explicitly set by
/// a pattern (so `ssh -G` and purple agree on a non-default value). It
/// compares `user` and `proxyjump` exactly. For `identityfile` it only
/// asserts that purple's resolved path appears somewhere in `ssh -G`'s
/// identity-file list, because OpenSSH always emits a long default set
/// (id_rsa, id_ed25519, id_ecdsa, ...) regardless of config content.
#[test]
fn purple_pattern_inheritance_matches_openssh() {
    if !ssh_binary_available() {
        eprintln!("skipping: ssh binary unavailable");
        return;
    }

    // Each case: (config content, alias to probe).
    let cases: &[(&str, &str)] = &[
        // User inherited from Host *.prod
        (
            "Host *.prod\n  User prod-admin\n\nHost web.prod\n  HostName 1.1.1.1\n",
            "web.prod",
        ),
        // User inherited from global Host *
        (
            "Host *\n  User global-u\n\nHost web\n  HostName 2.2.2.2\n",
            "web",
        ),
        // ProxyJump inherited from pattern
        (
            "Host bastion\n  HostName 10.0.0.1\n  User jumper\n\nHost *.internal\n  ProxyJump bastion\n\nHost db.internal\n  HostName 10.0.0.2\n  User dba\n",
            "db.internal",
        ),
        // IdentityFile inherited from pattern
        (
            "Host *.staging\n  IdentityFile ~/.ssh/staging_key\n\nHost app.staging\n  HostName 3.3.3.3\n  User app\n",
            "app.staging",
        ),
        // Multiple patterns: first-match semantics across pattern blocks
        // when the host itself has no explicit User value.
        (
            "Host *.east.prod\n  User east-u\n\nHost *.prod\n  User general-u\n\nHost app.east.prod\n  HostName 5.5.5.5\n",
            "app.east.prod",
        ),
        // Specific host BEFORE wildcard pattern (the canonical ordering
        // purple uses when adding hosts: specifics first, wildcards last).
        // Both ssh and purple agree: host-specific wins.
        (
            "Host override.prod\n  HostName 4.4.4.4\n  User specific-user\n\nHost *.prod\n  User pattern-user\n",
            "override.prod",
        ),
        // KNOWN DIVERGENCE (documented, not exercised here): when a
        // wildcard pattern block appears BEFORE a host-specific block in
        // file order, OpenSSH applies "first-match wins" top-to-bottom and
        // uses the pattern value, while purple's `host_entries()` returns
        // the host-specific value. Purple's `add_host` always inserts
        // specific hosts before trailing patterns (see
        // `find_trailing_pattern_start`), so the canonical purple-managed
        // config cannot reach this corner. Hand-edited configs with
        // patterns-before-specifics will see the divergence in the TUI
        // view only; `ssh` itself still uses the pattern value at connect
        // time. Fixing this requires changing `apply_pattern_inheritance`
        // to true first-match-wins semantics — deliberately out of scope
        // for the multi-alias data-integrity fix.
    ];

    for (content, alias) in cases {
        let (_dir, path) = write_temp(content);
        let ssh_map = ssh_resolve(&path, alias);

        let elements = SshConfigFile::parse_content(content);
        let config = SshConfigFile {
            elements,
            path: PathBuf::from("/tmp/t"),
            crlf: false,
            bom: false,
        };
        let entries = config.host_entries();
        let entry = entries
            .iter()
            .find(|e| e.alias == *alias)
            .unwrap_or_else(|| {
                panic!("alias {alias} missing from host_entries for config:\n{content}")
            });

        let ssh_user = ssh_map.get("user").cloned().unwrap_or_default();
        let ssh_proxy = ssh_map.get("proxyjump").cloned().unwrap_or_default();
        let ssh_identity = ssh_map.get("identityfile").cloned().unwrap_or_default();

        // User: purple's inherited value must match ssh's resolved user.
        // We only compare when purple filled a non-empty value (purple's
        // contract is that it reflects what the config explicitly says;
        // an empty value means "use ssh's default" and ssh -G's default
        // is $USER which is environment-dependent).
        if !entry.user.is_empty() {
            assert_eq!(
                entry.user, ssh_user,
                "pattern-inherited user diverged for {alias}:\n{content}"
            );
        }

        // ProxyJump: exact match when purple has a value (ssh -G prints
        // empty string when no ProxyJump is configured; a non-empty purple
        // value with no ssh resolution would mean purple hallucinated a
        // pattern match).
        if !entry.proxy_jump.is_empty() {
            assert_eq!(
                entry.proxy_jump, ssh_proxy,
                "pattern-inherited proxy_jump diverged for {alias}:\n{content}"
            );
        }

        // IdentityFile: purple stores a single resolved path; ssh -G emits
        // a newline-joined list that includes the user-configured path
        // followed by ssh's defaults. We verify purple's path appears in
        // that list so purple's ↗/cert indicators and command builders
        // point at the same key ssh will actually use.
        if !entry.identity_file.is_empty() {
            // Tilde expansion: ssh -G expands `~` to the runtime home, so
            // compare on the basename plus a containment check against
            // the full list.
            let needle_basename = entry
                .identity_file
                .rsplit('/')
                .next()
                .unwrap_or(&entry.identity_file);
            assert!(
                ssh_identity.contains(needle_basename),
                "pattern-inherited identity_file {} not found in ssh -G list for {alias}:\n{ssh_identity}\n\nconfig:\n{content}",
                entry.identity_file
            );
        }
    }
}

/// Every file in `fuzz/seed_corpus/fuzz_ssh_config/` represents a
/// hand-crafted edge case that the fuzzer uses as a starting point. Each
/// must survive purple's round-trip without changing how OpenSSH would
/// resolve any of the aliases present. `ssh -G` on a missing alias still
/// exercises the parser (it reads the whole file) so we use a probe alias
/// rather than trying to enumerate every defined host.
#[test]
fn openssh_roundtrip_on_fuzz_seed_corpus() {
    if !ssh_binary_available() {
        eprintln!("skipping: ssh binary unavailable");
        return;
    }
    let corpus_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/seed_corpus/fuzz_ssh_config");
    let Ok(entries) = std::fs::read_dir(&corpus_dir) else {
        eprintln!(
            "skipping: fuzz seed corpus not found at {}",
            corpus_dir.display()
        );
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("conf") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let roundtripped = roundtrip(&content);
        let (_d1, p1) = write_temp(&content);
        let (_d2, p2) = write_temp(&roundtripped);
        // Use a probe alias — whether or not it matches a defined host,
        // ssh -G has to walk the entire file, which exercises parsing of
        // every directive in the corpus file.
        let probe = "ground-truth-probe-alias";
        let orig = key_fields(&ssh_resolve(&p1, probe));
        let rt = key_fields(&ssh_resolve(&p2, probe));
        assert_eq!(
            orig,
            rt,
            "OpenSSH interpretation changed after round-trip for fuzz seed {}",
            path.display()
        );
    }
}
