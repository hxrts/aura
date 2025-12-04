//! # Block Screen
//!
//! Homepage showing the user's block with residents and storage.
//!
//! ## Reactive Signal Subscription
//!
//! When `AppCoreContext` is available, this screen subscribes to block state
//! changes via `use_future` and futures-signals. Updates are pushed to the
//! component automatically, triggering re-renders when data changes.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::KeyHintsBar;
use crate::tui::hooks::AppCoreContext;
use crate::tui::theme::Theme;
use crate::tui::types::{BlockBudget, KeyHint, Message, Resident};

/// Callback type for sending a message in the block channel
pub type BlockSendCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Callback type for inviting someone to the block
pub type BlockInviteCallback = Arc<dyn Fn() + Send + Sync>;

/// Callback type for navigating to neighborhood view
pub type BlockNavCallback = Arc<dyn Fn() + Send + Sync>;

/// Props for ResidentList
#[derive(Default, Props)]
pub struct ResidentListProps {
    pub residents: Vec<Resident>,
    pub selected_index: usize,
}

/// List of residents in the block
#[component]
pub fn ResidentList(props: &ResidentListProps) -> impl Into<AnyElement<'static>> {
    let residents = props.residents.clone();
    let selected = props.selected_index;
    let count = residents.len();
    let title = format!("Residents ({}/8)", count);

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
        ) {
            View(padding_left: 1) {
                Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(flex_direction: FlexDirection::Column, padding: 1) {
                #(if residents.is_empty() {
                    vec![element! {
                        View {
                            Text(content: "No residents yet", color: Theme::TEXT_MUTED)
                        }
                    }]
                } else {
                    residents.iter().enumerate().map(|(idx, r)| {
                        let is_selected = idx == selected;
                        let bg = if is_selected { Theme::BG_SELECTED } else { Theme::BG_DARK };
                        let name = r.name.clone();
                        let steward_badge = if r.is_steward { " ★" } else { "" }.to_string();
                        let self_badge = if r.is_self { " (you)" } else { "" }.to_string();
                        element! {
                            View(flex_direction: FlexDirection::Row, background_color: bg, padding_left: 1) {
                                Text(content: name, color: Theme::TEXT)
                                Text(content: steward_badge, color: Theme::WARNING)
                                Text(content: self_badge, color: Theme::TEXT_MUTED)
                            }
                        }
                    }).collect()
                })
            }
        }
    }
}

/// Props for BlockMessagesPanel
#[derive(Default, Props)]
pub struct BlockMessagesPanelProps {
    pub messages: Vec<Message>,
    pub channel_name: String,
}

/// Block messages panel
#[component]
pub fn BlockMessagesPanel(props: &BlockMessagesPanelProps) -> impl Into<AnyElement<'static>> {
    let messages = props.messages.clone();
    let title = format!("# {}", props.channel_name);

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER_FOCUS,
        ) {
            View(padding_left: 1) {
                Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                padding: 1,
                overflow: Overflow::Scroll,
            ) {
                #(if messages.is_empty() {
                    vec![element! {
                        View {
                            Text(content: "No messages yet. Start the conversation!", color: Theme::TEXT_MUTED)
                        }
                    }]
                } else {
                    messages.iter().map(|msg| {
                        let bg = if msg.is_own { Theme::MSG_OWN } else { Theme::MSG_OTHER };
                        let sender = msg.sender.clone();
                        let content = msg.content.clone();
                        let ts = msg.timestamp.clone();
                        element! {
                            View(
                                flex_direction: FlexDirection::Column,
                                margin_bottom: 1,
                                background_color: bg,
                                padding_left: 1,
                            ) {
                                View(flex_direction: FlexDirection::Row, gap: 2) {
                                    Text(content: sender, weight: Weight::Bold, color: Theme::TEXT_HIGHLIGHT)
                                    Text(content: ts, color: Theme::TEXT_MUTED)
                                }
                                Text(content: content, color: Theme::TEXT)
                            }
                        }
                    }).collect()
                })
            }
        }
    }
}

/// Props for StorageBudgetPanel
#[derive(Default, Props)]
pub struct StorageBudgetPanelProps {
    pub budget: BlockBudget,
}

/// Storage budget panel
#[component]
pub fn StorageBudgetPanel(props: &StorageBudgetPanelProps) -> impl Into<AnyElement<'static>> {
    let b = &props.budget;
    let usage_pct = b.usage_percent();
    let usage_color = if usage_pct > 90.0 {
        Theme::ERROR
    } else if usage_pct > 70.0 {
        Theme::WARNING
    } else {
        Theme::SUCCESS
    };

    let total_mb = b.total as f64 / (1024.0 * 1024.0);
    let used_mb = b.used as f64 / (1024.0 * 1024.0);
    let usage_text = format!("{:.1} / {:.1} MB ({:.0}%)", used_mb, total_mb, usage_pct);
    let residents_text = format!("Residents: {}/{}", b.resident_count, b.max_residents);

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            padding: 1,
        ) {
            View(flex_direction: FlexDirection::Row) {
                Text(content: "Storage: ", color: Theme::TEXT_MUTED)
                Text(content: usage_text, color: usage_color)
            }
            View(flex_direction: FlexDirection::Row) {
                Text(content: residents_text, color: Theme::TEXT)
            }
        }
    }
}

/// Props for BlockScreen
#[derive(Default, Props)]
pub struct BlockScreenProps {
    pub block_name: String,
    pub residents: Vec<Resident>,
    pub messages: Vec<Message>,
    pub budget: BlockBudget,
    pub channel_name: String,
    /// Callback when sending a message (receives message content)
    pub on_send: Option<BlockSendCallback>,
    /// Callback when inviting someone to the block
    pub on_invite: Option<BlockInviteCallback>,
    /// Callback when navigating to neighborhood view
    pub on_go_neighborhood: Option<BlockNavCallback>,
}

/// Convert aura-app resident role to TUI is_steward flag
fn is_steward_role(role: aura_app::views::block::ResidentRole) -> bool {
    matches!(
        role,
        aura_app::views::block::ResidentRole::Admin | aura_app::views::block::ResidentRole::Owner
    )
}

/// Convert aura-app resident to TUI resident
fn convert_resident(r: &aura_app::views::block::Resident, my_id: &str) -> Resident {
    Resident {
        id: r.id.clone(),
        name: r.name.clone(),
        is_steward: is_steward_role(r.role),
        is_self: r.id == my_id,
    }
}

/// Convert aura-app storage budget to TUI block budget
fn convert_budget(
    storage: &aura_app::views::block::StorageBudget,
    resident_count: u32,
) -> BlockBudget {
    BlockBudget {
        total: storage.total_bytes,
        used: storage.used_bytes,
        resident_count: resident_count as u8,
        max_residents: 8, // Default max
    }
}

/// The block screen (homepage)
///
/// ## Reactive Updates
///
/// When `AppCoreContext` is available in the context tree, this component will
/// subscribe to block state signals and automatically update when:
/// - Residents join/leave
/// - Storage usage changes
/// - Block name changes
#[component]
pub fn BlockScreen(props: &BlockScreenProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    // Try to get AppCoreContext for reactive signal subscription
    let app_ctx = hooks.try_use_context::<AppCoreContext>();

    // Initialize reactive state from props
    let reactive_block_name = hooks.use_state({
        let initial = props.block_name.clone();
        move || initial
    });
    let reactive_residents = hooks.use_state({
        let initial = props.residents.clone();
        move || initial
    });
    let reactive_budget = hooks.use_state({
        let initial = props.budget.clone();
        move || initial
    });

    // Subscribe to block signal updates if AppCoreContext is available
    if let Some(ctx) = app_ctx {
        hooks.use_future({
            let mut reactive_block_name = reactive_block_name.clone();
            let mut reactive_residents = reactive_residents.clone();
            let mut reactive_budget = reactive_budget.clone();
            let app_core = ctx.app_core.clone();
            async move {
                use futures_signals::signal::SignalExt;

                let signal = {
                    let core = app_core.read().await;
                    core.block_signal()
                };

                signal
                    .for_each(|block_state| {
                        // Use the block id as a proxy for "my_id" since we don't have access to identity
                        let my_id = &block_state.id;

                        let residents: Vec<Resident> = block_state
                            .residents
                            .iter()
                            .map(|r| convert_resident(r, my_id))
                            .collect();

                        let budget =
                            convert_budget(&block_state.storage, block_state.resident_count);

                        reactive_block_name.set(block_state.name.clone());
                        reactive_residents.set(residents);
                        reactive_budget.set(budget);
                        async {}
                    })
                    .await;
            }
        });
    }

    // Use reactive state for rendering
    let block_name = reactive_block_name.read().clone();
    let residents = reactive_residents.read().clone();
    let budget = reactive_budget.read().clone();

    // Messages come from props (would need chat signal integration)
    let messages = props.messages.clone();
    let channel_name = props.channel_name.clone();

    let resident_index = hooks.use_state(|| 0usize);

    let hints = vec![
        KeyHint::new("↑↓", "Navigate"),
        KeyHint::new("Enter", "Send message"),
        KeyHint::new("i", "Invite"),
        KeyHint::new("n", "Neighborhood"),
        KeyHint::new("Esc", "Menu"),
    ];

    let current_resident_index = resident_index.get();

    // Clone callbacks for event handler
    let on_send = props.on_send.clone();
    let on_invite = props.on_invite.clone();
    let on_go_neighborhood = props.on_go_neighborhood.clone();

    hooks.use_terminal_events({
        let mut resident_index = resident_index.clone();
        let resident_count = residents.len();
        move |event| match event {
            TerminalEvent::Key(KeyEvent { code, .. }) => match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    let idx = resident_index.get();
                    if idx > 0 {
                        resident_index.set(idx - 1);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let idx = resident_index.get();
                    if idx + 1 < resident_count {
                        resident_index.set(idx + 1);
                    }
                }
                // Send message (would open message input in full implementation)
                KeyCode::Enter => {
                    if let Some(ref callback) = on_send {
                        // In a full implementation, this would send the composed message
                        // For now, this signals intent to send a message
                        callback(String::new());
                    }
                }
                // Invite to block
                KeyCode::Char('i') => {
                    if let Some(ref callback) = on_invite {
                        callback();
                    }
                }
                // Go to neighborhood view
                KeyCode::Char('n') => {
                    if let Some(ref callback) = on_go_neighborhood {
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
                Text(content: block_name, weight: Weight::Bold, color: Theme::PRIMARY)
            }

            // Main content
            View(
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
                gap: 1,
            ) {
                // Sidebar (25%)
                View(width: 25pct, flex_direction: FlexDirection::Column, gap: 1) {
                    ResidentList(residents: residents, selected_index: current_resident_index)
                    StorageBudgetPanel(budget: budget)
                }
                // Messages (75%)
                BlockMessagesPanel(messages: messages, channel_name: channel_name)
            }

            // Key hints
            KeyHintsBar(hints: hints)
        }
    }
}

/// Run the block screen with sample data
pub async fn run_block_screen() -> std::io::Result<()> {
    let residents = vec![
        Resident::new("r1", "You").is_current_user().steward(),
        Resident::new("r2", "Alice"),
        Resident::new("r3", "Bob"),
    ];

    let messages = vec![
        Message::new("1", "Alice", "Welcome to the block!")
            .with_timestamp("10:00")
            .own(false),
        Message::new("2", "You", "Thanks for having me!")
            .with_timestamp("10:05")
            .own(true),
        Message::new("3", "Bob", "Hey everyone!")
            .with_timestamp("10:10")
            .own(false),
    ];

    let budget = BlockBudget {
        total: 10 * 1024 * 1024, // 10 MB
        used: 3 * 1024 * 1024,   // 3 MB
        resident_count: 3,
        max_residents: 8,
    };

    element! {
        BlockScreen(
            block_name: "My Block".to_string(),
            residents: residents,
            messages: messages,
            budget: budget,
            channel_name: "general".to_string(),
        )
    }
    .fullscreen()
    .await
}
