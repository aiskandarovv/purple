use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::app::forms::ProviderFormFields;
use crate::app::types::{ProviderFormBaseline, SyncRecord};
use crate::providers::config::ProviderConfig;

/// Provider-owned state grouped off the `App` god-struct. Holds the
/// provider config, the edit form, the in-flight sync tracking
/// (cancel flags, completed names, error aggregate), the pending
/// delete alias, the on-disk sync history and the dirty-check baseline.
/// Pure state container.
pub struct ProviderState {
    pub config: ProviderConfig,
    pub form: ProviderFormFields,
    pub syncing: HashMap<String, Arc<AtomicBool>>,
    /// Names of providers that completed during this sync batch.
    pub sync_done: Vec<String>,
    /// Whether any provider in the current batch had errors.
    pub sync_had_errors: bool,
    pub pending_delete: Option<String>,
    pub sync_history: HashMap<String, SyncRecord>,
    pub form_baseline: Option<ProviderFormBaseline>,
}

impl Default for ProviderState {
    /// Truly empty default. No disk I/O. Call sites that need persisted
    /// state (App::new) construct with struct-update syntax:
    /// `ProviderState { config: ProviderConfig::load(), sync_history: SyncRecord::load_all(), ..Default::default() }`.
    fn default() -> Self {
        Self {
            config: ProviderConfig::default(),
            form: ProviderFormFields::new(),
            syncing: HashMap::new(),
            sync_done: Vec::new(),
            sync_had_errors: false,
            pending_delete: None,
            sync_history: HashMap::new(),
            form_baseline: None,
        }
    }
}

impl ProviderState {
    /// Provider names sorted by last sync (most recent first), then configured,
    /// then unconfigured. Includes any unknown provider names found in the
    /// config file (e.g. typos or future providers).
    pub fn sorted_names(&self) -> Vec<String> {
        use crate::providers;
        let mut names: Vec<String> = providers::PROVIDER_NAMES
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Append configured providers not in the known list so they are visible and removable
        for section in &self.config.sections {
            if !names.contains(&section.provider) {
                names.push(section.provider.clone());
            }
        }
        names.sort_by(|a, b| {
            let conf_a = self.config.section(a.as_str()).is_some();
            let conf_b = self.config.section(b.as_str()).is_some();
            let ts_a = self.sync_history.get(a.as_str()).map_or(0, |r| r.timestamp);
            let ts_b = self.sync_history.get(b.as_str()).map_or(0, |r| r.timestamp);
            // Configured first (by most recent sync), then unconfigured alphabetically
            conf_b.cmp(&conf_a).then(ts_b.cmp(&ts_a)).then(a.cmp(b))
        });
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        // Must not touch disk. Constructed with ProviderConfig::default()
        // and an empty sync_history. App::new() layers the real on-disk
        // state on top via struct-update syntax.
        let s = ProviderState::default();
        assert!(s.config.sections.is_empty());
        assert!(s.config.path_override.is_none());
        assert!(s.syncing.is_empty());
        assert!(s.sync_done.is_empty());
        assert!(!s.sync_had_errors);
        assert!(s.pending_delete.is_none());
        assert!(s.sync_history.is_empty());
        assert!(s.form_baseline.is_none());
    }

    #[test]
    fn sorted_names_returns_configured_providers_before_unconfigured() {
        use crate::providers::config::ProviderSection;

        let mut state = ProviderState::default();
        state.config.sections.push(ProviderSection {
            provider: "vultr".to_string(),
            token: "tok".to_string(),
            alias_prefix: "vultr".to_string(),
            ..ProviderSection::default()
        });
        state.config.sections.push(ProviderSection {
            provider: "digitalocean".to_string(),
            token: "tok".to_string(),
            alias_prefix: "do".to_string(),
            ..ProviderSection::default()
        });
        state.sync_history.insert(
            "digitalocean".to_string(),
            crate::app::types::SyncRecord {
                timestamp: 2_000,
                message: "ok".to_string(),
                is_error: false,
            },
        );
        state.sync_history.insert(
            "vultr".to_string(),
            crate::app::types::SyncRecord {
                timestamp: 1_000,
                message: "ok".to_string(),
                is_error: false,
            },
        );

        let names = state.sorted_names();
        // Configured providers (most recent sync first) precede unconfigured.
        assert_eq!(&names[0], "digitalocean");
        assert_eq!(&names[1], "vultr");
        // Every known provider name must be present.
        for &known in crate::providers::PROVIDER_NAMES {
            assert!(names.iter().any(|n| n == known), "missing {}", known);
        }
        // Unconfigured tail is sorted alphabetically.
        let unconfigured: Vec<&String> = names.iter().skip(2).collect();
        let mut sorted = unconfigured.clone();
        sorted.sort();
        assert_eq!(unconfigured, sorted);
    }

    #[test]
    fn sorted_names_includes_unknown_providers_from_config() {
        use crate::providers::config::ProviderSection;

        let mut state = ProviderState::default();
        state.config.sections.push(ProviderSection {
            provider: "someday_provider".to_string(),
            token: "tok".to_string(),
            alias_prefix: "x".to_string(),
            ..ProviderSection::default()
        });

        let names = state.sorted_names();
        assert!(names.iter().any(|n| n == "someday_provider"));
    }
}
