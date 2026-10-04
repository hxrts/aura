#![allow(dead_code)]
struct ActualOwner;
struct ForeignOwner;
impl ForeignOwner {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "actual_runtime",
        receiver_type = ActualOwner,
        family = "runtime_helper"
    )]
    fn forbidden(&self) {}
}
fn main() {}
