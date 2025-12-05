//! Public Agent API
//!
//! Minimal public API surface for the agent runtime.

use super::{AgentConfig, AgentError, AgentResult, AuthorityContext};
use crate::handlers::{AuthService, InvitationService, RecoveryService, SessionService};
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
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
