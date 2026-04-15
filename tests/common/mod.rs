use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("api_contracts")
        .join(name)
}

/// Load a JSON fixture from `tests/api_contracts/`.
pub fn load_json(name: &str) -> Value {
    let path = fixture_path(name);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse fixture {}: {}", name, e))
}

/// Load an XML fixture and return the raw string.
#[allow(dead_code)]
pub fn load_xml(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e))
}

/// Assert a JSON value has an object key at the given dot-notation path.
/// Supports array indexing: `"key[0].nested"`.
pub fn assert_has_key(val: &Value, path: &str) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = val;
    for (i, part) in parts.iter().enumerate() {
        if let Some(idx_start) = part.find('[') {
            let key = &part[..idx_start];
            let idx: usize = part[idx_start + 1..part.len() - 1]
                .parse()
                .unwrap_or_else(|_| panic!("invalid array index in path '{}'", path));
            current = current
                .get(key)
                .unwrap_or_else(|| panic!("missing key '{}' at '{}'", key, path));
            current = current
                .get(idx)
                .unwrap_or_else(|| panic!("missing index [{}] at '{}'", idx, path));
        } else {
            current = current.get(part).unwrap_or_else(|| {
                let traversed = parts[..i].join(".");
                panic!(
                    "missing key '{}' in fixture (traversed: '{}')",
                    part, traversed
                );
            });
        }
    }
}
