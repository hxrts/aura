//! # Reactive Pipeline
//!
//! Small wiring layer that connects:
//! - The batching + ordering engine (`ReactiveScheduler`)
//!
//! This keeps "how facts are published" separate from "how views are updated".

use std::sync::Arc;

use aura_app::ReactiveHandler;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::identifiers::AuthorityId;
use aura_journal::fact::Fact;
use aura_journal::FactRegistry;
use tokio::sync::{broadcast, mpsc};
#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinHandle;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

use super::ViewUpdate;
use super::{
    ChatSignalView, ContactsSignalView, HomeSignalView, InvitationsSignalView, RecoverySignalView,
};
use super::{FactSource, ReactiveScheduler, SchedulerConfig};
use crate::runtime::AuraEffectSystem;

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
    #[cfg(not(target_arch = "wasm32"))]
    scheduler_task: Option<JoinHandle<()>>,
    /// Time effects for deterministic simulation support
    #[cfg(not(target_arch = "wasm32"))]
    time_effects: Arc<dyn PhysicalTimeEffects>,
}

impl ReactivePipeline {
    /// Start the reactive pipeline and spawn background tasks.
    ///
    /// Note: `FactStreamAdapter` batching is disabled here because the scheduler
    /// already performs batching with a configurable window.
    pub fn start(
        scheduler_config: SchedulerConfig,
        fact_registry: Arc<FactRegistry>,
        time_effects: Arc<dyn PhysicalTimeEffects>,
        effects: Arc<AuraEffectSystem>,
        own_authority: AuthorityId,
        reactive: ReactiveHandler,
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

        #[cfg(not(target_arch = "wasm32"))]
        let scheduler_task = Some(tokio::spawn(async move { scheduler.run().await }));
        #[cfg(target_arch = "wasm32")]
        spawn_local(async move { scheduler.run().await });

        Self {
            fact_tx,
            shutdown_tx,
            update_tx,
            updates,
            #[cfg(not(target_arch = "wasm32"))]
            scheduler_task,
            #[cfg(not(target_arch = "wasm32"))]
            time_effects,
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

    /// Get the view update sender for attaching to the effect system.
    ///
    /// This allows callers to subscribe to view updates and await fact processing.
    pub fn update_sender(&self) -> broadcast::Sender<ViewUpdate> {
        self.update_tx.clone()
    }

    /// Shutdown the scheduler.
    ///
    /// Uses effect-injected time for deterministic simulation support.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn shutdown(mut self) {
        let _ = self.shutdown_tx.send(()).await;
        if let Some(mut handle) = self.scheduler_task.take() {
            // Use select! with effect-injected sleep for deterministic shutdown timeout
            tokio::select! {
                _ = &mut handle => {
                    // Task completed normally
                }
                _ = async { let _ = self.time_effects.sleep_ms(2000).await; } => {
                    // Timeout elapsed, abort the task
                    handle.abort();
                }
            }
        }
    }

    /// Shutdown the scheduler on wasm targets.
    #[cfg(target_arch = "wasm32")]
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(()).await;
    }
}

impl Drop for ReactivePipeline {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.try_send(());
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(handle) = self.scheduler_task.as_mut() {
            handle.abort();
        }
    }
}
