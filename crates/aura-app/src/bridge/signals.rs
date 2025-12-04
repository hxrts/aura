//! # Signal-based Reactive Bridge
//!
//! This module provides a signal-based API for native Rust consumers
//! and web apps using dominator.
//!
//! ## Usage (Native Rust)
//!
//! ```rust,ignore
//! use futures_signals::signal::SignalExt;
//!
//! let app = AppCore::new(config)?;
//!
//! // Subscribe to chat state changes
//! let chat_signal = app.chat_signal();
//!
//! // Use with dominator
//! html!("div", {
//!     .text_signal(chat_signal.map(|s| format!("{} messages", s.messages.len())))
//! })
//! ```

use futures_signals::signal::{Mutable, Signal, SignalExt};
use futures_signals::signal_vec::{MutableVec, SignalVec, SignalVecExt};

/// A signal broadcaster that can have multiple subscribers.
///
/// Unlike `Mutable`, this is designed for one-to-many broadcasting
/// where the source pushes updates.
pub struct SignalBroadcaster<T: Clone + Send + Sync + 'static> {
    state: Mutable<T>,
}

impl<T: Clone + Send + Sync + 'static> SignalBroadcaster<T> {
    /// Create a new broadcaster with initial value
    pub fn new(initial: T) -> Self {
        Self {
            state: Mutable::new(initial),
        }
    }

    /// Get the current value
    pub fn get(&self) -> T {
        self.state.get_cloned()
    }

    /// Set a new value (broadcasts to all subscribers)
    pub fn set(&self, value: T) {
        self.state.set(value);
    }

    /// Get a signal that tracks this broadcaster
    pub fn signal(&self) -> impl Signal<Item = T> {
        self.state.signal_cloned()
    }

    /// Map the signal to a different type
    pub fn map_signal<U, F>(&self, f: F) -> impl Signal<Item = U>
    where
        U: Send + Sync + 'static,
        F: FnMut(T) -> U + Send + Sync + 'static,
    {
        self.state.signal_cloned().map(f)
    }
}

/// A signal vec broadcaster for collections
pub struct SignalVecBroadcaster<T: Clone + Send + Sync + 'static> {
    items: MutableVec<T>,
}

impl<T: Clone + Send + Sync + 'static> SignalVecBroadcaster<T> {
    /// Create a new broadcaster
    pub fn new() -> Self {
        Self {
            items: MutableVec::new(),
        }
    }

    /// Create from existing items
    pub fn from_vec(items: Vec<T>) -> Self {
        Self {
            items: MutableVec::new_with_values(items),
        }
    }

    /// Get all items as a vec
    pub fn get_cloned(&self) -> Vec<T> {
        self.items.lock_ref().to_vec()
    }

    /// Replace all items
    pub fn replace(&self, items: Vec<T>) {
        let mut lock = self.items.lock_mut();
        lock.replace_cloned(items);
    }

    /// Push an item
    pub fn push(&self, item: T) {
        self.items.lock_mut().push_cloned(item);
    }

    /// Clear all items
    pub fn clear(&self) {
        self.items.lock_mut().clear();
    }

    /// Get a signal vec that tracks this broadcaster
    pub fn signal_vec(&self) -> impl SignalVec<Item = T> {
        self.items.signal_vec_cloned()
    }

    /// Get a signal for the count
    pub fn count_signal(&self) -> impl Signal<Item = usize> {
        self.signal_vec().to_signal_map(|items| items.len())
    }
}

impl<T: Clone + Send + Sync + 'static> Default for SignalVecBroadcaster<T> {
    fn default() -> Self {
        Self::new()
    }
}
