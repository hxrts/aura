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
    fn emit<T: Clone + Send + Sync + 'static>(&mut self, value: T) -> Result<(), ReactiveError> {
        // Verify type matches
        if self.type_name != std::any::type_name::<T>() {
            return Err(ReactiveError::TypeMismatch {
                id: "unknown".to_string(),
                expected: self.type_name.to_string(),
                actual: std::any::type_name::<T>().to_string(),
            });
        }

        // Update value
        let wrapped = AnyValue(Arc::new(value));
        self.value = wrapped.clone();

        // Notify subscribers (ignore send errors - means no subscribers)
        let _ = self.sender.send(wrapped);

        Ok(())
    }

    /// Subscribe to changes.
    fn subscribe(&self) -> broadcast::Receiver<AnyValue> {
        self.sender.subscribe()
    }
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
                reason: format!("Signal '{}' already registered", id),
            });
        }

        signals.insert(id, SignalSlot::new(initial));
        Ok(())
    }

    /// Check if a signal is registered.
    pub async fn is_registered(&self, id: &SignalId) -> bool {
        self.signals.read().await.contains_key(id)
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

        slot.emit(value).map_err(|e| match e {
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
