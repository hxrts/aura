//! AuraEffectSystem Subsystems
//!
//! This module contains extracted subsystems from the monolithic AuraEffectSystem.
//! Each subsystem groups related fields and functionality for better organization
//! and maintainability.
//!
//! ## Subsystem Structure
//!
//! - **CryptoSubsystem**: Cryptographic operations, RNG, and secure storage
//! - **TransportSubsystem**: Network transport, inbox management, and statistics
//! - **JournalSubsystem**: Indexed journal, fact registry, and publication
//! - **ChoreographyState**: In-memory session state for choreography coordination
//!
//! ## Design Principles
//!
//! 1. **Cohesion**: Each subsystem groups tightly-related fields
//! 2. **Encapsulation**: Subsystems hide internal implementation details
//! 3. **Delegation**: AuraEffectSystem delegates to subsystems for operations
//! 4. **Testability**: Subsystems can be mocked independently

pub mod choreography;
pub mod crypto;
pub mod journal;
pub mod transport;
pub mod vm_bridge;
pub mod vm_fragment;

pub use choreography::ChoreographyState;
pub use crypto::CryptoSubsystem;
pub use journal::JournalSubsystem;
pub use transport::TransportSubsystem;
pub use vm_bridge::VmBridgeState;
#[allow(unused_imports)] // Re-exported for runtime-facing ownership helpers and tests.
pub(in crate::runtime) use vm_fragment::{
    VmFragmentId, VmFragmentOwnerRecord, VmFragmentOwnershipError, VmFragmentRegistry,
};
