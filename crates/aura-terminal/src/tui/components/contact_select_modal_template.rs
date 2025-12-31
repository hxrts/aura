//! # Contact Select Modal
//!
//! Modal for selecting a contact from a list.

use iocraft::prelude::*;
use std::sync::Arc;

use super::modal::ModalContent;
use super::{
    modal_footer, modal_header, status_message, ModalFooterProps, ModalHeaderProps, ModalStatus,
};
use crate::tui::theme::{Borders, Spacing, Theme};
use crate::tui::types::{Contact, KeyHint};

/// Callback type for selecting a contact (contact_id: String)
pub type ContactSelectCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Callback type for modal cancel
pub type ContactSelectCancelCallback = Arc<dyn Fn() + Send + Sync>;

/// Props for ContactSelectModal
#[derive(Default, Props)]
pub struct ContactSelectModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Modal title
    pub title: String,
    /// Available contacts to select from
    pub contacts: Vec<Contact>,
    /// Currently selected index
    pub selected_index: usize,
    /// Selected contact IDs (for multi-select)
    pub selected_ids: Vec<String>,
    /// Whether multi-select is enabled
    pub multi_select: bool,
    /// Error message if any
    pub error: String,
    /// Callback when a contact is selected
    pub on_select: Option<ContactSelectCallback>,
    /// Callback when canceling
    pub on_cancel: Option<ContactSelectCancelCallback>,
}

/// Modal for selecting a contact
#[component]
pub fn ContactSelectModal(props: &ContactSelectModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        }
        .into_any();
    }

    let title = props.title.clone();
    let contacts = props.contacts.clone();
    let selected_index = props.selected_index;
    let selected_ids = props.selected_ids.clone();
    let multi_select = props.multi_select;
    let error = props.error.clone();

    // Determine border color based on state
    let border_color = if !error.is_empty() {
        Theme::ERROR
    } else {
        Theme::PRIMARY
    };

    // Header props
    let header_props = ModalHeaderProps::new(title);

    // Footer props - conditionally include "Space" hint for multi-select
    let mut footer_hints = vec![
        KeyHint::new("Esc", "Cancel"),
        KeyHint::new("↑↓", "Navigate"),
    ];
    if multi_select {
        footer_hints.push(KeyHint::new("Space", "Toggle"));
    }
    footer_hints.push(KeyHint::new(
        "Enter",
        if multi_select { "Done" } else { "Select" },
    ));
    let footer_props = ModalFooterProps::new(footer_hints);

    // Error status
    let error_status = if !error.is_empty() {
        ModalStatus::Error(error)
    } else {
        ModalStatus::Idle
    };

    element! {
        ModalContent(
            flex_direction: FlexDirection::Column,
            border_style: Borders::PRIMARY,
            border_color: Some(border_color),
        ) {
            // Header
            #(Some(modal_header(&header_props).into()))

            // Body - contact list
            View(
                width: 100pct,
                padding: Spacing::MODAL_PADDING,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Scroll,
            ) {
                #(if contacts.is_empty() {
                    vec![element! {
                        View {
                            Text(content: "No contacts available", color: Theme::TEXT_MUTED)
                        }
                    }]
                } else {
                                        contacts.iter().enumerate().map(|(idx, contact)| {
                        let is_selected = idx == selected_index;
                        let is_checked =
                            multi_select && selected_ids.iter().any(|i| i == &contact.id);

                        // Use consistent list item colors
                        let bg = if is_selected {
                            Theme::LIST_BG_SELECTED
                        } else {
                            Theme::LIST_BG_NORMAL
                        };
                        let text_color = if is_selected {
                            Theme::LIST_TEXT_SELECTED
                        } else {
                            Theme::LIST_TEXT_NORMAL
                        };
                        let pointer_color = if is_selected {
                            Theme::LIST_TEXT_SELECTED
                        } else {
                            Theme::PRIMARY
                        };

                        let name = contact.nickname.clone();
                        let id = contact.id.clone();
                        let pointer = if is_selected { "➤ " } else { "  " };
                        let checkbox = if multi_select {
                            if is_checked { "[x] " } else { "[ ] " }
                        } else {
                            ""
                        };

                        element! {
                            View(
                                key: id,
                                flex_direction: FlexDirection::Row,
                                background_color: bg,
                                padding_left: Spacing::XS,
                            ) {
                                Text(content: pointer.to_string(), color: pointer_color)
                                Text(content: checkbox.to_string(), color: pointer_color)
                                Text(content: name, color: text_color)
                            }
                        }
                    }).collect()

                })

                // Error message
                #(Some(status_message(&error_status).into()))
            }

            // Footer
            #(Some(modal_footer(&footer_props).into()))
        }
    }
    .into_any()
}

/// State for contact select modal
#[derive(Clone, Debug, Default)]
pub struct ContactSelectState {
    /// Whether the modal is visible
    pub visible: bool,
    /// Modal title
    pub title: String,
    /// Available contacts
    pub contacts: Vec<Contact>,
    /// Currently selected index
    pub selected_index: usize,
    /// Error message if any
    pub error: Option<String>,
}

impl ContactSelectState {
    /// Create a new state
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the modal with contacts
    pub fn show(&mut self, title: &str, contacts: Vec<Contact>) {
        self.visible = true;
        self.title = title.to_string();
        self.contacts = contacts;
        self.selected_index = 0;
        self.error = None;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
        self.title.clear();
        self.contacts.clear();
        self.selected_index = 0;
        self.error = None;
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.selected_index + 1 < self.contacts.len() {
            self.selected_index += 1;
        }
    }

    /// Set an error
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Get the selected contact ID
    #[must_use]
    pub fn get_selected_id(&self) -> Option<String> {
        self.contacts.get(self.selected_index).map(|c| c.id.clone())
    }

    /// Check if selection is valid
    #[must_use]
    pub fn can_select(&self) -> bool {
        !self.contacts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::types::ContactStatus;

    #[test]
    fn test_contact_select_state() {
        let mut state = ContactSelectState::new();
        assert!(!state.visible);
        assert!(state.contacts.is_empty());

        let contacts = vec![
            Contact::new("c1", "Alice").with_status(ContactStatus::Active),
            Contact::new("c2", "Bob").with_status(ContactStatus::Active),
            Contact::new("c3", "Carol").with_status(ContactStatus::Active),
        ];

        state.show("Invite to Home", contacts);
        assert!(state.visible);
        assert_eq!(state.contacts.len(), 3);
        assert_eq!(state.selected_index, 0);
        assert!(state.can_select());

        // Navigate
        state.select_next();
        assert_eq!(state.selected_index, 1);
        assert_eq!(state.get_selected_id(), Some("c2".to_string()));

        state.select_prev();
        assert_eq!(state.selected_index, 0);
        assert_eq!(state.get_selected_id(), Some("c1".to_string()));

        // Hide
        state.hide();
        assert!(!state.visible);
        assert!(state.contacts.is_empty());
    }
}
