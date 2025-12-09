//! # Toast Component
//!
//! Notification toast messages

use iocraft::prelude::*;

use crate::tui::theme::Theme;

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
    pub fn icon(self) -> &'static str {
        match self {
            Self::Info => "ℹ",
            Self::Success => "✓",
            Self::Warning => "⚠",
            Self::Error => "✗",
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
        }
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

    /// Check if this toast is an error level toast
    pub fn is_error(&self) -> bool {
        matches!(self.level, ToastLevel::Error)
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
            background_color: Theme::BG_DARK,
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

/// Container for toast notifications (positioned at top-right)
#[component]
pub fn ToastContainer(props: &ToastContainerProps) -> impl Into<AnyElement<'static>> {
    let toasts = props.toasts.clone();

    if toasts.is_empty() {
        return element! {
            View {}
        };
    }

    element! {
        View(
            position: Position::Absolute,
            top: 1,
            right: 1,
            flex_direction: FlexDirection::Column,
            gap: 1,
            min_width: 30,
        ) {
            #(toasts.iter().map(|t| {
                let message = t.message.clone();
                let level = t.level;
                element! {
                    View {
                        Toast(message: message, level: level)
                    }
                }
            }).collect::<Vec<_>>())
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
            background_color: Theme::BG_DARK,
            border_style: BorderStyle::Single,
            border_edges: Edges::Top,
            border_color: color,
        ) {
            Text(content: icon, color: color)
            Text(content: message, color: Theme::TEXT)
        }
    }
}
