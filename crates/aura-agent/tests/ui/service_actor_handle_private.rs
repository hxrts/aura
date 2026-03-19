use aura_agent::runtime::services::ServiceActorHandle;

fn main() {
    let _ = std::mem::size_of::<ServiceActorHandle<(), ()>>();
}
