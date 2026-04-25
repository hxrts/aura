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
//! Uses `aura_app::ui::signals::CONTACTS_SIGNAL` with `ReactiveEffects::subscribe()`.
//!
//! ## Invitation Flows
//!
//! The contacts screen now handles both:
//! - **Accept Invitation (a)**: Accept a contact invite code received out-of-band
//! - **Send Invitation (n / i)**: Generate a new invite code to share with others
//! - **Invite Selected Contact To Channel (I)**: Send the selected contact a channel invitation
//!
//! In demo mode, `a` and `l` fill Alice's and Carol's codes respectively,
//! with Ctrl aliases available when the terminal preserves modifiers.
//!
//! ## Pure View Component
//!
//! This screen is a pure view that renders based on props from TuiState.
//! All event handling is done by the parent TuiShell (IoApp) via the state machine.

use iocraft::prelude::*;

use aura_app::harness_mode_enabled;
use aura_app::ui::contract::{contacts_friend_action_controls, ControlId};
use aura_app::ui::signals::{
    CONTACTS_SIGNAL, DISCOVERED_PEERS_SIGNAL, INVITATIONS_SIGNAL, SETTINGS_SIGNAL,
};

use crate::tui::callbacks::{StartChatCallback, UpdateNicknameCallback};
use crate::tui::components::{
    DiscoveredPeerInfo, DiscoveredPeersPanel, DiscoveredPeersState, InvitePeerCallback,
};
use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::layout::dim;
use crate::tui::props::ContactsViewProps;
use crate::tui::theme::{focus_border_color, Spacing, Theme};
use crate::tui::types::{
    short_id, Contact, ContactStatus, Invitation, InvitationDirection, InvitationStatus,
    InvitationType, ReadReceiptPolicyExt,
};
use aura_app::ui::signals::DiscoveredPeerMethod;
use aura_app::ui::types::{format_relative_time_from, ContactRelationshipState, EffectiveName};
use std::collections::HashSet;

fn contact_relationship_label(state: ContactRelationshipState) -> &'static str {
    match state {
        ContactRelationshipState::Contact => "Contact",
        ContactRelationshipState::PendingOutbound => "Pending outbound",
        ContactRelationshipState::PendingInbound => "Pending inbound",
        ContactRelationshipState::Friend => "Friend",
    }
}

fn contact_friend_action_hint(state: ContactRelationshipState) -> Option<String> {
    let controls = contacts_friend_action_controls(state);
    if controls.is_empty() {
        return None;
    }

    let labels = controls
        .iter()
        .filter_map(|control| match control {
            ControlId::ContactsSendFriendRequestButton => {
                Some(format!("{} send friend request", control.activation_key()?))
            }
            ControlId::ContactsAcceptFriendRequestButton => Some(format!(
                "{} accept friend request",
                control.activation_key()?
            )),
            ControlId::ContactsDeclineFriendRequestButton => Some(format!(
                "{} decline friend request",
                control.activation_key()?
            )),
            ControlId::ContactsRemoveFriendButton => match state {
                ContactRelationshipState::PendingOutbound => Some(format!(
                    "{} cancel friend request",
                    control.activation_key()?
                )),
                ContactRelationshipState::Friend => {
                    Some(format!("{} remove friend", control.activation_key()?))
                }
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    if labels.is_empty() {
        None
    } else {
        Some(labels.join(", "))
    }
}

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

    let name = if !c.nickname.is_empty() {
        c.nickname.clone()
    } else if let Some(suggested) = &c.nickname_suggestion {
        suggested.clone()
    } else {
        let id = c.id.clone();
        let short = id.chars().take(8).collect::<String>();
        format!("{short}...")
    };
    let relationship_badge = match c.relationship_state {
        ContactRelationshipState::Friend => " [friend]",
        ContactRelationshipState::PendingOutbound => " [pending→]",
        ContactRelationshipState::PendingInbound => " [pending←]",
        ContactRelationshipState::Contact if c.is_guardian => " [guardian]",
        _ => "",
    }
    .to_string();

    // Selection indicator: triangle when selected, space otherwise
    let indicator = if props.is_selected { "➤" } else { " " };
    let indicator_color = if props.is_selected {
        Theme::PRIMARY
    } else {
        text_color
    };
    let line = format!(
        "{} {} {}{}",
        indicator,
        c.status.icon(),
        name,
        relationship_badge
    );

    element! {
        View(
            background_color: bg,
            padding_left: 1,
            padding_right: 1,
            overflow: Overflow::Hidden,
        ) {
            Text(content: line, color: indicator_color)
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
    let border_color = focus_border_color(props.focused);

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
                Text(
                    content: format!("Contacts ({})", contacts.len()),
                    weight: Weight::Bold,
                    color: Theme::PRIMARY,
                )
            }
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                padding: Spacing::PANEL_PADDING,
                overflow: Overflow::Hidden,
            ) {
                #(if contacts.is_empty() {
                    Some(element! {
                        Text(content: "No contacts yet", color: Theme::TEXT_MUTED)
                    })
                } else {
                    None
                })
                #(contacts
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
                    }))
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
    let border_color = focus_border_color(props.focused);
    let detail_lines = props.contact.as_ref().map(|c| {
        let status_label = match c.status {
            ContactStatus::Active => "Active",
            ContactStatus::Offline => "Offline",
            ContactStatus::Pending => "Pending",
            ContactStatus::Blocked => "Blocked",
        };
        let guardian = if c.is_guardian { "Yes" } else { "No" };
        let read_receipts = c.read_receipt_policy.label();
        let nickname = if c.nickname.is_empty() {
            "Unknown".to_string()
        } else {
            c.nickname.clone()
        };
        let mut lines = vec![
            format!("Nickname: {nickname}"),
            format!("Status: {status_label}"),
            format!(
                "Relationship: {}",
                contact_relationship_label(c.relationship_state)
            ),
            "Authority: User/Home/Neighborhood".to_string(),
            format!("Guardian: {guardian}"),
            format!("Read Receipts: {read_receipts}"),
        ];
        if let Some(friend_actions) = contact_friend_action_hint(c.relationship_state) {
            lines.push(format!("Friend Actions: {friend_actions}"));
        }
        lines
    });

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
                overflow: Overflow::Hidden,
            ) {
                #(if let Some(lines) = detail_lines {
                    lines
                        .into_iter()
                        .map(|line| {
                            element! {
                                Text(content: line, color: Theme::TEXT)
                            }
                        })
                        .collect::<Vec<_>>()
                } else {
                    vec![element! {
                        Text(
                            content: "Select a contact to view details",
                            color: Theme::TEXT_MUTED,
                        )
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
/// omit grouped modal state such as `view.modals.nickname.visible`.
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

    /// Best-effort current time for expiring pending invitations
    pub now_ms: Option<u64>,

    // === Callbacks ===
    /// Callback when updating a contact's nickname
    pub(crate) on_update_nickname: Option<UpdateNicknameCallback>,
    /// Callback when starting a direct chat with a contact
    pub(crate) on_start_chat: Option<StartChatCallback>,
    /// Callback when inviting a discovered bootstrap candidate
    pub on_invite_lan_peer: Option<InvitePeerCallback>,
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
    let reactive_invitations = hooks.use_state(Vec::new);
    let own_authority_id = hooks.use_state(String::new);

    // Subscribe to contacts signal updates
    // Uses the unified ReactiveEffects system from aura-core
    hooks.use_future({
        let mut reactive_contacts = reactive_contacts.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CONTACTS_SIGNAL, move |contacts_state| {
                let contacts: Vec<Contact> =
                    contacts_state.all_contacts().map(Contact::from).collect();
                reactive_contacts.set(contacts);
            })
            .await;
        }
    });

    hooks.use_future({
        let mut own_authority_id = own_authority_id.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*SETTINGS_SIGNAL, move |settings_state| {
                own_authority_id.set(settings_state.authority_id);
            })
            .await;
        }
    });

    hooks.use_future({
        let mut reactive_invitations = reactive_invitations.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*INVITATIONS_SIGNAL, move |inv_state| {
                let invitations: Vec<Invitation> = inv_state
                    .all_pending()
                    .iter()
                    .chain(inv_state.all_sent().iter())
                    .chain(inv_state.all_history().iter())
                    .map(Invitation::from)
                    .collect();
                reactive_invitations.set(invitations);
            })
            .await;
        }
    });

    // Use reactive state for rendering (populated by signal subscription)
    let contacts = reactive_contacts.read().clone();
    let invitations = reactive_invitations.read().clone();
    let own_authority_id = own_authority_id.read().clone();

    // Bootstrap-candidate state (reactive via signal subscription)
    let lan_peers_state = hooks.use_state(DiscoveredPeersState::new);

    // Subscribe to discovered peers signal updates
    hooks.use_future({
        let mut lan_peers_state = lan_peers_state.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*DISCOVERED_PEERS_SIGNAL, move |peers_state| {
                let last_updated_ms = peers_state.last_updated_ms;
                let discovered_peers: Vec<DiscoveredPeerInfo> = peers_state
                    .peers
                    .iter()
                    .filter(|p| p.method == DiscoveredPeerMethod::BootstrapCandidate)
                    .map(|p| {
                        let authority_id = p.authority_id.to_string();
                        DiscoveredPeerInfo::new(&authority_id, &p.address)
                            .with_method(p.method.to_string())
                            .with_status(if p.invited {
                                crate::tui::components::PeerInvitationStatus::Pending
                            } else {
                                crate::tui::components::PeerInvitationStatus::None
                            })
                    })
                    .collect();

                let mut state = DiscoveredPeersState::new();
                state.set_peers(discovered_peers);
                state.set_last_updated_ms(last_updated_ms);
                lan_peers_state.set(state);
            })
            .await;
        }
    });

    let mut display_contacts = contacts;
    if !invitations.is_empty() {
        let mut existing_ids: HashSet<String> =
            display_contacts.iter().map(|c| c.id.clone()).collect();

        for invitation in invitations.iter() {
            if invitation.direction != InvitationDirection::Outbound {
                continue;
            }
            if invitation.status != InvitationStatus::Pending {
                continue;
            }
            if invitation.invitation_type != InvitationType::Contact {
                continue;
            }

            if let (Some(expires_at), Some(now_ms)) = (invitation.expires_at, props.now_ms) {
                if now_ms >= expires_at {
                    continue;
                }
            }

            let is_self_addressed =
                !own_authority_id.is_empty() && invitation.other_party_id == own_authority_id;

            let (id, name) = if !invitation.other_party_id.is_empty() && !is_self_addressed {
                let id = invitation.other_party_id.clone();
                let name = if !invitation.other_party_name.is_empty() {
                    invitation.other_party_name.clone()
                } else {
                    short_id(&id, 8)
                };
                (id, name)
            } else {
                let id = invitation.id.clone();
                let name = format!("Pending invite {}", short_id(&invitation.id, 6));
                (id, name)
            };

            if existing_ids.contains(&id) {
                continue;
            }
            existing_ids.insert(id.clone());

            display_contacts.push(Contact::new(id, name).with_status(ContactStatus::Pending));
        }
    }

    // === Pure view: Use props.view from TuiState instead of local state ===
    let current_selected = props.view.selected_index;
    let selected_contact = display_contacts.get(current_selected).cloned();

    // === Pure view: No use_terminal_events ===
    // All event handling is done by IoApp (the shell) via the state machine.
    // This component is purely presentational.

    let list_focused = props.view.focus.is_list();
    let lan_focused = list_focused && props.view.list_focus.is_lan();
    let contacts_focused = list_focused && props.view.list_focus.is_contacts();

    if harness_mode_enabled() {
        let selected_label = selected_contact
            .as_ref()
            .map(|contact| contact.effective_name())
            .unwrap_or_else(|| "None".to_string());
        let detail_lines = selected_contact
            .as_ref()
            .map(|contact| {
                let status_label = match contact.status {
                    ContactStatus::Active => "Active",
                    ContactStatus::Offline => "Offline",
                    ContactStatus::Pending => "Pending",
                    ContactStatus::Blocked => "Blocked",
                };
                vec![
                    format!("Selected contact: {selected_label}"),
                    format!("Status: {status_label}"),
                    format!(
                        "Relationship: {}",
                        contact_relationship_label(contact.relationship_state)
                    ),
                    format!(
                        "Guardian: {}",
                        if contact.is_guardian { "Yes" } else { "No" }
                    ),
                ]
            })
            .unwrap_or_else(|| vec![format!("Selected contact: {selected_label}")]);

        return element! {
            View(
                flex_direction: FlexDirection::Column,
                width: dim::TOTAL_WIDTH,
                height: dim::MIDDLE_HEIGHT,
                overflow: Overflow::Hidden,
                padding_left: 1,
                padding_right: 1,
            ) {
                Text(content: "Contacts", weight: Weight::Bold, color: Theme::PRIMARY)
                Text(
                    content: format!("Nearby peers: {}", lan_peers_state.read().peers.len()),
                    color: Theme::TEXT_MUTED,
                )
                Text(
                    content: format!("Contacts: {}", display_contacts.len()),
                    color: Theme::TEXT,
                )
                #(detail_lines.into_iter().map(|line| {
                    element! {
                        Text(content: line, color: Theme::TEXT)
                    }
                }))
            }
        };
    }

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
                // Left column: bootstrap candidates + contacts list (matches settings screen)
                View(
                    width: dim::TWO_PANEL_LEFT_WIDTH,
                    height: dim::MIDDLE_HEIGHT,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::Hidden,
                    gap: 0,
                ) {
                    // Discovered bootstrap candidates panel
                    #({
                        let state = lan_peers_state.read();
                        let now_ms = props.now_ms;
                        let last_updated_ms = state.last_updated_ms;
                        let status = if let Some(now) = now_ms {
                            if last_updated_ms > 0 {
                                format!(
                                    "Last scan: {}",
                                    format_relative_time_from(now, last_updated_ms)
                                )
                            } else {
                                "Discovery idle".to_string()
                            }
                        } else {
                            "Discovery idle".to_string()
                        };

                        let age_secs = if let Some(now) = now_ms {
                            now.saturating_sub(last_updated_ms) / 1000
                        } else {
                            0
                        };
                        let peers_with_age: Vec<DiscoveredPeerInfo> = state
                            .peers
                            .iter()
                            .cloned()
                            .map(|peer| peer.with_age(age_secs))
                            .collect();

                        Some(element! {
                            DiscoveredPeersPanel(
                                peers: peers_with_age,
                                selected_index: props.view.lan_selected_index,
                                focused: lan_focused,
                                status_line: status,
                            )
                        })
                    })
                    // Contacts list
                    ContactList(
                        contacts: display_contacts.clone(),
                        selected_index: current_selected,
                        focused: contacts_focused,
                    )
                }
                // Detail (matches settings screen width)
                View(
                    width: dim::TWO_PANEL_RIGHT_WIDTH,
                    height: dim::MIDDLE_HEIGHT,
                    overflow: Overflow::Hidden,
                ) {
                    ContactDetail(
                        contact: selected_contact,
                        focused: props.view.focus.is_detail(),
                    )
                }
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
            now_ms: None,
        )
    }
    .fullscreen()
    .await
}
