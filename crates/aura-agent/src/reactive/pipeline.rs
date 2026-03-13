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
use aura_core::identifiers::AuthorityId;
use aura_journal::fact::Fact;
use aura_journal::FactRegistry;
use tokio::sync::{broadcast, mpsc};

use super::ViewUpdate;
use super::{
    ChatSignalView, ContactsSignalView, HomeSignalView, InvitationsSignalView, RecoverySignalView,
};
use super::{FactSource, ReactiveScheduler, SchedulerConfig};
use crate::runtime::services::runtime_tasks::TaskGroup;
use crate::runtime::AuraEffectSystem;
use crate::runtime::{
    RuntimeDiagnostic, RuntimeDiagnosticKind, RuntimeDiagnosticSeverity, RuntimeDiagnosticSink,
};
use crate::task_registry::TaskSupervisor;

/// Owns the running scheduler + the single fact publication mechanism.
///
/// Intended integration:
/// - Runtime journal commit / inbound sync calls `publish_journal_facts()` with typed facts
/// - The scheduler processes them and drives view updates
pub struct ReactivePipeline {
    fact_tx: mpsc::Sender<FactSource>,
    shutdown_tx: mpsc::Sender<()>,
    update_tx: broadcast::Sender<ViewUpdate>,
    updates: broadcast::Receiver<ViewUpdate>,
    tasks: TaskGroup,
    diagnostics: Arc<RuntimeDiagnosticSink>,
    _owned_supervisor: Option<TaskSupervisor>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReactivePipelineError {
    #[error("reactive fact sink is closed")]
    FactSinkClosed,
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
        let (mut scheduler, fact_tx, shutdown_tx, update_tx) =
            ReactiveScheduler::new(scheduler_config, fact_registry, time_effects.clone());

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
        scheduler.register_view(Arc::new(ContactsSignalView::new(reactive.clone())));
        scheduler.register_view(Arc::new(RecoverySignalView::new(reactive.clone())));
        scheduler.register_view(Arc::new(HomeSignalView::new(own_authority, reactive)));

        let updates = scheduler.subscribe();

        tasks.spawn_named("scheduler", async move {
            scheduler.run().await;
        });

        Self {
            fact_tx,
            shutdown_tx,
            update_tx,
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
            .map_err(|_| {
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
                ReactivePipelineError::FactSinkClosed
            })
    }

    /// Subscribe to scheduler view updates.
    pub fn subscribe(&self) -> broadcast::Receiver<ViewUpdate> {
        self.updates.resubscribe()
    }

    /// Direct sender for injecting facts (useful for tests).
    pub fn fact_sender(&self) -> mpsc::Sender<FactSource> {
        self.fact_tx.clone()
    }

    /// Get the view update sender for attaching to the effect system.
    ///
    /// This allows callers to subscribe to view updates and await fact processing.
    pub fn update_sender(&self) -> broadcast::Sender<ViewUpdate> {
        self.update_tx.clone()
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
