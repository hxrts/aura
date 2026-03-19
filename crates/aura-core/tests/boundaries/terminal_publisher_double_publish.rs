use aura_core::{
    issue_operation_context, LifecyclePublicationCapability, OperationContextCapability,
    OperationTimeoutBudget, OwnedShutdownToken, OwnerEpoch, PublicationSequence, TraceContext,
};

fn main() {
    let context_capability = OperationContextCapability::new("operation:context");
    let capability = LifecyclePublicationCapability::new("semantic:lifecycle");
    let context = issue_operation_context(
        &context_capability,
        "invitation_accept",
        1u64,
        OwnerEpoch::new(0),
        PublicationSequence::new(0),
        OperationTimeoutBudget::deferred_local_policy(),
        OwnedShutdownToken::detached(),
        TraceContext::detached(),
    );
    let publisher = context.begin_terminal::<(), &'static str>(&capability);
    let _ = publisher.cancel();
    let _ = publisher.succeed(());
}
