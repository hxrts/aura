use aura_macros::service_surface;

#[service_surface(
    families = "NeighborhoodRelay",
    object_categories = "authoritative_shared,transport_protocol",
    discover = "descriptor_publication",
    permit = "runtime_policy",
    transfer = "transport_effects",
    select = "runtime_selector",
    authoritative = "ServiceDescriptor",
    runtime_local = "descriptor_cache",
    category = "service_surface"
)]
pub struct SampleService;

fn main() {}
