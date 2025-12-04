//! # Signal Utilities for Reactive TUI
//!
//! This module provides helper types, traits, and utilities for working with
//! futures-signals in the Aura TUI architecture.
//!
//! ## Overview
//!
//! We use futures-signals to provide fine-grained reactivity for view state:
//! - `Mutable<T>` for single values
//! - `MutableVec<T>` for collections
//! - Derived signals for computed state
//!
//! ## Usage Patterns
//!
//! ### Pattern 1: Simple Mutable State
//!
//! ```ignore
//! use futures_signals::signal::{Mutable, SignalExt};
//!
//! let count = Mutable::new(0);
//! count.set(5); // Automatically notifies subscribers
//! let value = count.get(); // Read current value
//! ```
//!
//! ### Pattern 2: Derived State
//!
//! ```ignore
//! use futures_signals::signal::{Mutable, SignalExt};
//!
//! let count = Mutable::new(0);
//! let doubled = count.signal().map(|n| n * 2);
//! // doubled automatically updates when count changes
//! ```
//!
//! ### Pattern 3: Collection Signals
//!
//! ```ignore
//! use futures_signals::signal_vec::{MutableVec, SignalVecExt};
//!
//! let items = MutableVec::new();
//! items.lock_mut().push_cloned("item1");
//!
//! let count = items.signal_vec_cloned()
//!     .to_signal_map(|items| items.len());
//! // count updates when items are added/removed
//! ```

use futures_signals::signal::{Mutable, Signal};
use futures_signals::signal_vec::{MutableVec, SignalVec, SignalVecExt};

// Note: Extension traits removed due to trait bound complexity.
// Use direct methods on Mutable<T> and MutableVec<T> instead,
// or use the helper types ReactiveState<T> and ReactiveVec<T> below.

/// Wrapper type for view state that provides both direct access and signal exposure.
///
/// This bridges the gap between our delta-based updates (which need direct mutation)
/// and signal-based rendering (which needs reactive subscriptions).
#[derive(Clone)]
pub struct ReactiveState<T: Clone> {
    /// The mutable state backing this reactive value
    state: Mutable<T>,
}

impl<T: Clone> ReactiveState<T> {
    /// Create new reactive state with an initial value
    pub fn new(initial: T) -> Self {
        Self {
            state: Mutable::new(initial),
        }
    }

    /// Get a clone of the current value
    pub fn get(&self) -> T {
        self.state.get_cloned()
    }

    /// Set a new value (notifies subscribers automatically)
    pub fn set(&self, value: T) {
        self.state.set(value);
    }

    /// Update the value using a closure
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        let mut lock = self.state.lock_mut();
        f(&mut *lock);
    }

    /// Get a signal that tracks this state
    pub fn signal(&self) -> impl Signal<Item = T> + Send + Sync + 'static
    where
        T: Send + Sync + 'static,
    {
        self.state.signal_cloned()
    }

    /// Get direct access to the underlying Mutable for advanced patterns
    pub fn as_mutable(&self) -> &Mutable<T> {
        &self.state
    }
}

/// Wrapper for reactive collections that provides both mutation and signal exposure
#[derive(Clone)]
pub struct ReactiveVec<T: Clone> {
    /// The mutable vec backing this reactive collection
    items: MutableVec<T>,
}

impl<T: Clone> ReactiveVec<T> {
    /// Create new reactive vec with initial items
    pub fn new() -> Self {
        Self {
            items: MutableVec::new(),
        }
    }

    /// Create from an existing vec
    pub fn from_vec(vec: Vec<T>) -> Self {
        Self {
            items: MutableVec::new_with_values(vec),
        }
    }

    /// Get a clone of all current items
    pub fn get_cloned(&self) -> Vec<T> {
        self.items.lock_ref().to_vec()
    }

    /// Push a new item (notifies subscribers automatically)
    pub fn push(&self, item: T) {
        self.items.lock_mut().push_cloned(item);
    }

    /// Clear all items
    pub fn clear(&self) {
        self.items.lock_mut().clear();
    }

    /// Replace all items with a new vec
    pub fn replace(&self, new_items: Vec<T>) {
        let mut lock = self.items.lock_mut();
        lock.clear();
        for item in new_items {
            lock.push_cloned(item);
        }
    }

    /// Update an item at a specific index
    pub fn update_at<F>(&self, index: usize, f: F)
    where
        F: FnOnce(&mut T),
    {
        let mut lock = self.items.lock_mut();
        // Get a copy of all items, update the one at index, then replace
        let mut items: Vec<T> = lock.to_vec();
        if let Some(item) = items.get_mut(index) {
            f(item);
            lock.replace_cloned(items);
        }
    }

    /// Remove item at index
    pub fn remove(&self, index: usize) {
        self.items.lock_mut().remove(index);
    }

    /// Get the current length
    pub fn len(&self) -> usize {
        self.items.lock_ref().len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.items.lock_ref().is_empty()
    }

    /// Get a signal vec that tracks this collection
    pub fn signal_vec(&self) -> impl SignalVec<Item = T> + Send + Sync + 'static
    where
        T: Send + Sync + 'static,
    {
        self.items.signal_vec_cloned()
    }

    /// Get a signal that emits the current count
    pub fn count_signal(&self) -> impl Signal<Item = usize> + Send + Sync + 'static
    where
        T: Send + Sync + 'static,
    {
        self.signal_vec().to_signal_map(|items| items.len())
    }

    /// Get direct access to the underlying MutableVec for advanced patterns
    pub fn as_mutable_vec(&self) -> &MutableVec<T> {
        &self.items
    }
}

impl<T: Clone> Default for ReactiveVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

// Note: combine_signals removed - futures-signals doesn't have map_ref! macro.
// For combining signals, manually sample them or use separate subscriptions.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reactive_state_basic() {
        let state = ReactiveState::new(42);
        assert_eq!(state.get(), 42);

        state.set(100);
        assert_eq!(state.get(), 100);
    }

    #[test]
    fn test_reactive_state_update() {
        let state = ReactiveState::new(0);
        state.update(|v| *v += 5);
        assert_eq!(state.get(), 5);
    }

    #[test]
    fn test_reactive_vec_basic() {
        let vec = ReactiveVec::new();
        assert_eq!(vec.len(), 0);
        assert!(vec.is_empty());

        vec.push(1);
        vec.push(2);
        assert_eq!(vec.len(), 2);
        assert!(!vec.is_empty());

        let items = vec.get_cloned();
        assert_eq!(items, vec![1, 2]);
    }

    #[test]
    fn test_reactive_vec_replace() {
        let vec = ReactiveVec::new();
        vec.push(1);
        vec.push(2);

        vec.replace(vec![10, 20, 30]);
        assert_eq!(vec.len(), 3);
        assert_eq!(vec.get_cloned(), vec![10, 20, 30]);
    }

    #[test]
    fn test_reactive_vec_update_at() {
        let vec = ReactiveVec::new();
        vec.push(1);
        vec.push(2);
        vec.push(3);

        vec.update_at(1, |v| *v *= 10);
        assert_eq!(vec.get_cloned(), vec![1, 20, 3]);
    }

    #[test]
    fn test_reactive_vec_remove() {
        let vec = ReactiveVec::new();
        vec.push(1);
        vec.push(2);
        vec.push(3);

        vec.remove(1);
        assert_eq!(vec.len(), 2);
        assert_eq!(vec.get_cloned(), vec![1, 3]);
    }
}
