//! # Recovery Screen
//!
//! Guardian management and account recovery
//!
//! ## Reactive Signal Subscription
//!
//! When `AppCoreContext` is available, this screen subscribes to recovery state
//! changes via the unified `ReactiveEffects` system. Updates are pushed to the
//! component automatically, triggering re-renders when data changes.
//!
//! Uses `aura_app::signal_defs::RECOVERY_SIGNAL` with `ReactiveEffects::subscribe()`.
//!
//! ## Pure View Component
//!
//! This screen is a pure view that renders based on props from TuiState.
//! All event handling is done by the parent TuiShell (IoApp) via the state machine.

use iocraft::prelude::*;

use aura_app::signal_defs::RECOVERY_SIGNAL;
use aura_core::effects::reactive::ReactiveEffects;

use crate::tui::callbacks::{ApprovalCallback, RecoveryCallback};
use crate::tui::components::{EmptyState, KeyValue, TabBar, TabItem};
use crate::tui::hooks::AppCoreContext;
use crate::tui::layout::dim;
use crate::tui::props::RecoveryViewProps;
use crate::tui::theme::{Icons, Spacing, Theme};
use crate::tui::types::{
    Guardian, GuardianApproval, GuardianStatus, PendingRequest, RecoveryState, RecoveryStatus,
    RecoveryTab,
};

/// Props for RecoveryTabBar
#[derive(Default, Props)]
pub struct RecoveryTabBarProps {
    pub active_tab: RecoveryTab,
    pub pending_count: usize,
}

/// Tab navigation bar for recovery screen (using generic TabBar component)
#[component]
pub fn RecoveryTabBar(props: &RecoveryTabBarProps) -> impl Into<AnyElement<'static>> {
    let active = props.active_tab;
    let pending_count = props.pending_count;

    // Convert RecoveryTab variants to TabItems with optional badge on Requests tab
    let tabs: Vec<TabItem> = RecoveryTab::all()
        .iter()
        .map(|&tab| {
            if tab == RecoveryTab::Requests && pending_count > 0 {
                TabItem::with_badge(tab.title(), pending_count)
            } else {
                TabItem::new(tab.title())
            }
        })
        .collect();

    let active_index = RecoveryTab::all()
        .iter()
        .position(|&t| t == active)
        .unwrap_or(0);

    element! {
        TabBar(tabs: tabs, active_index: active_index)
    }
}

/// Props for GuardiansPanel
#[derive(Default, Props)]
pub struct GuardiansPanelProps {
    pub guardians: Vec<Guardian>,
    pub selected_index: usize,
    pub threshold_required: u32,
    pub threshold_total: u32,
}

/// Guardians management panel
#[component]
pub fn GuardiansPanel(props: &GuardiansPanelProps) -> impl Into<AnyElement<'static>> {
    let guardians = props.guardians.clone();
    let selected = props.selected_index;
    let active_count = guardians
        .iter()
        .filter(|g| g.status == GuardianStatus::Active)
        .count();

    let threshold_text = if props.threshold_total == 0 {
        "Not configured".to_string()
    } else {
        format!(
            "{} of {} required ({} active)",
            props.threshold_required, props.threshold_total, active_count
        )
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            gap: 0,
        ) {
            // Guardian list
            View(
                flex_direction: FlexDirection::Column,
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER_FOCUS,
                flex_grow: 1.0,
            ) {
                View(padding_left: Spacing::PANEL_PADDING) {
                    Text(content: "Guardians", weight: Weight::Bold, color: Theme::PRIMARY)
                }
                View(
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    padding: Spacing::PANEL_PADDING,
                    overflow: Overflow::Scroll,
                ) {
                    #(if guardians.is_empty() {
                        vec![element! { View { EmptyState(title: "No guardians configured".to_string()) } }]
                    } else {
                        guardians.iter().enumerate().map(|(idx, g)| {
                            let is_selected = idx == selected;
                            // Use consistent list item colors
                            let bg = if is_selected { Theme::LIST_BG_SELECTED } else { Theme::LIST_BG_NORMAL };
                            let text_color = if is_selected { Theme::LIST_TEXT_SELECTED } else { Theme::LIST_TEXT_NORMAL };
                            let status_color = g.status.color();
                            let icon = g.status.icon().to_string();
                            let id = g.id.clone();
                            let name = g.name.clone();
                            let share_text = if g.has_share { " [share]" } else { "" }.to_string();
                            element! {
                                View(key: id, flex_direction: FlexDirection::Row, background_color: bg, padding_left: Spacing::XS, gap: Spacing::XS) {
                                    Text(content: icon, color: status_color)
                                    Text(content: name, color: text_color)
                                    Text(content: share_text, color: Theme::SECONDARY)
                                }
                            }
                        }).collect()
                    })
                }
            }

            // Threshold info
            View(
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER,
                padding: Spacing::PANEL_PADDING,
            ) {
                KeyValue(label: "Threshold".to_string(), value: threshold_text)
            }
        }
    }
}

/// Props for RecoveryPanel
#[derive(Default, Props)]
pub struct RecoveryPanelProps {
    pub status: RecoveryStatus,
}

/// Recovery process panel
#[component]
pub fn RecoveryPanel(props: &RecoveryPanelProps) -> impl Into<AnyElement<'static>> {
    let status = &props.status;
    let state_label = status.state.label().to_string();

    let state_color = match status.state {
        RecoveryState::None | RecoveryState::Cancelled => Theme::TEXT_MUTED,
        RecoveryState::Initiated => Theme::WARNING,
        RecoveryState::ThresholdMet | RecoveryState::Completed => Theme::SUCCESS,
        RecoveryState::InProgress => Theme::SECONDARY,
        RecoveryState::Failed => Theme::ERROR,
    };

    let progress_text = format!(
        "{} / {} approvals",
        status.approvals_received, status.threshold
    );

    let approvals = status.approvals.clone();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            gap: 0,
        ) {
            // Status header
            View(
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER,
                padding: Spacing::PANEL_PADDING,
            ) {
                View(flex_direction: FlexDirection::Row) {
                    Text(content: "Status: ", color: Theme::TEXT_MUTED)
                    Text(content: state_label, color: state_color)
                }
            }

            // Progress
            View(
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER,
                padding: Spacing::PANEL_PADDING,
            ) {
                KeyValue(label: "Progress".to_string(), value: progress_text)
            }

            // Approvals list
            View(
                flex_direction: FlexDirection::Column,
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER,
                flex_grow: 1.0,
            ) {
                View(padding_left: Spacing::PANEL_PADDING) {
                    Text(content: "Guardian Approvals", weight: Weight::Bold, color: Theme::PRIMARY)
                }
                View(
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    padding: Spacing::PANEL_PADDING,
                    overflow: Overflow::Scroll,
                ) {
                    #(if approvals.is_empty() {
                        vec![element! { View { EmptyState(title: "No recovery in progress".to_string()) } }]
                    } else {
                        approvals.iter().map(|a| {
                            let icon = if a.approved { Icons::CHECK } else { Icons::PENDING };
                            let icon_color = if a.approved { Theme::SUCCESS } else { Theme::TEXT_MUTED };
                            let name = a.guardian_name.clone();
                            let key = name.clone();
                            element! {
                                View(key: key, flex_direction: FlexDirection::Row, padding_left: Spacing::XS, gap: Spacing::XS) {
                                    Text(content: icon.to_string(), color: icon_color)
                                    Text(content: name, color: Theme::TEXT)
                                }
                            }
                        }).collect()
                    })
                }
            }
        }
    }
}

/// Props for PendingRequestsPanel
#[derive(Default, Props)]
pub struct PendingRequestsPanelProps {
    pub requests: Vec<PendingRequest>,
    pub selected_index: usize,
}

/// Pending recovery requests panel (requests from others that we can approve)
#[component]
pub fn PendingRequestsPanel(props: &PendingRequestsPanelProps) -> impl Into<AnyElement<'static>> {
    let requests = props.requests.clone();
    let selected = props.selected_index;

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            gap: 0,
        ) {
            // Requests list
            View(
                flex_direction: FlexDirection::Column,
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER_FOCUS,
                flex_grow: 1.0,
            ) {
                View(padding_left: Spacing::PANEL_PADDING) {
                    Text(content: "Recovery Requests", weight: Weight::Bold, color: Theme::PRIMARY)
                }
                View(
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    padding: Spacing::PANEL_PADDING,
                    overflow: Overflow::Scroll,
                ) {
                    #(if requests.is_empty() {
                        vec![element! { View { EmptyState(title: "No pending requests".to_string()) } }]
                    } else {
                        requests.iter().enumerate().map(|(idx, req)| {
                            let is_selected = idx == selected;
                            let bg = if is_selected { Theme::LIST_BG_SELECTED } else { Theme::LIST_BG_NORMAL };
                            let text_color = if is_selected { Theme::LIST_TEXT_SELECTED } else { Theme::LIST_TEXT_NORMAL };

                            let status_icon = if req.we_approved { Icons::CHECK } else { Icons::PENDING };
                            let status_color = if req.we_approved { Theme::SUCCESS } else { Theme::WARNING };

                            let progress_text = format!("{}/{}", req.approvals_received, req.approvals_required);
                            let account = req.account_name.clone();
                            let key = req.id.clone();

                            element! {
                                View(key: key, flex_direction: FlexDirection::Row, background_color: bg, padding_left: Spacing::XS, gap: Spacing::SM) {
                                    Text(content: status_icon.to_string(), color: status_color)
                                    View(flex_direction: FlexDirection::Column, flex_grow: 1.0) {
                                        Text(content: account, color: text_color)
                                        KeyValue(label: "Progress".to_string(), value: progress_text)
                                    }
                                }
                            }
                        }).collect()
                    })
                }
            }

            // Help text
            View(
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER,
                padding: Spacing::PANEL_PADDING,
            ) {
                Text(content: "Press Enter to approve selected request", color: Theme::TEXT_MUTED)
            }
        }
    }
}

/// Props for RecoveryScreen
///
/// ## Compile-Time Safety
///
/// The `view` field is a required struct that embeds all view state from TuiState.
/// This makes it a **compile-time error** to forget any view state field.
#[derive(Default, Props)]
pub struct RecoveryScreenProps {
    // === Domain data (from reactive signals) ===
    pub guardians: Vec<Guardian>,
    pub threshold_required: u32,
    pub threshold_total: u32,
    pub recovery_status: RecoveryStatus,
    /// Pending recovery requests from others (we are their guardian)
    pub pending_requests: Vec<PendingRequest>,

    // === View state from TuiState (REQUIRED - compile-time enforced) ===
    /// All view state extracted from TuiState via `extract_recovery_view_props()`.
    /// This is a single struct field so forgetting any view state is a compile error.
    pub view: RecoveryViewProps,

    // === Callbacks ===
    /// Callback when starting recovery
    pub on_start_recovery: Option<RecoveryCallback>,
    /// Callback when adding a guardian
    pub on_add_guardian: Option<RecoveryCallback>,
    /// Callback when approving a pending request (takes request_id)
    pub on_submit_approval: Option<ApprovalCallback>,
}

/// Convert aura-app guardian status to TUI guardian status
fn convert_guardian_status(status: aura_app::views::GuardianStatus) -> GuardianStatus {
    match status {
        aura_app::views::GuardianStatus::Active => GuardianStatus::Active,
        aura_app::views::GuardianStatus::Pending => GuardianStatus::Pending,
        aura_app::views::GuardianStatus::Revoked => GuardianStatus::Removed,
        aura_app::views::GuardianStatus::Offline => GuardianStatus::Offline,
    }
}

/// Convert aura-app guardian to TUI guardian
fn convert_guardian(g: &aura_app::views::Guardian) -> Guardian {
    Guardian {
        id: g.id.clone(),
        name: g.name.clone(),
        status: convert_guardian_status(g.status),
        has_share: g.status == aura_app::views::GuardianStatus::Active,
    }
}

/// Convert aura-app recovery process status to TUI recovery state
fn convert_recovery_state(status: aura_app::views::RecoveryProcessStatus) -> RecoveryState {
    match status {
        aura_app::views::RecoveryProcessStatus::Idle => RecoveryState::None,
        aura_app::views::RecoveryProcessStatus::Initiated => RecoveryState::Initiated,
        aura_app::views::RecoveryProcessStatus::WaitingForApprovals => RecoveryState::InProgress,
        aura_app::views::RecoveryProcessStatus::Approved => RecoveryState::ThresholdMet,
        aura_app::views::RecoveryProcessStatus::Completed => RecoveryState::Completed,
        aura_app::views::RecoveryProcessStatus::Failed => RecoveryState::Failed,
    }
}

/// Convert aura-app recovery state to TUI recovery status
fn convert_recovery_status(
    state: &aura_app::views::RecoveryState,
    guardians: &[aura_app::views::Guardian],
) -> RecoveryStatus {
    match &state.active_recovery {
        Some(process) => {
            // Build approvals list
            let approvals: Vec<GuardianApproval> = guardians
                .iter()
                .map(|g| GuardianApproval {
                    guardian_name: g.name.clone(),
                    approved: process.approved_by.contains(&g.id),
                })
                .collect();

            RecoveryStatus {
                state: convert_recovery_state(process.status),
                approvals_received: process.approvals_received,
                threshold: process.approvals_required,
                approvals,
            }
        }
        None => RecoveryStatus {
            state: RecoveryState::None,
            approvals_received: 0,
            threshold: state.threshold,
            approvals: vec![],
        },
    }
}

/// The recovery screen
///
/// ## Pure View Component
///
/// This screen is a pure view that renders based on props from TuiState.
/// All event handling is done by the parent TuiShell (IoApp) via the state machine.
///
/// ## Reactive Updates
///
/// When `AppCoreContext` is available in the context tree, this component will
/// subscribe to recovery state signals and automatically update when:
/// - Guardians are added/removed
/// - Recovery is initiated
/// - Guardian approvals are received
#[component]
pub fn RecoveryScreen(
    props: &RecoveryScreenProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    // Try to get AppCoreContext for reactive signal subscription
    let app_ctx = hooks.try_use_context::<AppCoreContext>();

    // Initialize reactive state from props
    let reactive_guardians = hooks.use_state({
        let initial = props.guardians.clone();
        move || initial
    });
    let reactive_threshold_required = hooks.use_state(|| props.threshold_required);
    let reactive_threshold_total = hooks.use_state(|| props.threshold_total);
    let reactive_recovery_status = hooks.use_state({
        let initial = props.recovery_status.clone();
        move || initial
    });
    let reactive_pending_requests = hooks.use_state({
        let initial = props.pending_requests.clone();
        move || initial
    });

    // Subscribe to recovery signal updates if AppCoreContext is available
    // Uses the unified ReactiveEffects system from aura-core
    if let Some(ctx) = app_ctx {
        hooks.use_future({
            let mut reactive_guardians = reactive_guardians.clone();
            let mut reactive_threshold_required = reactive_threshold_required.clone();
            let mut reactive_threshold_total = reactive_threshold_total.clone();
            let mut reactive_recovery_status = reactive_recovery_status.clone();
            let mut reactive_pending_requests = reactive_pending_requests.clone();
            let app_core = ctx.app_core.clone();
            async move {
                // Helper closure to convert RecoveryState to TUI types
                let convert_state = |recovery_state: &aura_app::views::RecoveryState| {
                    let guardians: Vec<Guardian> = recovery_state
                        .guardians
                        .iter()
                        .map(convert_guardian)
                        .collect();

                    let status = convert_recovery_status(recovery_state, &recovery_state.guardians);

                    let pending: Vec<PendingRequest> = recovery_state
                        .pending_requests
                        .iter()
                        .map(|p| PendingRequest::from(p))
                        .collect();

                    (
                        guardians,
                        recovery_state.threshold,
                        recovery_state.guardian_count,
                        status,
                        pending,
                    )
                };

                // FIRST: Read current signal value to catch up on any changes
                // that happened while this screen was unmounted
                {
                    let core = app_core.read().await;
                    if let Ok(recovery_state) = core.read(&*RECOVERY_SIGNAL).await {
                        let (guardians, threshold, total, status, pending) =
                            convert_state(&recovery_state);
                        reactive_guardians.set(guardians);
                        reactive_threshold_required.set(threshold);
                        reactive_threshold_total.set(total);
                        reactive_recovery_status.set(status);
                        reactive_pending_requests.set(pending);
                    }
                }

                // THEN: Subscribe for future updates
                let mut stream = {
                    let core = app_core.read().await;
                    core.subscribe(&*RECOVERY_SIGNAL)
                };

                // Subscribe to signal updates - runs until component unmounts
                while let Ok(recovery_state) = stream.recv().await {
                    let (guardians, threshold, total, status, pending) =
                        convert_state(&recovery_state);
                    reactive_guardians.set(guardians);
                    reactive_threshold_required.set(threshold);
                    reactive_threshold_total.set(total);
                    reactive_recovery_status.set(status);
                    reactive_pending_requests.set(pending);
                }
            }
        });
    }

    // Use reactive state for rendering
    let guardians = reactive_guardians.read().clone();
    let threshold_required = reactive_threshold_required.get();
    let threshold_total = reactive_threshold_total.get();
    let recovery_status = reactive_recovery_status.read().clone();
    let pending_requests = reactive_pending_requests.read().clone();

    // === Pure view: Use props.view from TuiState instead of local state ===
    let current_tab = props.view.tab;
    let current_guardian_index = props.view.selected_index;
    let current_request_index = props.view.selected_index;
    let request_count = pending_requests.len();

    // === Pure view: No use_terminal_events ===
    // All event handling is done by IoApp (the shell) via the state machine.
    // This component is purely presentational.

    // Layout: TabBar (2 rows) + Content (23 rows) = 25 = MIDDLE_HEIGHT
    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            overflow: Overflow::Hidden,
        ) {
            // Tab bar with pending count badge (2 rows: 1 content + 1 border)
            View(height: 2) {
                RecoveryTabBar(active_tab: current_tab, pending_count: request_count)
            }

            // Content based on active tab (23 rows)
            View(height: 23, overflow: Overflow::Hidden) {
                #(match current_tab {
                    RecoveryTab::Guardians => vec![element! {
                        View(height: 23) {
                            GuardiansPanel(
                                guardians: guardians.clone(),
                                selected_index: current_guardian_index,
                                threshold_required: threshold_required,
                                threshold_total: threshold_total,
                            )
                        }
                    }],
                    RecoveryTab::Recovery => vec![element! {
                        View(height: 23) {
                            RecoveryPanel(status: recovery_status.clone())
                        }
                    }],
                    RecoveryTab::Requests => vec![element! {
                        View(height: 23) {
                            PendingRequestsPanel(
                                requests: pending_requests.clone(),
                                selected_index: current_request_index,
                            )
                        }
                    }],
                })
            }
        }
    }
}

/// Run the recovery screen with sample data
pub async fn run_recovery_screen() -> std::io::Result<()> {
    let guardians = vec![
        Guardian::new("g1", "Alice")
            .with_status(GuardianStatus::Active)
            .with_share(),
        Guardian::new("g2", "Bob")
            .with_status(GuardianStatus::Active)
            .with_share(),
        Guardian::new("g3", "Carol").with_status(GuardianStatus::Pending),
    ];

    let recovery_status = RecoveryStatus {
        state: RecoveryState::Initiated,
        approvals_received: 1,
        threshold: 2,
        approvals: vec![
            GuardianApproval {
                guardian_name: "Alice".to_string(),
                approved: true,
            },
            GuardianApproval {
                guardian_name: "Bob".to_string(),
                approved: false,
            },
        ],
    };

    element! {
        RecoveryScreen(
            guardians: guardians,
            threshold_required: 2u32,
            threshold_total: 3u32,
            recovery_status: recovery_status,
        )
    }
    .fullscreen()
    .await
}
