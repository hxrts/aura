//! Aura Choreographic Protocol Implementations
//!
//! This crate contains concrete implementations of Aura's distributed protocols
//! using choreographic programming patterns. It builds on the middleware and
//! infrastructure provided by `aura-protocol`.
//!
//! ## Protocol Categories
//!
//! - **Threshold Cryptography**: DKD and FROST signing protocols
//! - **Coordination**: Journal synchronization and epoch management
//! - **Patterns**: Reusable coordination patterns like distributed lottery
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use aura_choreography::threshold_crypto::KeyFabricThresholdChoreography;
//! use aura_protocol::choreographic::ChoreographicHandlerBuilder;
//!
//! // Create choreographic handler with middleware
//! let handler = ChoreographicHandlerBuilder::new(effects)
//!     .with_device_name("device-1".to_string())
//!     .build_in_memory(device_id, context);
//!
//! // Execute KeyFabric threshold unwrapping
//! let threshold = KeyFabricThresholdChoreography::new(config, fabric_nodes, effects)?;
//! let result = threshold.execute(&mut handler, &mut endpoint, my_role).await?;
//! ```

#![allow(clippy::result_large_err)]
#![allow(clippy::large_enum_variant)]
#![allow(missing_docs, dead_code)]

/// Threshold cryptography protocol implementations
pub mod threshold_crypto;

/// Coordination protocol implementations  
pub mod coordination;

/// Reusable protocol patterns
pub mod patterns;

/// Common utilities shared across protocols
pub mod common;

/// Integration with aura-protocol infrastructure
pub mod integration;

/// Test utilities for choreographic protocols
/// TODO: Re-implement test utilities for KeyFabric choreographies
// #[cfg(any(test, feature = "test-utils"))]
// pub mod test_utils;

// Re-export key types from aura-protocol for convenience
pub use aura_protocol::effects::choreographic::{
    ChoreographicRole, ChoreographyError, ChoreographyEvent, ChoreographyMetrics,
};

// Re-export protocol implementations
pub use coordination::{
    EpochBumpChoreography, JournalSyncChoreography, SessionEpochMonitor,
};
pub use patterns::{DecentralizedLottery, LotteryMessage};
pub use threshold_crypto::{
    KeyFabricThresholdChoreography, KeyFabricShareContributionChoreography,
    KeyFabricNodeRotationChoreography, KeyFabricFrostSigningChoreography
};