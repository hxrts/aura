#[aura_macros::actor_root(
    owner = "maintenance_service",
    domain = "runtime_maintenance",
    supervision = "maintenance_task_root",
    category = "actor_owned"
)]
fn maintenance_service_root() {}

fn main() {}
