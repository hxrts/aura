//! # Display Utilities
//!
//! Portable display formatting utilities for consistent UI across frontends.
//!
//! This module provides formatting functions for:
//! - Relative time display ("2m ago", "1h ago")
//! - Selection indicators
//! - Other common display patterns

// ============================================================================
// Relative Time Formatting
// ============================================================================

/// Format an elapsed time as a relative time string.
///
/// # Arguments
/// * `elapsed_secs` - Elapsed time in seconds
///
/// # Returns
/// A human-readable relative time string.
///
/// # Examples
/// ```rust
/// use aura_app::views::display::format_relative_time;
///
/// assert_eq!(format_relative_time(30), "just now");
/// assert_eq!(format_relative_time(120), "2m ago");
/// assert_eq!(format_relative_time(7200), "2h ago");
/// assert_eq!(format_relative_time(172800), "2d ago");
/// ```
#[must_use]
pub fn format_relative_time(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        "just now".to_string()
    } else if elapsed_secs < 3600 {
        format!("{}m ago", elapsed_secs / 60)
    } else if elapsed_secs < 86400 {
        format!("{}h ago", elapsed_secs / 3600)
    } else {
        format!("{}d ago", elapsed_secs / 86400)
    }
}

/// Format an elapsed time in milliseconds as a relative time string.
///
/// Convenience wrapper around `format_relative_time` for millisecond values.
///
/// # Arguments
/// * `elapsed_ms` - Elapsed time in milliseconds
///
/// # Returns
/// A human-readable relative time string.
#[must_use]
pub fn format_relative_time_ms(elapsed_ms: u64) -> String {
    format_relative_time(elapsed_ms / 1000)
}

/// Format a relative time from two timestamps.
///
/// # Arguments
/// * `now_ms` - Current timestamp in milliseconds
/// * `ts_ms` - Target timestamp in milliseconds
///
/// # Returns
/// A human-readable relative time string showing how long ago `ts_ms` was.
#[must_use]
pub fn format_relative_time_from(now_ms: u64, ts_ms: u64) -> String {
    let elapsed_ms = now_ms.saturating_sub(ts_ms);
    format_relative_time_ms(elapsed_ms)
}

// ============================================================================
// Selection Indicators
// ============================================================================

/// Indicator shown for selected items.
pub const SELECTED_INDICATOR: &str = "➤ ";

/// Indicator shown for unselected items (padding).
pub const UNSELECTED_INDICATOR: &str = "  ";

/// Get the appropriate selection indicator.
///
/// # Arguments
/// * `selected` - Whether the item is selected
///
/// # Returns
/// The selection indicator string.
///
/// # Examples
/// ```rust
/// use aura_app::views::display::selection_indicator;
///
/// assert_eq!(selection_indicator(true), "➤ ");
/// assert_eq!(selection_indicator(false), "  ");
/// ```
#[must_use]
pub fn selection_indicator(selected: bool) -> &'static str {
    if selected {
        SELECTED_INDICATOR
    } else {
        UNSELECTED_INDICATOR
    }
}

// ============================================================================
// Time Constants
// ============================================================================

/// Seconds per minute.
pub const SECONDS_PER_MINUTE: u64 = 60;

/// Seconds per hour.
pub const SECONDS_PER_HOUR: u64 = 3600;

/// Seconds per day.
pub const SECONDS_PER_DAY: u64 = 86400;

/// Milliseconds per second.
pub const MS_PER_SECOND: u64 = 1000;

/// Milliseconds per minute.
pub const MS_PER_MINUTE: u64 = 60_000;

/// Milliseconds per hour.
pub const MS_PER_HOUR: u64 = 3_600_000;

// ============================================================================
// Absolute Timestamp Formatting
// ============================================================================

/// Format a timestamp (ms since epoch) as "HH:MM" for display.
///
/// Extracts the hour and minute components from a Unix epoch timestamp
/// and formats them as a 24-hour time string.
///
/// # Arguments
/// * `ts_ms` - Timestamp in milliseconds since Unix epoch
///
/// # Returns
/// A string in "HH:MM" format, or empty string for ts_ms == 0.
///
/// # Examples
/// ```rust
/// use aura_app::views::display::format_timestamp;
///
/// // 00:00 (midnight)
/// assert_eq!(format_timestamp(0), "");
///
/// // 12:30 on epoch day
/// let ts = 12 * 3_600_000 + 30 * 60_000;
/// assert_eq!(format_timestamp(ts), "12:30");
///
/// // 23:59
/// let ts = 23 * 3_600_000 + 59 * 60_000;
/// assert_eq!(format_timestamp(ts), "23:59");
/// ```
#[must_use]
pub fn format_timestamp(ts_ms: u64) -> String {
    if ts_ms == 0 {
        return String::new();
    }
    let hours = (ts_ms / MS_PER_HOUR) % 24;
    let minutes = (ts_ms / MS_PER_MINUTE) % 60;
    format!("{hours:02}:{minutes:02}")
}

/// Format a timestamp with seconds as "HH:MM:SS".
///
/// Extended version of `format_timestamp` that includes seconds.
///
/// # Arguments
/// * `ts_ms` - Timestamp in milliseconds since Unix epoch
///
/// # Returns
/// A string in "HH:MM:SS" format, or empty string for ts_ms == 0.
///
/// # Examples
/// ```rust
/// use aura_app::views::display::format_timestamp_full;
///
/// // 12:30:45
/// let ts = 12 * 3_600_000 + 30 * 60_000 + 45 * 1_000;
/// assert_eq!(format_timestamp_full(ts), "12:30:45");
/// ```
#[must_use]
pub fn format_timestamp_full(ts_ms: u64) -> String {
    if ts_ms == 0 {
        return String::new();
    }
    let hours = (ts_ms / MS_PER_HOUR) % 24;
    let minutes = (ts_ms / MS_PER_MINUTE) % 60;
    let seconds = (ts_ms / MS_PER_SECOND) % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

// ============================================================================
// Status Severity
// ============================================================================

/// Severity level for status indicators.
///
/// Used for consistent color/styling across frontends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StatusSeverity {
    /// Success state (e.g., synced, connected)
    Success,
    /// Warning state (e.g., syncing, degraded)
    Warning,
    /// Error state (e.g., disconnected, failed)
    Error,
    /// Informational state (neutral)
    #[default]
    Info,
    /// Disabled/inactive state
    Disabled,
}

impl StatusSeverity {
    /// Check if this severity indicates a problem.
    #[must_use]
    pub fn is_problem(&self) -> bool {
        matches!(self, Self::Warning | Self::Error)
    }

    /// Check if this severity indicates success.
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }
}

// ============================================================================
// Network Status Display
// ============================================================================

use crate::signal_defs::NetworkStatus;

/// Format network status as a display string.
///
/// # Arguments
/// * `status` - The network status to format
/// * `now_ms` - Optional current timestamp for relative time formatting
///
/// # Returns
/// A human-readable status string.
///
/// # Examples
/// ```rust,ignore
/// use aura_app::views::display::format_network_status;
/// use aura_app::signal_defs::NetworkStatus;
///
/// assert_eq!(format_network_status(&NetworkStatus::Disconnected, None), "Disconnected");
/// assert_eq!(format_network_status(&NetworkStatus::Syncing, None), "Syncing...");
/// ```
#[must_use]
pub fn format_network_status(status: &NetworkStatus, now_ms: Option<u64>) -> String {
    match status {
        NetworkStatus::Disconnected => "Disconnected".to_string(),
        NetworkStatus::NoPeers => "No contacts".to_string(),
        NetworkStatus::Syncing => "Syncing...".to_string(),
        NetworkStatus::Synced { last_sync_ms } => {
            if let Some(now) = now_ms {
                format!("Synced {}", format_relative_time_from(now, *last_sync_ms))
            } else {
                "Synced".to_string()
            }
        }
    }
}

/// Get the severity level for a network status.
///
/// # Arguments
/// * `status` - The network status to classify
///
/// # Returns
/// The appropriate severity for styling.
#[must_use]
pub fn network_status_severity(status: &NetworkStatus) -> StatusSeverity {
    match status {
        NetworkStatus::Disconnected => StatusSeverity::Error,
        NetworkStatus::NoPeers => StatusSeverity::Warning,
        NetworkStatus::Syncing => StatusSeverity::Warning,
        NetworkStatus::Synced { .. } => StatusSeverity::Success,
    }
}

/// Format network status with severity information.
///
/// Returns both the display string and severity for styling.
#[must_use]
pub fn format_network_status_with_severity(
    status: &NetworkStatus,
    now_ms: Option<u64>,
) -> (String, StatusSeverity) {
    (
        format_network_status(status, now_ms),
        network_status_severity(status),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Relative Time Tests
    // ========================================================================

    #[test]
    fn test_format_relative_time_just_now() {
        assert_eq!(format_relative_time(0), "just now");
        assert_eq!(format_relative_time(30), "just now");
        assert_eq!(format_relative_time(59), "just now");
    }

    #[test]
    fn test_format_relative_time_minutes() {
        assert_eq!(format_relative_time(60), "1m ago");
        assert_eq!(format_relative_time(120), "2m ago");
        assert_eq!(format_relative_time(3599), "59m ago");
    }

    #[test]
    fn test_format_relative_time_hours() {
        assert_eq!(format_relative_time(3600), "1h ago");
        assert_eq!(format_relative_time(7200), "2h ago");
        assert_eq!(format_relative_time(86399), "23h ago");
    }

    #[test]
    fn test_format_relative_time_days() {
        assert_eq!(format_relative_time(86400), "1d ago");
        assert_eq!(format_relative_time(172800), "2d ago");
        assert_eq!(format_relative_time(604800), "7d ago");
    }

    #[test]
    fn test_format_relative_time_boundary_exactly_60() {
        // At exactly 60 seconds, should show 1m
        assert_eq!(format_relative_time(60), "1m ago");
    }

    #[test]
    fn test_format_relative_time_ms() {
        assert_eq!(format_relative_time_ms(30_000), "just now");
        assert_eq!(format_relative_time_ms(120_000), "2m ago");
        assert_eq!(format_relative_time_ms(7_200_000), "2h ago");
    }

    #[test]
    fn test_format_relative_time_from() {
        let now = 100_000u64;
        let ts = 70_000u64; // 30 seconds ago
        assert_eq!(format_relative_time_from(now, ts), "just now");

        let ts2 = 0u64; // 100 seconds ago
        assert_eq!(format_relative_time_from(now, ts2), "1m ago");
    }

    #[test]
    fn test_format_relative_time_from_saturates() {
        // ts > now should not underflow
        let now = 100u64;
        let ts = 200u64;
        assert_eq!(format_relative_time_from(now, ts), "just now");
    }

    // ========================================================================
    // Selection Indicator Tests
    // ========================================================================

    #[test]
    fn test_selection_indicator() {
        assert_eq!(selection_indicator(true), "➤ ");
        assert_eq!(selection_indicator(false), "  ");
    }

    #[test]
    fn test_selection_indicator_lengths() {
        // Both indicators should have the same display width
        assert_eq!(SELECTED_INDICATOR.chars().count(), 2);
        assert_eq!(UNSELECTED_INDICATOR.chars().count(), 2);
    }

    // ========================================================================
    // Status Severity Tests
    // ========================================================================

    #[test]
    fn test_status_severity_is_problem() {
        assert!(!StatusSeverity::Success.is_problem());
        assert!(StatusSeverity::Warning.is_problem());
        assert!(StatusSeverity::Error.is_problem());
        assert!(!StatusSeverity::Info.is_problem());
        assert!(!StatusSeverity::Disabled.is_problem());
    }

    #[test]
    fn test_status_severity_is_success() {
        assert!(StatusSeverity::Success.is_success());
        assert!(!StatusSeverity::Warning.is_success());
        assert!(!StatusSeverity::Error.is_success());
        assert!(!StatusSeverity::Info.is_success());
        assert!(!StatusSeverity::Disabled.is_success());
    }

    // ========================================================================
    // Network Status Tests
    // ========================================================================

    #[test]
    fn test_format_network_status_disconnected() {
        assert_eq!(
            format_network_status(&NetworkStatus::Disconnected, None),
            "Disconnected"
        );
    }

    #[test]
    fn test_format_network_status_no_peers() {
        assert_eq!(
            format_network_status(&NetworkStatus::NoPeers, None),
            "No contacts"
        );
    }

    #[test]
    fn test_format_network_status_syncing() {
        assert_eq!(
            format_network_status(&NetworkStatus::Syncing, None),
            "Syncing..."
        );
    }

    #[test]
    fn test_format_network_status_synced_no_time() {
        let status = NetworkStatus::Synced { last_sync_ms: 1000 };
        assert_eq!(format_network_status(&status, None), "Synced");
    }

    #[test]
    fn test_format_network_status_synced_with_time() {
        let now_ms = 100_000u64;
        let last_sync_ms = 70_000u64; // 30 seconds ago
        let status = NetworkStatus::Synced { last_sync_ms };
        assert_eq!(
            format_network_status(&status, Some(now_ms)),
            "Synced just now"
        );
    }

    #[test]
    fn test_network_status_severity() {
        assert_eq!(
            network_status_severity(&NetworkStatus::Disconnected),
            StatusSeverity::Error
        );
        assert_eq!(
            network_status_severity(&NetworkStatus::NoPeers),
            StatusSeverity::Warning
        );
        assert_eq!(
            network_status_severity(&NetworkStatus::Syncing),
            StatusSeverity::Warning
        );
        assert_eq!(
            network_status_severity(&NetworkStatus::Synced { last_sync_ms: 0 }),
            StatusSeverity::Success
        );
    }

    #[test]
    fn test_format_network_status_with_severity() {
        let (text, severity) =
            format_network_status_with_severity(&NetworkStatus::Disconnected, None);
        assert_eq!(text, "Disconnected");
        assert_eq!(severity, StatusSeverity::Error);
    }

    // ========================================================================
    // Timestamp Formatting Tests
    // ========================================================================

    #[test]
    fn test_format_timestamp_zero() {
        assert_eq!(format_timestamp(0), "");
    }

    #[test]
    fn test_format_timestamp_midnight() {
        // 24 hours in ms = 86,400,000
        // Modulo 24 hours should give 00:00
        let ts = 24 * MS_PER_HOUR;
        assert_eq!(format_timestamp(ts), "00:00");
    }

    #[test]
    fn test_format_timestamp_noon() {
        let ts = 12 * MS_PER_HOUR;
        assert_eq!(format_timestamp(ts), "12:00");
    }

    #[test]
    fn test_format_timestamp_with_minutes() {
        let ts = 14 * MS_PER_HOUR + 30 * MS_PER_MINUTE;
        assert_eq!(format_timestamp(ts), "14:30");
    }

    #[test]
    fn test_format_timestamp_late_night() {
        let ts = 23 * MS_PER_HOUR + 59 * MS_PER_MINUTE;
        assert_eq!(format_timestamp(ts), "23:59");
    }

    #[test]
    fn test_format_timestamp_wraps_24h() {
        // 25 hours should wrap to 01:00
        let ts = 25 * MS_PER_HOUR;
        assert_eq!(format_timestamp(ts), "01:00");
    }

    #[test]
    fn test_format_timestamp_full_zero() {
        assert_eq!(format_timestamp_full(0), "");
    }

    #[test]
    fn test_format_timestamp_full_with_seconds() {
        let ts = 12 * MS_PER_HOUR + 30 * MS_PER_MINUTE + 45 * MS_PER_SECOND;
        assert_eq!(format_timestamp_full(ts), "12:30:45");
    }

    #[test]
    fn test_format_timestamp_full_single_digits() {
        let ts = 1 * MS_PER_HOUR + 5 * MS_PER_MINUTE + 9 * MS_PER_SECOND;
        assert_eq!(format_timestamp_full(ts), "01:05:09");
    }
}
