use aura_agent::HoldLocalIndexEntry;
use aura_core::{ContextId, HeldObject, ServiceProfile};

fn main() {
    let scope = ContextId::new_from_entropy([1u8; 32]);
    let index = HoldLocalIndexEntry {
        scope,
        content_key: "index-only".to_string(),
        profile: ServiceProfile::DeferredDeliveryHold,
        selector_count: 1,
        last_observed_ms: 0,
    };
    let _held: HeldObject = index;
}
