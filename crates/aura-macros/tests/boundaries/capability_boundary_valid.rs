use aura_core::{AuthorizedReadinessPublication, LifecyclePublicationCapability};

static DEMO_PROOF_CAPABILITY: std::sync::LazyLock<LifecyclePublicationCapability> =
    std::sync::LazyLock::new(|| LifecyclePublicationCapability::new("demo-proof"));
static DEMO_RUNTIME_HELPER_CAPABILITY: std::sync::LazyLock<LifecyclePublicationCapability> =
    std::sync::LazyLock::new(|| LifecyclePublicationCapability::new("demo-runtime-helper"));

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "demo-capability",
    family = "capability_accessor"
)]
fn capability_surface() -> &'static LifecyclePublicationCapability {
    static CAPABILITY: std::sync::LazyLock<LifecyclePublicationCapability> =
        std::sync::LazyLock::new(|| LifecyclePublicationCapability::new("demo-capability"));
    &CAPABILITY
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "demo-readiness",
    family = "authorizer"
)]
fn authorize_payload() -> AuthorizedReadinessPublication<&'static str> {
    let capability = aura_core::ReadinessPublicationCapability::new("demo-readiness");
    AuthorizedReadinessPublication::authorize(&capability, "ok")
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "demo-proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
fn issue_demo_proof(value: &str) -> String {
    let _ = &*DEMO_PROOF_CAPABILITY;
    value.to_string()
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "demo-runtime-helper",
    family = "runtime_helper"
)]
fn runtime_helper() -> Option<&'static str> {
    let _ = &*DEMO_RUNTIME_HELPER_CAPABILITY;
    None
}

fn main() {
    let _ = capability_surface();
    let _ = authorize_payload();
    let _ = issue_demo_proof("ok");
    let _ = runtime_helper();
}
