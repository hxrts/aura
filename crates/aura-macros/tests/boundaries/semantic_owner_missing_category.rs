use aura_core::{OperationContext, TraceContext};

struct DemoProof;

fn publish_done() {}

#[aura_macros::semantic_owner(
    owner = "demo-owner",
    wrapper = "missing_category_wrapper",
    terminal = "publish_done",
    postcondition = "demo_done",
    proof = DemoProof,
    authoritative_inputs = "",
    depends_on = "",
    child_ops = ""
)]
async fn missing_category(
    _context: Option<&mut OperationContext<&'static str, u64, TraceContext>>,
) {
    publish_done();
}

fn main() {}
