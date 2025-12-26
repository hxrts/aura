//! # Reactive Pipeline
//!
//! Small wiring layer that connects:
//! - The batching + ordering engine (`ReactiveScheduler`)
//!
//! This keeps "how facts are published" separate from "how views are updated".

use std::sync::Arc;

use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::identifiers::AuthorityId;
use aura_app::ReactiveHandler;
use aura_journal::fact::Fact;
use aura_journal::FactRegistry;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use super::ViewUpdate;
use super::{
    BlockSignalView, ChatSignalView, ContactsSignalView, InvitationsSignalView, RecoverySignalView,
};
use super::{FactSource, ReactiveScheduler, SchedulerConfig};

/// Owns the running scheduler + the single fact publication mechanism.
///
/// Intended integration:
/// - Runtime journal commit / inbound sync calls `publish_journal_facts()` with typed facts
/// - The scheduler processes them and drives view updates
pub struct ReactivePipeline {
    fact_tx: mpsc::Sender<FactSource>,
    shutdown_tx: mpsc::Sender<()>,
    updates: broadcast::Receiver<ViewUpdate>,
    scheduler_task: JoinHandle<()>,
}

impl ReactivePipeline {
    /// Start the reactive pipeline and spawn background tasks.
    ///
    /// Note: `FactStreamAdapter` batching is disabled here because the scheduler
    /// already performs batching with a configurable window.
    pub fn start(
        scheduler_config: SchedulerConfig,
        fact_registry: FactRegistry,
        time_effects: Arc<dyn PhysicalTimeEffects>,
        own_authority: AuthorityId,
        reactive: ReactiveHandler,
    ) -> Self {
        let (mut scheduler, fact_tx, shutdown_tx) =
            ReactiveScheduler::new(scheduler_config, fact_registry, time_effects);

        // Register UI-facing signal views (scheduler → signals).
        scheduler.register_view(Arc::new(ChatSignalView::new(
            own_authority,
            reactive.clone(),
        )));
        scheduler.register_view(Arc::new(InvitationsSignalView::new(
            own_authority,
            reactive.clone(),
        )));
        scheduler.register_view(Arc::new(ContactsSignalView::new(reactive.clone())));
        scheduler.register_view(Arc::new(RecoverySignalView::new(reactive.clone())));
        scheduler.register_view(Arc::new(BlockSignalView::new(reactive)));

        let updates = scheduler.subscribe();

        let scheduler_task = tokio::spawn(async move { scheduler.run().await });

        Self {
            fact_tx,
            shutdown_tx,
            updates,
            scheduler_task,
        }
    }

    /// Publish a batch of committed journal facts.
    pub async fn publish_journal_facts(&self, facts: Vec<Fact>) {
        let _ = self.fact_tx.send(FactSource::Journal(facts)).await;
    }

    /// Subscribe to scheduler view updates.
    pub fn subscribe(&self) -> broadcast::Receiver<ViewUpdate> {
        self.updates.resubscribe()
    }

    /// Direct sender for injecting facts (useful for tests).
    pub fn fact_sender(&self) -> mpsc::Sender<FactSource> {
        self.fact_tx.clone()
    }

    /// Shutdown the scheduler.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(()).await;
        self.scheduler_task.abort();
    }
}

impl Drop for ReactivePipeline {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.try_send(());
        self.scheduler_task.abort();
    }
}
