//! # Reactive Pipeline
//!
//! Small wiring layer that connects:
//! - The batching + ordering engine (`ReactiveScheduler`)
//!
//! This keeps "how facts are published" separate from "how views are updated".

use std::sync::Arc;
use std::time::Duration;

use aura_app::ReactiveHandler;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::types::identifiers::AuthorityId;
use aura_journal::fact::Fact;
use aura_journal::FactRegistry;
use tokio::sync::{broadcast, mpsc};

use super::ViewUpdate;
use super::{
    ChatSignalView, ContactsSignalView, HomeSignalView, InvitationsSignalView, RecoverySignalView,
};
use super::{FactSource, ReactiveScheduler, SchedulerConfig};
use crate::runtime::{
    AuraEffectSystem, RuntimeDiagnostic, RuntimeDiagnosticKind, RuntimeDiagnosticSeverity,
    RuntimeDiagnosticSink, TaskGroup,
};
use crate::task_registry::TaskSupervisor;

/// Original runtime binding minted only by actual pipeline assembly.
pub(super) struct PipelineRuntimeOwnerCapability {
    effects: std::sync::Weak<AuraEffectSystem>,
}
impl PipelineRuntimeOwnerCapability {
    pub(super) fn into_runtime(self) -> std::sync::Weak<AuraEffectSystem> {
        self.effects
    }
}

/// Owns the running scheduler + the single fact publication mechanism.
///
/// Intended integration:
/// - Runtime journal commit / inbound sync calls `publish_journal_facts()` with typed facts
/// - The scheduler processes them and drives view updates
pub struct ReactivePipeline {
    fact_tx: super::FactIngress,
    shutdown_tx: mpsc::Sender<()>,
    updates: broadcast::Receiver<ViewUpdate>,
    tasks: TaskGroup,
    diagnostics: Arc<RuntimeDiagnosticSink>,
    _owned_supervisor: Option<TaskSupervisor>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReactivePipelineError {
    #[error("reactive fact sink is closed")]
    FactSinkClosed {
        #[source]
        source: super::FactProcessingError,
    },
    #[error("required reactive shutdown failed")]
    RequiredShutdown {
        #[source]
        source: crate::task_registry::TaskSupervisionError,
    },
    #[error("reactive shutdown signal channel is unavailable")]
    ShutdownSignalUnavailable,
}

impl ReactivePipeline {
    const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

    /// Start the reactive pipeline with a dedicated local supervisor.
    ///
    /// This is intended for tests and standalone harnesses that are not running
    /// under the full runtime system.
    pub fn start_for_test(
        scheduler_config: SchedulerConfig,
        fact_registry: Arc<FactRegistry>,
        time_effects: Arc<dyn PhysicalTimeEffects>,
        effects: Arc<AuraEffectSystem>,
        own_authority: AuthorityId,
        reactive: ReactiveHandler,
    ) -> Self {
        let supervisor = TaskSupervisor::new();
        let tasks = supervisor.group("reactive_pipeline_test");
        Self::start_internal(
            tasks,
            Some(supervisor),
            scheduler_config,
            fact_registry,
            time_effects,
            effects,
            own_authority,
            reactive,
            Arc::new(RuntimeDiagnosticSink::new()),
        )
    }

    /// Start the reactive pipeline and spawn background tasks.
    ///
    /// Note: `FactStreamAdapter` batching is disabled here because the scheduler
    /// already performs batching with a configurable window.
    pub fn start(
        tasks: TaskGroup,
        scheduler_config: SchedulerConfig,
        fact_registry: Arc<FactRegistry>,
        time_effects: Arc<dyn PhysicalTimeEffects>,
        effects: Arc<AuraEffectSystem>,
        own_authority: AuthorityId,
        reactive: ReactiveHandler,
        diagnostics: Arc<RuntimeDiagnosticSink>,
    ) -> Self {
        Self::start_internal(
            tasks,
            None,
            scheduler_config,
            fact_registry,
            time_effects,
            effects,
            own_authority,
            reactive,
            diagnostics,
        )
    }

    fn start_internal(
        tasks: TaskGroup,
        owned_supervisor: Option<TaskSupervisor>,
        scheduler_config: SchedulerConfig,
        fact_registry: Arc<FactRegistry>,
        time_effects: Arc<dyn PhysicalTimeEffects>,
        effects: Arc<AuraEffectSystem>,
        own_authority: AuthorityId,
        reactive: ReactiveHandler,
        diagnostics: Arc<RuntimeDiagnosticSink>,
    ) -> Self {
        let (mut scheduler, fact_tx, shutdown_tx) = ReactiveScheduler::new_owned(
            scheduler_config,
            fact_registry,
            time_effects.clone(),
            PipelineRuntimeOwnerCapability {
                effects: Arc::downgrade(&effects),
            },
        );

        // Register UI-facing signal views (scheduler → signals).
        scheduler.register_view(Arc::new(ChatSignalView::new(
            own_authority,
            reactive.clone(),
            effects.clone(),
        )));
        scheduler.register_view(Arc::new(InvitationsSignalView::new(
            own_authority,
            reactive.clone(),
        )));
        scheduler.register_view(Arc::new(ContactsSignalView::new(
            own_authority,
            reactive.clone(),
        )));
        scheduler.register_view(Arc::new(RecoverySignalView::new(
            own_authority,
            reactive.clone(),
        )));
        scheduler.register_view(Arc::new(HomeSignalView::new(own_authority, reactive)));

        let updates = scheduler.subscribe();

        let fut = async move { scheduler.run().await };
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_try_named("scheduler", fut);
            } else {
                let _task_handle = tasks.spawn_try_named("scheduler", fut);
            }
        }

        Self {
            fact_tx,
            shutdown_tx,
            updates,
            tasks,
            diagnostics,
            _owned_supervisor: owned_supervisor,
        }
    }

    /// Publish a batch of committed journal facts.
    pub async fn publish_journal_facts(
        &self,
        facts: Vec<Fact>,
    ) -> Result<(), ReactivePipelineError> {
        self.fact_tx
            .send(FactSource::Journal(facts))
            .await
            .map_err(|source| {
                self.diagnostics.emit(RuntimeDiagnostic {
                    severity: RuntimeDiagnosticSeverity::Error,
                    kind: RuntimeDiagnosticKind::ReactiveFactPublishFailed,
                    component: "reactive_pipeline",
                    message: "reactive fact sink is closed".to_string(),
                });
                tracing::error!(
                    event = "runtime.reactive.fact_publish_failed",
                    "Reactive fact publication failed because the scheduler sink is closed"
                );
                ReactivePipelineError::FactSinkClosed { source }
            })
    }

    /// Publish replay facts and wait for this exact accepted target under the
    /// service owner's original resource window. Diagnostic Batch events never
    /// satisfy required replay readiness.
    pub(crate) async fn replay_required<T: PhysicalTimeEffects + Sync>(
        &self,
        facts: Vec<Fact>,
        time: &T,
        window: &aura_core::TimeoutBudget,
    ) -> Result<(), aura_core::time::timeout::TimeoutRunError<super::FactProcessingError>> {
        let target = self
            .fact_tx
            .publish_required(facts)
            .await
            .map_err(aura_core::time::timeout::TimeoutRunError::Operation)?;
        target.await_processed(&self.fact_tx, time, window).await
    }

    /// Actual owned scheduler failure, including a retained required clock cause.
    pub(crate) fn terminal_failure(&self) -> Option<crate::task_registry::TaskSupervisionError> {
        self.tasks.terminal_failure()
    }

    /// Subscribe to scheduler view updates.
    pub fn subscribe(&self) -> broadcast::Receiver<ViewUpdate> {
        self.updates.resubscribe()
    }

    /// Direct sender for injecting facts (useful for tests).
    pub fn fact_sender(&self) -> super::FactIngress {
        self.fact_tx.clone()
    }

    /// Required disposal consumes this pipeline under the caller's original
    /// drain owner. Cancellation, timer failure and forced abort are failures,
    /// never evidence of completed descendant destruction.
    pub(crate) async fn shutdown_with_original_budget<T: PhysicalTimeEffects>(
        self,
        time: &T,
        original: &aura_core::TimeoutBudget,
    ) -> Result<(), ReactivePipelineError> {
        // The owned graceful signal wakes the scheduler without cancellation; the exact
        // owned task group is still retained until descendant completion ACK.
        match self.shutdown_tx.try_send(()) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // A naturally finished scheduler still requires its actual task
                // completion, including native failure consumption below.
            }
        }
        self.tasks
            .wait_with_original_budget(time, original)
            .await
            .map_err(|source| ReactivePipelineError::RequiredShutdown { source })
    }

    pub async fn shutdown(self) -> Result<(), ReactivePipelineError> {
        let mut shutdown_error = None;
        if self.shutdown_tx.send(()).await.is_err() {
            self.diagnostics.emit(RuntimeDiagnostic {
                severity: RuntimeDiagnosticSeverity::Warn,
                kind: RuntimeDiagnosticKind::ReactiveShutdownSignalDropped,
                component: "reactive_pipeline",
                message: "reactive shutdown signal receiver is already closed".to_string(),
            });
            tracing::warn!(
                event = "runtime.reactive.shutdown_signal_dropped",
                "Reactive pipeline shutdown signal receiver was already closed"
            );
            shutdown_error = Some(ReactivePipelineError::ShutdownSignalUnavailable);
        }
        if let Err(error) = self
            .tasks
            .shutdown_with_timeout(Self::SHUTDOWN_TIMEOUT)
            .await
        {
            tracing::warn!(
                event = "runtime.reactive_pipeline.shutdown_escalated",
                error = %error,
                "Reactive pipeline required forced shutdown"
            );
        }
        match shutdown_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for ReactivePipeline {
    fn drop(&mut self) {
        if self.shutdown_tx.try_send(()).is_err() {
            tracing::debug!(
                event = "runtime.reactive.shutdown_signal_drop_ignored",
                "Reactive pipeline drop observed an already-closed shutdown channel"
            );
        }
        self.tasks.request_cancellation();
    }
}
