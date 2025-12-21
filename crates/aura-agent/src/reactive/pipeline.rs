//! # Reactive Pipeline
//!
//! Small wiring layer that connects:
//! - A single runtime fact publication surface (`FactStreamAdapter`)
//! - The batching + ordering engine (`ReactiveScheduler`)
//!
//! This keeps "how facts are published" separate from "how views are updated".

use std::sync::Arc;

use aura_core::effects::time::PhysicalTimeEffects;
use aura_journal::fact::Fact;
use aura_journal::FactRegistry;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use super::{FactSource, FactStreamAdapter, FactStreamConfig, ReactiveScheduler, SchedulerConfig};
use super::ViewUpdate;

/// Owns the running scheduler + the single fact publication mechanism.
///
/// Intended integration:
/// - Runtime journal commit / inbound sync calls `publish_journal_facts()` with typed facts
/// - The scheduler processes them and drives view updates
pub struct ReactivePipeline {
    fact_stream: FactStreamAdapter,
    fact_tx: mpsc::Sender<FactSource>,
    shutdown_tx: mpsc::Sender<()>,
    updates: broadcast::Receiver<ViewUpdate>,
    bridge_task: JoinHandle<()>,
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
    ) -> Self {
        let fact_stream = FactStreamAdapter::with_config(FactStreamConfig {
            enable_batching: false,
            ..FactStreamConfig::default()
        });

        let (scheduler, fact_tx, shutdown_tx) =
            ReactiveScheduler::new(scheduler_config, fact_registry, time_effects);

        let updates = scheduler.subscribe();

        // Bridge fact_stream → scheduler input.
        let mut fact_rx = fact_stream.subscribe();
        let bridge_fact_tx = fact_tx.clone();
        let bridge_task = tokio::spawn(async move {
            loop {
                match fact_rx.recv().await {
                    Ok(facts) => {
                        if facts.is_empty() {
                            continue;
                        }
                        if bridge_fact_tx
                            .send(FactSource::Journal(facts))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // The stream is lossy by definition. The integration work
                        // in work/002.md C2 must ensure eventual consistency.
                        continue;
                    }
                }
            }
        });

        let scheduler_task = tokio::spawn(async move { scheduler.run().await });

        Self {
            fact_stream,
            fact_tx,
            shutdown_tx,
            updates,
            bridge_task,
            scheduler_task,
        }
    }

    /// Publish a batch of committed journal facts.
    pub async fn publish_journal_facts(&self, facts: Vec<Fact>) {
        self.fact_stream.notify_facts(facts).await;
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
        self.bridge_task.abort();
        self.scheduler_task.abort();
    }
}

impl Drop for ReactivePipeline {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.try_send(());
        self.bridge_task.abort();
        self.scheduler_task.abort();
    }
}
