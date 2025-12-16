//! # Contacts Screen
//!
//! Petname management and invitations
//!
//! ## Reactive Signal Subscription
//!
//! When `AppCoreContext` is available, this screen subscribes to contacts state
//! changes via the unified `ReactiveEffects` system. Updates are pushed to the
//! component automatically, triggering re-renders when data changes.
//!
//! Uses `aura_app::signal_defs::CONTACTS_SIGNAL` with `ReactiveEffects::subscribe()`.
//!
//! ## Invitation Flows
//!
//! The contacts screen now handles both:
//! - **Accept Invitation (i)**: Import a contact invitation code received out-of-band
//! - **Send Invitation (n)**: Generate a new invitation code to share with others
//!
//! In demo mode, Ctrl+A and Ctrl+L fill Alice's and Carol's codes respectively.
//!
//! ## Pure View Component
//!
//! This screen is a pure view that renders based on props from TuiState.
//! All event handling is done by the parent TuiShell (IoApp) via the state machine.

use iocraft::prelude::*;
use std::sync::Arc;

use aura_app::signal_defs::CONTACTS_SIGNAL;
use aura_core::effects::reactive::ReactiveEffects;

use crate::tui::components::{
    DiscoveredPeerInfo, DiscoveredPeersPanel, DiscoveredPeersState, EmptyState, InvitePeerCallback,
    StatusIndicator,
};
use crate::tui::hooks::AppCoreContext;
use crate::tui::layout::dim;
use crate::tui::navigation::TwoPanelFocus;
use crate::tui::props::ContactsViewProps;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{Contact, ContactStatus};

/// Callback type for updating a contact's petname (contact_id: String, new_petname: String)
pub type UpdatePetnameCallback = Arc<dyn Fn(String, String) + Send + Sync>;

/// Callback type for starting a direct chat with a contact (contact_id: String)
pub type StartChatCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Callback type for importing an invitation code (code) -> triggers import flow
pub type ImportInvitationCallback = Arc<dyn Fn(String) + Send + Sync>;

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
    // Use consistent list item colors
    let bg = if props.is_selected {
        Theme::LIST_BG_SELECTED
    } else {
        Theme::LIST_BG_NORMAL
    };

    let text_color = if props.is_selected {
        Theme::LIST_TEXT_SELECTED
    } else {
        Theme::LIST_TEXT_NORMAL
    };

    let status = match c.status {
        ContactStatus::Active => crate::tui::components::Status::Online,
        ContactStatus::Pending => crate::tui::components::Status::Warning,
        ContactStatus::Blocked => crate::tui::components::Status::Error,
    };

    let name = c.petname.clone();
    let guardian_badge = if c.is_guardian { " [guardian]" } else { "" }.to_string();

    element! {
        View(
            flex_direction: FlexDirection::Row,
            background_color: bg,
            padding_left: Spacing::XS,
            padding_right: Spacing::XS,
            gap: Spacing::XS,
            overflow: Overflow::Hidden,
        ) {
            StatusIndicator(status: status, icon_only: true)
            Text(content: name, color: text_color, wrap: TextWrap::NoWrap)
            Text(content: guardian_badge, color: Theme::SECONDARY)
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
            flex_shrink: 1.0,
            border_style: BorderStyle::Round,
            border_color: border_color,
            overflow: Overflow::Hidden,
        ) {
            View(padding_left: Spacing::PANEL_PADDING) {
                Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
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
                        let id = contact.id.clone();
                        element! {
                            View(key: id) {
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
            flex_shrink: 1.0,
            border_style: BorderStyle::Round,
            border_color: border_color,
            overflow: Overflow::Hidden,
        ) {
            View(padding_left: Spacing::PANEL_PADDING) {
                Text(content: "Details", weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                padding: Spacing::PANEL_PADDING,
                overflow: Overflow::Scroll,
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
///
/// ## Compile-Time Safety
///
/// The `view` field is a required struct that embeds all view state from TuiState.
/// This makes it a **compile-time error** to forget any view state field, because
/// the entire `ContactsViewProps` struct must be passed - you can't accidentally
/// omit individual fields like `petname_modal_visible`.
#[derive(Default, Props)]
pub struct ContactsScreenProps {
    // === Domain data (from reactive signals) ===
    pub contacts: Vec<Contact>,
    /// Discovered LAN peers
    pub discovered_peers: Vec<DiscoveredPeerInfo>,

    // === View state from TuiState (REQUIRED - compile-time enforced) ===
    /// All view state extracted from TuiState via `extract_contacts_view_props()`.
    /// This is a single struct field so forgetting any view state is a compile error.
    pub view: ContactsViewProps,

    /// LAN peers selection index (local UI state, not from TuiState)
    pub lan_peers_selection: usize,

    // === Callbacks ===
    /// Callback when updating a contact's petname
    pub on_update_petname: Option<UpdatePetnameCallback>,
    /// Callback when starting a direct chat with a contact
    pub on_start_chat: Option<StartChatCallback>,
    /// Callback when inviting a discovered LAN peer
    pub on_invite_lan_peer: Option<InvitePeerCallback>,
    /// Callback when importing an invitation code
    pub on_import_invitation: Option<ImportInvitationCallback>,
}

/// Convert aura-app contact to TUI contact
fn convert_contact(c: &aura_app::views::Contact) -> Contact {
    // Determine contact status based on online state
    let status = if c.is_online {
        ContactStatus::Active
    } else {
        ContactStatus::Pending
    };

    Contact {
        id: c.id.clone(),
        petname: c.petname.clone(),
        suggested_name: c.suggested_name.clone(),
        status,
        is_guardian: c.is_guardian,
    }
}

/// The contacts screen
///
/// ## Pure View Component
///
/// This screen is a pure view that renders based on props from TuiState.
/// All event handling is done by the parent TuiShell (IoApp) via the state machine.
///
/// ## Reactive Updates
///
/// When `AppCoreContext` is available in the context tree, this component will
/// subscribe to contacts state signals and automatically update when:
/// - Contacts are added/removed
/// - Petnames are changed
/// - Guardian status is toggled
/// - Online status changes
#[component]
pub fn ContactsScreen(
    props: &ContactsScreenProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    // Try to get AppCoreContext for reactive signal subscription
    let app_ctx = hooks.try_use_context::<AppCoreContext>();

    // Initialize reactive state from props
    let reactive_contacts = hooks.use_state({
        let initial = props.contacts.clone();
        move || initial
    });

    // Subscribe to contacts signal updates if AppCoreContext is available
    // Uses the unified ReactiveEffects system from aura-core
    if let Some(ctx) = app_ctx {
        hooks.use_future({
            let mut reactive_contacts = reactive_contacts.clone();
            let app_core = ctx.app_core.clone();
            async move {
                // FIRST: Read current signal value to catch up on any changes
                // that happened while this screen was unmounted (e.g., contacts
                // added via invitation import while on another screen)
                {
                    let core = app_core.read().await;
                    if let Ok(contacts_state) = core.read(&*CONTACTS_SIGNAL).await {
                        let contacts: Vec<Contact> = contacts_state
                            .contacts
                            .iter()
                            .map(convert_contact)
                            .collect();
                        reactive_contacts.set(contacts);
                    }
                }

                // THEN: Subscribe for future updates
                let mut stream = {
                    let core = app_core.read().await;
                    core.subscribe(&*CONTACTS_SIGNAL)
                };

                // Subscribe to signal updates - runs until component unmounts
                while let Ok(contacts_state) = stream.recv().await {
                    let contacts: Vec<Contact> = contacts_state
                        .contacts
                        .iter()
                        .map(convert_contact)
                        .collect();

                    reactive_contacts.set(contacts);
                }
            }
        });
    }

    // Use reactive state for rendering
    let contacts = reactive_contacts.read().clone();

    // LAN discovered peers state (still using local state for peer data updates)
    let lan_peers_state = hooks.use_state({
        let initial_peers = props.discovered_peers.clone();
        move || {
            let mut state = DiscoveredPeersState::new();
            state.set_peers(initial_peers);
            state
        }
    });

    // === Pure view: Use props.view from TuiState instead of local state ===
    let current_selected = props.view.selected_index;
    let current_focus = props.view.focus;
    let is_detail_focused = current_focus == TwoPanelFocus::Detail;
    let selected_contact = contacts.get(current_selected).cloned();

    // NOTE: Modals have been moved to app.rs root level. See modal_frame.rs for details.

    // === Pure view: No use_terminal_events ===
    // All event handling is done by IoApp (the shell) via the state machine.
    // This component is purely presentational.

    // Layout: Full 25 rows for content (no input bar on this screen)
    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            overflow: Overflow::Hidden,
        ) {
            // Main content: list + detail - full 25 rows
            View(
                flex_direction: FlexDirection::Row,
                height: dim::MIDDLE_HEIGHT,
                overflow: Overflow::Hidden,
                gap: Spacing::XS,
            ) {
                // Left column: LAN peers + contacts list (24 chars = 30% of 80)
                View(
                    width: 24,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::Hidden,
                    gap: 0,
                ) {
                    // Discovered LAN peers panel (only show if there are peers)
                    #({
                        let state = lan_peers_state.read();
                        if state.has_peers() {
                            Some(element! {
                                DiscoveredPeersPanel(
                                    peers: state.peers.clone(),
                                    selected_index: props.lan_peers_selection,
                                    focused: false,
                                )
                            })
                        } else {
                            None
                        }
                    })
                    // Contacts list
                    ContactList(
                        contacts: contacts.clone(),
                        selected_index: current_selected,
                        focused: !is_detail_focused,
                    )
                }
                // Detail (remaining width ~55 chars)
                ContactDetail(
                    contact: selected_contact,
                    focused: is_detail_focused,
                )
            }

            // NOTE: All modals have been moved to app.rs root level and wrapped with ModalFrame
            // for consistent positioning. See the "CONTACTS SCREEN MODALS" section in app.rs.
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
        Contact::new("c3", "Carol")
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
