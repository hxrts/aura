//! Invitation CLI handlers.

use crate::handlers::HandlerContext;
use crate::InvitationAction;
use anyhow::{anyhow, Result};
// Import agent types from aura-agent (runtime layer)
use aura_agent::AuraAgent;
use aura_core::identifiers::AuthorityId;
use std::str::FromStr;

/// Handle invitation-related CLI commands
///
/// Processes invitation actions including create, accept, and status operations
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_invitation(ctx: &HandlerContext<'_>, action: &InvitationAction) -> Result<()> {
    let agent = ctx
        .agent()
        .ok_or_else(|| anyhow!("agent not available in handler context"))?;

    match action {
        InvitationAction::Create {
            account,
            invitee,
            role,
            ttl,
        } => {
            let invitation = create_invitation(agent, account, invitee, role, *ttl).await?;
            println!(
                "Invitation created: id={} to={} role={} ttl={:?}",
                invitation.invitation_id, invitee, role, ttl
            );
            Ok(())
        }
        InvitationAction::Accept { invitation_id } => {
            let service = agent.invitations().await?;
            let result = service.accept(invitation_id).await?;
            if result.success {
                println!("Invitation {} accepted", invitation_id);
            } else if let Some(err) = result.error {
                println!("Invitation {} failed: {}", invitation_id, err);
            }
            Ok(())
        }
        InvitationAction::Decline { invitation_id } => {
            let service = agent.invitations().await?;
            let result = service.decline(invitation_id).await?;
            if result.success {
                println!("Invitation {} declined", invitation_id);
            } else if let Some(err) = result.error {
                println!("Invitation {} decline failed: {}", invitation_id, err);
            }
            Ok(())
        }
        InvitationAction::Cancel { invitation_id } => {
            let service = agent.invitations().await?;
            let result = service.cancel(invitation_id).await?;
            if result.success {
                println!("Invitation {} canceled", invitation_id);
            } else if let Some(err) = result.error {
                println!("Invitation {} cancel failed: {}", invitation_id, err);
            }
            Ok(())
        }
        InvitationAction::List => {
            let service = agent.invitations().await?;
            let pending = service.list_pending().await;
            if pending.is_empty() {
                println!("No pending invitations.");
            } else {
                println!("Pending invitations:");
                for inv in pending {
                    println!(
                        "- {} → {} ({}) status={:?} expires={:?}",
                        inv.sender_id,
                        inv.receiver_id,
                        inv.invitation_type.as_type_string(),
                        inv.status,
                        inv.expires_at
                    );
                }
            }
            Ok(())
        }
    }
}

async fn create_invitation(
    agent: &AuraAgent,
    account: &str,
    invitee: &str,
    role: &str,
    ttl_secs: Option<u64>,
) -> Result<aura_agent::Invitation> {
    let receiver_id = AuthorityId::from_uuid(
        uuid::Uuid::from_str(invitee).map_err(|e| anyhow!("invalid invitee authority: {e}"))?,
    );
    let subject_authority = AuthorityId::from_uuid(
        uuid::Uuid::from_str(account).map_err(|e| anyhow!("invalid account authority: {e}"))?,
    );
    let service = agent.invitations().await?;
    let expires_ms = ttl_secs.map(|s| s * 1000);

    if role.eq_ignore_ascii_case("guardian") {
        service
            .invite_as_guardian(receiver_id, subject_authority, None, expires_ms)
            .await
            .map_err(|e| anyhow!(e))
    } else if role.eq_ignore_ascii_case("channel") {
        service
            .invite_to_channel(receiver_id, "channel".to_string(), None, expires_ms)
            .await
            .map_err(|e| anyhow!(e))
    } else {
        service
            .invite_as_contact(receiver_id, Some(role.to_string()), None, expires_ms)
            .await
            .map_err(|e| anyhow!(e))
    }
}
