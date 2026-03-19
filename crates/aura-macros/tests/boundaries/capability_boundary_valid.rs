use aura_core::{AuthorizedReadinessPublication, LifecyclePublicationCapability};

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "demo-capability"
)]
fn capability_surface() -> &'static LifecyclePublicationCapability {
    static CAPABILITY: std::sync::LazyLock<LifecyclePublicationCapability> =
        std::sync::LazyLock::new(|| LifecyclePublicationCapability::new("demo-capability"));
    &CAPABILITY
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "demo-readiness"
)]
fn authorize_payload() -> AuthorizedReadinessPublication<&'static str> {
    let capability = aura_core::ReadinessPublicationCapability::new("demo-readiness");
    AuthorizedReadinessPublication::authorize(&capability, "ok")
}

fn main() {
    let _ = capability_surface();
    let _ = authorize_payload();
}
