// ── What's New overlay strings ──────────────────────────────────────

pub const TITLE: &str = "What's new";
pub const FOOTER_CLOSE_KEYS: &str = "esc/q/n";
pub const FOOTER_CLOSE_LABEL: &str = " close ";
pub const FOOTER_SCROLL_KEYS: &str = "j/k";
pub const FOOTER_SCROLL_LABEL: &str = " scroll ";
pub const FOOTER_TOP_BOTTOM_KEYS: &str = "g/G";
pub const FOOTER_TOP_BOTTOM_LABEL: &str = " top/bottom";
pub const KIND_FEAT: &str = "+ feat  ";
pub const KIND_CHANGE: &str = "~ change";
pub const KIND_FIX: &str = "! fix   ";
pub const EMPTY: &str = "no release notes available.";

pub fn subtitle(from: Option<&str>, to: &str) -> String {
    match from {
        Some(f) if f != to => format!("upgraded from {} to {}", f, to),
        Some(_) => format!("you're on purple {}", to),
        None => format!("welcome to purple {}", to),
    }
}

pub fn update_available(version: &str) -> String {
    format!(
        "purple {} is available. run purple update to upgrade.",
        version
    )
}
