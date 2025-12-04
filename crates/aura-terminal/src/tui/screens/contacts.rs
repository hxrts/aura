//! # Contacts Screen
//!
//! Petname management

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::{EmptyState, KeyHintsBar, StatusIndicator};
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{Contact, ContactStatus, KeyHint};

/// Callback type for editing a contact's petname (contact_id: String)
pub type EditPetnameCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Callback type for toggling guardian status (contact_id: String)
pub type ToggleGuardianCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Props for ContactItem
#[derive(Default, Props)]
pub struct ContactItemProps {
    pub contact: Contact,
    pub is_selected: bool,
}

/// A single contact in the list
#[component]
pub fn ContactItem(props: &ContactItemProps) -> impl Into<AnyElement<'static>> {
    let c = &props.contact;
    let bg = if props.is_selected {
        Theme::BG_SELECTED
    } else {
        Theme::BG_DARK
    };

    let status = match c.status {
        ContactStatus::Active => crate::tui::components::Status::Online,
        ContactStatus::Pending => crate::tui::components::Status::Warning,
        ContactStatus::Blocked => crate::tui::components::Status::Error,
    };

    let name = c.petname.clone();
    let guardian_badge = if c.is_guardian { " [guardian]" } else { "" }.to_string();
    let suggestion = c
        .suggested_name
        .as_ref()
        .map(|s| format!(" (suggests: {})", s))
        .unwrap_or_default();

    element! {
        View(
            flex_direction: FlexDirection::Row,
            background_color: bg,
            padding_left: Spacing::XS,
            padding_right: Spacing::XS,
            gap: Spacing::XS,
        ) {
            StatusIndicator(status: status, icon_only: true)
            Text(content: name, color: Theme::TEXT)
            Text(content: guardian_badge, color: Theme::SECONDARY)
            Text(content: suggestion, color: Theme::TEXT_MUTED)
        }
    }
}

/// Props for ContactList
#[derive(Default, Props)]
pub struct ContactListProps {
    pub contacts: Vec<Contact>,
    pub selected_index: usize,
    pub focused: bool,
}

/// List of contacts
#[component]
pub fn ContactList(props: &ContactListProps) -> impl Into<AnyElement<'static>> {
    let border_color = if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    let count = props.contacts.len();
    let title = format!("Contacts ({})", count);
    let contacts = props.contacts.clone();
    let selected = props.selected_index;

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            border_style: BorderStyle::Round,
            border_color: border_color,
        ) {
            View(padding_left: Spacing::PANEL_PADDING) {
                Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                padding: Spacing::PANEL_PADDING,
                overflow: Overflow::Scroll,
            ) {
                #(if contacts.is_empty() {
                    vec![element! {
                        View {
                            EmptyState(title: "No contacts yet".to_string())
                        }
                    }]
                } else {
                    contacts.iter().enumerate().map(|(idx, contact)| {
                        let is_selected = idx == selected;
                        element! {
                            View {
                                ContactItem(contact: contact.clone(), is_selected: is_selected)
                            }
                        }
                    }).collect::<Vec<_>>()
                })
            }
        }
    }
}

/// Props for ContactDetail
#[derive(Default, Props)]
pub struct ContactDetailProps {
    pub contact: Option<Contact>,
    pub focused: bool,
}

/// Detail panel for selected contact
#[component]
pub fn ContactDetail(props: &ContactDetailProps) -> impl Into<AnyElement<'static>> {
    let border_color = if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            border_style: BorderStyle::Round,
            border_color: border_color,
        ) {
            View(padding_left: Spacing::PANEL_PADDING) {
                Text(content: "Details", weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                padding: Spacing::PANEL_PADDING,
            ) {
                #(if let Some(c) = &props.contact {
                    let petname = format!("Petname: {}", c.petname);
                    let status = format!("Status: {}", match c.status {
                        ContactStatus::Active => "Active",
                        ContactStatus::Pending => "Pending",
                        ContactStatus::Blocked => "Blocked",
                    });
                    let guardian = if c.is_guardian { "Yes" } else { "No" };
                    let guardian_text = format!("Guardian: {}", guardian);
                    let suggestion = c.suggested_name.as_ref()
                        .map(|s| format!("Suggested name: {}", s))
                        .unwrap_or_else(|| "No suggestion".to_string());

                    vec![
                        element! { View { Text(content: petname, color: Theme::TEXT) } },
                        element! { View { Text(content: status, color: Theme::TEXT) } },
                        element! { View { Text(content: guardian_text, color: Theme::TEXT) } },
                        element! { View(height: 1) },
                        element! { View { Text(content: suggestion, color: Theme::TEXT_MUTED) } },
                    ]
                } else {
                    vec![element! {
                        View {
                            Text(content: "Select a contact to view details", color: Theme::TEXT_MUTED)
                        }
                    }]
                })
            }
        }
    }
}

/// Props for ContactsScreen
#[derive(Default, Props)]
pub struct ContactsScreenProps {
    pub contacts: Vec<Contact>,
    /// Callback when editing a contact's petname
    pub on_edit_petname: Option<EditPetnameCallback>,
    /// Callback when toggling guardian status
    pub on_toggle_guardian: Option<ToggleGuardianCallback>,
}

/// The contacts screen
#[component]
pub fn ContactsScreen(
    props: &ContactsScreenProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let selected = hooks.use_state(|| 0usize);
    let detail_focused = hooks.use_state(|| false);

    let hints = vec![
        KeyHint::new("↑↓", "Navigate"),
        KeyHint::new("Tab", "Switch panel"),
        KeyHint::new("e", "Edit petname"),
        KeyHint::new("g", "Toggle guardian"),
        KeyHint::new("Esc", "Back"),
    ];

    let contacts = props.contacts.clone();
    let current_selected = selected.get();
    let is_detail_focused = detail_focused.get();
    let selected_contact = contacts.get(current_selected).cloned();

    // Clone callbacks for event handler
    let on_edit_petname = props.on_edit_petname.clone();
    let on_toggle_guardian = props.on_toggle_guardian.clone();

    hooks.use_terminal_events({
        let mut selected = selected.clone();
        let mut detail_focused = detail_focused.clone();
        let count = contacts.len();
        let contacts_for_handler = contacts.clone();
        move |event| match event {
            TerminalEvent::Key(KeyEvent { code, .. }) => match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    let current = selected.get();
                    if current > 0 {
                        selected.set(current - 1);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let current = selected.get();
                    if current + 1 < count {
                        selected.set(current + 1);
                    }
                }
                KeyCode::Tab => {
                    detail_focused.set(!detail_focused.get());
                }
                KeyCode::Enter => {
                    detail_focused.set(true);
                }
                // Edit petname - triggers callback with contact_id
                KeyCode::Char('e') => {
                    if let Some(contact) = contacts_for_handler.get(selected.get()) {
                        if let Some(ref callback) = on_edit_petname {
                            callback(contact.id.clone());
                        }
                    }
                }
                // Toggle guardian - triggers callback with contact_id
                KeyCode::Char('g') => {
                    if let Some(contact) = contacts_for_handler.get(selected.get()) {
                        if let Some(ref callback) = on_toggle_guardian {
                            callback(contact.id.clone());
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    });

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
        ) {
            // Header
            View(
                padding: Spacing::PANEL_PADDING,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(content: "Contacts", weight: Weight::Bold, color: Theme::PRIMARY)
            }

            // Main content: list + detail
            View(
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
                gap: 1,
            ) {
                // List (50%)
                View(width: 50pct) {
                    ContactList(
                        contacts: contacts.clone(),
                        selected_index: current_selected,
                        focused: !is_detail_focused,
                    )
                }
                // Detail (50%)
                View(flex_grow: 1.0) {
                    ContactDetail(
                        contact: selected_contact,
                        focused: is_detail_focused,
                    )
                }
            }

            // Key hints
            KeyHintsBar(hints: hints)
        }
    }
}

/// Run the contacts screen with sample data
pub async fn run_contacts_screen() -> std::io::Result<()> {
    let contacts = vec![
        Contact::new("c1", "Alice")
            .with_status(ContactStatus::Active)
            .guardian(),
        Contact::new("c2", "Bob").with_status(ContactStatus::Active),
        Contact::new("c3", "Charlie")
            .with_status(ContactStatus::Pending)
            .with_suggestion("Charles"),
        Contact::new("c4", "Diana").with_status(ContactStatus::Blocked),
    ];

    element! {
        ContactsScreen(contacts: contacts)
    }
    .fullscreen()
    .await
}
