//! # Contact Select Modal
//!
//! Modal for selecting a contact from a list.

use iocraft::prelude::*;
use std::sync::Arc;

use super::modal::ModalContent;
use crate::tui::theme::Theme;
use crate::tui::types::Contact;

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
    let error = props.error.clone();

    // Determine border color based on state
    let border_color = if !error.is_empty() {
        Theme::ERROR
    } else {
        Theme::PRIMARY
    };

    element! {
        ModalContent(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: Some(border_color),
        ) {
            // Header
            View(
                width: 100pct,
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

            // Body - contact list
            View(
                width: 100pct,
                padding: 2,
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
                        // Use consistent list item colors
                        let bg = if is_selected { Theme::LIST_BG_SELECTED } else { Theme::LIST_BG_NORMAL };
                        let text_color = if is_selected { Theme::LIST_TEXT_SELECTED } else { Theme::LIST_TEXT_NORMAL };
                        let pointer_color = if is_selected { Theme::LIST_TEXT_SELECTED } else { Theme::PRIMARY };
                        let name = contact.petname.clone();
                        let id = contact.id.clone();
                        let pointer = if is_selected { "▸ " } else { "  " }.to_string();
                        element! {
                            View(
                                key: id,
                                flex_direction: FlexDirection::Row,
                                background_color: bg,
                                padding_left: 1,
                            ) {
                                Text(content: pointer, color: pointer_color)
                                Text(content: name, color: text_color)
                            }
                        }
                    }).collect()
                })

                // Error message (if any)
                #(if !error.is_empty() {
                    Some(element! {
                        View(margin_top: 1) {
                            Text(content: error.clone(), color: Theme::ERROR)
                        }
                    })
                } else {
                    None
                })
            }

            // Footer with key hints
            View(
                width: 100pct,
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
                    Text(content: "↑↓", color: Theme::SECONDARY)
                    Text(content: "Navigate", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: 2) {
                    Text(content: "Enter", color: Theme::SECONDARY)
                    Text(content: "Select", color: Theme::TEXT_MUTED)
                }
            }
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
    pub fn get_selected_id(&self) -> Option<String> {
        self.contacts.get(self.selected_index).map(|c| c.id.clone())
    }

    /// Check if selection is valid
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

        state.show("Invite to Block", contacts);
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
