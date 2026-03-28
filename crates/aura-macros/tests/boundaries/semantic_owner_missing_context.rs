struct DemoProof;

fn publish_done() {}

#[aura_macros::semantic_owner(
    owner = "demo-owner",
    wrapper = "missing_context_wrapper",
    terminal = "publish_done",
    postcondition = "demo_done",
    proof = DemoProof,
    authoritative_inputs = "",
    depends_on = "",
    child_ops = "",
    category = "move_owned"
)]
async fn missing_context() {
    publish_done();
}

fn main() {}
