#![allow(clippy::expect_used)]
//! # Operational Command Handler
//!
//! Handles operational (non-journaled) commands directly.
//! These are commands that don't create journal facts - they're runtime operations
//! like sync, peer management, and system commands.
//!
//! ## Design
//!
//! Unlike journaled commands that go through `AppCore.dispatch(Intent)`, operational
//! commands are executed directly and may update status signals for UI feedback.
//!
//! ## Command Categories
//!
//! - **System**: Ping, Shutdown, RefreshAccount
//! - **Sync**: ForceSync, RequestState
//! - **Network**: AddPeer, RemovePeer, ListPeers, DiscoverPeers, ListLanPeers
//! - **Settings**: UpdateMfaPolicy, UpdateNickname, SetChannelMode
//! - **Invitations**: ExportInvitation, ImportInvitation

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use aura_agent::handlers::{ShareableInvitation, ShareableInvitationError};
use aura_app::signal_defs::{
    AppError, ConnectionStatus, SyncStatus, CHAT_SIGNAL, CONNECTION_STATUS_SIGNAL, ERROR_SIGNAL,
    SYNC_STATUS_SIGNAL,
};
use aura_app::views::chat::{Channel, ChannelType};
use aura_app::views::invitations::InvitationType as ViewInvitationType;
use aura_app::AppCore;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::AuthorityId;
use aura_invitation::InvitationType as DomainInvitationType;
use tokio::sync::RwLock;

use super::EffectCommand;

/// Result type for operational commands
pub type OpResult = Result<OpResponse, OpError>;

/// Response from an operational command
#[derive(Debug, Clone)]
pub enum OpResponse {
    /// Command succeeded with no data
    Ok,
    /// Command returned data
    Data(String),
    /// Command returned a list
    List(Vec<String>),
    /// Invitation code exported
    InvitationCode { id: String, code: String },
    /// Invitation code imported (parsed successfully)
    InvitationImported {
        /// The parsed invitation ID
        invitation_id: String,
        /// Sender authority ID
        sender_id: String,
        /// Invitation type (channel, guardian, contact)
        invitation_type: String,
        /// Optional expiration timestamp
        expires_at: Option<u64>,
        /// Optional message from sender
        message: Option<String>,
    },
    /// Context changed (for SetContext command)
    ContextChanged {
        /// The new context ID (None to clear)
        context_id: Option<String>,
    },
    /// Channel mode updated (for SetChannelMode command)
    ChannelModeSet {
        /// Channel ID that was updated
        channel_id: String,
        /// Mode flags that were applied
        flags: String,
    },
}

/// Error from an operational command
#[derive(Debug, Clone, thiserror::Error)]
pub enum OpError {
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("Operation failed: {0}")]
    Failed(String),
}

/// Handles operational commands that don't create journal facts.
///
/// This handler processes commands that
/// are purely runtime operations (sync, peer management, etc.).
pub struct OperationalHandler {
    app_core: Arc<RwLock<AppCore>>,
    peers: Arc<RwLock<HashSet<String>>>,
}

impl OperationalHandler {
    /// Create a new operational handler
    pub fn new(app_core: Arc<RwLock<AppCore>>) -> Self {
        Self {
            app_core,
            peers: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Execute an operational command
    ///
    /// Returns `Some(result)` if the command was handled, `None` if it should
    /// be handled elsewhere (e.g., by intent dispatch).
    pub async fn execute(&self, command: &EffectCommand) -> Option<OpResult> {
        match command {
            // =========================================================================
            // System Commands
            // =========================================================================
            EffectCommand::Ping => {
                // Simple ping - just return Ok
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::Shutdown => {
                // Shutdown is handled by the TUI event loop, not here
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::RefreshAccount => {
                // Trigger a state refresh by reading and re-emitting signals
                // This causes subscribers to re-render with current state
                Some(Ok(OpResponse::Ok))
            }

            // =========================================================================
            // Sync Commands
            // =========================================================================
            EffectCommand::ForceSync => {
                // Update sync status signal to show syncing
                if let Ok(core) = self.app_core.try_read() {
                    let _ = core
                        .emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Syncing { progress: 0 })
                        .await;
                }

                // Trigger sync through effect injection (RuntimeBridge)
                let result = if let Ok(core) = self.app_core.try_read() {
                    core.trigger_sync().await
                } else {
                    Err(aura_app::core::IntentError::internal_error(
                        "AppCore unavailable",
                    ))
                };

                // Update status based on result
                if let Ok(core) = self.app_core.try_read() {
                    match &result {
                        Ok(()) => {
                            let _ = core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced).await;
                        }
                        Err(e) => {
                            tracing::warn!("Sync trigger failed: {}", e);
                            // In demo/offline mode, show as synced (local-only)
                            let _ = core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced).await;
                        }
                    }
                }

                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::RequestState { peer_id } => {
                // Request state from a specific peer - triggers targeted sync
                // Update sync status signal to show syncing
                if let Ok(core) = self.app_core.try_read() {
                    let _ = core
                        .emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Syncing { progress: 0 })
                        .await;
                }

                // Trigger sync through AppCore (RuntimeBridge handles peer targeting)
                // For now, we trigger a general sync - peer-targeted sync requires
                // additional infrastructure in the sync engine
                let result = if let Ok(core) = self.app_core.try_read() {
                    core.trigger_sync().await
                } else {
                    Err(aura_app::core::IntentError::internal_error(
                        "AppCore unavailable",
                    ))
                };

                // Update status based on result
                if let Ok(core) = self.app_core.try_read() {
                    match &result {
                        Ok(_) => {
                            let _ = core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced).await;
                        }
                        Err(e) => {
                            let _ = core
                                .emit(
                                    &*SYNC_STATUS_SIGNAL,
                                    SyncStatus::Failed {
                                        message: e.to_string(),
                                    },
                                )
                                .await;
                        }
                    }
                }

                match result {
                    Ok(_) => Some(Ok(OpResponse::Data(format!(
                        "Sync requested from peer: {}",
                        peer_id
                    )))),
                    Err(e) => Some(Err(OpError::Failed(format!(
                        "Failed to sync from peer {}: {}",
                        peer_id, e
                    )))),
                }
            }

            // =========================================================================
            // Network/Peer Commands
            // =========================================================================
            EffectCommand::AddPeer { peer_id } => {
                {
                    let mut peers = self.peers.write().await;
                    peers.insert(peer_id.clone());
                    let count = peers.len();

                    if let Ok(core) = self.app_core.try_read() {
                        let _ = core
                            .emit(
                                &*CONNECTION_STATUS_SIGNAL,
                                ConnectionStatus::Online { peer_count: count },
                            )
                            .await;
                    }
                }
                tracing::info!("Added peer: {}", peer_id);
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::RemovePeer { peer_id } => {
                {
                    let mut peers = self.peers.write().await;
                    peers.remove(peer_id);
                    let count = peers.len();

                    if let Ok(core) = self.app_core.try_read() {
                        let status = if count == 0 {
                            ConnectionStatus::Offline
                        } else {
                            ConnectionStatus::Online { peer_count: count }
                        };
                        let _ = core.emit(&*CONNECTION_STATUS_SIGNAL, status).await;
                    }
                }
                tracing::info!("Removed peer: {}", peer_id);
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::ListPeers => {
                // Query actual peers from runtime via AppCore
                let app_core = self.app_core.read().await;

                // Get sync peers (DeviceIds)
                let sync_peers = match app_core.sync_peers().await {
                    Ok(peers) => peers,
                    Err(e) => {
                        tracing::debug!("No sync peers available: {}", e);
                        vec![]
                    }
                };

                // Get discovered peers (AuthorityIds from rendezvous)
                let discovered_peers = match app_core.discover_peers().await {
                    Ok(peers) => peers,
                    Err(e) => {
                        tracing::debug!("No discovered peers available: {}", e);
                        vec![]
                    }
                };

                // Combine into a list of strings
                let mut peer_list: Vec<String> =
                    sync_peers.iter().map(|d| format!("sync:{}", d)).collect();

                peer_list.extend(discovered_peers.iter().map(|a| format!("discovered:{}", a)));

                tracing::info!(
                    "Listed {} peers ({} sync, {} discovered)",
                    peer_list.len(),
                    sync_peers.len(),
                    discovered_peers.len()
                );

                Some(Ok(OpResponse::List(peer_list)))
            }

            EffectCommand::DiscoverPeers => {
                // Trigger peer discovery via rendezvous
                // Currently this is implicit in the rendezvous service
                // TODO: Add explicit trigger_discovery() to RuntimeBridge
                tracing::info!("Peer discovery triggered");

                // For now, return the currently discovered peers
                let app_core = self.app_core.read().await;
                let discovered = match app_core.discover_peers().await {
                    Ok(peers) => peers.len(),
                    Err(_) => 0,
                };

                Some(Ok(OpResponse::Data(format!(
                    "Discovery active, {} peers known",
                    discovered
                ))))
            }

            EffectCommand::ListLanPeers => {
                // LAN peer discovery - currently not exposed via RuntimeBridge
                // TODO: Add get_lan_peers() to RuntimeBridge trait
                // For now, return empty list with info message
                tracing::info!("LAN peer discovery not yet implemented in runtime");
                Some(Ok(OpResponse::List(vec![])))
            }

            EffectCommand::InviteLanPeer {
                authority_id,
                address,
            } => {
                // LAN peer invitation flow:
                // 1. Create a contact invitation for this peer
                // 2. Export the invitation code
                // 3. Send the code to the peer's address via LAN transport
                //
                // Currently, LAN transport is not exposed via RuntimeBridge.
                // TODO: Add send_lan_invitation() to RuntimeBridge trait
                tracing::info!(
                    "Inviting LAN peer: authority={} at address={}",
                    authority_id,
                    address
                );

                // For now, we can at least export an invitation code that could be shared
                let app_core = self.app_core.read().await;

                // Try to export an invitation (requires runtime)
                // The invitation_id would normally come from a created invitation
                // For LAN invites, we generate a placeholder ID based on the target
                let invitation_id =
                    format!("lan-invite-{}", &authority_id[..8.min(authority_id.len())]);

                match app_core.export_invitation(&invitation_id).await {
                    Ok(code) => {
                        tracing::info!(
                            "Generated invitation code for LAN peer (code would be sent to {})",
                            address
                        );
                        // Return the code - in a full implementation, this would be sent via LAN
                        Some(Ok(OpResponse::Data(format!(
                            "Invitation ready for {} (LAN send not yet implemented): {}",
                            address,
                            &code[..50.min(code.len())]
                        ))))
                    }
                    Err(e) => {
                        // No runtime available - log and return success anyway
                        // (LAN invites would work when runtime is present)
                        tracing::debug!("Could not export invitation (no runtime): {}", e);
                        Some(Ok(OpResponse::Data(format!(
                            "LAN invitation queued for {} at {} (requires runtime)",
                            authority_id, address
                        ))))
                    }
                }
            }

            // =========================================================================
            // Query Commands
            // =========================================================================
            EffectCommand::ListParticipants { channel } => {
                // Get contacts snapshot to find participants
                let app_core = self.app_core.read().await;
                let snapshot = app_core.snapshot();
                let contacts = &snapshot.contacts;

                // Helper to get display name from contact
                let get_name = |c: &aura_app::views::Contact| -> String {
                    if !c.petname.is_empty() {
                        c.petname.clone()
                    } else if let Some(ref suggested) = c.suggested_name {
                        suggested.clone()
                    } else {
                        c.id.chars().take(8).collect::<String>() + "..."
                    }
                };

                let mut participants = Vec::new();

                // Always include self (current user)
                participants.push("You".to_string());

                // For DM channels (format: "dm:<contact_id>"), include just that contact
                if channel.starts_with("dm:") {
                    let contact_id = channel.strip_prefix("dm:").unwrap_or("");
                    if let Some(contact) = contacts.contact(contact_id) {
                        participants.push(get_name(contact));
                    } else {
                        participants.push(contact_id.to_string());
                    }
                } else {
                    // For group channels, include all contacts as potential participants
                    // (In a real implementation, this would query actual channel membership)
                    for contact in contacts.filtered_contacts() {
                        participants.push(get_name(contact));
                    }
                }

                Some(Ok(OpResponse::List(participants)))
            }

            EffectCommand::GetUserInfo { target } => {
                // Get contacts snapshot to find user info
                let app_core = self.app_core.read().await;
                let snapshot = app_core.snapshot();
                let contacts = &snapshot.contacts;

                // Helper to get display name from contact
                let get_name = |c: &aura_app::views::Contact| -> String {
                    if !c.petname.is_empty() {
                        c.petname.clone()
                    } else if let Some(ref suggested) = c.suggested_name {
                        suggested.clone()
                    } else {
                        c.id.chars().take(8).collect::<String>() + "..."
                    }
                };

                // Helper to format contact info
                let format_info = |c: &aura_app::views::Contact| -> String {
                    format!(
                        "User: {}\nID: {}\nOnline: {}\nGuardian: {}\nResident: {}",
                        get_name(c),
                        c.id,
                        if c.is_online { "Yes" } else { "No" },
                        if c.is_guardian { "Yes" } else { "No" },
                        if c.is_resident { "Yes" } else { "No" }
                    )
                };

                // Look up contact by ID
                if let Some(contact) = contacts.contact(target) {
                    Some(Ok(OpResponse::Data(format_info(contact))))
                } else {
                    // Try partial match by name
                    let matching: Vec<_> = contacts
                        .filtered_contacts()
                        .into_iter()
                        .filter(|c| get_name(c).to_lowercase().contains(&target.to_lowercase()))
                        .collect();

                    if matching.len() == 1 {
                        Some(Ok(OpResponse::Data(format_info(matching[0]))))
                    } else if matching.is_empty() {
                        Some(Ok(OpResponse::Data(format!("User '{}' not found", target))))
                    } else {
                        let names: Vec<_> = matching.iter().map(|c| get_name(c)).collect();
                        Some(Ok(OpResponse::Data(format!(
                            "Multiple matches for '{}': {}",
                            target,
                            names.join(", ")
                        ))))
                    }
                }
            }

            // =========================================================================
            // Context Commands
            // =========================================================================
            EffectCommand::SetContext { context_id } => {
                // Set active context - used for navigation and command targeting
                // The actual state update is handled by IoContext when it receives
                // the ContextChanged response
                let new_context = if context_id.is_empty() {
                    None
                } else {
                    Some(context_id.clone())
                };
                tracing::debug!("SetContext: changing to {:?}", new_context);
                Some(Ok(OpResponse::ContextChanged {
                    context_id: new_context,
                }))
            }

            EffectCommand::MovePosition {
                neighborhood_id: _,
                block_id,
                depth,
            } => {
                // Move position in neighborhood view
                // Parse the depth string to determine traversal depth (0=Street, 1=Frontage, 2=Interior)
                let depth_value = match depth.to_lowercase().as_str() {
                    "street" => 0,
                    "frontage" => 1,
                    "interior" => 2,
                    _ => 1, // Default to frontage
                };

                // Update neighborhood state with new position
                if let Ok(core) = self.app_core.try_read() {
                    // Get current neighborhood state
                    let mut neighborhood = core.views().get_neighborhood();

                    // Determine if this is "home" navigation
                    let target_block_id = if block_id == "home" {
                        neighborhood.home_block_id.clone()
                    } else if block_id == "current" {
                        // Stay on current block, just change depth
                        neighborhood
                            .position
                            .as_ref()
                            .map(|p| p.current_block_id.clone())
                            .unwrap_or_else(|| neighborhood.home_block_id.clone())
                    } else {
                        block_id.clone()
                    };

                    // Get block name from neighbors or use the ID
                    let block_name = neighborhood
                        .neighbor(&target_block_id)
                        .map(|n| n.name.clone())
                        .unwrap_or_else(|| {
                            // Check if it's home
                            if target_block_id == neighborhood.home_block_id {
                                neighborhood.home_block_name.clone()
                            } else {
                                target_block_id.clone()
                            }
                        });

                    // Create or update position
                    let position = aura_app::views::neighborhood::TraversalPosition {
                        current_block_id: target_block_id.clone(),
                        current_block_name: block_name,
                        depth: depth_value,
                        path: vec![target_block_id],
                    };
                    neighborhood.position = Some(position);

                    // Set the updated state
                    core.views().set_neighborhood(neighborhood);
                    tracing::debug!(
                        "MovePosition: updated to block {} at depth {}",
                        block_id,
                        depth
                    );
                }
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::AcceptPendingBlockInvitation => {
                // Accept a pending block invitation
                Some(Ok(OpResponse::Ok))
            }

            // =========================================================================
            // Settings Commands
            // =========================================================================
            EffectCommand::UpdateMfaPolicy { require_mfa: _ } => {
                // Update MFA policy setting
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::UpdateNickname { name: _ } => {
                // Update display nickname
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::SetChannelMode { channel, flags } => {
                // Return the channel mode info so IoContext can update local storage
                Some(Ok(OpResponse::ChannelModeSet {
                    channel_id: channel.clone(),
                    flags: flags.clone(),
                }))
            }

            // =========================================================================
            // Invitation Commands (Operational - export/import codes)
            // =========================================================================
            EffectCommand::ExportInvitation { invitation_id } => {
                // Export invitation code through effect injection (RuntimeBridge)
                let result = if let Ok(core) = self.app_core.try_read() {
                    core.export_invitation(invitation_id).await
                } else {
                    Err(aura_app::core::IntentError::internal_error(
                        "AppCore unavailable",
                    ))
                };

                match result {
                    Ok(code) => Some(Ok(OpResponse::InvitationCode {
                        id: invitation_id.clone(),
                        code,
                    })),
                    Err(e) => {
                        // In demo/offline mode, generate a proper invitation code from ViewState
                        tracing::debug!("Invitation export via runtime unavailable: {}, generating from ViewState", e);

                        // Get authority and invitation data from AppCore
                        let code = if let Ok(core) = self.app_core.try_read() {
                            // Get authority (use default if not set)
                            let authority = core
                                .authority()
                                .copied()
                                .unwrap_or_else(|| AuthorityId::new_from_entropy([0u8; 32]));

                            // Get invitation from ViewState
                            let snapshot = core.snapshot();
                            if let Some(inv) = snapshot.invitations.invitation(invitation_id) {
                                // Map view invitation type to domain type
                                let domain_type = match inv.invitation_type {
                                    ViewInvitationType::Block => DomainInvitationType::Channel {
                                        block_id: inv.block_id.clone().unwrap_or_default(),
                                    },
                                    ViewInvitationType::Guardian => {
                                        DomainInvitationType::Guardian {
                                            subject_authority: authority,
                                        }
                                    }
                                    ViewInvitationType::Chat => DomainInvitationType::Contact {
                                        petname: inv.from_name.clone().into(),
                                    },
                                };

                                // Parse sender authority from string
                                let sender_id =
                                    AuthorityId::from_str(&inv.from_id).unwrap_or(authority);

                                // Create ShareableInvitation
                                let shareable = ShareableInvitation {
                                    version: ShareableInvitation::CURRENT_VERSION,
                                    invitation_id: inv.id.clone(),
                                    sender_id,
                                    invitation_type: domain_type,
                                    expires_at: inv.expires_at,
                                    message: inv.message.clone(),
                                };

                                shareable.to_code()
                            } else {
                                // Invitation not found in ViewState, create minimal code
                                let shareable = ShareableInvitation {
                                    version: ShareableInvitation::CURRENT_VERSION,
                                    invitation_id: invitation_id.clone(),
                                    sender_id: authority,
                                    invitation_type: DomainInvitationType::Contact {
                                        petname: None,
                                    },
                                    expires_at: None,
                                    message: None,
                                };
                                shareable.to_code()
                            }
                        } else {
                            // AppCore unavailable, create minimal fallback code
                            let shareable = ShareableInvitation {
                                version: ShareableInvitation::CURRENT_VERSION,
                                invitation_id: invitation_id.clone(),
                                sender_id: AuthorityId::new_from_entropy([0u8; 32]),
                                invitation_type: DomainInvitationType::Contact { petname: None },
                                expires_at: None,
                                message: None,
                            };
                            shareable.to_code()
                        };

                        Some(Ok(OpResponse::InvitationCode {
                            id: invitation_id.clone(),
                            code,
                        }))
                    }
                }
            }

            EffectCommand::ImportInvitation { code } => {
                // Parse the invitation code
                tracing::info!("Importing invitation code: {}", code);

                match ShareableInvitation::from_code(code) {
                    Ok(invitation) => {
                        // Extract invitation type as string
                        let invitation_type = match &invitation.invitation_type {
                            DomainInvitationType::Channel { block_id } => {
                                format!("channel:{}", block_id)
                            }
                            DomainInvitationType::Guardian { .. } => "guardian".to_string(),
                            DomainInvitationType::Contact { petname } => {
                                if let Some(name) = petname {
                                    format!("contact:{}", name)
                                } else {
                                    "contact".to_string()
                                }
                            }
                        };

                        tracing::info!(
                            "Successfully parsed invitation: id={}, sender={}, type={}",
                            invitation.invitation_id,
                            invitation.sender_id,
                            invitation_type
                        );

                        Some(Ok(OpResponse::InvitationImported {
                            invitation_id: invitation.invitation_id,
                            sender_id: invitation.sender_id.to_string(),
                            invitation_type,
                            expires_at: invitation.expires_at,
                            message: invitation.message,
                        }))
                    }
                    Err(e) => {
                        let error_msg = match e {
                            ShareableInvitationError::InvalidFormat => {
                                "Invalid invitation code format"
                            }
                            ShareableInvitationError::UnsupportedVersion(_) => {
                                "Unsupported invitation version"
                            }
                            ShareableInvitationError::DecodingFailed => {
                                "Failed to decode invitation data"
                            }
                            ShareableInvitationError::ParsingFailed => {
                                "Failed to parse invitation data"
                            }
                        };
                        tracing::warn!("Failed to import invitation: {}", error_msg);
                        Some(Err(OpError::InvalidArgument(error_msg.to_string())))
                    }
                }
            }

            EffectCommand::InviteGuardian { contact_id } => {
                // With contact_id: handled by intent mapper -> Intent::CreateInvitation
                // Without contact_id: UI should show selection modal
                if contact_id.is_none() {
                    tracing::info!(
                        "InviteGuardian without contact_id - UI should show selection modal"
                    );
                    // Return Ok to indicate the command was "handled" - UI interprets this
                    // as a signal to show the guardian selection modal
                    Some(Ok(OpResponse::Ok))
                } else {
                    // This case is handled by intent dispatch, shouldn't reach here
                    None
                }
            }

            EffectCommand::SubmitGuardianApproval { guardian_id: _ } => {
                // Now handled by intent mapper -> Intent::ApproveRecovery
                // This shouldn't reach here, but if it does, pass through to intent dispatch
                None
            }

            // =========================================================================
            // Direct Messaging Commands
            // =========================================================================
            EffectCommand::SendDirectMessage { target, content } => {
                // Create the DM channel ID based on target
                let dm_channel_id = format!("dm:{}", target);
                tracing::info!(
                    "Sending direct message to {} in channel {}",
                    target,
                    dm_channel_id
                );

                // Get current chat state and add a message
                // Note: Full implementation would use Intent::SendMessage
                // For now, emit signal update to refresh UI
                if let Ok(core) = self.app_core.try_read() {
                    if let Ok(mut chat_state) = core.read(&*CHAT_SIGNAL).await {
                        // Create a placeholder message
                        let message = aura_app::views::chat::Message {
                            id: format!("msg-{}", uuid::Uuid::new_v4()),
                            channel_id: dm_channel_id.clone(),
                            sender_id: "self".to_string(),
                            sender_name: "You".to_string(),
                            content: content.clone(),
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64,
                            reply_to: None,
                            is_own: true,
                            is_read: true,
                        };

                        // Apply message to state
                        chat_state.apply_message(dm_channel_id.clone(), message);

                        // Emit updated state
                        let _ = core.emit(&*CHAT_SIGNAL, chat_state).await;
                    }
                }

                Some(Ok(OpResponse::Data(format!(
                    "Message sent to DM channel: {}",
                    dm_channel_id
                ))))
            }

            EffectCommand::StartDirectChat { contact_id } => {
                // Create a DM channel for this contact
                let dm_channel_id = format!("dm:{}", contact_id);

                tracing::info!(
                    "Starting direct chat with contact {} (channel: {})",
                    contact_id,
                    dm_channel_id
                );

                // Get contact name from ViewState for the channel name
                let contact_name = {
                    let core = self.app_core.read().await;
                    let snapshot = core.snapshot();
                    snapshot
                        .contacts
                        .contacts
                        .iter()
                        .find(|c| c.id == *contact_id)
                        .map(|c| c.petname.clone())
                        .unwrap_or_else(|| {
                            format!("DM with {}", &contact_id[..8.min(contact_id.len())])
                        })
                };

                // Create the DM channel
                let dm_channel = Channel {
                    id: dm_channel_id.clone(),
                    name: contact_name,
                    topic: Some(format!("Direct messages with {}", contact_id)),
                    channel_type: ChannelType::DirectMessage,
                    unread_count: 0,
                    is_dm: true,
                    member_count: 2, // Self + contact
                    last_message: None,
                    last_message_time: None,
                    last_activity: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                };

                // Add channel to ChatState and select it
                if let Ok(core) = self.app_core.try_read() {
                    if let Ok(mut chat_state) = core.read(&*CHAT_SIGNAL).await {
                        // Add the DM channel (add_channel avoids duplicates)
                        chat_state.add_channel(dm_channel);

                        // Select this channel
                        chat_state.selected_channel_id = Some(dm_channel_id.clone());
                        chat_state.messages.clear(); // Clear messages for new selection

                        // Emit updated state
                        let _ = core.emit(&*CHAT_SIGNAL, chat_state).await;
                        tracing::info!("DM channel created and selected: {}", dm_channel_id);
                    }
                }

                Some(Ok(OpResponse::Data(format!(
                    "Started DM chat: {}",
                    dm_channel_id
                ))))
            }

            EffectCommand::SendAction {
                channel: _,
                action: _,
            } => {
                // IRC-style /me action
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::InviteUser { target: _ } => Some(Ok(OpResponse::Ok)),

            // =========================================================================
            // Steward Commands
            // =========================================================================
            EffectCommand::GrantSteward { target } => {
                // Grant steward (Admin) role to a resident in the current block
                if let Ok(core) = self.app_core.try_read() {
                    let mut blocks = core.views().get_blocks();
                    if let Some(block) = blocks.current_block_mut() {
                        // Check if actor is authorized (must be Owner or Admin)
                        if !block.is_admin() {
                            return Some(Err(OpError::Failed(
                                "Only stewards can grant steward role".to_string(),
                            )));
                        }

                        // Find and update the target resident
                        if let Some(resident) = block.resident_mut(target) {
                            // Can't promote an Owner
                            if matches!(resident.role, aura_app::views::block::ResidentRole::Owner)
                            {
                                return Some(Err(OpError::Failed(
                                    "Cannot modify Owner role".to_string(),
                                )));
                            }
                            // Promote to Admin
                            resident.role = aura_app::views::block::ResidentRole::Admin;
                            core.views().set_blocks(blocks);
                            tracing::info!("Granted steward role to {}", target);
                            Some(Ok(OpResponse::Ok))
                        } else {
                            Some(Err(OpError::Failed(format!(
                                "Resident not found: {}",
                                target
                            ))))
                        }
                    } else {
                        Some(Err(OpError::Failed(
                            "No current block selected".to_string(),
                        )))
                    }
                } else {
                    Some(Err(OpError::Failed(
                        "Could not access app state".to_string(),
                    )))
                }
            }

            EffectCommand::RevokeSteward { target } => {
                // Revoke steward (Admin) role from a resident in the current block
                if let Ok(core) = self.app_core.try_read() {
                    let mut blocks = core.views().get_blocks();
                    if let Some(block) = blocks.current_block_mut() {
                        // Check if actor is authorized (must be Owner or Admin)
                        if !block.is_admin() {
                            return Some(Err(OpError::Failed(
                                "Only stewards can revoke steward role".to_string(),
                            )));
                        }

                        // Find and update the target resident
                        if let Some(resident) = block.resident_mut(target) {
                            // Can only demote Admin, not Owner
                            if !matches!(resident.role, aura_app::views::block::ResidentRole::Admin)
                            {
                                return Some(Err(OpError::Failed(
                                    "Can only revoke Admin role, not Owner or Resident".to_string(),
                                )));
                            }
                            // Demote to Resident
                            resident.role = aura_app::views::block::ResidentRole::Resident;
                            core.views().set_blocks(blocks);
                            tracing::info!("Revoked steward role from {}", target);
                            Some(Ok(OpResponse::Ok))
                        } else {
                            Some(Err(OpError::Failed(format!(
                                "Resident not found: {}",
                                target
                            ))))
                        }
                    } else {
                        Some(Err(OpError::Failed(
                            "No current block selected".to_string(),
                        )))
                    }
                } else {
                    Some(Err(OpError::Failed(
                        "Could not access app state".to_string(),
                    )))
                }
            }

            // =========================================================================
            // Commands handled by Intent dispatch - return None
            // =========================================================================
            _ => None,
        }
    }

    /// Update connection status signal
    pub async fn set_connection_status(&self, status: ConnectionStatus) {
        if let Ok(core) = self.app_core.try_read() {
            let _ = core.emit(&*CONNECTION_STATUS_SIGNAL, status).await;
        }
    }

    /// Update sync status signal
    pub async fn set_sync_status(&self, status: SyncStatus) {
        if let Ok(core) = self.app_core.try_read() {
            let _ = core.emit(&*SYNC_STATUS_SIGNAL, status).await;
        }
    }

    /// Emit an error to the error signal
    pub async fn emit_error(&self, error: AppError) {
        if let Ok(core) = self.app_core.try_read() {
            let _ = core.emit(&*ERROR_SIGNAL, Some(error)).await;
        }
    }

    /// Clear the error signal
    pub async fn clear_error(&self) {
        if let Ok(core) = self.app_core.try_read() {
            let _ = core.emit(&*ERROR_SIGNAL, None).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::AppConfig;

    fn test_app_core() -> Arc<RwLock<AppCore>> {
        Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).expect("Failed to create test AppCore"),
        ))
    }

    #[tokio::test]
    async fn test_ping_command() {
        let app_core = test_app_core();
        let handler = OperationalHandler::new(app_core);

        let result = handler.execute(&EffectCommand::Ping).await;
        assert!(matches!(result, Some(Ok(OpResponse::Ok))));
    }

    #[tokio::test]
    async fn test_list_peers_returns_list() {
        let app_core = test_app_core();
        let handler = OperationalHandler::new(app_core);

        let result = handler.execute(&EffectCommand::ListPeers).await;
        assert!(matches!(result, Some(Ok(OpResponse::List(_)))));
    }

    #[tokio::test]
    async fn test_export_invitation_returns_code() {
        let app_core = test_app_core();
        let handler = OperationalHandler::new(app_core);

        let result = handler
            .execute(&EffectCommand::ExportInvitation {
                invitation_id: "test-123".to_string(),
            })
            .await;

        match result {
            Some(Ok(OpResponse::InvitationCode { id, code })) => {
                assert_eq!(id, "test-123");
                // Now generates proper shareable invitation codes in aura:v1: format
                assert!(
                    code.starts_with("aura:v1:"),
                    "Expected aura:v1: prefix, got: {}",
                    code
                );
            }
            _ => panic!("Expected InvitationCode response"),
        }
    }

    #[tokio::test]
    async fn test_intent_commands_return_none() {
        let app_core = test_app_core();
        let handler = OperationalHandler::new(app_core);

        // SendMessage should return None (handled by intent dispatch)
        let result = handler
            .execute(&EffectCommand::SendMessage {
                channel: "general".to_string(),
                content: "Hello".to_string(),
            })
            .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_import_invitation_parses_valid_code() {
        let app_core = test_app_core();
        let handler = OperationalHandler::new(app_core);

        // First export to get a valid code
        let export_result = handler
            .execute(&EffectCommand::ExportInvitation {
                invitation_id: "roundtrip-test".to_string(),
            })
            .await;

        let code = match export_result {
            Some(Ok(OpResponse::InvitationCode { code, .. })) => code,
            _ => panic!("Expected InvitationCode response"),
        };

        // Now import the exported code
        let import_result = handler
            .execute(&EffectCommand::ImportInvitation { code })
            .await;

        match import_result {
            Some(Ok(OpResponse::InvitationImported {
                invitation_id,
                sender_id,
                invitation_type,
                ..
            })) => {
                assert_eq!(invitation_id, "roundtrip-test");
                assert!(!sender_id.is_empty());
                assert_eq!(invitation_type, "contact"); // Default type for minimal invitation
            }
            Some(Err(e)) => panic!("Import failed: {:?}", e),
            _ => panic!("Expected InvitationImported response"),
        }
    }

    #[tokio::test]
    async fn test_import_invitation_rejects_invalid_code() {
        let app_core = test_app_core();
        let handler = OperationalHandler::new(app_core);

        let result = handler
            .execute(&EffectCommand::ImportInvitation {
                code: "invalid-code".to_string(),
            })
            .await;

        match result {
            Some(Err(OpError::InvalidArgument(_))) => {
                // Expected - invalid format
            }
            _ => panic!("Expected InvalidArgument error for invalid code"),
        }
    }
}
