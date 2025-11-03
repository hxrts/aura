//! Core shared types for the Aura platform
//!
//! This crate provides the fundamental data structures and type definitions used
//! across the entire Aura workspace. It serves as the single source of truth for
//! core identifiers, session types, protocol enums, and other shared domain types.
//!
//! # Design Principles
//!
//! - **Single Source of Truth**: Core types are defined once and used everywhere
//! - **Zero Duplication**: Eliminates redundant type definitions across crates
//! - **Clear Hierarchies**: Organized into logical modules by domain
//! - **Minimal Dependencies**: Only essential dependencies for serialization and utilities
//!
//! # Architecture
//!
//! Types are organized into modules by domain:
//! - `identifiers`: Core ID types (SessionId, EventId, etc.)
//! - `sessions`: Session-related types and status enums
//! - `protocols`: Protocol types and operation enums
//! - `content`: Content addressing and chunk types
//! - `capabilities`: Capability system types
//! - `peers`: Peer discovery and network coordination types
//! - `relationships`: Relationship and context types
//! - `session_utils`: Session type utilities, events, and formal verification properties

pub mod capabilities;
pub mod config;
pub mod content;
pub mod conversions;
pub mod crdt;
pub mod effects;
pub mod encoding;
pub mod errors;
pub mod fabric;
pub mod identifiers;
pub mod macros;
// pub mod middleware; // Temporarily disabled due to async lifetime issues
pub mod simple_middleware;
pub mod peers;
pub mod permissions;
pub mod protocol_types;
pub mod protocols;
pub mod relationships;
pub mod serialization;
pub mod session_utils;
pub mod sessions;

pub mod time_utils;
// Re-export all public types for convenient access
pub use capabilities::*;
pub use config::{
    AuraConfig, ConfigDefaults, ConfigFormat, ConfigLoader, ConfigMerge, ConfigValidation,
};
pub use content::*;
pub use crdt::{AutomergeCrdt, CrdtBuilder, CrdtError, CrdtOperation, CrdtState, CrdtValue};
pub use effects::{
    AuraEffects, ConsoleEffects, CryptoEffects, EffectsBuilder, NetworkEffects, RandomEffects,
    StorageEffects, TimeEffects,
};
pub use encoding::{FromBase64, FromHex, ToBase64, ToHex};
pub use errors::{
    AgentError, AuraError, CapabilityError, CryptoError, DataError, ErrorCode, ErrorContext,
    ErrorSeverity, InfrastructureError, ProtocolError, SessionError, SystemError,
};
pub use fabric::{
    CryptoBackendId, EdgeId, EdgeKind, HashFunctionId, KeyEdge, KeyNode, NodeCommitment, NodeId,
    NodeKind, NodePolicy, ShareHeader,
};
// Re-export Result from errors module separately to avoid naming conflicts
pub use errors::Result as AuraResult;
pub use identifiers::*;
// pub use middleware::{AuraMiddleware, HandlerMetadata, MiddlewareContext, MiddlewareError, MiddlewareResult, PerformanceProfile}; // Temporarily disabled
pub use simple_middleware::{MiddlewareContext, MiddlewareResult};
pub use peers::*;
pub use permissions::CanonicalPermission;
pub use protocol_types::*;
pub use protocols::*;
pub use relationships::*;
pub use serialization::{Result as SerializationResult, SerializationError};
pub use session_utils::*;
pub use sessions::*;

/// Result type for type-related operations
pub type Result<T> = std::result::Result<T, TypeError>;

/// Errors that can occur with type operations
#[derive(thiserror::Error, Debug, Clone)]
pub enum TypeError {
    /// Invalid identifier format error
    #[error("Invalid identifier format: {0}")]
    InvalidIdentifier(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Parse error
    #[error("Parse error: {0}")]
    ParseError(String),
}
