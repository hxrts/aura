//! Invitation CLI handlers.

use crate::InvitationAction;
use anyhow::{anyhow, Context, Result};
use aura_agent::AgentBuilder;
use aura_core::{AccountId, DeviceId};
use aura_core::identifiers::AuthorityId;
use aura_invitation::{
    device_invitation::{DeviceInvitationCoordinator, DeviceInvitationRequest, InvitationEnvelope},
    invitation_acceptance::InvitationAcceptanceCoordinator,
};
use aura_protocol::effect_traits::ConsoleEffects;
use aura_wot::{AccountAuthority, SerializableBiscuit};
use std::{fs, str::FromStr};

/// Handle invitation-related CLI commands
///
/// Processes invitation actions including create, accept, and status operations
pub async fn handle_invitation(
    effects: &aura_agent::AuraEffectSystem,
    action: &InvitationAction,
) -> Result<()> {
    // Get device_id from context (for now, create a temp one - this should be passed from CLI)
    let device_id = DeviceId::new(); // TODO: Pass device_id from caller

    match action {
        InvitationAction::Create {
            account,
            invitee,
            role,
            ttl,
        } => {
            let request = build_request(device_id, account, invitee, role, *ttl)?;

            // Create fresh agent for coordinator
            let agent = AgentBuilder::new()
                .with_authority(AuthorityId::new()) 
                .build_testing()?;
            let coord_effects = agent.runtime().effects().clone();

            let coordinator = DeviceInvitationCoordinator::new(std::sync::Arc::new(coord_effects));
            let response = coordinator
                .invite_device(request)
                .await
                .context("failed to create invitation")?;

            println!(
                "Invitation {} sent to {} (expires at {}).",
                response.invitation.invitation_id,
                response.invitation.invitee,
                response.invitation.expires_at
            );
            Ok(())
        }
        InvitationAction::Accept { envelope } => {
            let contents = fs::read_to_string(envelope)
                .with_context(|| format!("unable to read envelope {:?}", envelope))?;
            let envelope: InvitationEnvelope =
                serde_json::from_str(&contents).context("invalid invitation envelope")?;

            // Create fresh agent for coordinator
            let agent = AgentBuilder::new()
                .with_authority(AuthorityId::new()) 
                .build_testing()?;
            let coord_effects = agent.runtime().effects().clone();

            let coordinator =
                InvitationAcceptanceCoordinator::new(std::sync::Arc::new(coord_effects));
            let acceptance = coordinator
                .accept_invitation(envelope)
                .await
                .context("failed to accept invitation")?;

            println!(
                "Accepted invitation {} at {}.",
                acceptance.invitation_id, acceptance.accepted_at
            );
            Ok(())
        }
    }
}

fn build_request(
    device_id: DeviceId,
    account: &str,
    invitee: &str,
    role: &str,
    ttl: Option<u64>,
) -> Result<DeviceInvitationRequest> {
    let account_id = AccountId::from_str(account)
        .map_err(|err| anyhow!("invalid account id '{}': {}", account, err))?;
    let invitee_id = DeviceId::from_str(invitee)
        .map_err(|err| anyhow!("invalid invitee device id '{}': {}", invitee, err))?;

    // Create a Biscuit token for the invitation
    // TODO: Load actual account authority from storage
    let authority = AccountAuthority::new(account_id);
    let device_token = authority
        .create_device_token(invitee_id)
        .map_err(|e| anyhow!("failed to create device token: {}", e))?;
    let granted_token = SerializableBiscuit::new(device_token, authority.root_public_key());

    Ok(DeviceInvitationRequest {
        inviter: device_id,
        invitee: invitee_id,
        account_id,
        granted_token,
        device_role: role.to_string(),
        ttl_secs: ttl,
    })
}
