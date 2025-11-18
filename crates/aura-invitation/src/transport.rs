//! Rendezvous/SBB delivery placeholder used until the full transport stack lands.

use crate::InvitationResult;
// use aura_core::effects::ConsoleEffects; // Unused
use aura_core::identifiers::DeviceId;
use aura_protocol::orchestration::AuraEffects;

/// Log a rendezvous delivery intent so future transport work can hook in.
pub async fn deliver_via_rendezvous<C: AuraEffects + ?Sized>(
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
