//! # Invitation Create Modal
//!
//! Modal for creating new invitations from the TUI.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::theme::Theme;
use crate::tui::types::InvitationType;

/// Callback type for invitation creation
pub type CreateInvitationCallback =
    Arc<dyn Fn(InvitationType, Option<String>, Option<u64>) + Send + Sync>;

/// Callback type for modal cancellation
pub type CancelCallback = Arc<dyn Fn() + Send + Sync>;

/// Props for InvitationCreateModal
#[derive(Default, Props)]
pub struct InvitationCreateModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Whether the input is focused
    pub focused: bool,
    /// Whether creation is in progress
    pub creating: bool,
    /// Error message if creation failed
    pub error: String,
    /// Currently selected invitation type
    pub invitation_type: InvitationType,
    /// Optional message for the invitation
    pub message: String,
    /// TTL in hours (0 = no expiry)
    pub ttl_hours: u32,
    /// Callback for creating the invitation
    pub on_create: Option<CreateInvitationCallback>,
    /// Callback for canceling
    pub on_cancel: Option<CancelCallback>,
}

/// Modal for creating new invitations
#[component]
pub fn InvitationCreateModal(props: &InvitationCreateModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    let creating = props.creating;
    let has_error = !props.error.is_empty();
    let error = props.error.clone();
    let invitation_type = props.invitation_type;
    let message = props.message.clone();
    let ttl_hours = props.ttl_hours;

    let border_color = if has_error {
        Theme::ERROR
    } else if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    let submit_text = if creating {
        "Creating...".to_string()
    } else {
        "Create Invitation".to_string()
    };

    let can_submit = !creating;

    // Type selection display
    let type_label = invitation_type.label();
    let type_icon = invitation_type.icon();

    // Message display (truncated if too long)
    let message_display = if message.is_empty() {
        "(optional message)".to_string()
    } else if message.len() > 40 {
        format!("{}...", &message[..37])
    } else {
        message.clone()
    };

    let message_color = if message.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    // TTL display
    let ttl_display = if ttl_hours == 0 {
        "Never expires".to_string()
    } else if ttl_hours == 1 {
        "Expires in 1 hour".to_string()
    } else if ttl_hours < 24 {
        format!("Expires in {} hours", ttl_hours)
    } else if ttl_hours == 24 {
        "Expires in 1 day".to_string()
    } else {
        format!("Expires in {} days", ttl_hours / 24)
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
                width: Percent(60.0),
                flex_direction: FlexDirection::Column,
                background_color: Theme::BG_MODAL,
                border_style: BorderStyle::Round,
                border_color: Theme::PRIMARY,
            ) {
                // Header
                View(
                    padding: 2,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Bottom,
                    border_color: Theme::BORDER,
                ) {
                    Text(
                        content: "Create Invitation",
                        weight: Weight::Bold,
                        color: Theme::PRIMARY,
                    )
                    View(margin_top: 1) {
                        Text(
                            content: "Invite someone to connect with you",
                            color: Theme::TEXT_MUTED,
                        )
                    }
                }

                // Form content
                View(padding: 2, flex_direction: FlexDirection::Column) {
                    // Invitation Type selector
                    View(flex_direction: FlexDirection::Column, margin_bottom: 2) {
                        Text(content: "Type (Tab to change)", color: Theme::TEXT_MUTED)
                        View(
                            margin_top: 1,
                            flex_direction: FlexDirection::Row,
                            gap: 1,
                            border_style: BorderStyle::Round,
                            border_color: border_color,
                            padding_left: 1,
                            padding_right: 1,
                        ) {
                            Text(content: type_icon.to_string(), color: Theme::PRIMARY)
                            Text(content: type_label.to_string(), color: Theme::TEXT)
                        }
                    }

                    // Type descriptions
                    View(margin_bottom: 2) {
                        #(match invitation_type {
                            InvitationType::Contact => element! {
                                Text(
                                    content: "Add as a contact for messaging",
                                    color: Theme::TEXT_MUTED,
                                )
                            },
                            InvitationType::Guardian => element! {
                                Text(
                                    content: "Invite to be a guardian for account recovery",
                                    color: Theme::TEXT_MUTED,
                                )
                            },
                            InvitationType::Channel => element! {
                                Text(
                                    content: "Invite to join a chat channel",
                                    color: Theme::TEXT_MUTED,
                                )
                            },
                        })
                    }

                    // Optional message
                    View(flex_direction: FlexDirection::Column, margin_bottom: 2) {
                        Text(content: "Message (m to edit)", color: Theme::TEXT_MUTED)
                        View(
                            margin_top: 1,
                            border_style: BorderStyle::Round,
                            border_color: Theme::BORDER,
                            padding_left: 1,
                            padding_right: 1,
                        ) {
                            Text(content: message_display, color: message_color)
                        }
                    }

                    // TTL selector
                    View(flex_direction: FlexDirection::Column, margin_bottom: 2) {
                        Text(content: "Expiry (t to change)", color: Theme::TEXT_MUTED)
                        View(
                            margin_top: 1,
                            border_style: BorderStyle::Round,
                            border_color: Theme::BORDER,
                            padding_left: 1,
                            padding_right: 1,
                        ) {
                            Text(content: ttl_display, color: Theme::TEXT)
                        }
                    }

                    // Error message
                    #(if has_error {
                        Some(element! {
                            View(margin_top: 1) {
                                Text(content: error, color: Theme::ERROR)
                            }
                        })
                    } else {
                        None
                    })
                }

                // Footer with hints and button
                View(
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: 2,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Top,
                    border_color: Theme::BORDER,
                ) {
                    View(flex_direction: FlexDirection::Column, gap: 0) {
                        View(flex_direction: FlexDirection::Row, gap: 1) {
                            Text(content: "Tab", color: Theme::SECONDARY)
                            Text(content: "type", color: Theme::TEXT_MUTED)
                            Text(content: "t", color: Theme::SECONDARY)
                            Text(content: "ttl", color: Theme::TEXT_MUTED)
                            Text(content: "Esc", color: Theme::SECONDARY)
                            Text(content: "cancel", color: Theme::TEXT_MUTED)
                        }
                    }
                    View(
                        padding_left: 2,
                        padding_right: 2,
                        border_style: BorderStyle::Round,
                        border_color: if can_submit { Theme::PRIMARY } else { Theme::BORDER },
                    ) {
                        Text(
                            content: submit_text,
                            color: if can_submit { Theme::PRIMARY } else { Theme::TEXT_MUTED },
                        )
                    }
                }
            }
        }
    }
}

/// State for invitation create modal
#[derive(Clone, Debug, Default)]
pub struct InvitationCreateState {
    /// Whether the modal is visible
    pub visible: bool,
    /// Currently selected invitation type
    pub invitation_type: InvitationType,
    /// Optional message for the invitation
    pub message: String,
    /// TTL in hours (0 = no expiry)
    pub ttl_hours: u32,
    /// Whether creation is in progress
    pub creating: bool,
    /// Error message if creation failed
    pub error: Option<String>,
}

impl InvitationCreateState {
    /// Create a new invitation create state
    pub fn new() -> Self {
        Self {
            ttl_hours: 24, // Default to 24 hours
            ..Default::default()
        }
    }

    /// Show the modal
    pub fn show(&mut self) {
        self.visible = true;
        self.invitation_type = InvitationType::Contact;
        self.message.clear();
        self.ttl_hours = 24;
        self.creating = false;
        self.error = None;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Cycle to next invitation type
    pub fn next_type(&mut self) {
        self.invitation_type = match self.invitation_type {
            InvitationType::Contact => InvitationType::Guardian,
            InvitationType::Guardian => InvitationType::Channel,
            InvitationType::Channel => InvitationType::Contact,
        };
        self.error = None;
    }

    /// Cycle to previous invitation type
    pub fn prev_type(&mut self) {
        self.invitation_type = match self.invitation_type {
            InvitationType::Contact => InvitationType::Channel,
            InvitationType::Guardian => InvitationType::Contact,
            InvitationType::Channel => InvitationType::Guardian,
        };
        self.error = None;
    }

    /// Set the message
    pub fn set_message(&mut self, msg: impl Into<String>) {
        self.message = msg.into();
        self.error = None;
    }

    /// Append a character to the message
    pub fn push_char(&mut self, c: char) {
        self.message.push(c);
        self.error = None;
    }

    /// Remove last character from message
    pub fn backspace(&mut self) {
        self.message.pop();
    }

    /// Cycle TTL values: 0 (never) -> 1h -> 24h -> 72h -> 168h (1 week) -> 0
    pub fn cycle_ttl(&mut self) {
        self.ttl_hours = match self.ttl_hours {
            0 => 1,
            1 => 24,
            24 => 72,
            72 => 168,
            _ => 0,
        };
    }

    /// Check if submission is valid
    pub fn can_submit(&self) -> bool {
        !self.creating
    }

    /// Start creating invitation
    pub fn start_creating(&mut self) {
        self.creating = true;
        self.error = None;
    }

    /// Mark creation as complete
    pub fn finish_creating(&mut self) {
        self.creating = false;
        self.visible = false;
    }

    /// Set error message
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.creating = false;
        self.error = Some(error.into());
    }

    /// Get TTL in seconds (None if no expiry)
    pub fn ttl_secs(&self) -> Option<u64> {
        if self.ttl_hours == 0 {
            None
        } else {
            Some(self.ttl_hours as u64 * 3600)
        }
    }

    /// Get the message if not empty
    pub fn get_message(&self) -> Option<&str> {
        if self.message.is_empty() {
            None
        } else {
            Some(&self.message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invitation_create_state() {
        let mut state = InvitationCreateState::new();
        assert!(!state.visible);
        assert_eq!(state.ttl_hours, 24); // Default

        state.show();
        assert!(state.visible);
        assert_eq!(state.invitation_type, InvitationType::Contact);
        assert!(state.can_submit());

        state.next_type();
        assert_eq!(state.invitation_type, InvitationType::Guardian);

        state.next_type();
        assert_eq!(state.invitation_type, InvitationType::Channel);

        state.next_type();
        assert_eq!(state.invitation_type, InvitationType::Contact);
    }

    #[test]
    fn test_ttl_cycling() {
        let mut state = InvitationCreateState::new();
        state.show();
        assert_eq!(state.ttl_hours, 24);

        state.cycle_ttl();
        assert_eq!(state.ttl_hours, 72);

        state.cycle_ttl();
        assert_eq!(state.ttl_hours, 168);

        state.cycle_ttl();
        assert_eq!(state.ttl_hours, 0);

        state.cycle_ttl();
        assert_eq!(state.ttl_hours, 1);
    }

    #[test]
    fn test_message_handling() {
        let mut state = InvitationCreateState::new();
        state.show();
        assert!(state.get_message().is_none());

        state.push_char('H');
        state.push_char('i');
        assert_eq!(state.get_message(), Some("Hi"));

        state.backspace();
        assert_eq!(state.get_message(), Some("H"));

        state.set_message("Hello there!");
        assert_eq!(state.get_message(), Some("Hello there!"));
    }

    #[test]
    fn test_error_handling() {
        let mut state = InvitationCreateState::new();
        state.show();
        state.start_creating();
        assert!(state.creating);
        assert!(!state.can_submit());

        state.set_error("Network error");
        assert!(!state.creating);
        assert_eq!(state.error, Some("Network error".to_string()));
        assert!(state.visible); // Still visible after error

        // Typing clears error
        state.push_char('x');
        assert!(state.error.is_none());
    }
}
