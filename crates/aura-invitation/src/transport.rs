//! Rendezvous/SBB delivery placeholder used until the full transport stack lands.

use crate::InvitationResult;
use aura_core::identifiers::DeviceId;
use aura_protocol::effects::{AuraEffectSystem, ConsoleEffects};

/// Log a rendezvous delivery intent so future transport work can hook in.
pub async fn deliver_via_rendezvous(
    effects: &AuraEffectSystem,
    _payload: &[u8],
    inviter: DeviceId,
    invitee: DeviceId,
    ttl_window_secs: u64,
) -> InvitationResult<()> {
    let message = format!(
        "Rendezvous delivery queued: inviter={} invitee={} ttl={}s",
        inviter, invitee, ttl_window_secs
    );
    effects.log_info(&message, &[]);
    Ok(())
}
