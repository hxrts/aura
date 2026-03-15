use aura_core::{AuthorizedLifecyclePublication, OperationLifecycle};
use aura_core::effects::CapabilityKey;

fn main() {
    let raw = CapabilityKey::new("semantic:lifecycle");
    let lifecycle = OperationLifecycle::<&'static str, (), &'static str>::submitted();
    let _ = AuthorizedLifecyclePublication::authorize(&raw, lifecycle);
}
