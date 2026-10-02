//! Signal Graph - Reactive State Management
//!
//! The signal graph manages signal storage, dependency tracking, and change propagation.
//! It provides the foundation for the reactive effect system.

use aura_core::effects::reactive::{ReactiveError, SignalId};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

// ─────────────────────────────────────────────────────────────────────────────
// Signal Storage
// ─────────────────────────────────────────────────────────────────────────────

/// Type-erased value wrapper that implements Clone via Arc.
#[derive(Clone)]
pub struct AnyValue(pub(crate) Arc<dyn Any + Send + Sync>);

/// Type-erased signal value storage.
///
/// This allows storing values of any type in the graph while maintaining
/// type safety through the Signal<T> phantom type at the API level.
struct SignalSlot {
    /// The current value (type-erased)
    value: AnyValue,
    /// Monotonic revision of the published value (zero for initial registration).
    revision: u64,
    /// Broadcast channel for notifying subscribers
    sender: broadcast::Sender<AnyValue>,
    /// Type name for debugging
    type_name: &'static str,
}

impl SignalSlot {
    /// Create a new signal slot with an initial value.
    fn new<T: Clone + Send + Sync + 'static>(initial: T) -> Self {
        let (sender, _) = broadcast::channel(256); // Buffer size for updates
        Self {
            value: AnyValue(Arc::new(initial)),
            revision: 0,
            sender,
            type_name: std::any::type_name::<T>(),
        }
    }

    /// Read the current value.
    fn read<T: Clone + Send + Sync + 'static>(&self) -> Result<T, ReactiveError> {
        self.value
            .0
            .downcast_ref::<T>()
            .cloned()
            .ok_or_else(|| ReactiveError::TypeMismatch {
                id: "unknown".to_string(),
                expected: std::any::type_name::<T>().to_string(),
                actual: self.type_name.to_string(),
            })
    }

    /// Update the value and notify subscribers.
    fn emit<T: Clone + Send + Sync + 'static>(&mut self, value: T) -> Result<u64, ReactiveError> {
        // Verify type matches
        if self.type_name != std::any::type_name::<T>() {
            return Err(ReactiveError::TypeMismatch {
                id: "unknown".to_string(),
                expected: self.type_name.to_string(),
                actual: std::any::type_name::<T>().to_string(),
            });
        }

        let next_revision =
            self.revision
                .checked_add(1)
                .ok_or_else(|| ReactiveError::Internal {
                    reason: "reactive signal revision exhausted".to_string(),
                })?;

        // Update value and revision together while holding the graph write lock.
        let wrapped = AnyValue(Arc::new(value));
        self.value = wrapped.clone();
        self.revision = next_revision;

        // Notify subscribers (ignore send errors - means no subscribers)
        let _ = self.sender.send(wrapped);

        Ok(next_revision)
    }

    /// Subscribe to changes.
    fn subscribe(&self) -> broadcast::Receiver<AnyValue> {
        self.sender.subscribe()
    }
}

/// One value and its revision observed under the same graph lock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalSnapshot<T> {
    /// Published value.
    pub value: T,
    /// Revision assigned at registration or the last successful publication.
    pub revision: u64,
}

/// Result of publishing only when the caller's observed revision remains current.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConditionalEmit {
    /// The value was published at this revision.
    Published {
        /// Revision assigned to the published value.
        revision: u64,
    },
    /// A newer value was already published; no notification was sent.
    Stale {
        /// Revision that rejected the caller's stale replacement.
        current_revision: u64,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Signal Graph
// ─────────────────────────────────────────────────────────────────────────────

/// The signal graph manages reactive state.
///
/// It provides:
/// - Signal registration and storage
/// - Type-safe read/emit operations
/// - Subscription management
/// - (Future) Derived signal computation and dependency tracking
pub struct SignalGraph {
    /// Signal storage, keyed by SignalId
    signals: RwLock<HashMap<SignalId, SignalSlot>>,
}

impl SignalGraph {
    /// Create a new empty signal graph.
    pub fn new() -> Self {
        Self {
            signals: RwLock::new(HashMap::new()),
        }
    }

    /// Register a signal with an initial value.
    pub async fn register<T: Clone + Send + Sync + 'static>(
        &self,
        id: SignalId,
        initial: T,
    ) -> Result<(), ReactiveError> {
        let mut signals = self.signals.write().await;

        if signals.contains_key(&id) {
            return Err(ReactiveError::Internal {
                reason: format!("Signal '{id}' already registered"),
            });
        }

        signals.insert(id, SignalSlot::new(initial));
        Ok(())
    }

    /// Install a missing signal or verify the type of an existing one.
    /// Existing values and subscriptions are retained across retries.
    pub async fn ensure_registered<T: Clone + Send + Sync + 'static>(
        &self,
        id: SignalId,
        initial: T,
    ) -> Result<(), ReactiveError> {
        let mut signals = self.signals.write().await;
        if let Some(slot) = signals.get(&id) {
            slot.read::<T>().map(|_| ()).map_err(|error| match error {
                ReactiveError::TypeMismatch {
                    expected, actual, ..
                } => ReactiveError::TypeMismatch {
                    id: id.to_string(),
                    expected,
                    actual,
                },
                other => other,
            })
        } else {
            signals.insert(id, SignalSlot::new(initial));
            Ok(())
        }
    }

    /// Check if a signal is registered.
    pub async fn is_registered(&self, id: &SignalId) -> bool {
        self.signals.read().await.contains_key(id)
    }

    /// Number of receivers currently attached to one signal.
    pub async fn subscriber_count(&self, id: &SignalId) -> usize {
        self.signals
            .read()
            .await
            .get(id)
            .map_or(0, |slot| slot.sender.receiver_count())
    }

    /// Read the current value of a signal.
    pub async fn read<T: Clone + Send + Sync + 'static>(
        &self,
        id: &SignalId,
    ) -> Result<T, ReactiveError> {
        let signals = self.signals.read().await;

        let slot = signals
            .get(id)
            .ok_or_else(|| ReactiveError::SignalNotFound { id: id.to_string() })?;

        slot.read::<T>().map_err(|e| match e {
            ReactiveError::TypeMismatch {
                expected, actual, ..
            } => ReactiveError::TypeMismatch {
                id: id.to_string(),
                expected,
                actual,
            },
            other => other,
        })
    }

    /// Read a value and its revision atomically.
    pub async fn read_snapshot<T: Clone + Send + Sync + 'static>(
        &self,
        id: &SignalId,
    ) -> Result<SignalSnapshot<T>, ReactiveError> {
        let signals = self.signals.read().await;
        let slot = signals
            .get(id)
            .ok_or_else(|| ReactiveError::SignalNotFound { id: id.to_string() })?;
        Ok(SignalSnapshot {
            value: slot
                .read::<T>()
                .map_err(|error| signal_error_id(error, id))?,
            revision: slot.revision,
        })
    }

    /// Emit a new value to a signal.
    pub async fn emit<T: Clone + Send + Sync + 'static>(
        &self,
        id: &SignalId,
        value: T,
    ) -> Result<(), ReactiveError> {
        let mut signals = self.signals.write().await;

        let slot = signals
            .get_mut(id)
            .ok_or_else(|| ReactiveError::SignalNotFound { id: id.to_string() })?;

        slot.emit(value)
            .map(|_| ())
            .map_err(|error| signal_error_id(error, id))
    }

    /// Apply a fallible synchronous mutation to a clone of the current value.
    /// A rejected mutation leaves the value, revision, and subscribers unchanged.
    pub async fn update<T, R, E>(
        &self,
        id: &SignalId,
        update: impl FnOnce(&mut T) -> Result<R, E>,
    ) -> Result<Result<(R, SignalSnapshot<T>), E>, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        let mut signals = self.signals.write().await;
        let slot = signals
            .get_mut(id)
            .ok_or_else(|| ReactiveError::SignalNotFound { id: id.to_string() })?;
        let mut value = slot
            .read::<T>()
            .map_err(|error| signal_error_id(error, id))?;
        if slot.revision == u64::MAX {
            return Err(ReactiveError::Internal {
                reason: "reactive signal revision exhausted".to_string(),
            });
        }
        let result = match update(&mut value) {
            Ok(result) => result,
            Err(error) => return Ok(Err(error)),
        };
        let revision = slot
            .emit(value.clone())
            .map_err(|error| signal_error_id(error, id))?;
        Ok(Ok((result, SignalSnapshot { value, revision })))
    }

    /// Publish a value only if no update followed the caller's snapshot.
    pub async fn compare_and_emit<T: Clone + Send + Sync + 'static>(
        &self,
        id: &SignalId,
        expected_revision: u64,
        value: T,
    ) -> Result<ConditionalEmit, ReactiveError> {
        let mut signals = self.signals.write().await;
        let slot = signals
            .get_mut(id)
            .ok_or_else(|| ReactiveError::SignalNotFound { id: id.to_string() })?;
        slot.read::<T>()
            .map_err(|error| signal_error_id(error, id))?;
        if slot.revision != expected_revision {
            return Ok(ConditionalEmit::Stale {
                current_revision: slot.revision,
            });
        }
        let revision = slot
            .emit(value)
            .map_err(|error| signal_error_id(error, id))?;
        Ok(ConditionalEmit::Published { revision })
    }

    /// Subscribe to a signal's changes.
    ///
    /// Returns a broadcast receiver that yields type-erased values.
    /// The caller is responsible for downcasting.
    pub async fn subscribe(
        &self,
        id: &SignalId,
    ) -> Result<broadcast::Receiver<AnyValue>, ReactiveError> {
        let signals = self.signals.read().await;

        let slot = signals
            .get(id)
            .ok_or_else(|| ReactiveError::SignalNotFound { id: id.to_string() })?;

        Ok(slot.subscribe())
    }

    /// Get statistics about the signal graph.
    pub async fn stats(&self) -> SignalGraphStats {
        let signals = self.signals.read().await;
        SignalGraphStats {
            signal_count: signals.len(),
        }
    }
}

fn signal_error_id(error: ReactiveError, id: &SignalId) -> ReactiveError {
    match error {
        ReactiveError::TypeMismatch {
            expected, actual, ..
        } => ReactiveError::TypeMismatch {
            id: id.to_string(),
            expected,
            actual,
        },
        other => other,
    }
}

impl Default for SignalGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the signal graph.
#[derive(Debug, Clone)]
pub struct SignalGraphStats {
    /// Number of registered signals
    pub signal_count: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Typed Signal Receiver
// ─────────────────────────────────────────────────────────────────────────────

/// A typed receiver for signal updates.
///
/// Wraps a broadcast receiver and provides type-safe access to values.
pub struct TypedSignalReceiver<T> {
    receiver: broadcast::Receiver<AnyValue>,
    signal_id: SignalId,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Clone + Send + Sync + 'static> TypedSignalReceiver<T> {
    /// Create a new typed receiver.
    pub fn new(receiver: broadcast::Receiver<AnyValue>, signal_id: SignalId) -> Self {
        Self {
            receiver,
            signal_id,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Try to receive the next value without blocking.
    pub fn try_recv(&mut self) -> Option<T> {
        loop {
            match self.receiver.try_recv() {
                Ok(any_value) => {
                    if let Some(value) = any_value.0.downcast_ref::<T>() {
                        return Some(value.clone());
                    }
                    // Type mismatch, skip this value
                    continue;
                }
                Err(_) => return None,
            }
        }
    }

    /// Receive the next value, waiting if necessary.
    pub async fn recv(&mut self) -> Result<T, ReactiveError> {
        loop {
            match self.receiver.recv().await {
                Ok(any_value) => {
                    if let Some(value) = any_value.0.downcast_ref::<T>() {
                        return Ok(value.clone());
                    }
                    // Type mismatch, skip this value
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(ReactiveError::SubscriptionClosed {
                        id: self.signal_id.to_string(),
                    });
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Missed some values, continue receiving
                    continue;
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_signal_registration() {
        let graph = SignalGraph::new();
        let id = SignalId::new("test");

        assert!(!graph.is_registered(&id).await);

        graph.register(id.clone(), 42u32).await.unwrap();

        assert!(graph.is_registered(&id).await);
    }

    #[tokio::test]
    async fn ensure_registered_preserves_value_and_existing_subscriber() {
        let graph = SignalGraph::new();
        let id = SignalId::new("idempotent");
        graph.ensure_registered(id.clone(), 1u32).await.unwrap();
        let mut receiver = graph.subscribe(&id).await.unwrap();

        graph.emit(&id, 2u32).await.unwrap();
        graph.ensure_registered(id.clone(), 99u32).await.unwrap();

        assert_eq!(graph.read::<u32>(&id).await.unwrap(), 2);
        graph.emit(&id, 3u32).await.unwrap();
        assert_eq!(
            *receiver
                .recv()
                .await
                .unwrap()
                .0
                .downcast_ref::<u32>()
                .unwrap(),
            2
        );
        assert_eq!(
            *receiver
                .recv()
                .await
                .unwrap()
                .0
                .downcast_ref::<u32>()
                .unwrap(),
            3
        );
        assert_eq!(graph.stats().await.signal_count, 1);
    }

    #[tokio::test]
    async fn ensure_registered_rejects_type_conflict_without_resetting_slot() {
        let graph = SignalGraph::new();
        let id = SignalId::new("typed_registration");
        graph.ensure_registered(id.clone(), 7u32).await.unwrap();

        let error = graph
            .ensure_registered(id.clone(), String::new())
            .await
            .unwrap_err();
        assert!(
            matches!(error, ReactiveError::TypeMismatch { ref id, .. } if id == "typed_registration")
        );
        assert_eq!(graph.read::<u32>(&id).await.unwrap(), 7);
        assert_eq!(graph.stats().await.signal_count, 1);
    }

    #[tokio::test]
    async fn test_signal_read_write() {
        let graph = SignalGraph::new();
        let id = SignalId::new("counter");

        graph.register(id.clone(), 0u32).await.unwrap();

        // Read initial value
        let value: u32 = graph.read(&id).await.unwrap();
        assert_eq!(value, 0);

        // Emit new value
        graph.emit(&id, 42u32).await.unwrap();

        // Read updated value
        let value: u32 = graph.read(&id).await.unwrap();
        assert_eq!(value, 42);
    }

    #[tokio::test]
    async fn concurrent_updates_preserve_disjoint_fields_and_publish_each_revision() {
        let graph = Arc::new(SignalGraph::new());
        let id = SignalId::new("transaction_fields");
        graph.register(id.clone(), (0u32, 0u32)).await.unwrap();
        let mut receiver = graph.subscribe(&id).await.unwrap();

        let left_graph = graph.clone();
        let left_id = id.clone();
        let left = tokio::spawn(async move {
            for _ in 0..32 {
                left_graph
                    .update(&left_id, |value: &mut (u32, u32)| {
                        value.0 += 1;
                        Ok::<_, ()>(())
                    })
                    .await
                    .unwrap()
                    .unwrap();
            }
        });
        let right_graph = graph.clone();
        let right_id = id.clone();
        let right = tokio::spawn(async move {
            for _ in 0..32 {
                right_graph
                    .update(&right_id, |value: &mut (u32, u32)| {
                        value.1 += 1;
                        Ok::<_, ()>(())
                    })
                    .await
                    .unwrap()
                    .unwrap();
            }
        });
        left.await.unwrap();
        right.await.unwrap();

        assert_eq!(
            graph.read_snapshot::<(u32, u32)>(&id).await.unwrap(),
            SignalSnapshot {
                value: (32, 32),
                revision: 64,
            }
        );
        for _ in 0..64 {
            receiver.try_recv().unwrap();
        }
        assert!(matches!(
            receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn rejected_update_and_stale_compare_do_not_publish() {
        let graph = SignalGraph::new();
        let id = SignalId::new("conditional");
        graph.register(id.clone(), vec![1u32]).await.unwrap();
        let mut receiver = graph.subscribe(&id).await.unwrap();

        let rejected = graph
            .update(&id, |value: &mut Vec<u32>| {
                value.push(2);
                Err::<(), _>("rejected")
            })
            .await
            .unwrap();
        assert_eq!(rejected, Err("rejected"));
        assert_eq!(
            graph.read_snapshot::<Vec<u32>>(&id).await.unwrap().revision,
            0
        );
        assert!(matches!(
            receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));

        assert_eq!(
            graph.compare_and_emit(&id, 0, vec![3u32]).await.unwrap(),
            ConditionalEmit::Published { revision: 1 }
        );
        assert_eq!(
            graph.compare_and_emit(&id, 0, vec![4u32]).await.unwrap(),
            ConditionalEmit::Stale {
                current_revision: 1
            }
        );
        assert_eq!(
            graph.read_snapshot::<Vec<u32>>(&id).await.unwrap(),
            SignalSnapshot {
                value: vec![3],
                revision: 1,
            }
        );
        assert_eq!(
            *receiver
                .try_recv()
                .unwrap()
                .0
                .downcast_ref::<Vec<u32>>()
                .unwrap(),
            vec![3]
        );
        assert!(matches!(
            receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn ordinary_emit_advances_revision_seen_by_conditional_writer() {
        let graph = SignalGraph::new();
        let id = SignalId::new("ordinary_emit_revision");
        graph.register(id.clone(), 0u32).await.unwrap();
        let observed = graph.read_snapshot::<u32>(&id).await.unwrap();
        graph.emit(&id, 1u32).await.unwrap();
        assert_eq!(
            graph
                .compare_and_emit(&id, observed.revision, 2u32)
                .await
                .unwrap(),
            ConditionalEmit::Stale {
                current_revision: 1
            }
        );
        assert_eq!(
            graph.read_snapshot::<u32>(&id).await.unwrap(),
            SignalSnapshot {
                value: 1,
                revision: 1
            }
        );
    }

    #[tokio::test]
    async fn test_signal_not_found() {
        let graph = SignalGraph::new();
        let id = SignalId::new("nonexistent");

        let result: Result<u32, _> = graph.read(&id).await;
        assert!(matches!(result, Err(ReactiveError::SignalNotFound { .. })));
    }

    #[tokio::test]
    async fn test_type_mismatch() {
        let graph = SignalGraph::new();
        let id = SignalId::new("typed");

        graph.register(id.clone(), 42u32).await.unwrap();

        // Try to read as wrong type
        let result: Result<String, _> = graph.read(&id).await;
        assert!(matches!(result, Err(ReactiveError::TypeMismatch { .. })));
    }

    #[tokio::test]
    async fn test_subscription() {
        let graph = Arc::new(SignalGraph::new());
        let id = SignalId::new("observable");

        graph
            .register(id.clone(), "initial".to_string())
            .await
            .unwrap();

        // Create subscription
        let receiver = graph.subscribe(&id).await.unwrap();
        let mut typed_receiver = TypedSignalReceiver::<String>::new(receiver, id.clone());

        // Emit in background
        let graph_clone = graph.clone();
        let id_clone = id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            graph_clone
                .emit(&id_clone, "updated".to_string())
                .await
                .unwrap();
        });

        // Receive update
        let value = typed_receiver.recv().await.unwrap();
        assert_eq!(value, "updated");
    }
}
