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
        if let Err(e) = crate::preferences::save_last_seen_version(&current.to_string()) {
            log::warn!("[purple] failed to seed last_seen_version: {}", e);
        }
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
