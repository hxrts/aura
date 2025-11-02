//! Agent module - Refactored modular architecture
//!
//! This module provides a clean, modular Agent implementation with clear separation of concerns.
//! The agent coordinates threshold cryptography, distributed protocols, and storage operations.
//!
//! ## Refactored Module Structure
//!
//! The agent has been completely refactored into focused modules:
//!
//! - `core.rs` - Core agent logic (AgentCore struct and implementations)
//!   - Device and account identification
//!   - Key share and ledger management
//!   - Transport and storage abstractions
//!   - Security validation and key integrity checks
//!   - Helper methods for session commands, metadata storage, and encryption
//!
//! - `session/` - Modular session implementation (split from monolithic session.rs)
//!   - `states.rs` - Session state types and trait definitions
//!   - `bootstrap.rs` - Agent initialization and FROST key generation
//!   - `identity.rs` - DKD identity derivation protocols  
//!   - `storage_ops.rs` - Encrypted storage operations with capabilities
//!   - `coordination.rs` - Recovery and resharing protocol coordination
//!   - `trait_impls.rs` - Agent trait implementations for each state
//!
//! - `capabilities.rs` - Capability-based access control
//!   - Permission conversion and validation
//!   - AccessControlMetadata and protected data structures
//!   - Security validation and issue reporting
//!   - Capability manager integration
//!
//! - `factory.rs` - Agent factory and construction
//!   - AgentFactory for creating agents with different configurations
//!   - Dependency injection and configuration
//!   - Agent initialization workflows
//!
//! ## Original Structure
//!
//! The original monolithic agent.rs (3,622 lines) contained:
//! - Helper functions and utilities
//! - Effects implementation
//! - Core agent structs and methods (~370 lines)
//! - Session management and state machines (~2,226 lines)
//! - Factory pattern (~820 lines)

pub mod capabilities;
pub mod core;
pub mod factory;
pub mod session;

// Re-export main types for public API
pub use capabilities::StorageStats;
pub use core::AgentCore;
pub use factory::AgentFactory;
pub use session::{AgentProtocol, BootstrapConfig, ProtocolStatus, SessionState};

// Re-export session state implementations
pub use session::{Coordinating, Failed, Idle, Uninitialized};

// Re-export capability types
pub use capabilities::{AccessControlMetadata, Effects, KeyShare};
