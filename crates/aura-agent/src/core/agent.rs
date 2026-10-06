//! Public Agent API
//!
//! Minimal public API surface for the agent runtime.

use super::{AgentError, AgentResult, AuthorityContext};
use crate::handlers::{
    AuthServiceApi, ChatServiceApi, InvitationServiceApi, OtaActivationServiceApi,
    RecoveryServiceApi, SessionServiceApi,
};
use crate::runtime::services::ReconfigurationManager;
use crate::runtime::services::ThresholdSigningService;
use crate::runtime::system::{RuntimeActivityGate, RuntimePublicOperationError, RuntimeSystem};
use crate::runtime::{AuraEffectSystem, EffectContext, TaskSupervisor};
use aura_core::types::identifiers::{AccountId, AuthorityId};
use once_cell::sync::OnceCell;
use std::sync::Arc;

/// Main agent interface - thin facade delegating to runtime
///
/// Services are created on-demand and cached as lightweight wrappers around effects.
pub struct AuraAgent {
    /// The runtime system handling all operations
    runtime: RuntimeSystem,

    /// Authority context for this agent (includes cached account_id)
    context: AuthorityContext,

    /// Cached service instances
    services: ServiceRegistry,

    /// Set once the runtime-owned periodic sync task has been started.
    periodic_sync_started: std::sync::atomic::AtomicBool,
}

impl AuraAgent {
    /// Create a new agent with the given runtime system
    pub(crate) fn new(runtime: RuntimeSystem, authority_id: AuthorityId) -> Self {
        let context = AuthorityContext::new_with_device(authority_id, runtime.device_id());
        let services = ServiceRegistry::new(
            runtime.effects(),
            runtime.ceremony_runner().clone(),
            runtime.reconfiguration().clone(),
            runtime.tasks(),
            runtime.activity_gate(),
            context.clone(),
        );
        Self {
            runtime,
            services,
            context,
            periodic_sync_started: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Claim the one-time start of the runtime-owned periodic sync task.
    pub(crate) fn claim_periodic_sync_start(&self) -> bool {
        !self
            .periodic_sync_started
            .swap(true, std::sync::atomic::Ordering::AcqRel)
    }

    /// Get the authority ID for this agent
    pub fn authority_id(&self) -> AuthorityId {
        self.context.authority_id()
    }

    /// Get the authority context (read-only)
    pub fn context(&self) -> &AuthorityContext {
        &self.context
    }

    /// Access the runtime system (for advanced operations)
    pub fn runtime(&self) -> &RuntimeSystem {
        &self.runtime
    }

    /// Observed record of supervised runtime tasks that failed or panicked
    /// (group, task and typed cause), for diagnostics and tests.
    pub fn supervised_task_failures(&self) -> Vec<crate::runtime::TaskSupervisionError> {
        self.runtime.tasks().task_failures()
    }

    /// Observed names (`group::task`) of supervised runtime tasks still
    /// running, for diagnostics and tests.
    pub fn active_supervised_tasks(&self) -> Vec<String> {
        self.runtime.tasks().active_tasks()
    }

    /// Get the session management service
    ///
    /// Provides access to session creation, management, and lifecycle operations.
    /// Returns a cached service instance or an initialization error.
    pub fn sessions(&self) -> AgentResult<SessionServiceApi> {
        self.services.sessions()
    }

    /// Get the authentication service
    ///
    /// Provides access to authentication operations including challenge-response
    /// flows and device key verification.
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn auth(&self) -> AgentResult<AuthServiceApi> {
        self.services.auth()
    }

    /// Get the chat service
    ///
    /// Provides access to chat operations including group creation, messaging,
    /// and message history retrieval.
    /// Returns a cached service instance or an initialization error.
    pub fn chat(&self) -> AgentResult<ChatServiceApi> {
        self.services.chat()
    }

    /// Get the invitation service
    ///
    /// Provides access to invitation operations including creating, accepting,
    /// and declining invitations for channels, guardians, and contacts.
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn invitations(&self) -> AgentResult<InvitationServiceApi> {
        self.services.invitations()
    }

    /// Get the recovery service
    ///
    /// Provides access to guardian-based recovery operations including device
    /// addition/removal, tree replacement, and guardian set updates.
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn recovery(&self) -> AgentResult<RecoveryServiceApi> {
        self.services.recovery()
    }

    /// Get the OTA activation service
    ///
    /// Provides access to OTA hard-fork activation ceremonies through the shared runner.
    pub fn ota_activation(&self) -> AgentResult<OtaActivationServiceApi> {
        self.services.ota_activation()
    }

    /// Get the threshold signing service
    ///
    /// Provides access to unified threshold signing operations including:
    /// - Multi-device signing (your devices)
    /// - Guardian recovery approvals (cross-authority)
    /// - Group operation approvals (shared authority)
    ///
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn threshold_signing(&self) -> ThresholdSigningService {
        self.runtime.threshold_signing()
    }

    /// Shutdown the agent
    pub async fn shutdown(self, ctx: &EffectContext) -> AgentResult<()> {
        self.runtime.shutdown_typed(ctx).await.map_err(|source| {
            let message = "runtime shutdown failed".to_owned();
            let error = match source {
                source @ (crate::runtime::system::RuntimeShutdownError::ForeignWindow(_)
                | crate::runtime::system::RuntimeShutdownError::AdmissionClosed {
                    ..
                }) => aura_core::AuraError::PermissionDenied {
                    message,
                    source: Some(Arc::new(source)),
                },
                crate::runtime::system::RuntimeShutdownError::Budget(
                    source @ aura_core::TimeoutBudgetError::DeadlineExceeded { .. },
                ) => {
                    return AgentError::TimeoutWithSource {
                        message: "original runtime shutdown deadline exceeded".into(),
                        source: aura_core::AuraError::Internal {
                            message: "original runtime shutdown deadline exceeded".into(),
                            source: Some(Arc::new(source)),
                        },
                    }
                }
                crate::runtime::system::RuntimeShutdownError::Lifecycle(source) => return source,
                source @ (crate::runtime::system::RuntimeShutdownError::TaskTree(_)
                | crate::runtime::system::RuntimeShutdownError::Service(_)
                | crate::runtime::system::RuntimeShutdownError::Budget(_)) => {
                    aura_core::AuraError::Internal {
                        message,
                        source: Some(Arc::new(source)),
                    }
                }
            };
            AgentError::from(error)
        })
    }
}

struct ServiceRegistry {
    effects: Arc<AuraEffectSystem>,
    ceremony_runner: crate::runtime::services::ceremony_runner::CeremonyRunner,
    reconfiguration_manager: ReconfigurationManager,
    runtime_tasks: Arc<TaskSupervisor>,
    runtime_activity: Arc<RuntimeActivityGate>,
    authority_context: AuthorityContext,
    account_id: AccountId,
    sessions: OnceCell<SessionServiceApi>,
    auth: OnceCell<AuthServiceApi>,
    chat: OnceCell<ChatServiceApi>,
    invitations: OnceCell<InvitationServiceApi>,
    recovery: OnceCell<RecoveryServiceApi>,
    ota_activation: OnceCell<OtaActivationServiceApi>,
}

impl ServiceRegistry {
    fn new(
        effects: Arc<AuraEffectSystem>,
        ceremony_runner: crate::runtime::services::ceremony_runner::CeremonyRunner,
        reconfiguration_manager: ReconfigurationManager,
        runtime_tasks: Arc<TaskSupervisor>,
        runtime_activity: Arc<RuntimeActivityGate>,
        authority_context: AuthorityContext,
    ) -> Self {
        let account_id = authority_context.account_id();
        Self {
            effects,
            ceremony_runner,
            reconfiguration_manager,
            runtime_tasks,
            runtime_activity,
            authority_context,
            account_id,
            sessions: OnceCell::new(),
            auth: OnceCell::new(),
            chat: OnceCell::new(),
            invitations: OnceCell::new(),
            recovery: OnceCell::new(),
            ota_activation: OnceCell::new(),
        }
    }

    fn ensure_accepting_public_operations(&self) -> AgentResult<()> {
        self.runtime_activity
            .ensure_accepting_public_operations()
            .map_err(|error: RuntimePublicOperationError| AgentError::runtime(error.to_string()))
    }

    fn sessions(&self) -> AgentResult<SessionServiceApi> {
        self.ensure_accepting_public_operations()?;
        self.sessions
            .get_or_try_init(|| {
                SessionServiceApi::new(
                    self.effects.clone(),
                    self.authority_context.clone(),
                    self.account_id,
                )
            })
            .cloned()
    }

    fn auth(&self) -> AgentResult<AuthServiceApi> {
        self.ensure_accepting_public_operations()?;
        self.auth
            .get_or_try_init(|| {
                AuthServiceApi::new(
                    self.effects.clone(),
                    self.authority_context.clone(),
                    self.account_id,
                )
            })
            .cloned()
    }

    fn chat(&self) -> AgentResult<ChatServiceApi> {
        self.ensure_accepting_public_operations()?;
        self.chat
            .get_or_try_init(|| ChatServiceApi::new(self.effects.clone()))
            .cloned()
    }

    fn invitations(&self) -> AgentResult<InvitationServiceApi> {
        self.ensure_accepting_public_operations()?;
        self.invitations
            .get_or_try_init(|| {
                InvitationServiceApi::new_with_runner(
                    self.effects.clone(),
                    self.authority_context.clone(),
                    self.ceremony_runner.clone(),
                    self.runtime_tasks.clone(),
                )
            })
            .cloned()
    }

    fn recovery(&self) -> AgentResult<RecoveryServiceApi> {
        self.ensure_accepting_public_operations()?;
        self.recovery
            .get_or_try_init(|| {
                RecoveryServiceApi::new_with_runner(
                    self.effects.clone(),
                    self.authority_context.clone(),
                    self.ceremony_runner.clone(),
                    self.reconfiguration_manager.clone(),
                    self.runtime_tasks.clone(),
                )
            })
            .cloned()
    }

    fn ota_activation(&self) -> AgentResult<OtaActivationServiceApi> {
        self.ensure_accepting_public_operations()?;
        self.ota_activation
            .get_or_try_init(|| {
                OtaActivationServiceApi::new_with_runner(
                    self.effects.clone(),
                    self.authority_context.clone(),
                    self.ceremony_runner.clone(),
                    self.reconfiguration_manager.runtime_capability_handler(),
                    self.runtime_tasks.clone(),
                )
            })
            .cloned()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod shutdown_admission_tests {
    use super::*;
    #[tokio::test]
    async fn already_closed_runtime_does_not_publish_false_shutdown_completion() -> AgentResult<()>
    {
        let profile = tempfile::tempdir().map_err(aura_core::AuraError::from)?;
        let authority = AuthorityId::new_from_entropy([237; 32]);
        let mut config = crate::core::AgentConfig::default();
        config.storage.base_path = profile.path().to_path_buf();
        let context = EffectContext::new(
            authority,
            aura_core::ContextId::new_from_entropy([238; 32]),
            aura_core::effects::ExecutionMode::Testing,
        );
        let agent = crate::core::AgentBuilder::new()
            .with_authority(authority)
            .with_config(config)
            .build_testing_async(&context)
            .await?;
        let gate = agent.runtime().activity_gate();
        assert_eq!(
            gate.begin_shutdown(),
            crate::runtime::system::RuntimeActivityState::Running
        );
        let error = match agent.shutdown(&context).await {
            Err(error) => error,
            Ok(()) => {
                return Err(AgentError::internal(
                    "already-closing runtime falsely completed shutdown",
                ))
            }
        };
        let mut cause: &(dyn std::error::Error + 'static) = &error;
        loop {
            if let Some(crate::runtime::system::RuntimeShutdownError::AdmissionClosed { state }) =
                cause.downcast_ref::<crate::runtime::system::RuntimeShutdownError>()
            {
                assert_eq!(
                    *state,
                    crate::runtime::system::RuntimeActivityState::Stopping
                );
                break;
            }
            cause = cause
                .source()
                .ok_or_else(|| AgentError::internal("shutdown native source missing"))?;
        }
        assert_eq!(
            gate.state(),
            crate::runtime::system::RuntimeActivityState::Stopping
        );
        Ok(())
    }
}
