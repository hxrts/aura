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

use aura_app::signal_defs::{BLOCK_SIGNAL, CHAT_SIGNAL, CONTACTS_SIGNAL};

use crate::tui::callbacks::{
    BlockInviteCallback, BlockNavCallback, BlockSendCallback, GrantStewardCallback,
    RevokeStewardCallback,
};
use crate::tui::components::MessageInput;
use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::layout::dim;
use crate::tui::props::BlockViewProps;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{format_timestamp, BlockBudget, Contact, Message, Resident};

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

/// Props for PinnedMessagesPanel
#[derive(Default, Props)]
pub struct PinnedMessagesPanelProps {
    pub pinned: Vec<PinnedMessageRow>,
}

#[derive(Clone, Debug, Default)]
pub struct PinnedMessageRow {
    pub message_id: String,
    pub pinned_by: String,
    pub pinned_at: String,
}

/// Pinned messages panel
#[component]
pub fn PinnedMessagesPanel(props: &PinnedMessagesPanelProps) -> impl Into<AnyElement<'static>> {
    let pinned = props.pinned.clone();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_shrink: 1.0,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            padding_left: 1,
            padding_right: 1,
        ) {
            View(padding_left: 0) {
                Text(content: "Pinned", weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                padding_top: 1,
                overflow: Overflow::Scroll,
            ) {
                #(if pinned.is_empty() {
                    vec![element! {
                        View {
                            Text(content: "No pinned messages", color: Theme::TEXT_MUTED)
                        }
                    }]
                } else {
                    pinned.iter().map(|item| {
                        let message_id = item.message_id.clone();
                        let pinned_by = item.pinned_by.clone();
                        let pinned_at = item.pinned_at.clone();
                        element! {
                            View(
                                flex_direction: FlexDirection::Column,
                                margin_bottom: 1,
                                padding_left: 0,
                            ) {
                                Text(content: format!("• {}", message_id), color: Theme::TEXT)
                                Text(content: format!("{} @ {}", pinned_by, pinned_at), color: Theme::TEXT_MUTED)
                            }
                        }
                    }).collect()
                })
            }
        }
    }
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
///
/// ## Reactive Data Model
///
/// Domain data (block_name, residents, messages, budget, channel_name, contacts) is NOT
/// passed as props. Instead, the component subscribes to BLOCK_SIGNAL and CONTACTS_SIGNAL
/// directly via AppCoreContext. This ensures a single source of truth and prevents stale data bugs.
#[derive(Default, Props)]
pub struct BlockScreenProps {
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
fn convert_resident(r: &aura_app::views::block::Resident) -> Resident {
    Resident {
        id: r.id.to_string(),
        name: r.name.clone(),
        is_steward: is_steward_role(r.role),
        // Note: is_self should be determined by comparing with current user's AuthorityId
        // which isn't available in BlockState. The original code incorrectly compared with block ID.
        is_self: false,
    }
}

/// Convert aura-app storage budget to TUI block budget
fn convert_budget(storage: &aura_app::BlockFlowBudget, resident_count: u32) -> BlockBudget {
    BlockBudget {
        total: storage.total_allocation(),
        used: storage.total_used(),
        resident_count: resident_count as u8,
        max_residents: aura_app::MAX_RESIDENTS,
    }
}

fn format_contact_name(authority_id: &str, contacts: &[Contact]) -> String {
    if let Some(contact) = contacts.iter().find(|c| c.id == authority_id) {
        if !contact.nickname.is_empty() {
            return contact.nickname.clone();
        }
        if let Some(name) = contact.suggested_name.as_ref() {
            if !name.is_empty() {
                return name.clone();
            }
        }
    }
    short_id(authority_id, 8)
}

fn short_id(id: &str, len: usize) -> String {
    let trimmed = id.trim();
    if trimmed.len() <= len {
        trimmed.to_string()
    } else {
        trimmed.chars().take(len).collect()
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
    // Get AppCoreContext for reactive signal subscription (required for domain data)
    let app_ctx = hooks.use_context::<AppCoreContext>();

    // Initialize reactive state with defaults - will be populated by signal subscriptions
    let reactive_block_name = hooks.use_state(String::new);
    let reactive_residents = hooks.use_state(Vec::new);
    let reactive_budget = hooks.use_state(BlockBudget::default);
    let reactive_pins = hooks.use_state(|| Vec::<PinnedMessageRow>::new());
    let reactive_messages = hooks.use_state(Vec::new);
    let reactive_channel_name = hooks.use_state(|| "general".to_string());
    let reactive_contacts = hooks.use_state(Vec::new);

    // Subscribe to contacts signal for name resolution
    hooks.use_future({
        let mut reactive_contacts = reactive_contacts.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CONTACTS_SIGNAL, move |contacts_state| {
                let contacts: Vec<Contact> =
                    contacts_state.contacts.iter().map(Contact::from).collect();
                reactive_contacts.set(contacts);
            })
            .await;
        }
    });

    // Subscribe to block signal updates (for residents, budget, pinned messages)
    hooks.use_future({
        let mut reactive_block_name = reactive_block_name.clone();
        let mut reactive_residents = reactive_residents.clone();
        let mut reactive_budget = reactive_budget.clone();
        let mut reactive_pins = reactive_pins.clone();
        let reactive_contacts = reactive_contacts.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*BLOCK_SIGNAL, move |block_state| {
                let contacts = reactive_contacts.read().clone();

                let residents: Vec<Resident> =
                    block_state.residents.iter().map(convert_resident).collect();

                let budget = convert_budget(&block_state.storage, block_state.resident_count);

                let mut pinned = Vec::new();
                for message_id in &block_state.pinned_messages {
                    if let Some(meta) = block_state.pinned_metadata.get(message_id) {
                        pinned.push(PinnedMessageRow {
                            message_id: short_id(&meta.message_id, 10),
                            pinned_by: format_contact_name(
                                &meta.pinned_by.to_string(),
                                &contacts,
                            ),
                            pinned_at: format_timestamp(meta.pinned_at),
                        });
                    } else {
                        pinned.push(PinnedMessageRow {
                            message_id: short_id(message_id, 10),
                            pinned_by: "unknown".to_string(),
                            pinned_at: "--:--".to_string(),
                        });
                    }
                }

                reactive_block_name.set(block_state.name.clone());
                reactive_residents.set(residents);
                reactive_budget.set(budget);
                reactive_pins.set(pinned);
            })
            .await;
        }
    });

    // Subscribe to chat signal for block messages
    // Block messages are part of the unified chat system, filtered by selected channel
    hooks.use_future({
        let mut reactive_messages = reactive_messages.clone();
        let mut reactive_channel_name = reactive_channel_name.clone();
        let reactive_contacts = reactive_contacts.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CHAT_SIGNAL, move |chat_state| {
                let contacts = reactive_contacts.read().clone();

                // Get the selected channel name
                if let Some(channel_id) = &chat_state.selected_channel_id {
                    if let Some(channel) = chat_state.channels.iter().find(|c| &c.id == channel_id)
                    {
                        reactive_channel_name.set(channel.name.clone());
                    }
                }

                // Convert chat messages to TUI Message type
                let messages: Vec<Message> = chat_state
                    .messages
                    .iter()
                    .map(|m| {
                        let sender_name = format_contact_name(&m.sender_id.to_string(), &contacts);
                        Message::new(&m.id, &sender_name, &m.content)
                            .with_timestamp(format_timestamp(m.timestamp))
                            .own(m.is_own)
                    })
                    .collect();

                reactive_messages.set(messages);
            })
            .await;
        }
    });

    // Use reactive state for rendering (populated by signal subscriptions)
    let residents = reactive_residents.read().clone();
    let budget = reactive_budget.read().clone();
    let pinned = reactive_pins.read().clone();
    let messages = reactive_messages.read().clone();
    let channel_name = reactive_channel_name.read().clone();

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
                    PinnedMessagesPanel(pinned: pinned)
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

/// Run the block screen (requires AppCoreContext for domain data)
pub async fn run_block_screen() -> std::io::Result<()> {
    // Note: This standalone runner won't have domain data without AppCoreContext.
    // Domain data is obtained via signal subscriptions when context is available.
    element! {
        BlockScreen(
            view: BlockViewProps::default(),
        )
    }
    .fullscreen()
    .await
}
