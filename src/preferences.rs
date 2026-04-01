use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::app::{SortMode, ViewMode};
use crate::fs_util;

static PATH_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Override the preferences file path (used in tests to avoid writing to ~/.purple).
#[cfg(test)]
pub fn set_path_override(path: PathBuf) {
    *PATH_OVERRIDE.lock().unwrap() = Some(path);
}

fn path() -> Option<PathBuf> {
    if let Some(p) = PATH_OVERRIDE.lock().unwrap().clone() {
        return Some(p);
    }
    dirs::home_dir().map(|h| h.join(".purple/preferences"))
}

/// Load a value for a given key from ~/.purple/preferences.
fn load_value(key: &str) -> Option<String> {
    let path = path()?;
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Save a key=value pair to ~/.purple/preferences. Preserves unknown keys and comments.
fn save_value(key: &str, value: &str) -> io::Result<()> {
    let path = match path() {
        Some(p) => p,
        None => return Ok(()),
    };

    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    let mut found = false;

    for line in existing.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('#')
            && !trimmed.is_empty()
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key)
        {
            lines.push(format!("{}={}", key, value));
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !found {
        lines.push(format!("{}={}", key, value));
    }

    let content = lines.join("\n") + "\n";

    fs_util::atomic_write(&path, content.as_bytes())
}

/// Load sort mode from ~/.purple/preferences. Returns MostRecent if missing or invalid.
pub fn load_sort_mode() -> SortMode {
    load_value("sort_mode")
        .map(|v| SortMode::from_key(&v))
        .unwrap_or(SortMode::MostRecent)
}

/// Save sort mode to ~/.purple/preferences.
pub fn save_sort_mode(mode: SortMode) -> io::Result<()> {
    save_value("sort_mode", mode.to_key())
}

/// Load group_by from ~/.purple/preferences. New `group_by` key takes precedence
/// over the legacy `group_by_provider` key for backward compatibility.
/// Returns `GroupBy::Provider` if missing (preserving old default behavior).
pub fn load_group_by() -> crate::app::GroupBy {
    use crate::app::GroupBy;
    if let Some(v) = load_value("group_by") {
        return GroupBy::from_key(&v);
    }
    if let Some(v) = load_value("group_by_provider") {
        return if v == "true" {
            GroupBy::Provider
        } else {
            GroupBy::None
        };
    }
    GroupBy::Provider
}

/// Remove a key from ~/.purple/preferences. No-op if the key or file does not exist.
fn remove_value(key: &str) -> io::Result<()> {
    let path = match path() {
        Some(p) => p,
        None => return Ok(()),
    };
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    // Early return if key not present — avoids unnecessary rewrite
    let has_key = existing.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.starts_with('#')
            && !trimmed.is_empty()
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key)
    });
    if !has_key {
        return Ok(());
    }

    let lines: Vec<String> = existing
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                return true;
            }
            trimmed.split_once('=').is_none_or(|(k, _)| k.trim() != key)
        })
        .map(|l| l.to_string())
        .collect();
    let content = lines.join("\n") + "\n";
    fs_util::atomic_write(&path, content.as_bytes())
}

/// Save group_by to ~/.purple/preferences.
pub fn save_group_by(mode: &crate::app::GroupBy) -> io::Result<()> {
    save_value("group_by", &mode.to_key())?;
    // Best-effort cleanup: group_by key takes precedence on load, so
    // a leftover group_by_provider key is harmless if removal fails.
    let _ = remove_value("group_by_provider");
    Ok(())
}

/// Load view mode from ~/.purple/preferences. Returns Detailed if missing or invalid.
pub fn load_view_mode() -> ViewMode {
    load_value("view_mode")
        .map(|v| match v.as_str() {
            "compact" => ViewMode::Compact,
            _ => ViewMode::Detailed,
        })
        .unwrap_or(ViewMode::Detailed)
}

/// Save view mode to ~/.purple/preferences.
pub fn save_view_mode(mode: ViewMode) -> io::Result<()> {
    save_value(
        "view_mode",
        match mode {
            ViewMode::Compact => "compact",
            ViewMode::Detailed => "detailed",
        },
    )
}

/// Load collapsed groups from ~/.purple/preferences.
/// Returns a set of group header strings that were collapsed.
/// Uses unit separator (U+001F) as delimiter to avoid conflicts with commas in tag names.
pub fn load_collapsed_groups() -> std::collections::HashSet<String> {
    load_value("collapsed_groups")
        .filter(|v| !v.is_empty())
        .map(|v| v.split('\x1f').map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

/// Save collapsed groups to ~/.purple/preferences.
/// Uses unit separator (U+001F) as delimiter to avoid conflicts with commas in tag names.
pub fn save_collapsed_groups(groups: &std::collections::HashSet<String>) -> io::Result<()> {
    if groups.is_empty() {
        remove_value("collapsed_groups")
    } else {
        let mut sorted: Vec<&str> = groups.iter().map(|s| s.as_str()).collect();
        sorted.sort_unstable();
        save_value("collapsed_groups", &sorted.join("\x1f"))
    }
}

/// Load global askpass default from ~/.purple/preferences.
pub fn load_askpass_default() -> Option<String> {
    load_value("askpass").filter(|v| !v.is_empty())
}

/// Save global askpass default to ~/.purple/preferences.
pub fn save_askpass_default(source: &str) -> io::Result<()> {
    save_value("askpass", source)
}

#[cfg(test)]
mod tests {
    use super::*;

    // We test load_value/save_value logic by replicating the parsing inline,
    // since the real functions read from ~/.purple/preferences.

    fn parse_value(content: &str, key: &str) -> Option<String> {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim().to_string());
                }
            }
        }
        None
    }

    #[test]
    fn load_askpass_returns_value() {
        let content = "askpass=keychain\n";
        let val = parse_value(content, "askpass").filter(|v| !v.is_empty());
        assert_eq!(val, Some("keychain".to_string()));
    }

    #[test]
    fn load_askpass_returns_none_for_empty() {
        let content = "askpass=\n";
        let val = parse_value(content, "askpass").filter(|v| !v.is_empty());
        assert_eq!(val, None);
    }

    #[test]
    fn load_askpass_returns_none_when_missing() {
        let content = "sort_mode=alpha\n";
        let val = parse_value(content, "askpass").filter(|v| !v.is_empty());
        assert_eq!(val, None);
    }

    #[test]
    fn load_askpass_preserves_vault_uri() {
        let content = "askpass=vault:secret/ssh#password\n";
        let val = parse_value(content, "askpass").filter(|v| !v.is_empty());
        assert_eq!(val, Some("vault:secret/ssh#password".to_string()));
    }

    #[test]
    fn load_askpass_preserves_op_uri() {
        let content = "askpass=op://Vault/SSH/password\n";
        let val = parse_value(content, "askpass").filter(|v| !v.is_empty());
        assert_eq!(val, Some("op://Vault/SSH/password".to_string()));
    }

    #[test]
    fn load_askpass_among_other_prefs() {
        let content = "sort_mode=alpha\ngroup_by_provider=true\naskpass=bw:my-item\n";
        let val = parse_value(content, "askpass").filter(|v| !v.is_empty());
        assert_eq!(val, Some("bw:my-item".to_string()));
    }

    #[test]
    fn save_value_builds_correct_line() {
        // Verify the format that save_value produces
        let key = "askpass";
        let value = "keychain";
        let line = format!("{}={}", key, value);
        assert_eq!(line, "askpass=keychain");
    }

    #[test]
    fn save_value_replaces_existing() {
        // Simulate save_value logic
        let existing = "sort_mode=alpha\naskpass=old\n";
        let key = "askpass";
        let new_value = "vault:secret/ssh";

        let mut lines: Vec<String> = Vec::new();
        let mut found = false;
        for line in existing.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with('#')
                && !trimmed.is_empty()
                && trimmed
                    .split_once('=')
                    .is_some_and(|(k, _)| k.trim() == key)
            {
                lines.push(format!("{}={}", key, new_value));
                found = true;
            } else {
                lines.push(line.to_string());
            }
        }
        if !found {
            lines.push(format!("{}={}", key, new_value));
        }
        let content = lines.join("\n") + "\n";
        assert!(content.contains("askpass=vault:secret/ssh"));
        assert!(!content.contains("askpass=old"));
        assert!(content.contains("sort_mode=alpha"));
        assert!(found);
    }

    #[test]
    fn load_group_by_new_key_none() {
        let content = "group_by=none\n";
        let val = parse_value(content, "group_by").unwrap_or_default();
        assert_eq!(
            crate::app::GroupBy::from_key(&val),
            crate::app::GroupBy::None
        );
    }

    #[test]
    fn load_group_by_new_key_provider() {
        let content = "group_by=provider\n";
        let val = parse_value(content, "group_by").unwrap_or_default();
        assert_eq!(
            crate::app::GroupBy::from_key(&val),
            crate::app::GroupBy::Provider
        );
    }

    #[test]
    fn load_group_by_new_key_tag() {
        let content = "group_by=tag:production\n";
        let val = parse_value(content, "group_by").unwrap_or_default();
        assert_eq!(
            crate::app::GroupBy::from_key(&val),
            crate::app::GroupBy::Tag("production".to_string())
        );
    }

    #[test]
    fn load_group_by_backward_compat_true() {
        let content = "group_by_provider=true\n";
        let new_val = parse_value(content, "group_by");
        let old_val = parse_value(content, "group_by_provider");
        let result = if let Some(v) = new_val {
            crate::app::GroupBy::from_key(&v)
        } else if let Some(v) = old_val {
            if v == "true" {
                crate::app::GroupBy::Provider
            } else {
                crate::app::GroupBy::None
            }
        } else {
            crate::app::GroupBy::None
        };
        assert_eq!(result, crate::app::GroupBy::Provider);
    }

    #[test]
    fn load_group_by_backward_compat_false() {
        let content = "group_by_provider=false\n";
        let new_val = parse_value(content, "group_by");
        let old_val = parse_value(content, "group_by_provider");
        let result = if let Some(v) = new_val {
            crate::app::GroupBy::from_key(&v)
        } else if let Some(v) = old_val {
            if v == "true" {
                crate::app::GroupBy::Provider
            } else {
                crate::app::GroupBy::None
            }
        } else {
            crate::app::GroupBy::None
        };
        assert_eq!(result, crate::app::GroupBy::None);
    }

    #[test]
    fn load_group_by_new_key_overrides_old() {
        let content = "group_by_provider=true\ngroup_by=tag:staging\n";
        let new_val = parse_value(content, "group_by");
        let old_val = parse_value(content, "group_by_provider");
        let result = if let Some(v) = new_val {
            crate::app::GroupBy::from_key(&v)
        } else if let Some(v) = old_val {
            if v == "true" {
                crate::app::GroupBy::Provider
            } else {
                crate::app::GroupBy::None
            }
        } else {
            crate::app::GroupBy::None
        };
        assert_eq!(result, crate::app::GroupBy::Tag("staging".to_string()));
    }

    #[test]
    fn load_group_by_missing_defaults_to_provider() {
        let content = "sort_mode=alpha\n";
        let new_val = parse_value(content, "group_by");
        let old_val = parse_value(content, "group_by_provider");
        let result = if let Some(v) = new_val {
            crate::app::GroupBy::from_key(&v)
        } else if let Some(v) = old_val {
            if v == "true" {
                crate::app::GroupBy::Provider
            } else {
                crate::app::GroupBy::None
            }
        } else {
            crate::app::GroupBy::Provider
        };
        assert_eq!(result, crate::app::GroupBy::Provider);
    }

    #[test]
    fn save_group_by_format() {
        let key = "group_by";
        let value = crate::app::GroupBy::Tag("production".to_string()).to_key();
        let line = format!("{}={}", key, value);
        assert_eq!(line, "group_by=tag:production");
    }

    #[test]
    fn save_value_appends_new_key() {
        let existing = "sort_mode=alpha\n";
        let key = "askpass";
        let new_value = "keychain";

        let mut lines: Vec<String> = Vec::new();
        let mut found = false;
        for line in existing.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with('#')
                && !trimmed.is_empty()
                && trimmed
                    .split_once('=')
                    .is_some_and(|(k, _)| k.trim() == key)
            {
                lines.push(format!("{}={}", key, new_value));
                found = true;
            } else {
                lines.push(line.to_string());
            }
        }
        if !found {
            lines.push(format!("{}={}", key, new_value));
        }
        let content = lines.join("\n") + "\n";
        assert!(content.contains("askpass=keychain"));
        assert!(content.contains("sort_mode=alpha"));
        assert!(!found); // Was appended, not replaced
    }

    // --- Real file I/O tests using set_path_override ---
    //
    // PATH_OVERRIDE is a global Mutex<Option<PathBuf>>, so these tests must
    // not run concurrently. We use a module-level Mutex (IO_TEST_LOCK) as a
    // guard: holding it serialises access to PATH_OVERRIDE for the duration
    // of each test body.

    static IO_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static TEST_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn with_temp_prefs<F: FnOnce(&std::path::Path)>(label: &str, f: F) {
        let _guard = IO_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let id = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "purple_prefs_{}_{}_{id}",
            label,
            std::process::id(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("preferences");
        set_path_override(path.clone());
        f(&path);
        std::fs::remove_dir_all(&dir).ok();
        // Reset to nonexistent so other tests don't accidentally inherit it
        set_path_override(std::env::temp_dir().join("purple_prefs_nonexistent_after_test"));
    }

    #[test]
    fn save_and_load_group_by_roundtrip_tag() {
        with_temp_prefs("roundtrip_tag", |_path| {
            let mode = crate::app::GroupBy::Tag("production".to_string());
            save_group_by(&mode).unwrap();
            let loaded = load_group_by();
            assert_eq!(loaded, crate::app::GroupBy::Tag("production".to_string()));
        });
    }

    #[test]
    fn save_and_load_group_by_roundtrip_provider() {
        with_temp_prefs("roundtrip_provider", |_path| {
            save_group_by(&crate::app::GroupBy::Provider).unwrap();
            let loaded = load_group_by();
            assert_eq!(loaded, crate::app::GroupBy::Provider);
        });
    }

    #[test]
    fn save_and_load_group_by_roundtrip_none() {
        with_temp_prefs("roundtrip_none", |_path| {
            save_group_by(&crate::app::GroupBy::None).unwrap();
            let loaded = load_group_by();
            assert_eq!(loaded, crate::app::GroupBy::None);
        });
    }

    #[test]
    fn save_group_by_removes_legacy_key() {
        with_temp_prefs("legacy_key", |path| {
            std::fs::write(path, "group_by_provider=true\nsort_mode=alpha\n").unwrap();
            save_group_by(&crate::app::GroupBy::Provider).unwrap();
            let content = std::fs::read_to_string(path).unwrap();
            assert!(
                content.contains("group_by=provider"),
                "new key should exist"
            );
            assert!(
                !content.contains("group_by_provider"),
                "legacy key should be removed"
            );
            assert!(content.contains("sort_mode=alpha"), "other keys preserved");
        });
    }

    #[test]
    fn load_group_by_backward_compat_real_file() {
        with_temp_prefs("compat_true", |path| {
            std::fs::write(path, "group_by_provider=true\n").unwrap();
            let loaded = load_group_by();
            assert_eq!(loaded, crate::app::GroupBy::Provider);
        });
    }

    #[test]
    fn load_group_by_empty_file_defaults_to_provider() {
        with_temp_prefs("empty_file", |path| {
            std::fs::write(path, "").unwrap();
            let loaded = load_group_by();
            assert_eq!(loaded, crate::app::GroupBy::Provider);
        });
    }

    #[test]
    fn load_group_by_missing_file_defaults_to_provider() {
        let _guard = IO_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path =
            std::env::temp_dir().join(format!("purple_prefs_missing_{}", std::process::id()));
        // Ensure it does not exist
        let _ = std::fs::remove_file(&path);
        set_path_override(path);
        let loaded = load_group_by();
        assert_eq!(loaded, crate::app::GroupBy::Provider);
        set_path_override(std::env::temp_dir().join("purple_prefs_nonexistent_after_test"));
    }

    #[test]
    fn save_group_by_tag_with_special_chars_roundtrip() {
        with_temp_prefs("tag_special", |_path| {
            let mode = crate::app::GroupBy::Tag("us-east-1".to_string());
            save_group_by(&mode).unwrap();
            let loaded = load_group_by();
            assert_eq!(loaded, crate::app::GroupBy::Tag("us-east-1".to_string()));
        });
    }

    #[test]
    fn save_group_by_preserves_other_prefs() {
        with_temp_prefs("preserves_other", |path| {
            std::fs::write(path, "sort_mode=alpha\nview_mode=detailed\n").unwrap();
            save_group_by(&crate::app::GroupBy::Tag("staging".to_string())).unwrap();
            let content = std::fs::read_to_string(path).unwrap();
            assert!(content.contains("sort_mode=alpha"), "sort_mode preserved");
            assert!(
                content.contains("view_mode=detailed"),
                "view_mode preserved"
            );
            assert!(content.contains("group_by=tag:staging"), "group_by written");
        });
    }

    #[test]
    fn remove_value_noop_when_key_not_present() {
        let content = "sort_mode=alpha\nview_mode=compact\n";
        let lines: Vec<&str> = content.lines().collect();
        let has_key = lines.iter().any(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with('#')
                && !trimmed.is_empty()
                && trimmed
                    .split_once('=')
                    .is_some_and(|(k, _)| k.trim() == "nonexistent")
        });
        assert!(!has_key);
    }

    #[test]
    fn remove_value_preserves_comments_and_empty_lines() {
        let content = "# comment\n\nsort_mode=alpha\ngroup_by_provider=true\nview_mode=compact\n";
        let key = "group_by_provider";
        let lines: Vec<String> = content
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.is_empty() {
                    return true;
                }
                trimmed.split_once('=').is_none_or(|(k, _)| k.trim() != key)
            })
            .map(|l| l.to_string())
            .collect();
        let result = lines.join("\n") + "\n";
        assert!(result.contains("# comment"));
        assert!(result.contains("sort_mode=alpha"));
        assert!(result.contains("view_mode=compact"));
        assert!(!result.contains("group_by_provider"));
    }

    #[test]
    fn remove_value_handles_key_as_only_line() {
        let content = "group_by_provider=true\n";
        let key = "group_by_provider";
        let lines: Vec<String> = content
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.is_empty() {
                    return true;
                }
                trimmed.split_once('=').is_none_or(|(k, _)| k.trim() != key)
            })
            .map(|l| l.to_string())
            .collect();
        let result = lines.join("\n") + "\n";
        assert!(!result.contains("group_by_provider"));
    }

    #[test]
    fn remove_value_real_file_io() {
        with_temp_prefs("remove_real_io", |path| {
            std::fs::write(
                path,
                "sort_mode=alpha\ngroup_by_provider=true\nview_mode=compact\n",
            )
            .unwrap();
            // save_group_by calls remove_value("group_by_provider") internally
            save_group_by(&crate::app::GroupBy::Provider).unwrap();
            let content = std::fs::read_to_string(path).unwrap();
            assert!(!content.contains("group_by_provider"));
            assert!(content.contains("sort_mode=alpha"));
            assert!(content.contains("view_mode=compact"));
        });
    }

    #[test]
    fn remove_value_noop_real_file_io() {
        with_temp_prefs("remove_noop_io", |path| {
            std::fs::write(path, "sort_mode=alpha\n").unwrap();
            let before = std::fs::read_to_string(path).unwrap();
            // save_group_by calls remove_value("group_by_provider"), which should be a no-op
            // since the key doesn't exist. We save Provider to trigger the remove path.
            save_group_by(&crate::app::GroupBy::Provider).unwrap();
            let after = std::fs::read_to_string(path).unwrap();
            // The file will have group_by=provider added, but group_by_provider should
            // not have been written and removed (no-op path exercised)
            assert!(after.contains("sort_mode=alpha"));
            assert!(!before.contains("group_by_provider"));
            assert!(!after.contains("group_by_provider"));
        });
    }

    // --- Collapsed groups persistence ---

    #[test]
    fn save_load_collapsed_groups_roundtrip() {
        with_temp_prefs("collapsed_roundtrip", |_path| {
            let mut groups = std::collections::HashSet::new();
            groups.insert("production".to_string());
            groups.insert("staging".to_string());
            save_collapsed_groups(&groups).unwrap();

            let loaded = load_collapsed_groups();
            assert_eq!(loaded, groups);
        });
    }

    #[test]
    fn save_load_collapsed_groups_empty() {
        with_temp_prefs("collapsed_empty", |_path| {
            let groups = std::collections::HashSet::new();
            save_collapsed_groups(&groups).unwrap();

            let loaded = load_collapsed_groups();
            assert!(loaded.is_empty());
        });
    }

    #[test]
    fn save_load_collapsed_groups_special_chars() {
        with_temp_prefs("collapsed_special", |_path| {
            let mut groups = std::collections::HashSet::new();
            groups.insert("us-east,prod".to_string());
            save_collapsed_groups(&groups).unwrap();

            let loaded = load_collapsed_groups();
            assert_eq!(loaded, groups);
            assert!(loaded.contains("us-east,prod"));
        });
    }

    // --- View mode defaults ---

    #[test]
    fn load_view_mode_defaults_to_detailed() {
        with_temp_prefs("view_mode_default", |_path| {
            // No preferences file content written, but file exists (empty)
            // load_view_mode reads "view_mode" key; missing -> Detailed
            let mode = load_view_mode();
            assert_eq!(mode, ViewMode::Detailed);
        });
    }

    #[test]
    fn load_view_mode_explicit_compact() {
        with_temp_prefs("view_mode_compact", |path| {
            std::fs::write(path, "view_mode=compact\n").unwrap();
            let mode = load_view_mode();
            assert_eq!(mode, ViewMode::Compact);
        });
    }
}
