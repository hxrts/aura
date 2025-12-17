//! Invitation CLI handlers - Terminal-Specific Formatting
//!
//! This module provides terminal-specific invitation formatting for CLI and TUI.
//! Business logic has been moved to `aura_app::workflows::invitation`.
//!
//! ## Architecture
//!
//! - **Business Logic**: `aura_app::workflows::invitation` (portable)
//! - **Formatting**: This module (terminal-specific)
//!
//! Returns structured `CliOutput` for testability.

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use crate::InvitationAction;
use aura_core::identifiers::AuthorityId;
use std::str::FromStr;

// Re-export workflow functions for backward compatibility
// NOTE: Most operations require RuntimeBridge extension (see TODOs in aura-app)
use aura_agent::handlers::ShareableInvitation;
use aura_agent::{AuraAgent, InvitationService};

// NOTE: Workflow functions imported but not yet used (waiting for RuntimeBridge extension)
// use aura_app::workflows::invitation::{
//     accept_invitation, cancel_invitation, decline_invitation, export_invitation, import_invitation,
//     list_invitations,
// };

/// Handle invitation-related CLI commands
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_invitation(
    ctx: &HandlerContext<'_>,
    action: &InvitationAction,
) -> TerminalResult<CliOutput> {
    let agent = ctx
        .agent()
        .ok_or_else(|| TerminalError::Operation("agent not available in handler context".into()))?;

    match action {
        InvitationAction::Create {
            account,
            invitee,
            role,
            ttl,
        } => {
            let mut output = CliOutput::new();
            let invitation = create_invitation(agent, account, invitee, role, *ttl).await?;
            output.println(format!(
                "Invitation created: id={} to={} role={} ttl={:?}",
                invitation.invitation_id, invitee, role, ttl
            ));
            Ok(output)
        }
        InvitationAction::Accept { invitation_id } => {
            let mut output = CliOutput::new();
            let service = agent.invitations().await?;
            let result = service.accept(invitation_id).await?;
            if result.success {
                output.println(format!("Invitation {} accepted", invitation_id));
            } else if let Some(err) = result.error {
                output.eprintln(format!("Invitation {} failed: {}", invitation_id, err));
            }
            Ok(output)
        }
        InvitationAction::Decline { invitation_id } => {
            let mut output = CliOutput::new();
            let service = agent.invitations().await?;
            let result = service.decline(invitation_id).await?;
            if result.success {
                output.println(format!("Invitation {} declined", invitation_id));
            } else if let Some(err) = result.error {
                output.eprintln(format!(
                    "Invitation {} decline failed: {}",
                    invitation_id, err
                ));
            }
            Ok(output)
        }
        InvitationAction::Cancel { invitation_id } => {
            let mut output = CliOutput::new();
            let service = agent.invitations().await?;
            let result = service.cancel(invitation_id).await?;
            if result.success {
                output.println(format!("Invitation {} canceled", invitation_id));
            } else if let Some(err) = result.error {
                output.eprintln(format!(
                    "Invitation {} cancel failed: {}",
                    invitation_id, err
                ));
            }
            Ok(output)
        }
        InvitationAction::List => {
            let mut output = CliOutput::new();
            let service = agent.invitations().await?;
            let pending = service.list_pending().await;
            if pending.is_empty() {
                output.println("No pending invitations.");
            } else {
                output.section("Pending invitations");
                for inv in pending {
                    output.println(format!(
                        "  - {} → {} ({}) status={:?} expires={:?}",
                        inv.sender_id,
                        inv.receiver_id,
                        inv.invitation_type.as_type_string(),
                        inv.status,
                        inv.expires_at
                    ));
                }
            }
            Ok(output)
        }
        InvitationAction::Export { invitation_id } => {
            let mut output = CliOutput::new();
            let service = agent.invitations().await?;
            let code = service.export_code(invitation_id).await?;
            output.section("Shareable Invitation Code");
            output.println(&code);
            output.blank();
            output.println("Share this code with the recipient.");
            output.println("They can import it using: aura invite import <code>");
            Ok(output)
        }
        InvitationAction::Import { code } => {
            let mut output = CliOutput::new();
            let shareable = InvitationService::import_code(code)
                .map_err(|e| TerminalError::Input(format!("Invalid invitation code: {}", e)))?;

            output.section("Invitation Details");
            output.kv("Invitation ID", shareable.invitation_id.to_string());
            output.kv("From", shareable.sender_id.to_string());
            output.kv("Type", format_invitation_type(&shareable));

            if let Some(msg) = &shareable.message {
                output.kv("Message", msg);
            }

            if let Some(exp) = shareable.expires_at {
                // Convert ms to human-readable timestamp
                let secs = exp / 1000;
                let nanos = ((exp % 1000) * 1_000_000) as u32;
                if let Some(dt) =
                    std::time::UNIX_EPOCH.checked_add(std::time::Duration::new(secs, nanos))
                {
                    output.kv("Expires", format!("{:?}", dt));
                } else {
                    output.kv("Expires", format!("{} (ms since epoch)", exp));
                }
            } else {
                output.kv("Expires", "Never");
            }

            output.blank();
            output.println("To accept this invitation, use:");
            output.println(format!("  aura invite accept {}", shareable.invitation_id));
            Ok(output)
        }
    }
}

/// Format invitation type for display
fn format_invitation_type(shareable: &ShareableInvitation) -> String {
    match &shareable.invitation_type {
        aura_agent::handlers::InvitationType::Contact { petname } => {
            if let Some(name) = petname {
                format!("Contact (petname: {})", name)
            } else {
                "Contact".to_string()
            }
        }
        aura_agent::handlers::InvitationType::Guardian { subject_authority } => {
            format!("Guardian (for: {})", subject_authority)
        }
        aura_agent::handlers::InvitationType::Channel { block_id } => {
            format!("Channel (block: {})", block_id)
        }
    }
}

async fn create_invitation(
    agent: &AuraAgent,
    account: &str,
    invitee: &str,
    role: &str,
    ttl_secs: Option<u64>,
) -> TerminalResult<aura_agent::Invitation> {
    let receiver_id = AuthorityId::from_uuid(
        uuid::Uuid::from_str(invitee)
            .map_err(|e| TerminalError::Input(format!("invalid invitee authority: {}", e)))?,
    );
    let subject_authority = AuthorityId::from_uuid(
        uuid::Uuid::from_str(account)
            .map_err(|e| TerminalError::Input(format!("invalid account authority: {}", e)))?,
    );
    let service = agent.invitations().await?;
    let expires_ms = ttl_secs.map(|s| s * 1000);

    if role.eq_ignore_ascii_case("guardian") {
        service
            .invite_as_guardian(receiver_id, subject_authority, None, expires_ms)
            .await
            .map_err(|e| TerminalError::Operation(e.to_string()))
    } else if role.eq_ignore_ascii_case("channel") {
        service
            .invite_to_channel(receiver_id, "channel".to_string(), None, expires_ms)
            .await
            .map_err(|e| TerminalError::Operation(e.to_string()))
    } else {
        service
            .invite_as_contact(receiver_id, Some(role.to_string()), None, expires_ms)
            .await
            .map_err(|e| TerminalError::Operation(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_agent::handlers::InvitationType;

    fn test_shareable(invitation_type: InvitationType) -> ShareableInvitation {
        ShareableInvitation {
            version: 1,
            invitation_id: "test-invitation-123".to_string(),
            sender_id: AuthorityId::new(),
            invitation_type,
            message: None,
            expires_at: None,
        }
    }

    #[test]
    fn test_format_invitation_type_contact_without_petname() {
        let shareable = test_shareable(InvitationType::Contact { petname: None });
        let result = format_invitation_type(&shareable);
        assert_eq!(result, "Contact");
    }

    #[test]
    fn test_format_invitation_type_contact_with_petname() {
        let shareable = test_shareable(InvitationType::Contact {
            petname: Some("Alice".to_string()),
        });
        let result = format_invitation_type(&shareable);
        assert_eq!(result, "Contact (petname: Alice)");
    }

    #[test]
    fn test_format_invitation_type_guardian() {
        let subject = AuthorityId::new();
        let shareable = test_shareable(InvitationType::Guardian {
            subject_authority: subject,
        });
        let result = format_invitation_type(&shareable);
        assert!(result.starts_with("Guardian (for: "));
        assert!(result.contains(&subject.to_string()));
    }

    #[test]
    fn test_format_invitation_type_channel() {
        let shareable = test_shareable(InvitationType::Channel {
            block_id: "block-123".to_string(),
        });
        let result = format_invitation_type(&shareable);
        assert_eq!(result, "Channel (block: block-123)");
    }
}
