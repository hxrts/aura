//! Aura Agent: Capability-driven identity and session management
//!
//! This crate provides capability-based agents for the Aura platform:
//! - Pure capability-driven authorization (no legacy policies)
//! - Group messaging with BeeKEM for secure communication
//! - Causal encryption for forward secrecy
//! - Network transport and storage integration
//! - Distributed protocol coordination
//!
//! # Agent Architecture
//!
//! ## [`CapabilityAgent`]: Core capability-driven functionality
//! - Pure capability-based authorization and group messaging
//! - No external dependencies (transport, storage)
//! - Ideal for testing, embedded systems, library integration
//! - Methods: `check_capability()`, `create_group()`, `encrypt()`, `decrypt()`
//!
//! ## [`IntegratedAgent`]: Full system integration
//! - All CapabilityAgent features plus transport and storage
//! - Network-aware capability delegation and revocation
//! - Encrypted data storage with capability-based access control
//! - Methods: `bootstrap()`, `network_connect()`, `store()`, `retrieve()`
//!
//!
//! # Method Naming Conventions
//!
//! - **Core operations**: `check_capability()`, `require_capability()`
//! - **Group operations**: `create_group()`, `list_groups()`
//! - **Data operations**: `encrypt()`, `decrypt()`, `store()`, `retrieve()`
//! - **Network operations**: `network_` prefix for distributed operations
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use aura_agent::{CapabilityAgent, IntegratedAgent};
//! use aura_journal::{DeviceId, AccountId, CapabilityScope};
//!
//! // Core capability agent
//! let device_id = DeviceId::new();
//! let account_id = AccountId::new();
//! let mut agent = CapabilityAgent::new(device_id, account_id);
//!
//! // Bootstrap new account
//! agent.bootstrap_account(vec![device_id], 2)?;
//!
//! // Check capabilities
//! let scope = CapabilityScope::simple("mls", "admin");
//! if agent.check_capability(&scope) {
//!     // Create group with new naming
//!     agent.create_group("team-chat", vec![])?;
//! }
//!
//! // Full integrated agent
//! let integrated = IntegratedAgent::new(device_id, account_id, storage_path).await?;
//! integrated.bootstrap(initial_devices, threshold).await?;
//! integrated.network_connect(peer_id, "127.0.0.1:8080").await?;
//! ```

#![allow(missing_docs, dead_code, unused_imports, unused_variables, clippy::all)]

// Re-export commonly used types and traits for convenience
use serde::{Deserialize, Serialize};
pub use tokio::sync::RwLock;
pub use uuid::Uuid;

/// Core agent functionality and protocol orchestration
pub mod agent;
/// Credential management and session tickets for agent authentication
pub mod credential;
/// P2P Distributed Key Derivation (DKD) orchestration
pub mod dkd;
/// Refined error handling with grouped error types
pub mod error;
/// Guardian management for account recovery and delegation
pub mod guardian;
/// Invitation management for establishing relationships
pub mod invitation;
/// Recovery protocols for restoring access to compromised accounts
pub mod recovery;
/// Multi-device relationship key management for SSB
pub mod relationship_keys;
/// Core types and data structures used throughout the agent system
pub mod types;

pub mod secure_storage;

// New capability-driven agent architecture
/// Pure capability-driven agent with no external dependencies
pub mod capability_agent;
/// Integrated agent with transport and storage capabilities
pub mod integrated_agent;

pub use agent::{DeviceAgent};
pub use credential::*;
pub use guardian::*;
pub use recovery::*;
pub use relationship_keys::*;
pub use types::*;

// Export new capability-driven agents
pub use capability_agent::{AgentConfig, CapabilityAgent};
pub use integrated_agent::{IntegratedAgent, NetworkStats, StorageStats};

// Export error types (both new structured and old compatibility)
pub use error::{
    AgentError, CapabilityError, CryptoError, DataError, InfrastructureError, ProtocolError,
    Result, SystemError,
};
