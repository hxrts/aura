//! Public Agent API
//!
//! Minimal public API surface for the agent runtime.

use super::{AgentConfig, AgentError, AgentResult, AuthorityContext};
use crate::runtime::{EffectSystemBuilder, RuntimeSystem};
use aura_core::identifiers::AuthorityId;

/// Main agent interface - thin facade delegating to runtime
pub struct AuraAgent {
    /// The runtime system handling all operations
    runtime: RuntimeSystem,

    /// Authority context for this agent
    context: AuthorityContext,
}

impl AuraAgent {
    /// Create a new agent with the given runtime system
    pub(crate) fn new(runtime: RuntimeSystem, authority_id: AuthorityId) -> Self {
        Self {
            runtime,
            context: AuthorityContext::new(authority_id),
        }
    }

    /// Get the authority ID for this agent
    pub fn authority_id(&self) -> AuthorityId {
        self.context.authority_id
    }

    /// Get the authority context (read-only)
    pub fn context(&self) -> &AuthorityContext {
        &self.context
    }

    /// Access the runtime system (for advanced operations)
    pub fn runtime(&self) -> &RuntimeSystem {
        &self.runtime
    }

    /// Shutdown the agent
    pub async fn shutdown(self) -> AgentResult<()> {
        self.runtime.shutdown().await.map_err(AgentError::runtime)
    }
}

/// Builder for creating agents
pub struct AgentBuilder {
    config: AgentConfig,
    authority_id: Option<AuthorityId>,
}

impl AgentBuilder {
    /// Create a new agent builder
    pub fn new() -> Self {
        Self {
            config: AgentConfig::default(),
            authority_id: None,
        }
    }

    /// Set the authority ID
    pub fn with_authority(mut self, authority_id: AuthorityId) -> Self {
        self.authority_id = Some(authority_id);
        self
    }

    /// Set the configuration
    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    /// Build a production agent
    pub async fn build_production(self) -> AgentResult<AuraAgent> {
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let runtime = EffectSystemBuilder::production()
            .with_config(self.config)
            .with_authority(authority_id)
            .build()
            .await
            .map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a testing agent
    pub fn build_testing(self) -> AgentResult<AuraAgent> {
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let runtime = EffectSystemBuilder::testing()
            .with_config(self.config)
            .with_authority(authority_id)
            .build_sync()
            .map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a simulation agent
    pub fn build_simulation(self, seed: u64) -> AgentResult<AuraAgent> {
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let runtime = EffectSystemBuilder::simulation(seed)
            .with_config(self.config)
            .with_authority(authority_id)
            .build_sync()
            .map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
