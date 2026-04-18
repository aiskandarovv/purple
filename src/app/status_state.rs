use std::collections::VecDeque;
use std::time::Instant;

/// Status/toast-owned state grouped off the `App` god-struct. Contains the
/// footer status message, the active toast and the toast queue. Pure state
/// container plus the routing helpers that only touch these three fields.
/// `tick_status` stays on `App` because it must read `syncing_providers` to
/// suppress expiry during in-flight provider syncs.
#[derive(Default)]
pub struct StatusCenter {
    pub status: Option<StatusMessage>,
    pub toast: Option<StatusMessage>,
    pub toast_queue: VecDeque<StatusMessage>,
}

impl StatusCenter {
    #[deprecated(note = "use notify() / notify_error() instead")]
    pub fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        let class = if is_error {
            MessageClass::Error
        } else {
            MessageClass::Success
        };
        // Errors are sticky by default so the user cannot miss them.
        let sticky = matches!(class, MessageClass::Error);
        let msg = StatusMessage {
            text: text.into(),
            class,
            tick_count: 0,
            sticky,
            created_at: std::time::Instant::now(),
        };
        if msg.is_toast() {
            self.push_toast(msg);
        } else {
            log::debug!("footer <- {:?}: {}", msg.class, msg.text);
            self.status = Some(msg);
        }
    }

    /// Push a toast message. Success toasts replace the current toast
    /// immediately (last-write-wins). Warning and Error toasts are queued
    /// (max `TOAST_QUEUE_MAX`) so they are never lost.
    pub(crate) fn push_toast(&mut self, msg: StatusMessage) {
        log::debug!("toast <- {:?}: {}", msg.class, msg.text);
        if msg.class == MessageClass::Success {
            // Success replaces any active toast and clears the queue.
            self.toast = Some(msg);
            self.toast_queue.clear();
        } else if self.toast.is_some() {
            if self.toast_queue.len() >= crate::ui::design::TOAST_QUEUE_MAX {
                if let Some(dropped) = self.toast_queue.front() {
                    log::debug!("toast queue full, dropping: {}", dropped.text);
                }
                self.toast_queue.pop_front();
            }
            self.toast_queue.push_back(msg);
        } else {
            self.toast = Some(msg);
        }
    }

    /// Set an Info-class status message that displays in the footer only.
    #[deprecated(note = "use notify_info() instead")]
    pub fn set_info_status(&mut self, text: impl Into<String>) {
        let text = text.into();
        log::debug!("footer <- Info: {}", text);
        self.status = Some(StatusMessage {
            text,
            class: MessageClass::Info,
            tick_count: 0,
            sticky: false,
            created_at: std::time::Instant::now(),
        });
    }

    /// Like `notify` but skips the write when a sticky message is active.
    /// Use for background/timer events (ping expiry, sync ticks) that must
    /// not clobber an in-progress or critical sticky message.
    /// Routes to Info (footer) by default, Error toast if is_error.
    #[deprecated(note = "use notify_background() / notify_background_error() instead")]
    pub fn set_background_status(&mut self, text: impl Into<String>, is_error: bool) {
        if is_error {
            let msg = StatusMessage {
                text: text.into(),
                class: MessageClass::Error,
                tick_count: 0,
                sticky: true,
                created_at: std::time::Instant::now(),
            };
            self.push_toast(msg);
            return;
        }
        if self.status.as_ref().is_some_and(|s| s.sticky) {
            log::debug!("background status suppressed (sticky active)");
            return;
        }
        let text = text.into();
        log::debug!("footer <- Info: {}", text);
        self.status = Some(StatusMessage {
            text,
            class: MessageClass::Info,
            tick_count: 0,
            sticky: false,
            created_at: std::time::Instant::now(),
        });
    }

    /// Sticky messages always go to the footer (`self.status`), even when the
    /// class is Error. The `sticky` flag overrides the normal toast routing
    /// because sticky messages (Vault SSH signing summaries, progress spinners)
    /// must remain visible in the footer until explicitly replaced.
    #[deprecated(note = "use notify_progress() / notify_sticky_error() instead")]
    pub fn set_sticky_status(&mut self, text: impl Into<String>, is_error: bool) {
        let text = text.into();
        let class = if is_error {
            MessageClass::Error
        } else {
            MessageClass::Progress
        };
        log::debug!("footer <- sticky {:?}: {}", class, text);
        self.status = Some(StatusMessage {
            text,
            class,
            tick_count: 0,
            sticky: true,
            created_at: std::time::Instant::now(),
        });
    }

    /// Tick the toast message timer. Uses wall-clock time via `created_at`
    /// so expiry is independent of the tick rate. Called every tick; the
    /// actual check is `created_at.elapsed() > timeout_ms()`.
    pub fn tick_toast(&mut self) {
        if let Some(ref toast) = self.toast {
            if toast.sticky {
                return;
            }
            let timeout_ms = toast.timeout_ms();
            if timeout_ms != u64::MAX && toast.created_at.elapsed().as_millis() as u64 > timeout_ms
            {
                log::debug!("toast expired: {}", toast.text);
                self.toast = self.toast_queue.pop_front();
            }
        }
    }
}

/// Classification of status messages for routing to toast overlay vs footer.
///
/// Five levels: Success / Info / Warning / Error / Progress. Severity rises
/// from Info to Error. Toast vs footer routing follows attention-urgency:
/// Success, Warning and Error draw the eye via toast; Info and Progress
/// sit in the footer for passive consumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageClass {
    /// User action succeeded (copy, sort, delete). Toast, length-proportional timeout.
    /// Color: green `\u{2713}`.
    Success,
    /// Background event (sync complete, config reload). Footer, length-proportional timeout.
    /// Color: muted.
    Info,
    /// Caution or degraded state (stale hosts, deprecated config,
    /// validation failure, empty-state notice). Toast, length-proportional
    /// timeout (longer than Success). Auto-expires.
    /// Color: yellow `\u{26A0}`.
    Warning,
    /// Error condition requiring acknowledgement. Toast, **sticky by default**
    /// so the user cannot miss it. Cleared by next user action.
    /// Color: red `\u{2716}`.
    Error,
    /// Long-running operation with spinner. Footer, sticky.
    /// Color: muted with spinner.
    Progress,
}

/// Status message displayed as toast overlay or in the footer.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub class: MessageClass,
    /// Retained for backward compatibility with tests that inspect it.
    /// Expiry logic uses `created_at` (wall-clock) instead.
    #[allow(dead_code)]
    pub tick_count: u32,
    /// When true the message never auto-expires and `notify_background`
    /// will not overwrite it. Cleared by `notify` or `notify_progress`.
    pub sticky: bool,
    /// Wall-clock instant when the message was created. Used by the drain
    /// bar renderer for smooth (frame-rate-independent) animation instead
    /// of the discrete `tick_count`.
    pub created_at: Instant,
}

impl StatusMessage {
    /// Backward compat: is this an error- or warning-class message?
    pub fn is_error(&self) -> bool {
        matches!(self.class, MessageClass::Error | MessageClass::Warning)
    }

    /// Timeout in milliseconds for this message class.
    ///
    /// Length-proportional: shorter messages clear faster, longer messages
    /// stay visible longer to give the user time to read. The minimum keeps
    /// 1-word messages on screen long enough to register; the per-word
    /// component scales with reading time. Errors and Progress are sticky
    /// (return `u64::MAX`).
    ///
    /// All timing is in wall-clock milliseconds, independent of the tick
    /// rate. Both `tick_toast` (expiry) and `render_toast` (drain bar)
    /// compare `created_at.elapsed()` against this value.
    pub fn timeout_ms(&self) -> u64 {
        let words = self
            .text
            .split_whitespace()
            .count()
            .min(crate::ui::design::WORD_CAP) as u64;
        let proportional = words.saturating_mul(crate::ui::design::MS_PER_WORD);
        let min_ms = match self.class {
            MessageClass::Success | MessageClass::Info => crate::ui::design::TIMEOUT_MIN_MS,
            MessageClass::Warning => crate::ui::design::TIMEOUT_MIN_WARNING_MS,
            MessageClass::Error | MessageClass::Progress => return u64::MAX,
        };
        min_ms.max(proportional)
    }

    /// Should this message render as a toast overlay?
    pub fn is_toast(&self) -> bool {
        matches!(
            self.class,
            MessageClass::Success | MessageClass::Warning | MessageClass::Error
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(text: &str, class: MessageClass, sticky: bool) -> StatusMessage {
        StatusMessage {
            text: text.to_string(),
            class,
            tick_count: 0,
            sticky,
            created_at: std::time::Instant::now(),
        }
    }

    #[test]
    fn default_is_quiet() {
        let s = StatusCenter::default();
        assert!(s.status.is_none());
        assert!(s.toast.is_none());
        assert!(s.toast_queue.is_empty());
    }

    #[test]
    #[allow(deprecated)]
    fn test_set_status_info_populates_status_field() {
        let mut s = StatusCenter::default();
        // Info class is routed to the footer, not a toast.
        s.set_info_status("hello");
        assert!(s.status.is_some());
        assert_eq!(s.status.as_ref().unwrap().text, "hello");
        assert!(s.toast.is_none());
    }

    #[test]
    #[allow(deprecated)]
    fn test_set_status_error_is_routed_to_sticky_toast() {
        let mut s = StatusCenter::default();
        s.set_status("boom", true);
        // Errors are toasts and sticky, so they live in `toast`.
        assert!(s.toast.is_some());
        let toast = s.toast.as_ref().unwrap();
        assert_eq!(toast.class, MessageClass::Error);
        assert!(toast.sticky);
    }

    #[test]
    #[allow(deprecated)]
    fn test_set_sticky_status_writes_footer_and_marks_sticky() {
        let mut s = StatusCenter::default();
        s.set_sticky_status("signing cert", false);
        let footer = s.status.as_ref().expect("footer status set");
        assert_eq!(footer.text, "signing cert");
        assert_eq!(footer.class, MessageClass::Progress);
        assert!(
            footer.sticky,
            "sticky progress message must stay until replaced"
        );
        // Sticky messages never go to the toast slot.
        assert!(s.toast.is_none());
    }

    #[test]
    fn tick_toast_advances_queue_once_active_expires() {
        let mut s = StatusCenter::default();
        // First warning occupies the active toast slot.
        s.push_toast(msg("first", MessageClass::Warning, false));
        // Second warning queues because the slot is taken.
        s.push_toast(msg("second", MessageClass::Warning, false));
        assert_eq!(s.toast.as_ref().unwrap().text, "first");
        assert_eq!(s.toast_queue.len(), 1);

        // Force the active toast into the expired state by rewinding
        // created_at past its wall-clock timeout.
        let expired_at = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(60))
            .expect("instant subtraction");
        if let Some(active) = s.toast.as_mut() {
            active.created_at = expired_at;
        }
        s.tick_toast();
        // Queue drains into the active slot.
        assert_eq!(s.toast.as_ref().unwrap().text, "second");
        assert!(s.toast_queue.is_empty());
    }

    #[test]
    fn tick_toast_does_not_expire_sticky_toast() {
        let mut s = StatusCenter::default();
        s.push_toast(msg("stay", MessageClass::Error, true));
        let expired_at = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(3600))
            .expect("instant subtraction");
        if let Some(active) = s.toast.as_mut() {
            active.created_at = expired_at;
        }
        s.tick_toast();
        assert!(s.toast.is_some(), "sticky toast must not expire");
    }
}
