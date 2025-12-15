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
use std::{future::Future, pin::Pin, sync::Arc};

use aura_app::signal_defs::INVITATIONS_SIGNAL;
use aura_core::effects::reactive::ReactiveEffects;

use crate::tui::components::{
    EmptyState, InvitationCodeModal, InvitationCreateModal, InvitationImportModal,
};
use crate::tui::hooks::AppCoreContext;
use crate::tui::layout::dim;
use crate::tui::navigation::TwoPanelFocus;
use crate::tui::props::InvitationsViewProps;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{
    format_timestamp, Invitation, InvitationDirection, InvitationFilter, InvitationStatus,
    InvitationType,
};

/// Callback type for invitation actions (invitation_id)
pub type InvitationCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Callback type for creating new invitations (invitation_type, message, ttl_secs)
pub type CreateInvitationCallback = Arc<dyn Fn(String, Option<String>, Option<u64>) + Send + Sync>;

/// Callback type for exporting invitation code (invitation_id) -> returns code string
pub type ExportInvitationCallback = Arc<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> + Send + Sync,
>;

/// Callback type for importing invitation code (code) -> triggers import flow
pub type ImportInvitationCallback = Arc<dyn Fn(String) + Send + Sync>;

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

    let status_color = match inv.status {
        InvitationStatus::Pending => Theme::WARNING,
        InvitationStatus::Accepted => Theme::SUCCESS,
        InvitationStatus::Declined => Theme::ERROR,
        InvitationStatus::Expired => Theme::LIST_TEXT_MUTED,
        InvitationStatus::Cancelled => Theme::LIST_TEXT_MUTED,
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
    /// Whether running in demo mode (enables quick-fill shortcuts)
    pub demo_mode: bool,
    /// Alice's invite code for demo mode (press 'a' in import modal to fill)
    pub demo_alice_code: String,
    /// Carol's invite code for demo mode (press 'c' in import modal to fill)
    pub demo_carol_code: String,
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
    let is_create_modal_visible = props.view.create_modal_visible;
    let is_code_modal_visible = props.view.code_modal_visible;
    let is_import_modal_visible = props.view.import_modal_visible;

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

    // Demo mode settings
    let demo_mode = props.demo_mode;

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

            // Create invitation modal (overlays everything) - uses props.view from TuiState
            #(if is_create_modal_visible {
                // Convert type index to InvitationType
                let invitation_type = match props.view.create_modal_type_index {
                    0 => InvitationType::Guardian,
                    1 => InvitationType::Contact,
                    _ => InvitationType::Channel,
                };
                Some(element! {
                    InvitationCreateModal(
                        visible: true,
                        focused: true,
                        creating: false,
                        error: String::new(),
                        invitation_type: invitation_type,
                        message: props.view.create_modal_message.clone(),
                        ttl_hours: props.view.create_modal_ttl_hours as u32,
                    )
                })
            } else {
                None
            })

            // Invitation code modal (overlays everything) - uses props.view from TuiState
            #(if is_code_modal_visible {
                Some(element! {
                    InvitationCodeModal(
                        visible: true,
                        code: props.view.code_modal_code.clone(),
                        invitation_type: "Guardian".to_string(),
                    )
                })
            } else {
                None
            })

            // Import invitation modal (overlays everything) - uses props.view from TuiState
            #(if is_import_modal_visible {
                Some(element! {
                    InvitationImportModal(
                        visible: true,
                        focused: true,
                        code: props.view.import_modal_code.clone(),
                        error: String::new(),
                        importing: props.view.import_modal_importing,
                        demo_mode: demo_mode,
                    )
                })
            } else {
                None
            })
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
