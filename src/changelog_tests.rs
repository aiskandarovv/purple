use super::*;
use semver::Version;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/changelog/{}", name)).unwrap()
}

#[test]
fn parses_simple_changelog() {
    let sections = parse(&fixture("simple.md"));
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].version, Version::parse("2.41.0").unwrap());
    assert_eq!(sections[0].date.as_deref(), Some("2026-04-15"));
    assert_eq!(sections[0].entries.len(), 2);
    assert_eq!(sections[1].version, Version::parse("2.40.0").unwrap());
    assert_eq!(sections[1].date, None);
}

#[test]
fn classifier_uses_strict_prefixes_only() {
    let sections = parse(&fixture("mixed_prefixes.md"));
    let kinds: Vec<EntryKind> = sections[0].entries.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![
            EntryKind::Feature,
            EntryKind::Fix,
            EntryKind::Change,
            EntryKind::Change,
        ]
    );
    assert_eq!(sections[0].entries[0].text, "command palette");
    assert_eq!(sections[0].entries[1].text, "resolve crash on resize");
    assert_eq!(sections[0].entries[2].text, "default sort to alpha");
    assert_eq!(
        sections[0].entries[3].text,
        "bullet without prefix becomes change"
    );
}

#[test]
fn handles_unicode() {
    let sections = parse(&fixture("unicode.md"));
    assert_eq!(sections[0].entries.len(), 2);
    assert!(sections[0].entries[0].text.contains("für"));
    assert!(sections[0].entries[1].text.contains("日本語"));
}

#[test]
fn skips_malformed_headers_and_keeps_dates() {
    let sections = parse(&fixture("malformed.md"));
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].version, Version::parse("3.0.0").unwrap());
    assert_eq!(sections[0].date.as_deref(), Some("2026-01-01"));
}

#[test]
fn drops_empty_sections() {
    let sections = parse(&fixture("empty_section.md"));
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].version, Version::parse("0.9.0").unwrap());
}

#[test]
fn case_insensitive_prefix_match() {
    let sections = parse("## 1.0.0\n- Feat: capitalised\n- FIX: upper\n");
    let kinds: Vec<EntryKind> = sections[0].entries.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, vec![EntryKind::Feature, EntryKind::Fix]);
}

#[test]
fn versions_to_show_returns_empty_when_equal() {
    let sections = vec![section("2.0.0"), section("1.0.0")];
    let v = Version::parse("2.0.0").unwrap();
    assert!(versions_to_show(&sections, Some(&v), &v, 5).is_empty());
}

#[test]
fn versions_to_show_returns_intermediate_only() {
    let sections = vec![
        section("3.0.0"),
        section("2.5.0"),
        section("2.0.0"),
        section("1.0.0"),
    ];
    let last = Version::parse("2.0.0").unwrap();
    let cur = Version::parse("3.0.0").unwrap();
    let shown = versions_to_show(&sections, Some(&last), &cur, 5);
    let versions: Vec<String> = shown.iter().map(|s| s.version.to_string()).collect();
    assert_eq!(versions, vec!["3.0.0", "2.5.0"]);
}

#[test]
fn versions_to_show_caps_at_limit() {
    let sections: Vec<_> = (1..=10)
        .rev()
        .map(|n| section(&format!("{}.0.0", n)))
        .collect();
    let cur = Version::parse("10.0.0").unwrap();
    let shown = versions_to_show(&sections, Some(&Version::parse("0.0.1").unwrap()), &cur, 5);
    assert_eq!(shown.len(), 5);
}

#[test]
fn versions_to_show_handles_short_changelog_without_panic() {
    let sections = vec![section("2.0.0")];
    let cur = Version::parse("2.0.0").unwrap();
    let shown = versions_to_show(&sections, None, &cur, 5);
    assert_eq!(shown.len(), 1);
}

#[test]
fn versions_to_show_returns_last_n_when_no_last_seen() {
    let sections: Vec<_> = (1..=10)
        .rev()
        .map(|n| section(&format!("{}.0.0", n)))
        .collect();
    let cur = Version::parse("10.0.0").unwrap();
    let shown = versions_to_show(&sections, None, &cur, 5);
    assert_eq!(shown.len(), 5);
}

#[test]
fn versions_to_show_empty_on_downgrade() {
    let sections = vec![section("2.0.0"), section("1.0.0")];
    let last = Version::parse("3.0.0").unwrap();
    let cur = Version::parse("2.0.0").unwrap();
    assert!(versions_to_show(&sections, Some(&last), &cur, 5).is_empty());
}

#[test]
fn versions_to_show_skips_pre_release_below_stable() {
    let sections = parse("## 2.42.0\n- feat: x\n\n## 2.42.0-rc.1\n- feat: rc\n");
    let cur = Version::parse("2.42.0").unwrap();
    let last = Version::parse("2.42.0-rc.1").unwrap();
    let shown = versions_to_show(&sections, Some(&last), &cur, 5);
    let versions: Vec<String> = shown.iter().map(|s| s.version.to_string()).collect();
    assert_eq!(versions, vec!["2.42.0"]);
}

#[test]
fn cached_parses_only_once() {
    let first = cached() as *const Vec<Section>;
    let second = cached() as *const Vec<Section>;
    assert_eq!(first, second, "OnceLock must return the same allocation");
}

fn section(v: &str) -> Section {
    Section {
        version: Version::parse(v).unwrap(),
        date: None,
        entries: vec![Entry {
            kind: EntryKind::Change,
            text: "x".into(),
        }],
    }
}
