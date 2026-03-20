use aura_core::{OperationContext, TraceContext};

struct DemoProof;

#[aura_macros::semantic_owner(
    owner = "demo-owner",
    terminal = "publish_done",
    postcondition = "demo_done",
    proof = DemoProof,
    authoritative_inputs = "",
    depends_on = "",
    child_ops = "",
    category = "move_owned"
)]
async fn missing_terminal_path(
    _context: Option<&mut OperationContext<&'static str, u64, TraceContext>>,
) {
    let _ = 1usize;
}

fn main() {}
