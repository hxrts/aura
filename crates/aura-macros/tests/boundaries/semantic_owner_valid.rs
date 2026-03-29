use aura_core::{
    OperationContext, OperationTimeoutBudget, OwnedShutdownToken, SemanticOwnerPostcondition,
    SemanticSuccessProof, TraceContext,
};

struct DemoProof;

impl SemanticSuccessProof for DemoProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
        SemanticOwnerPostcondition::new("demo_done")
    }
}

fn publish_done() {}

#[aura_macros::semantic_owner(
    owner = "demo-owner",
    wrapper = "valid_owner_wrapper",
    terminal = "publish_done",
    postcondition = "demo_done",
    proof = DemoProof,
    authoritative_inputs = "runtime",
    depends_on = "",
    child_ops = "",
    category = "move_owned"
)]
async fn valid_owner(
    _context: Option<&mut OperationContext<&'static str, u64, TraceContext>>,
) {
    publish_done();
}

async fn valid_owner_wrapper(context: Option<&mut OperationContext<&'static str, u64, TraceContext>>) {
    valid_owner(context).await;
}

fn main() {
    let _ = (
        OperationTimeoutBudget::deferred_local_policy(),
        OwnedShutdownToken::detached(),
    );
}
