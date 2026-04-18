use std::collections::HashMap;

use crate::app::forms::TunnelForm;
use crate::app::types::TunnelFormBaseline;
use crate::tunnel::{ActiveTunnel, TunnelRule};

/// Tunnel-owned state grouped off the `App` god-struct. Contains the rule
/// list, the edit form, the live child-process map, form baseline for the
/// dirty check, the pending delete index and the cached per-host summary
/// strings. Pure state container; behaviour lives on `App` or on dedicated
/// methods here.
pub struct TunnelState {
    pub list: Vec<TunnelRule>,
    pub form: TunnelForm,
    pub active: HashMap<String, ActiveTunnel>,
    pub form_baseline: Option<TunnelFormBaseline>,
    pub pending_delete: Option<usize>,
    pub summaries_cache: HashMap<String, String>,
}

impl Default for TunnelState {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            form: TunnelForm::new(),
            active: HashMap::new(),
            form_baseline: None,
            pending_delete: None,
            summaries_cache: HashMap::new(),
        }
    }
}

impl TunnelState {
    /// Poll active tunnels for exit. Returns (alias, message, is_error) tuples.
    pub fn poll(&mut self) -> Vec<(String, String, bool)> {
        if self.active.is_empty() {
            return Vec::new();
        }
        let mut exited = Vec::new();
        let mut to_remove = Vec::new();
        for (alias, tunnel) in &mut self.active {
            match tunnel.child.try_wait() {
                Ok(Some(status)) => {
                    let stderr_msg = tunnel.child.stderr.take().and_then(|mut stderr| {
                        use std::io::Read;
                        let mut buf = vec![0u8; 1024];
                        match stderr.read(&mut buf) {
                            Ok(n) if n > 0 => {
                                let s = String::from_utf8_lossy(&buf[..n]);
                                let trimmed = s.trim();
                                if trimmed.is_empty() {
                                    None
                                } else {
                                    Some(trimmed.to_string())
                                }
                            }
                            _ => None,
                        }
                    });
                    let exit_code = status.code().unwrap_or(-1);
                    if !status.success() {
                        log::error!(
                            "[external] Tunnel exited unexpectedly: alias={alias} exit={exit_code}"
                        );
                        if let Some(ref err) = stderr_msg {
                            log::debug!("[external] Tunnel stderr: {}", err.trim());
                        }
                    }
                    let (msg, is_error) = if status.success() {
                        (format!("Tunnel for {} closed.", alias), false)
                    } else if let Some(err) = stderr_msg {
                        (format!("Tunnel for {}: {}", alias, err), true)
                    } else {
                        (
                            format!("Tunnel for {} exited with code {}.", alias, exit_code),
                            true,
                        )
                    };
                    exited.push((alias.clone(), msg, is_error));
                    to_remove.push(alias.clone());
                }
                Ok(None) => {}
                Err(e) => {
                    exited.push((
                        alias.clone(),
                        format!("Tunnel for {} lost: {}", alias, e),
                        true,
                    ));
                    to_remove.push(alias.clone());
                }
            }
        }
        for alias in to_remove {
            self.active.remove(&alias);
        }
        exited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_empty() {
        let s = TunnelState::default();
        assert!(s.list.is_empty());
        assert!(s.active.is_empty());
        assert!(s.pending_delete.is_none());
        assert!(s.summaries_cache.is_empty());
    }

    #[test]
    fn poll_on_empty_returns_empty_vec() {
        // Fast path: no active tunnels means no exit events to report and
        // no child processes to reap. Spawning real ssh child processes
        // belongs in integration tests.
        let mut s = TunnelState::default();
        let result = s.poll();
        assert!(result.is_empty());
        assert!(s.active.is_empty());
    }
}
