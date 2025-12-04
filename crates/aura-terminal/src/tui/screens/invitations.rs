//! # Invitations Screen
//!
//! Display and manage guardian invitations
//!
//! ## Reactive Signal Subscription
//!
//! When `AppCoreContext` is available, this screen subscribes to invitations state
//! changes via `use_future` and futures-signals. Updates are pushed to the
//! component automatically, triggering re-renders when data changes.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::{EmptyState, KeyHintsBar};
use crate::tui::hooks::AppCoreContext;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{
    format_timestamp, Invitation, InvitationDirection, InvitationFilter, InvitationStatus,
    InvitationType, KeyHint,
};

/// Callback type for invitation actions (invitation_id)
pub type InvitationCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Props for FilterTabs
#[derive(Default, Props)]
pub struct FilterTabsProps {
    pub filter: InvitationFilter,
}

/// Tab bar for filtering invitations
#[component]
pub fn FilterTabs(props: &FilterTabsProps) -> impl Into<AnyElement<'static>> {
    let filters = [
        InvitationFilter::All,
        InvitationFilter::Sent,
        InvitationFilter::Received,
    ];
    let current = props.filter;

    element! {
        View(
            flex_direction: FlexDirection::Row,
            gap: Spacing::SM,
            padding: Spacing::PANEL_PADDING,
            border_style: BorderStyle::Single,
            border_edges: Edges::Bottom,
            border_color: Theme::BORDER,
        ) {
            #(filters.iter().map(|&f| {
                let is_active = f == current;
                let color = if is_active { Theme::PRIMARY } else { Theme::TEXT_MUTED };
                let weight = if is_active { Weight::Bold } else { Weight::Normal };
                let label = f.label().to_string();
                element! {
                    Text(content: label, color: color, weight: weight)
                }
            }))
        }
    }
}

/// Props for InvitationItem
#[derive(Default, Props)]
pub struct InvitationItemProps {
    pub invitation: Invitation,
    pub is_selected: bool,
}

/// A single invitation in the list
#[component]
pub fn InvitationItem(props: &InvitationItemProps) -> impl Into<AnyElement<'static>> {
    let inv = &props.invitation;
    let bg = if props.is_selected {
        Theme::BG_SELECTED
    } else {
        Theme::BG_DARK
    };

    let status_color = match inv.status {
        InvitationStatus::Pending => Theme::WARNING,
        InvitationStatus::Accepted => Theme::SUCCESS,
        InvitationStatus::Declined => Theme::ERROR,
        InvitationStatus::Expired => Theme::TEXT_MUTED,
        InvitationStatus::Cancelled => Theme::TEXT_MUTED,
    };

    let type_icon = inv.invitation_type.icon().to_string();
    let direction_icon = inv.direction.icon().to_string();
    let name = inv.other_party_name.clone();
    let status_text = format!("[{}]", inv.status.label());

    element! {
        View(
            flex_direction: FlexDirection::Row,
            gap: Spacing::XS,
            background_color: bg,
            padding_left: Spacing::XS,
            padding_right: Spacing::XS,
        ) {
            Text(content: type_icon, color: Theme::SECONDARY)
            Text(content: direction_icon, color: Theme::TEXT_MUTED)
            Text(content: name, color: Theme::TEXT)
            Text(content: status_text, color: status_color)
        }
    }
}

/// Props for InvitationList
#[derive(Default, Props)]
pub struct InvitationListProps {
    pub invitations: Vec<Invitation>,
    pub selected_index: usize,
    pub focused: bool,
}

/// List of invitations
#[component]
pub fn InvitationList(props: &InvitationListProps) -> impl Into<AnyElement<'static>> {
    let border_color = if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    let count = props.invitations.len();
    let title = format!("Invitations ({})", count);
    let invitations = props.invitations.clone();
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
                #(if invitations.is_empty() {
                    vec![element! { View { EmptyState(title: "No invitations".to_string()) } }]
                } else {
                    invitations.iter().enumerate().map(|(idx, inv)| {
                        let is_selected = idx == selected;
                        element! {
                            View { InvitationItem(invitation: inv.clone(), is_selected: is_selected) }
                        }
                    }).collect::<Vec<_>>()
                })
            }
        }
    }
}

/// Props for InvitationDetail
#[derive(Default, Props)]
pub struct InvitationDetailProps {
    pub invitation: Option<Invitation>,
    pub focused: bool,
}

/// Detail panel for selected invitation
#[component]
pub fn InvitationDetail(props: &InvitationDetailProps) -> impl Into<AnyElement<'static>> {
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
                #(if let Some(inv) = &props.invitation {
                    let type_label = inv.invitation_type.label().to_string();
                    let direction_label = format!("{}: {}", inv.direction.label(), inv.other_party_name);
                    let status_label = format!("Status: {}", inv.status.label());
                    let created = format!("Created: {}", format_timestamp(inv.created_at));
                    let message_text = inv.message.clone().unwrap_or_default();
                    let has_message = inv.message.is_some();
                    let expires_text = inv.expires_at
                        .map(|exp| format!("Expires: {}", format_timestamp(exp)))
                        .unwrap_or_default();
                    let has_expires = inv.expires_at.is_some();

                    let status_color = match inv.status {
                        InvitationStatus::Pending => Theme::WARNING,
                        InvitationStatus::Accepted => Theme::SUCCESS,
                        InvitationStatus::Declined => Theme::ERROR,
                        InvitationStatus::Expired => Theme::TEXT_MUTED,
                        InvitationStatus::Cancelled => Theme::TEXT_MUTED,
                    };

                    vec![
                        element! {
                            View(flex_direction: FlexDirection::Row) {
                                Text(content: "Type: ", color: Theme::TEXT_MUTED)
                                Text(content: type_label, color: Theme::TEXT)
                            }
                        },
                        element! {
                            View(flex_direction: FlexDirection::Row) {
                                Text(content: direction_label, color: Theme::TEXT)
                            }
                        },
                        element! {
                            View(flex_direction: FlexDirection::Row) {
                                Text(content: status_label, color: status_color)
                            }
                        },
                        element! {
                            View {}
                        },
                        element! {
                            View(flex_direction: FlexDirection::Column) {
                                #(if has_message {
                                    vec![
                                        element! { Text(content: "Message:", color: Theme::TEXT_MUTED) },
                                        element! { Text(content: message_text.clone(), color: Theme::TEXT) },
                                    ]
                                } else {
                                    vec![]
                                })
                            }
                        },
                        element! {
                            View {
                                Text(content: created, color: Theme::TEXT_MUTED)
                            }
                        },
                        element! {
                            View {
                                #(if has_expires {
                                    vec![element! { Text(content: expires_text.clone(), color: Theme::TEXT_MUTED) }]
                                } else {
                                    vec![]
                                })
                            }
                        },
                    ]
                } else {
                    vec![element! {
                        View {
                            Text(content: "Select an invitation to view details", color: Theme::TEXT_MUTED)
                        }
                    }]
                })
            }
        }
    }
}

/// Props for InvitationsScreen
#[derive(Default, Props)]
pub struct InvitationsScreenProps {
    pub invitations: Vec<Invitation>,
    pub filter: InvitationFilter,
    pub selected_index: usize,
    /// Callback when accepting an invitation
    pub on_accept: Option<InvitationCallback>,
    /// Callback when declining an invitation
    pub on_decline: Option<InvitationCallback>,
}

/// Convert aura-app invitation type to TUI invitation type
fn convert_invitation_type(
    inv_type: aura_app::views::InvitationType,
) -> InvitationType {
    match inv_type {
        aura_app::views::InvitationType::Block => InvitationType::Guardian,
        aura_app::views::InvitationType::Guardian => InvitationType::Guardian,
        aura_app::views::InvitationType::Chat => InvitationType::Guardian,
    }
}

/// Convert aura-app invitation status to TUI invitation status
fn convert_invitation_status(
    status: aura_app::views::InvitationStatus,
) -> InvitationStatus {
    match status {
        aura_app::views::InvitationStatus::Pending => InvitationStatus::Pending,
        aura_app::views::InvitationStatus::Accepted => InvitationStatus::Accepted,
        aura_app::views::InvitationStatus::Rejected => InvitationStatus::Declined,
        aura_app::views::InvitationStatus::Expired => InvitationStatus::Expired,
        aura_app::views::InvitationStatus::Revoked => InvitationStatus::Cancelled,
    }
}

/// Convert aura-app invitation direction to TUI invitation direction
fn convert_invitation_direction(
    dir: aura_app::views::InvitationDirection,
) -> InvitationDirection {
    match dir {
        aura_app::views::InvitationDirection::Received => InvitationDirection::Inbound,
        aura_app::views::InvitationDirection::Sent => InvitationDirection::Outbound,
    }
}

/// Convert aura-app invitation to TUI invitation
fn convert_invitation(inv: &aura_app::views::Invitation) -> Invitation {
    // Get the "other party" name and ID based on direction
    let (other_party_id, other_party_name) = match inv.direction {
        aura_app::views::InvitationDirection::Sent => {
            let id = inv.to_id.clone().unwrap_or_default();
            let name = inv.to_name.clone().unwrap_or_else(|| id.clone());
            (id, name)
        }
        aura_app::views::InvitationDirection::Received => {
            (inv.from_id.clone(), inv.from_name.clone())
        }
    };

    Invitation {
        id: inv.id.clone(),
        direction: convert_invitation_direction(inv.direction),
        other_party_id,
        other_party_name,
        invitation_type: convert_invitation_type(inv.invitation_type),
        status: convert_invitation_status(inv.status),
        created_at: inv.created_at,
        expires_at: inv.expires_at,
        message: inv.message.clone(),
    }
}

/// The invitations screen
///
/// ## Reactive Updates
///
/// When `AppCoreContext` is available in the context tree, this component will
/// subscribe to invitations state signals and automatically update when:
/// - New invitations are received
/// - Invitations are accepted/declined
/// - Invitation status changes
#[component]
pub fn InvitationsScreen(
    props: &InvitationsScreenProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    // Try to get AppCoreContext for reactive signal subscription
    let app_ctx = hooks.try_use_context::<AppCoreContext>();

    // Initialize reactive state from props
    let reactive_invitations = hooks.use_state({
        let initial = props.invitations.clone();
        move || initial
    });

    // Subscribe to invitations signal updates if AppCoreContext is available
    if let Some(ctx) = app_ctx {
        hooks.use_future({
            let mut reactive_invitations = reactive_invitations.clone();
            let app_core = ctx.app_core.clone();
            async move {
                use futures_signals::signal::SignalExt;

                let signal = {
                    let core = app_core.read().await;
                    core.invitations_signal()
                };

                signal
                    .for_each(|invitations_state| {
                        // Combine all invitations from pending, sent, and history
                        let all_invitations: Vec<Invitation> = invitations_state
                            .pending
                            .iter()
                            .chain(invitations_state.sent.iter())
                            .chain(invitations_state.history.iter())
                            .map(convert_invitation)
                            .collect();

                        reactive_invitations.set(all_invitations);
                        async {}
                    })
                    .await;
            }
        });
    }

    // Use reactive state for rendering
    let all_invitations = reactive_invitations.read().clone();

    let selected = hooks.use_state(|| props.selected_index);
    let filter = hooks.use_state(|| props.filter);
    let detail_focused = hooks.use_state(|| false);

    let hints = vec![
        KeyHint::new("↑↓", "Navigate"),
        KeyHint::new("Tab", "Filter"),
        KeyHint::new("Enter", "Details"),
        KeyHint::new("a", "Accept"),
        KeyHint::new("d", "Decline"),
        KeyHint::new("n", "New"),
        KeyHint::new("Esc", "Back"),
    ];

    let current_filter = filter.get();

    // Filter invitations
    let filtered: Vec<Invitation> = all_invitations
        .iter()
        .filter(|inv| match current_filter {
            InvitationFilter::All => true,
            InvitationFilter::Sent => inv.direction == InvitationDirection::Outbound,
            InvitationFilter::Received => inv.direction == InvitationDirection::Inbound,
        })
        .cloned()
        .collect();

    let current_selected = selected.get();
    let is_detail_focused = detail_focused.get();
    let selected_invitation = filtered.get(current_selected).cloned();

    // Clone callbacks for event handler
    let on_accept = props.on_accept.clone();
    let on_decline = props.on_decline.clone();

    hooks.use_terminal_events({
        let mut selected = selected.clone();
        let mut filter = filter.clone();
        let mut detail_focused = detail_focused.clone();
        let count = filtered.len();
        let filtered_for_handler = filtered.clone();
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
                    if !detail_focused.get() {
                        filter.set(filter.get().next());
                        selected.set(0);
                    } else {
                        detail_focused.set(false);
                    }
                }
                KeyCode::Enter => {
                    detail_focused.set(!detail_focused.get());
                }
                KeyCode::Char('a') => {
                    // Accept selected invitation
                    if let Some(ref callback) = on_accept {
                        if let Some(inv) = filtered_for_handler.get(selected.get()) {
                            // Only accept received pending invitations
                            if inv.direction == InvitationDirection::Inbound
                                && inv.status == InvitationStatus::Pending
                            {
                                callback(inv.id.clone());
                            }
                        }
                    }
                }
                KeyCode::Char('d') => {
                    // Decline selected invitation
                    if let Some(ref callback) = on_decline {
                        if let Some(inv) = filtered_for_handler.get(selected.get()) {
                            // Only decline received pending invitations
                            if inv.direction == InvitationDirection::Inbound
                                && inv.status == InvitationStatus::Pending
                            {
                                callback(inv.id.clone());
                            }
                        }
                    }
                }
                KeyCode::Char('1') => {
                    filter.set(InvitationFilter::All);
                    selected.set(0);
                }
                KeyCode::Char('2') => {
                    filter.set(InvitationFilter::Sent);
                    selected.set(0);
                }
                KeyCode::Char('3') => {
                    filter.set(InvitationFilter::Received);
                    selected.set(0);
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
            // Filter tabs
            FilterTabs(filter: current_filter)

            // Main content: list + detail
            View(
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
                gap: 1,
            ) {
                // List (35%)
                View(width: 35pct) {
                    InvitationList(
                        invitations: filtered.clone(),
                        selected_index: current_selected,
                        focused: !is_detail_focused,
                    )
                }
                // Detail (65%)
                View(width: 65pct) {
                    InvitationDetail(
                        invitation: selected_invitation,
                        focused: is_detail_focused,
                    )
                }
            }

            // Key hints
            KeyHintsBar(hints: hints)
        }
    }
}

/// Run the invitations screen with sample data
pub async fn run_invitations_screen() -> std::io::Result<()> {
    let invitations = vec![
        Invitation::new("1", "Alice", InvitationDirection::Outbound)
            .with_status(InvitationStatus::Pending)
            .with_message("Would you like to be my guardian?"),
        Invitation::new("2", "Bob", InvitationDirection::Inbound)
            .with_status(InvitationStatus::Pending)
            .with_message("Requesting guardian access"),
        Invitation::new("3", "Charlie", InvitationDirection::Outbound)
            .with_status(InvitationStatus::Accepted),
        Invitation::new("4", "Diana", InvitationDirection::Inbound)
            .with_status(InvitationStatus::Declined),
        Invitation::new("5", "Eve", InvitationDirection::Outbound)
            .with_status(InvitationStatus::Expired),
    ];

    element! {
        InvitationsScreen(
            invitations: invitations,
            filter: InvitationFilter::All,
            selected_index: 0usize,
        )
    }
    .fullscreen()
    .await
}
