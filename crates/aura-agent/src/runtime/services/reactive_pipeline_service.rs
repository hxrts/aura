use super::runtime_tasks::TaskGroup;
use super::service_actor::{validate_actor_transition, ActorLifecyclePhase};
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use crate::reactive::{ReactivePipeline, SchedulerConfig};
use crate::runtime::{AuraEffectSystem, RuntimeDiagnosticSink};
use async_trait::async_trait;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::identifiers::AuthorityId;
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

#[derive(Clone)]
pub struct ReactivePipelineService {
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    diagnostics: Arc<RuntimeDiagnosticSink>,
    pipeline: Arc<RwLock<Option<ReactivePipeline>>>,
    state: Arc<RwLock<ReactivePipelineServiceState>>,
    lifecycle: Arc<Mutex<()>>,
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
            pipeline: Arc::new(RwLock::new(None)),
            state: Arc::new(RwLock::new(ReactivePipelineServiceState::Stopped)),
            lifecycle: Arc::new(Mutex::new(())),
        }
    }

    async fn mark_state(&self, next: ReactivePipelineServiceState) {
        *self.state.write().await = next;
    }

    pub async fn is_running(&self) -> bool {
        let state = *self.state.read().await;
        state == ReactivePipelineServiceState::Running && self.pipeline.read().await.is_some()
    }

    async fn start_managed(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        let _guard = self.lifecycle.lock().await;
        let current = *self.state.read().await;
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

        self.effects.attach_fact_sink(pipeline.fact_sender());
        self.effects
            .attach_view_update_sender(pipeline.update_sender());

        let existing = match self.effects.load_committed_facts(self.authority_id).await {
            Ok(existing) => existing,
            Err(error) => {
                let _ = pipeline.shutdown().await;
                self.mark_state(ReactivePipelineServiceState::Failed).await;
                return Err(ServiceError::startup_failed(
                    self.name(),
                    format!("failed to load committed facts: {error}"),
                ));
            }
        };
        if !existing.is_empty() {
            if let Err(error) = pipeline.publish_journal_facts(existing).await {
                let _ = pipeline.shutdown().await;
                self.mark_state(ReactivePipelineServiceState::Failed).await;
                return Err(ServiceError::startup_failed(
                    self.name(),
                    format!("failed to replay committed facts: {error}"),
                ));
            }
        }

        *self.pipeline.write().await = Some(pipeline);
        self.mark_state(ReactivePipelineServiceState::Running).await;
        Ok(())
    }

    async fn stop_managed(&self) -> Result<(), ServiceError> {
        let _guard = self.lifecycle.lock().await;
        let current = *self.state.read().await;
        if current == ReactivePipelineServiceState::Stopped {
            return Ok(());
        }
        validate_actor_transition(self.name(), current.phase(), ActorLifecyclePhase::Stopping)?;
        self.mark_state(ReactivePipelineServiceState::Stopping)
            .await;

        let pipeline = self.pipeline.write().await.take();
        let shutdown_error = if let Some(pipeline) = pipeline {
            tokio::time::timeout(Self::SHUTDOWN_TIMEOUT, pipeline.shutdown())
                .await
                .map_err(|error| {
                    ServiceError::shutdown_failed(
                        self.name(),
                        format!("reactive pipeline shutdown timed out: {error}"),
                    )
                })?
                .map_err(|error| {
                    ServiceError::shutdown_failed(
                        self.name(),
                        format!("reactive pipeline shutdown failed: {error}"),
                    )
                })
                .err()
        } else {
            None
        };

        match shutdown_error {
            Some(error) => {
                self.mark_state(ReactivePipelineServiceState::Failed).await;
                Err(error)
            }
            None => {
                self.mark_state(ReactivePipelineServiceState::Stopped).await;
                Ok(())
            }
        }
    }
}

#[async_trait]
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
        match *self.state.read().await {
            ReactivePipelineServiceState::Stopped => ServiceHealth::Stopped,
            ReactivePipelineServiceState::Starting => ServiceHealth::Starting,
            ReactivePipelineServiceState::Stopping => ServiceHealth::Stopping,
            ReactivePipelineServiceState::Failed => ServiceHealth::Unhealthy {
                reason: "reactive pipeline entered failed lifecycle state".to_string(),
            },
            ReactivePipelineServiceState::Running => {
                if self.pipeline.read().await.is_some() {
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
