//! # Toast Component
//!
//! Notification toast messages

use iocraft::prelude::*;

use crate::tui::layout::dim;
use crate::tui::theme::Theme;

/// Toast severity level
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastLevel {
    #[default]
    Info,
    Success,
    Warning,
    Error,
    /// Special level for conflict notifications (operations rolled back due to conflicts)
    Conflict,
}

impl ToastLevel {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Info => "ℹ",
            Self::Success => "✓",
            Self::Warning => "⚠",
            Self::Error => "✗",
            Self::Conflict => "⇄", // Two-way arrows for conflict
        }
    }

    /// Alias for icon() - returns the indicator symbol for this level
    pub fn indicator(self) -> &'static str {
        self.icon()
    }

    pub fn color(self) -> Color {
        match self {
            Self::Info => Theme::SECONDARY,
            Self::Success => Theme::SUCCESS,
            Self::Warning => Theme::WARNING,
            Self::Error => Theme::ERROR,
            Self::Conflict => Theme::WARNING, // Use warning color for conflicts
        }
    }

    /// Whether this level represents a conflict that requires user attention
    pub fn is_conflict(self) -> bool {
        matches!(self, Self::Conflict)
    }
}

/// A toast message
#[derive(Clone, Debug, Default)]
pub struct ToastMessage {
    pub id: String,
    pub message: String,
    pub level: ToastLevel,
}

impl ToastMessage {
    pub fn new(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            message: message.into(),
            level: ToastLevel::Info,
        }
    }

    pub fn with_level(mut self, level: ToastLevel) -> Self {
        self.level = level;
        self
    }

    pub fn info(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(id, message).with_level(ToastLevel::Info)
    }

    pub fn success(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(id, message).with_level(ToastLevel::Success)
    }

    pub fn warning(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(id, message).with_level(ToastLevel::Warning)
    }

    pub fn error(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(id, message).with_level(ToastLevel::Error)
    }

    /// Create a conflict notification toast
    ///
    /// Used when an optimistic operation is rolled back due to a conflict
    /// with another concurrent operation.
    pub fn conflict(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(id, message).with_level(ToastLevel::Conflict)
    }

    /// Check if this toast is an error level toast
    pub fn is_error(&self) -> bool {
        matches!(self.level, ToastLevel::Error)
    }

    /// Check if this toast is a conflict notification
    pub fn is_conflict(&self) -> bool {
        matches!(self.level, ToastLevel::Conflict)
    }
}

/// Props for Toast
#[derive(Default, Props)]
pub struct ToastProps {
    pub message: String,
    pub level: ToastLevel,
}

/// A single toast notification
#[component]
pub fn Toast(props: &ToastProps) -> impl Into<AnyElement<'static>> {
    let icon = props.level.icon().to_string();
    let color = props.level.color();
    let message = props.message.clone();

    element! {
        View(
            flex_direction: FlexDirection::Row,
            gap: 1,
            padding_left: 1,
            padding_right: 1,
            border_style: BorderStyle::Round,
            border_color: color,
        ) {
            Text(content: icon, color: color)
            Text(content: message, color: Theme::TEXT)
        }
    }
}

/// Props for ToastContainer
#[derive(Default, Props)]
pub struct ToastContainerProps {
    pub toasts: Vec<ToastMessage>,
}

/// Toast notification overlay that appears over the footer.
///
/// This component uses absolute positioning to overlay the footer area
/// (rows 28-30) with the same dimensions (FOOTER_HEIGHT x TOTAL_WIDTH).
/// Rendered conditionally in app.rs when toasts are active.
#[component]
pub fn ToastContainer(props: &ToastContainerProps) -> impl Into<AnyElement<'static>> {
    let toasts = props.toasts.clone();

    // Show only the most recent toast
    let toast = match toasts.last() {
        Some(t) => t,
        None => {
            // Return empty element - no toasts to show
            return element! { View {} };
        }
    };
    let icon = toast.level.icon().to_string();
    let color = toast.level.color();

    // Truncate long messages to fit in footer width
    let max_msg_len = 60; // Leave room for icon and dismiss hint
    let message = if toast.message.len() > max_msg_len {
        format!("{}...", &toast.message[..max_msg_len - 3])
    } else {
        toast.message.clone()
    };

    // Absolute positioned overlay - same dimensions and position as footer
    element! {
        View(
            position: Position::Absolute,
            top: dim::NAV_HEIGHT + dim::MIDDLE_HEIGHT,  // Row 28 (footer start)
            left: 0u16,
            width: dim::TOTAL_WIDTH,
            height: dim::FOOTER_HEIGHT,  // Same height as footer (3 rows)
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            background_color: Theme::BG_MODAL,
            border_style: BorderStyle::Round,
            border_color: color,
            padding_left: 1,
            padding_right: 1,
            gap: 1,
            overflow: Overflow::Hidden,
        ) {
            Text(content: icon, color: color, weight: Weight::Bold, wrap: TextWrap::NoWrap)
            View(flex_grow: 1.0) {
                Text(content: message, color: Theme::TEXT, wrap: TextWrap::NoWrap)
            }
            Text(content: "[Esc] dismiss", color: Theme::TEXT_MUTED, wrap: TextWrap::NoWrap)
        }
    }
}

/// Props for StatusBar
#[derive(Default, Props)]
pub struct StatusBarProps {
    pub message: String,
    pub level: ToastLevel,
    pub visible: bool,
}

/// A status bar notification (bottom of screen)
#[component]
pub fn StatusBar(props: &StatusBarProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    let icon = props.level.icon().to_string();
    let color = props.level.color();
    let message = props.message.clone();

    element! {
        View(
            flex_direction: FlexDirection::Row,
            gap: 1,
            padding_left: 1,
            padding_right: 1,
            border_style: BorderStyle::Round,
            border_color: color,
        ) {
            Text(content: icon, color: color)
            Text(content: message, color: Theme::TEXT)
        }
    }
}
