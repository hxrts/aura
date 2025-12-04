//! # App Screen
//!
//! Main application with screen navigation

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::KeyHintsBar;
use crate::tui::context::IoContext;
use crate::tui::effects::EffectCommand;
use crate::tui::screens::block::{BlockInviteCallback, BlockNavCallback, BlockSendCallback};
use crate::tui::screens::chat::SendCallback;
use crate::tui::screens::contacts::{EditPetnameCallback, ToggleGuardianCallback};
use crate::tui::screens::invitations::InvitationCallback;
use crate::tui::screens::neighborhood::{GoHomeCallback, NavigationCallback};
use crate::tui::screens::recovery::RecoveryCallback;
use crate::tui::screens::settings::MfaCallback;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{
    BlockBudget, BlockSummary, Channel, Contact, ContactStatus, Device, Guardian, GuardianStatus,
    Invitation, InvitationDirection, InvitationFilter, InvitationStatus, KeyHint, Message,
    MfaPolicy, RecoveryState, RecoveryStatus, Resident, TraversalDepth,
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
            border_style: BorderStyle::Single,
            border_edges: Edges::Bottom,
            border_color: Theme::BORDER,
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

/// Props for IoApp
#[derive(Default, Props)]
pub struct IoAppProps {
    // Sample data for all screens
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
    // Neighborhood screen data
    pub neighborhood_name: String,
    pub blocks: Vec<BlockSummary>,
    pub traversal_depth: TraversalDepth,
    // Effect dispatch callback for sending messages
    pub on_send: Option<SendCallback>,
    // Effect dispatch callbacks for invitation actions
    pub on_accept_invitation: Option<InvitationCallback>,
    pub on_decline_invitation: Option<InvitationCallback>,
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
    pub on_edit_petname: Option<EditPetnameCallback>,
    pub on_toggle_guardian: Option<ToggleGuardianCallback>,
    // Effect dispatch callbacks for block actions
    pub on_block_send: Option<BlockSendCallback>,
    pub on_block_invite: Option<BlockInviteCallback>,
    pub on_block_navigate_neighborhood: Option<BlockNavCallback>,
}

/// Main application with screen navigation
#[component]
pub fn IoApp(props: &IoAppProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let screen = hooks.use_state(|| Screen::Block);

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
    // Neighborhood screen data
    let neighborhood_name = props.neighborhood_name.clone();
    let blocks = props.blocks.clone();
    let traversal_depth = props.traversal_depth;
    // Effect dispatch callbacks
    let on_send = props.on_send.clone();
    let on_accept_invitation = props.on_accept_invitation.clone();
    let on_decline_invitation = props.on_decline_invitation.clone();
    let on_enter_block = props.on_enter_block.clone();
    let on_go_home = props.on_go_home.clone();
    let on_back_to_street = props.on_back_to_street.clone();
    let on_start_recovery = props.on_start_recovery.clone();
    let on_add_guardian = props.on_add_guardian.clone();
    let on_update_mfa = props.on_update_mfa.clone();
    let on_edit_petname = props.on_edit_petname.clone();
    let on_toggle_guardian = props.on_toggle_guardian.clone();
    let on_block_send = props.on_block_send.clone();
    let on_block_invite = props.on_block_invite.clone();
    let on_block_navigate_neighborhood = props.on_block_navigate_neighborhood.clone();

    let hints = vec![
        KeyHint::new("1-8", "Switch screen"),
        KeyHint::new("Tab", "Next screen"),
        KeyHint::new("S-Tab", "Prev screen"),
        KeyHint::new("q", "Quit"),
    ];

    let current_screen = screen.get();

    hooks.use_terminal_events({
        let mut screen = screen.clone();
        move |event| match event {
            TerminalEvent::Key(KeyEvent {
                code, modifiers, ..
            }) => match code {
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
                KeyCode::Char('q') => {
                    // Would use system context to exit
                }
                _ => {}
            },
            _ => {}
        }
    });

    // Chat screen data
    let chat_hints = vec![
        KeyHint::new("↑↓", "Navigate"),
        KeyHint::new("Enter", "Send"),
        KeyHint::new("Tab", "Switch panel"),
    ];

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
        ) {
            // Screen tab bar
            ScreenTabBar(active: current_screen)

            // Screen content
            View(flex_grow: 1.0) {
                #(match current_screen {
                    Screen::Block => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            BlockScreen(
                                block_name: block_name.clone(),
                                residents: residents.clone(),
                                messages: messages.clone(),
                                budget: block_budget.clone(),
                                channel_name: channel_name.clone(),
                                on_send: on_block_send.clone(),
                                on_invite: on_block_invite.clone(),
                                on_go_neighborhood: on_block_navigate_neighborhood.clone(),
                            )
                        }
                    }],
                    Screen::Chat => {
                        let idx: usize = 0;
                        vec![element! {
                            View(width: 100pct, height: 100pct) {
                                ChatScreen(
                                    channels: channels.clone(),
                                    messages: messages.clone(),
                                    hints: chat_hints.clone(),
                                    initial_channel_index: idx,
                                    on_send: on_send.clone(),
                                )
                            }
                        }]
                    }
                    Screen::Contacts => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            ContactsScreen(
                                contacts: contacts.clone(),
                                on_edit_petname: on_edit_petname.clone(),
                                on_toggle_guardian: on_toggle_guardian.clone(),
                            )
                        }
                    }],
                    Screen::Neighborhood => vec![element! {
                        View(width: 100pct, height: 100pct) {
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
                        View(width: 100pct, height: 100pct) {
                            InvitationsScreen(
                                invitations: invitations.clone(),
                                filter: InvitationFilter::All,
                                selected_index: 0usize,
                                on_accept: on_accept_invitation.clone(),
                                on_decline: on_decline_invitation.clone(),
                            )
                        }
                    }],
                    Screen::Settings => vec![element! {
                        View(width: 100pct, height: 100pct) {
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
                        View(width: 100pct, height: 100pct) {
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
                        View(width: 100pct, height: 100pct) {
                            HelpScreen(commands: help_commands.clone())
                        }
                    }],
                })
            }

            // Global hints
            KeyHintsBar(hints: hints)
        }
    }
}

/// Run the full application with screen navigation
pub async fn run_app() -> std::io::Result<()> {
    // Sample data for all screens
    let channels = vec![
        Channel::new("1", "general").with_unread(3).selected(true),
        Channel::new("2", "random").with_unread(0).selected(false),
        Channel::new("3", "dev").with_unread(1).selected(false),
    ];

    let messages = vec![
        Message::new("1", "Alice", "Hello everyone!")
            .with_timestamp("10:30")
            .own(false),
        Message::new("2", "You", "Hi Alice!")
            .with_timestamp("10:31")
            .own(true),
        Message::new("3", "Bob", "What's up?")
            .with_timestamp("10:32")
            .own(false),
    ];

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

    let invitations = vec![
        Invitation::new("1", "Alice", InvitationDirection::Outbound)
            .with_status(InvitationStatus::Pending),
        Invitation::new("2", "Bob", InvitationDirection::Inbound)
            .with_status(InvitationStatus::Pending),
        Invitation::new("3", "Charlie", InvitationDirection::Outbound)
            .with_status(InvitationStatus::Accepted),
    ];

    let guardians = vec![
        Guardian::new("g1", "Alice")
            .with_status(GuardianStatus::Active)
            .with_share(),
        Guardian::new("g2", "Bob")
            .with_status(GuardianStatus::Active)
            .with_share(),
        Guardian::new("g3", "Charlie").with_status(GuardianStatus::Pending),
    ];

    let devices = vec![
        Device::new("d1", "MacBook Pro").current(),
        Device::new("d2", "iPhone"),
        Device::new("d3", "iPad"),
    ];

    let recovery_status = RecoveryStatus {
        state: RecoveryState::None,
        approvals_received: 0,
        threshold: 2,
        approvals: vec![],
    };

    // Block screen data
    let residents = vec![
        Resident::new("r1", "You").is_current_user().steward(),
        Resident::new("r2", "Alice"),
        Resident::new("r3", "Bob"),
    ];

    let block_budget = BlockBudget {
        total: 10 * 1024 * 1024,
        used: 3 * 1024 * 1024,
        resident_count: 3,
        max_residents: 8,
    };

    // Contacts screen data
    let contacts = vec![
        Contact::new("c1", "Alice")
            .with_status(ContactStatus::Active)
            .guardian(),
        Contact::new("c2", "Bob").with_status(ContactStatus::Active),
        Contact::new("c3", "Charlie")
            .with_status(ContactStatus::Pending)
            .with_suggestion("Charles"),
        Contact::new("c4", "Diana").with_status(ContactStatus::Blocked),
    ];

    // Neighborhood screen data
    let blocks = vec![
        BlockSummary::new("b1")
            .with_name("My Block")
            .with_residents(3)
            .home(),
        BlockSummary::new("b2")
            .with_name("Alice's Block")
            .with_residents(5)
            .accessible(),
        BlockSummary::new("b3")
            .with_name("Bob's Block")
            .with_residents(2)
            .accessible(),
        BlockSummary::new("b4").with_residents(8),
        BlockSummary::new("b5")
            .with_name("Community")
            .with_residents(4)
            .accessible(),
    ];

    element! {
        IoApp(
            channels: channels,
            messages: messages,
            help_commands: help_commands,
            invitations: invitations,
            guardians: guardians,
            devices: devices,
            display_name: "You".to_string(),
            threshold_k: 2u8,
            threshold_n: 3u8,
            mfa_policy: MfaPolicy::SensitiveOnly,
            recovery_status: recovery_status,
            // Block screen data
            block_name: "My Block".to_string(),
            residents: residents,
            block_budget: block_budget,
            channel_name: "general".to_string(),
            // Contacts screen data
            contacts: contacts,
            // Neighborhood screen data
            neighborhood_name: "Downtown".to_string(),
            blocks: blocks,
            traversal_depth: TraversalDepth::Street,
        )
    }
    .fullscreen()
    .await
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

    // RecoveryCallback for adding a guardian (logs for now, would open modal)
    let on_add_guardian: RecoveryCallback = Arc::new(|| {
        // TODO: This should open a modal to invite a guardian
        // For now, just log the action
        tracing::info!("Add guardian action triggered - would open invitation modal");
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

    // EditPetnameCallback for editing a contact's petname (placeholder - would open modal)
    let on_edit_petname: EditPetnameCallback = Arc::new(|contact_id: String| {
        // TODO: This should open a modal to edit the petname
        // For now, just log the action
        tracing::info!("Edit petname action triggered for contact: {}", contact_id);
    });

    // ToggleGuardianCallback for toggling guardian status (placeholder - would dispatch effect)
    let on_toggle_guardian: ToggleGuardianCallback = Arc::new(|contact_id: String| {
        // TODO: This should dispatch an effect to toggle guardian status
        // For now, just log the action
        tracing::info!(
            "Toggle guardian action triggered for contact: {}",
            contact_id
        );
    });

    // BlockSendCallback for sending a message in the block channel (placeholder - would open compose modal)
    let on_block_send: BlockSendCallback = Arc::new(|_content: String| {
        // TODO: This should open a message compose modal or focus input
        // For now, just log the action
        tracing::info!("Block send action triggered - would open compose modal");
    });

    // BlockInviteCallback for inviting someone to the block (placeholder - would open invite modal)
    let on_block_invite: BlockInviteCallback = Arc::new(|| {
        // TODO: This should open an invitation modal
        // For now, just log the action
        tracing::info!("Block invite action triggered - would open invitation modal");
    });

    // BlockNavCallback for navigating to neighborhood view (placeholder - would switch screen)
    let on_block_navigate_neighborhood: BlockNavCallback = Arc::new(|| {
        // TODO: This should switch to the neighborhood screen
        // For now, just log the action
        tracing::info!("Navigate to neighborhood action triggered");
    });

    // Get data from IoContext
    let channels = ctx_arc.get_channels();
    let messages = ctx_arc.get_messages();
    let guardians = ctx_arc.get_guardians();
    let recovery_status = ctx_arc.get_recovery_status();
    let invitations = ctx_arc.get_invitations();
    let contacts = ctx_arc.get_contacts();
    let residents = ctx_arc.get_residents();
    let block_budget = ctx_arc.get_block_budget();

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

    // TODO: Get devices from context when available
    let devices = vec![Device::new("d1", "Current Device").current()];

    // Get threshold info from recovery status
    let threshold_k = recovery_status.threshold as u8;
    let threshold_n = guardians.len().max(recovery_status.threshold as usize) as u8;

    element! {
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
            // Neighborhood screen data
            neighborhood_name: neighborhood_name,
            blocks: blocks,
            traversal_depth: TraversalDepth::Street,
            // Effect dispatch callbacks
            on_send: Some(on_send),
            on_accept_invitation: Some(on_accept_invitation),
            on_decline_invitation: Some(on_decline_invitation),
            on_enter_block: Some(on_enter_block),
            on_go_home: Some(on_go_home),
            on_back_to_street: Some(on_back_to_street),
            // Recovery callbacks
            on_start_recovery: Some(on_start_recovery),
            on_add_guardian: Some(on_add_guardian),
            // Settings callbacks
            on_update_mfa: Some(on_update_mfa),
            // Contacts callbacks
            on_edit_petname: Some(on_edit_petname),
            on_toggle_guardian: Some(on_toggle_guardian),
            // Block callbacks
            on_block_send: Some(on_block_send),
            on_block_invite: Some(on_block_invite),
            on_block_navigate_neighborhood: Some(on_block_navigate_neighborhood),
        )
    }
    .fullscreen()
    .await
}
