use semver::Version;

pub struct PostInitOutcome {
    pub upgrade_toast: Option<String>,
}

pub fn evaluate() -> PostInitOutcome {
    let current = match Version::parse(env!("CARGO_PKG_VERSION")) {
        Ok(v) => v,
        Err(_) => {
            return PostInitOutcome {
                upgrade_toast: None,
            };
        }
    };
    let last = crate::preferences::load_last_seen_version()
        .ok()
        .flatten()
        .and_then(|s| Version::parse(s.as_str()).ok());

    // First-ever launch has no last_seen_version. The Welcome screen already
    // introduces purple; adding a sticky "what's new" toast on top would be
    // noise. Leave last_seen_version unset so the Welcome handler can seed
    // it on close, after which future launches compare normally.
    if last.is_none() {
        return PostInitOutcome {
            upgrade_toast: None,
        };
    }

    if let Some(ref seen) = last {
        if seen >= &current {
            return PostInitOutcome {
                upgrade_toast: None,
            };
        }
    }

    let sections = crate::changelog::cached();
    let shown = crate::changelog::versions_to_show(sections, last.as_ref(), &current, 5);
    if shown.is_empty() {
        // Do not silently advance last_seen_version here. Bumping it on every
        // launch lets dev builds with a higher Cargo.toml version race ahead of
        // the installed release, which then suppresses the upgrade toast on the
        // next real install. last_seen_version only advances via explicit user
        // actions (Welcome close, What's New close).
        return PostInitOutcome {
            upgrade_toast: None,
        };
    }

    log::debug!(
        "[purple] queued upgrade toast: {} sections (last_seen={:?}, current={})",
        shown.len(),
        last.as_ref().map(|v| v.to_string()),
        current
    );
    PostInitOutcome {
        upgrade_toast: Some(crate::messages::whats_new_toast::upgraded(
            &current.to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences;

    fn current() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    #[test]
    fn first_launch_returns_no_toast() {
        preferences::tests_helpers::with_temp_prefs("onboarding_first", |_| {
            let outcome = evaluate();
            assert!(
                outcome.upgrade_toast.is_none(),
                "first launch must not show upgrade toast"
            );
        });
    }

    #[test]
    fn up_to_date_returns_no_toast() {
        preferences::tests_helpers::with_temp_prefs("onboarding_up_to_date", |_| {
            preferences::save_last_seen_version(&current()).unwrap();
            let outcome = evaluate();
            assert!(outcome.upgrade_toast.is_none());
            // evaluate() must never rewrite last_seen_version: any write
            // would race ahead of the installed release and suppress a
            // legitimate upgrade toast after a brew/curl install.
            assert_eq!(
                preferences::load_last_seen_version().unwrap().as_deref(),
                Some(current().as_str()),
                "evaluate() must not touch last_seen_version when up-to-date"
            );
        });
    }

    #[test]
    fn downgrade_returns_no_toast() {
        preferences::tests_helpers::with_temp_prefs("onboarding_downgrade", |_| {
            preferences::save_last_seen_version("999.0.0").unwrap();
            let outcome = evaluate();
            assert!(outcome.upgrade_toast.is_none());
        });
    }

    #[test]
    fn upgrade_with_new_sections_returns_toast() {
        preferences::tests_helpers::with_temp_prefs("onboarding_upgrade_toast", |_| {
            preferences::save_last_seen_version("0.0.1").unwrap();
            let outcome = evaluate();
            let fragment = crate::messages::whats_new_toast::INVITE_FRAGMENT;
            assert!(
                outcome
                    .upgrade_toast
                    .as_deref()
                    .is_some_and(|t| t.contains(fragment)),
                "expected upgrade toast with invite fragment"
            );
        });
    }

    #[test]
    fn evaluate_never_writes_last_seen_version() {
        // Regression: the old `shown.is_empty()` arm silently wrote
        // last_seen_version = current, which let dev builds (Cargo.toml
        // version ahead of any CHANGELOG entry) race ahead of the next
        // installed release and suppress its upgrade toast. The fix is a
        // pure delete of that write — evaluate() now never mutates the
        // pref on ANY code path. The `shown.is_empty()` arm itself is
        // hard to reach without stubbing `changelog::cached()` because a
        // shipped CHANGELOG.md always has entries in the current-version
        // range, so this property-style test sweeps every reachable arm
        // (first-launch, up-to-date, downgrade, upgrade-with-sections,
        // unparseable) and asserts the pref comes out exactly as it went
        // in. If someone re-introduces a pref-write in any arm, at least
        // one of these scenarios will catch it.
        let scenarios: &[(&str, Option<&str>)] = &[
            ("first_launch", None),
            ("same_version", Some(env!("CARGO_PKG_VERSION"))),
            ("downgrade", Some("999.0.0")),
            ("older_version", Some("0.0.1")),
            ("unparseable", Some("not-a-semver")),
        ];
        for (label, input) in scenarios {
            preferences::tests_helpers::with_temp_prefs(
                &format!("onboarding_no_writes_{label}"),
                |_| {
                    if let Some(v) = input {
                        preferences::save_last_seen_version(v).unwrap();
                    }
                    let _ = evaluate();
                    let after = preferences::load_last_seen_version().unwrap();
                    assert_eq!(
                        after.as_deref(),
                        *input,
                        "[{}] evaluate() must not touch last_seen_version",
                        label
                    );
                },
            );
        }
    }

    #[test]
    fn unparseable_last_seen_falls_through_to_first_launch() {
        preferences::tests_helpers::with_temp_prefs("onboarding_unparseable", |_| {
            preferences::save_last_seen_version("not-a-semver").unwrap();
            let outcome = evaluate();
            assert!(
                outcome.upgrade_toast.is_none(),
                "garbled last_seen must be treated as first launch, not surface a toast"
            );
        });
    }
}
