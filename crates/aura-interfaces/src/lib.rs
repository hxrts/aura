//! Core trait abstractions for Aura services
//!
//! This crate provides trait abstractions that enable dependency injection,
//! testing, and loose coupling between service layers.

pub mod effects;
pub mod storage;
pub mod transport;

pub use effects::EffectsProvider;
pub use storage::{AccessController, StorageBackend};
pub use transport::ProtocolTransport;
