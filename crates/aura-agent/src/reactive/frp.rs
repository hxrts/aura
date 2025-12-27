//! # FRP Infrastructure
//!
//! Functional Reactive Programming primitives for Aura UI layers.
//! Implements Dynamic<T> with compositional combinators.
//!
//! This module provides UI-agnostic reactive programming infrastructure
//! that can be used by CLI, webapp, or other UI layers.

use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, RwLock};

/// FRP primitive for reactive values
///
/// Dynamic<T> represents a time-varying value that can be observed
/// and transformed using FRP combinators (map, combine, filter, fold).
///
/// ## Example
/// ```ignore
/// let count = Dynamic::new(0);
    /// let doubled = count.map(|x| x * 2).await;
    /// let is_positive = count.map(|x| x > 0).await;
/// ```
pub struct Dynamic<T: Clone + Send + Sync + 'static> {
    state: Arc<RwLock<T>>,
    updates: broadcast::Sender<T>,
}

impl<T: Clone + Send + Sync + 'static> Dynamic<T> {
    /// Create a new Dynamic with initial value
    pub fn new(initial: T) -> Self {
        let (tx, _) = broadcast::channel(64);
        Self {
            state: Arc::new(RwLock::new(initial)),
            updates: tx,
        }
    }

    /// Get current value (async read lock)
    pub async fn get(&self) -> T {
        self.state.read().await.clone()
    }

    /// Set value and notify subscribers
    pub async fn set(&self, value: T) {
        *self.state.write().await = value.clone();
        let _ = self.updates.send(value);
    }

    /// Update value using a function and notify subscribers
    pub async fn update<F>(&self, f: F)
    where
        F: FnOnce(&T) -> T,
    {
        let new_value = {
            let current = self.state.read().await;
            f(&current)
        };
        self.set(new_value).await;
    }

    /// Subscribe to value changes
    pub fn subscribe(&self) -> broadcast::Receiver<T> {
        self.updates.subscribe()
    }

    /// Try to receive the latest update without blocking
    /// Returns None if no update is available
    pub fn try_recv(&self, rx: &mut broadcast::Receiver<T>) -> Option<T> {
        rx.try_recv().ok()
    }

    /// Map combinator: transform values
    ///
    /// Creates a new Dynamic that applies function `f` to each value.
    /// Subscribers to the mapped Dynamic will receive transformed values.
    ///
    /// ## Example
    /// ```ignore
    /// let count = Dynamic::new(5);
    /// let doubled = count.map(|x| x * 2).await;  // Always contains count * 2
    /// ```
    pub async fn map<U, F>(&self, f: F) -> Dynamic<U>
    where
        F: Fn(&T) -> U + Send + Sync + 'static,
        U: Clone + Send + Sync + 'static,
    {
        let initial = self.get().await;
        let mapped = Dynamic::new(f(&initial));

        // Subscribe BEFORE spawning to ensure we don't miss any updates
        let mut rx = self.subscribe();
        let mapped_clone = mapped.clone();
        let state = self.state.clone();
        let f = Arc::new(f);

        // Use a oneshot channel to signal when the task is ready
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            // Signal that we're ready to receive
            let _ = ready_tx.send(());

            loop {
                match rx.recv().await {
                    Ok(value) => {
                        let transformed = f(&value);
                        mapped_clone.set(transformed).await;
                    }
                    Err(RecvError::Lagged(_)) => {
                        // Re-sync from the latest state on lag to avoid stale values.
                        let latest = state.read().await.clone();
                        let transformed = f(&latest);
                        mapped_clone.set(transformed).await;
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });

        // Wait for the spawned task to signal ready - ensures deterministic behavior
        let _ = ready_rx.await;

        mapped
    }

    /// Combine combinator: merge two dynamics
    ///
    /// Creates a new Dynamic that combines values from two source Dynamics.
    /// Updates when either source changes.
    ///
    /// ## Example
    /// ```ignore
    /// let first = Dynamic::new("Hello");
    /// let second = Dynamic::new("World");
    /// let combined = first.combine(&second, |a, b| format!("{} {}", a, b)).await;
    /// ```
    pub async fn combine<U, V, F>(&self, other: &Dynamic<U>, f: F) -> Dynamic<V>
    where
        U: Clone + Send + Sync + 'static,
        V: Clone + Send + Sync + 'static,
        F: Fn(&T, &U) -> V + Send + Sync + 'static,
    {
        let a = self.get().await;
        let b = other.get().await;
        let combined = Dynamic::new(f(&a, &b));

        let mut rx_self = self.subscribe();
        let mut rx_other = other.subscribe();
        let combined_clone = combined.clone();
        let self_state = self.state.clone();
        let other_state = other.state.clone();
        let f = Arc::new(f);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    res = rx_self.recv() => {
                        match res {
                            Ok(_) | Err(RecvError::Lagged(_)) => {
                                let a = self_state.read().await.clone();
                                let b = other_state.read().await.clone();
                                let result = f(&a, &b);
                                combined_clone.set(result).await;
                            }
                            Err(RecvError::Closed) => break,
                        }
                    }
                    res = rx_other.recv() => {
                        match res {
                            Ok(_) | Err(RecvError::Lagged(_)) => {
                                let a = self_state.read().await.clone();
                                let b = other_state.read().await.clone();
                                let result = f(&a, &b);
                                combined_clone.set(result).await;
                            }
                            Err(RecvError::Closed) => break,
                        }
                    }
                }
            }
        });

        combined
    }

    /// Filter combinator: conditional propagation
    ///
    /// Creates a new Dynamic that only propagates values matching the predicate.
    /// When a value fails the predicate, the Dynamic retains its previous value.
    ///
    /// ## Example
    /// ```ignore
    /// let numbers = Dynamic::new(5);
    /// let positive_only = numbers.filter(|x| x > 0).await;
    /// ```
    pub async fn filter<F>(&self, predicate: F) -> Dynamic<T>
    where
        F: Fn(&T) -> bool + Send + Sync + 'static,
    {
        let initial = self.get().await;
        let filtered = Dynamic::new(initial);

        let mut rx = self.subscribe();
        let filtered_clone = filtered.clone();
        let state = self.state.clone();
        let predicate = Arc::new(predicate);

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(value) => {
                        if predicate(&value) {
                            filtered_clone.set(value).await;
                        }
                    }
                    Err(RecvError::Lagged(_)) => {
                        let latest = state.read().await.clone();
                        if predicate(&latest) {
                            filtered_clone.set(latest).await;
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });

        filtered
    }

    /// Fold combinator: accumulate over time
    ///
    /// Creates a new Dynamic that maintains an accumulated state.
    /// Each update applies the fold function to the accumulator and new value.
    ///
    /// ## Example
    /// ```ignore
    /// let events = Dynamic::new(1);
    /// let sum = events.fold(0, |acc, x| acc + x);  // Running sum
    /// ```
    pub fn fold<Acc, F>(&self, init: Acc, f: F) -> Dynamic<Acc>
    where
        Acc: Clone + Send + Sync + 'static,
        F: Fn(Acc, &T) -> Acc + Send + Sync + 'static,
    {
        let folded = Dynamic::new(init);
        let mut rx = self.subscribe();
        let folded_clone = folded.clone();
        let state = self.state.clone();
        let f = Arc::new(f);

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(value) => {
                        let acc = folded_clone.get().await;
                        let new_acc = f(acc, &value);
                        folded_clone.set(new_acc).await;
                    }
                    Err(RecvError::Lagged(_)) => {
                        let latest = state.read().await.clone();
                        let acc = folded_clone.get().await;
                        let new_acc = f(acc, &latest);
                        folded_clone.set(new_acc).await;
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });

        folded
    }
}

impl<T: Clone + Send + Sync + 'static> Clone for Dynamic<T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            updates: self.updates.clone(),
        }
    }
}

// ReactiveScheduler is implemented in the `scheduler` module.
// See scheduler.rs for the full implementation including:
// - Fact ingestion from journal via FactSource enum
// - Topological view sorting (for glitch-freedom)
// - Batching with configurable window (5ms default)
// - ViewReduction trait for transforming facts to deltas

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_dynamic_new_and_get() {
        let dynamic = Dynamic::new(42);
        assert_eq!(dynamic.get().await, 42);
    }

    #[tokio::test]
    async fn test_dynamic_set() {
        let dynamic = Dynamic::new(0);
        dynamic.set(100).await;
        assert_eq!(dynamic.get().await, 100);
    }

    #[tokio::test]
    async fn test_dynamic_update() {
        let dynamic = Dynamic::new(10);
        dynamic.update(|x| x * 2).await;
        assert_eq!(dynamic.get().await, 20);
    }

    #[tokio::test]
    async fn test_dynamic_subscribe() {
        let dynamic = Dynamic::new(0);
        let mut rx = dynamic.subscribe();

        dynamic.set(5).await;
        let value = rx.recv().await.unwrap();
        assert_eq!(value, 5);

        dynamic.set(10).await;
        let value = rx.recv().await.unwrap();
        assert_eq!(value, 10);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_map_combinator() {
        let source = Dynamic::new(5);
        let doubled = source.map(|x| x * 2).await;

        // Wait for initial value to propagate
        sleep(Duration::from_millis(10)).await;
        assert_eq!(doubled.get().await, 10);

        source.set(10).await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(doubled.get().await, 20);

        source.set(15).await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(doubled.get().await, 30);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_map_string_transformation() {
        let source = Dynamic::new("hello");
        let upper = source.map(|s| s.to_uppercase()).await;

        sleep(Duration::from_millis(10)).await;
        assert_eq!(upper.get().await, "HELLO");

        source.set("world").await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(upper.get().await, "WORLD");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_combine_combinator() {
        let first = Dynamic::new(3);
        let second = Dynamic::new(4);
        let sum = first.combine(&second, |a, b| a + b).await;

        sleep(Duration::from_millis(10)).await;
        assert_eq!(sum.get().await, 7);

        first.set(10).await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(sum.get().await, 14);

        second.set(5).await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(sum.get().await, 15);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_combine_strings() {
        let first = Dynamic::new("Hello");
        let second = Dynamic::new("World");
        let combined = first
            .combine(&second, |a, b| format!("{} {}", a, b))
            .await;

        sleep(Duration::from_millis(10)).await;
        assert_eq!(combined.get().await, "Hello World");

        first.set("Hi").await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(combined.get().await, "Hi World");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_filter_combinator() {
        let source = Dynamic::new(5);
        let positive_only = source.filter(|x| *x > 0).await;

        sleep(Duration::from_millis(10)).await;
        assert_eq!(positive_only.get().await, 5);

        source.set(10).await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(positive_only.get().await, 10);

        // Negative value should not propagate
        source.set(-5).await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(positive_only.get().await, 10); // Unchanged

        source.set(20).await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(positive_only.get().await, 20);
    }

    #[tokio::test]
    async fn test_fold_combinator() {
        let events = Dynamic::new(1);
        let sum = events.fold(0, |acc, x| acc + x);

        sleep(Duration::from_millis(10)).await;
        assert_eq!(sum.get().await, 0); // Initial accumulator

        events.set(5).await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(sum.get().await, 5);

        events.set(3).await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(sum.get().await, 8);

        events.set(2).await;
        sleep(Duration::from_millis(10)).await;
        assert_eq!(sum.get().await, 10);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_composition_chain() {
        // Compose multiple combinators
        let source = Dynamic::new(2);
        let doubled = source.map(|x| x * 2).await;
        let squared = doubled.map(|x| x * x).await;

        sleep(Duration::from_millis(20)).await;
        assert_eq!(squared.get().await, 16); // (2 * 2)^2 = 16

        source.set(3).await;
        sleep(Duration::from_millis(20)).await;
        assert_eq!(squared.get().await, 36); // (3 * 2)^2 = 36
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let source = Dynamic::new(0);
        let mut rx1 = source.subscribe();
        let mut rx2 = source.subscribe();

        source.set(42).await;

        let val1 = rx1.recv().await.unwrap();
        let val2 = rx2.recv().await.unwrap();

        assert_eq!(val1, 42);
        assert_eq!(val2, 42);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_map_and_filter_composition() {
        let source = Dynamic::new(1);
        let doubled = source.map(|x| x * 2).await;
        let even_only = doubled.filter(|x| x % 2 == 0).await;

        sleep(Duration::from_millis(20)).await;
        assert_eq!(even_only.get().await, 2);

        source.set(5).await;
        sleep(Duration::from_millis(20)).await;
        assert_eq!(even_only.get().await, 10);

        // This should be filtered out (but won't reach filter since map always produces even)
        source.set(7).await;
        sleep(Duration::from_millis(20)).await;
        assert_eq!(even_only.get().await, 14);
    }

    #[tokio::test]
    async fn test_clone_dynamic() {
        let d1 = Dynamic::new(42);
        let d2 = d1.clone();

        assert_eq!(d1.get().await, 42);
        assert_eq!(d2.get().await, 42);

        d1.set(100).await;
        sleep(Duration::from_millis(10)).await;

        // Both should see the update (same underlying state)
        assert_eq!(d1.get().await, 100);
        assert_eq!(d2.get().await, 100);
    }

    #[tokio::test]
    async fn test_complex_fold_example() {
        // Running average
        let numbers = Dynamic::new(10);
        let (count, sum) = (Dynamic::new(0), Dynamic::new(0));

        // Update count and sum on each new number
        let mut rx = numbers.subscribe();
        let count_clone = count.clone();
        let sum_clone = sum.clone();

        tokio::spawn(async move {
            while let Ok(n) = rx.recv().await {
                count_clone.update(|c| c + 1).await;
                sum_clone.update(|s| s + n).await;
            }
        });

        numbers.set(5).await;
        sleep(Duration::from_millis(10)).await;
        numbers.set(15).await;
        sleep(Duration::from_millis(10)).await;

        assert_eq!(count.get().await, 2);
        assert_eq!(sum.get().await, 20); // 5 + 15
    }
}
