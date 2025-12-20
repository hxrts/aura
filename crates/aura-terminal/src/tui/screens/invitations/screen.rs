//! # Invitations Screen
//!
//! Display and manage guardian invitations
//!
//! ## Reactive Signal Subscription
//!
//! When `AppCoreContext` is available, this screen subscribes to invitations state
//! changes via the unified `ReactiveEffects` system. Updates are pushed to the
//! component automatically, triggering re-renders when data changes.
//!
//! Uses `aura_app::signal_defs::INVITATIONS_SIGNAL` with `ReactiveEffects::subscribe()`.
//!
//! ## Pure View Component
//!
//! This screen is a pure view that renders based on props from TuiState.
//! All event handling is done by the parent TuiShell (IoApp) via the state machine.

use iocraft::prelude::*;

use aura_app::signal_defs::INVITATIONS_SIGNAL;
use aura_core::effects::reactive::ReactiveEffects;

use crate::tui::callbacks::{
    CreateInvitationCallback, ExportInvitationCallback, ImportInvitationCallback,
    InvitationCallback,
};
use crate::tui::components::{DetailPanel, KeyValue, ListPanel, TabBar, TabItem};
use crate::tui::hooks::AppCoreContext;
use crate::tui::layout::dim;
use crate::tui::navigation::TwoPanelFocus;
use crate::tui::props::InvitationsViewProps;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{
    format_timestamp, Invitation, InvitationDirection, InvitationFilter, InvitationStatus,
    InvitationType,
};

/// Props for FilterTabs
#[derive(Default, Props)]
pub struct FilterTabsProps {
    pub filter: InvitationFilter,
}

/// Tab bar for filtering invitations (using generic TabBar component)
#[component]
pub fn FilterTabs(props: &FilterTabsProps) -> impl Into<AnyElement<'static>> {
    let filters = [
        InvitationFilter::All,
        InvitationFilter::Sent,
        InvitationFilter::Received,
    ];
    let current = props.filter;

    // Convert filter enum to TabItem vec and find active index
    let tabs: Vec<TabItem> = filters.iter().map(|f| TabItem::new(f.label())).collect();
    let active_index = filters.iter().position(|&f| f == current).unwrap_or(0);

    element! {
        TabBar(tabs: tabs, active_index: active_index, gap: Some(Spacing::SM))
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

    let muted_color = if props.is_selected {
        Theme::LIST_TEXT_SELECTED
    } else {
        Theme::LIST_TEXT_MUTED
    };

    let status_color = inv.status.color();

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
            Text(content: direction_icon, color: muted_color)
            Text(content: name, color: text_color, wrap: TextWrap::NoWrap)
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
    let invitations = props.invitations.clone();
    let selected = props.selected_index;

    // Build list items
    let items: Vec<AnyElement<'static>> = invitations
        .iter()
        .enumerate()
        .map(|(idx, inv)| {
            let is_selected = idx == selected;
            let id = inv.id.clone();
            element! {
                View(key: id) {
                    InvitationItem(invitation: inv.clone(), is_selected: is_selected)
                }
            }
            .into_any()
        })
        .collect();

    element! {
        ListPanel(
            title: "Invitations".to_string(),
            count: invitations.len(),
            focused: props.focused,
            items: items,
            empty_message: "No invitations".to_string(),
        )
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
    let on_accept = props.on_accept.clone();
    let on_decline = props.on_decline.clone();

    // Build content based on whether an invitation is selected
    let content: Vec<AnyElement<'static>> = if let Some(inv) = &props.invitation {
        let mut nodes: Vec<AnyElement<'static>> = vec![
            element! { KeyValue(label: "Type".to_string(), value: inv.invitation_type.label().to_string()) }.into_any(),
            element! { KeyValue(label: "Direction".to_string(), value: inv.direction.label().to_string()) }.into_any(),
            element! { KeyValue(label: "Status".to_string(), value: inv.status.label().to_string()) }.into_any(),
            element! { KeyValue(label: "Other Party".to_string(), value: inv.other_party_name.clone()) }.into_any(),
            element! { KeyValue(label: "Created".to_string(), value: format_timestamp(inv.created_at)) }.into_any(),
        ];

        // Add action buttons for pending invitations
        if inv.status == InvitationStatus::Pending {
            let mut actions: Vec<AnyElement<'static>> = Vec::new();
            if let Some(cb) = on_accept {
                let id = inv.id.clone();
                actions.push(
                    element! {
                        Button(has_focus: true, handler: move |_| cb(id.clone())) {
                            Text(content: "Accept", color: Theme::PRIMARY)
                        }
                    }
                    .into_any(),
                );
            }
            if let Some(cb) = on_decline {
                let id = inv.id.clone();
                actions.push(
                    element! {
                        Button(has_focus: true, handler: move |_| cb(id.clone())) {
                            Text(content: "Decline", color: Theme::ERROR)
                        }
                    }
                    .into_any(),
                );
            }
            if !actions.is_empty() {
                nodes.push(
                    element! {
                        View(flex_direction: FlexDirection::Row, gap: Spacing::SM) { #(actions) }
                    }
                    .into_any(),
                );
            }
        }

        nodes
    } else {
        vec![]
    };

    element! {
        DetailPanel(
            title: "Details".to_string(),
            focused: props.focused,
            content: content,
            empty_message: "Select an invitation to view details".to_string(),
        )
    }
}

/// Props for InvitationsScreen
///
/// ## Compile-Time Safety
///
/// The `view` field is a required struct that embeds all view state from TuiState.
/// This makes it a **compile-time error** to forget any view state field.
#[derive(Default, Props)]
pub struct InvitationsScreenProps {
    // === Domain data (from reactive signals) ===
    pub invitations: Vec<Invitation>,

    // === View state from TuiState (REQUIRED - compile-time enforced) ===
    /// All view state extracted from TuiState via `extract_invitations_view_props()`.
    /// This is a single struct field so forgetting any view state is a compile error.
    pub view: InvitationsViewProps,

    // === Callbacks (still needed for effect dispatch) ===
    /// Callback when accepting an invitation
    pub on_accept: Option<InvitationCallback>,
    /// Callback when declining an invitation
    pub on_decline: Option<InvitationCallback>,
    /// Callback when creating a new invitation
    pub on_create: Option<CreateInvitationCallback>,
    /// Callback when exporting an invitation
    pub on_export: Option<ExportInvitationCallback>,
    /// Callback when importing an invitation code
    pub on_import: Option<ImportInvitationCallback>,
    /// Code to display (set after export)
    pub exported_code: Option<String>,
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
            let id = inv
                .to_id
                .as_ref()
                .map(|id| id.to_string())
                .unwrap_or_default();
            let name = inv.to_name.clone().unwrap_or_else(|| id.clone());
            (id, name)
        }
        aura_app::views::InvitationDirection::Received => {
            (inv.from_id.to_string(), inv.from_name.clone())
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
/// ## Pure View Component
///
/// This screen is a pure view that renders based on props from TuiState.
/// All event handling is done by the parent TuiShell (IoApp) via the state machine.
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
    // Uses the unified ReactiveEffects system from aura-core
    if let Some(ref ctx) = app_ctx {
        hooks.use_future({
            let mut reactive_invitations = reactive_invitations.clone();
            let app_core = ctx.app_core.clone();
            async move {
                // FIRST: Read current signal value to catch up on any changes
                // that happened while this screen was unmounted
                {
                    let core = app_core.read().await;
                    if let Ok(invitations_state) = core.read(&*INVITATIONS_SIGNAL).await {
                        let all_invitations: Vec<Invitation> = invitations_state
                            .pending
                            .iter()
                            .chain(invitations_state.sent.iter())
                            .chain(invitations_state.history.iter())
                            .map(convert_invitation)
                            .collect();
                        reactive_invitations.set(all_invitations);
                    }
                }

                // THEN: Subscribe for future updates
                let mut stream = {
                    let core = app_core.read().await;
                    core.subscribe(&*INVITATIONS_SIGNAL)
                };

                // Subscribe to signal updates - runs until component unmounts
                while let Ok(invitations_state) = stream.recv().await {
                    // Combine all invitations from pending, sent, and history
                    let all_invitations: Vec<Invitation> = invitations_state
                        .pending
                        .iter()
                        .chain(invitations_state.sent.iter())
                        .chain(invitations_state.history.iter())
                        .map(convert_invitation)
                        .collect();

                    reactive_invitations.set(all_invitations);
                }
            }
        });
    }

    // Use reactive state for rendering
    let all_invitations = reactive_invitations.read().clone();

    // === Pure view: Use props.view from TuiState instead of local state ===
    // NOTE: Modal visibility state (create_modal_visible, code_modal_visible, import_modal_visible)
    // is handled by app.rs which renders all modals at root level.
    let current_filter = props.view.filter;

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

    let current_selected = props.view.selected_index;
    let current_focus = props.view.focus;
    let is_detail_focused = current_focus == TwoPanelFocus::Detail;
    let selected_invitation = filtered.get(current_selected).cloned();

    // === Pure view: No use_terminal_events ===
    // All event handling is done by IoApp (the shell) via the state machine.
    // This component is purely presentational.

    // Layout: FilterTabs (2 rows) + Main content (23 rows) = 25 = MIDDLE_HEIGHT
    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            overflow: Overflow::Hidden,
        ) {
            // Filter tabs (2 rows: 1 content + 1 border)
            View(height: 2) {
                FilterTabs(filter: current_filter)
            }

            // Main content: list + detail (23 rows)
            View(
                flex_direction: FlexDirection::Row,
                height: 23,
                overflow: Overflow::Hidden,
                gap: Spacing::XS,
            ) {
                // List (24 chars = 30% of 80)
                View(width: 24) {
                    InvitationList(
                        invitations: filtered.clone(),
                        selected_index: current_selected,
                        focused: !is_detail_focused,
                    )
                }
                // Detail (remaining width ~55 chars)
                InvitationDetail(
                    invitation: selected_invitation,
                    focused: is_detail_focused,
                )
            }

            // NOTE: All modals have been moved to app.rs root level and wrapped with ModalFrame
            // for consistent positioning. See the "INVITATIONS SCREEN MODALS" section in app.rs.
        }
    }
}

/// Run the invitations screen with sample data
#[allow(dead_code)]
pub async fn run_invitations_screen() -> std::io::Result<()> {
    let invitations = vec![
        Invitation::new("1", "Alice", InvitationDirection::Outbound)
            .with_status(InvitationStatus::Pending)
            .with_message("Would you like to be my guardian?"),
        Invitation::new("2", "Bob", InvitationDirection::Inbound)
            .with_status(InvitationStatus::Pending)
            .with_message("Requesting guardian access"),
        Invitation::new("3", "Carol", InvitationDirection::Outbound)
            .with_status(InvitationStatus::Accepted),
        Invitation::new("4", "Diana", InvitationDirection::Inbound)
            .with_status(InvitationStatus::Declined),
        Invitation::new("5", "Eve", InvitationDirection::Outbound)
            .with_status(InvitationStatus::Expired),
    ];

    element! {
        InvitationsScreen(
            invitations: invitations,
        )
    }
    .fullscreen()
    .await
}
