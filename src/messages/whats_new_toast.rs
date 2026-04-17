// ── What's New upgrade toast ─────────────────────────────────────────
//
// Stable fragment used both to build the toast and to identify it for
// dismissal. Keep `upgraded()` output in sync with `INVITE_FRAGMENT` so
// `text.contains(INVITE_FRAGMENT)` remains a reliable match.

pub const INVITE_FRAGMENT: &str = "press n for what's new";

pub fn upgraded(version: &str) -> String {
    format!("v{} installed. {}", version, INVITE_FRAGMENT)
}
