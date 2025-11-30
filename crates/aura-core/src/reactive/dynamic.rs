//! Dynamic<T> - A reactive value with change notifications
//!
//! `Dynamic<T>` wraps a value and provides subscription-based change notification.
//! It is the core primitive for reactive UI updates and database subscriptions.
//!
//! # Runtime Agnostic Design
//!
//! This module uses only std primitives (RwLock, AtomicU64) to remain runtime-agnostic.
//! Higher layers (aura-cli, aura-effects) can wrap subscriptions in async adapters if needed.

// Allow expect on RwLock::read/write - lock poisoning from panics
// is unrecoverable, so expect() is the appropriate handling pattern.
#![allow(clippy::expect_used)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Inner state of a Dynamic value.
struct DynamicInner<T> {
    /// The current value, protected by RwLock for sync access.
    value: RwLock<T>,
    /// Version counter incremented on each update.
    version: AtomicU64,
}

/// A reactive value that can be observed for changes.
///
/// `Dynamic<T>` provides:
/// - `get()`: Synchronously read the current value
/// - `set()`: Update the value and increment version
/// - `subscribe()`: Get a `Subscription` for polling changes
/// - `map()`: Create a derived `Dynamic<U>` with a sync link for propagation
///
/// # Thread Safety
///
/// `Dynamic<T>` is `Send + Sync` and can be safely shared across threads.
/// The inner value is protected by `RwLock` for sync access.
///
/// # Runtime Agnostic
///
/// This type uses only std primitives. Subscriptions are poll-based rather than
/// push-based, making it compatible with any async runtime or sync-only code.
/// Higher layers can wrap `Subscription` in async adapters if needed.
///
/// # Example
///
/// ```rust,ignore
/// use aura_core::reactive::Dynamic;
///
/// let counter = Dynamic::new(0);
/// let mut sub = counter.subscribe();
///
/// counter.set(1);
/// assert_eq!(counter.get(), 1);
///
/// // Poll for changes:
/// if let Some(value) = sub.poll() {
///     assert_eq!(value, 1);
/// }
/// ```
#[derive(Clone)]
pub struct Dynamic<T> {
    inner: Arc<DynamicInner<T>>,
}

impl<T: Clone + Send + Sync + 'static> Dynamic<T> {
    /// Create a new Dynamic with the given initial value.
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new(DynamicInner {
                value: RwLock::new(value),
                version: AtomicU64::new(0),
            }),
        }
    }

    /// Get the current value.
    ///
    /// This is a synchronous operation that clones the value.
    pub fn get(&self) -> T {
        self.inner
            .value
            .read()
            .expect("Dynamic lock poisoned")
            .clone()
    }

    /// Get the current version number.
    ///
    /// The version is incremented each time `set()` is called.
    pub fn version(&self) -> u64 {
        self.inner.version.load(Ordering::Acquire)
    }

    /// Set a new value and increment the version.
    ///
    /// This is a synchronous operation. Subscriptions will see the
    /// new value on their next `poll()` call.
    pub fn set(&self, value: T) {
        // Update the stored value
        {
            let mut guard = self.inner.value.write().expect("Dynamic lock poisoned");
            *guard = value;
        }

        // Increment version to signal change
        self.inner.version.fetch_add(1, Ordering::Release);
    }

    /// Subscribe to value changes.
    ///
    /// Returns a `Subscription` that can poll for changes.
    /// The subscription tracks the version it last saw and returns
    /// new values when the Dynamic has been updated.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let d = Dynamic::new(0);
    /// let mut sub = d.subscribe();
    ///
    /// d.set(42);
    /// assert_eq!(sub.poll(), Some(42));
    /// assert_eq!(sub.poll(), None); // No new changes
    /// ```
    pub fn subscribe(&self) -> Subscription<T> {
        Subscription {
            source: self.inner.clone(),
            last_version: self.inner.version.load(Ordering::Acquire),
        }
    }

    /// Create a derived Dynamic that transforms values using the given function.
    ///
    /// Returns a tuple of:
    /// - The derived `Dynamic<U>` initialized with the mapped current value
    /// - A `DynamicLink` that can be used to propagate updates
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let counter = Dynamic::new(5);
    /// let (doubled, mut link) = counter.map(|x| x * 2);
    ///
    /// assert_eq!(doubled.get(), 10);
    ///
    /// // Manual propagation (sync)
    /// counter.set(10);
    /// link.propagate();
    /// assert_eq!(doubled.get(), 20);
    /// ```
    ///
    /// # Architectural Note
    ///
    /// This method avoids spawning async tasks in Layer 1 (aura-core).
    /// The returned `DynamicLink` provides sync propagation methods.
    /// Higher layers can wrap the link in async polling loops if needed.
    pub fn map<U, F>(&self, f: F) -> (Dynamic<U>, DynamicLink<T, U, F>)
    where
        U: Clone + Send + Sync + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        // Compute initial value
        let initial = f(self.get());
        let derived = Dynamic::new(initial);

        let link = DynamicLink {
            source_sub: self.subscribe(),
            target: derived.clone(),
            transform: f,
        };

        (derived, link)
    }

    /// Update the value using a function.
    ///
    /// This is a convenience method that reads the current value,
    /// applies the function, and sets the result.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let counter = Dynamic::new(0);
    /// counter.update(|x| x + 1);
    /// assert_eq!(counter.get(), 1);
    /// ```
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(T) -> T,
    {
        let new_value = f(self.get());
        self.set(new_value);
    }
}

impl<T: Clone + Send + Sync + Default + 'static> Default for Dynamic<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Clone + Send + Sync + std::fmt::Debug + 'static> std::fmt::Debug for Dynamic<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dynamic")
            .field("value", &self.get())
            .field("version", &self.version())
            .finish()
    }
}

/// A subscription to a Dynamic value for polling changes.
///
/// `Subscription` tracks the version it last observed and provides
/// polling-based change detection. This is runtime-agnostic and works
/// with any async runtime or in sync-only code.
///
/// # Example
///
/// ```rust,ignore
/// let d = Dynamic::new(0);
/// let mut sub = d.subscribe();
///
/// d.set(42);
///
/// // Poll returns Some if value changed
/// assert_eq!(sub.poll(), Some(42));
/// // Poll returns None if no change
/// assert_eq!(sub.poll(), None);
///
/// // Get current value regardless of change
/// assert_eq!(sub.get(), 42);
/// ```
pub struct Subscription<T> {
    source: Arc<DynamicInner<T>>,
    last_version: u64,
}

impl<T: Clone + Send + Sync + 'static> Subscription<T> {
    /// Check if the source has changed since the last poll.
    pub fn has_changed(&self) -> bool {
        self.source.version.load(Ordering::Acquire) > self.last_version
    }

    /// Poll for a new value.
    ///
    /// Returns `Some(value)` if the source has been updated since the last poll,
    /// updating the subscription's tracked version. Returns `None` if no change.
    pub fn poll(&mut self) -> Option<T> {
        let current_version = self.source.version.load(Ordering::Acquire);
        if current_version > self.last_version {
            self.last_version = current_version;
            Some(
                self.source
                    .value
                    .read()
                    .expect("Dynamic lock poisoned")
                    .clone(),
            )
        } else {
            None
        }
    }

    /// Get the current value regardless of whether it changed.
    pub fn get(&self) -> T {
        self.source
            .value
            .read()
            .expect("Dynamic lock poisoned")
            .clone()
    }

    /// Get the current version of the source.
    pub fn source_version(&self) -> u64 {
        self.source.version.load(Ordering::Acquire)
    }

    /// Get the last version this subscription observed.
    pub fn last_observed_version(&self) -> u64 {
        self.last_version
    }
}

/// A link between a source Dynamic and a derived Dynamic.
///
/// `DynamicLink` enables controlled propagation of updates from a source
/// to a derived Dynamic. This design separates the data structure (Layer 1)
/// from runtime behavior (higher layers).
///
/// # Usage Patterns
///
/// 1. **Sync propagation**: Use `propagate()` in a sync context to
///    update the derived value if the source changed.
///
/// 2. **Custom async propagation**: Higher layers can call `propagate()`
///    in an async loop with their runtime's sleep/interval mechanisms.
pub struct DynamicLink<T, U, F>
where
    T: Clone + Send + Sync + 'static,
    U: Clone + Send + Sync + 'static,
    F: Fn(T) -> U + Send + Sync + 'static,
{
    source_sub: Subscription<T>,
    target: Dynamic<U>,
    transform: F,
}

impl<T, U, F> DynamicLink<T, U, F>
where
    T: Clone + Send + Sync + 'static,
    U: Clone + Send + Sync + 'static,
    F: Fn(T) -> U + Send + Sync + 'static,
{
    /// Propagate any updates from source to target.
    ///
    /// Returns `true` if an update was propagated, `false` otherwise.
    pub fn propagate(&mut self) -> bool {
        if let Some(value) = self.source_sub.poll() {
            let mapped = (self.transform)(value);
            self.target.set(mapped);
            true
        } else {
            false
        }
    }

    /// Check if the source has changed since the last propagation.
    pub fn has_pending_update(&self) -> bool {
        self.source_sub.has_changed()
    }

    /// Get a reference to the target Dynamic.
    pub fn target(&self) -> &Dynamic<U> {
        &self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_new_and_get() {
        let d = Dynamic::new(42);
        assert_eq!(d.get(), 42);
    }

    #[test]
    fn test_dynamic_set() {
        let d = Dynamic::new(0);
        d.set(100);
        assert_eq!(d.get(), 100);
    }

    #[test]
    fn test_dynamic_update() {
        let d = Dynamic::new(10);
        d.update(|x| x * 2);
        assert_eq!(d.get(), 20);
    }

    #[test]
    fn test_dynamic_clone_shares_state() {
        let d1 = Dynamic::new(0);
        let d2 = d1.clone();

        d1.set(42);
        assert_eq!(d2.get(), 42);
    }

    #[test]
    fn test_dynamic_version() {
        let d = Dynamic::new(0);
        assert_eq!(d.version(), 0);

        d.set(1);
        assert_eq!(d.version(), 1);

        d.set(2);
        assert_eq!(d.version(), 2);
    }

    #[test]
    fn test_dynamic_default() {
        let d: Dynamic<i32> = Dynamic::default();
        assert_eq!(d.get(), 0);
    }

    #[test]
    fn test_dynamic_debug() {
        let d = Dynamic::new(42);
        let debug_str = format!("{:?}", d);
        assert!(debug_str.contains("Dynamic"));
        assert!(debug_str.contains("42"));
    }

    #[test]
    fn test_subscription_poll() {
        let d = Dynamic::new(0);
        let mut sub = d.subscribe();

        // Initially no changes (subscription starts at current version)
        assert_eq!(sub.poll(), None);

        // After set, poll returns the new value
        d.set(1);
        assert_eq!(sub.poll(), Some(1));

        // Second poll returns None (no new changes)
        assert_eq!(sub.poll(), None);

        // Another update
        d.set(2);
        assert_eq!(sub.poll(), Some(2));
    }

    #[test]
    fn test_subscription_has_changed() {
        let d = Dynamic::new(0);
        let mut sub = d.subscribe();

        assert!(!sub.has_changed());

        d.set(1);
        assert!(sub.has_changed());

        // Polling consumes the change
        let _ = sub.poll();
        assert!(!sub.has_changed());
    }

    #[test]
    fn test_subscription_get() {
        let d = Dynamic::new(42);
        let sub = d.subscribe();

        assert_eq!(sub.get(), 42);

        d.set(100);
        assert_eq!(sub.get(), 100);
    }

    #[test]
    fn test_dynamic_map_initial_value() {
        let source = Dynamic::new(5);
        let (doubled, _link) = source.map(|x| x * 2);

        // Initial value should be mapped
        assert_eq!(doubled.get(), 10);
    }

    #[test]
    fn test_dynamic_map_sync_propagation() {
        let source = Dynamic::new(5);
        let (doubled, mut link) = source.map(|x| x * 2);

        assert_eq!(doubled.get(), 10);

        // Update source
        source.set(10);

        // Manual propagation
        assert!(link.propagate());
        assert_eq!(doubled.get(), 20);

        // No more updates available
        assert!(!link.propagate());
    }

    #[test]
    fn test_dynamic_map_has_pending_update() {
        let source = Dynamic::new(5);
        let (_, link) = source.map(|x| x * 2);

        assert!(!link.has_pending_update());

        source.set(10);
        assert!(link.has_pending_update());
    }

    #[test]
    fn test_dynamic_multiple_subscribers() {
        let d = Dynamic::new(0);
        let mut sub1 = d.subscribe();
        let mut sub2 = d.subscribe();

        d.set(42);

        assert_eq!(sub1.poll(), Some(42));
        assert_eq!(sub2.poll(), Some(42));
    }

    #[test]
    fn test_subscription_coalesces_updates() {
        let d = Dynamic::new(0);
        let mut sub = d.subscribe();

        // Multiple rapid updates
        d.set(1);
        d.set(2);
        d.set(3);

        // Poll gets the latest value (version-based, not queue-based)
        assert_eq!(sub.poll(), Some(3));
        assert_eq!(sub.poll(), None);
    }
}
