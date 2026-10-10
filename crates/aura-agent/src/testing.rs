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

/// Selected physical provider with explicit native read-failure injection.
pub(crate) struct NativeReadFaultClock {
    clock: aura_testkit::time::ManualPhysicalClock,
    failed: std::sync::atomic::AtomicBool,
}
#[derive(Debug, thiserror::Error)]
#[error("injected selected physical read failure")]
pub(crate) struct NativePhysicalReadFault;
impl NativeReadFaultClock {
    pub(crate) fn new(now_ms: u64) -> Self {
        Self {
            clock: aura_testkit::time::ManualPhysicalClock::new(now_ms),
            failed: std::sync::atomic::AtomicBool::new(false),
        }
    }
    pub(crate) fn fail_reads(&self) {
        self.failed.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
#[async_trait::async_trait]
impl aura_core::effects::PhysicalTimeEffects for NativeReadFaultClock {
    async fn physical_time(
        &self,
    ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
        if self.failed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(aura_core::effects::TimeError::ProviderFailure {
                operation: aura_core::effects::time::TimeProviderOperation::ReadPhysicalClock,
                source: Some(std::sync::Arc::new(NativePhysicalReadFault)),
            });
        }
        aura_core::effects::PhysicalTimeEffects::physical_time(&self.clock).await
    }
    async fn sleep_ms(&self, ms: u64) -> Result<(), aura_core::effects::TimeError> {
        aura_core::effects::PhysicalTimeEffects::sleep_ms(&self.clock, ms).await
    }
    async fn wait_until_physical_deadline(
        &self,
        endpoint: aura_core::types::window::WindowPosition<
            aura_core::types::window::PhysicalMillis,
        >,
    ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
        aura_core::effects::PhysicalTimeEffects::wait_until_physical_deadline(&self.clock, endpoint)
            .await
    }
}
