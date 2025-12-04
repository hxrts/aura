//! # Fact Stream Adapter
//!
//! Provides a WASM-compatible fact streaming infrastructure that bridges
//! journal fact commits to the reactive scheduler.
//!
//! ## Multi-threaded Mode (Native)
//!
//! Uses tokio broadcast channels for concurrent fact distribution.
//!
//! ## Single-threaded Mode (WASM)
//!
//! Uses a callback-based approach with local state for WASM compatibility.
//! Can be enabled via feature flag when needed.

use aura_effects::time::monotonic_now;
use aura_journal::fact::Fact;
use cfg_if::cfg_if;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio::time::Instant;

/// Configuration for fact streaming
#[derive(Debug, Clone)]
pub struct FactStreamConfig {
    /// Channel capacity for buffering facts
    pub channel_capacity: usize,
    /// Enable batching (groups facts for efficiency)
    pub enable_batching: bool,
    /// Batch window in milliseconds (if batching enabled)
    pub batch_window_ms: u64,
}

impl Default for FactStreamConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 1000,
            enable_batching: true,
            batch_window_ms: 5,
        }
    }
}

cfg_if! {
    if #[cfg(not(feature = "wasm-compat"))] {
        /// Fact stream adapter for bridging journal commits to reactive views (native mode)
        #[derive(Clone)]
        pub struct FactStreamAdapter {
            /// Broadcast sender for streaming facts
            sender: broadcast::Sender<Vec<Fact>>,
            /// Configuration
            config: FactStreamConfig,
            /// Batch accumulator (if batching enabled)
            batch: Arc<RwLock<Vec<Fact>>>,
            /// Last batch flush time
            last_flush: Arc<RwLock<Instant>>,
        }
    } else {
        /// Fact stream adapter for bridging journal commits to reactive views (WASM mode)
        #[derive(Clone)]
        pub struct FactStreamAdapter {
            /// Single-threaded callback for WASM mode
            callback: Arc<RwLock<Option<Box<dyn Fn(Vec<Fact>) + Send + Sync>>>>,
            /// Configuration
            config: FactStreamConfig,
            /// Batch accumulator (if batching enabled)
            batch: Arc<RwLock<Vec<Fact>>>,
            /// Last batch flush time
            last_flush: Arc<RwLock<Instant>>,
        }
    }
}

cfg_if! {
    if #[cfg(not(feature = "wasm-compat"))] {
        impl FactStreamAdapter {
            /// Create a new fact stream adapter with default configuration
            pub fn new() -> Self {
                Self::with_config(FactStreamConfig::default())
            }

            /// Create a new fact stream adapter with custom configuration
            pub fn with_config(config: FactStreamConfig) -> Self {
                let (sender, _) = broadcast::channel(config.channel_capacity);

                Self {
                    sender,
                    config,
                    batch: Arc::new(RwLock::new(Vec::new())),
                    last_flush: Arc::new(RwLock::new(monotonic_now())),
                }
            }
        }
    } else {
        impl FactStreamAdapter {
            /// Create a new fact stream adapter with default configuration
            pub fn new() -> Self {
                Self::with_config(FactStreamConfig::default())
            }

            /// Create a new fact stream adapter with custom configuration (WASM mode)
            pub fn with_config(config: FactStreamConfig) -> Self {
                Self {
                    callback: Arc::new(RwLock::new(None)),
                    config,
                    batch: Arc::new(RwLock::new(Vec::new())),
                    last_flush: Arc::new(RwLock::new(monotonic_now())),
                }
            }
        }
    }
}

cfg_if! {
    if #[cfg(not(feature = "wasm-compat"))] {
        impl FactStreamAdapter {
            /// Subscribe to fact stream (multi-threaded mode)
            ///
            /// Returns a receiver that will receive batches of facts as they are committed.
            pub fn subscribe(&self) -> broadcast::Receiver<Vec<Fact>> {
                self.sender.subscribe()
            }
        }
    } else {
        impl FactStreamAdapter {
            /// Set callback for fact stream (WASM single-threaded mode)
            ///
            /// The callback will be invoked with batches of facts as they are committed.
            pub async fn set_callback<F>(&self, callback: F)
            where
                F: Fn(Vec<Fact>) + Send + Sync + 'static,
            {
                let mut cb = self.callback.write().await;
                *cb = Some(Box::new(callback));
            }
        }
    }
}

impl FactStreamAdapter {
    /// Notify the adapter of a new fact commit
    ///
    /// This method should be called whenever facts are committed to the journal.
    /// Facts will be batched and broadcast according to the configuration.
    pub async fn notify_fact(&self, fact: Fact) {
        self.notify_facts(vec![fact]).await;
    }

    /// Notify the adapter of multiple fact commits
    ///
    /// More efficient than calling `notify_fact` multiple times.
    pub async fn notify_facts(&self, facts: Vec<Fact>) {
        if facts.is_empty() {
            return;
        }

        if self.config.enable_batching {
            // Add to batch
            {
                let mut batch = self.batch.write().await;
                batch.extend(facts);
            }

            // Check if we should flush
            let should_flush = {
                let last_flush = self.last_flush.read().await;
                last_flush.elapsed().as_millis() as u64 >= self.config.batch_window_ms
            };

            if should_flush {
                self.flush_batch().await;
            }
        } else {
            // Immediate broadcast without batching
            self.broadcast(facts).await;
        }
    }

    /// Manually flush the current batch
    ///
    /// Useful for forcing a flush before the batch window expires.
    pub async fn flush_batch(&self) {
        let facts = {
            let mut batch = self.batch.write().await;
            if batch.is_empty() {
                return;
            }
            std::mem::take(&mut *batch)
        };

        // Update last flush time
        {
            let mut last_flush = self.last_flush.write().await;
            *last_flush = monotonic_now();
        }

        self.broadcast(facts).await;
    }
}

cfg_if! {
    if #[cfg(not(feature = "wasm-compat"))] {
        impl FactStreamAdapter {
            /// Broadcast facts to subscribers
            async fn broadcast(&self, facts: Vec<Fact>) {
                if facts.is_empty() {
                    return;
                }

                // Ignore send errors (happens when there are no subscribers)
                let _ = self.sender.send(facts);
            }
        }
    } else {
        impl FactStreamAdapter {
            /// Invoke callback with facts (WASM mode)
            async fn broadcast(&self, facts: Vec<Fact>) {
                if facts.is_empty() {
                    return;
                }

                let callback = self.callback.read().await;
                if let Some(cb) = callback.as_ref() {
                    cb(facts);
                }
            }
        }
    }
}

impl FactStreamAdapter {
    /// Get streaming statistics
    pub async fn stats(&self) -> FactStreamStats {
        let batch_size = self.batch.read().await.len();
        let last_flush = self.last_flush.read().await;

        cfg_if! {
            if #[cfg(not(feature = "wasm-compat"))] {
                FactStreamStats {
                    pending_batch_size: batch_size,
                    time_since_last_flush_ms: last_flush.elapsed().as_millis() as u64,
                    subscriber_count: self.sender.receiver_count(),
                }
            } else {
                FactStreamStats {
                    pending_batch_size: batch_size,
                    time_since_last_flush_ms: last_flush.elapsed().as_millis() as u64,
                    subscriber_count: if self.callback.read().await.is_some() {
                        1
                    } else {
                        0
                    },
                }
            }
        }
    }
}

impl Default for FactStreamAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the fact stream
#[derive(Debug, Clone)]
pub struct FactStreamStats {
    /// Number of facts in the current batch (not yet flushed)
    pub pending_batch_size: usize,
    /// Milliseconds since the last batch flush
    pub time_since_last_flush_ms: u64,
    /// Number of active subscribers
    pub subscriber_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
    use aura_core::Hash32;
    use aura_journal::fact::FactContent;

    fn make_test_fact(order: u8) -> Fact {
        Fact {
            order: OrderTime([order; 32]),
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
            content: FactContent::Snapshot(aura_journal::fact::SnapshotFact {
                state_hash: Hash32([0u8; 32]),
                superseded_facts: vec![],
                sequence: 0,
            }),
        }
    }

    #[tokio::test]
    #[cfg(not(feature = "wasm-compat"))]
    async fn test_immediate_broadcast() {
        let config = FactStreamConfig {
            channel_capacity: 100,
            enable_batching: false,
            batch_window_ms: 0,
        };

        let adapter = FactStreamAdapter::with_config(config);
        let mut receiver = adapter.subscribe();

        // Send a fact
        let fact = make_test_fact(1);
        adapter.notify_fact(fact.clone()).await;

        // Receive should succeed immediately
        let received = tokio::time::timeout(std::time::Duration::from_millis(10), receiver.recv())
            .await
            .expect("Should receive within timeout")
            .expect("Should receive facts");

        assert_eq!(received.len(), 1);
    }

    #[tokio::test]
    #[cfg(not(feature = "wasm-compat"))]
    async fn test_batching() {
        let config = FactStreamConfig {
            channel_capacity: 100,
            enable_batching: true,
            batch_window_ms: 50,
        };

        let adapter = FactStreamAdapter::with_config(config);
        let mut receiver = adapter.subscribe();

        // Send multiple facts
        adapter.notify_fact(make_test_fact(1)).await;
        adapter.notify_fact(make_test_fact(2)).await;
        adapter.notify_fact(make_test_fact(3)).await;

        // Should be batched, not yet flushed
        let stats = adapter.stats().await;
        assert_eq!(stats.pending_batch_size, 3);

        // Manual flush
        adapter.flush_batch().await;

        // Should receive all 3 facts in one batch
        let received = receiver.recv().await.expect("Should receive facts");
        assert_eq!(received.len(), 3);

        // Batch should be empty now
        let stats = adapter.stats().await;
        assert_eq!(stats.pending_batch_size, 0);
    }

    #[tokio::test]
    #[cfg(not(feature = "wasm-compat"))]
    async fn test_multiple_subscribers() {
        let adapter = FactStreamAdapter::new();
        let mut rx1 = adapter.subscribe();
        let mut rx2 = adapter.subscribe();

        let stats = adapter.stats().await;
        assert_eq!(stats.subscriber_count, 2);

        // Send fact
        adapter.notify_facts(vec![make_test_fact(1)]).await;
        adapter.flush_batch().await;

        // Both should receive
        let r1 = rx1.recv().await.expect("rx1 should receive");
        let r2 = rx2.recv().await.expect("rx2 should receive");

        assert_eq!(r1.len(), 1);
        assert_eq!(r2.len(), 1);
    }
}
