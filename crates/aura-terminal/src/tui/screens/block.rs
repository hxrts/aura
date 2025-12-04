//! # Block Screen
//!
//! Homepage showing the user's block with residents and storage.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::KeyHintsBar;
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

/// The block screen (homepage)
#[component]
pub fn BlockScreen(props: &BlockScreenProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let resident_index = hooks.use_state(|| 0usize);

    let hints = vec![
        KeyHint::new("↑↓", "Navigate"),
        KeyHint::new("Enter", "Send message"),
        KeyHint::new("i", "Invite"),
        KeyHint::new("n", "Neighborhood"),
        KeyHint::new("Esc", "Menu"),
    ];

    let block_name = props.block_name.clone();
    let residents = props.residents.clone();
    let messages = props.messages.clone();
    let budget = props.budget.clone();
    let channel_name = props.channel_name.clone();
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
