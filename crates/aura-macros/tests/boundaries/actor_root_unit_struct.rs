#[aura_macros::actor_root(
    owner = "runtime_service",
    domain = "runtime",
    supervision = "runtime_task_root",
    category = "actor_owned"
)]
pub struct RuntimeService;

fn main() {}
