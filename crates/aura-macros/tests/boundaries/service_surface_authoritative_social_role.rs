use aura_macros::service_surface;

#[service_surface(
    families = "Move",
    object_categories = "authoritative_shared,transport_protocol",
    discover = "descriptor_publication",
    permit = "runtime_policy",
    transfer = "transport_effects",
    select = "runtime_selector",
    authoritative = "NeighborhoodMoveDescriptor",
    runtime_local = "move_queue",
    category = "service_surface"
)]
pub struct SampleService;

fn main() {}
