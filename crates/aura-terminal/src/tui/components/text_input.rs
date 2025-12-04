//! # Text Input Component
//!
//! Single-line text input with cursor.

use iocraft::prelude::*;

use crate::tui::theme::Theme;

/// Props for TextInput
#[derive(Default, Props)]
pub struct TextInputProps {
    /// Current input value
    pub value: String,
    /// Placeholder text when empty
    pub placeholder: String,
    /// Cursor position
    pub cursor: usize,
    /// Whether the input is focused
    pub focused: bool,
}

/// A single-line text input with cursor (display-only)
///
/// State management should be handled by the parent component
/// using `hooks.use_state()` with Copy-able wrapper types.
#[component]
pub fn TextInput(props: &TextInputProps) -> impl Into<AnyElement<'static>> {
    let border_color = if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    let display_text = if props.value.is_empty() {
        props.placeholder.clone()
    } else {
        props.value.clone()
    };

    let text_color = if props.value.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    element! {
        View(
            border_style: BorderStyle::Round,
            border_color: border_color,
            padding_left: 1,
            padding_right: 1,
        ) {
            Text(content: display_text, color: text_color)
        }
    }
}
