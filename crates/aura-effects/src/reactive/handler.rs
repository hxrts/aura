//! Reactive Effect Handler
//!
//! Implements the ReactiveEffects trait using SignalGraph for state management.
//! Supports query-bound signals for automatic updates when facts change.
// Runtime-agnostic handler uses std sync primitives intentionally.
#![allow(clippy::disallowed_types)]

use async_trait::async_trait;
use aura_core::effects::reactive::{
    ReactiveEffects, ReactiveError, Signal, SignalId, SignalStream,
};
use aura_core::query::{FactPredicate, Query};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::graph::SignalGraph;

// ─────────────────────────────────────────────────────────────────────────────
// Reactive Handler
// ─────────────────────────────────────────────────────────────────────────────

/// Production reactive effect handler.
///
/// Implements `ReactiveEffects` using a `SignalGraph` for state management.
/// This handler can be shared across components via `Arc`.
///
/// Supports query-bound signals where the signal's value is derived from
/// executing a query against journal facts. When facts change, bound queries
/// are automatically re-evaluated.
pub struct ReactiveHandler {
    /// The signal graph managing reactive state
    graph: Arc<SignalGraph>,
    /// Sync-safe set of registered signal IDs for fast is_registered checks
    registered_ids: Arc<RwLock<HashSet<SignalId>>>,
    /// Maps signal IDs to their query dependencies (for query-bound signals)
    query_deps: Arc<RwLock<HashMap<SignalId, Vec<FactPredicate>>>>,
    /// Background task registry for subscription forwarding
    tasks: Arc<ReactiveTaskRegistry>,
}

impl ReactiveHandler {
    /// Create a new reactive handler with an empty signal graph.
    pub fn new() -> Self {
        Self {
            graph: Arc::new(SignalGraph::new()),
            registered_ids: Arc::new(RwLock::new(HashSet::new())),
            query_deps: Arc::new(RwLock::new(HashMap::new())),
            tasks: Arc::new(ReactiveTaskRegistry::new()),
        }
    }

    /// Create a handler with a shared signal graph.
    ///
    /// This allows multiple handlers to share the same reactive state.
    pub fn with_graph(graph: Arc<SignalGraph>) -> Self {
        Self {
            graph,
            registered_ids: Arc::new(RwLock::new(HashSet::new())),
            query_deps: Arc::new(RwLock::new(HashMap::new())),
            tasks: Arc::new(ReactiveTaskRegistry::new()),
        }
    }

    /// Create a handler with shared graph and registration tracking.
    pub fn with_graph_and_registry(
        graph: Arc<SignalGraph>,
        registered_ids: Arc<RwLock<HashSet<SignalId>>>,
    ) -> Self {
        Self {
            graph,
            registered_ids,
            query_deps: Arc::new(RwLock::new(HashMap::new())),
            tasks: Arc::new(ReactiveTaskRegistry::new()),
        }
    }

    /// Get a reference to the underlying signal graph.
    pub fn graph(&self) -> &Arc<SignalGraph> {
        &self.graph
    }

    /// Get statistics about the handler's signal graph.
    pub async fn stats(&self) -> super::graph::SignalGraphStats {
        self.graph.stats().await
    }

    /// Get all signals that depend on a given fact predicate.
    ///
    /// Used internally to find which signals need re-evaluation when facts change.
    fn signals_for_predicate(&self, predicate: &FactPredicate) -> Vec<SignalId> {
        self.query_deps
            .read()
            .map(|deps| {
                deps.iter()
                    .filter_map(|(signal_id, predicates)| {
                        if predicates.iter().any(|p| p.matches(predicate)) {
                            Some(signal_id.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Default for ReactiveHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ReactiveHandler {
    fn clone(&self) -> Self {
        Self {
            graph: self.graph.clone(),
            registered_ids: self.registered_ids.clone(),
            query_deps: self.query_deps.clone(),
            tasks: self.tasks.clone(),
        }
    }
}

impl Drop for ReactiveHandler {
    fn drop(&mut self) {
        if Arc::strong_count(&self.tasks) == 1 {
            self.tasks.shutdown();
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Task registry
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct ReactiveTaskRegistry {
    shutdown_tx: watch::Sender<bool>,
    handles: std::sync::Mutex<Vec<JoinHandle<()>>>,
}

impl ReactiveTaskRegistry {
    fn new() -> Self {
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        Self {
            shutdown_tx,
            handles: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn spawn_cancellable<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = shutdown_rx.changed() => {}
                _ = fut => {}
            }
        });
        if let Ok(mut handles) = self.handles.lock() {
            handles.push(handle);
        }
    }

    fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
        if let Ok(mut handles) = self.handles.lock() {
            for handle in handles.drain(..) {
                handle.abort();
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ReactiveEffects Implementation
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl ReactiveEffects for ReactiveHandler {
    async fn read<T>(&self, signal: &Signal<T>) -> Result<T, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.graph.read(signal.id()).await
    }

    async fn emit<T>(&self, signal: &Signal<T>, value: T) -> Result<(), ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.graph.emit(signal.id(), value).await
    }

    fn subscribe<T>(&self, signal: &Signal<T>) -> SignalStream<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        // We need to create a synchronous subscription here since the trait
        // method isn't async. We'll wrap the async operation in a blocking call.
        // In practice, signals should be pre-registered before subscribing.

        // Create a channel for this subscription
        let (tx, rx) = broadcast::channel::<T>(256);

        // Spawn a task to forward from the graph's subscription
        let graph = self.graph.clone();
        let signal_id = signal.id().clone();

        self.tasks.spawn_cancellable(async move {
            if let Ok(mut receiver) = graph.subscribe(&signal_id).await {
                loop {
                    match receiver.recv().await {
                        Ok(any_value) => {
                            if let Some(value) = any_value.0.downcast_ref::<T>() {
                                if tx.send(value.clone()).is_err() {
                                    // No receivers, stop forwarding
                                    break;
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    }
                }
            }
        });

        SignalStream::new(rx, signal.id().clone())
    }

    async fn register<T>(&self, signal: &Signal<T>, initial: T) -> Result<(), ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        // Register with the graph
        self.graph.register(signal.id().clone(), initial).await?;

        // Track the registration in our sync-safe set
        if let Ok(mut ids) = self.registered_ids.write() {
            ids.insert(signal.id().clone());
        }

        Ok(())
    }

    fn is_registered(&self, signal_id: &SignalId) -> bool {
        // Use our sync-safe registration tracking set
        // This avoids needing to block on async operations
        self.registered_ids
            .read()
            .map(|ids| ids.contains(signal_id))
            .unwrap_or(false)
    }

    async fn register_query<Q: Query>(
        &self,
        signal: &Signal<Q::Result>,
        query: Q,
    ) -> Result<(), ReactiveError> {
        // Get the query's dependencies for invalidation tracking
        let deps = query.dependencies();

        // Register the signal with default value.
        // Initial query execution is the caller's responsibility via QueryEffects.
        // This separation keeps ReactiveHandler focused on signal management.
        let initial: Q::Result = Default::default();
        self.register(signal, initial).await?;

        // Store dependencies for predicate-based invalidation
        if let Ok(mut deps_map) = self.query_deps.write() {
            deps_map.insert(signal.id().clone(), deps);
        }

        Ok(())
    }

    fn query_dependencies(&self, signal_id: &SignalId) -> Option<Vec<FactPredicate>> {
        self.query_deps
            .read()
            .ok()
            .and_then(|deps| deps.get(signal_id).cloned())
    }

    async fn invalidate_queries(&self, changed: &FactPredicate) {
        // Find all signals that depend on this predicate
        let affected_signals = self.signals_for_predicate(changed);

        // Log affected signals for debugging.
        // Query re-execution and signal emission is handled by AppCore::commit_pending_facts_and_emit()
        // which has access to both QueryEffects and the view snapshot.
        for signal_id in affected_signals {
            tracing::debug!(
                signal_id = %signal_id,
                predicate = ?changed,
                "Signal invalidated due to fact change"
            );
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
    async fn test_handler_creation() {
        let handler = ReactiveHandler::new();
        let stats = handler.stats().await;
        assert_eq!(stats.signal_count, 0);
    }

    #[tokio::test]
    async fn test_handler_register_and_read() {
        let handler = ReactiveHandler::new();
        let signal: Signal<u32> = Signal::new("counter");

        handler.register(&signal, 42).await.unwrap();

        let value = handler.read(&signal).await.unwrap();
        assert_eq!(value, 42);
    }

    #[tokio::test]
    async fn test_handler_emit() {
        let handler = ReactiveHandler::new();
        let signal: Signal<String> = Signal::new("message");

        handler
            .register(&signal, "hello".to_string())
            .await
            .unwrap();

        handler.emit(&signal, "world".to_string()).await.unwrap();

        let value = handler.read(&signal).await.unwrap();
        assert_eq!(value, "world");
    }

    #[tokio::test]
    async fn test_shared_graph() {
        let graph = Arc::new(SignalGraph::new());
        let handler1 = ReactiveHandler::with_graph(graph.clone());
        let handler2 = ReactiveHandler::with_graph(graph);

        let signal: Signal<i32> = Signal::new("shared");

        // Register via handler1
        handler1.register(&signal, 100).await.unwrap();

        // Read via handler2
        let value: i32 = handler2.read(&signal).await.unwrap();
        assert_eq!(value, 100);

        // Emit via handler2
        handler2.emit(&signal, 200).await.unwrap();

        // Read via handler1
        let value: i32 = handler1.read(&signal).await.unwrap();
        assert_eq!(value, 200);
    }

    #[tokio::test]
    async fn test_is_registered() {
        let handler = ReactiveHandler::new();
        let signal: Signal<bool> = Signal::new("flag");

        assert!(!handler.is_registered(signal.id()));

        handler.register(&signal, true).await.unwrap();

        assert!(handler.is_registered(signal.id()));
    }

    // === Edge Case Tests for Phase 6.4 ===

    #[tokio::test]
    async fn test_empty_string_signal() {
        let handler = ReactiveHandler::new();
        let signal: Signal<String> = Signal::new("empty");

        handler.register(&signal, String::new()).await.unwrap();

        let value = handler.read(&signal).await.unwrap();
        assert_eq!(value, "");

        // Emit another empty string
        handler.emit(&signal, String::new()).await.unwrap();
        let value = handler.read(&signal).await.unwrap();
        assert_eq!(value, "");
    }

    #[tokio::test]
    async fn test_zero_value_signal() {
        let handler = ReactiveHandler::new();
        let signal: Signal<i64> = Signal::new("zero");

        handler.register(&signal, 0).await.unwrap();

        let value = handler.read(&signal).await.unwrap();
        assert_eq!(value, 0);
    }

    #[tokio::test]
    async fn test_rapid_updates() {
        let handler = ReactiveHandler::new();
        let signal: Signal<u32> = Signal::new("counter");

        handler.register(&signal, 0).await.unwrap();

        // Rapid fire updates
        for i in 1..=100 {
            handler.emit(&signal, i).await.unwrap();
        }

        // Final value should be 100
        let value = handler.read(&signal).await.unwrap();
        assert_eq!(value, 100);
    }

    #[tokio::test]
    async fn test_read_unregistered_signal() {
        let handler = ReactiveHandler::new();
        let signal: Signal<u32> = Signal::new("never_registered");

        let result = handler.read(&signal).await;
        assert!(matches!(result, Err(ReactiveError::SignalNotFound { .. })));
    }

    #[tokio::test]
    async fn test_emit_unregistered_signal() {
        let handler = ReactiveHandler::new();
        let signal: Signal<u32> = Signal::new("never_registered");

        let result = handler.emit(&signal, 42).await;
        assert!(matches!(result, Err(ReactiveError::SignalNotFound { .. })));
    }

    #[tokio::test]
    async fn test_duplicate_registration() {
        let handler = ReactiveHandler::new();
        let signal: Signal<u32> = Signal::new("duplicate");

        // First registration succeeds
        handler.register(&signal, 1).await.unwrap();

        // Second registration fails
        let result = handler.register(&signal, 2).await;
        assert!(matches!(result, Err(ReactiveError::Internal { .. })));

        // Original value preserved
        let value = handler.read(&signal).await.unwrap();
        assert_eq!(value, 1);
    }

    #[tokio::test]
    async fn test_clone_handler_shares_state() {
        let handler1 = ReactiveHandler::new();
        let signal: Signal<u32> = Signal::new("cloned");

        handler1.register(&signal, 10).await.unwrap();

        // Clone the handler
        let handler2 = handler1.clone();

        // Both handlers see the same value
        let v1: u32 = handler1.read(&signal).await.unwrap();
        let v2: u32 = handler2.read(&signal).await.unwrap();
        assert_eq!(v1, v2);

        // Emit via handler2
        handler2.emit(&signal, 20).await.unwrap();

        // Both handlers see the update
        let v1: u32 = handler1.read(&signal).await.unwrap();
        let v2: u32 = handler2.read(&signal).await.unwrap();
        assert_eq!(v1, 20);
        assert_eq!(v2, 20);
    }

    #[tokio::test]
    async fn test_complex_type_signal() {
        #[derive(Clone, Debug, PartialEq)]
        struct ComplexState {
            count: u32,
            label: String,
            values: Vec<i32>,
        }

        let handler = ReactiveHandler::new();
        let signal: Signal<ComplexState> = Signal::new("complex");

        let initial = ComplexState {
            count: 0,
            label: "initial".to_string(),
            values: vec![1, 2, 3],
        };

        handler.register(&signal, initial.clone()).await.unwrap();

        let read_state = handler.read(&signal).await.unwrap();
        assert_eq!(read_state, initial);

        // Update with new complex state
        let updated = ComplexState {
            count: 42,
            label: "updated".to_string(),
            values: vec![4, 5, 6, 7],
        };

        handler.emit(&signal, updated.clone()).await.unwrap();

        let read_updated = handler.read(&signal).await.unwrap();
        assert_eq!(read_updated, updated);
    }

    #[tokio::test]
    async fn test_option_type_signal() {
        let handler = ReactiveHandler::new();
        let signal: Signal<Option<String>> = Signal::new("optional");

        handler.register(&signal, None).await.unwrap();

        let value = handler.read(&signal).await.unwrap();
        assert_eq!(value, None);

        handler
            .emit(&signal, Some("value".to_string()))
            .await
            .unwrap();

        let value = handler.read(&signal).await.unwrap();
        assert_eq!(value, Some("value".to_string()));

        handler.emit(&signal, None).await.unwrap();

        let value = handler.read(&signal).await.unwrap();
        assert_eq!(value, None);
    }
}
