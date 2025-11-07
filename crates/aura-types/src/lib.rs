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
//! - `session_epochs`: Session epochs, participant IDs, and session status enums
//! - `protocols`: Protocol types and operation enums
//! - `content`: Content addressing and chunk types
//! - `capabilities`: Capability system types
//! - `peers`: Peer discovery and network coordination types
//! - `relationships`: Relationship and context types
//! - `session_utils`: Session type utilities, events, and formal verification properties

pub mod config;
pub mod content;
pub mod conversions;
pub mod effects;
pub mod encoding;
pub mod errors;
pub mod identifiers;
pub mod macros;
pub mod permissions;
pub mod protocols;
pub mod relationships;
pub mod semilattice;
pub mod serialization;
pub mod session_epochs;
pub mod session_utils;
pub mod time;
// Re-export all public types for convenient access
pub use config::{
    AuraConfig, ConfigDefaults, ConfigFormat, ConfigLoader, ConfigMerge, ConfigValidation,
};
pub use content::*;
pub use encoding::{FromBase64, FromHex, ToBase64, ToHex};
pub use errors::{
    AgentError, AuraError, CapabilityError, CryptoError, DataError, ErrorCode, ErrorContext,
    ErrorSeverity, InfrastructureError, ProtocolError, SessionError, SystemError,
};
// Re-export Result from errors module separately to avoid naming conflicts
pub use errors::Result as AuraResult;
pub use identifiers::*;
pub use permissions::CanonicalPermission;
pub use protocols::*;
pub use relationships::*;
pub use serialization::{Result as SerializationResult, SerializationError};
pub use session_epochs::*;
pub use session_utils::*;
pub use time::{
    current_system_time, current_unix_timestamp, current_unix_timestamp_millis, LamportTimestamp,
};

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
