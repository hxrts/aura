//! # App Screen
//!
//! Main application with screen navigation

use iocraft::prelude::*;
use std::sync::{Arc, RwLock};

use crate::tui::components::{
    AccountSetupModal, DemoInviteCodes, DiscoveredPeerInfo, InvitePeerCallback, KeyHintsBar,
};
use crate::tui::context::IoContext;
use crate::tui::effects::EffectCommand;
use crate::tui::hooks::AppCoreContext;
use crate::tui::screens::block::{BlockInviteCallback, BlockNavCallback, BlockSendCallback};
use crate::tui::screens::chat::{ChannelSelectCallback, CreateChannelCallback, SendCallback};
use crate::tui::screens::contacts::{StartChatCallback, ToggleGuardianCallback, UpdatePetnameCallback};
use crate::tui::screens::invitations::{
    CreateInvitationCallback, ExportInvitationCallback, ImportInvitationCallback,
    InvitationCallback,
};
use crate::tui::screens::neighborhood::{GoHomeCallback, NavigationCallback};
use crate::tui::screens::recovery::RecoveryCallback;
use crate::tui::screens::settings::MfaCallback;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{
    BlockBudget, BlockSummary, Channel, Contact, Device, Guardian, Invitation, InvitationFilter,
    KeyHint, Message, MfaPolicy, RecoveryStatus, Resident, TraversalDepth,
};

use super::router::Screen;
use super::{
    BlockScreen, ChatScreen, ContactsScreen, HelpCommand, HelpScreen, InvitationsScreen,
    NeighborhoodScreen, RecoveryScreen, SettingsScreen,
};

/// Props for ScreenTabBar
#[derive(Default, Props)]
pub struct ScreenTabBarProps {
    pub active: Screen,
}

/// Tab bar for screen navigation
#[component]
pub fn ScreenTabBar(props: &ScreenTabBarProps) -> impl Into<AnyElement<'static>> {
    let active = props.active;

    element! {
        View(
            flex_direction: FlexDirection::Row,
            gap: Spacing::SM,
            padding: Spacing::PANEL_PADDING,
        ) {
            #(Screen::all().iter().map(|&screen| {
                let is_active = screen == active;
                let color = if is_active { Theme::PRIMARY } else { Theme::TEXT_MUTED };
                let weight = if is_active { Weight::Bold } else { Weight::Normal };
                let title = screen.name().to_string();
                element! {
                    Text(content: title, color: color, weight: weight)
                }
            }))
        }
    }
}

/// Props for StatusBar
#[derive(Default, Props)]
pub struct StatusBarProps {
    /// Whether sync is in progress
    pub syncing: bool,
    /// Last sync time (ms since epoch), None if never synced
    pub last_sync_time: Option<u64>,
    /// Number of known peers
    pub peer_count: usize,
}

/// Format a timestamp as relative time (e.g., "2m ago", "1h ago")
fn format_relative_time(ts_ms: u64) -> String {
    // Simple implementation - just show minutes/hours ago
    // TODO: In production, use actual current time comparison
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    if now_ms < ts_ms {
        return "just now".to_string();
    }

    let elapsed_ms = now_ms - ts_ms;
    let elapsed_secs = elapsed_ms / 1000;

    if elapsed_secs < 60 {
        "just now".to_string()
    } else if elapsed_secs < 3600 {
        format!("{}m ago", elapsed_secs / 60)
    } else if elapsed_secs < 86400 {
        format!("{}h ago", elapsed_secs / 3600)
    } else {
        format!("{}d ago", elapsed_secs / 86400)
    }
}

/// Status bar showing sync status and peer count
#[component]
pub fn StatusBar(props: &StatusBarProps) -> impl Into<AnyElement<'static>> {
    let sync_status = if props.syncing {
        "Syncing...".to_string()
    } else if let Some(ts) = props.last_sync_time {
        format!("Synced {}", format_relative_time(ts))
    } else {
        "Not synced".to_string()
    };

    let sync_color = if props.syncing {
        Theme::WARNING
    } else if props.last_sync_time.is_some() {
        Theme::SUCCESS
    } else {
        Theme::TEXT_MUTED
    };

    let peer_status = format!("{} peers", props.peer_count);

    element! {
        View(
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::End,
            gap: Spacing::MD,
            padding_left: Spacing::SM,
            padding_right: Spacing::SM,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
        ) {
            Text(content: sync_status, color: sync_color)
            Text(content: " | ", color: Theme::BORDER)
            Text(content: peer_status, color: Theme::TEXT_MUTED)
        }
    }
}

/// Props for IoApp
#[derive(Default, Props)]
pub struct IoAppProps {
    // Screen data - populated from IoContext via reactive views
    pub channels: Vec<Channel>,
    pub messages: Vec<Message>,
    pub help_commands: Vec<HelpCommand>,
    pub invitations: Vec<Invitation>,
    pub guardians: Vec<Guardian>,
    pub devices: Vec<Device>,
    pub display_name: String,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub mfa_policy: MfaPolicy,
    pub recovery_status: RecoveryStatus,
    // Block screen data
    pub block_name: String,
    pub residents: Vec<Resident>,
    pub block_budget: BlockBudget,
    pub channel_name: String,
    // Contacts screen data
    pub contacts: Vec<Contact>,
    /// Discovered LAN peers
    pub discovered_peers: Vec<DiscoveredPeerInfo>,
    // Neighborhood screen data
    pub neighborhood_name: String,
    pub blocks: Vec<BlockSummary>,
    pub traversal_depth: TraversalDepth,
    // Effect dispatch callback for sending messages
    pub on_send: Option<SendCallback>,
    // Effect dispatch callback for channel selection
    pub on_channel_select: Option<ChannelSelectCallback>,
    // Effect dispatch callback for creating new channels
    pub on_create_channel: Option<CreateChannelCallback>,
    // Effect dispatch callbacks for invitation actions
    pub on_accept_invitation: Option<InvitationCallback>,
    pub on_decline_invitation: Option<InvitationCallback>,
    pub on_create_invitation: Option<CreateInvitationCallback>,
    pub on_export_invitation: Option<ExportInvitationCallback>,
    pub on_import_invitation: Option<ImportInvitationCallback>,
    // Effect dispatch callbacks for neighborhood navigation
    pub on_enter_block: Option<NavigationCallback>,
    pub on_go_home: Option<GoHomeCallback>,
    pub on_back_to_street: Option<GoHomeCallback>,
    // Effect dispatch callbacks for recovery actions
    pub on_start_recovery: Option<RecoveryCallback>,
    pub on_add_guardian: Option<RecoveryCallback>,
    // Effect dispatch callback for settings
    pub on_update_mfa: Option<MfaCallback>,
    // Effect dispatch callbacks for contacts actions
    pub on_update_petname: Option<UpdatePetnameCallback>,
    pub on_toggle_guardian: Option<ToggleGuardianCallback>,
    pub on_start_chat: Option<StartChatCallback>,
    pub on_invite_lan_peer: Option<InvitePeerCallback>,
    // Effect dispatch callbacks for block actions
    pub on_block_send: Option<BlockSendCallback>,
    pub on_block_invite: Option<BlockInviteCallback>,
    pub on_block_navigate_neighborhood: Option<BlockNavCallback>,
    // Account setup
    /// Whether to show account setup modal on start
    pub show_account_setup: bool,
    /// Callback for account creation
    pub on_create_account: Option<CreateAccountCallback>,
    // Sync status
    /// Whether sync is in progress
    pub sync_in_progress: bool,
    /// Last sync time (ms since epoch)
    pub last_sync_time: Option<u64>,
    /// Number of known peers
    pub peer_count: usize,
    // Demo mode
    /// Whether running in demo mode
    pub demo_mode: bool,
    /// Alice's invite code (for demo mode)
    pub demo_alice_code: String,
    /// Charlie's invite code (for demo mode)
    pub demo_charlie_code: String,
}

/// Callback for creating an account
pub type CreateAccountCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Main application with screen navigation
#[component]
pub fn IoApp(props: &IoAppProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let screen = hooks.use_state(|| Screen::Block);
    let should_exit = hooks.use_state(|| false);
    let mut system = hooks.use_context_mut::<SystemContext>();

    // Account setup modal state (using pattern from chat.rs for non-Copy types)
    // - Bool flags use use_state (Copy)
    // - String uses Arc<RwLock<String>> (thread-safe shared state)
    // - Version counter triggers re-renders
    let account_visible = hooks.use_state(|| props.show_account_setup);
    let account_creating = hooks.use_state(|| false);
    // account_error is not stored (errors are transient, shown inline)
    let _account_error: Option<String> = None;
    let account_display_name: Arc<RwLock<String>> = Arc::new(RwLock::new(String::new()));
    let account_display_name_for_handler = account_display_name.clone();
    let account_version = hooks.use_state(|| 0usize);
    let on_create_account = props.on_create_account.clone();

    // Handle exit request
    if should_exit.get() {
        system.exit();
    }

    // Clone props for use
    let channels = props.channels.clone();
    let messages = props.messages.clone();
    let help_commands = props.help_commands.clone();
    let invitations = props.invitations.clone();
    let guardians = props.guardians.clone();
    let devices = props.devices.clone();
    let display_name = props.display_name.clone();
    let threshold_k = props.threshold_k;
    let threshold_n = props.threshold_n;
    let mfa_policy = props.mfa_policy;
    let recovery_status = props.recovery_status.clone();
    // Block screen data
    let block_name = props.block_name.clone();
    let residents = props.residents.clone();
    let block_budget = props.block_budget.clone();
    let channel_name = props.channel_name.clone();
    // Contacts screen data
    let contacts = props.contacts.clone();
    let discovered_peers = props.discovered_peers.clone();
    // Neighborhood screen data
    let neighborhood_name = props.neighborhood_name.clone();
    let blocks = props.blocks.clone();
    let traversal_depth = props.traversal_depth;
    // Effect dispatch callbacks
    let on_send = props.on_send.clone();
    let on_channel_select = props.on_channel_select.clone();
    let on_create_channel = props.on_create_channel.clone();
    let on_accept_invitation = props.on_accept_invitation.clone();
    let on_decline_invitation = props.on_decline_invitation.clone();
    let on_create_invitation = props.on_create_invitation.clone();
    let on_export_invitation = props.on_export_invitation.clone();
    let on_import_invitation = props.on_import_invitation.clone();
    let on_enter_block = props.on_enter_block.clone();
    let on_go_home = props.on_go_home.clone();
    let on_back_to_street = props.on_back_to_street.clone();
    let on_start_recovery = props.on_start_recovery.clone();
    let on_add_guardian = props.on_add_guardian.clone();
    let on_update_mfa = props.on_update_mfa.clone();
    let on_update_petname = props.on_update_petname.clone();
    let on_toggle_guardian = props.on_toggle_guardian.clone();
    let on_start_chat = props.on_start_chat.clone();
    let on_invite_lan_peer = props.on_invite_lan_peer.clone();
    let on_block_send = props.on_block_send.clone();
    let on_block_invite = props.on_block_invite.clone();
    let on_block_navigate_neighborhood = props.on_block_navigate_neighborhood.clone();

    // Demo modal state
    let demo_modal_visible = hooks.use_state(|| false);

    let current_screen = screen.get();

    // Build screen-specific hints based on current screen
    let screen_hints: Vec<KeyHint> = match current_screen {
        Screen::Block => vec![
            KeyHint::new("i", "Insert"),
            KeyHint::new("v", "Invite"),
            KeyHint::new("n", "Neighborhood"),
            KeyHint::new("↑↓", "Navigate"),
        ],
        Screen::Chat => vec![
            KeyHint::new("i", "Insert"),
            KeyHint::new("n", "New channel"),
            KeyHint::new("h/l", "Focus"),
            KeyHint::new("↑↓", "Navigate"),
        ],
        Screen::Contacts => vec![
            KeyHint::new("e", "Edit name"),
            KeyHint::new("g", "Guardian"),
            KeyHint::new("c", "Chat"),
            KeyHint::new("i", "Invite"),
        ],
        Screen::Neighborhood => vec![
            KeyHint::new("Enter", "Enter"),
            KeyHint::new("g", "Home"),
            KeyHint::new("↑↓←→", "Navigate"),
        ],
        Screen::Invitations => vec![
            KeyHint::new("n", "New"),
            KeyHint::new("i", "Import"),
            KeyHint::new("e", "Export"),
            KeyHint::new("f", "Filter"),
        ],
        Screen::Settings => vec![
            KeyHint::new("↑↓", "Section"),
            KeyHint::new("h/l", "Panel"),
            KeyHint::new("Space", "Toggle"),
        ],
        Screen::Recovery => vec![
            KeyHint::new("a", "Add guardian"),
            KeyHint::new("s", "Start recovery"),
            KeyHint::new("h/l", "Tab"),
        ],
        Screen::Help => vec![
            KeyHint::new("↑↓", "Navigate"),
        ],
    };

    hooks.use_terminal_events({
        let mut screen = screen.clone();
        let mut should_exit = should_exit.clone();
        let mut account_visible = account_visible.clone();
        let mut account_creating = account_creating.clone();
        let mut account_version = account_version.clone();
        let account_display_name = account_display_name_for_handler.clone();
        let on_create_account = on_create_account.clone();
        let mut demo_modal_visible = demo_modal_visible.clone();
        let is_demo_mode = props.demo_mode;
        move |event| match event {
            TerminalEvent::Key(KeyEvent {
                code, modifiers, ..
            }) => {
                // Handle demo modal (close on Esc or 'd')
                if demo_modal_visible.get() {
                    match code {
                        KeyCode::Esc | KeyCode::Char('d') => {
                            demo_modal_visible.set(false);
                        }
                        _ => {}
                    }
                    return;
                }

                // Handle account setup modal input first (captures all input when visible)
                if account_visible.get() {
                    match code {
                        KeyCode::Char(c) => {
                            if let Ok(mut guard) = account_display_name.write() {
                                guard.push(c);
                            }
                            account_version.set(account_version.get().wrapping_add(1));
                        }
                        KeyCode::Backspace => {
                            if let Ok(mut guard) = account_display_name.write() {
                                guard.pop();
                            }
                            account_version.set(account_version.get().wrapping_add(1));
                        }
                        KeyCode::Enter => {
                            // Check if we can submit (name not empty and not creating)
                            let can_submit = account_display_name
                                .read()
                                .map(|n| !n.is_empty())
                                .unwrap_or(false)
                                && !account_creating.get();

                            if can_submit {
                                // Trigger account creation callback
                                if let Some(ref callback) = on_create_account {
                                    let name = account_display_name
                                        .read()
                                        .map(|n| n.clone())
                                        .unwrap_or_default();
                                    callback(name);
                                }
                                // Mark as creating
                                account_creating.set(true);
                                account_version.set(account_version.get().wrapping_add(1));
                            }
                        }
                        KeyCode::Esc => {
                            // Allow canceling the modal
                            account_visible.set(false);
                            account_version.set(account_version.get().wrapping_add(1));
                        }
                        _ => {}
                    }
                    return; // Don't process other keys when modal is visible
                }

                // Normal screen navigation
                match code {
                    KeyCode::Char('1') => screen.set(Screen::Block),
                    KeyCode::Char('2') => screen.set(Screen::Chat),
                    KeyCode::Char('3') => screen.set(Screen::Contacts),
                    KeyCode::Char('4') => screen.set(Screen::Neighborhood),
                    KeyCode::Char('5') => screen.set(Screen::Invitations),
                    KeyCode::Char('6') => screen.set(Screen::Settings),
                    KeyCode::Char('7') => screen.set(Screen::Recovery),
                    KeyCode::Char('8') => screen.set(Screen::Help),
                    KeyCode::Tab => {
                        if modifiers.contains(KeyModifiers::SHIFT) {
                            screen.set(screen.get().prev());
                        } else {
                            screen.set(screen.get().next());
                        }
                    }
                    KeyCode::Char('d') if is_demo_mode => demo_modal_visible.set(true),
                    KeyCode::Char('q') => should_exit.set(true),
                    _ => {}
                }
            }
            _ => {}
        }
    });

    // Extract account setup state for rendering
    let modal_visible = account_visible.get();
    let modal_creating = account_creating.get();
    let modal_display_name = account_display_name
        .read()
        .map(|s| s.clone())
        .unwrap_or_default();
    // account_error is always None for now (errors are transient, not stored)
    let modal_error = String::new();
    // account_version is used for triggering re-renders (not directly in UI)
    let _ = account_version.get();

    // Extract sync status from props
    let syncing = props.sync_in_progress;
    let last_sync = props.last_sync_time;
    let peers = props.peer_count;

    // Extract demo mode props
    let alice_code = props.demo_alice_code.clone();
    let charlie_code = props.demo_charlie_code.clone();
    let show_demo_modal = demo_modal_visible.get();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
        ) {
            // Screen tab bar
            ScreenTabBar(active: current_screen)

            // Status bar showing sync status
            StatusBar(syncing: syncing, last_sync_time: last_sync, peer_count: peers)

            // Screen content - flex_grow fills available space, overflow clips to make room for hints
            View(flex_grow: 1.0, flex_shrink: 1.0, overflow: Overflow::Hidden) {
                #(match current_screen {
                    Screen::Block => vec![element! {
                        View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                            BlockScreen(
                                block_name: block_name.clone(),
                                residents: residents.clone(),
                                messages: messages.clone(),
                                budget: block_budget.clone(),
                                channel_name: channel_name.clone(),
                                contacts: contacts.clone(),
                                on_send: on_block_send.clone(),
                                on_invite: on_block_invite.clone(),
                                on_go_neighborhood: on_block_navigate_neighborhood.clone(),
                            )
                        }
                    }],
                    Screen::Chat => {
                        let idx: usize = 0;
                        vec![element! {
                            View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                                ChatScreen(
                                    channels: channels.clone(),
                                    messages: messages.clone(),
                                    initial_channel_index: idx,
                                    on_send: on_send.clone(),
                                    on_channel_select: on_channel_select.clone(),
                                    on_create_channel: on_create_channel.clone(),
                                )
                            }
                        }]
                    }
                    Screen::Contacts => vec![element! {
                        View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                            ContactsScreen(
                                contacts: contacts.clone(),
                                discovered_peers: discovered_peers.clone(),
                                on_update_petname: on_update_petname.clone(),
                                on_toggle_guardian: on_toggle_guardian.clone(),
                                on_start_chat: on_start_chat.clone(),
                                on_invite_lan_peer: on_invite_lan_peer.clone(),
                            )
                        }
                    }],
                    Screen::Neighborhood => vec![element! {
                        View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                            NeighborhoodScreen(
                                neighborhood_name: neighborhood_name.clone(),
                                blocks: blocks.clone(),
                                depth: traversal_depth,
                                on_enter_block: on_enter_block.clone(),
                                on_go_home: on_go_home.clone(),
                                on_back_to_street: on_back_to_street.clone(),
                            )
                        }
                    }],
                    Screen::Invitations => vec![element! {
                        View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                            InvitationsScreen(
                                invitations: invitations.clone(),
                                filter: InvitationFilter::All,
                                selected_index: 0usize,
                                on_accept: on_accept_invitation.clone(),
                                on_decline: on_decline_invitation.clone(),
                                on_create: on_create_invitation.clone(),
                                on_export: on_export_invitation.clone(),
                                on_import: on_import_invitation.clone(),
                                demo_mode: !alice_code.is_empty(),
                                demo_alice_code: alice_code.clone(),
                                demo_charlie_code: charlie_code.clone(),
                            )
                        }
                    }],
                    Screen::Settings => vec![element! {
                        View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                            SettingsScreen(
                                display_name: display_name.clone(),
                                threshold_k: threshold_k,
                                threshold_n: threshold_n,
                                contact_count: contacts.len(),
                                devices: devices.clone(),
                                mfa_policy: mfa_policy,
                                on_update_mfa: on_update_mfa.clone(),
                            )
                        }
                    }],
                    Screen::Recovery => vec![element! {
                        View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                            RecoveryScreen(
                                guardians: guardians.clone(),
                                threshold_required: threshold_k as u32,
                                threshold_total: threshold_n as u32,
                                recovery_status: recovery_status.clone(),
                                on_start_recovery: on_start_recovery.clone(),
                                on_add_guardian: on_add_guardian.clone(),
                            )
                        }
                    }],
                    Screen::Help => vec![element! {
                        View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                            HelpScreen(commands: help_commands.clone())
                        }
                    }],
                })
            }

            // Key hints bar with screen-specific and global navigation hints
            KeyHintsBar(screen_hints: screen_hints, demo_mode: props.demo_mode)

            // Demo mode invite codes modal (triggered by 'd' key)
            DemoInviteCodes(
                alice_code: alice_code,
                charlie_code: charlie_code,
                visible: show_demo_modal,
            )

            // Account setup modal overlay
            AccountSetupModal(
                visible: modal_visible,
                display_name: modal_display_name,
                focused: true,
                creating: modal_creating,
                error: modal_error,
            )
        }
    }
}

/// Run the application with IoContext (real data)
///
/// This version uses the IoContext to fetch actual data from the reactive
/// views instead of mock data.
pub async fn run_app_with_context(ctx: IoContext) -> std::io::Result<()> {
    // Create effect dispatch callbacks
    let ctx_arc = Arc::new(ctx);

    // SendCallback for ChatScreen - fires async dispatch in background
    let ctx_for_send = ctx_arc.clone();
    let on_send: SendCallback = Arc::new(move |channel_id: String, content: String| {
        let ctx = ctx_for_send.clone();
        let cmd = EffectCommand::SendMessage {
            channel: channel_id,
            content,
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to send message: {}", e);
            }
        });
    });

    // ChannelSelectCallback for selecting a channel (triggers real-time message updates)
    let app_core_for_select = ctx_arc.app_core().clone();
    let on_channel_select: ChannelSelectCallback = Arc::new(move |channel_id: String| {
        // Use try_read since we're in a sync callback
        // This is safe because select_channel uses lock_mut() internally
        if let Ok(core) = app_core_for_select.try_read() {
            core.views().select_channel(Some(channel_id));
        }
    });

    // CreateChannelCallback for creating new chat channels
    let ctx_for_create_channel = ctx_arc.clone();
    let on_create_channel: CreateChannelCallback =
        Arc::new(move |name: String, topic: Option<String>| {
            let ctx = ctx_for_create_channel.clone();
            // Start with empty members (creator is automatically included)
            let cmd = EffectCommand::CreateChannel {
                name,
                topic,
                members: vec![],
            };
            tokio::spawn(async move {
                if let Err(e) = ctx.dispatch(cmd).await {
                    eprintln!("Failed to create channel: {}", e);
                }
            });
        });

    // InvitationCallback for accepting invitations
    let ctx_for_accept = ctx_arc.clone();
    let on_accept_invitation: InvitationCallback = Arc::new(move |invitation_id: String| {
        let ctx = ctx_for_accept.clone();
        let cmd = EffectCommand::AcceptInvitation { invitation_id };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to accept invitation: {}", e);
            }
        });
    });

    // InvitationCallback for declining invitations
    let ctx_for_decline = ctx_arc.clone();
    let on_decline_invitation: InvitationCallback = Arc::new(move |invitation_id: String| {
        let ctx = ctx_for_decline.clone();
        let cmd = EffectCommand::DeclineInvitation { invitation_id };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to decline invitation: {}", e);
            }
        });
    });

    // CreateInvitationCallback for creating new invitations
    let ctx_for_create = ctx_arc.clone();
    let on_create_invitation: CreateInvitationCallback =
        Arc::new(move |invitation_type: String, message: Option<String>, ttl_secs: Option<u64>| {
            let ctx = ctx_for_create.clone();
            let cmd = EffectCommand::CreateInvitation {
                invitation_type,
                message,
                ttl_secs,
            };
            tokio::spawn(async move {
                if let Err(e) = ctx.dispatch(cmd).await {
                    eprintln!("Failed to create invitation: {}", e);
                }
            });
        });

    // ExportInvitationCallback for exporting invitation codes
    let ctx_for_export = ctx_arc.clone();
    let on_export_invitation: ExportInvitationCallback =
        Arc::new(move |invitation_id: String| {
            let ctx = ctx_for_export.clone();
            let cmd = EffectCommand::ExportInvitation { invitation_id };
            tokio::spawn(async move {
                if let Err(e) = ctx.dispatch(cmd).await {
                    eprintln!("Failed to export invitation: {}", e);
                }
            });
        });

    // ImportInvitationCallback for importing invitation codes
    let ctx_for_import = ctx_arc.clone();
    let on_import_invitation: ImportInvitationCallback = Arc::new(move |code: String| {
        let ctx = ctx_for_import.clone();
        let cmd = EffectCommand::ImportInvitation { code };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to import invitation: {}", e);
            }
        });
    });

    // NavigationCallback for entering a block
    let ctx_for_enter = ctx_arc.clone();
    let on_enter_block: NavigationCallback =
        Arc::new(move |block_id: String, depth: TraversalDepth| {
            let ctx = ctx_for_enter.clone();
            let depth_str = match depth {
                TraversalDepth::Street => "Street",
                TraversalDepth::Frontage => "Frontage",
                TraversalDepth::Interior => "Interior",
            }
            .to_string();
            let cmd = EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(), // Uses current neighborhood
                block_id,
                depth: depth_str,
            };
            tokio::spawn(async move {
                if let Err(e) = ctx.dispatch(cmd).await {
                    eprintln!("Failed to enter block: {}", e);
                }
            });
        });

    // GoHomeCallback for navigating to home block
    let ctx_for_home = ctx_arc.clone();
    let on_go_home: GoHomeCallback = Arc::new(move || {
        let ctx = ctx_for_home.clone();
        let cmd = EffectCommand::MovePosition {
            neighborhood_id: "current".to_string(),
            block_id: "home".to_string(), // Special block_id to indicate home
            depth: "Interior".to_string(),
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to go home: {}", e);
            }
        });
    });

    // GoHomeCallback for returning to street view
    let ctx_for_street = ctx_arc.clone();
    let on_back_to_street: GoHomeCallback = Arc::new(move || {
        let ctx = ctx_for_street.clone();
        let cmd = EffectCommand::MovePosition {
            neighborhood_id: "current".to_string(),
            block_id: "current".to_string(), // Stay on current block
            depth: "Street".to_string(),     // But change to street view
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to return to street: {}", e);
            }
        });
    });

    // RecoveryCallback for starting recovery
    let ctx_for_recovery = ctx_arc.clone();
    let on_start_recovery: RecoveryCallback = Arc::new(move || {
        let ctx = ctx_for_recovery.clone();
        let cmd = EffectCommand::StartRecovery;
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to start recovery: {}", e);
            }
        });
    });

    // RecoveryCallback for adding a guardian - dispatches InviteGuardian command
    let ctx_for_guardian = ctx_arc.clone();
    let on_add_guardian: RecoveryCallback = Arc::new(move || {
        let ctx = ctx_for_guardian.clone();
        let cmd = EffectCommand::InviteGuardian { contact_id: None };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to invite guardian: {}", e);
            }
        });
    });

    // MfaCallback for updating MFA policy
    let ctx_for_mfa = ctx_arc.clone();
    let on_update_mfa: MfaCallback = Arc::new(move |require_mfa: bool| {
        let ctx = ctx_for_mfa.clone();
        let cmd = EffectCommand::UpdateMfaPolicy { require_mfa };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to update MFA policy: {}", e);
            }
        });
    });

    // UpdatePetnameCallback for updating a contact's petname - dispatches UpdateContactPetname command
    let ctx_for_update_petname = ctx_arc.clone();
    let on_update_petname: UpdatePetnameCallback =
        Arc::new(move |contact_id: String, new_petname: String| {
            let ctx = ctx_for_update_petname.clone();
            let cmd = EffectCommand::UpdateContactPetname {
                contact_id,
                petname: new_petname,
            };
            tokio::spawn(async move {
                if let Err(e) = ctx.dispatch(cmd).await {
                    eprintln!("Failed to update petname: {}", e);
                }
            });
        });

    // ToggleGuardianCallback for toggling guardian status - dispatches ToggleContactGuardian command
    let ctx_for_toggle = ctx_arc.clone();
    let on_toggle_guardian: ToggleGuardianCallback = Arc::new(move |contact_id: String| {
        let ctx = ctx_for_toggle.clone();
        let cmd = EffectCommand::ToggleContactGuardian { contact_id };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to toggle guardian status: {}", e);
            }
        });
    });

    // StartChatCallback for starting a direct chat with a contact
    let ctx_for_start_chat = ctx_arc.clone();
    let on_start_chat: StartChatCallback = Arc::new(move |contact_id: String| {
        let ctx = ctx_for_start_chat.clone();
        let cmd = EffectCommand::StartDirectChat { contact_id };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to start direct chat: {}", e);
            }
        });
    });

    // InvitePeerCallback for inviting a discovered LAN peer
    let ctx_for_invite_peer = ctx_arc.clone();
    let on_invite_lan_peer: InvitePeerCallback =
        Arc::new(move |authority_id: String, address: String| {
            let ctx = ctx_for_invite_peer.clone();
            let cmd = EffectCommand::InviteLanPeer {
                authority_id,
                address,
            };
            tokio::spawn(async move {
                if let Err(e) = ctx.dispatch(cmd).await {
                    eprintln!("Failed to invite LAN peer: {}", e);
                }
            });
        });

    // BlockSendCallback for sending a message in the block channel
    // Note: Full implementation requires compose modal/input focus UI
    // The SendMessage command is ready for use when UI provides the message content
    let on_block_send: BlockSendCallback = Arc::new(|content: String| {
        // Log the action - compose modal implementation needed
        // When modal is implemented: dispatch SendMessage { channel, content }
        tracing::info!(
            "Block send triggered with content '{}' - awaiting compose UI",
            content
        );
    });

    // BlockInviteCallback for inviting a contact to the block
    let ctx_for_block_invite = ctx_arc.clone();
    let on_block_invite: BlockInviteCallback = Arc::new(move |contact_id: String| {
        let ctx = ctx_for_block_invite.clone();
        let cmd = EffectCommand::SendBlockInvitation {
            contact_id: contact_id.clone(),
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to send block invitation: {}", e);
            }
        });
        tracing::info!("Block invite sent to contact: {}", contact_id);
    });

    // BlockNavCallback for navigating to neighborhood view - dispatches MovePosition to Street depth
    let ctx_for_nav = ctx_arc.clone();
    let on_block_navigate_neighborhood: BlockNavCallback = Arc::new(move || {
        let ctx = ctx_for_nav.clone();
        let cmd = EffectCommand::MovePosition {
            neighborhood_id: "current".to_string(),
            block_id: "current".to_string(),
            depth: "Street".to_string(),
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to navigate to neighborhood: {}", e);
            }
        });
    });

    // CreateAccountCallback for account creation from setup modal
    let ctx_for_create = ctx_arc.clone();
    let on_create_account: CreateAccountCallback = Arc::new(move |display_name: String| {
        let ctx = ctx_for_create.clone();
        let cmd = EffectCommand::CreateAccount { display_name };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to create account: {}", e);
            }
        });
    });

    // Check if account already exists to determine if we show setup modal
    let show_account_setup = !ctx_arc.has_account();

    // Get data from IoContext
    let channels = ctx_arc.get_channels();
    let messages = ctx_arc.get_messages();
    let guardians = ctx_arc.get_guardians();
    let recovery_status = ctx_arc.get_recovery_status();
    let invitations = ctx_arc.get_invitations();
    let contacts = ctx_arc.get_contacts();
    let residents = ctx_arc.get_residents();
    let block_budget = ctx_arc.get_block_budget();

    // Get discovered LAN peers from context (populated by LAN discovery events)
    // For now, starts empty and will be populated via LanPeersUpdated events
    let discovered_peers: Vec<DiscoveredPeerInfo> = Vec::new();

    // Get block info from snapshot
    let block_snap = ctx_arc.snapshot_block();
    let block_name = block_snap
        .block
        .as_ref()
        .and_then(|b| b.name.clone())
        .unwrap_or_else(|| "My Block".to_string());
    let channel_name = ctx_arc
        .get_selected_channel()
        .unwrap_or_else(|| "general".to_string());

    // Get neighborhood info from snapshot
    let neighborhood_snap = ctx_arc.snapshot_neighborhood();
    let neighborhood_name = neighborhood_snap
        .neighborhood_name
        .clone()
        .unwrap_or_else(|| "Neighborhood".to_string());
    let blocks: Vec<BlockSummary> = neighborhood_snap
        .blocks
        .iter()
        .map(|b| {
            let name = b.name.clone().unwrap_or_else(|| b.id.clone());
            BlockSummary::new(&b.id)
                .with_name(&name)
                .with_residents(b.resident_count)
        })
        .collect();

    // Static data (help commands, devices) - these could come from context later
    let help_commands = vec![
        HelpCommand::new("/join", "Join a channel", "Join #channel-name", "Channels"),
        HelpCommand::new(
            "/leave",
            "Leave current channel",
            "Leave the current channel",
            "Channels",
        ),
        HelpCommand::new(
            "/msg",
            "Send direct message",
            "Send a DM to user",
            "Messaging",
        ),
        HelpCommand::new(
            "/invite",
            "Invite to channel",
            "Invite user to channel",
            "Invitations",
        ),
        HelpCommand::new(
            "/help",
            "Show this help",
            "Display help information",
            "General",
        ),
    ];

    // Device list: Currently hardcoded as placeholder
    // Future: Retrieve from TreeEffects::get_current_state() via commitment tree LeafNodes
    // See docs/001_system_architecture.md for device information derived from LeafNode
    let devices = vec![Device::new("d1", "Current Device").current()];

    // Get threshold info from recovery status
    let threshold_k = recovery_status.threshold as u8;
    let threshold_n = guardians.len().max(recovery_status.threshold as usize) as u8;

    // Get sync status for status bar display
    let sync_in_progress = ctx_arc.is_syncing().await;
    let last_sync_time = ctx_arc.last_sync_time().await;
    let peer_count = ctx_arc.known_peers_count().await;

    // Create AppCoreContext for components to access AppCore and signals
    // AppCore is always available (demo mode uses agent-less AppCore)
    let app_core_context = AppCoreContext::new(ctx_arc.app_core().clone(), ctx_arc.clone());

    // Wrap the app in ContextProvider
    // This enables components to use `hooks.use_context::<AppCoreContext>()` for
    // reactive signal subscription via `use_future`
    {
        let context = app_core_context;
        element! {
            ContextProvider(value: Context::owned(context)) {
                IoApp(
                    channels: channels,
                    messages: messages,
                    help_commands: help_commands,
                    invitations: invitations,
                    guardians: guardians,
                    devices: devices,
                    display_name: "You".to_string(),
                    threshold_k: threshold_k,
                    threshold_n: threshold_n,
                    mfa_policy: MfaPolicy::SensitiveOnly,
                    recovery_status: recovery_status,
                    // Block screen data
                    block_name: block_name,
                    residents: residents,
                    block_budget: block_budget,
                    channel_name: channel_name,
                    // Contacts screen data
                    contacts: contacts,
                    discovered_peers: discovered_peers,
                    // Neighborhood screen data
                    neighborhood_name: neighborhood_name,
                    blocks: blocks,
                    traversal_depth: TraversalDepth::Street,
                    // Effect dispatch callbacks
                    on_send: Some(on_send),
                    on_channel_select: Some(on_channel_select.clone()),
                    on_create_channel: Some(on_create_channel),
                    on_accept_invitation: Some(on_accept_invitation),
                    on_decline_invitation: Some(on_decline_invitation),
                    on_create_invitation: Some(on_create_invitation),
                    on_export_invitation: Some(on_export_invitation),
                    on_import_invitation: Some(on_import_invitation),
                    on_enter_block: Some(on_enter_block),
                    on_go_home: Some(on_go_home),
                    on_back_to_street: Some(on_back_to_street),
                    // Recovery callbacks
                    on_start_recovery: Some(on_start_recovery),
                    on_add_guardian: Some(on_add_guardian),
                    // Settings callbacks
                    on_update_mfa: Some(on_update_mfa),
                    // Contacts callbacks
                    on_update_petname: Some(on_update_petname),
                    on_toggle_guardian: Some(on_toggle_guardian),
                    on_start_chat: Some(on_start_chat),
                    on_invite_lan_peer: Some(on_invite_lan_peer),
                    // Block callbacks
                    on_block_send: Some(on_block_send),
                    on_block_invite: Some(on_block_invite),
                    on_block_navigate_neighborhood: Some(on_block_navigate_neighborhood),
                    // Account setup
                    show_account_setup: show_account_setup,
                    on_create_account: Some(on_create_account.clone()),
                    // Sync status
                    sync_in_progress: sync_in_progress,
                    last_sync_time: last_sync_time,
                    peer_count: peer_count,
                    // Demo mode (get from context)
                    demo_mode: ctx_arc.is_demo_mode(),
                    demo_alice_code: ctx_arc.demo_alice_code(),
                    demo_charlie_code: ctx_arc.demo_charlie_code(),
                )
            }
        }
        .fullscreen()
        .await
    }
}
