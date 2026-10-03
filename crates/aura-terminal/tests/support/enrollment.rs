//! Real provisional invitee setup shared by enrollment integration tests.

use std::path::Path;
use std::sync::Arc;

use aura_agent::core::config::StorageConfig;
use aura_agent::core::{AgentBuilder, AgentConfig};
use aura_agent::{AuraAgent, EffectContext, SharedTransport};
use aura_core::effects::ExecutionMode;
use aura_core::DeviceId;

pub async fn provisional_invitee_setup(
    storage_path: &Path,
    device_id: DeviceId,
    seed: u64,
    shared_transport: SharedTransport,
) -> (Arc<AuraAgent>, String) {
    let (authority, context) =
        aura_terminal::handlers::tui::create_account(storage_path, "EnrollmentInvitee")
            .await
            .expect("create actual provisional invitee account");
    let effects = EffectContext::new(authority, context, ExecutionMode::Simulation { seed });
    let agent = Arc::new(
        AgentBuilder::new()
            .with_config(AgentConfig {
                device_id,
                storage: StorageConfig {
                    base_path: storage_path.to_path_buf(),
                    ..StorageConfig::default()
                },
                ..AgentConfig::default()
            })
            .with_authority(authority)
            .build_simulation_async_with_shared_transport(seed, &effects, shared_transport)
            .await
            .expect("build actual provisional invitee runtime"),
    );
    let bridge = agent.clone().as_runtime_bridge();
    bridge
        .bootstrap_signing_keys()
        .await
        .expect("invitee signing ready");
    let setup_code = bridge
        .export_device_enrollment_setup_request()
        .await
        .expect("export actual invitee signing statement");
    (agent, setup_code)
}
