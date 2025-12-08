//! # Contacts Screen
//!
//! Petname management
//!
//! ## Reactive Signal Subscription
//!
//! When `AppCoreContext` is available, this screen subscribes to contacts state
//! changes via `use_future` and futures-signals. Updates are pushed to the
//! component automatically, triggering re-renders when data changes.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::{
    DiscoveredPeerInfo, DiscoveredPeersPanel, DiscoveredPeersState, EmptyState, InvitePeerCallback,
    StatusIndicator, TextInputModal, TextInputState,
};
use crate::tui::navigation::{is_nav_key_press, navigate_list, NavKey, NavThrottle, TwoPanelFocus};
use crate::tui::hooks::AppCoreContext;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{Contact, ContactStatus};

/// Callback type for updating a contact's petname (contact_id: String, new_petname: String)
pub type UpdatePetnameCallback = Arc<dyn Fn(String, String) + Send + Sync>;

/// Callback type for toggling guardian status (contact_id: String)
pub type ToggleGuardianCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Callback type for starting a direct chat with a contact (contact_id: String)
pub type StartChatCallback = Arc<dyn Fn(String) + Send + Sync>;

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
#[derive(Default, Props)]
pub struct ContactsScreenProps {
    pub contacts: Vec<Contact>,
    /// Discovered LAN peers
    pub discovered_peers: Vec<DiscoveredPeerInfo>,
    /// Callback when updating a contact's petname
    pub on_update_petname: Option<UpdatePetnameCallback>,
    /// Callback when toggling guardian status
    pub on_toggle_guardian: Option<ToggleGuardianCallback>,
    /// Callback when starting a direct chat with a contact
    pub on_start_chat: Option<StartChatCallback>,
    /// Callback when inviting a discovered LAN peer
    pub on_invite_lan_peer: Option<InvitePeerCallback>,
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
    if let Some(ctx) = app_ctx {
        hooks.use_future({
            let mut reactive_contacts = reactive_contacts.clone();
            let app_core = ctx.app_core.clone();
            async move {
                use futures_signals::signal::SignalExt;

                let signal = {
                    let core = app_core.read().await;
                    core.contacts_signal()
                };

                signal
                    .for_each(|contacts_state| {
                        let contacts: Vec<Contact> = contacts_state
                            .contacts
                            .iter()
                            .map(convert_contact)
                            .collect();

                        reactive_contacts.set(contacts);
                        async {}
                    })
                    .await;
            }
        });
    }

    // Use reactive state for rendering
    let contacts = reactive_contacts.read().clone();

    let mut selected = hooks.use_state(|| 0usize);
    let mut panel_focus = hooks.use_state(|| TwoPanelFocus::List);

    // Modal state for editing petnames
    let petname_modal_state = hooks.use_state(TextInputState::new);

    // LAN discovered peers state
    let mut lan_peers_state = hooks.use_state({
        let initial_peers = props.discovered_peers.clone();
        move || {
            let mut state = DiscoveredPeersState::new();
            state.set_peers(initial_peers);
            state
        }
    });

    // Update LAN peers when props change
    {
        let mut state = lan_peers_state.read().clone();
        if state.peers.len() != props.discovered_peers.len() {
            state.set_peers(props.discovered_peers.clone());
            lan_peers_state.set(state);
        }
    }

    let current_selected = selected.get();
    let current_focus = panel_focus.get();
    let is_detail_focused = current_focus == TwoPanelFocus::Detail;
    let selected_contact = contacts.get(current_selected).cloned();

    // Clone callbacks for event handler
    let on_update_petname = props.on_update_petname.clone();
    let on_toggle_guardian = props.on_toggle_guardian.clone();
    let on_start_chat = props.on_start_chat.clone();
    let on_invite_lan_peer = props.on_invite_lan_peer.clone();

    // Throttle for navigation keys - persists across renders using use_ref
    let mut nav_throttle = hooks.use_ref(NavThrottle::new);

    hooks.use_terminal_events({
        let mut petname_modal_state = petname_modal_state.clone();
        let count = contacts.len();
        let contacts_for_handler = contacts.clone();
        move |event| {
            // Check if modal is visible
            let modal_visible = petname_modal_state.read().visible;

            // Handle navigation keys first (only when modal is not visible)
            if !modal_visible {
                if let Some(nav_key) = is_nav_key_press(&event) {
                    if nav_throttle.write().try_navigate() {
                        match nav_key {
                            // Horizontal: toggle between list and detail
                            NavKey::Left | NavKey::Right => {
                                let new_focus = panel_focus.get().navigate(nav_key);
                                panel_focus.set(new_focus);
                            }
                            // Vertical: navigate within list when list is focused
                            NavKey::Up | NavKey::Down => {
                                if panel_focus.get() == TwoPanelFocus::List && count > 0 {
                                    let new_idx = navigate_list(selected.get(), count, nav_key);
                                    selected.set(new_idx);
                                }
                            }
                        }
                    }
                    return;
                }
            }

            match event {
                TerminalEvent::Key(KeyEvent { code, .. }) => {
                    if modal_visible {
                        // Handle modal keys
                        match code {
                            KeyCode::Esc => {
                                let mut state = petname_modal_state.read().clone();
                                state.hide();
                                petname_modal_state.set(state);
                            }
                            KeyCode::Enter => {
                                let state = petname_modal_state.read().clone();
                                if state.can_submit() {
                                    if let Some(ref callback) = on_update_petname {
                                        if let Some(contact_id) = state.get_context_id() {
                                            callback(contact_id.to_string(), state.get_value().to_string());
                                        }
                                    }
                                    // Close modal
                                    let mut state = petname_modal_state.read().clone();
                                    state.hide();
                                    petname_modal_state.set(state);
                                }
                            }
                            KeyCode::Backspace => {
                                let mut state = petname_modal_state.read().clone();
                                state.pop_char();
                                petname_modal_state.set(state);
                            }
                            KeyCode::Char(c) => {
                                let mut state = petname_modal_state.read().clone();
                                state.push_char(c);
                                petname_modal_state.set(state);
                            }
                            _ => {}
                        }
                    } else {
                        // Normal screen keys (non-navigation hotkeys)
                        match code {
                            KeyCode::Enter => {
                                panel_focus.set(TwoPanelFocus::Detail);
                            }
                            // Edit petname - show modal with current petname
                            KeyCode::Char('e') => {
                                if let Some(contact) = contacts_for_handler.get(selected.get()) {
                                    let mut state = petname_modal_state.read().clone();
                                    state.show(
                                        "Edit Petname",
                                        &contact.petname,
                                        "Enter petname...",
                                        Some(contact.id.clone()),
                                    );
                                    petname_modal_state.set(state);
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
                            // Start chat - triggers callback with contact_id
                            KeyCode::Char('c') => {
                                if let Some(contact) = contacts_for_handler.get(selected.get()) {
                                    if let Some(ref callback) = on_start_chat {
                                        callback(contact.id.clone());
                                    }
                                }
                            }
                            // Invite discovered LAN peer
                            KeyCode::Char('i') => {
                                let state = lan_peers_state.read();
                                if let Some(peer) = state.get_selected() {
                                    if let Some(ref callback) = on_invite_lan_peer {
                                        callback(peer.authority_id.clone(), peer.address.clone());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    });

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
            flex_grow: 1.0,
            flex_shrink: 1.0,
            overflow: Overflow::Hidden,
        ) {
            // Main content: list + detail
            View(
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Hidden,
                gap: Spacing::XS,
            ) {
                // Left column: LAN peers + contacts list (30%)
                View(
                    width: 30pct,
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 1.0,
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
                                    selected_index: state.selected_index,
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
                // Detail (70%)
                ContactDetail(
                    contact: selected_contact,
                    focused: is_detail_focused,
                )
            }

            // Petname edit modal (overlays everything)
            #(if petname_modal_state.read().visible {
                let modal_state = petname_modal_state.read().clone();
                Some(element! {
                    TextInputModal(
                        visible: true,
                        focused: true,
                        title: modal_state.title.clone(),
                        value: modal_state.value.clone(),
                        placeholder: modal_state.placeholder.clone(),
                        error: modal_state.error.clone().unwrap_or_default(),
                        submitting: modal_state.submitting,
                    )
                })
            } else {
                None
            })
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
