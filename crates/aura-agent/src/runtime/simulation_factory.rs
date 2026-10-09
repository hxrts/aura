//! Simulation Environment Factory Implementation
//!
//! This module provides the `SimulationEnvironmentFactory` implementation for creating
//! `AuraEffectSystem` instances suitable for simulation. This enables the simulator
//! to work through a trait-based abstraction rather than directly importing concrete types.
//!
//! # Architecture
//!
//! The factory pattern decouples the simulator (Layer 8) from the agent's effect system
//! internals (Layer 6), following the dependency inversion principle:
//!
//! ```text
//! aura-core (Layer 1)          aura-agent (Layer 6)          aura-simulator (Layer 8)
//! ┌────────────────────┐       ┌────────────────────┐        ┌────────────────────┐
//! │ SimulationEnv-     │       │ AuraEffectSystem   │        │ Uses factory via   │
//! │ vironmentFactory   │◄──────│ EffectSystemFactory│◄───────│ trait bounds       │
//! │ (trait)            │       │ (impl)             │        │                    │
//! └────────────────────┘       └────────────────────┘        └────────────────────┘
//! ```
//!
//! # Blocking Lock Usage
//!
//! Uses `parking_lot::RwLock` for shared simulation transport because this is
//! test/simulation infrastructure with brief sync-only operations. See
//! `SharedTransport` documentation for details.

#![allow(clippy::disallowed_types)]

cfg_if::cfg_if! {
    if #[cfg(feature = "simulation")] {
        use aura_core::effects::{
            SimulationEnvironmentConfig, SimulationEnvironmentError, SimulationEnvironmentFactory,
            TransportEnvelope,
        };
        use std::sync::Arc;

        use super::effects::AuraEffectSystem;
        use crate::core::AgentConfig;
        use parking_lot::RwLock;
    }
}

/// Factory for creating `AuraEffectSystem` instances for simulation
///
/// This factory implements the `SimulationEnvironmentFactory` trait from `aura-core`,
/// allowing the simulator to create effect systems without directly depending on
/// `AuraEffectSystem` internals.
///
/// # Example
///
/// ```rust,ignore
/// use aura_agent::runtime::EffectSystemFactory;
/// use aura_core::effects::{SimulationEnvironmentFactory, SimulationEnvironmentConfig};
///
/// async fn run_simulation<F: SimulationEnvironmentFactory>(factory: &F) {
///     let config = SimulationEnvironmentConfig::new(42, device_id, authority_id);
///     let effects = factory.create_simulation_environment(config).await?;
///     // Use effects...
/// }
///
/// // Create factory and run simulation
/// let factory = EffectSystemFactory::default();
/// run_simulation(&factory).await;
/// ```
#[cfg(feature = "simulation")]
#[derive(Debug, Clone, Default)]
pub struct EffectSystemFactory {
    /// Base configuration for created effect systems
    base_config: AgentConfig,
}

#[cfg(feature = "simulation")]
impl EffectSystemFactory {
    /// Create a new factory with default configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new factory with custom base configuration
    pub fn with_config(config: AgentConfig) -> Self {
        Self {
            base_config: config,
        }
    }

    /// Convert simulation config to agent config
    fn to_agent_config(&self, config: &SimulationEnvironmentConfig) -> AgentConfig {
        let mut agent_config = self.base_config.clone();
        agent_config.device_id = config.device_id;
        agent_config
    }
}

#[cfg(feature = "simulation")]
#[async_trait::async_trait]
impl SimulationEnvironmentFactory for EffectSystemFactory {
    type EffectSystem = AuraEffectSystem;

    async fn create_simulation_environment(
        &self,
        config: SimulationEnvironmentConfig,
    ) -> Result<Arc<Self::EffectSystem>, SimulationEnvironmentError> {
        let agent_config = self.to_agent_config(&config);

        // Simulation factory is runtime infrastructure and intentionally uses
        // explicit simulation constructors with externally provided seeds.
        #[allow(clippy::disallowed_methods)]
        let effect_system = AuraEffectSystem::simulation_for_authority(
            &agent_config,
            config.seed,
            config.authority_id,
        )
        .map_err(|e| SimulationEnvironmentError::CreationFailed(e.to_string()))?;

        Ok(Arc::new(effect_system))
    }

    async fn create_simulation_environment_with_shared_transport(
        &self,
        config: SimulationEnvironmentConfig,
        shared_inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    ) -> Result<Arc<Self::EffectSystem>, SimulationEnvironmentError> {
        let agent_config = self.to_agent_config(&config);

        let effect_system = AuraEffectSystem::simulation_with_shared_inbox_for_authority(
            &agent_config,
            config.seed,
            config.authority_id,
            shared_inbox,
        )
        .map_err(|e| SimulationEnvironmentError::CreationFailed(e.to_string()))?;

        Ok(Arc::new(effect_system))
    }
}

/// Drive a membership rekey with a closed set of simulation runtimes.
///
/// Every supplied runtime must observe the exact same canonical membership.
/// The native owners exchange device keys, run DKG and authenticate consensus
/// witnesses; this adapter neither supplies keys nor constructs certificates.
/// All peer drivers are lexically owned and cancelled when the attempt ends.
#[cfg(feature = "simulation")]
pub async fn rekey_simulated_channel(
    context: aura_core::ContextId,
    channel: aura_core::types::identifiers::ChannelId,
    participants: &[Arc<AuraEffectSystem>],
) -> Result<aura_amp::ChannelEpochCommitFact, aura_core::AuraError> {
    use super::channel_consensus::ChannelConsensusWitness;
    use super::channel_key_ceremony::{process_channel_key_invites, ChannelKeyInvite};
    use super::channel_rekey::{rekey_channel, REKEY_MAX_POLLS};
    use super::context_dkg::{load_roster, ChannelKeyScope};
    use super::device_key_exchange::{process_device_key_messages, runtime_known_peer};
    use aura_core::effects::{ExecutionMode, PhysicalTimeEffects};
    use aura_core::AuraError;
    use aura_journal::DomainFact;
    use aura_protocol::amp::AmpJournalEffects;
    use aura_protocol::effects::AuraEffects;
    use std::collections::BTreeSet;

    if participants.len() < 2
        || participants
            .iter()
            .any(|effects| !matches!(effects.execution_mode(), ExecutionMode::Simulation { .. }))
    {
        return Err(AuraError::invalid(
            "rekey requires simulation member runtimes",
        ));
    }
    let scope = ChannelKeyScope { context, channel };
    let members: BTreeSet<_> = participants
        .iter()
        .map(|effects| aura_guards::GuardContextProvider::authority_id(effects.as_ref()))
        .collect();
    if members.len() != participants.len() {
        return Err(AuraError::invalid("duplicate simulation rekey member"));
    }
    let state =
        aura_protocol::amp::get_channel_state(participants[0].as_ref(), context, channel).await?;
    for effects in participants {
        let observed: BTreeSet<_> =
            aura_amp::channel_membership_observations(effects.as_ref(), context, channel)
                .await?
                .participants()
                .collect();
        let peer_state =
            aura_protocol::amp::get_channel_state(effects.as_ref(), context, channel).await?;
        // Message generations and locally retained bootstrap metadata need
        // not match. Membership and the parent key epoch must agree before
        // this attempt; the coordinator retains the original parent roster.
        if observed != members || peer_state.chan_epoch != state.chan_epoch {
            return Err(AuraError::invalid(format!(
                "simulation rekey canonical state disagreement: observed={observed:?}, expected={members:?}, epoch={}, expected_epoch={}",
                peer_state.chan_epoch, state.chan_epoch,
            )));
        }
    }
    let roster: BTreeSet<_> = if state.chan_epoch == 0 {
        let bootstrap = state
            .bootstrap
            .as_ref()
            .ok_or_else(|| AuraError::not_found("current channel bootstrap roster"))?;
        bootstrap
            .recipients
            .iter()
            .copied()
            .chain(std::iter::once(bootstrap.dealer))
            .collect()
    } else {
        load_roster(participants[0].as_ref(), scope, state.chan_epoch)
            .await?
            .0
            .participants
            .into_iter()
            .collect()
    };
    let coordinator_id = roster
        .intersection(&members)
        .next()
        .copied()
        .ok_or_else(|| AuraError::permission_denied("no retained member can coordinate rekey"))?;
    let coordinator = participants
        .iter()
        .find(|effects| {
            aura_guards::GuardContextProvider::authority_id(effects.as_ref()) == coordinator_id
        })
        .ok_or_else(|| AuraError::not_found("simulation coordinator"))?;
    let peers = participants.iter().filter(|effects| {
        aura_guards::GuardContextProvider::authority_id(effects.as_ref()) != coordinator_id
    });
    let peer_drivers = futures::future::try_join_all(peers.map(|effects| async move {
        let witness = ChannelConsensusWitness::default();
        for _ in 0..REKEY_MAX_POLLS {
            process_device_key_messages(effects, |peer, scope| {
                runtime_known_peer(effects, peer, scope)
            })
            .await?;
            let outcomes = process_channel_key_invites(
                effects,
                |invite: ChannelKeyInvite| async move {
                    let observed = aura_amp::channel_membership_observations(
                        effects.as_ref(),
                        invite.scope.context,
                        invite.scope.channel,
                    )
                    .await?;
                    Ok(invite.scope == scope
                        && observed.has_standing(invite.coordinator)
                        && invite
                            .participants
                            .iter()
                            .all(|member| observed.has_standing(*member)))
                },
                REKEY_MAX_POLLS,
            )
            .await?;
            for (_, outcome) in outcomes {
                outcome?;
            }
            witness
                .process(effects, |requested, from| async move {
                    Ok(requested == scope
                        && aura_amp::channel_membership_observations(
                            effects.as_ref(),
                            context,
                            channel,
                        )
                        .await?
                        .has_standing(from))
                })
                .await?;
            effects.sleep_ms(50).await?;
        }
        Err::<(), AuraError>(AuraError::internal(
            "simulation rekey peer deadline exceeded",
        ))
    }));
    let fact = tokio::select! {
        result = Box::pin(rekey_channel(coordinator, scope, state.chan_epoch, &roster, members, REKEY_MAX_POLLS)) => result?,
        result = peer_drivers => {
            result?;
            return Err(AuraError::internal("simulation rekey peers ended before agreement"));
        }
    };
    for effects in participants {
        let (_, public) = load_roster(effects, scope, state.chan_epoch + 1).await?;
        fact.verify_with(&aura_core::crypto::tree_signing::PublicKeyPackage::from(
            public,
        ))?;
        effects
            .insert_relational_fact(fact.committed_bump_fact())
            .await?;
        effects
            .commit_relational_facts(vec![fact.to_generic()])
            .await?;
    }
    Ok(fact)
}

#[cfg(all(test, feature = "simulation"))]
mod tests {
    use super::*;
    use aura_core::effects::RuntimeEffectsBundle;
    use aura_core::{AuthorityId, DeviceId};
    use parking_lot::RwLock;

    #[tokio::test]
    async fn test_factory_creates_effect_system() {
        let factory = EffectSystemFactory::new();
        let device_id = DeviceId::new_from_entropy([1u8; 32]);
        let authority_id = AuthorityId::new_from_entropy([11u8; 32]);
        let config = SimulationEnvironmentConfig::new(42, device_id, authority_id);

        let result = factory.create_simulation_environment(config).await;
        assert!(result.is_ok());

        let effects = match result {
            Ok(effects) => effects,
            Err(err) => panic!("failed to create simulation environment: {err:?}"),
        };
        assert!(effects.is_simulation_mode());
    }

    #[tokio::test]
    async fn test_factory_with_shared_transport() {
        let factory = EffectSystemFactory::new();
        let device_id = DeviceId::new_from_entropy([2u8; 32]);
        let authority_id = AuthorityId::new_from_entropy([12u8; 32]);
        let config = SimulationEnvironmentConfig::new(42, device_id, authority_id);
        let shared_inbox = Arc::new(RwLock::new(Vec::new()));

        let result = factory
            .create_simulation_environment_with_shared_transport(config, shared_inbox)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_factory_with_explicit_authority() {
        let factory = EffectSystemFactory::new();
        let device_id = DeviceId::new_from_entropy([3u8; 32]);
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
        let config = SimulationEnvironmentConfig::new(42, device_id, authority_id);

        let result = factory.create_simulation_environment(config).await;
        assert!(result.is_ok());
    }
}
