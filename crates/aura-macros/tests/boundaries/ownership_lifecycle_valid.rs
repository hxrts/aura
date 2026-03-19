#[aura_macros::ownership_lifecycle(
    initial = "Submitted",
    ordered = "Submitted,Dispatched,Ready",
    terminals = "Succeeded,Failed,Cancelled"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoLifecycle {
    Submitted,
    Dispatched,
    Ready,
    Succeeded,
    Failed,
    Cancelled,
}

fn main() {
    assert!(DemoLifecycle::Submitted.can_transition_to(DemoLifecycle::Ready));
    assert!(DemoLifecycle::Ready.can_transition_to(DemoLifecycle::Succeeded));
    assert!(!DemoLifecycle::Succeeded.can_transition_to(DemoLifecycle::Failed));
    assert!(DemoLifecycle::Cancelled.is_terminal());
}
