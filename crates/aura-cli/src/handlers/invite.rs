//! Invitation CLI handlers.

use crate::InvitationAction;
use anyhow::{anyhow, Context, Result};
use aura_core::{AccountId, Cap, DeviceId};
use aura_invitation::{
    device_invitation::{DeviceInvitationCoordinator, DeviceInvitationRequest, InvitationEnvelope},
    invitation_acceptance::InvitationAcceptanceCoordinator,
};
use aura_protocol::effects::{AuraEffectSystem, ConsoleEffects};
use std::{fs, str::FromStr};

/// Handle invitation-related CLI commands
///
/// Processes invitation actions including create, accept, and status operations
pub async fn handle_invitation(
    effects: &AuraEffectSystem,
    action: &InvitationAction,
) -> Result<()> {
    match action {
        InvitationAction::Create {
            account,
            invitee,
            role,
            ttl,
        } => {
            let request = build_request(effects, account, invitee, role, *ttl)?;
            let coordinator = DeviceInvitationCoordinator::new(effects.clone());
            let response = coordinator
                .invite_device(request)
                .await
                .context("failed to create invitation")?;

            let _ = effects
                .log_info(&format!(
                    "Invitation {} sent to {} (expires at {}).",
                    response.invitation.invitation_id,
                    response.invitation.invitee,
                    response.invitation.expires_at
                ))
                .await;
            Ok(())
        }
        InvitationAction::Accept { envelope } => {
            let contents = fs::read_to_string(envelope)
                .with_context(|| format!("unable to read envelope {:?}", envelope))?;
            let envelope: InvitationEnvelope =
                serde_json::from_str(&contents).context("invalid invitation envelope")?;

            let coordinator = InvitationAcceptanceCoordinator::new(effects.clone());
            let acceptance = coordinator
                .accept_invitation(envelope)
                .await
                .context("failed to accept invitation")?;

            let _ = effects
                .log_info(&format!(
                    "Accepted invitation {} at {}.",
                    acceptance.invitation_id, acceptance.accepted_at
                ))
                .await;
            Ok(())
        }
    }
}

fn build_request(
    effects: &AuraEffectSystem,
    account: &str,
    invitee: &str,
    role: &str,
    ttl: Option<u64>,
) -> Result<DeviceInvitationRequest> {
    let account_id = AccountId::from_str(account)
        .map_err(|err| anyhow!("invalid account id '{}': {}", account, err))?;
    let invitee_id = DeviceId::from_str(invitee)
        .map_err(|err| anyhow!("invalid invitee device id '{}': {}", invitee, err))?;

    Ok(DeviceInvitationRequest {
        inviter: effects.device_id(),
        invitee: invitee_id,
        account_id,
        granted_capabilities: Cap::top(),
        device_role: role.to_string(),
        ttl_secs: ttl,
    })
}
