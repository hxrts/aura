//! # Reactive Primitives for TUI and Database Subscriptions
//!
//! This module provides core reactive primitives that enable reactive UI updates
//! and database subscription patterns throughout Aura.
//!
//! ## Core Types
//!
//! - [`Dynamic<T>`]: A reactive value that can be observed for changes.
//!   Wraps a value and provides subscription-based change notification.
//!
//! - [`Subscription<T>`]: A polling-based subscription to a `Dynamic<T>`.
//!   Tracks version changes for efficient change detection.
//!
//! - [`Delta<T>`]: Represents incremental changes to a collection.
//!   Used for efficient list updates without full re-renders.
//!
//! ## Design Principles
//!
//! 1. **Runtime-agnostic**: All primitives use only std types (RwLock, AtomicU64).
//!    Works with any async runtime or in sync-only code.
//!
//! 2. **Poll-based subscriptions**: Subscriptions track versions and poll for
//!    changes rather than using push-based channels.
//!
//! 3. **Composable**: `Dynamic<T>` supports `map()` for derived values with
//!    sync propagation via `DynamicLink`.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_core::reactive::{Dynamic, Subscription};
//!
//! // Create a reactive value
//! let counter = Dynamic::new(0);
//!
//! // Get current value
//! assert_eq!(counter.get(), 0);
//!
//! // Subscribe to changes
//! let mut sub = counter.subscribe();
//!
//! // Update value
//! counter.set(1);
//!
//! // Poll for changes
//! if let Some(value) = sub.poll() {
//!     assert_eq!(value, 1);
//! }
//! ```

mod delta;
mod dynamic;

pub use delta::{apply_delta, try_apply_delta, Delta, DeltaError};
pub use dynamic::{Dynamic, DynamicLink, Subscription};
