//! # Chat Create Modal
//!
//! Modal for creating new chat groups/channels.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::theme::Theme;

/// Callback type for modal cancel
pub type CancelCallback = Arc<dyn Fn() + Send + Sync>;

/// Callback type for creating chat (name, topic)
pub type CreateChatCallback = Arc<dyn Fn(String, Option<String>) + Send + Sync>;

/// Props for ChatCreateModal
#[derive(Default, Props)]
pub struct ChatCreateModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Whether the input is focused
    pub focused: bool,
    /// The current name input
    pub name: String,
    /// The current topic input
    pub topic: String,
    /// Which field is active (0 = name, 1 = topic)
    pub active_field: usize,
    /// Error message if creation failed
    pub error: String,
    /// Whether creation is in progress
    pub creating: bool,
    /// Callback when creating
    pub on_create: Option<CreateChatCallback>,
    /// Callback when canceling
    pub on_cancel: Option<CancelCallback>,
}

/// Modal for creating new chat groups
#[component]
pub fn ChatCreateModal(props: &ChatCreateModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    let name = props.name.clone();
    let topic = props.topic.clone();
    let active_field = props.active_field;
    let error = props.error.clone();
    let creating = props.creating;

    // Determine border color based on state
    let border_color = if !error.is_empty() {
        Theme::ERROR
    } else if creating {
        Theme::WARNING
    } else {
        Theme::PRIMARY
    };

    // Display text for fields
    let name_display = if name.is_empty() {
        "Enter group name...".to_string()
    } else {
        name.clone()
    };

    let topic_display = if topic.is_empty() {
        "Enter topic (optional)...".to_string()
    } else {
        topic.clone()
    };

    let name_color = if name.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    let topic_color = if topic.is_empty() {
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
            background_color: Theme::OVERLAY,
        ) {
            View(
                width: Percent(50.0),
                flex_direction: FlexDirection::Column,
                background_color: Theme::BG_DARK,
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
                        content: "New Chat Group",
                        weight: Weight::Bold,
                        color: Theme::PRIMARY,
                    )
                }

                // Body
                View(padding: 2, flex_direction: FlexDirection::Column) {
                    // Name field
                    View(margin_bottom: 1) {
                        Text(content: "Group Name:", color: Theme::TEXT)
                    }
                    View(
                        flex_direction: FlexDirection::Column,
                        background_color: Theme::BG_DARK,
                        border_style: BorderStyle::Round,
                        border_color: if active_field == 0 { Theme::PRIMARY } else { Theme::BORDER },
                        padding: 1,
                        margin_bottom: 2,
                    ) {
                        Text(
                            content: name_display,
                            color: name_color,
                        )
                    }

                    // Topic field
                    View(margin_bottom: 1) {
                        Text(content: "Topic (optional):", color: Theme::TEXT)
                    }
                    View(
                        flex_direction: FlexDirection::Column,
                        background_color: Theme::BG_DARK,
                        border_style: BorderStyle::Round,
                        border_color: if active_field == 1 { Theme::PRIMARY } else { Theme::BORDER },
                        padding: 1,
                        margin_bottom: 1,
                    ) {
                        Text(
                            content: topic_display,
                            color: topic_color,
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
                    #(if creating {
                        Some(element! {
                            View(margin_top: 1) {
                                Text(content: "Creating...", color: Theme::WARNING)
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
                        Text(content: "Tab", color: Theme::SECONDARY)
                        Text(content: "Next field", color: Theme::TEXT_MUTED)
                    }
                    View(flex_direction: FlexDirection::Row, gap: 2) {
                        Text(content: "Enter", color: Theme::SECONDARY)
                        Text(content: "Create", color: Theme::TEXT_MUTED)
                    }
                }
            }
        }
    }
}

/// State for chat create modal
#[derive(Clone, Debug, Default)]
pub struct ChatCreateState {
    /// Whether the modal is visible
    pub visible: bool,
    /// The current name input
    pub name: String,
    /// The current topic input
    pub topic: String,
    /// Which field is active (0 = name, 1 = topic)
    pub active_field: usize,
    /// Error message if creation failed
    pub error: Option<String>,
    /// Whether creation is in progress
    pub creating: bool,
}

impl ChatCreateState {
    /// Create a new state
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the modal
    pub fn show(&mut self) {
        self.visible = true;
        self.name.clear();
        self.topic.clear();
        self.active_field = 0;
        self.error = None;
        self.creating = false;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
        self.name.clear();
        self.topic.clear();
        self.active_field = 0;
        self.error = None;
        self.creating = false;
    }

    /// Move to next field
    pub fn next_field(&mut self) {
        self.active_field = (self.active_field + 1) % 2;
    }

    /// Move to previous field
    pub fn prev_field(&mut self) {
        self.active_field = if self.active_field == 0 { 1 } else { 0 };
    }

    /// Push a character to the active field
    pub fn push_char(&mut self, c: char) {
        match self.active_field {
            0 => self.name.push(c),
            1 => self.topic.push(c),
            _ => {}
        }
        self.error = None;
    }

    /// Pop a character from the active field
    pub fn pop_char(&mut self) {
        match self.active_field {
            0 => {
                self.name.pop();
            }
            1 => {
                self.topic.pop();
            }
            _ => {}
        }
        self.error = None;
    }

    /// Set an error
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.creating = false;
    }

    /// Mark as creating
    pub fn start_creating(&mut self) {
        self.creating = true;
        self.error = None;
    }

    /// Check if can submit
    pub fn can_submit(&self) -> bool {
        !self.name.is_empty() && !self.creating
    }

    /// Get the name
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Get the topic
    pub fn get_topic(&self) -> Option<&str> {
        if self.topic.is_empty() {
            None
        } else {
            Some(&self.topic)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_create_state() {
        let mut state = ChatCreateState::new();
        assert!(!state.visible);
        assert!(state.name.is_empty());
        assert!(!state.can_submit());

        state.show();
        assert!(state.visible);
        assert_eq!(state.active_field, 0);
        assert!(!state.can_submit());

        // Type in name field
        state.push_char('T');
        state.push_char('e');
        state.push_char('s');
        state.push_char('t');
        assert_eq!(state.name, "Test");
        assert!(state.can_submit());

        // Switch to topic field
        state.next_field();
        assert_eq!(state.active_field, 1);

        // Type in topic field
        state.push_char('H');
        state.push_char('i');
        assert_eq!(state.topic, "Hi");

        // Backspace
        state.pop_char();
        assert_eq!(state.topic, "H");

        // Check values
        assert_eq!(state.get_name(), "Test");
        assert_eq!(state.get_topic(), Some("H"));

        state.hide();
        assert!(!state.visible);
        assert!(state.name.is_empty());
    }
}
