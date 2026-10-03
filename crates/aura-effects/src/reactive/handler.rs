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
#[cfg(feature = "test-support")]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use tokio::sync::watch;
#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinHandle;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

use super::graph::{ConditionalEmit, SignalGraph, SignalSnapshot};

const REACTIVE_SUBSCRIPTION_BUFFER_CAPACITY: usize = 256;

/// Platform task spawner for reactive lifecycle tests.
#[cfg(feature = "test-support")]
#[derive(Debug)]
pub struct TestTaskSpawner;

/// Counts task allocations while running them with the test runtime.
#[cfg(feature = "test-support")]
#[derive(Debug, Default)]
pub struct CountingTestTaskSpawner {
    spawned: AtomicUsize,
    failure: Arc<tokio::sync::Mutex<Option<aura_core::AuraError>>>,
}

#[cfg(feature = "test-support")]
impl CountingTestTaskSpawner {
    /// First native required-task fault retained by this test supervisor.
    pub async fn failure(&self) -> Option<aura_core::AuraError> {
        self.failure.lock().await.clone()
    }

    /// Number of owned tasks allocated through this spawner.
    pub fn spawned_count(&self) -> usize {
        self.spawned.load(Ordering::SeqCst)
    }
}

#[cfg(feature = "test-support")]
impl aura_core::effects::task::TaskSpawner for CountingTestTaskSpawner {
    fn spawn_fallible_cancellable(
        &self,
        _name: &'static str,
        fut: futures::future::BoxFuture<'static, Result<(), aura_core::AuraError>>,
        token: Arc<dyn aura_core::effects::task::CancellationToken>,
    ) -> Result<(), aura_core::AuraError> {
        let failure = self.failure.clone();
        self.spawn_cancellable(
            Box::pin(async move {
                if let Err(error) = fut.await {
                    let mut first = failure.lock().await;
                    if first.is_none() {
                        *first = Some(error);
                    }
                }
            }),
            token,
        );
        Ok(())
    }
    fn spawn_local_fallible_cancellable(
        &self,
        _name: &'static str,
        fut: futures::future::LocalBoxFuture<'static, Result<(), aura_core::AuraError>>,
        token: Arc<dyn aura_core::effects::task::CancellationToken>,
    ) -> Result<(), aura_core::AuraError> {
        let failure = self.failure.clone();
        self.spawn_local_cancellable(
            Box::pin(async move {
                if let Err(error) = fut.await {
                    let mut first = failure.lock().await;
                    if first.is_none() {
                        *first = Some(error);
                    }
                }
            }),
            token,
        );
        Ok(())
    }

    fn spawn(&self, fut: futures::future::BoxFuture<'static, ()>) {
        self.spawned.fetch_add(1, Ordering::SeqCst);
        <TestTaskSpawner as aura_core::effects::task::TaskSpawner>::spawn(&TestTaskSpawner, fut);
    }

    fn spawn_cancellable(
        &self,
        fut: futures::future::BoxFuture<'static, ()>,
        token: Arc<dyn aura_core::effects::task::CancellationToken>,
    ) {
        self.spawned.fetch_add(1, Ordering::SeqCst);
        <TestTaskSpawner as aura_core::effects::task::TaskSpawner>::spawn_cancellable(
            &TestTaskSpawner,
            fut,
            token,
        );
    }

    fn spawn_local(&self, fut: futures::future::LocalBoxFuture<'static, ()>) {
        self.spawned.fetch_add(1, Ordering::SeqCst);
        <TestTaskSpawner as aura_core::effects::task::TaskSpawner>::spawn_local(
            &TestTaskSpawner,
            fut,
        );
    }

    fn spawn_local_cancellable(
        &self,
        fut: futures::future::LocalBoxFuture<'static, ()>,
        token: Arc<dyn aura_core::effects::task::CancellationToken>,
    ) {
        self.spawned.fetch_add(1, Ordering::SeqCst);
        <TestTaskSpawner as aura_core::effects::task::TaskSpawner>::spawn_local_cancellable(
            &TestTaskSpawner,
            fut,
            token,
        );
    }

    fn cancellation_token(&self) -> Arc<dyn aura_core::effects::task::CancellationToken> {
        <TestTaskSpawner as aura_core::effects::task::TaskSpawner>::cancellation_token(
            &TestTaskSpawner,
        )
    }
}

#[cfg(all(feature = "test-support", not(target_arch = "wasm32")))]
impl aura_core::effects::task::TaskSpawner for TestTaskSpawner {
    fn spawn(&self, fut: futures::future::BoxFuture<'static, ()>) {
        tokio::spawn(fut);
    }

    fn spawn_cancellable(
        &self,
        fut: futures::future::BoxFuture<'static, ()>,
        token: Arc<dyn aura_core::effects::task::CancellationToken>,
    ) {
        tokio::spawn(async move {
            tokio::select! {
                _ = token.cancelled() => {},
                _ = fut => {},
            }
        });
    }

    fn spawn_local(&self, fut: futures::future::LocalBoxFuture<'static, ()>) {
        tokio::task::spawn_local(fut);
    }

    fn spawn_local_cancellable(
        &self,
        fut: futures::future::LocalBoxFuture<'static, ()>,
        token: Arc<dyn aura_core::effects::task::CancellationToken>,
    ) {
        tokio::task::spawn_local(async move {
            tokio::select! {
                _ = token.cancelled() => {},
                _ = fut => {},
            }
        });
    }

    fn cancellation_token(&self) -> Arc<dyn aura_core::effects::task::CancellationToken> {
        Arc::new(aura_core::effects::task::NeverCancel)
    }
}

#[cfg(all(feature = "test-support", target_arch = "wasm32"))]
impl aura_core::effects::task::TaskSpawner for TestTaskSpawner {
    fn spawn(&self, fut: futures::future::BoxFuture<'static, ()>) {
        wasm_bindgen_futures::spawn_local(fut);
    }

    fn spawn_cancellable(
        &self,
        fut: futures::future::BoxFuture<'static, ()>,
        token: Arc<dyn aura_core::effects::task::CancellationToken>,
    ) {
        use futures::FutureExt;
        wasm_bindgen_futures::spawn_local(async move {
            futures::select! {
                _ = token.cancelled().fuse() => {},
                _ = fut.fuse() => {},
            }
        });
    }

    fn spawn_local(&self, fut: futures::future::LocalBoxFuture<'static, ()>) {
        wasm_bindgen_futures::spawn_local(fut);
    }

    fn spawn_local_cancellable(
        &self,
        fut: futures::future::LocalBoxFuture<'static, ()>,
        token: Arc<dyn aura_core::effects::task::CancellationToken>,
    ) {
        use futures::FutureExt;
        wasm_bindgen_futures::spawn_local(async move {
            futures::select! {
                _ = token.cancelled().fuse() => {},
                _ = fut.fuse() => {},
            }
        });
    }

    fn cancellation_token(&self) -> Arc<dyn aura_core::effects::task::CancellationToken> {
        Arc::new(aura_core::effects::task::NeverCancel)
    }
}

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

    /// Read a value and the revision at which it was published.
    pub async fn read_snapshot<T>(
        &self,
        signal: &Signal<T>,
    ) -> Result<SignalSnapshot<T>, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.graph.read_snapshot(signal.id()).await
    }

    /// Serialize a fallible mutation against other publications to this graph.
    /// The callback is synchronous and must not await or perform external effects.
    pub async fn update_signal<T, R, E>(
        &self,
        signal: &Signal<T>,
        update: impl FnOnce(&mut T) -> Result<R, E>,
    ) -> Result<Result<(R, SignalSnapshot<T>), E>, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.graph.update(signal.id(), update).await
    }

    /// Publish only if the caller's observed revision is still current.
    pub async fn compare_and_emit<T>(
        &self,
        signal: &Signal<T>,
        expected_revision: u64,
        value: T,
    ) -> Result<ConditionalEmit, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.graph
            .compare_and_emit(signal.id(), expected_revision, value)
            .await
    }

    /// Subscribe after the graph receiver has attached. The returned stream
    /// cannot miss an emission between subscription setup and task scheduling.
    pub async fn subscribe_attached<T>(
        &self,
        signal: &Signal<T>,
    ) -> Result<SignalStream<T>, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.graph.read::<T>(signal.id()).await?;
        let receiver = self.graph.subscribe(signal.id()).await?;
        Ok(self.forward_subscription(signal.id().clone(), receiver))
    }

    fn forward_subscription<T>(
        &self,
        signal_id: SignalId,
        mut receiver: broadcast::Receiver<super::graph::AnyValue>,
    ) -> SignalStream<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        let (tx, rx) = broadcast::channel::<T>(REACTIVE_SUBSCRIPTION_BUFFER_CAPACITY);
        let stream_id = signal_id.clone();
        self.tasks.spawn_cancellable(async move {
            loop {
                tokio::select! {
                    _ = tx.closed() => break,
                    received = receiver.recv() => match received {
                        Ok(any_value) => {
                            if let Some(value) = any_value.0.downcast_ref::<T>() {
                                if tx.send(value.clone()).is_err() {
                                    break;
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(signal_id = %signal_id, skipped,
                                "reactive subscription lagged; updates were dropped");
                        }
                    }
                }
            }
        });
        SignalStream::new(rx, stream_id)
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
    #[cfg(not(target_arch = "wasm32"))]
    handles: std::sync::Mutex<Vec<JoinHandle<()>>>,
}

impl ReactiveTaskRegistry {
    fn new() -> Self {
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        Self {
            shutdown_tx,
            #[cfg(not(target_arch = "wasm32"))]
            handles: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn spawn_cancellable<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        #[cfg(target_arch = "wasm32")]
        spawn_local(async move {
            tokio::select! {
                _ = shutdown_rx.changed() => {}
                _ = fut => {}
            }
        });

        #[cfg(not(target_arch = "wasm32"))]
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = shutdown_rx.changed() => {}
                _ = fut => {}
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(mut handles) = self.handles.lock() {
            handles.push(handle);
        }
    }

    fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
        #[cfg(not(target_arch = "wasm32"))]
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

    fn subscribe<T>(&self, signal: &Signal<T>) -> Result<SignalStream<T>, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        if !self.is_registered(signal.id()) {
            return Err(ReactiveError::SignalNotFound {
                id: signal.id().to_string(),
            });
        }

        // We need to create a synchronous subscription here since the trait
        // method isn't async. We'll wrap the async operation in a blocking call.
        // In practice, signals should be pre-registered before subscribing.

        // Spawn a task to forward from the graph's subscription
        let graph = self.graph.clone();
        let signal_id = signal.id().clone();
        let (tx, rx) = broadcast::channel::<T>(REACTIVE_SUBSCRIPTION_BUFFER_CAPACITY);

        self.tasks.spawn_cancellable(async move {
            match graph.subscribe(&signal_id).await {
                Ok(mut receiver) => loop {
                    tokio::select! {
                    _ = tx.closed() => break,
                    received = receiver.recv() => match received {
                        Ok(any_value) => {
                            if let Some(value) = any_value.0.downcast_ref::<T>() {
                                if tx.send(value.clone()).is_err() {
                                    // No receivers, stop forwarding
                                    break;
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(
                                signal_id = %signal_id,
                                skipped,
                                "reactive subscription lagged; updates were dropped"
                            );
                            continue;
                        }
                    }
                    }
                },
                Err(error) => {
                    tracing::warn!(
                        signal_id = %signal_id,
                        error = %error,
                        "reactive subscription forwarding task exited before attaching"
                    );
                }
            }
        });

        Ok(SignalStream::new(rx, signal.id().clone()))
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

    async fn ensure_registered<T>(
        &self,
        signal: &Signal<T>,
        initial: T,
    ) -> Result<(), ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.graph
            .ensure_registered(signal.id().clone(), initial)
            .await?;
        self.registered_ids
            .write()
            .map_err(|_| ReactiveError::Internal {
                reason: "reactive registration tracking lock poisoned".to_string(),
            })?
            .insert(signal.id().clone());
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

        // Retain the current value and subscribers on binding retries.
        // Initial query execution is the caller's responsibility via QueryEffects.
        // This separation keeps ReactiveHandler focused on signal management.
        let initial: Q::Result = Default::default();
        self.ensure_registered(signal, initial).await?;

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
    async fn ensure_registered_keeps_live_value_and_attached_subscription() {
        let handler = ReactiveHandler::new();
        let signal: Signal<u32> = Signal::new("handler_idempotent");
        handler.ensure_registered(&signal, 1).await.unwrap();
        let mut stream = handler.subscribe_attached(&signal).await.unwrap();

        handler.emit(&signal, 2).await.unwrap();
        handler.ensure_registered(&signal, 99).await.unwrap();

        assert_eq!(handler.read(&signal).await.unwrap(), 2);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), stream.recv())
                .await
                .unwrap()
                .unwrap(),
            2
        );
        assert_eq!(handler.stats().await.signal_count, 1);
    }

    #[tokio::test]
    async fn subscribe_attached_fails_before_registration() {
        let handler = ReactiveHandler::new();
        let signal: Signal<u32> = Signal::new("missing_attached");
        let result = handler.subscribe_attached(&signal).await;
        assert!(
            matches!(result, Err(ReactiveError::SignalNotFound { id }) if id == "missing_attached")
        );
    }

    #[tokio::test]
    async fn subscribe_attached_receives_emit_before_forwarder_is_scheduled() {
        let handler = ReactiveHandler::new();
        let signal: Signal<u32> = Signal::new("attached_first_emit");
        handler.register(&signal, 0).await.unwrap();
        let mut stream = handler.subscribe_attached(&signal).await.unwrap();

        // No yield between attachment and emission: forwarding may not have run yet.
        handler.emit(&signal, 1).await.unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), stream.recv())
                .await
                .unwrap()
                .unwrap(),
            1
        );
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
    async fn handler_transaction_api_preserves_revision_and_stream_delivery() {
        let handler = ReactiveHandler::new();
        let signal: Signal<Vec<u32>> = Signal::new("handler_transaction");
        handler.register(&signal, vec![]).await.unwrap();
        let mut stream = handler.subscribe_attached(&signal).await.unwrap();

        assert_eq!(handler.read_snapshot(&signal).await.unwrap().revision, 0);
        let result = handler
            .update_signal(&signal, |items| {
                items.push(7);
                Ok::<_, ()>(items.len())
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            result,
            (
                1,
                SignalSnapshot {
                    value: vec![7],
                    revision: 1
                }
            )
        );
        assert_eq!(
            handler.compare_and_emit(&signal, 0, vec![8]).await.unwrap(),
            ConditionalEmit::Stale {
                current_revision: 1
            }
        );
        assert_eq!(stream.recv().await.unwrap(), vec![7]);
        assert_eq!(handler.read_snapshot(&signal).await.unwrap().value, vec![7]);
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
    async fn test_subscribe_unregistered_signal_fails_fast() {
        let handler = ReactiveHandler::new();
        let signal: Signal<u32> = Signal::new("never_registered");

        let result = handler.subscribe(&signal);
        assert!(matches!(result, Err(ReactiveError::SignalNotFound { .. })));
    }

    #[tokio::test]
    async fn test_subscription_lag_returns_newer_snapshot_after_drops() {
        let handler = ReactiveHandler::new();
        let signal: Signal<u32> = Signal::new("lagged");

        handler.register(&signal, 0).await.unwrap();
        let mut stream = handler.subscribe(&signal).unwrap();

        for value in 1..=(REACTIVE_SUBSCRIPTION_BUFFER_CAPACITY as u32 + 32) {
            handler.emit(&signal, value).await.unwrap();
        }

        let received = stream.recv().await.unwrap();
        assert!(received > 1);
        assert_eq!(
            handler.read(&signal).await.unwrap(),
            REACTIVE_SUBSCRIPTION_BUFFER_CAPACITY as u32 + 32
        );
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
