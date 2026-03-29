use aura_core::{OperationContext, TraceContext};

fn publish_done() {}

#[aura_macros::semantic_owner(
    owner = "demo-owner",
    wrapper = "missing_proof_wrapper",
    terminal = "publish_done",
    postcondition = "demo_done",
    authoritative_inputs = "",
    depends_on = "",
    child_ops = "",
    category = "move_owned"
)]
async fn missing_proof(
    _context: Option<&mut OperationContext<&'static str, u64, TraceContext>>,
) {
    publish_done();
}

fn main() {}
