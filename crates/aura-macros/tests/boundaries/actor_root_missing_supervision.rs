#[aura_macros::actor_root(
    owner = "maintenance_service",
    domain = "runtime_maintenance",
    category = "actor_owned"
)]
pub struct RuntimeMaintenanceService;

fn main() {}
