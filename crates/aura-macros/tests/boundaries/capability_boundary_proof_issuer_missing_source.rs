static SEMANTIC_POSTCONDITION_PROOF_CAPABILITY: std::sync::LazyLock<
    aura_core::LifecyclePublicationCapability,
> = std::sync::LazyLock::new(|| {
    aura_core::LifecyclePublicationCapability::new("semantic_postcondition_proof")
});

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
fn issue_pending_invitation_consumed_proof(invitation_id: &str) -> String {
    let _ = &*SEMANTIC_POSTCONDITION_PROOF_CAPABILITY;
    invitation_id.to_string()
}

fn main() {}
