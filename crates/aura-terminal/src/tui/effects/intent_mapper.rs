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
//! use crate::tui::effects::intent_context::{IntoIntent, IntentContext};
//!
//! let ctx = IntentContext::from_snapshot(&snapshot);
//! if let Some(intent) = command.into_intent(&ctx) {
//!     // Dispatch through AppCore
//!     app_core.dispatch(intent)?;
//! } else {
//!     // Handle operationally via OperationalHandler
//!     operational_handler.execute(&command).await?;
//! }
//! ```

use aura_app::{Intent, IntentChannelType, InvitationType};

use super::intent_context::{parse_context_id, IntentContext, IntoIntent};
use super::EffectCommand;

// Deprecated re-exports for backwards compatibility
#[deprecated(since = "0.1.0", note = "Use IntentContext instead")]
pub use super::intent_context::IntentContext as CommandContext;
#[deprecated(since = "0.1.0", note = "Use parse_context_id instead")]
pub use super::intent_context::parse_context_id as parse_channel_id;

/// Map a TUI EffectCommand to a domain Intent.
///
/// Returns `Some(Intent)` for commands that should be journaled via AppCore.
/// Returns `None` for operational commands that should be handled by OperationalHandler.
///
/// This is a convenience function that delegates to the `IntoIntent` trait.
///
/// # Arguments
///
/// * `cmd` - The command to map
/// * `ctx` - Context providing current block and recovery IDs for commands that need them
#[inline]
pub fn command_to_intent(cmd: &EffectCommand, ctx: &IntentContext) -> Option<Intent> {
    cmd.into_intent(ctx)
}

/// Check if a command can be dispatched directly through AppCore.
///
/// Returns true if the command has an Intent mapping.
#[inline]
pub fn is_intent_command(cmd: &EffectCommand) -> bool {
    cmd.is_intent_command()
}

impl IntoIntent for EffectCommand {
    fn into_intent(&self, ctx: &IntentContext) -> Option<Intent> {
        match self {
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
                authority_id: "self".to_string(),
                public_key: device_name.clone(),
            }),

            EffectCommand::RemoveDevice { device_id } => Some(Intent::RemoveDevice {
                authority_id: "self".to_string(),
                device_id: device_id.clone(),
            }),

            // =========================================================================
            // Recovery Commands → Recovery Intents
            // =========================================================================
            EffectCommand::StartRecovery => Some(Intent::InitiateRecovery),

            EffectCommand::CompleteRecovery => Some(Intent::CompleteRecovery {
                recovery_context: ctx.recovery_id_or_nil(),
                recovered_authority_id: None,
            }),

            EffectCommand::CancelRecovery => Some(Intent::RejectRecovery {
                recovery_context: ctx.recovery_id_or_nil(),
                reason: "Cancelled by user".to_string(),
            }),

            // =========================================================================
            // Contact Commands → Contact Intents
            // =========================================================================
            EffectCommand::UpdateContactNickname {
                contact_id,
                nickname,
            } => Some(Intent::SetNickname {
                contact_id: contact_id.clone(),
                nickname: nickname.clone(),
            }),

            EffectCommand::ToggleContactGuardian { contact_id } => Some(Intent::ToggleGuardian {
                contact_id: contact_id.clone(),
                is_guardian: true,
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

            EffectCommand::SendBlockInvitation { contact_id } => Some(Intent::InviteToBlock {
                block_id: ctx.block_id_or_nil(),
                invitee_id: contact_id.clone(),
            }),

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

            EffectCommand::GrantSteward { target } => Some(Intent::GrantSteward {
                block_id: ctx.block_id_or_nil(),
                target_id: target.clone(),
            }),

            EffectCommand::RevokeSteward { target } => Some(Intent::RevokeSteward {
                block_id: ctx.block_id_or_nil(),
                target_id: target.clone(),
            }),

            // =========================================================================
            // Guardian Commands → Guardian Intents
            // =========================================================================
            EffectCommand::InviteGuardian { contact_id } => {
                if contact_id.is_some() {
                    Some(Intent::CreateInvitation {
                        invitation_type: InvitationType::Guardian,
                    })
                } else {
                    None
                }
            }

            EffectCommand::SubmitGuardianApproval { guardian_id: _ } => {
                Some(Intent::ApproveRecovery {
                    recovery_context: ctx.recovery_id_or_nil(),
                })
            }

            // =========================================================================
            // Operational Commands → None (handled by OperationalHandler)
            // =========================================================================
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
            | EffectCommand::ExportAccountBackup
            | EffectCommand::ImportAccountBackup { .. } => None,

            #[cfg(test)]
            EffectCommand::UnknownCommandForTest => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_ctx() -> IntentContext {
        IntentContext::empty()
    }

    #[test]
    fn test_send_message_mapping() {
        let cmd = EffectCommand::SendMessage {
            channel: "general".to_string(),
            content: "Hello!".to_string(),
        };
        let intent = cmd.into_intent(&empty_ctx());
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
                cmd.into_intent(&ctx).is_none(),
                "Expected {:?} to return None",
                cmd
            );
        }
    }

    #[test]
    fn test_is_intent_command() {
        assert!(EffectCommand::SendMessage {
            channel: "test".to_string(),
            content: "hello".to_string(),
        }
        .is_intent_command());

        assert!(!EffectCommand::Ping.is_intent_command());
        assert!(!EffectCommand::ForceSync.is_intent_command());
    }

    #[test]
    fn test_create_channel_mapping() {
        let cmd = EffectCommand::CreateChannel {
            name: "new-channel".to_string(),
            topic: Some("Discussion".to_string()),
            members: vec![],
        };
        let intent = cmd.into_intent(&empty_ctx());
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
        let intent = cmd.into_intent(&empty_ctx());
        assert!(matches!(intent, Some(Intent::InitiateRecovery)));
    }

    #[test]
    fn test_command_context_uses_block_id() {
        let block_id =
            aura_core::identifiers::ContextId::from(uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap());
        let ctx = IntentContext {
            block_id: Some(block_id),
            recovery_context_id: None,
            authority_id: None,
            channel_id: None,
        };

        let cmd = EffectCommand::BanUser {
            target: "bad-user".to_string(),
            reason: Some("spam".to_string()),
        };
        let intent = cmd.into_intent(&ctx);
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
            aura_core::identifiers::ContextId::from(uuid::Uuid::parse_str("660e8400-e29b-41d4-a716-446655440001").unwrap());
        let ctx = IntentContext {
            block_id: None,
            recovery_context_id: Some(recovery_id),
            authority_id: None,
            channel_id: None,
        };

        let cmd = EffectCommand::CompleteRecovery;
        let intent = cmd.into_intent(&ctx);
        if let Some(Intent::CompleteRecovery {
            recovery_context, ..
        }) = intent
        {
            assert_eq!(recovery_context, recovery_id);
        } else {
            panic!("Expected CompleteRecovery intent");
        }
    }

    #[test]
    fn test_command_to_intent_function() {
        // Test the convenience function works
        let ctx = empty_ctx();
        let cmd = EffectCommand::StartRecovery;
        let intent = command_to_intent(&cmd, &ctx);
        assert!(matches!(intent, Some(Intent::InitiateRecovery)));
    }
}
