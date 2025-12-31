//! Agent builder infrastructure.

use super::{AgentConfig, AgentError, AgentResult};
use crate::core::agent::AuraAgent;
use crate::runtime::services::SyncManagerConfig;
use crate::runtime::{EffectContext, EffectSystemBuilder};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId};

/// Builder for creating agents
pub struct AgentBuilder {
    config: AgentConfig,
    authority_id: Option<AuthorityId>,
    sync_config: Option<SyncManagerConfig>,
}

impl AgentBuilder {
    /// Create a new agent builder
    pub fn new() -> Self {
        Self {
            config: AgentConfig::default(),
            authority_id: None,
            sync_config: None,
        }
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

    /// Set the configuration
    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    /// Build a production agent
    pub async fn build_production(self, _ctx: &EffectContext) -> AgentResult<AuraAgent> {
        let sync_config = self.sync_config.clone();
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
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        let runtime = builder
            .build(&temp_context)
            .await
            .map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a testing agent
    pub fn build_testing(self) -> AgentResult<AuraAgent> {
        let sync_config = self.sync_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let mut builder = EffectSystemBuilder::testing()
            .with_config(self.config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        let runtime = builder.build_sync().map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a testing agent using an existing async runtime
    pub async fn build_testing_async(self, ctx: &EffectContext) -> AgentResult<AuraAgent> {
        let sync_config = self.sync_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let mut builder = EffectSystemBuilder::testing()
            .with_config(self.config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        let runtime = builder.build(ctx).await.map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a simulation agent
    pub fn build_simulation(self, seed: u64) -> AgentResult<AuraAgent> {
        let sync_config = self.sync_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let mut builder = EffectSystemBuilder::simulation(seed)
            .with_config(self.config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        let runtime = builder.build_sync().map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a simulation agent using an existing async runtime
    pub async fn build_simulation_async(
        self,
        seed: u64,
        ctx: &EffectContext,
    ) -> AgentResult<AuraAgent> {
        let sync_config = self.sync_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let mut builder = EffectSystemBuilder::simulation(seed)
            .with_config(self.config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        let runtime = builder.build(ctx).await.map_err(AgentError::runtime)?;

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
        let sync_config = self.sync_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let mut builder = EffectSystemBuilder::simulation(seed)
            .with_config(self.config)
            .with_authority(authority_id)
            .with_shared_transport(shared_transport);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        let runtime = builder.build(ctx).await.map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
