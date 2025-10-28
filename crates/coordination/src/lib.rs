//! Protocol Coordination for Aura
#![allow(clippy::result_large_err)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::single_match_else)]
#![allow(clippy::manual_async_fn)]
#![allow(clippy::empty_line_after_doc_comments)]
//!
//! This crate provides coordination infrastructure for Aura's distributed protocols.
//!
//! ## Architecture
//!
//! The coordination crate is organized into several key layers:
//!
//! - **`protocols/`** - Complete protocol implementations combining session types
//!   and choreographic execution (DKD, Recovery, Resharing)
//! - **`execution/`** - Protocol execution infrastructure and context management
//! - **`session_types/`** - Session type infrastructure for type-safe protocols
//! - **`local_runtime/`** - Local session runtime for per-device coordination
//! - **`instrumentation/`** - Dev console integration hooks and trace recording
//! - **`utils/`** - Low-level coordination-specific utilities
//!
//! ## Usage
//!
//! For most use cases, import the complete protocol implementations:
//!
//! ```rust,ignore
//! use aura_coordination::protocols::{dkd_choreography, recovery_choreography};
//! use aura_coordination::execution::ProtocolContext;
//! ```
//!
//! For dev console integration:
//!
//! ```rust,ignore
//! #[cfg(feature = "dev-console")]
//! use aura_coordination::instrumentation::{InstrumentationHooks, TraceRecorder};
//! ```
//!
//! ## Crate Dependencies
//!
//! This crate follows a clean dependency hierarchy:
//!
//! - **Dependencies**: `aura-crypto`, `aura-journal`, `session`, `aura-groups`
//! - **Dev Dependencies**: `aura-test-utils` (for testing utilities)
//! - **Used By**: `aura-simulator` (protocol execution), `aura-sim-server` (trace recording)
//! - **Features**: `dev-console` enables instrumentation for the Aura Dev Console

#![allow(
    missing_docs,
    dead_code,
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::too_many_arguments
)]

// ========== Error Types ==========
pub mod error;
pub use error::{CoordinationError, Result};

// ========== Basic Infrastructure ==========
pub mod utils;

// ========== Core Types ==========
pub mod types;
pub use types::{
    IdentifierMapping, KeyShare, ParticipantId, PublicKeyPackage, SealedShare, SessionId,
    ThresholdConfig, ThresholdSignature,
};

// Utility exports
pub use utils::{compute_lottery_ticket, determine_lock_winner};

// ========== Protocol Execution ==========
pub mod execution;
pub use execution::{ProductionTimeSource, ProtocolContext, MemoryTransport, Transport};

// ========== Complete Protocols ==========
pub mod protocols;
// Direct exports for core protocols - lifecycle-only architecture
pub use protocols::{
    CounterLifecycle, CounterLifecycleError, DkdLifecycle, DkdLifecycleError, GroupLifecycle,
    GroupLifecycleError, LockingLifecycle, LockingLifecycleError, RecoveryLifecycle,
    RecoveryLifecycleError, ResharingLifecycle, ResharingLifecycleError,
};

// Counter choreographic functions removed - use CounterLifecycle through LifecycleScheduler

// ========== Lifecycle Scheduler ==========
pub mod runtime;
pub use runtime::lifecycle_scheduler::LifecycleScheduler;

// ========== Tracing and Logging ==========
pub mod tracing;

// ========== Test Utilities ==========
// Test utilities are now located in tests/test_utils.rs

// ========== Error Recovery ==========
pub mod error_recovery;

// ========== Local Session Runtime ==========
pub mod local_runtime;
pub mod session_runtime_config;
pub use local_runtime::{
    DkdResult, LocalSessionRuntime, SessionCommand, SessionProtocolType, SessionResponse,
    SessionStatusInfo,
};
pub use session_runtime_config::{
    ConfigurationError, SecurityConfig, SessionConfig, SessionRuntimeConfig,
    SessionRuntimeConfigBuilder, SessionRuntimeFactory, TransportConfig,
};

// ========== FROST Session Management ==========
pub mod frost_session_manager;
pub use frost_session_manager::{FrostSession, FrostSessionManager};

// ========== Capability Authorization ==========
pub mod capability_authorization;
pub use capability_authorization::{
    create_capability_authorization_manager, CapabilityAuthError, CapabilityAuthorizationManager,
};

// ========== Session Types ==========
pub mod session_types;
pub use protocols::{ProtocolWrapper, ProtocolWrapperBuilder, ProtocolWrapperError};
pub use session_types::{SessionProtocol, SessionTypedProtocol};

// ========== Service Layer Architecture ==========
pub mod coordination_service;
pub mod protocol_results;
pub use coordination_service::{CoordinationService, ServiceHealthStatus};

// ========== Dev Console Instrumentation ==========
pub mod instrumentation;
