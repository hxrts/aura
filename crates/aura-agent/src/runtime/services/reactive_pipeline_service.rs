use super::service_actor::{validate_actor_transition, ActorLifecyclePhase};
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use crate::reactive::{ReactivePipeline, SchedulerConfig};
use crate::runtime::TaskGroup;
use crate::runtime::{AuraEffectSystem, RuntimeDiagnosticSink};
use async_trait::async_trait;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::types::identifiers::AuthorityId;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReactivePipelineServiceState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

impl ReactivePipelineServiceState {
    fn phase(self) -> ActorLifecyclePhase {
        match self {
            Self::Stopped => ActorLifecyclePhase::Stopped,
            Self::Starting => ActorLifecyclePhase::Starting,
            Self::Running => ActorLifecyclePhase::Running,
            Self::Stopping => ActorLifecyclePhase::Stopping,
            Self::Failed => ActorLifecyclePhase::Failed,
        }
    }
}

struct ReactivePipelineShared {
    pipeline: RwLock<Option<ReactivePipeline>>,
    state: RwLock<ReactivePipelineServiceState>,
    lifecycle: Mutex<()>,
    startup_window: RwLock<Option<aura_core::TimeoutBudget>>,
}

#[derive(Clone)]
#[aura_macros::actor_root(
    owner = "reactive_pipeline_service",
    domain = "reactive_pipeline",
    supervision = "reactive_pipeline_task_root",
    category = "actor_owned"
)]
pub struct ReactivePipelineService {
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    diagnostics: Arc<RuntimeDiagnosticSink>,
    shared: Arc<ReactivePipelineShared>,
}

impl ReactivePipelineService {
    const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

    pub fn new(
        effects: Arc<AuraEffectSystem>,
        authority_id: AuthorityId,
        diagnostics: Arc<RuntimeDiagnosticSink>,
    ) -> Self {
        Self {
            effects,
            authority_id,
            diagnostics,
            shared: Arc::new(ReactivePipelineShared {
                pipeline: RwLock::new(None),
                state: RwLock::new(ReactivePipelineServiceState::Stopped),
                lifecycle: Mutex::new(()),
                startup_window: RwLock::new(None),
            }),
        }
    }

    async fn mark_state(&self, next: ReactivePipelineServiceState) {
        *self.shared.state.write().await = next;
    }

    /// Required post-registration replay consumes the original retained startup
    /// window; absence/stoppage never represents successful readiness.
    pub async fn replay_committed_facts(&self) -> Result<(), ServiceError> {
        let window = self
            .shared
            .startup_window
            .read()
            .await
            .clone()
            .ok_or_else(|| {
                ServiceError::unavailable(self.name(), "original startup window is absent")
            })?;
        aura_core::time::timeout::execute_with_timeout_budget(
            self.effects.as_ref(),
            &window,
            || async {
                let pipeline = self.shared.pipeline.read().await;
                let pipeline = pipeline.as_ref().ok_or_else(|| {
                    ServiceError::unavailable(self.name(), "required reactive pipeline is absent")
                })?;
                let facts = self
                    .effects
                    .load_committed_facts(self.authority_id)
                    .await
                    .map_err(|source| {
                        ServiceError::startup_failed(self.name(), "required replay read failed")
                            .with_cause(source)
                    })?;
                // An empty replay still produces a genuine accepted target; it validates
                // the same running ingress and its processing acknowledgment.
                pipeline
                    .replay_required(facts, self.effects.as_ref(), &window)
                    .await
                    .map_err(|source| {
                        ServiceError::startup_failed(
                            self.name(),
                            "required replay processing failed",
                        )
                        .with_cause(source)
                    })
            },
        )
        .await
        .map_err(|source| super::traits::service_window_failure(self.name(), source))
    }

    pub async fn is_running(&self) -> bool {
        let state = *self.shared.state.read().await;
        state == ReactivePipelineServiceState::Running
            && self
                .shared
                .pipeline
                .read()
                .await
                .as_ref()
                .is_some_and(|pipeline| pipeline.terminal_failure().is_none())
    }

    async fn start_managed(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        let _guard = self.shared.lifecycle.lock().await;
        let current = *self.shared.state.read().await;
        if current == ReactivePipelineServiceState::Running {
            return Ok(());
        }
        validate_actor_transition(self.name(), current.phase(), ActorLifecyclePhase::Starting)?;
        self.mark_state(ReactivePipelineServiceState::Starting)
            .await;

        let time_effects: Arc<dyn PhysicalTimeEffects> =
            Arc::new(self.effects.time_effects().clone());
        let tasks: TaskGroup = context.tasks().group(self.name());
        let pipeline = ReactivePipeline::start(
            tasks,
            SchedulerConfig::default(),
            self.effects.fact_registry(),
            time_effects,
            self.effects.clone(),
            self.authority_id,
            self.effects.reactive_handler(),
            self.diagnostics.clone(),
        );

        self.effects
            .attach_fact_sink(pipeline.fact_sender())
            .map_err(|source| {
                ServiceError::startup_failed(
                    "reactive_pipeline",
                    "attaching original pipeline ingress failed",
                )
                .with_cause(source)
            })?;

        // Install before the required await: cancellation of startup leaves the
        // actual pipeline owned by this service and included in caller cleanup.
        *self.shared.pipeline.write().await = Some(pipeline);
        *self.shared.startup_window.write().await = Some(context.startup_window().clone());
        if let Err(source) = self.replay_committed_facts().await {
            self.mark_state(ReactivePipelineServiceState::Failed).await;
            return Err(source);
        }

        self.mark_state(ReactivePipelineServiceState::Running).await;
        Ok(())
    }

    async fn stop_managed(&self) -> Result<(), ServiceError> {
        let started = self.effects.physical_time().await.map_err(|source| {
            ServiceError::unavailable(self.name(), "required cleanup clock failed")
                .with_cause(source)
        })?;
        let original =
            aura_core::TimeoutBudget::from_start_and_timeout(&started, Self::SHUTDOWN_TIMEOUT)
                .map_err(|source| {
                    ServiceError::new(
                        self.name(),
                        super::traits::ServiceErrorKind::InvalidConfiguration,
                        "invalid original pipeline cleanup window",
                    )
                    .with_cause(source)
                })?;
        self.stop_with_original_budget(&original).await
    }

    /// Whole-runtime drain supplies its already held original resource window.
    /// It must settle admitted canonical publications before entering this path.
    pub(crate) async fn stop_with_original_budget(
        &self,
        original: &aura_core::TimeoutBudget,
    ) -> Result<(), ServiceError> {
        let _guard = self.shared.lifecycle.lock().await;
        let current = *self.shared.state.read().await;
        if current == ReactivePipelineServiceState::Stopped {
            return Ok(());
        }
        validate_actor_transition(self.name(), current.phase(), ActorLifecyclePhase::Stopping)?;
        self.mark_state(ReactivePipelineServiceState::Stopping)
            .await;
        let pipeline = self.shared.pipeline.write().await.take();
        if let Some(pipeline) = pipeline {
            if let Err(source) = pipeline
                .shutdown_with_original_budget(self.effects.as_ref(), original)
                .await
            {
                self.mark_state(ReactivePipelineServiceState::Failed).await;
                return Err(ServiceError::shutdown_failed(
                    self.name(),
                    "required owned pipeline disposal failed",
                )
                .with_cause(source));
            }
        }
        *self.shared.startup_window.write().await = None;
        self.mark_state(ReactivePipelineServiceState::Stopped).await;
        Ok(())
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for ReactivePipelineService {
    fn name(&self) -> &'static str {
        "reactive_pipeline"
    }

    async fn start(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        self.start_managed(context).await
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        self.stop_managed().await
    }

    async fn health(&self) -> ServiceHealth {
        match *self.shared.state.read().await {
            ReactivePipelineServiceState::Stopped => ServiceHealth::Stopped,
            ReactivePipelineServiceState::Starting => ServiceHealth::Starting,
            ReactivePipelineServiceState::Stopping => ServiceHealth::Stopping,
            ReactivePipelineServiceState::Failed => ServiceHealth::Unhealthy {
                reason: "reactive pipeline entered failed lifecycle state".to_string(),
            },
            ReactivePipelineServiceState::Running => {
                if self
                    .shared
                    .pipeline
                    .read()
                    .await
                    .as_ref()
                    .is_some_and(|pipeline| pipeline.terminal_failure().is_none())
                {
                    ServiceHealth::Healthy
                } else {
                    ServiceHealth::Unhealthy {
                        reason: "reactive pipeline missing running instance".to_string(),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{TimeoutBudget, TimeoutBudgetError};
    use aura_guards::GuardContextProvider;
    use aura_testkit::time::ManualPhysicalClock;
    use std::error::Error;

    fn source_of<'a, T: Error + 'static>(error: &'a (dyn Error + 'static)) -> Option<&'a T> {
        let mut current = Some(error);
        while let Some(cause) = current {
            if let Some(original) = cause.downcast_ref::<T>() {
                return Some(original);
            }
            current = cause.source();
        }
        None
    }

    #[tokio::test]
    async fn required_startup_replay_retains_original_window_after_signal_registration() {
        let config = crate::core::AgentConfig::default();
        let clock = ManualPhysicalClock::new(1000);
        let effects = Arc::new(
            crate::testing::simulation_effect_system(&config)
                .with_physical_time_provider(Arc::new(clock.clone())),
        );
        let supervisor = Arc::new(crate::runtime::TaskSupervisor::new());
        let start = effects
            .physical_time()
            .await
            .expect("actual startup observation");
        let window = TimeoutBudget::from_start_and_timeout(&start, Duration::from_secs(30))
            .expect("original startup resource policy");
        let context =
            RuntimeServiceContext::new(supervisor.clone(), Arc::new(clock.clone()), window.clone());
        let service = ReactivePipelineService::new(
            effects.clone(),
            effects.authority_id(),
            Arc::new(RuntimeDiagnosticSink::new()),
        );
        service
            .start(&context)
            .await
            .expect("actual empty replay processing ACK");
        assert!(service.is_running().await);
        assert!(service
            .shared
            .startup_window
            .read()
            .await
            .as_ref()
            .expect("retained original startup owner")
            .shares_observation_owner_with(&window));
        // Frontend signal registration does not grant a fresh service deadline.
        aura_app::signal_defs::register_app_signals(&effects.reactive_handler())
            .await
            .expect("actual frontend signal registration");
        clock.set_time(31_000);
        let failure = service
            .replay_committed_facts()
            .await
            .expect_err("late replay cannot allocate a renewed startup window");
        assert!(matches!(
            source_of::<TimeoutBudgetError>(&failure),
            Some(TimeoutBudgetError::DeadlineExceeded {
                deadline_at_ms: 31_000,
                ..
            })
        ));
        service
            .stop()
            .await
            .expect("actual owned scheduler destruction");
        assert!(!service.is_running().await);
    }

    #[tokio::test]
    async fn required_startup_replay_clock_failure_keeps_pipeline_for_owned_cleanup() {
        let config = crate::core::AgentConfig::default();
        let clock = ManualPhysicalClock::new(3000);
        let effects = Arc::new(
            crate::testing::simulation_effect_system(&config)
                .with_physical_time_provider(Arc::new(clock.clone())),
        );
        let supervisor = Arc::new(crate::runtime::TaskSupervisor::new());
        let start = effects
            .physical_time()
            .await
            .expect("actual startup observation");
        let window = TimeoutBudget::from_start_and_timeout(&start, Duration::from_secs(30))
            .expect("original startup resource policy");
        let context = RuntimeServiceContext::new(supervisor, Arc::new(clock.clone()), window);
        let service = ReactivePipelineService::new(
            effects.clone(),
            effects.authority_id(),
            Arc::new(RuntimeDiagnosticSink::new()),
        );
        clock
            .fail_next_observation(aura_core::effects::time::TimeError::OperationFailed {
                reason: "actual startup provider failure".into(),
            })
            .await;
        let failure = service
            .start(&context)
            .await
            .expect_err("required startup clock cannot report readiness");
        assert!(
            source_of::<aura_core::effects::time::TimeError>(&failure).is_some(),
            "actual provider cause survives service error: {failure}"
        );
        assert!(!service.is_running().await);
        assert!(
            service.shared.pipeline.read().await.is_some(),
            "partial pipeline stays owned for cleanup"
        );
        service
            .stop()
            .await
            .expect("partial actual scheduler cleanup");
        assert!(service.shared.pipeline.read().await.is_none());
    }
}
