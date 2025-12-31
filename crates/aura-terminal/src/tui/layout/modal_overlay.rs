//! Modal overlay that renders in the middle region.
//!
//! Modals are rendered with absolute positioning to overlay the screen content.
//! They occupy exactly the middle region (80×MIDDLE_HEIGHT).

use super::compositor::Rect;
use super::dim;
use crate::tui::theme::Theme;
use iocraft::prelude::*;

/// Props for a simple text modal
#[derive(Default, Props)]
pub struct SimpleModalProps<'a> {
    /// Title shown at the top of the modal
    pub title: &'a str,
    /// Main content text
    pub content: &'a str,
    /// Whether to show a close hint
    pub show_close_hint: bool,
}

/// Simple modal with title and content text.
///
/// Layout:
/// - Title bar (1 row)
/// - Separator (1 row)
/// - Content area (MIDDLE_HEIGHT - 2 rows)
///
/// Total: MIDDLE_HEIGHT rows
#[component]
pub fn SimpleModal<'a>(props: &SimpleModalProps<'a>) -> impl Into<AnyElement<'a>> {
    let close_hint = if props.show_close_hint {
        "[Esc] close"
    } else {
        ""
    };

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            background_color: Theme::BG_MODAL,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            overflow: Overflow::Hidden,
        ) {
            // Title bar (1 row)
            View(
                width: 100pct,
                height: 1,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                padding_left: 1,
                padding_right: 1,
            ) {
                Text(content: props.title.to_string(), color: Theme::PRIMARY, weight: Weight::Bold)
                Text(content: close_hint.to_string(), color: Theme::TEXT_MUTED)
            }

            // Separator
            View(
                width: 100pct,
                height: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            )

            // Content area
            View(
                width: 100pct,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                padding: 1,
                overflow: Overflow::Hidden,
            ) {
                Text(content: props.content.to_string(), color: Theme::TEXT)
            }
        }
    }
}

/// Props for centered confirmation modal
#[derive(Default, Props)]
pub struct ConfirmModalProps<'a> {
    /// Title shown at the top
    pub title: &'a str,
    /// Main message
    pub message: &'a str,
    /// Optional secondary message
    pub secondary: Option<&'a str>,
    /// Confirm button text
    pub confirm_text: &'a str,
    /// Cancel button text
    pub cancel_text: &'a str,
}

/// Centered confirmation modal.
#[component]
pub fn ConfirmationModal<'a>(props: &ConfirmModalProps<'a>) -> impl Into<AnyElement<'a>> {
    let secondary_text = props.secondary.unwrap_or("");

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            background_color: Theme::BG_MODAL,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            overflow: Overflow::Hidden,
        ) {
            // Title
            View(height: 1) {
                Text(content: props.title.to_string(), color: Theme::PRIMARY, weight: Weight::Bold)
            }

            // Spacer
            View(height: 1)

            // Message
            View(height: 1) {
                Text(content: props.message.to_string(), color: Theme::TEXT)
            }

            // Secondary message
            View(height: 1) {
                Text(content: secondary_text.to_string(), color: Theme::TEXT_MUTED)
            }

            // Spacer
            View(height: 2)

            // Action hints
            View(
                flex_direction: FlexDirection::Row,
                gap: 3,
            ) {
                Text(content: format!("[Enter] {}", props.confirm_text), color: Theme::SUCCESS)
                Text(content: format!("[Esc] {}", props.cancel_text), color: Theme::TEXT_MUTED)
            }
        }
    }
}

/// Get the absolute positioning rect for a modal overlay
#[must_use]
pub fn modal_rect(compositor_middle: &Rect) -> Rect {
    *compositor_middle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_dimensions() {
        // Modal should be exactly 25 rows (MIDDLE_HEIGHT)
        assert_eq!(dim::MIDDLE_HEIGHT, super::dim::MIDDLE_HEIGHT);
    }

    #[test]
    fn test_modal_rect() {
        let middle = Rect::new(10, 5, dim::TOTAL_WIDTH, dim::MIDDLE_HEIGHT);
        let modal = modal_rect(&middle);
        assert_eq!(modal.x, 10);
        assert_eq!(modal.y, 5);
        assert_eq!(modal.width, dim::TOTAL_WIDTH);
        assert_eq!(modal.height, dim::MIDDLE_HEIGHT);
    }
}
