use aura_core::effects::CapabilityKey;
use aura_core::AuthorizedReadinessPublication;

fn main() {
    let raw = CapabilityKey::new("semantic:readiness");
    let _ = AuthorizedReadinessPublication::authorize(&raw, "channel_membership_ready");
}
