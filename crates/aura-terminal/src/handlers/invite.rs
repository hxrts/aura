//! Invitation CLI handlers - Terminal-Specific Formatting
//!
//! This module provides terminal-specific invitation formatting for CLI and TUI.
//! Business logic has been moved to `aura_app::ui::workflows::invitation`.
//!
//! ## Architecture
//!
//! - **Business Logic**: `aura_app::ui::workflows::invitation` (portable)
//! - **Formatting**: This module (terminal-specific)
//!
//! Returns structured `CliOutput` for testability.

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use crate::InvitationAction;
use aura_core::identifiers::AuthorityId;
use std::str::FromStr;

// CLI handlers use direct agent service access (more efficient for CLI context)
// TUI handlers should use aura_app::ui::workflows::invitation for portability
use aura_agent::handlers::ShareableInvitation;
use aura_agent::{AuraAgent, InvitationService};

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
            let service = agent.invitations()?;
            let result = service.accept(invitation_id).await?;
            if result.success {
                output.println(format!("Invitation {invitation_id} accepted"));
            } else if let Some(err) = result.error {
                output.eprintln(format!("Invitation {invitation_id} failed: {err}"));
            }
            Ok(output)
        }
        InvitationAction::Decline { invitation_id } => {
            let mut output = CliOutput::new();
            let service = agent.invitations()?;
            let result = service.decline(invitation_id).await?;
            if result.success {
                output.println(format!("Invitation {invitation_id} declined"));
            } else if let Some(err) = result.error {
                output.eprintln(format!("Invitation {invitation_id} decline failed: {err}"));
            }
            Ok(output)
        }
        InvitationAction::Cancel { invitation_id } => {
            let mut output = CliOutput::new();
            let service = agent.invitations()?;
            let result = service.cancel(invitation_id).await?;
            if result.success {
                output.println(format!("Invitation {invitation_id} canceled"));
            } else if let Some(err) = result.error {
                output.eprintln(format!("Invitation {invitation_id} cancel failed: {err}"));
            }
            Ok(output)
        }
        InvitationAction::List => {
            let mut output = CliOutput::new();
            let service = agent.invitations()?;
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
            let service = agent.invitations()?;
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
                .map_err(|e| TerminalError::Input(format!("Invalid invitation code: {e}")))?;

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
                    output.kv("Expires", format!("{dt:?}"));
                } else {
                    output.kv("Expires", format!("{exp} (ms since epoch)"));
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
        aura_agent::handlers::InvitationType::Contact { nickname } => {
            if let Some(name) = nickname {
                format!("Contact (nickname: {name})")
            } else {
                "Contact".to_string()
            }
        }
        aura_agent::handlers::InvitationType::Guardian { subject_authority } => {
            format!("Guardian (for: {subject_authority})")
        }
        aura_agent::handlers::InvitationType::Channel { home_id } => {
            format!("Channel (home: {home_id})")
        }
        aura_agent::handlers::InvitationType::DeviceEnrollment {
            subject_authority,
            device_id,
            device_name,
            pending_epoch,
            ..
        } => {
            let label = device_name
                .as_deref()
                .map(|s| format!(" (name: {s})"))
                .unwrap_or_default();
            format!(
                "Device enrollment (authority: {subject_authority}, device: {device_id}{label}, pending_epoch: {pending_epoch})"
            )
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
            .map_err(|e| TerminalError::Input(format!("invalid invitee authority: {e}")))?,
    );
    let subject_authority = AuthorityId::from_uuid(
        uuid::Uuid::from_str(account)
            .map_err(|e| TerminalError::Input(format!("invalid account authority: {e}")))?,
    );
    let service = agent.invitations()?;
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
            sender_id: AuthorityId::new_from_entropy([42u8; 32]),
            invitation_type,
            message: None,
            expires_at: None,
        }
    }

    #[test]
    fn test_format_invitation_type_contact_without_nickname() {
        let shareable = test_shareable(InvitationType::Contact { nickname: None });
        let result = format_invitation_type(&shareable);
        assert_eq!(result, "Contact");
    }

    #[test]
    fn test_format_invitation_type_contact_with_nickname() {
        let shareable = test_shareable(InvitationType::Contact {
            nickname: Some("Alice".to_string()),
        });
        let result = format_invitation_type(&shareable);
        assert_eq!(result, "Contact (nickname: Alice)");
    }

    #[test]
    fn test_format_invitation_type_guardian() {
        let subject = AuthorityId::new_from_entropy([99u8; 32]);
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
            home_id: "home-123".to_string(),
        });
        let result = format_invitation_type(&shareable);
        assert_eq!(result, "Channel (home: home-123)");
    }
}
