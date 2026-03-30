use aura_macros::service_surface;

#[service_surface(
    families = "Establish",
    object_categories = "authoritative_shared,transport_protocol,runtime_derived_local",
    discover = "descriptor_publication",
    permit = "runtime_policy",
    transfer = "transport_effects",
    select = "runtime_selector",
    authoritative = "RendezvousDescriptor",
    runtime_local = "guardian_retry_budget",
    category = "service_surface"
)]
pub struct SampleService;

fn main() {}
