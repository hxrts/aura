//! Toast overlay that renders in the footer region.
//!
//! Toasts are rendered with absolute positioning to overlay the footer.
//! They occupy exactly the footer region (80×3).

use super::compositor::Rect;
use super::dim;
use super::toast_trait::ToastLevel;
use crate::tui::theme::Theme;
use iocraft::prelude::*;

/// Props for ToastOverlay
#[derive(Default, Props)]
pub struct ToastOverlayProps<'a> {
    /// Toast level (info, success, warning, error)
    pub level: ToastLevel,
    /// Message to display
    pub message: &'a str,
    /// Whether the toast can be dismissed
    pub dismissable: bool,
}

/// Get color for toast level
fn level_color(level: ToastLevel) -> Color {
    match level {
        ToastLevel::Info => Theme::PRIMARY,
        ToastLevel::Success => Theme::SUCCESS,
        ToastLevel::Warning => Theme::WARNING,
        ToastLevel::Error => Theme::ERROR,
    }
}

/// Get icon for toast level
fn level_icon(level: ToastLevel) -> &'static str {
    match level {
        ToastLevel::Info => "ℹ",
        ToastLevel::Success => "✓",
        ToastLevel::Warning => "⚠",
        ToastLevel::Error => "✗",
    }
}

/// Toast overlay component that renders in the footer region.
///
/// The toast includes:
/// - Border (top)
/// - Icon + Message (2 rows)
///
/// Total: 3 rows = FOOTER_HEIGHT
#[component]
pub fn ToastOverlay<'a>(props: &ToastOverlayProps<'a>) -> impl Into<AnyElement<'a>> {
    let color = level_color(props.level);
    let icon = level_icon(props.level);
    let dismiss_hint = if props.dismissable { "[Esc]" } else { "" };

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::FOOTER_HEIGHT,
            flex_direction: FlexDirection::Column,
            background_color: Theme::BG_MODAL,
            border_style: BorderStyle::Single,
            border_edges: Edges::Top,
            border_color: color,
            overflow: Overflow::Hidden,
        ) {
            // Message area (2 rows)
            View(
                width: 100pct,
                height: dim::KEY_HINTS_HEIGHT,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding_left: 1,
                padding_right: 1,
            ) {
                // Icon + Message
                View(
                    flex_direction: FlexDirection::Row,
                    gap: 1,
                ) {
                    Text(content: icon.to_string(), color: color)
                    Text(content: props.message.to_string(), color: Theme::TEXT)
                }

                // Dismiss hint
                Text(content: dismiss_hint.to_string(), color: Theme::TEXT_MUTED)
            }
        }
    }
}

/// Simple info toast
#[derive(Default, Props)]
pub struct InfoToastProps<'a> {
    pub message: &'a str,
}

#[component]
pub fn InfoToast<'a>(props: &InfoToastProps<'a>) -> impl Into<AnyElement<'a>> {
    element! {
        ToastOverlay(level: ToastLevel::Info, message: props.message, dismissable: true)
    }
}

/// Success toast
#[derive(Default, Props)]
pub struct SuccessToastProps<'a> {
    pub message: &'a str,
}

#[component]
pub fn SuccessToast<'a>(props: &SuccessToastProps<'a>) -> impl Into<AnyElement<'a>> {
    element! {
        ToastOverlay(level: ToastLevel::Success, message: props.message, dismissable: true)
    }
}

/// Warning toast
#[derive(Default, Props)]
pub struct WarningToastProps<'a> {
    pub message: &'a str,
}

#[component]
pub fn WarningToast<'a>(props: &WarningToastProps<'a>) -> impl Into<AnyElement<'a>> {
    element! {
        ToastOverlay(level: ToastLevel::Warning, message: props.message, dismissable: true)
    }
}

/// Error toast
#[derive(Default, Props)]
pub struct ErrorToastProps<'a> {
    pub message: &'a str,
}

#[component]
pub fn ErrorToast<'a>(props: &ErrorToastProps<'a>) -> impl Into<AnyElement<'a>> {
    element! {
        ToastOverlay(level: ToastLevel::Error, message: props.message, dismissable: true)
    }
}

/// Get the absolute positioning rect for a toast overlay
pub fn toast_rect(compositor_footer: &Rect) -> Rect {
    *compositor_footer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toast_dimensions() {
        // Toast should be exactly 3 rows (FOOTER_HEIGHT)
        assert_eq!(dim::FOOTER_HEIGHT, 3);
    }

    #[test]
    fn test_toast_rect() {
        let footer = Rect::new(10, 30, 80, 3);
        let toast = toast_rect(&footer);
        assert_eq!(toast.x, 10);
        assert_eq!(toast.y, 30);
        assert_eq!(toast.width, 80);
        assert_eq!(toast.height, 3);
    }

    #[test]
    fn test_level_colors() {
        assert_eq!(level_color(ToastLevel::Info), Theme::PRIMARY);
        assert_eq!(level_color(ToastLevel::Success), Theme::SUCCESS);
        assert_eq!(level_color(ToastLevel::Warning), Theme::WARNING);
        assert_eq!(level_color(ToastLevel::Error), Theme::ERROR);
    }

    #[test]
    fn test_level_icons() {
        assert_eq!(level_icon(ToastLevel::Info), "ℹ");
        assert_eq!(level_icon(ToastLevel::Success), "✓");
        assert_eq!(level_icon(ToastLevel::Warning), "⚠");
        assert_eq!(level_icon(ToastLevel::Error), "✗");
    }
}
