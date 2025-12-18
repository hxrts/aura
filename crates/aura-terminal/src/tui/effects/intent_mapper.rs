//! # Intent Mapper
//!
//! Maps TUI `EffectCommand` to domain `Intent` for unified dispatch.
//!
//! This module provides the bridge between TUI-specific commands and the
//! domain-centric Intent system in aura-app. Commands that represent user
//! actions that should be journaled are mapped to Intents.
//!
//! ## Mapping Strategy
//!
//! - **Journaled commands** (SendMessage, CreateChannel, etc.) → Intent
//! - **Operational commands** (Ping, ForceSync, ListPeers) → None (handled by OperationalHandler)
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::tui::effects::intent_mapper::{command_to_intent, CommandContext};
//!
//! let ctx = CommandContext::from_snapshots(&block_snapshot, &recovery_snapshot);
//! if let Some(intent) = command_to_intent(&command, &ctx) {
//!     // Dispatch through AppCore
//!     app_core.dispatch(intent)?;
//! } else {
//!     // Handle operationally via OperationalHandler
//!     operational_handler.execute(&command).await?;
//! }
//! ```

use aura_app::{Intent, IntentChannelType, InvitationType};
use aura_core::identifiers::ContextId;

/// Context information for command-to-intent mapping.
///
/// This provides the current block and recovery context IDs needed to
/// properly construct Intents that require context (moderation commands,
/// recovery commands, etc.).
///
/// ## Construction
///
/// Use `from_app_core_snapshot()` to construct from an AppCore snapshot,
/// or set fields directly for testing.
#[derive(Debug, Clone, Default)]
pub struct CommandContext {
    /// Current block ID for moderation commands (BanUser, MuteUser, etc.)
    pub current_block_id: Option<ContextId>,

    /// Active recovery context ID for recovery commands (CompleteRecovery, ApproveRecovery, etc.)
    pub recovery_context_id: Option<ContextId>,
}

impl CommandContext {
    /// Create an empty context (uses nil_context() for all values)
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create context from an AppCore StateSnapshot
    pub fn from_snapshot(snapshot: &aura_app::StateSnapshot) -> Self {
        // Extract current block ID from neighborhood position
        let current_block_id = snapshot.neighborhood.position.as_ref().map(|p| {
            // Try to parse as UUID first, then hash the string
            if let Ok(uuid) = uuid::Uuid::parse_str(&p.current_block_id) {
                ContextId::from(uuid)
            } else {
                // Hash the string for deterministic ID
                let hash = aura_core::hash::hash(p.current_block_id.as_bytes());
                let mut bytes = [0u8; 16];
                bytes.copy_from_slice(&hash[..16]);
                ContextId::from(uuid::Uuid::from_bytes(bytes))
            }
        });

        // Extract recovery context ID from active recovery
        let recovery_context_id = snapshot.recovery.active_recovery.as_ref().map(|r| {
            // Try to parse as UUID first, then hash the string
            if let Ok(uuid) = uuid::Uuid::parse_str(&r.id) {
                ContextId::from(uuid)
            } else {
                // Hash the string for deterministic ID
                let hash = aura_core::hash::hash(r.id.as_bytes());
                let mut bytes = [0u8; 16];
                bytes.copy_from_slice(&hash[..16]);
                ContextId::from(uuid::Uuid::from_bytes(bytes))
            }
        });

        Self {
            current_block_id,
            recovery_context_id,
        }
    }

    /// Get the current block ID, falling back to nil_context() if not set
    fn block_id_or_nil(&self) -> ContextId {
        self.current_block_id.unwrap_or_else(nil_context)
    }

    /// Get the recovery context ID, falling back to nil_context() if not set
    fn recovery_id_or_nil(&self) -> ContextId {
        self.recovery_context_id.unwrap_or_else(nil_context)
    }
}

use super::EffectCommand;

/// Create a nil/default ContextId for placeholder purposes.
///
/// Used when a command doesn't specify a context but the Intent requires one.
/// The actual context is resolved at dispatch time in AppCore.
fn nil_context() -> ContextId {
    ContextId::new_from_entropy([0u8; 32])
}

/// Map a TUI EffectCommand to a domain Intent.
///
/// Returns `Some(Intent)` for commands that should be journaled via AppCore.
/// Returns `None` for operational commands that should be handled by OperationalHandler.
///
/// # Arguments
///
/// * `cmd` - The command to map
/// * `ctx` - Context providing current block and recovery IDs for commands that need them
pub fn command_to_intent(cmd: &EffectCommand, ctx: &CommandContext) -> Option<Intent> {
    match cmd {
        // =========================================================================
        // Chat Commands → Chat Intents
        // =========================================================================
        EffectCommand::SendMessage { channel, content } => Some(Intent::SendMessage {
            channel_id: parse_context_id(channel),
            content: content.clone(),
            reply_to: None,
        }),

        EffectCommand::CreateChannel { name, .. } => Some(Intent::CreateChannel {
            name: name.clone(),
            channel_type: IntentChannelType::Block,
        }),

        EffectCommand::JoinChannel { channel } => Some(Intent::JoinChannel {
            channel_id: parse_context_id(channel),
        }),

        EffectCommand::LeaveChannel { channel } => Some(Intent::LeaveChannel {
            channel_id: parse_context_id(channel),
        }),

        // RetryMessage re-sends a failed message with the same content
        EffectCommand::RetryMessage {
            message_id: _,
            channel,
            content,
        } => Some(Intent::SendMessage {
            channel_id: parse_context_id(channel),
            content: content.clone(),
            reply_to: None,
        }),

        EffectCommand::CloseChannel { channel } => {
            // Map to UpdateChannel with close semantics
            // Note: Intent doesn't have CloseChannel, we use UpdateChannel
            Some(Intent::UpdateChannel {
                channel_id: parse_context_id(channel),
                name: None,
                description: Some("[closed]".to_string()),
            })
        }

        EffectCommand::SetTopic { channel, text } => Some(Intent::SetBlockTopic {
            block_id: parse_context_id(channel),
            topic: text.clone(),
        }),

        // =========================================================================
        // Account & Settings Commands → Authority/Account Intents
        // =========================================================================
        EffectCommand::CreateAccount { display_name } => Some(Intent::CreateAccount {
            name: display_name.clone(),
        }),

        EffectCommand::UpdateThreshold {
            threshold_k,
            threshold_n: _,
        } => Some(Intent::UpdateThreshold {
            threshold: *threshold_k as u32,
        }),

        EffectCommand::AddDevice { device_name } => Some(Intent::AddDevice {
            authority_id: "self".to_string(), // Current authority
            public_key: device_name.clone(),  // Placeholder - real impl uses public key
        }),

        EffectCommand::RemoveDevice { device_id } => Some(Intent::RemoveDevice {
            authority_id: "self".to_string(),
            device_id: device_id.clone(),
        }),

        // =========================================================================
        // Recovery Commands → Recovery Intents
        // =========================================================================
        EffectCommand::StartRecovery => Some(Intent::InitiateRecovery),

        EffectCommand::CompleteRecovery => {
            // CompleteRecovery needs the recovery context ID and recovered authority
            // The recovered_authority_id will be provided by the RecoverySession
            // after guardians reconstruct it via FROST threshold signatures
            Some(Intent::CompleteRecovery {
                recovery_context: ctx.recovery_id_or_nil(),
                recovered_authority_id: None, // Will be set from RecoverySession
            })
        }

        EffectCommand::CancelRecovery => {
            // Map to RejectRecovery with cancellation reason
            Some(Intent::RejectRecovery {
                recovery_context: ctx.recovery_id_or_nil(),
                reason: "Cancelled by user".to_string(),
            })
        }

        // =========================================================================
        // Contact Commands → Contact Intents
        // =========================================================================
        EffectCommand::UpdateContactPetname {
            contact_id,
            petname,
        } => Some(Intent::SetPetname {
            contact_id: contact_id.clone(),
            petname: petname.clone(),
        }),

        EffectCommand::ToggleContactGuardian { contact_id } => Some(Intent::ToggleGuardian {
            contact_id: contact_id.clone(),
            is_guardian: true, // Toggle semantics - actual state checked in AppCore
        }),

        // =========================================================================
        // Invitation Commands → Invitation Intents
        // =========================================================================
        EffectCommand::CreateInvitation {
            invitation_type, ..
        } => {
            let inv_type = match invitation_type.to_lowercase().as_str() {
                "guardian" => InvitationType::Guardian,
                "chat" | "channel" => InvitationType::Chat,
                _ => InvitationType::Block,
            };
            Some(Intent::CreateInvitation {
                invitation_type: inv_type,
            })
        }

        EffectCommand::AcceptInvitation { invitation_id } => Some(Intent::AcceptInvitation {
            invitation_fact: invitation_id.clone(),
        }),

        EffectCommand::DeclineInvitation { invitation_id } => Some(Intent::RejectInvitation {
            invitation_fact: invitation_id.clone(),
        }),

        // =========================================================================
        // Block Commands → Block Intents
        // =========================================================================
        EffectCommand::CreateBlock { name } => Some(Intent::CreateBlock {
            name: name.clone().unwrap_or_else(|| "New Block".to_string()),
        }),

        EffectCommand::SendBlockInvitation { contact_id } => {
            // Use current block_id from context
            Some(Intent::InviteToBlock {
                block_id: ctx.block_id_or_nil(),
                invitee_id: contact_id.clone(),
            })
        }

        // =========================================================================
        // Block Moderation Commands → Moderation Intents
        // =========================================================================
        EffectCommand::KickUser {
            channel,
            target,
            reason,
        } => Some(Intent::KickUser {
            block_id: parse_context_id(channel),
            target_id: target.clone(),
            reason: reason
                .clone()
                .unwrap_or_else(|| "No reason given".to_string()),
        }),

        EffectCommand::BanUser { target, reason } => Some(Intent::BanUser {
            block_id: ctx.block_id_or_nil(),
            target_id: target.clone(),
            reason: reason
                .clone()
                .unwrap_or_else(|| "No reason given".to_string()),
        }),

        EffectCommand::UnbanUser { target } => Some(Intent::UnbanUser {
            block_id: ctx.block_id_or_nil(),
            target_id: target.clone(),
        }),

        EffectCommand::MuteUser {
            target,
            duration_secs,
        } => Some(Intent::MuteUser {
            block_id: ctx.block_id_or_nil(),
            target_id: target.clone(),
            duration_secs: *duration_secs,
        }),

        EffectCommand::UnmuteUser { target } => Some(Intent::UnmuteUser {
            block_id: ctx.block_id_or_nil(),
            target_id: target.clone(),
        }),

        EffectCommand::PinMessage { message_id } => Some(Intent::PinMessage {
            block_id: ctx.block_id_or_nil(),
            message_id: message_id.clone(),
        }),

        EffectCommand::UnpinMessage { message_id } => Some(Intent::UnpinMessage {
            block_id: ctx.block_id_or_nil(),
            message_id: message_id.clone(),
        }),

        EffectCommand::GrantSteward { .. } => {
            // Map to authority management - placeholder for steward role
            // The Intent system doesn't have direct steward grant, use AddDevice pattern
            None // Handled by OperationalHandler for now
        }

        EffectCommand::RevokeSteward { .. } => {
            None // Handled by OperationalHandler for now
        }

        // =========================================================================
        // Guardian Commands → Guardian Intents
        // =========================================================================
        EffectCommand::InviteGuardian { contact_id } => {
            // If contact_id is provided, create a guardian invitation for that contact
            // If None, UI should show selection modal (handled by TUI, returns None here)
            if contact_id.is_some() {
                Some(Intent::CreateInvitation {
                    invitation_type: InvitationType::Guardian,
                })
            } else {
                // No contact specified - TUI should show selection modal
                None
            }
        }

        EffectCommand::SubmitGuardianApproval { guardian_id: _ } => {
            // Guardian approving a recovery request
            // Uses recovery context from CommandContext
            Some(Intent::ApproveRecovery {
                recovery_context: ctx.recovery_id_or_nil(),
            })
        }

        // =========================================================================
        // Operational Commands → None (handled by OperationalHandler)
        // =========================================================================
        // These commands don't create journal facts, they're operational
        EffectCommand::Ping
        | EffectCommand::Shutdown
        | EffectCommand::RefreshAccount
        | EffectCommand::ForceSync
        | EffectCommand::RequestState { .. }
        | EffectCommand::AddPeer { .. }
        | EffectCommand::RemovePeer { .. }
        | EffectCommand::ListPeers
        | EffectCommand::DiscoverPeers
        | EffectCommand::ListLanPeers
        | EffectCommand::InviteLanPeer { .. }
        | EffectCommand::ListParticipants { .. }
        | EffectCommand::GetUserInfo { .. }
        | EffectCommand::SetContext { .. }
        | EffectCommand::AcceptPendingBlockInvitation
        | EffectCommand::MovePosition { .. }
        | EffectCommand::UpdateMfaPolicy { .. }
        | EffectCommand::UpdateNickname { .. }
        | EffectCommand::SetChannelMode { .. }
        | EffectCommand::ExportInvitation { .. }
        | EffectCommand::ImportInvitation { .. }
        | EffectCommand::SendDirectMessage { .. }
        | EffectCommand::StartDirectChat { .. }
        | EffectCommand::SendAction { .. }
        | EffectCommand::InviteUser { .. }
        | EffectCommand::UnknownCommandForTest
        | EffectCommand::ExportAccountBackup
        | EffectCommand::ImportAccountBackup { .. } => None,
    }
}

/// Check if a command can be dispatched directly through AppCore.
///
/// Returns true if the command has an Intent mapping.
/// Uses an empty context for checking - actual dispatch should provide real context.
#[inline]
pub fn is_intent_command(cmd: &EffectCommand) -> bool {
    command_to_intent(cmd, &CommandContext::empty()).is_some()
}

/// Parse a channel/block ID string into a ContextId.
///
/// Uses deterministic hashing for consistent IDs across sessions.
fn parse_context_id(id_str: &str) -> ContextId {
    // Try to parse as UUID first
    if let Ok(uuid) = uuid::Uuid::parse_str(id_str) {
        return ContextId::from(uuid);
    }

    // Fall back to deterministic hashing for named channels
    let hash = aura_core::hash::hash(id_str.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    ContextId::from(uuid::Uuid::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_ctx() -> CommandContext {
        CommandContext::empty()
    }

    #[test]
    fn test_send_message_mapping() {
        let cmd = EffectCommand::SendMessage {
            channel: "general".to_string(),
            content: "Hello!".to_string(),
        };
        let intent = command_to_intent(&cmd, &empty_ctx());
        assert!(intent.is_some());
        if let Some(Intent::SendMessage { content, .. }) = intent {
            assert_eq!(content, "Hello!");
        } else {
            panic!("Expected SendMessage intent");
        }
    }

    #[test]
    fn test_operational_commands_return_none() {
        let ctx = empty_ctx();
        let commands = vec![
            EffectCommand::Ping,
            EffectCommand::ForceSync,
            EffectCommand::ListPeers,
            EffectCommand::RefreshAccount,
        ];

        for cmd in commands {
            assert!(
                command_to_intent(&cmd, &ctx).is_none(),
                "Expected {:?} to return None",
                cmd
            );
        }
    }

    #[test]
    fn test_create_channel_mapping() {
        let cmd = EffectCommand::CreateChannel {
            name: "new-channel".to_string(),
            topic: Some("Discussion".to_string()),
            members: vec![],
        };
        let intent = command_to_intent(&cmd, &empty_ctx());
        assert!(intent.is_some());
        if let Some(Intent::CreateChannel { name, .. }) = intent {
            assert_eq!(name, "new-channel");
        } else {
            panic!("Expected CreateChannel intent");
        }
    }

    #[test]
    fn test_recovery_mapping() {
        let cmd = EffectCommand::StartRecovery;
        let intent = command_to_intent(&cmd, &empty_ctx());
        assert!(matches!(intent, Some(Intent::InitiateRecovery)));
    }

    #[test]
    fn test_context_id_parsing() {
        // Named channel
        let id1 = parse_context_id("general");
        let id2 = parse_context_id("general");
        assert_eq!(id1, id2); // Deterministic

        // Different channel
        let id3 = parse_context_id("random");
        assert_ne!(id1, id3);

        // UUID format
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let id4 = parse_context_id(uuid_str);
        assert_eq!(
            id4,
            ContextId::from(uuid::Uuid::parse_str(uuid_str).unwrap())
        );
    }

    #[test]
    fn test_command_context_uses_block_id() {
        let block_id =
            ContextId::from(uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap());
        let ctx = CommandContext {
            current_block_id: Some(block_id),
            recovery_context_id: None,
        };

        // BanUser should use the block_id from context
        let cmd = EffectCommand::BanUser {
            target: "bad-user".to_string(),
            reason: Some("spam".to_string()),
        };
        let intent = command_to_intent(&cmd, &ctx);
        if let Some(Intent::BanUser {
            block_id: actual_block_id,
            ..
        }) = intent
        {
            assert_eq!(actual_block_id, block_id);
        } else {
            panic!("Expected BanUser intent");
        }
    }

    #[test]
    fn test_command_context_uses_recovery_id() {
        let recovery_id =
            ContextId::from(uuid::Uuid::parse_str("660e8400-e29b-41d4-a716-446655440001").unwrap());
        let ctx = CommandContext {
            current_block_id: None,
            recovery_context_id: Some(recovery_id),
        };

        // CompleteRecovery should use the recovery_id from context
        let cmd = EffectCommand::CompleteRecovery;
        let intent = command_to_intent(&cmd, &ctx);
        if let Some(Intent::CompleteRecovery {
            recovery_context, ..
        }) = intent
        {
            assert_eq!(recovery_context, recovery_id);
        } else {
            panic!("Expected CompleteRecovery intent");
        }
    }
}
