//! # Chat Create Modal
//!
//! Modal for creating new chat groups/channels.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::{
    contact_multi_select, labeled_input, modal_footer, modal_header, status_message,
    threshold_selector, ContactMultiSelectItem, ContactMultiSelectProps, LabeledInputProps,
    ModalFooterProps, ModalHeaderProps, ModalStatus, ThresholdSelectorProps,
};
use crate::tui::layout::dim;
use crate::tui::state::CreateChannelStep;
use crate::tui::theme::{Borders, Spacing, Theme};
use crate::tui::types::KeyHint;

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
    /// Wizard step
    pub step: CreateChannelStep,
    /// The current name input
    pub name: String,
    /// The current topic input
    pub topic: String,
    /// Contact candidates (id, name)
    pub contacts: Vec<(String, String)>,
    /// Selected indices
    pub selected_indices: Vec<usize>,
    /// Focused contact index
    pub focused_index: usize,
    /// Threshold k
    pub threshold_k: u8,
    /// Threshold n
    pub threshold_n: u8,
    /// Number of selected members
    pub members_count: usize,
    /// Which field is active (0 = name, 1 = topic)
    pub active_field: usize,
    /// Error message if creation failed
    pub error: String,
    /// Whether creation is in progress
    pub creating: bool,
    /// Status text (waiting, etc.)
    pub status: String,
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
    let step = props.step.clone();

    // Determine border color based on state
    let border_color = if !error.is_empty() {
        Theme::ERROR
    } else if creating {
        Theme::WARNING
    } else {
        Theme::PRIMARY
    };

    // Header props with step indicator (3 steps: Details, Members, Threshold)
    let step_num = match step {
        CreateChannelStep::Details => 1,
        CreateChannelStep::Members => 2,
        CreateChannelStep::Threshold => 3,
    };
    let header_props = ModalHeaderProps::new("New Chat Group").with_step(step_num, 3);

    // Footer hints vary by step
    let footer_hints = match step {
        CreateChannelStep::Details => vec![
            KeyHint::new("Esc", "Cancel"),
            KeyHint::new("Tab", "Next Field"),
            KeyHint::new("Enter", "Continue"),
        ],
        CreateChannelStep::Members => vec![
            KeyHint::new("Esc", "Back"),
            KeyHint::new("Space", "Select"),
            KeyHint::new("Enter", "Continue"),
        ],
        CreateChannelStep::Threshold => vec![
            KeyHint::new("Esc", "Back"),
            KeyHint::new("↑↓", "Adjust"),
            KeyHint::new("Enter", "Create"),
        ],
    };
    let footer_props = ModalFooterProps::new(footer_hints);

    // Status for error/creating states
    let status = if !error.is_empty() {
        ModalStatus::Error(error)
    } else if creating {
        ModalStatus::Loading("Creating...".to_string())
    } else {
        ModalStatus::Idle
    };

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            background_color: Theme::BG_MODAL,
            border_style: Borders::PRIMARY,
            border_color: border_color,
            overflow: Overflow::Hidden,
        ) {
            // Header
            #(Some(modal_header(&header_props).into()))

            // Body - fills available space
            View(
                width: 100pct,
                padding_left: Spacing::MODAL_PADDING,
                padding_right: Spacing::MODAL_PADDING,
                padding_top: Spacing::XS,
                padding_bottom: Spacing::MODAL_PADDING,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Hidden,
            ) {
                #(match step {
                    CreateChannelStep::Details => {
                        let name_input = LabeledInputProps::new("Group Name:", "Enter group name...")
                            .with_value(name)
                            .with_focused(active_field == 0);
                        let topic_input = LabeledInputProps::new("Topic (optional):", "Enter topic...")
                            .with_value(topic)
                            .with_focused(active_field == 1);
                        vec![element! {
                            View(flex_direction: FlexDirection::Column, gap: Spacing::SM) {
                                #(Some(labeled_input(&name_input).into()))
                                #(Some(labeled_input(&topic_input).into()))
                            }
                        }.into_any()]
                    }
                    CreateChannelStep::Members => {
                        let items = props
                            .contacts
                            .iter()
                            .map(|(_, name)| ContactMultiSelectItem {
                                name: name.clone(),
                                badge: None,
                            })
                            .collect::<Vec<_>>();
                        let selector = ContactMultiSelectProps {
                            prompt: "Invite Contacts".to_string(),
                            items,
                            selected: props.selected_indices.clone(),
                            focused: props.focused_index,
                            min_selected: None,
                            footer_hint: Some(
                                "↑↓/jk Navigate  Space Select  Enter Confirm  Esc Cancel".to_string(),
                            ),
                        };
                        vec![contact_multi_select(&selector).into()]
                    }
                    CreateChannelStep::Threshold => {
                        let selector = ThresholdSelectorProps {
                            prompt: "Group Threshold".to_string(),
                            subtext: Some(format!(
                                "Require {} of {} signatures (includes you)",
                                props.threshold_k, props.threshold_n
                            )),
                            k: props.threshold_k,
                            n: props.threshold_n,
                            low_hint: Some("Low: any one signer".to_string()),
                            show_hint: true,
                        };
                        vec![threshold_selector(&selector).into()]
                    },
                })

                // Status message (error/loading)
                #(Some(status_message(&status).into()))
            }

            // Footer with key hints
            #(Some(modal_footer(&footer_props).into()))
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
