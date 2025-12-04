//! # Message Input Component
//!
//! Chat message composer with optional reply context.

use iocraft::prelude::*;

use crate::tui::theme::Theme;

/// Props for MessageInput
#[derive(Default, Props)]
pub struct MessageInputProps {
    /// Current message text
    pub value: String,
    /// Placeholder text when empty
    pub placeholder: String,
    /// Whether the input is focused
    pub focused: bool,
    /// Reply context (if replying to a message)
    pub reply_to: Option<String>,
    /// Whether currently sending a message
    pub sending: bool,
}

/// Chat message input field with optional reply context
///
/// State management handled by parent component.
#[component]
pub fn MessageInput(props: &MessageInputProps) -> impl Into<AnyElement<'static>> {
    let border_color = if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    let is_empty = props.value.is_empty();
    let display_text = if is_empty {
        props.placeholder.clone()
    } else {
        props.value.clone()
    };

    let text_color = if is_empty {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    let reply_to = props.reply_to.clone();
    let has_reply = reply_to.is_some();
    let sending = props.sending;

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: border_color,
        ) {
            // Reply context (if any)
            #(if has_reply {
                let reply_text = reply_to.unwrap_or_default();
                Some(element! {
                    View(
                        flex_direction: FlexDirection::Row,
                        padding_left: 1,
                        padding_right: 1,
                        background_color: Theme::BG_DARK,
                        border_style: BorderStyle::Single,
                        border_edges: Edges::Bottom,
                        border_color: Theme::BORDER,
                    ) {
                        Text(content: "↩ Reply to: ", color: Theme::TEXT_MUTED)
                        Text(content: reply_text, color: Theme::TEXT)
                        View(flex_grow: 1.0)
                        Text(content: "[Esc] Cancel", color: Theme::TEXT_MUTED)
                    }
                })
            } else {
                None
            })
            // Input area
            View(
                flex_direction: FlexDirection::Row,
                padding_left: 1,
                padding_right: 1,
                align_items: AlignItems::Center,
            ) {
                #(if sending {
                    Some(element! {
                        Text(content: "⏳ ", color: Theme::SECONDARY)
                    })
                } else {
                    Some(element! {
                        Text(content: "› ", color: Theme::PRIMARY)
                    })
                })
                View(flex_grow: 1.0) {
                    Text(content: display_text, color: text_color)
                }
            }
            // Hint bar
            View(
                flex_direction: FlexDirection::Row,
                padding_left: 1,
                padding_right: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            ) {
                Text(content: "Enter", color: Theme::SECONDARY)
                Text(content: " Send  ", color: Theme::TEXT_MUTED)
                Text(content: "Shift+Enter", color: Theme::SECONDARY)
                Text(content: " Newline  ", color: Theme::TEXT_MUTED)
                Text(content: "/", color: Theme::SECONDARY)
                Text(content: " Commands", color: Theme::TEXT_MUTED)
            }
        }
    }
}

/// State helper for message input
#[derive(Clone, Debug, Default)]
pub struct MessageInputState {
    /// The message text
    pub text: String,
    /// Reply context (message being replied to)
    pub reply_to: Option<String>,
    /// Whether currently sending
    pub sending: bool,
}

impl MessageInputState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the message text
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    /// Append character to message
    pub fn push_char(&mut self, c: char) {
        self.text.push(c);
    }

    /// Delete last character
    pub fn backspace(&mut self) {
        self.text.pop();
    }

    /// Set reply context
    pub fn set_reply(&mut self, reply_to: impl Into<String>) {
        self.reply_to = Some(reply_to.into());
    }

    /// Clear reply context
    pub fn clear_reply(&mut self) {
        self.reply_to = None;
    }

    /// Clear message text (keeps reply context)
    pub fn clear_text(&mut self) {
        self.text.clear();
    }

    /// Clear everything
    pub fn clear(&mut self) {
        self.text.clear();
        self.reply_to = None;
        self.sending = false;
    }

    /// Get the message ready for sending
    pub fn take_message(&mut self) -> Option<(String, Option<String>)> {
        if self.text.is_empty() {
            return None;
        }
        let msg = std::mem::take(&mut self.text);
        let reply = self.reply_to.take();
        Some((msg, reply))
    }

    /// Check if message is empty
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Check if this looks like a command (starts with /)
    pub fn is_command(&self) -> bool {
        self.text.starts_with('/')
    }
}
