//! Toast Queue System
//!
//! Type-enforced toast queue that ensures only one toast is visible at a time.
//!
//! ## Auto-Dismiss Behavior
//!
//! - Info, Success, Warning toasts auto-dismiss after 5 seconds (50 ticks at 100ms/tick)
//! - Error toasts do NOT auto-dismiss and must be manually dismissed (Escape key)

use std::collections::VecDeque;

// Re-export portable toast lifecycle constants from aura-app
pub use aura_app::ui::types::{
    duration_ticks, ms_to_ticks, should_auto_dismiss, ticks_to_ms, will_auto_dismiss,
    DEFAULT_TOAST_DURATION_MS, DEFAULT_TOAST_TICKS, MAX_PENDING_TOASTS, NO_AUTO_DISMISS,
    TOAST_TICK_RATE_MS,
};

// Use portable constants from aura-app
use aura_app::ui::types::{
    DEFAULT_TOAST_TICKS as PORTABLE_DEFAULT_TICKS, MAX_PENDING_TOASTS as PORTABLE_MAX_PENDING,
    NO_AUTO_DISMISS as PORTABLE_NO_DISMISS,
};

/// Toast queue that ensures only one toast is visible at a time.
///
/// **Type Enforcement**: This is the ONLY way to show toasts.
/// Remove `Vec<Toast>` fields and use this queue instead.
///
/// ## Behavior
///
/// - Toasts are shown in FIFO order
/// - Auto-dismiss via `tick()` when `ticks_remaining` reaches 0
/// - Manual dismiss via `dismiss()` or Escape key
/// - One modal + one toast can coexist (different screen regions)
#[derive(Clone, Debug, Default)]
pub struct ToastQueue {
    /// Queue of pending toasts (FIFO)
    pending: VecDeque<QueuedToast>,
    /// Currently active toast (if any)
    active: Option<QueuedToast>,
}

/// A queued toast notification
#[derive(Clone, Debug)]
pub struct QueuedToast {
    /// Unique ID for this toast
    pub id: u64,
    /// Toast message
    pub message: String,
    /// Severity level
    pub level: ToastLevel,
    /// Ticks remaining before auto-dismiss
    pub ticks_remaining: u32,
    /// Full error text when the message was shortened for display
    pub details: Option<String>,
}

impl QueuedToast {
    /// Create a new toast with appropriate duration based on level.
    ///
    /// - Error toasts: Never auto-dismiss (must be manually dismissed)
    /// - Other toasts: Auto-dismiss after 5 seconds (50 ticks at 100ms/tick)
    pub fn new(id: u64, message: impl Into<String>, level: ToastLevel) -> Self {
        // Use portable constants from aura-app
        let ticks_remaining = if level == ToastLevel::Error {
            PORTABLE_NO_DISMISS
        } else {
            PORTABLE_DEFAULT_TICKS
        };
        let (message, details) = if level == ToastLevel::Error {
            user_facing_error(&message.into())
        } else {
            (message.into(), None)
        };
        Self {
            id,
            message,
            level,
            ticks_remaining,
            details,
        }
    }

    /// Create with custom duration (in ticks, 100ms per tick)
    #[must_use]
    pub fn with_duration(mut self, ticks: u32) -> Self {
        self.ticks_remaining = ticks;
        self
    }

    /// Create an info toast (auto-dismisses after 5 seconds)
    pub fn info(id: u64, message: impl Into<String>) -> Self {
        Self::new(id, message, ToastLevel::Info)
    }

    /// Create a success toast (auto-dismisses after 5 seconds)
    pub fn success(id: u64, message: impl Into<String>) -> Self {
        Self::new(id, message, ToastLevel::Success)
    }

    /// Create a warning toast (auto-dismisses after 5 seconds)
    pub fn warning(id: u64, message: impl Into<String>) -> Self {
        Self::new(id, message, ToastLevel::Warning)
    }

    /// Create an error toast (does NOT auto-dismiss, must be manually dismissed)
    pub fn error(id: u64, message: impl Into<String>) -> Self {
        Self::new(id, message, ToastLevel::Error)
    }

    /// Check if this toast should auto-dismiss
    #[must_use]
    pub fn auto_dismisses(&self) -> bool {
        // Use portable function from aura-app
        should_auto_dismiss(self.level.into(), self.ticks_remaining)
    }
}

/// Markers that identify internal error chains rather than user-facing text.
const INTERNAL_ERROR_MARKERS: &[&str] = &[
    "internal error",
    "agent configuration error",
    "json parsing",
    "detail=",
    "_id=",
    "amp operation failed",
    "operation failed:",
    "invalid:",
];

/// Stable, user-facing sentences the runtime uses when an inviter rejects or
/// never confirms a contact invitation acceptance.
const INVITATION_OUTCOME_SENTENCES: &[&str] = &[
    "The inviter revoked this contact invitation",
    "This contact invitation has expired",
    "This contact invitation was already used",
    "The inviter did not confirm this contact invitation",
];

/// The runtime's invitation-outcome sentence inside a wrapped error chain.
fn invitation_outcome_sentence(raw: &str) -> Option<String> {
    INVITATION_OUTCOME_SENTENCES.iter().find_map(|sentence| {
        let start = raw.find(sentence)?;
        let rest = &raw[start..];
        let end = rest.find(['"', ')', '\n']).unwrap_or(rest.len());
        Some(rest[..end].trim_end_matches('.').trim().to_string())
    })
}

/// Turn a raw error string into a short user-facing message.
///
/// Returns the message to show and, when it differs, the raw text kept as
/// details for copying (`y`) and diagnostics.
#[must_use]
pub fn user_facing_error(raw: &str) -> (String, Option<String>) {
    let lowered = raw.to_ascii_lowercase();
    let friendly = if let Some(outcome) = invitation_outcome_sentence(raw) {
        Some(outcome)
    } else if lowered.contains("invalid invite code") || lowered.contains("invalid invitation code")
    {
        Some("That invitation code isn't valid".to_string())
    } else if lowered.contains("amp_send_message") || lowered.contains("send_message failed") {
        Some("Couldn't send the message - retry".to_string())
    } else if INTERNAL_ERROR_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        // Keep the leading human label ("Failed to import invitation") and drop
        // the internal chain behind it.
        let head = raw.split(": ").next().unwrap_or(raw).trim();
        let head_is_internal = INTERNAL_ERROR_MARKERS
            .iter()
            .any(|marker| head.to_ascii_lowercase().contains(marker))
            || head.chars().next().is_some_and(|c| !c.is_ascii_uppercase());
        Some(if head_is_internal || head.len() == raw.trim().len() {
            "Something went wrong (press y to copy details)".to_string()
        } else {
            format!("{head} (press y to copy details)")
        })
    } else {
        None
    };
    match friendly {
        Some(message) => (message, Some(raw.to_string())),
        None => (raw.to_string(), None),
    }
}

impl ToastQueue {
    /// Create a new empty toast queue
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue a toast. If no toast is active, it becomes active immediately.
    pub fn enqueue(&mut self, toast: QueuedToast) {
        // A repeated error would reappear as soon as the visible copy is dismissed.
        let is_duplicate_error =
            toast.level == ToastLevel::Error
                && self.active.iter().chain(self.pending.iter()).any(|queued| {
                    queued.level == ToastLevel::Error && queued.message == toast.message
                });
        if is_duplicate_error {
            return;
        }
        if self.active.is_none() {
            self.active = Some(toast);
        } else {
            // Use portable constant from aura-app
            if self.pending.len() >= PORTABLE_MAX_PENDING {
                // Drop the oldest pending toast to keep memory bounded.
                let _ = self.pending.pop_front();
            }
            self.pending.push_back(toast);
        }
    }

    /// Dismiss the active toast and activate the next one in the queue (if any).
    /// Returns the dismissed toast.
    pub fn dismiss(&mut self) -> Option<QueuedToast> {
        let dismissed = self.active.take();
        self.active = self.pending.pop_front();
        dismissed
    }

    /// Get a reference to the currently active toast (for rendering).
    #[must_use]
    pub fn current(&self) -> Option<&QueuedToast> {
        self.active.as_ref()
    }

    /// Check if any toast is currently active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Process a tick: decrement timer and auto-dismiss expired toasts.
    ///
    /// Error toasts are skipped (they never auto-dismiss).
    /// Returns true if a toast was auto-dismissed.
    pub fn tick(&mut self) -> bool {
        if let Some(toast) = &mut self.active {
            // Error toasts don't auto-dismiss
            if toast.level == ToastLevel::Error {
                return false;
            }

            toast.ticks_remaining = toast.ticks_remaining.saturating_sub(1);
            if toast.ticks_remaining == 0 {
                self.active = self.pending.pop_front();
                return true;
            }
        }
        false
    }

    /// Clear all toasts (active and pending).
    pub fn clear(&mut self) {
        self.active = None;
        self.pending.clear();
    }

    /// Get the number of pending toasts (not including active).
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

/// Toast severity level
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastLevel {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl ToastLevel {
    /// Get the dismissal priority (higher = dismiss first on Escape)
    /// Priority: Error (3) > Warning (2) > Info/Success (1)
    ///
    /// Delegates to portable implementation from aura-app.
    #[must_use]
    pub fn priority(self) -> u8 {
        // Use portable priority from aura-app
        aura_app::ui::types::ToastLevel::from(self).priority()
    }
}

/// Convert local ToastLevel to portable ToastLevel from aura-app
impl From<ToastLevel> for aura_app::ui::types::ToastLevel {
    fn from(level: ToastLevel) -> Self {
        match level {
            ToastLevel::Info => Self::Info,
            ToastLevel::Success => Self::Success,
            ToastLevel::Warning => Self::Warning,
            ToastLevel::Error => Self::Error,
        }
    }
}

/// Convert portable ToastLevel from aura-app to local ToastLevel
impl From<aura_app::ui::types::ToastLevel> for ToastLevel {
    fn from(level: aura_app::ui::types::ToastLevel) -> Self {
        match level {
            aura_app::ui::types::ToastLevel::Info => Self::Info,
            aura_app::ui::types::ToastLevel::Success => Self::Success,
            aura_app::ui::types::ToastLevel::Warning => Self::Warning,
            aura_app::ui::types::ToastLevel::Error => Self::Error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_outcomes_show_the_runtime_sentence() {
        let raw =
            "Internal error: operation_kind=AcceptContactInvitation; detail=accept invitation \
                   failed: Invalid: The inviter revoked this contact invitation";
        let toast = QueuedToast::error(1, raw);
        assert_eq!(toast.message, "The inviter revoked this contact invitation");
        assert_eq!(toast.details.as_deref(), Some(raw));

        let (message, _) = user_facing_error(
            "detail=The inviter did not confirm this contact invitation within 30s; try again when they are online",
        );
        assert_eq!(
            message,
            "The inviter did not confirm this contact invitation within 30s; try again when they are online"
        );
    }

    #[test]
    fn internal_error_chains_are_shortened_and_kept_as_details() {
        let raw = "Internal error: import invitation: Internal error: Validation failed: \
                   Invalid invite code: Agent configuration error: JSON parsing failed";
        let toast = QueuedToast::error(1, raw);
        assert_eq!(toast.message, "That invitation code isn't valid");
        assert_eq!(toast.details.as_deref(), Some(raw));

        let raw = "Failed to accept invitation: Internal error: detail=context_id=abc";
        let toast = QueuedToast::error(2, raw);
        assert_eq!(
            toast.message,
            "Failed to accept invitation (press y to copy details)"
        );
        assert_eq!(toast.details.as_deref(), Some(raw));
    }

    #[test]
    fn user_facing_errors_are_left_unchanged() {
        let toast = QueuedToast::error(1, "Invalid authority ID for device enrollment invitee");
        assert_eq!(
            toast.message,
            "Invalid authority ID for device enrollment invitee"
        );
        assert!(toast.details.is_none());
    }

    #[test]
    fn duplicate_error_toasts_are_not_queued() {
        let mut queue = ToastQueue::new();
        queue.enqueue(QueuedToast::error(1, "Couldn't reach peer"));
        queue.enqueue(QueuedToast::error(2, "Couldn't reach peer"));
        queue.dismiss();
        assert!(queue.current().is_none());
    }
}
