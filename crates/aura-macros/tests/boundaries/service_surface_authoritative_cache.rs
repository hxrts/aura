use aura_macros::service_surface;

#[service_surface(
    families = "Move",
    object_categories = "authoritative_shared,transport_protocol,runtime_derived_local",
    discover = "descriptor_publication",
    permit = "runtime_policy",
    transfer = "transport_effects",
    select = "runtime_selector",
    authoritative = "descriptor_cache",
    runtime_local = "selected_transport",
    category = "service_surface"
)]
pub struct SampleService;

fn main() {}
