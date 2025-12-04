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
