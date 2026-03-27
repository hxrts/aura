#[aura_macros::capability_boundary(category = "capability_gated")]
fn capability_surface() -> &'static str {
    "ok"
}

fn main() {}
