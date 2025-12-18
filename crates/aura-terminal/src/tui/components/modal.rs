//! # Modal Components
//!
//! Modal dialog overlays that exactly fill the middle panel region (80×25).
//! All modals use fixed dimensions matching the middle screen panel.
//!
//! ## ModalFrame
//!
//! ALL modals MUST use ModalFrame to ensure consistent positioning.
//! The frame positions the modal to exactly fill the middle panel region (80×25).
//!
//! Modals should be rendered at ROOT level in app.rs, wrapped in ModalFrame:
//!
//! ```ignore
//! #(if modal_visible {
//!     Some(element! {
//!         ModalFrame {
//!             MyModalContent(...)
//!         }
//!     })
//! } else {
//!     None
//! })
//! ```
//!
//! **Important:**
//! - ALL modals must be rendered at root level in app.rs
//! - Do NOT render modals inside screen components
//! - This ensures consistent positioning across all modals

use iocraft::prelude::*;

use crate::tui::layout::dim;
use crate::tui::theme::{Borders, Spacing, Theme};

// =============================================================================
// Modal Frame
// =============================================================================

/// Props for ModalFrame
#[derive(Default, Props)]
pub struct ModalFrameProps<'a> {
    /// Child content (the modal body)
    pub children: Vec<AnyElement<'a>>,
}

/// Unified modal frame that positions content in the middle panel region.
///
/// This component handles the absolute positioning so that all modals
/// appear at exactly the same location: filling the middle panel (rows 3-27).
///
/// Use this as the outermost wrapper for ALL modal components.
/// Children should use `ModalContent` as their root element.
#[component]
pub fn ModalFrame<'a>(props: &mut ModalFrameProps<'a>) -> impl Into<AnyElement<'a>> {
    element! {
        View(
            position: Position::Absolute,
            top: dim::NAV_HEIGHT,
            left: 0u16,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            overflow: Overflow::Hidden,
            // Ensure modal frame has solid background to prevent content bleeding through
            background_color: Theme::BG_MODAL,
        ) {
            #(&mut props.children)
        }
    }
}

// =============================================================================
// Modal Content - Compile-time safe modal body wrapper
// =============================================================================

/// Props for ModalContent - intentionally does NOT include position props
/// to prevent the nested absolute positioning bug at compile time.
///
/// Use this as the root element for all modal body content.
#[derive(Default, Props)]
pub struct ModalContentProps<'a> {
    /// Child content
    pub children: Vec<AnyElement<'a>>,
    /// Flex direction for content layout
    pub flex_direction: FlexDirection,
    /// Background color (defaults to BG_MODAL)
    pub background_color: Option<Color>,
    /// Border style
    pub border_style: BorderStyle,
    /// Border color (defaults to BORDER_FOCUS)
    pub border_color: Option<Color>,
    /// Whether to hide overflow (defaults to true)
    pub overflow_hidden: bool,
    /// Justify content (defaults to FlexStart)
    pub justify_content: Option<JustifyContent>,
    /// Align items (defaults to Stretch)
    pub align_items: Option<AlignItems>,
}

/// Modal content wrapper that fills its ModalFrame container.
///
/// **IMPORTANT**: Use this component as the root element for ALL modal bodies.
/// It automatically handles sizing (100% width and height) and prevents
/// the nested absolute positioning bug at compile time by not exposing
/// position props.
///
/// # Example
/// ```ignore
/// element! {
///     ModalFrame {
///         ModalContent(
///             flex_direction: FlexDirection::Column,
///             border_style: BorderStyle::Round,
///             border_color: Theme::PRIMARY,
///         ) {
///             // Your modal content here
///         }
///     }
/// }
/// ```
#[component]
pub fn ModalContent<'a>(props: &mut ModalContentProps<'a>) -> impl Into<AnyElement<'a>> {
    let bg = props.background_color.unwrap_or(Theme::BG_MODAL);
    let border = props.border_color.unwrap_or(Theme::BORDER_FOCUS);
    let justify = props.justify_content.unwrap_or(JustifyContent::FlexStart);
    let align = props.align_items.unwrap_or(AlignItems::Stretch);
    let overflow = if props.overflow_hidden {
        Overflow::Hidden
    } else {
        Overflow::Visible
    };

    element! {
        View(
            // Size is always 100% - this is enforced, not configurable
            width: 100pct,
            height: 100pct,
            // Layout props from user
            flex_direction: props.flex_direction,
            justify_content: justify,
            align_items: align,
            // Styling props from user
            background_color: bg,
            border_style: props.border_style,
            border_color: border,
            overflow: overflow,
        ) {
            #(&mut props.children)
        }
    }
}

// =============================================================================
// Confirm Modal
// =============================================================================

/// Props for ConfirmModal
#[derive(Default, Props)]
pub struct ConfirmModalProps {
    /// Modal title
    pub title: String,
    /// Confirmation message
    pub message: String,
    /// Whether the modal is visible
    pub visible: bool,
    /// Confirm button text
    pub confirm_text: String,
    /// Cancel button text
    pub cancel_text: String,
    /// Whether confirm is focused (vs cancel)
    pub confirm_focused: bool,
}

/// A confirmation dialog with Yes/No buttons
#[component]
pub fn ConfirmModal(props: &ConfirmModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        }
        .into_any();
    }

    let title = props.title.clone();
    let message = props.message.clone();
    let confirm_text = if props.confirm_text.is_empty() {
        "Confirm".to_string()
    } else {
        props.confirm_text.clone()
    };
    let cancel_text = if props.cancel_text.is_empty() {
        "Cancel".to_string()
    } else {
        props.cancel_text.clone()
    };

    let confirm_border = if props.confirm_focused {
        Theme::PRIMARY
    } else {
        Theme::BORDER
    };
    let confirm_fg = if props.confirm_focused {
        Theme::PRIMARY
    } else {
        Theme::TEXT_MUTED
    };
    let cancel_border = if !props.confirm_focused {
        Theme::SECONDARY
    } else {
        Theme::BORDER
    };
    let cancel_fg = if !props.confirm_focused {
        Theme::SECONDARY
    } else {
        Theme::TEXT_MUTED
    };

    // Use ModalContent to prevent nested absolute positioning bugs
    element! {
        ModalContent(
            flex_direction: FlexDirection::Column,
            justify_content: Some(JustifyContent::Center),
            align_items: Some(AlignItems::Center),
            border_style: Borders::PRIMARY,
            border_color: Some(Theme::BORDER_FOCUS),
        ) {
            // Title bar
            View(
                width: 100pct,
                padding: Spacing::PANEL_PADDING,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
            }
            // Message
            View(
                width: 100pct,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                padding: Spacing::PANEL_PADDING,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
            ) {
                Text(content: message, color: Theme::TEXT)
            }
            // Buttons
            View(
                width: 100pct,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::End,
                gap: Spacing::SM,
                padding: Spacing::PANEL_PADDING,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            ) {
                View(
                    padding_left: Spacing::SM,
                    padding_right: Spacing::SM,
                    border_style: Borders::PRIMARY,
                    border_color: cancel_border,
                ) {
                    Text(content: cancel_text, color: cancel_fg)
                }
                View(
                    padding_left: Spacing::SM,
                    padding_right: Spacing::SM,
                    border_style: Borders::PRIMARY,
                    border_color: confirm_border,
                ) {
                    Text(content: confirm_text, color: confirm_fg)
                }
            }
        }
    }
    .into_any()
}

// =============================================================================
// Input Modal
// =============================================================================

/// Props for InputModal
#[derive(Default, Props)]
pub struct InputModalProps {
    /// Modal title
    pub title: String,
    /// Input label
    pub label: String,
    /// Current input value
    pub value: String,
    /// Placeholder text
    pub placeholder: String,
    /// Whether the modal is visible
    pub visible: bool,
}

/// A modal with text input
#[component]
pub fn InputModal(props: &InputModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        }
        .into_any();
    }

    let title = props.title.clone();
    let label = props.label.clone();
    let value = props.value.clone();
    let placeholder = props.placeholder.clone();

    let display_text = if value.is_empty() { placeholder } else { value };
    let text_color = if props.value.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    // Use ModalContent to prevent nested absolute positioning bugs
    element! {
        ModalContent(
            flex_direction: FlexDirection::Column,
            border_style: Borders::PRIMARY,
            border_color: Some(Theme::BORDER_FOCUS),
        ) {
            // Title bar
            View(
                width: 100pct,
                padding: Spacing::PANEL_PADDING,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
            }
            // Label + Input
            View(
                width: 100pct,
                padding: Spacing::PANEL_PADDING,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                gap: Spacing::XS,
            ) {
                Text(content: label, color: Theme::TEXT_MUTED)
                View(
                    border_style: Borders::INPUT,
                    border_color: Theme::BORDER_FOCUS,
                    padding_left: Spacing::LIST_ITEM_PADDING,
                    padding_right: Spacing::LIST_ITEM_PADDING,
                ) {
                    Text(content: display_text, color: text_color)
                }
            }
            // Hints
            View(
                width: 100pct,
                padding: Spacing::PANEL_PADDING,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            ) {
                Text(content: "Enter to confirm · Esc to cancel", color: Theme::TEXT_MUTED)
            }
        }
    }
    .into_any()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tui::layout::dim;

    #[test]
    fn test_modal_frame_dimensions() {
        // Modal frame should position at nav height and fill middle region
        assert_eq!(dim::NAV_HEIGHT, 3);
        assert_eq!(dim::TOTAL_WIDTH, 80);
        assert_eq!(dim::MIDDLE_HEIGHT, 25);
    }
}
