//! # Account Setup Modal
//!
//! Modal for first-time account creation during onboarding.

use iocraft::prelude::*;

use crate::tui::layout::dim;
use crate::tui::theme::Theme;

/// Props for AccountSetupModal
#[derive(Default, Props)]
pub struct AccountSetupModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Current display name input
    pub display_name: String,
    /// Whether the input is focused
    pub focused: bool,
    /// Whether account creation is in progress
    pub creating: bool,
    /// Whether account was created successfully
    pub success: bool,
    /// Error message if creation failed
    pub error: String,
}

/// Account setup modal for first-time users
#[component]
pub fn AccountSetupModal(props: &AccountSetupModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    let display_name = props.display_name.clone();
    let creating = props.creating;
    let success = props.success;
    let has_error = !props.error.is_empty();
    let error = props.error.clone();

    // Show success/error result view
    if success || has_error {
        let (status_icon, status_text, status_color) = if success {
            ("✓", "Account created successfully!", Theme::SUCCESS)
        } else {
            ("✗", error.as_str(), Theme::ERROR)
        };

        return element! {
            View(
                position: Position::Absolute,
                top: 0u16,
                left: 0u16,
                width: dim::TOTAL_WIDTH,
                height: dim::MIDDLE_HEIGHT,
                flex_direction: FlexDirection::Column,
                background_color: Theme::BG_MODAL,
                border_style: BorderStyle::Round,
                border_color: if success { Theme::SUCCESS } else { Theme::ERROR },
                overflow: Overflow::Hidden,
            ) {
                // Header
                View(
                    width: 100pct,
                    padding: 2,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Bottom,
                    border_color: Theme::BORDER,
                ) {
                    Text(
                        content: if success { "Account Created" } else { "Account Creation Failed" },
                        weight: Weight::Bold,
                        color: status_color,
                    )
                }

                // Status content
                View(
                    width: 100pct,
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    padding: 3,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                ) {
                    View(flex_direction: FlexDirection::Row, gap: 2) {
                        Text(content: status_icon, color: status_color, weight: Weight::Bold)
                        Text(content: status_text, color: status_color)
                    }
                    #(if success {
                        Some(element! {
                            View(margin_top: 2) {
                                Text(
                                    content: format!("Welcome, {}!", display_name),
                                    color: Theme::TEXT_MUTED,
                                )
                            }
                        })
                    } else {
                        None
                    })
                }

                // Footer
                View(
                    width: 100pct,
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    padding: 2,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Top,
                    border_color: Theme::BORDER,
                ) {
                    View(flex_direction: FlexDirection::Row, gap: 1) {
                        Text(content: "Enter", color: Theme::SECONDARY)
                        Text(content: if success { "to continue" } else { "to try again" }, color: Theme::TEXT_MUTED)
                    }
                }
            }
        };
    }

    // Show creating spinner
    if creating {
        return element! {
            View(
                position: Position::Absolute,
                top: 0u16,
                left: 0u16,
                width: dim::TOTAL_WIDTH,
                height: dim::MIDDLE_HEIGHT,
                flex_direction: FlexDirection::Column,
                background_color: Theme::BG_MODAL,
                border_style: BorderStyle::Round,
                border_color: Theme::PRIMARY,
                overflow: Overflow::Hidden,
            ) {
                // Header
                View(
                    width: 100pct,
                    padding: 2,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Bottom,
                    border_color: Theme::BORDER,
                ) {
                    Text(
                        content: "Creating Account",
                        weight: Weight::Bold,
                        color: Theme::PRIMARY,
                    )
                }

                // Creating content
                View(
                    width: 100pct,
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    padding: 3,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                ) {
                    Text(content: "Generating FROST threshold keys...", color: Theme::TEXT_MUTED)
                    View(margin_top: 1) {
                        Text(content: "Please wait...", color: Theme::TEXT_MUTED)
                    }
                }
            }
        };
    }

    // Show input form (default state)
    let can_submit = !display_name.is_empty();

    let placeholder = if display_name.is_empty() {
        "Enter your name...".to_string()
    } else {
        display_name.clone()
    };

    let text_color = if display_name.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    let border_color = if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

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
            border_color: Theme::PRIMARY,
            overflow: Overflow::Hidden,
        ) {
            // Welcome header
            View(
                width: 100pct,
                padding: 2,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(
                    content: "Welcome to Aura",
                    weight: Weight::Bold,
                    color: Theme::PRIMARY,
                )
                View(margin_top: 1) {
                    Text(
                        content: "Create your threshold identity",
                        color: Theme::TEXT_MUTED,
                    )
                }
            }

            // Form content
            View(
                width: 100pct,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                padding: 2,
                flex_direction: FlexDirection::Column,
            ) {
                // Description
                View(margin_bottom: 2) {
                    Text(
                        content: "Your account uses FROST threshold signatures for security.",
                        color: Theme::TEXT_MUTED,
                    )
                }
                Text(
                    content: "This creates a single-device account. Add guardians later",
                    color: Theme::TEXT_MUTED,
                )
                Text(
                    content: "in Settings to enable social recovery.",
                    color: Theme::TEXT_MUTED,
                )

                // Display name input
                View(margin_top: 2, flex_direction: FlexDirection::Column) {
                    Text(content: "Display Name *", color: Theme::TEXT_MUTED)
                    View(
                        margin_top: 1,
                        border_style: BorderStyle::Round,
                        border_color: border_color,
                        padding_left: 1,
                        padding_right: 1,
                        padding_top: 0,
                        padding_bottom: 0,
                    ) {
                        Text(content: placeholder, color: text_color)
                    }
                }
            }

            // Footer with hints and button
            View(
                width: 100pct,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: 2,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            ) {
                View(flex_direction: FlexDirection::Row, gap: 1) {
                    Text(content: "Enter", color: Theme::SECONDARY)
                    Text(content: "to create", color: Theme::TEXT_MUTED)
                }
                View(
                    padding_left: 2,
                    padding_right: 2,
                    border_style: BorderStyle::Round,
                    border_color: if can_submit { Theme::PRIMARY } else { Theme::BORDER },
                ) {
                    Text(
                        content: "Create Account",
                        color: if can_submit { Theme::PRIMARY } else { Theme::TEXT_MUTED },
                    )
                }
            }
        }
    }
}

/// State for account setup modal
#[derive(Clone, Debug, Default)]
pub struct AccountSetupState {
    /// Whether the modal is visible
    pub visible: bool,
    /// Current display name input
    pub display_name: String,
    /// Whether account creation is in progress
    pub creating: bool,
    /// Whether account was created successfully
    pub success: bool,
    /// Error message if creation failed
    pub error: Option<String>,
}

impl AccountSetupState {
    /// Create a new account setup state
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the modal
    pub fn show(&mut self) {
        self.visible = true;
        self.display_name.clear();
        self.creating = false;
        self.success = false;
        self.error = None;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Set the display name
    pub fn set_display_name(&mut self, name: impl Into<String>) {
        self.display_name = name.into();
        self.error = None; // Clear error on input
    }

    /// Append a character
    pub fn push_char(&mut self, c: char) {
        self.display_name.push(c);
        self.error = None;
    }

    /// Remove last character
    pub fn backspace(&mut self) {
        self.display_name.pop();
    }

    /// Check if submission is valid
    pub fn can_submit(&self) -> bool {
        !self.display_name.is_empty() && !self.creating && !self.success
    }

    /// Start creating account
    pub fn start_creating(&mut self) {
        self.creating = true;
        self.success = false;
        self.error = None;
    }

    /// Mark creation as successful (shows success screen)
    pub fn set_success(&mut self) {
        self.creating = false;
        self.success = true;
        self.error = None;
    }

    /// Mark creation as complete and dismiss modal
    pub fn finish_creating(&mut self) {
        self.creating = false;
        self.success = false;
        self.visible = false;
    }

    /// Set error message
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.creating = false;
        self.success = false;
        self.error = Some(error.into());
    }

    /// Reset to input state (for retry after error)
    pub fn reset_to_input(&mut self) {
        self.creating = false;
        self.success = false;
        self.error = None;
    }

    /// Check if in success state
    pub fn is_success(&self) -> bool {
        self.success
    }

    /// Check if in error state
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Get display name if valid
    pub fn get_display_name(&self) -> Option<&str> {
        if self.display_name.is_empty() {
            None
        } else {
            Some(&self.display_name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_setup_state() {
        let mut state = AccountSetupState::new();
        assert!(!state.visible);
        assert!(!state.can_submit());

        state.show();
        assert!(state.visible);
        assert!(!state.can_submit()); // No name yet

        state.push_char('A');
        state.push_char('l');
        state.push_char('i');
        state.push_char('c');
        state.push_char('e');
        assert_eq!(state.display_name, "Alice");
        assert!(state.can_submit());

        state.backspace();
        assert_eq!(state.display_name, "Alic");

        state.start_creating();
        assert!(state.creating);
        assert!(!state.can_submit()); // Creating, can't submit again

        state.finish_creating();
        assert!(!state.creating);
        assert!(!state.visible); // Auto-hides on finish
    }

    #[test]
    fn test_error_handling() {
        let mut state = AccountSetupState::new();
        state.show();
        state.set_display_name("Test");
        state.start_creating();
        state.set_error("Network error");

        assert!(!state.creating);
        assert_eq!(state.error, Some("Network error".to_string()));
        assert!(state.visible); // Still visible after error

        // Typing clears error
        state.push_char('!');
        assert!(state.error.is_none());
    }
}
