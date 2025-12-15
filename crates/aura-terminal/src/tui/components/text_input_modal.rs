//! # Text Input Modal
//!
//! Generic modal for single text input with submit/cancel actions.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::theme::Theme;

/// Callback type for modal submit (value: String)
pub type TextInputSubmitCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Callback type for modal cancel
pub type TextInputCancelCallback = Arc<dyn Fn() + Send + Sync>;

/// Props for TextInputModal
#[derive(Default, Props)]
pub struct TextInputModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Whether the input is focused
    pub focused: bool,
    /// Modal title
    pub title: String,
    /// Current input value
    pub value: String,
    /// Placeholder text
    pub placeholder: String,
    /// Error message if any
    pub error: String,
    /// Whether submission is in progress
    pub submitting: bool,
    /// Callback when submitting
    pub on_submit: Option<TextInputSubmitCallback>,
    /// Callback when canceling
    pub on_cancel: Option<TextInputCancelCallback>,
}

/// Modal for single text input
#[component]
pub fn TextInputModal(props: &TextInputModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    let value = props.value.clone();
    let title = props.title.clone();
    let placeholder = props.placeholder.clone();
    let error = props.error.clone();
    let submitting = props.submitting;

    // Determine border color based on state
    let border_color = if !error.is_empty() {
        Theme::ERROR
    } else if submitting {
        Theme::WARNING
    } else {
        Theme::PRIMARY
    };

    // Display text for input
    let display_value = if value.is_empty() {
        placeholder.clone()
    } else {
        value.clone()
    };

    let value_color = if value.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    element! {
        View(
            position: Position::Absolute,
            width: 100pct,
            height: 100pct,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,

        ) {
            View(
                width: Percent(50.0),
                flex_direction: FlexDirection::Column,
                background_color: Theme::BG_MODAL,
                border_style: BorderStyle::Round,
                border_color: border_color,
            ) {
                // Header
                View(
                    padding: 2,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Bottom,
                    border_color: Theme::BORDER,
                ) {
                    Text(
                        content: title,
                        weight: Weight::Bold,
                        color: Theme::PRIMARY,
                    )
                }

                // Body
                View(padding: 2, flex_direction: FlexDirection::Column) {
                    // Input field
                    View(
                        flex_direction: FlexDirection::Column,

                        border_style: BorderStyle::Round,
                        border_color: Theme::PRIMARY,
                        padding: 1,
                        margin_bottom: 1,
                    ) {
                        Text(
                            content: display_value,
                            color: value_color,
                        )
                    }

                    // Error message (if any)
                    #(if !error.is_empty() {
                        Some(element! {
                            View(margin_bottom: 1) {
                                Text(content: error, color: Theme::ERROR)
                            }
                        })
                    } else {
                        None
                    })

                    // Status message
                    #(if submitting {
                        Some(element! {
                            View(margin_top: 1) {
                                Text(content: "Saving...", color: Theme::WARNING)
                            }
                        })
                    } else {
                        None
                    })
                }

                // Footer with key hints
                View(
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    padding: 2,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Top,
                    border_color: Theme::BORDER,
                ) {
                    View(flex_direction: FlexDirection::Row, gap: 2) {
                        Text(content: "Esc", color: Theme::SECONDARY)
                        Text(content: "Cancel", color: Theme::TEXT_MUTED)
                    }
                    View(flex_direction: FlexDirection::Row, gap: 2) {
                        Text(content: "Enter", color: Theme::SECONDARY)
                        Text(content: "Save", color: Theme::TEXT_MUTED)
                    }
                }
            }
        }
    }
}

/// State for text input modal
#[derive(Clone, Debug, Default)]
pub struct TextInputState {
    /// Whether the modal is visible
    pub visible: bool,
    /// Modal title
    pub title: String,
    /// Current input value
    pub value: String,
    /// Placeholder text
    pub placeholder: String,
    /// Error message if any
    pub error: Option<String>,
    /// Whether submission is in progress
    pub submitting: bool,
    /// Associated ID (e.g., contact_id for petname edit)
    pub context_id: Option<String>,
}

impl TextInputState {
    /// Create a new state
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the modal with initial configuration
    pub fn show(
        &mut self,
        title: &str,
        initial_value: &str,
        placeholder: &str,
        context_id: Option<String>,
    ) {
        self.visible = true;
        self.title = title.to_string();
        self.value = initial_value.to_string();
        self.placeholder = placeholder.to_string();
        self.error = None;
        self.submitting = false;
        self.context_id = context_id;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
        self.value.clear();
        self.title.clear();
        self.placeholder.clear();
        self.error = None;
        self.submitting = false;
        self.context_id = None;
    }

    /// Push a character to the input
    pub fn push_char(&mut self, c: char) {
        self.value.push(c);
        self.error = None;
    }

    /// Pop a character from the input
    pub fn pop_char(&mut self) {
        self.value.pop();
        self.error = None;
    }

    /// Set an error
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.submitting = false;
    }

    /// Mark as submitting
    pub fn start_submitting(&mut self) {
        self.submitting = true;
        self.error = None;
    }

    /// Check if can submit
    pub fn can_submit(&self) -> bool {
        !self.value.is_empty() && !self.submitting
    }

    /// Get the current value
    pub fn get_value(&self) -> &str {
        &self.value
    }

    /// Get the context ID
    pub fn get_context_id(&self) -> Option<&str> {
        self.context_id.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_input_state() {
        let mut state = TextInputState::new();
        assert!(!state.visible);
        assert!(state.value.is_empty());
        assert!(!state.can_submit());

        state.show(
            "Edit Petname",
            "Alice",
            "Enter name...",
            Some("contact-123".to_string()),
        );
        assert!(state.visible);
        assert_eq!(state.value, "Alice");
        assert_eq!(state.title, "Edit Petname");
        assert_eq!(state.context_id, Some("contact-123".to_string()));
        assert!(state.can_submit());

        // Type more
        state.push_char('!');
        assert_eq!(state.value, "Alice!");

        // Backspace
        state.pop_char();
        assert_eq!(state.value, "Alice");

        // Hide
        state.hide();
        assert!(!state.visible);
        assert!(state.value.is_empty());
        assert!(state.context_id.is_none());
    }
}
