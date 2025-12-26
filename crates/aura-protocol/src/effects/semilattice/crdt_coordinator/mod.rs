//! CRDT Coordinator for Choreographic Protocol Integration
//!
//! This module provides the `CrdtCoordinator` that bridges CRDT handlers
//! with choreographic protocols, enabling distributed state synchronization
//! across all four CRDT types (CvRDT, CmRDT, Delta-CRDT, MvRDT).
//!
//! # Builder Pattern
//!
//! The coordinator uses a clean builder pattern for ergonomic setup:
//!
//! ```ignore
//! // Convergent CRDT with default state
//! let coordinator = CrdtCoordinator::with_cv(authority_id);
//!
//! // Convergent CRDT with initial state
//! let coordinator = CrdtCoordinator::with_cv_state(authority_id, my_state);
//!
//! // Commutative CRDT
//! let coordinator = CrdtCoordinator::with_cm(authority_id, initial_state);
//!
//! // Delta CRDT with compaction threshold
//! let coordinator = CrdtCoordinator::with_delta_threshold(authority_id, 100);
//!
//! // Meet-semilattice CRDT
//! let coordinator = CrdtCoordinator::with_mv(authority_id);
//!
//! // RECOMMENDED: Use pre-composed handlers from aura-composition
//! // let factory = HandlerFactory::for_testing(device_id)?;
//! // let registry = factory.create_registry()?;
//! // Extract handlers from registry for coordination
//!
//! // Compose handlers externally and inject them
//! // let coordinator = CrdtCoordinator::new(authority_id)
//! //     .with_cv_handler(precomposed_cv_handler)
//! //     .with_delta_handler(precomposed_delta_handler);
//! ```
//!
//! # Integration with Choreographies
//!
//! Use the coordinator in anti-entropy and other synchronization protocols:
//!
//! ```ignore
//! let coordinator = CrdtCoordinator::with_cv_state(authority_id, journal_state);
//! let result = execute_anti_entropy(
//!     authority_id,
//!     config,
//!     is_requester,
//!     &effect_system,
//!     coordinator,
//! ).await?;
//! ```

mod clock;
mod coordinator;
mod error;

pub use clock::{increment_actor, max_counter, merge_vector_clocks};
pub use coordinator::CrdtCoordinator;
pub use error::CrdtCoordinatorError;

