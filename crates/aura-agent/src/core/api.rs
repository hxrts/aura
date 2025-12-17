//! Public Agent API
//!
//! Minimal public API surface for the agent runtime.

use super::{AgentConfig, AgentError, AgentResult, AuthorityContext};
use crate::handlers::{
    AuthService, ChatService, InvitationService, RecoveryService, SessionService,
};
use crate::runtime::services::ThresholdSigningService;
use crate::runtime::system::RuntimeSystem;
use crate::runtime::{EffectContext, EffectSystemBuilder};
use aura_core::{
    hash::hash,
    identifiers::{AccountId, AuthorityId, ContextId},
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Main agent interface - thin facade delegating to runtime
pub struct AuraAgent {
    /// The runtime system handling all operations
    runtime: RuntimeSystem,

    /// Authority context for this agent
    context: AuthorityContext,

    /// Session management service (lazily initialized)
    session_service: Arc<RwLock<Option<SessionService>>>,

    /// Authentication service (lazily initialized)
    auth_service: Arc<RwLock<Option<AuthService>>>,

    /// Invitation service (lazily initialized)
    invitation_service: Arc<RwLock<Option<InvitationService>>>,

    /// Recovery service (lazily initialized)
    recovery_service: Arc<RwLock<Option<RecoveryService>>>,

    /// Threshold signing service (lazily initialized)
    threshold_signing_service: Arc<RwLock<Option<ThresholdSigningService>>>,
}

impl AuraAgent {
    /// Create a new agent with the given runtime system
    pub(crate) fn new(runtime: RuntimeSystem, authority_id: AuthorityId) -> Self {
        Self {
            runtime,
            context: AuthorityContext::new(authority_id),
            session_service: Arc::new(RwLock::new(None)),
            auth_service: Arc::new(RwLock::new(None)),
            invitation_service: Arc::new(RwLock::new(None)),
            recovery_service: Arc::new(RwLock::new(None)),
            threshold_signing_service: Arc::new(RwLock::new(None)),
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
    pub async fn sessions(&self) -> SessionService {
        // Check if already initialized
        {
            let guard = self.session_service.read().await;
            if guard.is_some() {
                return SessionService::new(
                    self.runtime.effects(),
                    self.context.clone(),
                    AccountId::new_from_entropy(hash(&self.context.authority_id.to_bytes())),
                );
            }
        }

        // Initialize lazily
        let service = SessionService::new(
            self.runtime.effects(),
            self.context.clone(),
            AccountId::new_from_entropy(hash(&self.context.authority_id.to_bytes())),
        );

        // Store for future use (though we return a new instance each time for simplicity)
        {
            let mut guard = self.session_service.write().await;
            *guard = Some(SessionService::new(
                self.runtime.effects(),
                self.context.clone(),
                AccountId::new_from_entropy(hash(&self.context.authority_id.to_bytes())),
            ));
        }

        service
    }

    /// Get the authentication service
    ///
    /// Provides access to authentication operations including challenge-response
    /// flows and device key verification.
    pub async fn auth(&self) -> AgentResult<AuthService> {
        // Check if already initialized
        {
            let guard = self.auth_service.read().await;
            if guard.is_some() {
                return AuthService::new(
                    self.runtime.effects(),
                    self.context.clone(),
                    AccountId::new_from_entropy(hash(&self.context.authority_id.to_bytes())),
                );
            }
        }

        // Initialize lazily
        let service = AuthService::new(
            self.runtime.effects(),
            self.context.clone(),
            AccountId::new_from_entropy(hash(&self.context.authority_id.to_bytes())),
        )?;

        // Store for future use
        {
            let mut guard = self.auth_service.write().await;
            *guard = Some(AuthService::new(
                self.runtime.effects(),
                self.context.clone(),
                AccountId::new_from_entropy(hash(&self.context.authority_id.to_bytes())),
            )?);
        }

        Ok(service)
    }

    /// Get the chat service
    ///
    /// Provides access to chat operations including group creation, messaging,
    /// and message history retrieval.
    pub fn chat(&self) -> ChatService {
        // ChatService is simple - just wraps the effects with the handler
        // No lazy initialization needed since it's stateless
        ChatService::new(self.runtime.effects())
    }

    /// Get the invitation service
    ///
    /// Provides access to invitation operations including creating, accepting,
    /// and declining invitations for channels, guardians, and contacts.
    pub async fn invitations(&self) -> AgentResult<InvitationService> {
        // Check if already initialized
        {
            let guard = self.invitation_service.read().await;
            if guard.is_some() {
                return InvitationService::new(self.runtime.effects(), self.context.clone());
            }
        }

        // Initialize lazily
        let service = InvitationService::new(self.runtime.effects(), self.context.clone())?;

        // Store for future use
        {
            let mut guard = self.invitation_service.write().await;
            *guard = Some(InvitationService::new(
                self.runtime.effects(),
                self.context.clone(),
            )?);
        }

        Ok(service)
    }

    /// Get the recovery service
    ///
    /// Provides access to guardian-based recovery operations including device
    /// addition/removal, tree replacement, and guardian set updates.
    pub async fn recovery(&self) -> AgentResult<RecoveryService> {
        // Check if already initialized
        {
            let guard = self.recovery_service.read().await;
            if guard.is_some() {
                return RecoveryService::new(self.runtime.effects(), self.context.clone());
            }
        }

        // Initialize lazily
        let service = RecoveryService::new(self.runtime.effects(), self.context.clone())?;

        // Store for future use
        {
            let mut guard = self.recovery_service.write().await;
            *guard = Some(RecoveryService::new(
                self.runtime.effects(),
                self.context.clone(),
            )?);
        }

        Ok(service)
    }

    /// Get the threshold signing service
    ///
    /// Provides access to unified threshold signing operations including:
    /// - Multi-device signing (your devices)
    /// - Guardian recovery approvals (cross-authority)
    /// - Group operation approvals (shared authority)
    pub async fn threshold_signing(&self) -> ThresholdSigningService {
        // Check if already initialized (note: we return a new instance each time
        // since the service maintains state via Arc internally)
        {
            let guard = self.threshold_signing_service.read().await;
            if guard.is_some() {
                return ThresholdSigningService::new(self.runtime.effects());
            }
        }

        // Initialize lazily
        let service = ThresholdSigningService::new(self.runtime.effects());

        // Store for future use (though we return a new instance each time for simplicity)
        {
            let mut guard = self.threshold_signing_service.write().await;
            *guard = Some(ThresholdSigningService::new(self.runtime.effects()));
        }

        service
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
        let recovery_service = self.recovery().await?;
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
                                    let effects_guard = effects.read().await;

                                    use aura_core::effects::ThresholdSigningEffects;
                                    effects_guard
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
                                            .mark_failed(&ceremony_id, Some(format!("Commit failed: {}", e)))
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
    pub async fn build_production(self, _ctx: &EffectContext) -> AgentResult<AuraAgent> {
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

        let runtime = EffectSystemBuilder::production()
            .with_config(self.config)
            .with_authority(authority_id)
            .build(&temp_context)
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

    /// Build a testing agent using an existing async runtime
    pub async fn build_testing_async(self, ctx: &EffectContext) -> AgentResult<AuraAgent> {
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let runtime = EffectSystemBuilder::testing()
            .with_config(self.config)
            .with_authority(authority_id)
            .build(ctx)
            .await
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

    /// Build a simulation agent using an existing async runtime
    pub async fn build_simulation_async(
        self,
        seed: u64,
        ctx: &EffectContext,
    ) -> AgentResult<AuraAgent> {
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let runtime = EffectSystemBuilder::simulation(seed)
            .with_config(self.config)
            .with_authority(authority_id)
            .build(ctx)
            .await
            .map_err(AgentError::runtime)?;

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
        shared_inbox: std::sync::Arc<std::sync::RwLock<Vec<aura_core::effects::TransportEnvelope>>>,
    ) -> AgentResult<AuraAgent> {
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let runtime = EffectSystemBuilder::simulation(seed)
            .with_config(self.config)
            .with_authority(authority_id)
            .with_shared_transport_inbox(shared_inbox)
            .build(ctx)
            .await
            .map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
