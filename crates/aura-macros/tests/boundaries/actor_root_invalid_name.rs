#[aura_macros::actor_root(
    owner = "maintenance_root",
    domain = "runtime_maintenance",
    supervision = "maintenance_task_root",
    category = "actor_owned"
)]
pub struct MaintenanceRoot;

fn main() {}
