use aura_core::LifecyclePublicationCapability;

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "demo-capability"
)]
fn capability_surface() -> &'static LifecyclePublicationCapability {
    static CAPABILITY: std::sync::LazyLock<LifecyclePublicationCapability> =
        std::sync::LazyLock::new(|| LifecyclePublicationCapability::new("demo-capability"));
    &CAPABILITY
}

fn main() {}
