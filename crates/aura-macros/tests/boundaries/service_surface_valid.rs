use aura_macros::service_surface;

#[service_surface(
    families = "Establish,Move",
    object_categories = "authoritative_shared,transport_protocol,runtime_derived_local,proof_accounting",
    discover = "descriptor_publication",
    permit = "runtime_policy",
    transfer = "transport_effects",
    select = "runtime_selector",
    authoritative = "ServiceDescriptor,ChannelEstablished",
    runtime_local = "descriptor_cache,selected_transport",
    category = "service_surface"
)]
pub struct SampleService;

fn main() {
    let declaration = SampleService::SERVICE_SURFACE_DECLARATION;
    assert_eq!(declaration.service_name, "SampleService");
    assert_eq!(declaration.families.len(), 2);
}
