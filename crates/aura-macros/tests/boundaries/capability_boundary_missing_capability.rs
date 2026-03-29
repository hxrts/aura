#[aura_macros::capability_boundary(
    category = "capability_gated",
    family = "runtime_helper"
)]
fn capability_surface() -> &'static str {
    "ok"
}

fn main() {}
