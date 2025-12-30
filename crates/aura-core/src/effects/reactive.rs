//! Reactive Effect Traits (FRP as Algebraic Effects)
//!
//! This module unifies functional reactive programming with the algebraic effect system.
//! FRP operations (reading signals, emitting values, subscribing to changes) are modeled
//! as effects that handlers can interpret.
//!
//! # Effect Classification
//!
//! - **Category**: Infrastructure Effect
//! - **Implementation**: `aura-effects` (Layer 3)
//! - **Usage**: All crates needing reactive state management
//!
//! # Core Concepts
//!
//! - **Signal<T>**: A time-varying value of type T (behavior in FRP terminology)
//! - **SignalStream<T>**: A stream of value changes (event stream in FRP terminology)
//! - **ReactiveEffects**: The effect trait for all reactive operations
//! - **Query-bound signals**: Signals that automatically update when facts change
//!
//! # Design Principles
//!
//! 1. **Effects all the way down**: Reading a signal is an effect, not a direct memory access
//! 2. **Composition**: Reactive effects compose with other effects (auth, journaling, etc.)
//! 3. **Testability**: Mock handlers enable deterministic testing of reactive flows
//! 4. **Type safety**: Signals are phantom-typed for compile-time correctness
//! 5. **Query binding**: Signals can be bound to queries for automatic updates

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::query::{FactPredicate, Query};

// ─────────────────────────────────────────────────────────────────────────────
// Error Types
// ─────────────────────────────────────────────────────────────────────────────

/// Error type for reactive operations.
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum ReactiveError {
    /// Signal not found in the reactive graph
    #[error("Signal not found: {id}")]
    SignalNotFound { id: String },

    /// Type mismatch when reading or emitting
    #[error("Type mismatch for signal {id}: expected {expected}, got {actual}")]
    TypeMismatch {
        id: String,
        expected: String,
        actual: String,
    },

    /// Subscription channel closed
    #[error("Subscription channel closed for signal: {id}")]
    SubscriptionClosed { id: String },

    /// Emission failed (e.g., no subscribers, channel full)
    #[error("Failed to emit to signal {id}: {reason}")]
    EmissionFailed { id: String, reason: String },

    /// Derivation cycle detected
    #[error("Cycle detected in signal derivation: {path}")]
    CycleDetected { path: String },

    /// Handler not available
    #[error("Reactive handler not available")]
    HandlerUnavailable,

    /// Internal error
    #[error("Internal reactive error: {reason}")]
    Internal { reason: String },
}

// ─────────────────────────────────────────────────────────────────────────────
// Signal Types
// ─────────────────────────────────────────────────────────────────────────────

/// Unique identifier for a signal.
///
/// SignalIds are globally unique and can be used to look up signals
/// in the reactive graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SignalId(String);

impl SignalId {
    /// Create a new signal ID from a string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Create a unique signal ID with an auto-generated suffix.
    pub fn unique(prefix: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        Self(format!("{prefix}:{id}"))
    }

    /// Get the string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SignalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for SignalId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for SignalId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

/// A typed signal representing a time-varying value.
///
/// Signals are the core primitive of the reactive system. They represent
/// values that change over time and can be:
/// - Read (get current value)
/// - Emitted to (update value)
/// - Subscribed to (receive change notifications)
/// - Derived from (computed from other signals)
///
/// The type parameter T is phantom - actual storage is handled by the
/// reactive handler, enabling type-safe access without runtime type tags.
#[derive(Debug)]
pub struct Signal<T> {
    /// Unique identifier for this signal
    id: SignalId,
    /// Phantom type marker
    _phantom: PhantomData<T>,
}

impl<T> Signal<T> {
    /// Create a new signal with the given ID.
    pub fn new(id: impl Into<SignalId>) -> Self {
        Self {
            id: id.into(),
            _phantom: PhantomData,
        }
    }

    /// Create a signal with a unique auto-generated ID.
    pub fn unique(prefix: &str) -> Self {
        Self {
            id: SignalId::unique(prefix),
            _phantom: PhantomData,
        }
    }

    /// Get the signal's ID.
    pub fn id(&self) -> &SignalId {
        &self.id
    }
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<T> PartialEq for Signal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for Signal<T> {}

impl<T> Hash for Signal<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Signal Stream
// ─────────────────────────────────────────────────────────────────────────────

/// A stream of signal value changes.
///
/// SignalStream wraps a broadcast receiver and provides filtering capabilities.
/// It implements async iteration for use in event loops.
#[allow(clippy::type_complexity)]
pub struct SignalStream<T: Clone> {
    /// The underlying broadcast receiver
    receiver: broadcast::Receiver<T>,
    /// Optional filter predicate
    filter: Option<Box<dyn Fn(&T) -> bool + Send + Sync>>,
    /// Signal ID for error reporting
    signal_id: SignalId,
}

impl<T: Clone> SignalStream<T> {
    /// Create a new signal stream.
    pub fn new(receiver: broadcast::Receiver<T>, signal_id: SignalId) -> Self {
        Self {
            receiver,
            filter: None,
            signal_id,
        }
    }

    /// Add a filter to this stream.
    ///
    /// Only values matching the predicate will be yielded.
    pub fn filter<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&T) -> bool + Send + Sync + 'static,
    {
        self.filter = Some(Box::new(predicate));
        self
    }

    /// Try to receive the next value without blocking.
    ///
    /// Returns `None` if no value is available or the channel is closed.
    pub fn try_recv(&mut self) -> Option<T> {
        loop {
            match self.receiver.try_recv() {
                Ok(value) => {
                    if let Some(ref filter) = self.filter {
                        if filter(&value) {
                            return Some(value);
                        }
                        // Value didn't match filter, try again
                        continue;
                    }
                    return Some(value);
                }
                Err(_) => return None,
            }
        }
    }

    /// Receive the next value, waiting if necessary.
    ///
    /// Returns an error if the channel is closed.
    pub async fn recv(&mut self) -> Result<T, ReactiveError> {
        loop {
            match self.receiver.recv().await {
                Ok(value) => {
                    if let Some(ref filter) = self.filter {
                        if filter(&value) {
                            return Ok(value);
                        }
                        // Value didn't match filter, try again
                        continue;
                    }
                    return Ok(value);
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
// Reactive Effects Trait
// ─────────────────────────────────────────────────────────────────────────────

/// Reactive effects for FRP-style state management.
///
/// This trait unifies the effect system with functional reactive programming.
/// All state access and mutation goes through these effects, enabling:
/// - Transparent reactivity (handlers track dependencies)
/// - Effect composition (reactive ops compose with other effects)
/// - Testability (mock handlers for deterministic testing)
///
/// # Example
///
/// ```ignore
/// // Reading a signal
/// let chat_state = effects.read(&CHAT_SIGNAL).await?;
///
/// // Emitting to a signal
/// effects.emit(&CHAT_SIGNAL, new_state).await?;
///
/// // Subscribing to changes
/// let mut stream = effects.subscribe(&CHAT_SIGNAL);
/// while let Ok(value) = stream.recv().await {
///     println!("Chat updated: {:?}", value);
/// }
/// ```
#[async_trait]
pub trait ReactiveEffects: Send + Sync {
    /// Read the current value of a signal.
    ///
    /// This is a point-in-time snapshot. For continuous updates, use `subscribe`.
    ///
    /// # Errors
    ///
    /// Returns `ReactiveError::SignalNotFound` if the signal doesn't exist.
    async fn read<T>(&self, signal: &Signal<T>) -> Result<T, ReactiveError>
    where
        T: Clone + Send + Sync + 'static;

    /// Emit a new value to a signal.
    ///
    /// This updates the signal's value and notifies all subscribers.
    /// Derived signals that depend on this signal are also updated.
    ///
    /// # Errors
    ///
    /// Returns `ReactiveError::SignalNotFound` if the signal doesn't exist.
    async fn emit<T>(&self, signal: &Signal<T>, value: T) -> Result<(), ReactiveError>
    where
        T: Clone + Send + Sync + 'static;

    /// Subscribe to signal changes.
    ///
    /// Returns a stream that yields values when the signal changes.
    /// The stream can be filtered to only receive specific updates.
    fn subscribe<T>(&self, signal: &Signal<T>) -> SignalStream<T>
    where
        T: Clone + Send + Sync + 'static;

    /// Register a signal with an initial value.
    ///
    /// This creates the signal in the reactive graph. Signals must be
    /// registered before they can be read, emitted to, or subscribed.
    ///
    /// # Errors
    ///
    /// Returns an error if a signal with the same ID already exists.
    async fn register<T>(&self, signal: &Signal<T>, initial: T) -> Result<(), ReactiveError>
    where
        T: Clone + Send + Sync + 'static;

    /// Check if a signal is registered.
    fn is_registered(&self, signal_id: &SignalId) -> bool;

    /// Register a signal bound to a query.
    ///
    /// This creates a signal whose value is derived from executing the query.
    /// When facts matching the query's `dependencies()` change, the query is
    /// automatically re-evaluated and the signal is updated.
    ///
    /// # Query-Signal Flow
    ///
    /// ```text
    /// Facts Change → Check dependencies() → Re-execute query → Emit to signal
    /// ```
    ///
    /// # Example
    ///
    /// ```ignore
    /// use aura_app::queries::ChannelsQuery;
    ///
    /// // Register a signal bound to a query
    /// effects.register_query(&CHANNELS_SIGNAL, ChannelsQuery::default()).await?;
    ///
    /// // Signal automatically updates when channel facts change
    /// let channels = effects.read(&CHANNELS_SIGNAL).await?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the signal already exists or query binding fails.
    async fn register_query<Q: Query>(
        &self,
        signal: &Signal<Q::Result>,
        query: Q,
    ) -> Result<(), ReactiveError>;

    /// Get the fact predicates that a signal depends on.
    ///
    /// Returns `None` if the signal is not bound to a query.
    /// Returns `Some(predicates)` if the signal was registered with `register_query`.
    fn query_dependencies(&self, signal_id: &SignalId) -> Option<Vec<FactPredicate>>;

    /// Notify that facts matching a predicate have changed.
    ///
    /// This triggers re-evaluation of all query-bound signals whose
    /// dependencies intersect with the changed predicate.
    ///
    /// Called by `JournalEffects` when facts are committed.
    async fn invalidate_queries(&self, changed: &FactPredicate);
}

/// Extension trait for derived signals.
///
/// This trait provides combinators for creating derived signals that
/// automatically update when their sources change.
#[async_trait]
pub trait ReactiveDeriveEffects: ReactiveEffects {
    /// Create a derived signal that maps values from a source signal.
    ///
    /// The derived signal automatically updates when the source changes.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let unread_count = effects.map(&CHAT_SIGNAL, |chat| {
    ///     chat.channels.iter().map(|c| c.unread_count).sum()
    /// });
    /// ```
    async fn map<A, B, F>(&self, source: &Signal<A>, f: F) -> Result<Signal<B>, ReactiveError>
    where
        A: Clone + Send + Sync + 'static,
        B: Clone + Send + Sync + 'static,
        F: Fn(A) -> B + Send + Sync + 'static;

    /// Create a derived signal that combines two source signals.
    ///
    /// The derived signal updates when either source changes.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let status = effects.combine(&CONNECTION_SIGNAL, &SYNC_SIGNAL, |conn, sync| {
    ///     format!("Connected: {}, Syncing: {}", conn.is_connected, sync.in_progress)
    /// });
    /// ```
    async fn combine<A, B, C, F>(
        &self,
        a: &Signal<A>,
        b: &Signal<B>,
        f: F,
    ) -> Result<Signal<C>, ReactiveError>
    where
        A: Clone + Send + Sync + 'static,
        B: Clone + Send + Sync + 'static,
        C: Clone + Send + Sync + 'static,
        F: Fn(A, B) -> C + Send + Sync + 'static;

    /// Create a derived signal that filters and maps values.
    ///
    /// Only emits when the filter returns Some.
    async fn filter_map<A, B, F>(
        &self,
        source: &Signal<A>,
        f: F,
    ) -> Result<Signal<Option<B>>, ReactiveError>
    where
        A: Clone + Send + Sync + 'static,
        B: Clone + Send + Sync + 'static,
        F: Fn(A) -> Option<B> + Send + Sync + 'static;
}

// ─────────────────────────────────────────────────────────────────────────────
// Blanket Implementations
// ─────────────────────────────────────────────────────────────────────────────

/// Blanket implementation for Arc<T> where T: ReactiveEffects
#[async_trait]
impl<T: ReactiveEffects + ?Sized> ReactiveEffects for Arc<T> {
    async fn read<V>(&self, signal: &Signal<V>) -> Result<V, ReactiveError>
    where
        V: Clone + Send + Sync + 'static,
    {
        (**self).read(signal).await
    }

    async fn emit<V>(&self, signal: &Signal<V>, value: V) -> Result<(), ReactiveError>
    where
        V: Clone + Send + Sync + 'static,
    {
        (**self).emit(signal, value).await
    }

    fn subscribe<V>(&self, signal: &Signal<V>) -> SignalStream<V>
    where
        V: Clone + Send + Sync + 'static,
    {
        (**self).subscribe(signal)
    }

    async fn register<V>(&self, signal: &Signal<V>, initial: V) -> Result<(), ReactiveError>
    where
        V: Clone + Send + Sync + 'static,
    {
        (**self).register(signal, initial).await
    }

    fn is_registered(&self, signal_id: &SignalId) -> bool {
        (**self).is_registered(signal_id)
    }

    async fn register_query<Q: Query>(
        &self,
        signal: &Signal<Q::Result>,
        query: Q,
    ) -> Result<(), ReactiveError> {
        (**self).register_query(signal, query).await
    }

    fn query_dependencies(&self, signal_id: &SignalId) -> Option<Vec<FactPredicate>> {
        (**self).query_dependencies(signal_id)
    }

    async fn invalidate_queries(&self, changed: &FactPredicate) {
        (**self).invalidate_queries(changed).await;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_id_creation() {
        let id1 = SignalId::new("chat");
        assert_eq!(id1.as_str(), "chat");

        let id2 = SignalId::from("recovery");
        assert_eq!(id2.as_str(), "recovery");
    }

    #[test]
    fn test_signal_id_unique() {
        let id1 = SignalId::unique("test");
        let id2 = SignalId::unique("test");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_signal_creation() {
        let signal: Signal<String> = Signal::new("chat");
        assert_eq!(signal.id().as_str(), "chat");
    }

    #[test]
    fn test_signal_clone() {
        let signal1: Signal<u32> = Signal::new("counter");
        let signal2 = signal1.clone();
        assert_eq!(signal1.id(), signal2.id());
    }

    #[test]
    fn test_signal_equality() {
        let signal1: Signal<String> = Signal::new("chat");
        let signal2: Signal<String> = Signal::new("chat");
        let signal3: Signal<String> = Signal::new("recovery");

        assert_eq!(signal1, signal2);
        assert_ne!(signal1, signal3);
    }

    #[test]
    fn test_reactive_error_display() {
        let err = ReactiveError::SignalNotFound {
            id: "chat".to_string(),
        };
        assert!(err.to_string().contains("chat"));

        let err = ReactiveError::TypeMismatch {
            id: "counter".to_string(),
            expected: "u32".to_string(),
            actual: "String".to_string(),
        };
        assert!(err.to_string().contains("counter"));
        assert!(err.to_string().contains("u32"));
    }
}
