use std::sync::Arc;

use crate::core::AgentConfig;
use crate::{AuraEffectSystem, AuthorityId, SharedTransport};

#[track_caller]
pub(crate) fn simulation_effect_system(config: &AgentConfig) -> AuraEffectSystem {
    AuraEffectSystem::simulation_for_test(config).unwrap_or_else(|error| panic!("{error}"))
}

#[track_caller]
pub(crate) fn simulation_effect_system_arc(config: &AgentConfig) -> Arc<AuraEffectSystem> {
    Arc::new(simulation_effect_system(config))
}

#[track_caller]
pub(crate) fn simulation_effect_system_for_authority(
    config: &AgentConfig,
    authority: AuthorityId,
) -> AuraEffectSystem {
    AuraEffectSystem::simulation_for_test_for_authority(config, authority)
        .unwrap_or_else(|error| panic!("{error}"))
}

#[track_caller]
pub(crate) fn simulation_effect_system_for_authority_arc(
    config: &AgentConfig,
    authority: AuthorityId,
) -> Arc<AuraEffectSystem> {
    Arc::new(simulation_effect_system_for_authority(config, authority))
}

#[track_caller]
pub(crate) fn simulation_effect_system_with_shared_transport_for_authority(
    config: &AgentConfig,
    authority: AuthorityId,
    shared_transport: SharedTransport,
) -> AuraEffectSystem {
    AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
        config,
        authority,
        shared_transport,
    )
    .unwrap_or_else(|error| panic!("{error}"))
}

#[track_caller]
pub(crate) fn simulation_effect_system_with_shared_transport_for_authority_arc(
    config: &AgentConfig,
    authority: AuthorityId,
    shared_transport: SharedTransport,
) -> Arc<AuraEffectSystem> {
    Arc::new(
        simulation_effect_system_with_shared_transport_for_authority(
            config,
            authority,
            shared_transport,
        ),
    )
}

/// Serializes tests that set, or depend on the absence of, process-wide
/// harness environment variables (`AURA_HARNESS_MODE`): runtime assembly reads
/// them, so a concurrent setter changes another test's production checks.
pub(crate) fn harness_env_lock() -> &'static async_lock::Mutex<()> {
    static LOCK: std::sync::OnceLock<async_lock::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| async_lock::Mutex::new(()))
}
