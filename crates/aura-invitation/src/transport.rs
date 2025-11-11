//! Rendezvous/SBB delivery placeholder used until the full transport stack lands.

use crate::InvitationResult;
use aura_core::effects::ConsoleEffects;
use aura_core::identifiers::DeviceId;

/// Log a rendezvous delivery intent so future transport work can hook in.
pub async fn deliver_via_rendezvous<C: ConsoleEffects>(
    console: &C,
    _payload: &[u8],
    inviter: DeviceId,
    invitee: DeviceId,
    ttl_window_secs: u64,
) -> InvitationResult<()> {
    let message = format!(
        "Rendezvous delivery queued: inviter={} invitee={} ttl={}s",
        inviter, invitee, ttl_window_secs
    );
    console.log_info(&message).await?;
    Ok(())
}
