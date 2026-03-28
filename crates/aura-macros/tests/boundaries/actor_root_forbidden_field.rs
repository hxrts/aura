use aura_core::OperationContext;

#[aura_macros::actor_root(
    owner = "runtime_service",
    domain = "runtime",
    supervision = "runtime_task_root",
    category = "actor_owned"
)]
pub struct RuntimeService {
    terminal: OperationContext<&'static str, u64, aura_core::TraceContext>,
}

fn main() {}
