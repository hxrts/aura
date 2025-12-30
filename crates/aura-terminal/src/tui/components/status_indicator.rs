//! # Status Indicator Component
//!
//! Visual status indicators with unicode icons.

use iocraft::prelude::*;

use crate::tui::theme::{Icons, Spacing, Theme};

/// Status types for the indicator
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Online/connected
    Online,
    /// Offline/disconnected
    #[default]
    Offline,
    /// Pending/loading
    Pending,
    /// Success/verified
    Success,
    /// Warning
    Warning,
    /// Error/failed
    Error,
    /// Informational
    Info,
}

impl Status {
    /// Get the icon for this status
    pub fn icon(&self) -> &'static str {
        match self {
            Status::Online => Icons::ONLINE,
            Status::Offline => Icons::OFFLINE,
            Status::Pending => Icons::PENDING,
            Status::Success => Icons::CHECK,
            Status::Warning => Icons::WARNING,
            Status::Error => Icons::CROSS,
            Status::Info => Icons::INFO,
        }
    }

    /// Get the color for this status
    pub fn color(&self) -> Color {
        match self {
            Status::Online | Status::Success => Theme::SUCCESS,
            Status::Offline => Theme::TEXT_MUTED,
            Status::Pending => Theme::WARNING,
            Status::Warning => Theme::WARNING,
            Status::Error => Theme::ERROR,
            Status::Info => Theme::INFO,
        }
    }
}

/// Props for StatusIndicator
#[derive(Default, Props)]
pub struct StatusIndicatorProps {
    /// The status to display
    pub status: Status,
    /// Optional label text
    pub label: String,
    /// Show icon only (no label)
    pub icon_only: bool,
}

/// A status indicator with icon and optional label
#[component]
pub fn StatusIndicator(props: &StatusIndicatorProps) -> impl Into<AnyElement<'static>> {
    let icon = props.status.icon();
    let color = props.status.color();
    let label = props.label.clone();
    let show_label = !props.icon_only && !label.is_empty();

    element! {
        View(flex_direction: FlexDirection::Row, align_items: AlignItems::Center, gap: Spacing::XS) {
            Text(content: icon, color: color)
            #(if show_label {
                Some(element! {
                    Text(content: label, color: Theme::TEXT)
                })
            } else {
                None
            })
        }
    }
}

/// Props for StatusDot
#[derive(Default, Props)]
pub struct StatusDotProps {
    /// Whether the dot is "on" (filled) or "off" (empty)
    pub active: bool,
    /// Color when active (defaults to SUCCESS)
    pub active_color: Option<Color>,
    /// Color when inactive (defaults to TEXT_MUTED)
    pub inactive_color: Option<Color>,
}

/// A simple status dot (filled or empty circle)
#[component]
pub fn StatusDot(props: &StatusDotProps) -> impl Into<AnyElement<'static>> {
    let (icon, color) = if props.active {
        (Icons::ONLINE, props.active_color.unwrap_or(Theme::SUCCESS))
    } else {
        (
            Icons::OFFLINE,
            props.inactive_color.unwrap_or(Theme::TEXT_MUTED),
        )
    };

    element! {
        Text(content: icon, color: color)
    }
}

/// Props for ProgressDots
#[derive(Default, Props)]
pub struct ProgressDotsProps {
    /// Current step (0-indexed)
    pub current: usize,
    /// Total steps
    pub total: usize,
    /// Active color
    pub active_color: Option<Color>,
}

/// Progress indicator using dots
#[component]
pub fn ProgressDots(props: &ProgressDotsProps) -> impl Into<AnyElement<'static>> {
    let current = props.current;
    let total = props.total;
    let active_color = props.active_color.unwrap_or(Theme::PRIMARY);

    element! {
        View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
            #((0..total).map(|i| {
                let is_active = i <= current;
                let (icon, color) = if is_active {
                    (Icons::ONLINE, active_color)
                } else {
                    (Icons::OFFLINE, Theme::TEXT_MUTED)
                };
                element! {
                    Text(content: icon, color: color)
                }
            }))
        }
    }
}

/// Sync status for the indicator
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SyncIndicatorStatus {
    /// Not yet synced
    #[default]
    LocalOnly,
    /// Currently syncing
    Syncing,
    /// Fully synced
    Synced,
    /// Sync failed
    Failed,
}

impl SyncIndicatorStatus {
    /// Get the icon for this sync status
    pub fn icon(&self) -> &'static str {
        match self {
            SyncIndicatorStatus::LocalOnly => Icons::OFFLINE, // Empty circle
            SyncIndicatorStatus::Syncing => Icons::PENDING,   // Hourglass/loading
            SyncIndicatorStatus::Synced => Icons::ONLINE,     // Filled circle
            SyncIndicatorStatus::Failed => Icons::CROSS,      // X mark
        }
    }

    /// Get the color for this sync status
    pub fn color(&self) -> Color {
        match self {
            SyncIndicatorStatus::LocalOnly => Theme::TEXT_MUTED,
            SyncIndicatorStatus::Syncing => Theme::WARNING,
            SyncIndicatorStatus::Synced => Theme::SUCCESS,
            SyncIndicatorStatus::Failed => Theme::ERROR,
        }
    }
}

/// Props for SyncStatusIndicator
#[derive(Default, Props)]
pub struct SyncStatusIndicatorProps {
    /// The sync status to display
    pub status: SyncIndicatorStatus,
    /// Optional label text
    pub label: String,
    /// Show icon only (no label)
    pub icon_only: bool,
    /// Show progress (e.g., "2/3 peers")
    pub progress: Option<(usize, usize)>,
}

/// A sync status indicator with icon, optional label, and optional progress
#[component]
pub fn SyncStatusIndicator(props: &SyncStatusIndicatorProps) -> impl Into<AnyElement<'static>> {
    let icon = props.status.icon();
    let color = props.status.color();
    let label = props.label.clone();
    let show_label = !props.icon_only && !label.is_empty();

    // Build progress text if provided
    let progress_text = props
        .progress
        .map(|(current, total)| format!("{current}/{total}"));

    element! {
        View(flex_direction: FlexDirection::Row, align_items: AlignItems::Center, gap: Spacing::XS) {
            Text(content: icon, color: color)
            #(if show_label {
                Some(element! {
                    Text(content: label, color: Theme::TEXT)
                })
            } else {
                None
            })
            #(progress_text.map(|text| {
                element! {
                    Text(content: text, color: Theme::TEXT_MUTED)
                }
            }))
        }
    }
}

/// Props for DeliveryStatusIndicator
#[derive(Default, Props)]
pub struct DeliveryStatusIndicatorProps {
    /// Whether the message is still being sent
    pub sending: bool,
    /// Whether the message was sent (network acknowledged)
    pub sent: bool,
    /// Whether the message was delivered to recipient
    pub delivered: bool,
    /// Whether the message was read by recipient
    pub read: bool,
    /// Whether delivery failed
    pub failed: bool,
}

/// A delivery status indicator (checkmarks for message delivery)
///
/// Shows the appropriate icon based on delivery state:
/// - Sending: hourglass
/// - Sent: single gray check
/// - Delivered: double gray check
/// - Read: double blue check
/// - Failed: red X
#[component]
pub fn DeliveryStatusIndicator(
    props: &DeliveryStatusIndicatorProps,
) -> impl Into<AnyElement<'static>> {
    let (icon, color) = if props.failed {
        (Icons::CROSS, Theme::ERROR)
    } else if props.read {
        (Icons::CHECK_DOUBLE, Theme::INFO) // Blue for read
    } else if props.delivered {
        (Icons::CHECK_DOUBLE, Theme::TEXT_MUTED) // Gray for delivered
    } else if props.sent {
        (Icons::CHECK, Theme::TEXT_MUTED) // Single gray check
    } else {
        // sending or default case
        (Icons::PENDING, Theme::TEXT_MUTED)
    };

    element! {
        Text(content: icon, color: color)
    }
}
