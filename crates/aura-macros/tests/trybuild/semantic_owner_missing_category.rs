use aura_core::{OperationContext, TraceContext};

fn publish_done() {}

#[aura_macros::semantic_owner(owner = "demo-owner", terminal = "publish_done")]
async fn missing_category(
    _context: Option<&mut OperationContext<&'static str, u64, TraceContext>>,
) {
    publish_done();
}

fn main() {}
