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
//! - **ChoreographySubsystem**: Session state and composite handler adapter
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

pub use choreography::ChoreographySubsystem;
pub use crypto::CryptoSubsystem;
pub use journal::JournalSubsystem;
pub use transport::TransportSubsystem;
