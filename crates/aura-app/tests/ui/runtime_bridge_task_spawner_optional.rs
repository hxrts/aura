use aura_app::runtime_bridge::{OfflineRuntimeBridge, RuntimeBridge};
use aura_core::{AuthorityId, OwnedTaskSpawner};

fn main() {
    let bridge = OfflineRuntimeBridge::new(AuthorityId::from_entropy([7; 32]));
    let _: Option<OwnedTaskSpawner> = bridge.task_spawner();
}
