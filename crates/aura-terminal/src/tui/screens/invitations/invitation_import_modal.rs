//! # Invitation Import Modal
//!
//! Modal for importing invitation codes received out-of-band.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::layout::dim;
use crate::tui::theme::{Borders, Spacing, Theme};

/// Callback type for modal cancel
pub type CancelCallback = Arc<dyn Fn() + Send + Sync>;

/// Callback type for importing invitation (code)
pub type ImportCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Props for InvitationImportModal
#[derive(Default, Props)]
pub struct InvitationImportModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Whether the input is focused
    pub focused: bool,
    /// The current code input
    pub code: String,
    /// Error message if import failed
    pub error: String,
    /// Whether import is in progress
    pub importing: bool,
    /// Callback when importing
    pub on_import: Option<ImportCallback>,
    /// Callback when canceling
    pub on_cancel: Option<CancelCallback>,
    /// Whether running in demo mode (enables quick-fill shortcuts)
    pub demo_mode: bool,
}

/// Modal for importing invitation codes
#[component]
pub fn InvitationImportModal(props: &InvitationImportModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    let code = props.code.clone();
    let error = props.error.clone();
    let importing = props.importing;

    #[cfg(feature = "development")]
    let demo_hints: Option<AnyElement<'static>> = if props.demo_mode && code.is_empty() {
        Some(
            element! {
                View(
                    width: 100pct,
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    padding: 1,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Bottom,
                    border_color: Theme::WARNING,
                ) {
                    Text(content: "[DEMO] ", color: Theme::WARNING, weight: Weight::Bold)
                    Text(content: "Press ", color: Theme::TEXT_MUTED)
                    Text(content: "Ctrl+a", color: Theme::SECONDARY, weight: Weight::Bold)
                    Text(content: " for Alice's code, ", color: Theme::TEXT_MUTED)
                    Text(content: "Ctrl+l", color: Theme::SECONDARY, weight: Weight::Bold)
                    Text(content: " for Carol's code", color: Theme::TEXT_MUTED)
                }
            }
            .into_any(),
        )
    } else {
        None
    };

    #[cfg(not(feature = "development"))]
    let demo_hints: Option<AnyElement<'static>> = None;

    // Determine border color based on state
    let border_color = if !error.is_empty() {
        Theme::ERROR
    } else if importing {
        Theme::WARNING
    } else {
        Theme::PRIMARY
    };

    // Create display text for code input
    let code_display = if code.is_empty() {
        "Paste invitation code here...".to_string()
    } else {
        code.clone()
    };

    let code_color = if code.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
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
            View(
                width: 100pct,
                padding: Spacing::PANEL_PADDING,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(
                    content: "Import Invitation",
                    weight: Weight::Bold,
                    color: Theme::PRIMARY,
                )
            }

            // Body - fills available space
            View(
                width: 100pct,
                padding: Spacing::MODAL_PADDING,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Hidden,
            ) {
                // Instructions
                View(margin_bottom: Spacing::XS) {
                    Text(
                        content: "Paste the invitation code you received:",
                        color: Theme::TEXT,
                    )
                }

                // Code input box
                View(
                    width: 100pct,
                    flex_direction: FlexDirection::Column,
                    border_style: Borders::INPUT,
                    border_color: if props.focused { Theme::PRIMARY } else { Theme::BORDER },
                    padding: Spacing::PANEL_PADDING,
                    margin_bottom: Spacing::XS,
                ) {
                    Text(
                        content: code_display,
                        color: code_color,
                        wrap: TextWrap::Wrap,
                    )
                }

                // Error message (if any)
                #(if !error.is_empty() {
                    Some(element! {
                        View(margin_bottom: Spacing::XS) {
                            Text(content: error, color: Theme::ERROR)
                        }
                    })
                } else {
                    None
                })

                // Status message
                #(if importing {
                    Some(element! {
                        View(margin_top: Spacing::XS) {
                            Text(content: "Importing...", color: Theme::WARNING)
                        }
                    })
                } else {
                    None
                })
            }

            #(demo_hints)

            // Footer with key hints
            View(
                width: 100pct,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                padding: Spacing::PANEL_PADDING,
                gap: Spacing::LG,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            ) {
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "Esc", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Cancel", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "Ctrl+V", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Paste", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "Enter", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Import", color: Theme::TEXT_MUTED)
                }
            }
        }
    }
}

/// State for invitation import modal
#[derive(Clone, Debug, Default)]
pub struct InvitationImportState {
    /// Whether the modal is visible
    pub visible: bool,
    /// The current code input
    pub code: String,
    /// Error message if import failed
    pub error: Option<String>,
    /// Whether import is in progress
    pub importing: bool,
}

impl InvitationImportState {
    /// Create a new import state
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the modal
    pub fn show(&mut self) {
        self.visible = true;
        self.code.clear();
        self.error = None;
        self.importing = false;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
        self.code.clear();
        self.error = None;
        self.importing = false;
    }

    /// Set the code
    pub fn set_code(&mut self, code: String) {
        self.code = code;
        self.error = None; // Clear error when code changes
    }

    /// Append a character to the code
    pub fn push_char(&mut self, c: char) {
        self.code.push(c);
        self.error = None;
    }

    /// Remove the last character from the code
    pub fn pop_char(&mut self) {
        self.code.pop();
        self.error = None;
    }

    /// Clear the code
    pub fn clear_code(&mut self) {
        self.code.clear();
        self.error = None;
    }

    /// Set an error
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.importing = false;
    }

    /// Mark as importing
    pub fn start_import(&mut self) {
        self.importing = true;
        self.error = None;
    }

    /// Check if can submit
    pub fn can_submit(&self) -> bool {
        !self.code.is_empty() && !self.importing
    }

    /// Get the current code
    pub fn get_code(&self) -> &str {
        &self.code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invitation_import_state() {
        let mut state = InvitationImportState::new();
        assert!(!state.visible);
        assert!(state.code.is_empty());
        assert!(!state.can_submit());

        state.show();
        assert!(state.visible);
        assert!(!state.can_submit());

        state.push_char('A');
        state.push_char('B');
        state.push_char('C');
        assert_eq!(state.code, "ABC");
        assert!(state.can_submit());

        state.pop_char();
        assert_eq!(state.code, "AB");

        state.set_code("NEW-CODE".to_string());
        assert_eq!(state.code, "NEW-CODE");

        state.start_import();
        assert!(state.importing);
        assert!(!state.can_submit());

        state.set_error("Import failed".to_string());
        assert!(!state.importing);
        assert!(state.error.is_some());

        state.hide();
        assert!(!state.visible);
        assert!(state.code.is_empty());
    }
}
