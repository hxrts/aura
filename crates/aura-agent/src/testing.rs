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
