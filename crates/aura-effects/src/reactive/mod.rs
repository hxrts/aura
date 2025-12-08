//! Reactive Effect Handlers
//!
//! This module provides the implementation of reactive effects (FRP as algebraic effects).
//!
//! # Overview
//!
//! The reactive system provides:
//! - `SignalGraph`: Manages signal storage, subscriptions, and (future) dependency tracking
//! - `ReactiveHandler`: Implements `ReactiveEffects` trait using `SignalGraph`
//!
//! # Usage
//!
//! ```ignore
//! use aura_effects::reactive::ReactiveHandler;
//! use aura_core::effects::reactive::{ReactiveEffects, Signal};
//!
//! let handler = ReactiveHandler::new();
//!
//! // Register a signal with an initial value
//! let counter: Signal<u32> = Signal::new("counter");
//! handler.register(&counter, 0).await?;
//!
//! // Read the current value
//! let value = handler.read(&counter).await?;
//!
//! // Emit a new value
//! handler.emit(&counter, value + 1).await?;
//!
//! // Subscribe to changes
//! let mut stream = handler.subscribe(&counter);
//! while let Ok(value) = stream.recv().await {
//!     println!("Counter updated: {}", value);
//! }
//! ```
//!
//! # Sharing State
//!
//! Multiple handlers can share the same signal graph:
//!
//! ```ignore
//! use std::sync::Arc;
//! use aura_effects::reactive::{ReactiveHandler, SignalGraph};
//!
//! let graph = Arc::new(SignalGraph::new());
//! let handler1 = ReactiveHandler::with_graph(graph.clone());
//! let handler2 = ReactiveHandler::with_graph(graph);
//!
//! // Both handlers see the same signals
//! ```

pub mod graph;
pub mod handler;

pub use graph::{SignalGraph, SignalGraphStats, TypedSignalReceiver};
pub use handler::ReactiveHandler;
