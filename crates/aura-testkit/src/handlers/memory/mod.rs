//! Layer 4: Memory-Based Handler Implementations
//!
//! In-memory handler implementations for testing, simulation, and non-persistent coordination.
//! Implement same interfaces as persistent handlers with in-memory storage for fast access.
//!
//! **Handler Types** (per docs/106_effect_system_and_runtime.md):
//! - **MemoryChoreographicHandler**: In-memory choreographic protocol coordination
//!   - Session state management without persistence (perfect for aura-simulator)
//!   - Protocol coordination for test scenarios
//!   - Fast, stateful choreography execution with deterministic behavior
//!
//! - **MemoryLedgerHandler**: In-memory effect_api operations
//!   - Event storage for testing without disk I/O
//!   - Device authorization tracking
//!   - Audit trail for test scenarios and replay
//!
//! **Design Principle** (per docs/106_effect_system_and_runtime.md):
//! Memory handlers provide same handler contracts as persistent implementations, enabling
//! handler swapping for testing. Implement full effect trait semantics while avoiding
//! external dependencies and I/O overhead. Enable deterministic simulation (ExecutionMode::Simulation)

pub mod choreographic_memory;
// pub mod guardian_authorization; // Removed - replaced by Biscuit-based authorization
pub mod effect_api_memory;

pub use choreographic_memory::MemoryChoreographicHandler;
// pub use guardian_authorization::GuardianAuthorizationHandler; // Removed
pub use effect_api_memory::MemoryLedgerHandler;
