#[aura_macros::ownership_lifecycle(
    initial = "Submitted",
    ordered = "Submitted,Dispatched,Ready",
    terminals = "Succeeded,Failed,Cancelled"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoLifecycle {
    Submitted,
    Dispatched,
    Succeeded,
    Failed,
    Cancelled,
}

fn main() {}
