//! Toast Notification Lifecycle
//!
//! Portable constants and types for toast notification behavior across all frontends.
//!
//! ## Auto-Dismiss Behavior
//!
//! - Info, Success, Warning toasts auto-dismiss after 5 seconds (50 ticks at 100ms/tick)
//! - Error toasts do NOT auto-dismiss and must be manually dismissed

/// Default tick rate for toast timers (100ms per tick)
pub const TOAST_TICK_RATE_MS: u64 = 100;

/// Default duration for auto-dismissable toasts: 50 ticks = 5 seconds at 100ms/tick
pub const DEFAULT_TOAST_TICKS: u32 = 50;

/// Default toast duration in milliseconds (5 seconds)
pub const DEFAULT_TOAST_DURATION_MS: u64 = 5000;

/// Special value indicating a toast should never auto-dismiss
pub const NO_AUTO_DISMISS: u32 = u32::MAX;

/// Hard cap on pending toasts to prevent unbounded memory growth
pub const MAX_PENDING_TOASTS: usize = 128;

/// Hard cap on pending modals to prevent unbounded memory growth
pub const MAX_PENDING_MODALS: usize = 64;

/// Toast severity level
///
/// Determines visual styling and auto-dismiss behavior.
/// Note: Error toasts never auto-dismiss and must be manually dismissed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToastLevel {
    /// Informational message - neutral styling, auto-dismisses
    #[default]
    Info,
    /// Success message - positive styling, auto-dismisses
    Success,
    /// Warning message - caution styling, auto-dismisses
    Warning,
    /// Error message - critical styling, does NOT auto-dismiss
    Error,
}

impl ToastLevel {
    /// Get the dismissal priority (higher = dismiss first on Escape)
    ///
    /// Priority ordering: Error (3) > Warning (2) > Info/Success (1)
    /// This ensures error toasts are dismissed first when user presses Escape.
    #[must_use]
    pub fn priority(self) -> u8 {
        match self {
            Self::Error => 3,
            Self::Warning => 2,
            Self::Info | Self::Success => 1,
        }
    }

    /// Check if this level auto-dismisses by default
    ///
    /// Error toasts require manual dismissal; all others auto-dismiss.
    #[must_use]
    pub fn auto_dismisses(self) -> bool {
        !matches!(self, Self::Error)
    }

    /// Get the default duration in ticks for this level
    ///
    /// Returns `NO_AUTO_DISMISS` for Error, `DEFAULT_TOAST_TICKS` otherwise.
    #[must_use]
    pub fn default_ticks(self) -> u32 {
        if self.auto_dismisses() {
            DEFAULT_TOAST_TICKS
        } else {
            NO_AUTO_DISMISS
        }
    }

    /// Get a display label for this level
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Success => "Success",
            Self::Warning => "Warning",
            Self::Error => "Error",
        }
    }

    /// Get a lowercase label for this level (for logging/serialization)
    #[must_use]
    pub fn label_lowercase(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// Convert milliseconds to ticks (at 100ms per tick)
#[must_use]
pub const fn ms_to_ticks(ms: u64) -> u32 {
    (ms / TOAST_TICK_RATE_MS) as u32
}

/// Convert ticks to milliseconds (at 100ms per tick)
#[must_use]
pub const fn ticks_to_ms(ticks: u32) -> u64 {
    ticks as u64 * TOAST_TICK_RATE_MS
}

/// Calculate ticks for a custom duration, respecting NO_AUTO_DISMISS
///
/// Pass `None` for no auto-dismiss behavior.
#[must_use]
pub fn duration_ticks(ms: Option<u64>) -> u32 {
    match ms {
        Some(ms) => ms_to_ticks(ms),
        None => NO_AUTO_DISMISS,
    }
}

/// Check if a tick count represents auto-dismiss behavior
#[must_use]
pub fn will_auto_dismiss(ticks: u32) -> bool {
    ticks != NO_AUTO_DISMISS
}

/// Check if a toast at the given level with given ticks should auto-dismiss
///
/// A toast auto-dismisses if:
/// 1. Its level auto-dismisses (not Error)
/// 2. Its ticks value is not NO_AUTO_DISMISS
#[must_use]
pub fn should_auto_dismiss(level: ToastLevel, ticks: u32) -> bool {
    level.auto_dismisses() && will_auto_dismiss(ticks)
}

// ============================================================================
// Modal Priority System
// ============================================================================

/// Modal priority level for determining interruption behavior
///
/// When a high-priority modal is queued while another modal is active,
/// this determines whether to interrupt the current modal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModalPriority {
    /// Normal priority - waits in queue (default)
    #[default]
    Normal,
    /// High priority - may interrupt normal modals
    High,
    /// Critical priority - interrupts everything except other critical modals
    Critical,
}

impl ModalPriority {
    /// Get a display label for this priority
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::High => "High",
            Self::Critical => "Critical",
        }
    }

    /// Get a lowercase label for this priority (for logging/serialization)
    #[must_use]
    pub fn label_lowercase(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

/// Check if an incoming modal should interrupt the current modal
///
/// Interruption rules:
/// - Critical interrupts Normal and High
/// - High interrupts Normal
/// - Same priority does not interrupt
/// - Normal never interrupts
///
/// # Arguments
/// * `current` - Priority of the currently active modal
/// * `incoming` - Priority of the modal wanting to be shown
///
/// # Returns
/// `true` if the incoming modal should replace the current modal
#[must_use]
pub fn should_interrupt_modal(current: ModalPriority, incoming: ModalPriority) -> bool {
    // Interruption only happens when incoming is strictly higher priority
    incoming > current
}

/// Check if a modal at the given priority can be dismissed by user action
///
/// Critical modals typically cannot be dismissed by normal means (Escape key).
#[must_use]
pub fn modal_can_user_dismiss(priority: ModalPriority) -> bool {
    // Critical modals require explicit confirmation, not casual dismissal
    priority != ModalPriority::Critical
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_ticks_constant() {
        // 50 ticks * 100ms = 5000ms = 5 seconds
        assert_eq!(DEFAULT_TOAST_TICKS, 50);
        assert_eq!(ticks_to_ms(DEFAULT_TOAST_TICKS), 5000);
    }

    #[test]
    fn test_no_auto_dismiss_constant() {
        assert_eq!(NO_AUTO_DISMISS, u32::MAX);
    }

    #[test]
    fn test_max_pending_toasts() {
        assert_eq!(MAX_PENDING_TOASTS, 128);
    }

    #[test]
    fn test_toast_level_auto_dismisses() {
        assert!(ToastLevel::Info.auto_dismisses());
        assert!(ToastLevel::Success.auto_dismisses());
        assert!(ToastLevel::Warning.auto_dismisses());
        assert!(!ToastLevel::Error.auto_dismisses());
    }

    #[test]
    fn test_toast_level_default_ticks() {
        assert_eq!(ToastLevel::Info.default_ticks(), DEFAULT_TOAST_TICKS);
        assert_eq!(ToastLevel::Success.default_ticks(), DEFAULT_TOAST_TICKS);
        assert_eq!(ToastLevel::Warning.default_ticks(), DEFAULT_TOAST_TICKS);
        assert_eq!(ToastLevel::Error.default_ticks(), NO_AUTO_DISMISS);
    }

    #[test]
    fn test_toast_level_priority() {
        assert!(ToastLevel::Error.priority() > ToastLevel::Warning.priority());
        assert!(ToastLevel::Warning.priority() > ToastLevel::Info.priority());
        assert_eq!(ToastLevel::Info.priority(), ToastLevel::Success.priority());
    }

    #[test]
    fn test_ms_to_ticks_conversion() {
        assert_eq!(ms_to_ticks(0), 0);
        assert_eq!(ms_to_ticks(100), 1);
        assert_eq!(ms_to_ticks(500), 5);
        assert_eq!(ms_to_ticks(5000), 50);
        assert_eq!(ms_to_ticks(10000), 100);
    }

    #[test]
    fn test_ticks_to_ms_conversion() {
        assert_eq!(ticks_to_ms(0), 0);
        assert_eq!(ticks_to_ms(1), 100);
        assert_eq!(ticks_to_ms(50), 5000);
        assert_eq!(ticks_to_ms(100), 10000);
    }

    #[test]
    fn test_duration_ticks() {
        assert_eq!(duration_ticks(Some(5000)), 50);
        assert_eq!(duration_ticks(Some(100)), 1);
        assert_eq!(duration_ticks(None), NO_AUTO_DISMISS);
    }

    #[test]
    fn test_will_auto_dismiss() {
        assert!(will_auto_dismiss(50));
        assert!(will_auto_dismiss(1));
        assert!(will_auto_dismiss(0));
        assert!(!will_auto_dismiss(NO_AUTO_DISMISS));
    }

    #[test]
    fn test_should_auto_dismiss() {
        // Info with normal ticks - should auto-dismiss
        assert!(should_auto_dismiss(ToastLevel::Info, DEFAULT_TOAST_TICKS));

        // Error with normal ticks - should NOT auto-dismiss (error level)
        assert!(!should_auto_dismiss(ToastLevel::Error, DEFAULT_TOAST_TICKS));

        // Info with NO_AUTO_DISMISS - should NOT auto-dismiss (explicit no-dismiss)
        assert!(!should_auto_dismiss(ToastLevel::Info, NO_AUTO_DISMISS));

        // Error with NO_AUTO_DISMISS - should NOT auto-dismiss (both conditions)
        assert!(!should_auto_dismiss(ToastLevel::Error, NO_AUTO_DISMISS));
    }

    #[test]
    fn test_toast_level_labels() {
        assert_eq!(ToastLevel::Info.label(), "Info");
        assert_eq!(ToastLevel::Success.label(), "Success");
        assert_eq!(ToastLevel::Warning.label(), "Warning");
        assert_eq!(ToastLevel::Error.label(), "Error");

        assert_eq!(ToastLevel::Info.label_lowercase(), "info");
        assert_eq!(ToastLevel::Success.label_lowercase(), "success");
        assert_eq!(ToastLevel::Warning.label_lowercase(), "warning");
        assert_eq!(ToastLevel::Error.label_lowercase(), "error");
    }

    // ========================================================================
    // Modal Priority Tests
    // ========================================================================

    #[test]
    fn test_max_pending_modals() {
        assert_eq!(MAX_PENDING_MODALS, 64);
    }

    #[test]
    fn test_modal_priority_ordering() {
        assert!(ModalPriority::Normal < ModalPriority::High);
        assert!(ModalPriority::High < ModalPriority::Critical);
        assert!(ModalPriority::Normal < ModalPriority::Critical);
    }

    #[test]
    fn test_should_interrupt_modal() {
        use ModalPriority::*;

        // Critical interrupts everything except Critical
        assert!(should_interrupt_modal(Normal, Critical));
        assert!(should_interrupt_modal(High, Critical));
        assert!(!should_interrupt_modal(Critical, Critical)); // same priority

        // High interrupts Normal only
        assert!(should_interrupt_modal(Normal, High));
        assert!(!should_interrupt_modal(High, High)); // same priority
        assert!(!should_interrupt_modal(Critical, High)); // lower incoming

        // Normal never interrupts
        assert!(!should_interrupt_modal(Normal, Normal)); // same priority
        assert!(!should_interrupt_modal(High, Normal)); // lower incoming
        assert!(!should_interrupt_modal(Critical, Normal)); // lower incoming
    }

    #[test]
    fn test_modal_can_user_dismiss() {
        assert!(modal_can_user_dismiss(ModalPriority::Normal));
        assert!(modal_can_user_dismiss(ModalPriority::High));
        assert!(!modal_can_user_dismiss(ModalPriority::Critical));
    }

    #[test]
    fn test_modal_priority_labels() {
        assert_eq!(ModalPriority::Normal.label(), "Normal");
        assert_eq!(ModalPriority::High.label(), "High");
        assert_eq!(ModalPriority::Critical.label(), "Critical");

        assert_eq!(ModalPriority::Normal.label_lowercase(), "normal");
        assert_eq!(ModalPriority::High.label_lowercase(), "high");
        assert_eq!(ModalPriority::Critical.label_lowercase(), "critical");
    }
}
