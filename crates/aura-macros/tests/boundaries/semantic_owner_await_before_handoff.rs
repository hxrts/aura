use aura_core::{
    OperationTimeoutBudget, OwnedShutdownToken, SemanticOwnerPostcondition, SemanticSuccessProof,
};

struct DemoProof;

impl SemanticSuccessProof for DemoProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
        SemanticOwnerPostcondition::new("demo_done")
    }
}

fn publish_done() {}

struct DemoTransfer;

impl DemoTransfer {
    fn handoff_to_app_workflow(&self) {}
}

async fn preflight() {}

#[aura_macros::semantic_owner(
    owner = "demo-owner",
    wrapper = "invalid_owner_wrapper",
    terminal = "publish_done",
    postcondition = "demo_done",
    proof = DemoProof,
    authoritative_inputs = "runtime",
    depends_on = "",
    child_ops = "",
    category = "move_owned"
)]
async fn invalid_owner(
    _context: Option<&mut aura_core::OperationContext<&'static str, u64, aura_core::TraceContext>>,
) {
    let transfer = DemoTransfer;
    preflight().await;
    transfer.handoff_to_app_workflow();
    publish_done();
}

fn main() {
    let _ = (
        OperationTimeoutBudget::deferred_local_policy(),
        OwnedShutdownToken::detached(),
    );
}
