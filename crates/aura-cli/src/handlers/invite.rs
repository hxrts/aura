//! Invitation CLI handlers.

use crate::InvitationAction;
use anyhow::{anyhow, Context, Result};
use aura_agent::{AgentBuilder, AuraEffectSystem, EffectContext};
use aura_core::identifiers::AuthorityId;
use aura_core::{effects::StorageEffects, AccountId, DeviceId};
use aura_invitation::{
    device_invitation::{DeviceInvitationCoordinator, DeviceInvitationRequest, InvitationEnvelope},
    invitation_acceptance::InvitationAcceptanceCoordinator,
};
use aura_wot::{AccountAuthority, SerializableBiscuit};
use biscuit_auth::{KeyPair, PrivateKey};
use std::{fs, str::FromStr};

/// Handle invitation-related CLI commands
///
/// Processes invitation actions including create, accept, and status operations
pub async fn handle_invitation(
    _ctx: &EffectContext,
    effects: &aura_agent::AuraEffectSystem,
    action: &InvitationAction,
) -> Result<()> {
    // Derive device_id from caller authority context
    let device_id = DeviceId::from_uuid(_ctx.authority_id().uuid());

    match action {
        InvitationAction::Create {
            account,
            invitee,
            role,
            ttl,
        } => {
            let request = build_request(effects, device_id, account, invitee, role, *ttl).await?;

            // Create fresh agent for coordinator
            let agent = AgentBuilder::new()
                .with_authority(AuthorityId::new())
                .build_testing()?;
            let coord_effects = agent.runtime().effects().clone();

            let coordinator = DeviceInvitationCoordinator::new(coord_effects);
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

            let coordinator = InvitationAcceptanceCoordinator::new(coord_effects);
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

async fn build_request(
    effects: &AuraEffectSystem,
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

    // Load authority from storage if available; otherwise create and persist a new authority
    let authority = load_account_authority(effects, account_id).await?;
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

/// Load an account authority from storage, persisting a new one if not present.
async fn load_account_authority(
    effects: &AuraEffectSystem,
    account_id: AccountId,
) -> Result<AccountAuthority> {
    let key = format!("account_authority:{}:root_key", account_id);

    if let Ok(Some(raw)) = effects.retrieve(&key).await {
        // Stored as raw private key bytes
        if raw.len() == 32 {
            if let Ok(private) = PrivateKey::from_bytes(&raw) {
                let keypair = KeyPair::from(&private);
                return Ok(AccountAuthority::from_keypair(account_id, keypair));
            }
        }
    }

    // Create and persist a new authority for future use
    let authority = AccountAuthority::new(account_id);
    let private_bytes = authority.root_keypair().private().to_bytes();
    effects
        .store(&key, private_bytes.to_vec())
        .await
        .map_err(anyhow::Error::from)?;

    Ok(authority)
}
