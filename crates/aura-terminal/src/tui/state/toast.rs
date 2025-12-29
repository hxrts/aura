//! Toast Queue System
//!
//! Type-enforced toast queue that ensures only one toast is visible at a time.
//!
//! ## Auto-Dismiss Behavior
//!
//! - Info, Success, Warning toasts auto-dismiss after 5 seconds (50 ticks at 100ms/tick)
//! - Error toasts do NOT auto-dismiss and must be manually dismissed (Escape key)

use std::collections::VecDeque;

/// Default duration for auto-dismissable toasts: 50 ticks = 5 seconds at 100ms/tick
const DEFAULT_TOAST_TICKS: u32 = 50;

/// Special value indicating a toast should never auto-dismiss
const NO_AUTO_DISMISS: u32 = u32::MAX;
/// Hard cap on pending toasts to prevent unbounded growth.
const MAX_PENDING_TOASTS: usize = 128;

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
}

impl QueuedToast {
    /// Create a new toast with appropriate duration based on level.
    ///
    /// - Error toasts: Never auto-dismiss (must be manually dismissed)
    /// - Other toasts: Auto-dismiss after 5 seconds (50 ticks at 100ms/tick)
    pub fn new(id: u64, message: impl Into<String>, level: ToastLevel) -> Self {
        let ticks_remaining = if level == ToastLevel::Error {
            NO_AUTO_DISMISS
        } else {
            DEFAULT_TOAST_TICKS
        };
        Self {
            id,
            message: message.into(),
            level,
            ticks_remaining,
        }
    }

    /// Create with custom duration (in ticks, 100ms per tick)
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
    pub fn auto_dismisses(&self) -> bool {
        self.level != ToastLevel::Error && self.ticks_remaining != NO_AUTO_DISMISS
    }
}

impl ToastQueue {
    /// Create a new empty toast queue
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue a toast. If no toast is active, it becomes active immediately.
    pub fn enqueue(&mut self, toast: QueuedToast) {
        if self.active.is_none() {
            self.active = Some(toast);
        } else {
            if self.pending.len() >= MAX_PENDING_TOASTS {
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
    pub fn current(&self) -> Option<&QueuedToast> {
        self.active.as_ref()
    }

    /// Check if any toast is currently active.
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
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

/// Toast notification (legacy struct for compatibility)
#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub level: ToastLevel,
    pub duration_ms: u64,
    pub created_at: u64,
    /// Ticks remaining before auto-dismiss (decremented on each Tick event)
    /// Default: 50 ticks (~5 seconds at 100ms/tick)
    /// Error toasts use u32::MAX and never auto-dismiss.
    pub ticks_remaining: u32,
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
    pub fn priority(self) -> u8 {
        match self {
            Self::Error => 3,
            Self::Warning => 2,
            Self::Info | Self::Success => 1,
        }
    }
}
