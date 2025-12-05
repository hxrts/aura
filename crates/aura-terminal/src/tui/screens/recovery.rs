//! # Recovery Screen
//!
//! Guardian management and account recovery
//!
//! ## Reactive Signal Subscription
//!
//! When `AppCoreContext` is available, this screen subscribes to recovery state
//! changes via `use_future` and futures-signals. Updates are pushed to the
//! component automatically, triggering re-renders when data changes.

use iocraft::prelude::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::tui::components::{EmptyState, KeyHintsBar};
use crate::tui::hooks::AppCoreContext;
use crate::tui::theme::{Icons, Spacing, Theme};
use crate::tui::types::{
    Guardian, GuardianApproval, GuardianStatus, KeyHint, RecoveryState, RecoveryStatus, RecoveryTab,
};

/// Callback type for recovery actions (no args)
pub type RecoveryCallback = Arc<dyn Fn() + Send + Sync>;

/// Props for TabBar
#[derive(Default, Props)]
pub struct TabBarProps {
    pub active_tab: RecoveryTab,
}

/// Tab navigation bar
#[component]
pub fn TabBar(props: &TabBarProps) -> impl Into<AnyElement<'static>> {
    let active = props.active_tab;

    element! {
        View(
            flex_direction: FlexDirection::Row,
            gap: Spacing::MD,
            padding: Spacing::PANEL_PADDING,
            border_style: BorderStyle::Single,
            border_edges: Edges::Bottom,
            border_color: Theme::BORDER,
        ) {
            #([RecoveryTab::Guardians, RecoveryTab::Recovery].iter().map(|&tab| {
                let is_active = tab == active;
                let color = if is_active { Theme::PRIMARY } else { Theme::TEXT_MUTED };
                let weight = if is_active { Weight::Bold } else { Weight::Normal };
                let title = tab.title().to_string();
                element! {
                    Text(content: title, color: color, weight: weight)
                }
            }))
        }
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
            gap: Spacing::XS,
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
                            let bg = if is_selected { Theme::BG_SELECTED } else { Theme::BG_DARK };
                            let status_color = match g.status {
                                GuardianStatus::Active => Theme::SUCCESS,
                                GuardianStatus::Pending => Theme::WARNING,
                                GuardianStatus::Offline => Theme::TEXT_MUTED,
                                GuardianStatus::Declined | GuardianStatus::Removed => Theme::ERROR,
                            };
                            let icon = g.status.icon().to_string();
                            let id = g.id.clone();
                            let name = g.name.clone();
                            let share_text = if g.has_share { " [share]" } else { "" }.to_string();
                            element! {
                                View(key: id, flex_direction: FlexDirection::Row, background_color: bg, padding_left: Spacing::XS, gap: Spacing::XS) {
                                    Text(content: icon, color: status_color)
                                    Text(content: name, color: Theme::TEXT)
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
                View(flex_direction: FlexDirection::Row) {
                    Text(content: "Threshold: ", color: Theme::TEXT_MUTED)
                    Text(content: threshold_text, color: Theme::TEXT)
                }
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
            gap: Spacing::XS,
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
                View(flex_direction: FlexDirection::Row) {
                    Text(content: "Progress: ", color: Theme::TEXT_MUTED)
                    Text(content: progress_text, color: Theme::TEXT)
                }
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

/// Props for RecoveryScreen
#[derive(Default, Props)]
pub struct RecoveryScreenProps {
    pub guardians: Vec<Guardian>,
    pub threshold_required: u32,
    pub threshold_total: u32,
    pub recovery_status: RecoveryStatus,
    /// Callback when starting recovery
    pub on_start_recovery: Option<RecoveryCallback>,
    /// Callback when adding a guardian
    pub on_add_guardian: Option<RecoveryCallback>,
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

    // Subscribe to recovery signal updates if AppCoreContext is available
    if let Some(ctx) = app_ctx {
        hooks.use_future({
            let mut reactive_guardians = reactive_guardians.clone();
            let mut reactive_threshold_required = reactive_threshold_required.clone();
            let mut reactive_threshold_total = reactive_threshold_total.clone();
            let mut reactive_recovery_status = reactive_recovery_status.clone();
            let app_core = ctx.app_core.clone();
            async move {
                use futures_signals::signal::SignalExt;

                let signal = {
                    let core = app_core.read().await;
                    core.recovery_signal()
                };

                signal
                    .for_each(|recovery_state| {
                        // Convert guardians
                        let guardians: Vec<Guardian> = recovery_state
                            .guardians
                            .iter()
                            .map(convert_guardian)
                            .collect();

                        // Convert recovery status
                        let status =
                            convert_recovery_status(&recovery_state, &recovery_state.guardians);

                        reactive_guardians.set(guardians);
                        reactive_threshold_required.set(recovery_state.threshold);
                        reactive_threshold_total.set(recovery_state.guardian_count);
                        reactive_recovery_status.set(status);
                        async {}
                    })
                    .await;
            }
        });
    }

    // Use reactive state for rendering
    let guardians = reactive_guardians.read().clone();
    let threshold_required = reactive_threshold_required.get();
    let threshold_total = reactive_threshold_total.get();
    let recovery_status = reactive_recovery_status.read().clone();

    let active_tab = hooks.use_state(|| RecoveryTab::Guardians);
    let guardian_index = hooks.use_state(|| 0usize);

    let hints = vec![
        KeyHint::new("←→", "Switch tab"),
        KeyHint::new("↑↓", "Navigate"),
        KeyHint::new("a", "Add guardian"),
        KeyHint::new("s", "Start recovery"),
        KeyHint::new("Esc", "Back"),
    ];

    let current_tab = active_tab.get();
    let current_guardian_index = guardian_index.get();

    // Clone callbacks for event handler
    let on_start_recovery = props.on_start_recovery.clone();
    let on_add_guardian = props.on_add_guardian.clone();

    // Throttle for navigation keys - persists across renders using use_ref
    let mut nav_throttle = hooks.use_ref(|| Instant::now() - Duration::from_millis(200));
    let throttle_duration = Duration::from_millis(150);

    hooks.use_terminal_events({
        let mut active_tab = active_tab.clone();
        let mut guardian_index = guardian_index.clone();
        let guardian_count = guardians.len();
        move |event| match event {
            TerminalEvent::Key(KeyEvent { code, .. }) => match code {
                KeyCode::Left | KeyCode::Char('1') => {
                    active_tab.set(RecoveryTab::Guardians);
                }
                KeyCode::Right | KeyCode::Char('2') => {
                    active_tab.set(RecoveryTab::Recovery);
                }
                KeyCode::Tab => {
                    active_tab.set(active_tab.get().next());
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let should_move = nav_throttle.read().elapsed() >= throttle_duration;
                    if should_move
                        && active_tab.get() == RecoveryTab::Guardians
                        && guardian_count > 0
                    {
                        let idx = guardian_index.get();
                        if idx > 0 {
                            guardian_index.set(idx - 1);
                        }
                        nav_throttle.set(Instant::now());
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let should_move = nav_throttle.read().elapsed() >= throttle_duration;
                    if should_move
                        && active_tab.get() == RecoveryTab::Guardians
                        && guardian_count > 0
                    {
                        let idx = guardian_index.get();
                        if idx + 1 < guardian_count {
                            guardian_index.set(idx + 1);
                        }
                        nav_throttle.set(Instant::now());
                    }
                }
                // Add guardian - triggers callback
                KeyCode::Char('a') => {
                    if let Some(ref callback) = on_add_guardian {
                        callback();
                    }
                }
                // Start recovery - triggers callback
                KeyCode::Char('s') => {
                    if let Some(ref callback) = on_start_recovery {
                        callback();
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
                padding: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(content: "Recovery", weight: Weight::Bold, color: Theme::PRIMARY)
            }

            // Tab bar
            TabBar(active_tab: current_tab)

            // Content based on active tab
            View(flex_grow: 1.0, padding: Spacing::PANEL_PADDING) {
                #(match current_tab {
                    RecoveryTab::Guardians => vec![element! {
                        View(flex_grow: 1.0) {
                            GuardiansPanel(
                                guardians: guardians.clone(),
                                selected_index: current_guardian_index,
                                threshold_required: threshold_required,
                                threshold_total: threshold_total,
                            )
                        }
                    }],
                    RecoveryTab::Recovery => vec![element! {
                        View(flex_grow: 1.0) {
                            RecoveryPanel(status: recovery_status.clone())
                        }
                    }],
                })
            }

            // Key hints
            KeyHintsBar(hints: hints)
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
        Guardian::new("g3", "Charlie").with_status(GuardianStatus::Pending),
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
