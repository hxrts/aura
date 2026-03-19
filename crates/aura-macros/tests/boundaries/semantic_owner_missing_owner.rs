use aura_core::{OperationContext, TraceContext};

fn publish_done() {}

#[aura_macros::semantic_owner(
    terminal = "publish_done",
    category = "move_owned"
)]
async fn missing_owner(
    _context: Option<&mut OperationContext<&'static str, u64, TraceContext>>,
) {
    publish_done();
}

fn main() {}
