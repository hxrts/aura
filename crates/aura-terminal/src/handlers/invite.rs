//! Invitation CLI handlers.

use crate::handlers::HandlerContext;
use crate::InvitationAction;
use anyhow::{anyhow, Context, Result};
use aura_agent::AgentBuilder;
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
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_invitation(ctx: &HandlerContext<'_>, action: &InvitationAction) -> Result<()> {
    let _device_id = ctx.device_id();

    match action {
        InvitationAction::Create {
            account,
            invitee,
            role,
            ttl,
        } => {
            let request = build_request(ctx, account, invitee, role, *ttl).await?;

            // Create fresh agent for coordinator
            let agent = AgentBuilder::new()
                .with_authority(crate::ids::authority_id(&format!(
                    "invite:create:{}",
                    account
                )))
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
                .with_authority(crate::ids::authority_id("invite:accept"))
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
    ctx: &HandlerContext<'_>,
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
    let authority = load_account_authority(ctx, account_id).await?;
    let device_token = authority
        .create_device_token(invitee_id)
        .map_err(|e| anyhow!("failed to create device token: {}", e))?;
    let granted_token = SerializableBiscuit::new(device_token, authority.root_public_key());

    Ok(DeviceInvitationRequest {
        inviter: ctx.device_id(),
        invitee: invitee_id,
        account_id,
        granted_token,
        device_role: role.to_string(),
        ttl_secs: ttl,
    })
}

/// Load an account authority from storage, persisting a new one if not present.
async fn load_account_authority(
    ctx: &HandlerContext<'_>,
    account_id: AccountId,
) -> Result<AccountAuthority> {
    let key = format!("account_authority:{}:root_key", account_id);

    if let Ok(Some(raw)) = ctx.effects().retrieve(&key).await {
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
    ctx.effects()
        .store(&key, private_bytes.to_vec())
        .await
        .map_err(anyhow::Error::from)?;

    Ok(authority)
}
