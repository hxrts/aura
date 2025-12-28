//! # Contacts Screen
//!
//! Nickname management and invitations
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
//! - **Accept Invitation (a)**: Accept a contact invitation code received out-of-band
//! - **Send Invitation (n)**: Generate a new invitation code to share with others
//!
//! In demo mode, Ctrl+A and Ctrl+L fill Alice's and Carol's codes respectively.
//!
//! ## Pure View Component
//!
//! This screen is a pure view that renders based on props from TuiState.
//! All event handling is done by the parent TuiShell (IoApp) via the state machine.

use iocraft::prelude::*;

use aura_app::signal_defs::{CONTACTS_SIGNAL, DISCOVERED_PEERS_SIGNAL};

use crate::tui::callbacks::{ImportInvitationCallback, StartChatCallback, UpdateNicknameCallback};
use crate::tui::components::{
    DetailPanel, DiscoveredPeerInfo, DiscoveredPeersPanel, DiscoveredPeersState,
    InvitePeerCallback, KeyValue, ListPanel, StatusIndicator,
};
use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::layout::dim;
use crate::tui::props::ContactsViewProps;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{Contact, ContactStatus};

/// Props for ContactItem
#[derive(Default, Props)]
pub struct ContactItemProps {
    pub contact: Contact,
    pub is_selected: bool,
}

/// A single contact in the list
///
/// Uses the same selection indicator pattern as Settings screen.
#[component]
pub fn ContactItem(props: &ContactItemProps) -> impl Into<AnyElement<'static>> {
    let c = &props.contact;

    // Selection styling (matches SimpleSelectableItem)
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
        ContactStatus::Offline => crate::tui::components::Status::Offline,
        ContactStatus::Pending => crate::tui::components::Status::Warning,
        ContactStatus::Blocked => crate::tui::components::Status::Error,
    };

    let name = if !c.nickname.is_empty() {
        c.nickname.clone()
    } else if let Some(suggested) = &c.suggested_name {
        suggested.clone()
    } else {
        let id = c.id.clone();
        let short = id.chars().take(8).collect::<String>();
        format!("{short}...")
    };
    let guardian_badge = if c.is_guardian { " [guardian]" } else { "" }.to_string();

    // Selection indicator: triangle when selected, space otherwise
    let indicator = if props.is_selected { "➤" } else { " " };
    let indicator_color = if props.is_selected {
        Theme::PRIMARY
    } else {
        text_color
    };

    element! {
        View(
            flex_direction: FlexDirection::Row,
            background_color: bg,
            padding_right: 1,
            overflow: Overflow::Hidden,
        ) {
            Text(content: format!("{} ", indicator), color: indicator_color)
            StatusIndicator(status: status, icon_only: true)
            View(margin_left: Spacing::XS) {
                Text(content: name, color: text_color, wrap: TextWrap::NoWrap)
            }
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
    let contacts = props.contacts.clone();
    let selected = props.selected_index;

    // Build list items
    let items: Vec<AnyElement<'static>> = contacts
        .iter()
        .enumerate()
        .map(|(idx, contact)| {
            let is_selected = idx == selected;
            let id = contact.id.clone();
            element! {
                View(key: id) {
                    ContactItem(contact: contact.clone(), is_selected: is_selected)
                }
            }
            .into_any()
        })
        .collect();

    element! {
        ListPanel(
            title: "Contacts".to_string(),
            count: contacts.len(),
            focused: props.focused,
            items: items,
            empty_message: "No contacts yet".to_string(),
        )
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
    // Build content based on whether a contact is selected
    let content: Vec<AnyElement<'static>> = if let Some(c) = &props.contact {
        let status_label = match c.status {
            ContactStatus::Active => "Active",
            ContactStatus::Offline => "Offline",
            ContactStatus::Pending => "Pending",
            ContactStatus::Blocked => "Blocked",
        };
        let guardian = if c.is_guardian { "Yes" } else { "No" };

        vec![
            element! { KeyValue(label: "Nickname".to_string(), value: c.nickname.clone()) }
                .into_any(),
            element! { KeyValue(label: "Status".to_string(), value: status_label.to_string()) }
                .into_any(),
            element! { KeyValue(label: "Guardian".to_string(), value: guardian.to_string()) }
                .into_any(),
        ]
    } else {
        vec![]
    };

    element! {
        DetailPanel(
            title: "Details".to_string(),
            focused: props.focused,
            content: content,
            empty_message: "Select a contact to view details".to_string(),
        )
    }
}

/// Props for ContactsScreen
///
/// ## Compile-Time Safety
///
/// The `view` field is a required struct that embeds all view state from TuiState.
/// This makes it a **compile-time error** to forget any view state field, because
/// the entire `ContactsViewProps` struct must be passed - you can't accidentally
/// omit individual fields like `nickname_modal_visible`.
///
/// ## Reactive Data Model
///
/// Domain data (contacts, discovered_peers) is NOT passed as props.
/// Instead, the component subscribes to signals directly via AppCoreContext.
/// This ensures a single source of truth and prevents stale data bugs.
#[derive(Default, Props)]
pub struct ContactsScreenProps {
    // === View state from TuiState (REQUIRED - compile-time enforced) ===
    /// All view state extracted from TuiState via `extract_contacts_view_props()`.
    /// This is a single struct field so forgetting any view state is a compile error.
    pub view: ContactsViewProps,

    /// LAN peers selection index (local UI state, not from TuiState)
    pub lan_peers_selection: usize,

    // === Callbacks ===
    /// Callback when updating a contact's nickname
    pub on_update_nickname: Option<UpdateNicknameCallback>,
    /// Callback when starting a direct chat with a contact
    pub on_start_chat: Option<StartChatCallback>,
    /// Callback when inviting a discovered LAN peer
    pub on_invite_lan_peer: Option<InvitePeerCallback>,
    /// Callback when importing an invitation code
    pub on_import_invitation: Option<ImportInvitationCallback>,
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
/// - Nicknames are changed
/// - Guardian status is toggled
/// - Online status changes
#[component]
pub fn ContactsScreen(
    props: &ContactsScreenProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    // Get AppCoreContext for reactive signal subscription (required for domain data)
    let app_ctx = hooks.use_context::<AppCoreContext>();

    // Initialize reactive state with defaults - will be populated by signal subscriptions
    let reactive_contacts = hooks.use_state(Vec::new);

    // Subscribe to contacts signal updates
    // Uses the unified ReactiveEffects system from aura-core
    hooks.use_future({
        let mut reactive_contacts = reactive_contacts.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CONTACTS_SIGNAL, move |contacts_state| {
                let contacts: Vec<Contact> =
                    contacts_state.contacts.iter().map(Contact::from).collect();
                reactive_contacts.set(contacts);
            })
            .await;
        }
    });

    // Use reactive state for rendering (populated by signal subscription)
    let contacts = reactive_contacts.read().clone();

    // LAN discovered peers state (reactive via signal subscription)
    let lan_peers_state = hooks.use_state(DiscoveredPeersState::new);

    // Subscribe to discovered peers signal updates
    hooks.use_future({
        let mut lan_peers_state = lan_peers_state.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*DISCOVERED_PEERS_SIGNAL, move |peers_state| {
                let discovered_peers: Vec<DiscoveredPeerInfo> = peers_state
                    .peers
                    .iter()
                    .map(|p| {
                        DiscoveredPeerInfo::new(&p.authority_id, &p.address)
                            .with_method(&p.method)
                            .with_status(if p.invited {
                                crate::tui::components::PeerInvitationStatus::Pending
                            } else {
                                crate::tui::components::PeerInvitationStatus::None
                            })
                    })
                    .collect();

                let mut state = DiscoveredPeersState::new();
                state.set_peers(discovered_peers);
                lan_peers_state.set(state);
            })
            .await;
        }
    });

    // === Pure view: Use props.view from TuiState instead of local state ===
    let current_selected = props.view.selected_index;
    let selected_contact = contacts.get(current_selected).cloned();

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
            // Main content: list + detail - full 25 rows (matches settings screen ratio)
            View(
                flex_direction: FlexDirection::Row,
                height: dim::MIDDLE_HEIGHT,
                overflow: Overflow::Hidden,
                gap: dim::TWO_PANEL_GAP,
            ) {
                // Left column: LAN peers + contacts list (matches settings screen)
                View(
                    width: dim::TWO_PANEL_LEFT_WIDTH,
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
                        focused: false,
                    )
                }
                // Detail (matches settings screen width)
                ContactDetail(
                    contact: selected_contact,
                    focused: false,
                )
            }
        }
    }
}

/// Run the contacts screen (requires AppCoreContext for domain data)
pub async fn run_contacts_screen() -> std::io::Result<()> {
    // Note: This standalone runner won't have domain data without AppCoreContext.
    // Domain data is obtained via signal subscriptions when context is available.
    element! {
        ContactsScreen(
            view: ContactsViewProps::default(),
        )
    }
    .fullscreen()
    .await
}
