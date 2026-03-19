use aura_core::{OperationContext, TraceContext};

#[aura_macros::semantic_owner(
    owner = "demo-owner",
    terminal = "publish_done",
    category = "move_owned"
)]
async fn missing_terminal_path(
    _context: Option<&mut OperationContext<&'static str, u64, TraceContext>>,
) {
    let _ = 1usize;
}

fn main() {}
