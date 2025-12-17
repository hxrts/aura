//! # Block Screen
//!
//! Homepage showing the user's block with residents and storage.
//!
//! ## Reactive Signal Subscription
//!
//! When `AppCoreContext` is available, this screen subscribes to block state
//! changes via the unified `ReactiveEffects` system. Updates are pushed to the
//! component automatically, triggering re-renders when data changes.
//!
//! Uses `aura_app::signal_defs::BLOCK_SIGNAL` with `ReactiveEffects::subscribe()`.

use iocraft::prelude::*;

use aura_app::signal_defs::BLOCK_SIGNAL;
use aura_core::effects::reactive::ReactiveEffects;

use crate::tui::callbacks::{
    BlockInviteCallback, BlockNavCallback, BlockSendCallback, GrantStewardCallback,
    RevokeStewardCallback,
};
use crate::tui::components::MessageInput;
use crate::tui::hooks::AppCoreContext;
use crate::tui::layout::dim;
use crate::tui::props::BlockViewProps;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{BlockBudget, Contact, Message, Resident};

/// Which panel is focused
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockFocus {
    /// Residents list is focused (normal mode)
    #[default]
    Residents,
    /// Messages panel is focused
    Messages,
    /// Message input is focused (insert mode)
    Input,
}

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
            flex_grow: 0.0,
            flex_shrink: 1.0,
            max_height: 8,  // Limit height to make room for Storage panel
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            overflow: Overflow::Hidden,
        ) {
            View(padding_left: 1) {
                Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(flex_direction: FlexDirection::Column, flex_grow: 1.0, flex_shrink: 1.0, padding: 1, overflow: Overflow::Scroll) {
                #(if residents.is_empty() {
                    vec![element! {
                        View {
                            Text(content: "No residents yet", color: Theme::TEXT_MUTED)
                        }
                    }]
                } else {
                    residents.iter().enumerate().map(|(idx, r)| {
                        let is_selected = idx == selected;
                        // Use consistent list item colors
                        let bg = if is_selected { Theme::LIST_BG_SELECTED } else { Theme::LIST_BG_NORMAL };
                        let text_color = if is_selected { Theme::LIST_TEXT_SELECTED } else { Theme::LIST_TEXT_NORMAL };
                        let muted_color = if is_selected { Theme::LIST_TEXT_SELECTED } else { Theme::LIST_TEXT_MUTED };
                        let name = r.name.clone();
                        let id = r.id.clone();
                        let steward_badge = if r.is_steward { " ⚖︎" } else { "" }.to_string();
                        let self_badge = if r.is_self { " (you)" } else { "" }.to_string();
                        element! {
                            View(key: id, flex_direction: FlexDirection::Row, background_color: bg, padding_left: 1) {
                                Text(content: name, color: text_color)
                                Text(content: steward_badge, color: Theme::WARNING)
                                Text(content: self_badge, color: muted_color)
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
            flex_shrink: 1.0,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER_FOCUS,
        ) {
            View(padding_left: 1) {
                Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
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
    // Compact format to fit narrow sidebar
    let usage_text = format!("{:.0}/{:.0}MB", used_mb, total_mb);
    let pct_text = format!("({}%)", usage_pct as u32);
    let residents_text = format!("{}/{}", b.resident_count, b.max_residents);

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_shrink: 1.0,  // Allow shrinking to fit within container
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            padding_left: 1,
            padding_right: 1,
        ) {
            View(padding_left: 0) {
                Text(content: "Storage", weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(flex_direction: FlexDirection::Row, gap: 1) {
                Text(content: usage_text, color: usage_color)
                Text(content: pct_text, color: Theme::TEXT_MUTED)
            }
            View(flex_direction: FlexDirection::Row) {
                Text(content: "Members: ", color: Theme::TEXT_MUTED)
                Text(content: residents_text, color: Theme::TEXT)
            }
        }
    }
}

/// Props for BlockScreen
///
/// ## Compile-Time Safety
///
/// The `view` field is a required struct that embeds all view state from TuiState.
/// This makes it a **compile-time error** to forget any view state field.
#[derive(Default, Props)]
pub struct BlockScreenProps {
    // === Domain data (from reactive signals) ===
    pub block_name: String,
    pub residents: Vec<Resident>,
    pub messages: Vec<Message>,
    pub budget: BlockBudget,
    pub channel_name: String,
    /// Available contacts for invite modal
    pub contacts: Vec<Contact>,

    // === View state from TuiState (REQUIRED - compile-time enforced) ===
    /// All view state extracted from TuiState via `extract_block_view_props()`.
    /// This is a single struct field so forgetting any view state is a compile error.
    pub view: BlockViewProps,

    // === Callbacks (still needed for effect dispatch) ===
    /// Callback when sending a message (receives message content)
    pub on_send: Option<BlockSendCallback>,
    /// Callback when inviting someone to the block (receives contact_id)
    pub on_invite: Option<BlockInviteCallback>,
    /// Callback when navigating to neighborhood view
    pub on_go_neighborhood: Option<BlockNavCallback>,
    /// Callback when granting steward role (receives resident_id)
    pub on_grant_steward: Option<GrantStewardCallback>,
    /// Callback when revoking steward role (receives resident_id)
    pub on_revoke_steward: Option<RevokeStewardCallback>,
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
/// ## Pure View Component
///
/// This screen is a pure view that renders based on props from TuiState.
/// All event handling is done by the parent TuiShell (IoApp) via the state machine.
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
    // Uses the unified ReactiveEffects system from aura-core
    if let Some(ctx) = app_ctx {
        hooks.use_future({
            let mut reactive_block_name = reactive_block_name.clone();
            let mut reactive_residents = reactive_residents.clone();
            let mut reactive_budget = reactive_budget.clone();
            let app_core = ctx.app_core.clone();
            async move {
                // Helper closure to convert BlockState to TUI types
                let convert_block_state = |block_state: &aura_app::views::BlockState| {
                    let my_id = &block_state.id;

                    let residents: Vec<Resident> = block_state
                        .residents
                        .iter()
                        .map(|r| convert_resident(r, my_id))
                        .collect();

                    let budget = convert_budget(&block_state.storage, block_state.resident_count);

                    (block_state.name.clone(), residents, budget)
                };

                // FIRST: Read current signal value to catch up on any changes
                // that happened while this screen was unmounted
                {
                    let core = app_core.read().await;
                    if let Ok(block_state) = core.read(&*BLOCK_SIGNAL).await {
                        let (name, residents, budget) = convert_block_state(&block_state);
                        reactive_block_name.set(name);
                        reactive_residents.set(residents);
                        reactive_budget.set(budget);
                    }
                }

                // THEN: Subscribe for future updates
                let mut stream = {
                    let core = app_core.read().await;
                    core.subscribe(&*BLOCK_SIGNAL)
                };

                // Subscribe to signal updates - runs until component unmounts
                while let Ok(block_state) = stream.recv().await {
                    let (name, residents, budget) = convert_block_state(&block_state);
                    reactive_block_name.set(name);
                    reactive_residents.set(residents);
                    reactive_budget.set(budget);
                }
            }
        });
    }

    // Use reactive state for rendering
    let residents = reactive_residents.read().clone();
    let budget = reactive_budget.read().clone();

    // Messages come from props
    let messages = props.messages.clone();
    let channel_name = props.channel_name.clone();

    // === Pure view: Use props.view from TuiState instead of local state ===
    let current_focus = props.view.focus;
    let current_resident_index = props.view.selected_resident;
    let display_input_text = props.view.input_buffer.clone();
    let input_focused = props.view.insert_mode || current_focus == BlockFocus::Input;

    // === Pure view: No use_terminal_events ===
    // All event handling is done by IoApp (the shell) via the state machine.
    // This component is purely presentational.

    // Layout: Main content (22 rows) + MessageInput (3 rows) = 25 = MIDDLE_HEIGHT
    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            overflow: Overflow::Hidden,
        ) {
            // Main content - fixed 22 rows for sidebar + messages
            View(
                flex_direction: FlexDirection::Row,
                height: 22,
                overflow: Overflow::Hidden,
                gap: Spacing::XS,
            ) {
                // Sidebar (24 chars = 30% of 80) - overflow scroll allows internal scrolling
                View(width: 24, flex_direction: FlexDirection::Column, overflow: Overflow::Scroll, gap: 0) {
                    ResidentList(residents: residents, selected_index: current_resident_index)
                    StorageBudgetPanel(budget: budget)
                }
                // Messages (remaining width ~55 chars)
                View(flex_grow: 1.0, height: 22, overflow: Overflow::Hidden) {
                    BlockMessagesPanel(messages: messages, channel_name: channel_name)
                }
            }

            // Message input (3 rows)
            View(height: 3) {
                MessageInput(
                    value: display_input_text,
                    placeholder: "Type a message...".to_string(),
                    focused: input_focused,
                    reply_to: None::<String>,
                    sending: false,
                )
            }
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
