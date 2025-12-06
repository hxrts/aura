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
use std::time::{Duration, Instant};

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
            width: 100pct,
            overflow: Overflow::Hidden,
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
            overflow: Overflow::Hidden,
        ) {
            Text(content: type_icon, color: Theme::SECONDARY)
            Text(content: direction_icon, color: Theme::TEXT_MUTED)
            Text(content: name, color: Theme::TEXT, wrap: TextWrap::NoWrap)
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
                        let id = inv.id.clone();
                        element! {
                            View(key: id) { InvitationItem(invitation: inv.clone(), is_selected: is_selected) }
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
    pub on_accept: Option<InvitationCallback>,
    pub on_decline: Option<InvitationCallback>,
}

/// Detail panel for selected invitation
#[component]
pub fn InvitationDetail(props: &InvitationDetailProps) -> impl Into<AnyElement<'static>> + '_ {
    let border_color = if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };
    let on_accept = props.on_accept.clone();
    let on_decline = props.on_decline.clone();

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
                    let mut nodes: Vec<AnyElement<'static>> = vec![
                        element! { Text(content: format!("Type: {}", inv.invitation_type.label()), color: Theme::TEXT) }.into_any(),
                        element! { Text(content: format!("Direction: {}", inv.direction.label()), color: Theme::TEXT) }.into_any(),
                        element! { Text(content: format!("Status: {}", inv.status.label()), color: Theme::TEXT) }.into_any(),
                        element! { Text(content: format!("Other Party: {}", inv.other_party_name), color: Theme::TEXT) }.into_any(),
                        element! { Text(content: format!("Created: {}", format_timestamp(inv.created_at)), color: Theme::TEXT_MUTED) }.into_any(),
                    ];

                    if inv.status == InvitationStatus::Pending {
                        let mut actions: Vec<AnyElement<'static>> = Vec::new();
                        if let Some(cb) = on_accept {
                            let id = inv.id.clone();
                            actions.push(element! {
                                Button(has_focus: true, handler: move |_| cb(id.clone())) {
                                    Text(content: "Accept", color: Theme::PRIMARY)
                                }
                            }.into_any());
                        }
                        if let Some(cb) = on_decline {
                            let id = inv.id.clone();
                            actions.push(element! {
                                Button(has_focus: true, handler: move |_| cb(id.clone())) {
                                    Text(content: "Decline", color: Theme::ERROR)
                                }
                            }.into_any());
                        }
                        if !actions.is_empty() {
                            nodes.push(element! {
                                View(flex_direction: FlexDirection::Row, gap: Spacing::SM) { #(actions) }
                            }.into_any());
                        }
                    }

                    nodes
                } else {
                    vec![element! { Text(content: "Select an invitation to view details", color: Theme::TEXT_MUTED) }.into_any()]
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
fn convert_invitation_type(inv_type: aura_app::views::InvitationType) -> InvitationType {
    match inv_type {
        aura_app::views::InvitationType::Block => InvitationType::Guardian,
        aura_app::views::InvitationType::Guardian => InvitationType::Guardian,
        aura_app::views::InvitationType::Chat => InvitationType::Guardian,
    }
}

/// Convert aura-app invitation status to TUI invitation status
fn convert_invitation_status(status: aura_app::views::InvitationStatus) -> InvitationStatus {
    match status {
        aura_app::views::InvitationStatus::Pending => InvitationStatus::Pending,
        aura_app::views::InvitationStatus::Accepted => InvitationStatus::Accepted,
        aura_app::views::InvitationStatus::Rejected => InvitationStatus::Declined,
        aura_app::views::InvitationStatus::Expired => InvitationStatus::Expired,
        aura_app::views::InvitationStatus::Revoked => InvitationStatus::Cancelled,
    }
}

/// Convert aura-app invitation direction to TUI invitation direction
fn convert_invitation_direction(dir: aura_app::views::InvitationDirection) -> InvitationDirection {
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

    // Throttle for navigation keys - persists across renders using use_ref
    let mut nav_throttle = hooks.use_ref(|| Instant::now() - Duration::from_millis(200));
    let throttle_duration = Duration::from_millis(150);

    hooks.use_terminal_events({
        let mut selected = selected.clone();
        let mut filter = filter.clone();
        let mut detail_focused = detail_focused.clone();
        let count = filtered.len();
        let filtered_for_handler = filtered.clone();
        move |event| match event {
            TerminalEvent::Key(KeyEvent { code, .. }) => match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    let should_move = nav_throttle.read().elapsed() >= throttle_duration;
                    if should_move {
                        let current = selected.get();
                        if current > 0 {
                            selected.set(current - 1);
                        }
                        nav_throttle.set(Instant::now());
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let should_move = nav_throttle.read().elapsed() >= throttle_duration;
                    if should_move {
                        let current = selected.get();
                        if current + 1 < count {
                            selected.set(current + 1);
                        }
                        nav_throttle.set(Instant::now());
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
            overflow: Overflow::Hidden,
        ) {
            // Header
            View(
                width: 100pct,
                overflow: Overflow::Hidden,
                padding: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(content: "Invitations", weight: Weight::Bold, color: Theme::PRIMARY)
            }

            // Filter tabs
            FilterTabs(filter: current_filter)

            // Main content: list + detail
            View(
                flex_direction: FlexDirection::Row,
                width: 100pct,
                flex_grow: 1.0,
                overflow: Overflow::Hidden,
                gap: Spacing::XS,
            ) {
                // List (30%)
                View(width: 30pct) {
                    InvitationList(
                        invitations: filtered.clone(),
                        selected_index: current_selected,
                        focused: !is_detail_focused,
                    )
                }
                // Detail (75%)
                InvitationDetail(
                    invitation: selected_invitation,
                    focused: is_detail_focused,
                )
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
