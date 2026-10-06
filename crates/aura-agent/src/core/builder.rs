//! Agent builder infrastructure.

use super::{AgentConfig, AgentError, AgentResult};
use crate::core::agent::AuraAgent;
use crate::runtime::services::{RendezvousManagerConfig, SyncManagerConfig};
use crate::runtime::{EffectContext, EffectSystemBuilder};
use aura_core::hash::hash;
use aura_core::types::identifiers::{AuthorityId, ContextId};

/// Builder for creating agents
pub struct AgentBuilder {
    config: AgentConfig,
    authority_id: Option<AuthorityId>,
    sync_config: Option<SyncManagerConfig>,
    rendezvous_config: Option<RendezvousManagerConfig>,
    profile_owner: Option<std::sync::Arc<aura_effects::profile_storage::OwnedProfileLease>>,
}

impl AgentBuilder {
    /// Create a new agent builder
    pub fn new() -> Self {
        Self {
            config: AgentConfig::default(),
            authority_id: None,
            sync_config: None,
            rendezvous_config: None,
            profile_owner: None,
        }
    }

    /// Share the actual bootstrap provider resource with production assembly.
    pub fn with_profile_owner(
        mut self,
        owner: std::sync::Arc<aura_effects::profile_storage::OwnedProfileLease>,
    ) -> Self {
        self.profile_owner = Some(owner);
        self
    }

    fn reject_profile_in_nonproduction(&self) -> AgentResult<()> {
        if self.profile_owner.is_some() {
            return Err(AgentError::from(aura_core::AuraError::Storage {
                message: "production profile resource requires production runtime assembly".into(),
                source: Some(std::sync::Arc::new(
                    aura_core::effects::profile_storage::ProfileStorageError::Invalid(
                        "profile resource supplied to nonproduction assembly".into(),
                    ),
                )),
            }));
        }
        Ok(())
    }

    /// Set the authority ID
    pub fn with_authority(mut self, authority_id: AuthorityId) -> Self {
        self.authority_id = Some(authority_id);
        self
    }

    /// Enable the sync service with default configuration.
    pub fn with_sync(mut self) -> Self {
        self.sync_config = Some(SyncManagerConfig::default());
        self
    }

    /// Enable the sync service with a custom configuration.
    pub fn with_sync_config(mut self, config: SyncManagerConfig) -> Self {
        self.sync_config = Some(config);
        self
    }

    /// Enable the rendezvous service with default configuration.
    pub fn with_rendezvous(mut self) -> Self {
        self.rendezvous_config = Some(RendezvousManagerConfig::default());
        self
    }

    /// Enable the rendezvous service with a custom configuration.
    pub fn with_rendezvous_config(mut self, config: RendezvousManagerConfig) -> Self {
        self.rendezvous_config = Some(config);
        self
    }

    /// Set the configuration
    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    /// Build a production agent
    pub async fn build_production(self, _ctx: &EffectContext) -> AgentResult<AuraAgent> {
        let sync_config = self.sync_config.clone().unwrap_or_default();
        let rendezvous_config = self.rendezvous_config.clone().unwrap_or_default();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        // Build-time context used only for effect wiring
        let context_entropy = hash(&authority_id.to_bytes());
        let temp_context = EffectContext::new(
            authority_id,
            ContextId::new_from_entropy(context_entropy),
            aura_core::effects::ExecutionMode::Production,
        );

        let mut builder = EffectSystemBuilder::production()
            .with_config(self.config)
            .with_authority(authority_id);
        if let Some(owner) = self.profile_owner {
            builder = builder.with_profile_owner(owner);
        }
        builder = builder
            .with_sync_config(sync_config)
            .with_rendezvous_config(rendezvous_config);
        let runtime = builder.build(&temp_context).await.map_err(|source| {
            AgentError::from(aura_core::AuraError::Internal {
                message: "assemble original owned production profile".into(),
                source: Some(std::sync::Arc::new(source)),
            })
        })?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a testing agent
    pub fn build_testing(self) -> AgentResult<AuraAgent> {
        self.reject_profile_in_nonproduction()?;
        let sync_config = self.sync_config.clone();
        let rendezvous_config = self.rendezvous_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let (config, profile) = Self::owned_nonproduction_profile(self.config)?;
        let mut builder = EffectSystemBuilder::testing_with_owned_profile(profile)
            .with_config(config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        if let Some(rendezvous_config) = rendezvous_config {
            builder = builder.with_rendezvous_config(rendezvous_config);
        }
        let runtime = builder.build_sync().map_err(AgentError::from)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a testing agent using an existing async runtime
    pub async fn build_testing_async(self, ctx: &EffectContext) -> AgentResult<AuraAgent> {
        self.reject_profile_in_nonproduction()?;
        let sync_config = self.sync_config.clone();
        let rendezvous_config = self.rendezvous_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let (config, profile) = Self::owned_nonproduction_profile(self.config)?;
        let mut builder = EffectSystemBuilder::testing_with_owned_profile(profile)
            .with_config(config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        if let Some(rendezvous_config) = rendezvous_config {
            builder = builder.with_rendezvous_config(rendezvous_config);
        }
        let runtime = builder.build(ctx).await.map_err(AgentError::from)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a simulation agent
    pub fn build_simulation(self, seed: u64) -> AgentResult<AuraAgent> {
        self.reject_profile_in_nonproduction()?;
        let sync_config = self.sync_config.clone();
        let rendezvous_config = self.rendezvous_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let mut builder = EffectSystemBuilder::simulation(seed)
            .with_config(self.config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        if let Some(rendezvous_config) = rendezvous_config {
            builder = builder.with_rendezvous_config(rendezvous_config);
        }
        let runtime = builder.build_sync().map_err(AgentError::from)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a simulation agent using an existing async runtime
    pub async fn build_simulation_async(
        self,
        seed: u64,
        ctx: &EffectContext,
    ) -> AgentResult<AuraAgent> {
        self.reject_profile_in_nonproduction()?;
        let sync_config = self.sync_config.clone();
        let rendezvous_config = self.rendezvous_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let (config, profile) = Self::owned_nonproduction_profile(self.config)?;
        let mut builder = EffectSystemBuilder::simulation_with_owned_profile(seed, profile)
            .with_config(config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        if let Some(rendezvous_config) = rendezvous_config {
            builder = builder.with_rendezvous_config(rendezvous_config);
        }
        let runtime = builder.build(ctx).await.map_err(AgentError::from)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a simulation agent with shared transport inbox for multi-agent scenarios
    ///
    /// This enables communication between multiple simulated agents (e.g., Bob, Alice, Carol)
    /// by providing a shared transport layer that routes messages based on destination authority.
    pub async fn build_simulation_async_with_shared_transport(
        self,
        seed: u64,
        ctx: &EffectContext,
        shared_transport: crate::SharedTransport,
    ) -> AgentResult<AuraAgent> {
        self.reject_profile_in_nonproduction()?;
        // Multi-agent simulation runs the production sync service, as
        // `build_production` does; its timers use the runtime's (simulated)
        // time effects.
        let sync_config = self.sync_config.clone().unwrap_or_default();
        let rendezvous_config = self.rendezvous_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let (config, profile) = Self::owned_nonproduction_profile(self.config)?;
        let mut builder = EffectSystemBuilder::simulation_with_owned_profile(seed, profile)
            .with_config(config)
            .with_authority(authority_id)
            .with_shared_transport(shared_transport)
            .with_sync_config(sync_config);
        if let Some(rendezvous_config) = rendezvous_config {
            builder = builder.with_rendezvous_config(rendezvous_config);
        }
        let runtime = builder.build(ctx).await.map_err(AgentError::from)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }
}

impl AgentBuilder {
    /// Testing and simulation agents own an isolated profile directory, so
    /// enrollment and key rotation run under original selected secret
    /// custody. The configuration is normalized first so the lease and the
    /// runtime select the same directory.
    fn owned_nonproduction_profile(
        config: AgentConfig,
    ) -> AgentResult<(
        AgentConfig,
        crate::runtime::builder::TestingOwnedProfileCapability,
    )> {
        let config = crate::runtime::AuraEffectSystem::normalized_nonproduction_config(config)?;
        let profile = crate::runtime::builder::TestingOwnedProfileCapability::acquire(&config)?;
        Ok((config, profile))
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
