use aura_app::runtime_bridge::{OfflineRuntimeBridge, RuntimeBridge};
use aura_core::{AuthorityId, OwnedShutdownToken};

fn main() {
    let bridge = OfflineRuntimeBridge::new(AuthorityId::from_entropy([7; 32]));
    let _: Option<OwnedShutdownToken> = bridge.cancellation_token();
}
