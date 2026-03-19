use aura_core::{OperationContext, OperationTimeoutBudget, OwnedShutdownToken, TraceContext};

fn publish_done() {}

#[aura_macros::semantic_owner(
    owner = "demo-owner",
    terminal = "publish_done",
    category = "move_owned"
)]
async fn valid_owner(
    _context: Option<&mut OperationContext<&'static str, u64, TraceContext>>,
) {
    publish_done();
}

fn main() {
    let _ = (
        OperationTimeoutBudget::deferred_local_policy(),
        OwnedShutdownToken::detached(),
    );
}
