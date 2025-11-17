//! Memory-Based Handler Implementations
//!
//! This module contains in-memory handler implementations primarily used for
//! testing, development, and non-persistent coordination scenarios.
//!
//! ## Handler Types
//!
//! - **Choreographic Memory**: In-memory choreographic protocol coordination
//!   - Session state management without persistence
//!   - Protocol coordination for testing scenarios
//!   - Fast, stateful choreography execution
//!
//! - **Ledger Memory**: In-memory ledger operations
//!   - Event storage without persistence
//!   - Device authorization tracking
//!   - Audit trail for testing scenarios
//!
//! - **Guardian Authorization**: In-memory guardian management
//!   - Guardian registration and authorization
//!   - Recovery coordination without persistence
//!   - Testing support for guardian protocols
//!
//! ## Design Principles
//!
//! Memory handlers provide the same interfaces as persistent handlers but store
//! all state in memory for fast access and easy testing. They implement the full
//! handler contracts while avoiding external dependencies or persistence overhead.

pub mod choreographic_memory;
pub mod guardian_authorization;
pub mod ledger_memory;

pub use choreographic_memory::MemoryChoreographicHandler;
pub use guardian_authorization::GuardianAuthorizationHandler;
pub use ledger_memory::MemoryLedgerHandler;
