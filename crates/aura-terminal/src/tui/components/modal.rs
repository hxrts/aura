//! # Modal Components
//!
//! Modal dialog overlays that exactly fill the middle panel region (80×25).
//! All modals use fixed dimensions matching the middle screen panel.

use iocraft::prelude::*;

use crate::tui::layout::dim;
use crate::tui::theme::Theme;

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
        };
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

    // Modal positioned at middle panel region (below nav bar)
    element! {
        View(
            position: Position::Absolute,
            top: 0u16,
            left: 0u16,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            background_color: Theme::BG_MODAL,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER_FOCUS,
            overflow: Overflow::Hidden,
        ) {
            // Title bar
            View(
                width: 100pct,
                padding: 1,
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
                padding: 1,
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
                gap: 2,
                padding: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            ) {
                View(
                    padding_left: 2,
                    padding_right: 2,
                    border_style: BorderStyle::Round,
                    border_color: cancel_border,
                ) {
                    Text(content: cancel_text, color: cancel_fg)
                }
                View(
                    padding_left: 2,
                    padding_right: 2,
                    border_style: BorderStyle::Round,
                    border_color: confirm_border,
                ) {
                    Text(content: confirm_text, color: confirm_fg)
                }
            }
        }
    }
}

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
        };
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

    // Modal positioned at middle panel region (below nav bar)
    element! {
        View(
            position: Position::Absolute,
            top: 0u16,
            left: 0u16,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            background_color: Theme::BG_MODAL,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER_FOCUS,
            overflow: Overflow::Hidden,
        ) {
            // Title bar
            View(
                width: 100pct,
                padding: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
            }
            // Label + Input
            View(
                width: 100pct,
                padding: 1,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                gap: 1,
            ) {
                Text(content: label, color: Theme::TEXT_MUTED)
                View(
                    border_style: BorderStyle::Round,
                    border_color: Theme::BORDER_FOCUS,
                    padding_left: 1,
                    padding_right: 1,
                ) {
                    Text(content: display_text, color: text_color)
                }
            }
            // Hints
            View(
                width: 100pct,
                padding: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            ) {
                Text(content: "Enter to confirm · Esc to cancel", color: Theme::TEXT_MUTED)
            }
        }
    }
}
