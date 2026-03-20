use aura_core::{OperationContext, SemanticOwnerPostcondition, SemanticSuccessProof, TraceContext};

struct DemoProof;

impl SemanticSuccessProof for DemoProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
        SemanticOwnerPostcondition::new("demo_done")
    }
}

fn publish_done() {}

#[aura_macros::semantic_owner(
    owner = "demo-owner",
    terminal = "publish_done",
    postcondition = "demo_done",
    proof = DemoProof,
    depends_on = "",
    child_ops = "",
    category = "move_owned"
)]
async fn missing_authoritative_inputs(
    _context: Option<&mut OperationContext<&'static str, u64, TraceContext>>,
) {
    publish_done();
}

fn main() {}
