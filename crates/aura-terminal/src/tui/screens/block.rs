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
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::tui::components::{ContactSelectModal, ContactSelectState, MessageInput};
use crate::tui::hooks::AppCoreContext;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{BlockBudget, Contact, Message, Resident};

/// Input text shared between render and event handler (thread-safe)
type SharedText = Arc<RwLock<String>>;

/// Which panel is focused
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockFocus {
    /// Residents list is focused (normal mode)
    #[default]
    Residents,
    /// Message input is focused (insert mode)
    Input,
}

/// Callback type for sending a message in the block channel
pub type BlockSendCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Callback type for inviting someone to the block (contact_id: String)
pub type BlockInviteCallback = Arc<dyn Fn(String) + Send + Sync>;

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
#[derive(Default, Props)]
pub struct BlockScreenProps {
    pub block_name: String,
    pub residents: Vec<Resident>,
    pub messages: Vec<Message>,
    pub budget: BlockBudget,
    pub channel_name: String,
    /// Available contacts for invite modal
    pub contacts: Vec<Contact>,
    /// Callback when sending a message (receives message content)
    pub on_send: Option<BlockSendCallback>,
    /// Callback when inviting someone to the block (receives contact_id)
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
    let residents = reactive_residents.read().clone();
    let budget = reactive_budget.read().clone();

    // Messages come from props (would need chat signal integration)
    let messages = props.messages.clone();
    let channel_name = props.channel_name.clone();

    let resident_index = hooks.use_state(|| 0usize);
    let focus = hooks.use_state(|| BlockFocus::Residents);
    let invite_modal_state = hooks.use_state(ContactSelectState::new);

    // Input text state - use Arc<RwLock> for thread-safe sharing
    let input_text: SharedText = hooks
        .use_state(|| Arc::new(RwLock::new(String::new())))
        .read()
        .clone();
    let input_text_for_handler = input_text.clone();

    // Version counter to trigger rerenders when input changes
    let mut input_version = hooks.use_state(|| 0usize);

    // Get contacts from props
    let contacts = props.contacts.clone();

    // Check focus and modal state
    let is_invite_modal_visible = invite_modal_state.read().visible;

    let current_resident_index = resident_index.get();

    // Clone callbacks for event handler
    let on_send = props.on_send.clone();
    let on_invite = props.on_invite.clone();
    let on_go_neighborhood = props.on_go_neighborhood.clone();

    let resident_count = residents.len();

    // Throttle for navigation keys - persists across renders using use_ref
    let mut nav_throttle = hooks.use_ref(|| Instant::now() - Duration::from_millis(200));
    let throttle_duration = Duration::from_millis(150);

    hooks.use_terminal_events({
        let input_text = input_text_for_handler;
        let mut resident_index = resident_index.clone();
        let mut focus = focus.clone();
        let mut invite_modal_state = invite_modal_state.clone();
        let contacts_for_modal = contacts.clone();
        move |event| {
            let current_focus = focus.get();
            let is_invite_modal_visible = invite_modal_state.read().visible;

            match event {
                TerminalEvent::Key(KeyEvent {
                    code, modifiers, kind, ..
                }) if kind != KeyEventKind::Release => {
                    if is_invite_modal_visible {
                        // Invite modal key handling
                        match code {
                            KeyCode::Esc => {
                                let mut state = invite_modal_state.read().clone();
                                state.hide();
                                invite_modal_state.set(state);
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                let mut state = invite_modal_state.read().clone();
                                state.select_prev();
                                invite_modal_state.set(state);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                let mut state = invite_modal_state.read().clone();
                                state.select_next();
                                invite_modal_state.set(state);
                            }
                            KeyCode::Enter => {
                                let state = invite_modal_state.read().clone();
                                if state.can_select() {
                                    if let Some(contact_id) = state.get_selected_id() {
                                        if let Some(ref callback) = on_invite {
                                            callback(contact_id);
                                        }
                                    }
                                }
                                let mut state = invite_modal_state.read().clone();
                                state.hide();
                                invite_modal_state.set(state);
                            }
                            _ => {}
                        }
                    } else if current_focus == BlockFocus::Input {
                        // Insert mode: handle Escape, Enter, Backspace, and character input
                        match code {
                            // Escape exits insert mode back to residents
                            KeyCode::Esc => {
                                focus.set(BlockFocus::Residents);
                            }
                            // Shift+Enter adds newline, plain Enter sends message
                            KeyCode::Enter => {
                                if modifiers.contains(KeyModifiers::SHIFT) {
                                    // Shift+Enter: add newline
                                    if let Ok(mut guard) = input_text.write() {
                                        guard.push('\n');
                                    }
                                    input_version.set(input_version.get().wrapping_add(1));
                                } else {
                                    // Plain Enter: send message
                                    if let Ok(text) = input_text.read() {
                                        let text = text.clone();
                                        if !text.is_empty() {
                                            if let Some(ref callback) = on_send {
                                                callback(text);
                                            }
                                            if let Ok(mut guard) = input_text.write() {
                                                guard.clear();
                                            }
                                            input_version.set(input_version.get().wrapping_add(1));
                                        }
                                    }
                                }
                            }
                            // Backspace removes last character
                            KeyCode::Backspace => {
                                if let Ok(mut guard) = input_text.write() {
                                    guard.pop();
                                }
                                input_version.set(input_version.get().wrapping_add(1));
                            }
                            // Character input
                            KeyCode::Char(c) => {
                                if !modifiers.contains(KeyModifiers::CONTROL) {
                                    if let Ok(mut guard) = input_text.write() {
                                        guard.push(c);
                                    }
                                    input_version.set(input_version.get().wrapping_add(1));
                                }
                            }
                            // All other keys ignored in insert mode
                            _ => {}
                        }
                    } else {
                        // Normal mode key handling
                        match code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                let should_move = nav_throttle.read().elapsed() >= throttle_duration;
                                if should_move {
                                    let idx = resident_index.get();
                                    if idx > 0 {
                                        resident_index.set(idx - 1);
                                    }
                                    nav_throttle.set(Instant::now());
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                let should_move = nav_throttle.read().elapsed() >= throttle_duration;
                                if should_move {
                                    let idx = resident_index.get();
                                    if idx + 1 < resident_count {
                                        resident_index.set(idx + 1);
                                    }
                                    nav_throttle.set(Instant::now());
                                }
                            }
                            // 'i' enters insert mode
                            KeyCode::Char('i') => {
                                focus.set(BlockFocus::Input);
                            }
                            // Invite - show modal with contacts
                            KeyCode::Char('v') => {
                                let mut state = invite_modal_state.read().clone();
                                state.show("Invite to Block", contacts_for_modal.clone());
                                invite_modal_state.set(state);
                            }
                            // Go to neighborhood
                            KeyCode::Char('n') => {
                                if let Some(ref callback) = on_go_neighborhood {
                                    callback();
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    });

    // Read input text for display (using version to ensure fresh read)
    let _ = input_version.get(); // Force dependency on version
    let display_input_text = input_text.read().map(|g| g.clone()).unwrap_or_default();

    let current_focus = focus.get();
    let input_focused = current_focus == BlockFocus::Input;
    let modal_state = invite_modal_state.read().clone();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
            flex_grow: 1.0,
            flex_shrink: 1.0,
            overflow: Overflow::Hidden,
        ) {
            // Main content - constrained height with overflow hidden
            View(
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Hidden,
                gap: Spacing::XS,
            ) {
                // Sidebar (30%) - overflow scroll allows internal scrolling
                View(width: 30pct, flex_direction: FlexDirection::Column, flex_shrink: 1.0, overflow: Overflow::Scroll, gap: 0) {
                    ResidentList(residents: residents, selected_index: current_resident_index)
                    StorageBudgetPanel(budget: budget)
                }
                // Messages (70%)
                View(flex_grow: 1.0, overflow: Overflow::Hidden) {
                    BlockMessagesPanel(messages: messages, channel_name: channel_name)
                }
            }

            // Message input (always visible, like Chat screen)
            MessageInput(
                value: display_input_text,
                placeholder: "Type a message...".to_string(),
                focused: input_focused,
                reply_to: None::<String>,
                sending: false,
            )

            // Invite modal overlay (shown when modal is visible)
            ContactSelectModal(
                title: modal_state.title.clone(),
                contacts: modal_state.contacts.clone(),
                selected_index: modal_state.selected_index,
                visible: is_invite_modal_visible,
            )
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
