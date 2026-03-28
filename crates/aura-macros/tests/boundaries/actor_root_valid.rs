use aura_core::OwnedShutdownToken;

#[aura_macros::actor_root(
    owner = "maintenance_service",
    domain = "runtime_maintenance",
    supervision = "maintenance_task_root",
    category = "actor_owned"
)]
pub struct RuntimeMaintenanceService {
    state: u8,
}

fn main() {
    let declaration = RuntimeMaintenanceService::actor_root_declaration();
    assert_eq!(declaration.owner_name(), "maintenance_service");
    assert_eq!(declaration.domain_name(), "runtime_maintenance");
    assert_eq!(declaration.supervision_gate(), "maintenance_task_root");
    let _registration = RuntimeMaintenanceService::register_actor_root_supervision(
        7u8,
        OwnedShutdownToken::detached(),
    );
    let _service = RuntimeMaintenanceService { state: 1 };
}
