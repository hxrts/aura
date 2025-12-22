//! Public Agent API
//!
//! Minimal public API surface for the agent runtime.

use super::{AgentConfig, AgentError, AgentResult, AuthorityContext};
use crate::handlers::{
    AuthService, ChatService, InvitationService, RecoveryService, SessionService,
};
use crate::runtime::services::SyncManagerConfig;
use crate::runtime::services::ThresholdSigningService;
use crate::runtime::system::RuntimeSystem;
use crate::runtime::{EffectContext, EffectSystemBuilder};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId};

/// Main agent interface - thin facade delegating to runtime
///
/// Services are created on-demand as lightweight wrappers around effects.
/// No lazy initialization needed since services are stateless.
pub struct AuraAgent {
    /// The runtime system handling all operations
    runtime: RuntimeSystem,

    /// Authority context for this agent (includes cached account_id)
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

    /// Get the session management service
    ///
    /// Provides access to session creation, management, and lifecycle operations.
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn sessions(&self) -> SessionService {
        SessionService::new(
            self.runtime.effects(),
            self.context.clone(),
            self.context.account_id,
        )
    }

    /// Get the authentication service
    ///
    /// Provides access to authentication operations including challenge-response
    /// flows and device key verification.
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn auth(&self) -> AgentResult<AuthService> {
        AuthService::new(
            self.runtime.effects(),
            self.context.clone(),
            self.context.account_id,
        )
    }

    /// Get the chat service
    ///
    /// Provides access to chat operations including group creation, messaging,
    /// and message history retrieval.
    pub fn chat(&self) -> ChatService {
        ChatService::new(self.runtime.effects())
    }

    /// Get the invitation service
    ///
    /// Provides access to invitation operations including creating, accepting,
    /// and declining invitations for channels, guardians, and contacts.
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn invitations(&self) -> AgentResult<InvitationService> {
        InvitationService::new(self.runtime.effects(), self.context.clone())
    }

    /// Get the recovery service
    ///
    /// Provides access to guardian-based recovery operations including device
    /// addition/removal, tree replacement, and guardian set updates.
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn recovery(&self) -> AgentResult<RecoveryService> {
        RecoveryService::new(self.runtime.effects(), self.context.clone())
    }

    /// Get the threshold signing service
    ///
    /// Provides access to unified threshold signing operations including:
    /// - Multi-device signing (your devices)
    /// - Guardian recovery approvals (cross-authority)
    /// - Group operation approvals (shared authority)
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn threshold_signing(&self) -> ThresholdSigningService {
        ThresholdSigningService::new(self.runtime.effects())
    }

    /// Get the ceremony tracker for guardian ceremony coordination
    ///
    /// The ceremony tracker manages state for in-progress guardian ceremonies,
    /// including tracking which guardians have accepted invitations and whether
    /// the threshold has been reached.
    ///
    /// # Returns
    /// A cloneable reference to the ceremony tracker service
    pub async fn ceremony_tracker(&self) -> crate::runtime::services::CeremonyTracker {
        self.runtime.ceremony_tracker().clone()
    }

    /// Process guardian ceremony acceptances and auto-complete when threshold is reached
    ///
    /// This method should be called periodically (e.g., in a background task) to:
    /// 1. Poll for incoming guardian acceptance messages via transport
    /// 2. Update the ceremony tracker with each acceptance
    /// 3. Automatically commit ceremonies when threshold is reached
    ///
    /// # Returns
    /// Number of acceptances processed and number of ceremonies completed
    pub async fn process_ceremony_acceptances(&self) -> AgentResult<(usize, usize)> {
        // Get recovery service and ceremony tracker
        let recovery_service = self.recovery()?;
        let ceremony_tracker = self.ceremony_tracker().await;

        // Process incoming acceptances from transport
        let acceptances = recovery_service.process_guardian_acceptances().await?;
        let acceptance_count = acceptances.len();
        let mut completed_count = 0;

        // Update ceremony tracker and check for threshold completion
        for (ceremony_id, guardian_id) in acceptances {
            // Clone guardian_id for logging since mark_accepted takes ownership
            let guardian_id_clone = guardian_id.clone();

            match ceremony_tracker
                .mark_accepted(&ceremony_id, guardian_id)
                .await
            {
                Ok(threshold_reached) => {
                    if threshold_reached {
                        tracing::info!(
                            ceremony_id = %ceremony_id,
                            "Ceremony threshold reached - committing guardian key rotation"
                        );

                        // Get ceremony state to retrieve new epoch
                        match ceremony_tracker.get(&ceremony_id).await {
                            Ok(ceremony_state) => {
                                if ceremony_state.is_committed {
                                    continue;
                                }

                                let new_epoch = ceremony_state.new_epoch;
                                let authority_id = self.authority_id();

                                tracing::info!(
                                    ceremony_id = %ceremony_id,
                                    new_epoch,
                                    "Activating new guardian epoch"
                                );

                                // Commit the key rotation to activate the new epoch
                                let commit_result = {
                                    let effects = self.runtime.effects();

                                    use aura_core::effects::ThresholdSigningEffects;
                                    effects
                                        .commit_key_rotation(&authority_id, new_epoch)
                                        .await
                                };

                                match commit_result {
                                    Ok(()) => {
                                        tracing::info!(
                                            ceremony_id = %ceremony_id,
                                            new_epoch,
                                            "Guardian ceremony committed successfully"
                                        );

                                        let _ = ceremony_tracker.mark_committed(&ceremony_id).await;
                                        completed_count += 1;
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            ceremony_id = %ceremony_id,
                                            new_epoch,
                                            error = %e,
                                            "Failed to commit guardian key rotation"
                                        );

                                        // Mark ceremony as failed
                                        let _ = ceremony_tracker
                                            .mark_failed(
                                                &ceremony_id,
                                                Some(format!("Commit failed: {}", e)),
                                            )
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    ceremony_id = %ceremony_id,
                                    error = %e,
                                    "Failed to retrieve ceremony state for commit"
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        ceremony_id = %ceremony_id,
                        guardian_id = %guardian_id_clone,
                        error = %e,
                        "Failed to mark guardian as accepted"
                    );
                }
            }
        }

        Ok((acceptance_count, completed_count))
    }

    /// Shutdown the agent
    pub async fn shutdown(self, ctx: &EffectContext) -> AgentResult<()> {
        self.runtime
            .shutdown(ctx)
            .await
            .map_err(AgentError::runtime)
    }
}

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
        let runtime = builder.build(&temp_context).await.map_err(AgentError::runtime)?;

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
