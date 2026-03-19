use aura_core::effects::CapabilityKey;
use aura_core::{
    issue_operation_context, LifecyclePublicationCapability, OperationProgress,
    OperationTimeoutBudget, OwnedShutdownToken, OwnerEpoch, PublicationSequence, TraceContext,
};

fn main() {
    let raw = CapabilityKey::new("operation:context");
    let mut context = issue_operation_context(
        &raw,
        "invitation_accept",
        1u64,
        OwnerEpoch::new(0),
        PublicationSequence::new(0),
        OperationTimeoutBudget::deferred_local_policy(),
        OwnedShutdownToken::detached(),
        TraceContext::detached(),
    );
    let capability = LifecyclePublicationCapability::new("semantic:lifecycle");
    let _ = context.publish_update(&capability, OperationProgress::submitted());
}
