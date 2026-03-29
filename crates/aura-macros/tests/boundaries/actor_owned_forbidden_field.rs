use aura_core::OperationContext;

#[derive(Clone)]
struct DemoCommand;

struct FakeOperationId;
struct FakeOperationInstanceId;
struct FakeTraceContext;

#[aura_macros::actor_owned(
    owner = "demo-manager",
    domain = "demo-domain",
    gate = "demo-ingress",
    command = DemoCommand,
    capacity = 64,
    category = "actor_owned"
)]
struct DemoManager {
    operation: OperationContext<FakeOperationId, FakeOperationInstanceId, FakeTraceContext>,
}

fn main() {}
