use aura_core::ownership::OperationContext;
use aura_core::{OperationTimeoutBudget, OwnedShutdownToken, OwnerEpoch, PublicationSequence, TraceContext};

fn main() {
    let _context = OperationContext {
        operation_id: "invitation_accept",
        instance_id: 1u64,
        owner_epoch: OwnerEpoch::new(0),
        publication_sequence: PublicationSequence::new(0),
        timeout_budget: OperationTimeoutBudget::deferred_local_policy(),
        shutdown_token: OwnedShutdownToken::detached(),
        trace_context: TraceContext::detached(),
    };
}
