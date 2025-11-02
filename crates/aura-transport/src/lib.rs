//! Unified transport layer for Aura
//!
//! Provides a clean, layered architecture:
//! - Core Transport Layer: Transport trait and implementations
//! - Adapter Layer: Protocol-specific adapters (AuraProtocolHandler, ChoreoHandler)
//! - Handler Layer: High-level protocol handlers

// Core modules
pub mod adapters;
pub mod core;
pub mod error;
pub mod types;

// Legacy modules (transitional)
pub mod handlers;

// Re-export unified types and traits
pub use error::{TransportError, TransportErrorBuilder, TransportResult};
pub use types::{
    MessageMetadata, MessagePriority, TransportConfig, TransportEnvelope, TransportType,
};

// Re-export core transport system
pub use core::{MemoryTransport, Transport, TransportFactory};

// Re-export adapters
pub use adapters::{ChoreographicAdapter, ProtocolAdapter};
