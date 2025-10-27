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
//! - `relationships`: Relationship and context types

pub mod identifiers;
pub mod sessions;
pub mod protocols;
pub mod content;
pub mod capabilities;
pub mod relationships;

// Re-export all public types for convenient access
pub use identifiers::*;
pub use sessions::*;
pub use protocols::*;
pub use content::*;
pub use capabilities::*;
pub use relationships::*;

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