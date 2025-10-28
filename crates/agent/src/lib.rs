//! Aura Agent: Unified capability-driven identity and session management
//!
//! This crate provides a unified agent implementation with session types for
//! compile-time state safety and generic transport/storage abstractions.
//!
//! ## Architecture Overview
//!
//! The unified agent replaces the previous multiple agent types with a single,
//! session-typed, generic implementation that provides:
//!
//! - **Session Types** - Compile-time state safety preventing invalid operations
//! - **Generic Abstractions** - Transport and Storage traits for maximum testability
//! - **State-Gated API** - Only valid operations available for each state
//! - **Witness-Based Transitions** - Cryptographic proofs for state changes
//!
//! ## Agent States and Transitions
//!
//! The unified agent has four main states:
//! - **Uninitialized** - Agent created but not bootstrapped
//! - **Idle** - Ready to perform operations
//! - **Coordinating** - Running long-term protocols (limited API)
//! - **Failed** - Error state (can attempt recovery)
//!
//! ## Usage Examples
//!
//! ### Basic Agent Usage with Compile-Time Safety
//!
//! ```rust,ignore
//! use aura_agent::{AgentFactory, BootstrapConfig, Agent};
//! use aura_types::{AccountId, DeviceId, DeviceIdExt, GuardianId};
//! use uuid::Uuid;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let device_id = DeviceId::new_with_effects(&aura_crypto::Effects::test());
//!     let account_id = AccountId::new(Uuid::new_v4());
//!
//!     // 1. Create uninitialized agent (compile-time enforced)
//!     let uninit_agent = AgentFactory::create_test(device_id, account_id).await?;
//!
//!     // 2. This WON'T compile - can't store data before bootstrap:
//!     // uninit_agent.store_data(b"data", vec!["read".to_string()]).await?;
//!
//!     // 3. Must bootstrap first (consumes uninitialized agent)
//!     let bootstrap_config = BootstrapConfig {
//!         threshold: 2,
//!         share_count: 3,
//!         parameters: Default::default(),
//!     };
//!     let idle_agent = uninit_agent.bootstrap(bootstrap_config).await?;
//!
//!     // 4. Now operations are allowed (compile-time safe)
//!     let identity = idle_agent.derive_identity("my-app", "user-context").await?;
//!     let data_id = idle_agent.store_data(b"secret data", vec!["read".to_string()]).await?;
//!
//!     // 5. Start long-running protocol (consumes idle agent)
//!     let coordinating_agent = idle_agent.initiate_recovery(serde_json::json!({})).await?;
//!
//!     // 6. This WON'T compile - can't start another protocol while coordinating:
//!     // coordinating_agent.initiate_resharing(3, vec![device_id]).await?;
//!
//!     // 7. Can only check status or cancel while coordinating
//!     let status = coordinating_agent.check_protocol_status().await?;
//!
//!     Ok(())
//! }
//! ```

#![allow(missing_docs, dead_code, unused_imports, unused_variables, clippy::all)]

// Re-export commonly used types and traits for convenience
use serde::{Deserialize, Serialize};
pub use tokio::sync::RwLock;
pub use uuid::Uuid;

// ========== Agent Implementation ==========
/// Agent implementation with session types
pub mod agent;

/// Agent trait definitions
pub mod traits;

/// Infrastructure implementations of Transport and Storage
pub mod infrastructure;

/// Transport adapters for bridging agent and coordination layers
pub mod transport_adapter;

/// FROST threshold signature management
pub mod frost_manager;

/// Structured error hierarchy
pub mod error;

/// Platform-specific secure storage
pub mod secure_storage;

// ========== Essential Types ==========
/// Derived identity result from DKD protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedIdentity {
    pub app_id: String,
    pub context: String,
    pub identity_key: Vec<u8>, // Placeholder - should be actual key type
    pub proof: Vec<u8>,        // Placeholder - should be actual proof type
}

// ========== Re-exports for Convenience ==========

// Agent types (main interface)
pub use agent::{
    // Agent-specific types
    AgentCore,
    AgentFactory,
    AgentProtocol,
    BootstrapConfig,
    Coordinating,
    Failed,
    Idle,
    KeyShare,
    ProtocolCompleted,
    ProtocolStatus,
    Storage,
    StorageStats,
    Transport,
    // Type aliases for convenience
    UnifiedAgent,
    // Session states
    Uninitialized,
};

// Infrastructure implementations
pub use infrastructure::{ProductionFactory, ProductionStorage, ProductionTransport};

// Transport adapters
pub use transport_adapter::{CoordinationTransportAdapter, TransportAdapterFactory};

// FROST management
pub use frost_manager::{FrostAgent, FrostKeyManager, FrostSigningSession};

// Agent traits
pub use traits::{Agent, CoordinatingAgent, GroupAgent, IdentityAgent, NetworkAgent, StorageAgent};

// Error types
pub use error::{
    AgentError, CapabilityError, DataError, InfrastructureError, ProtocolError, Result,
};

// Secure storage types
pub use secure_storage::{
    DeviceAttestation, PlatformSecureStorage, SecureStorage, SecurityLevel,
    #[cfg(target_os = "android")]
    AndroidKeystoreStorage,
};
