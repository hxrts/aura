//! Invitation CLI handlers.

use crate::handlers::HandlerContext;
use crate::InvitationAction;
use anyhow::{anyhow, Result};
use aura_core::{effects::StorageEffects, AccountId, DeviceId};
use aura_wot::{AccountAuthority, SerializableBiscuit};
use biscuit_auth::{KeyPair, PrivateKey};
use std::str::FromStr;

/// Minimal invitation payload used by the CLI while the coordinator wiring is pending.
#[derive(Debug)]
struct PreparedInvitation {
    inviter: DeviceId,
    invitee: DeviceId,
    account_id: AccountId,
    granted_token: SerializableBiscuit,
    device_role: String,
    ttl_secs: Option<u64>,
}

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

            // Coordinator integration pending effect system RwLock update.
            // Coordinators expect Arc<E: AuraEffects> but agent uses Arc<RwLock<AuraEffectSystem>>.
            println!(
                "Invitation request prepared for {} to account {} with role '{}' (ttl: {:?}).",
                request.invitee, request.account_id, request.device_role, request.ttl_secs
            );
            println!("Note: Full coordinator integration pending effect system update.");
            Ok(())
        }
        InvitationAction::Accept { envelope } => {
            // Coordinator integration pending effect system RwLock update.
            println!("Accept invitation from envelope {:?}.", envelope);
            println!("Note: Full coordinator integration pending effect system update.");
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
) -> Result<PreparedInvitation> {
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

    Ok(PreparedInvitation {
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
