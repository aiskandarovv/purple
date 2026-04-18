use std::collections::HashMap;

use crate::ssh_config::model::HostEntry;

/// Ping/health-check state for all hosts.
pub struct PingState {
    pub status: HashMap<String, PingStatus>,
    pub has_pinged: bool,
    pub generation: u64,
    pub slow_threshold_ms: u16,
    pub auto_ping: bool,
    pub filter_down_only: bool,
    pub checked_at: Option<std::time::Instant>,
}

impl Default for PingState {
    fn default() -> Self {
        Self {
            status: HashMap::new(),
            has_pinged: false,
            generation: 0,
            slow_threshold_ms: 500,
            auto_ping: false,
            filter_down_only: false,
            checked_at: None,
        }
    }
}

impl PingState {
    /// Construct with slow threshold + auto-ping loaded from preferences.
    pub fn from_preferences() -> Self {
        Self {
            slow_threshold_ms: crate::preferences::load_slow_threshold(),
            auto_ping: crate::preferences::load_auto_ping(),
            ..Self::default()
        }
    }
}

/// Ping status for a host.
#[derive(Debug, Clone, PartialEq)]
pub enum PingStatus {
    Checking,
    Reachable { rtt_ms: u32 },
    Slow { rtt_ms: u32 },
    Unreachable,
    Skipped,
}

/// Classify a ping result into a PingStatus based on RTT and threshold.
pub fn classify_ping(rtt_ms: Option<u32>, slow_threshold_ms: u16) -> PingStatus {
    match rtt_ms {
        Some(ms) if ms >= slow_threshold_ms as u32 => PingStatus::Slow { rtt_ms: ms },
        Some(ms) => PingStatus::Reachable { rtt_ms: ms },
        None => PingStatus::Unreachable,
    }
}

/// Propagate a ping result to all hosts that use the given alias as ProxyJump bastion.
pub fn propagate_ping_to_dependents(
    hosts: &[HostEntry],
    ping_status: &mut HashMap<String, PingStatus>,
    bastion_alias: &str,
    status: &PingStatus,
) {
    for h in hosts {
        if h.proxy_jump == bastion_alias {
            ping_status.insert(h.alias.clone(), status.clone());
        }
    }
}

/// Sort key for ping status: unreachable first, slow, reachable, unchecked last.
pub fn ping_sort_key(status: Option<&PingStatus>) -> u8 {
    match status {
        Some(PingStatus::Unreachable) => 0,
        Some(PingStatus::Slow { .. }) => 1,
        Some(PingStatus::Reachable { .. }) => 2,
        Some(PingStatus::Checking) => 3,
        Some(PingStatus::Skipped) | None => 4,
    }
}

/// Status glyph for dual encoding (color + shape).
/// ● online, ▲ slow, ✖ down. Checking uses animated spinner via tick.
pub fn status_glyph(status: Option<&PingStatus>, tick: u64) -> &'static str {
    match status {
        Some(PingStatus::Reachable { .. }) => "\u{25CF}", // ●
        Some(PingStatus::Slow { .. }) => "\u{25B2}",      // ▲
        Some(PingStatus::Unreachable) => "\u{2716}",      // ✖
        Some(PingStatus::Checking) => {
            crate::animation::SPINNER_FRAMES
                [(tick as usize) % crate::animation::SPINNER_FRAMES.len()]
        }
        Some(PingStatus::Skipped) => "",
        None => "\u{25CB}", // ○
    }
}
