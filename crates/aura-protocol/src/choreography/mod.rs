//! Layer 4: Choreographic Protocol Infrastructure - MPST & Session Types
//!
//! Distributed protocols using choreographic global specifications that automatically
//! project to per-role local session types. Ensures deadlock freedom by construction
//! and compile-time verification of message matching (per docs/107_mpst_and_choreography.md).
//!
//! **Choreography Model** (per docs/107_mpst_and_choreography.md):
//! 1. Define protocol once globally (all roles visible)
//! 2. Compiler projects to per-role local types (session types for each role)
//! 3. Execute using aura-mpst runtime with effect traits
//! 4. Deadlock freedom guaranteed by session type properties
//!
//! **Guard Chain Integration** (per docs/003_information_flow_contract.md):
//! Each choreography message flows through guard chain with annotations:
//! **@guard_capability** → **@flow_cost** → **@leak** → Journal → Transport
//!
//! **Message Flow**:
//! 1. Effect invocation (choreographic method call)
//! 2. Guard evaluation (CapGuard checks Biscuit, FlowGuard charges budget)
//! 3. Delta facts merged atomically (JournalCoupler)
//! 4. Transport happens (NetworkEffects::send)
//!
//! **Module Organization**:
//! - **crdt_sync**: CRDT synchronization message types (shared infrastructure)
//! - **handler_bridge**: Choreographic execution trait abstractions
//!
//! **Composition Principle**: Domain-specific choreographies (journal sync, consensus)
//! live in feature crates (aura-anti-entropy, aura-consensus) to avoid circular dependencies.

pub mod crdt_sync;
pub mod handler_bridge;

// Re-export CRDT synchronization types
pub use crdt_sync::{CrdtOperation, CrdtSyncData, CrdtSyncRequest, CrdtSyncResponse, CrdtType};

// Re-export the clean handler bridge traits
pub use handler_bridge::{
    ChoreographicAdapter, ChoreographicEndpoint, ChoreographicHandler, DefaultEndpoint,
    SendGuardProfile,
};

// NOTE: Epoch management has been moved to aura-sync (Layer 5)
// Import aura-sync directly if you need epoch coordination protocols

// #[cfg(test)]
// pub use handler_bridge::MockChoreographicAdapter;
