//! Choreographic Protocol Implementations
//!
//! This module contains concrete choreographic protocol implementations for various
//! distributed operations in Aura. All protocols use the rumpsteak-aura DSL for
//! session-typed communication with automatic guard chain enforcement.
//!
//! ## Protocol Categories
//!
//! ### State Synchronization
//!
//! The anti_entropy module implements digest-based state reconciliation for CRDT
//! synchronization. The protocol follows a four-phase pattern where the requester
//! initiates synchronization, the responder sends state digests using bloom filters,
//! the requester identifies missing operations, and the responder transmits those
//! operations. This protocol integrates with all four CRDT handlers (CvHandler for
//! convergent state, CmHandler for causal operations, DeltaHandler for bandwidth
//! optimization, and MvHandler for meet-semilattice constraints). Anti-entropy
//! ensures eventual consistency across replicas while minimizing network overhead
//! through incremental synchronization.
//!
//! ### Coordination Primitives Only
//!
//! This layer (aura-protocol) contains only coordination primitives and infrastructure
//! protocols. Domain-specific threshold cryptography protocols are implemented in the
//! aura-frost crate (Layer 5) according to the clean architecture principles. The
//! aura-frost crate contains complete threshold signature choreographies including
//! FROST key generation, threshold signing ceremonies, and key resharing protocols.
//! Those domain-specific protocols use the coordination infrastructure from this
//! layer but implement their business logic in the appropriate feature layer.
//!
//! ### Tree Operations
//!
//! The tree_coordination module implements choreographies for coordinating tree
//! operations with threshold approval. The protocol supports multiple roles including
//! initiators who propose operations, approvers who vote on proposals, and observers
//! who track results without voting. The choreography handles operation validation,
//! approval collection, threshold checking, and attested operation generation. Tree
//! synchronization is integrated to ensure all participants converge on the same
//! tree state before and after operations.
//!
//! The snapshot module implements coordinated garbage collection through threshold-
//! approved snapshots. The protocol addresses upgrade safety by treating snapshot
//! commits as protocol version gates, allowing old peers to refuse pruning while
//! continuing to merge new operations. The choreography proceeds through proposal
//! (where the proposer suggests a snapshot cut), approval (where quorum members
//! provide partial signatures), finalization (where the proposer aggregates signatures
//! and distributes the snapshot), and upgrade safety checks (where protocol versions
//! are validated before garbage collection). This ensures atomic compaction across
//! replicas while maintaining forward compatibility.
//!
//! ### Over-the-Air Upgrades
//!
//! The ota module implements OTA upgrade orchestration with soft fork and hard fork
//! support. The protocol enables safe protocol upgrades by coordinating adoption
//! across multiple devices with optional mandatory activation for breaking changes.
//! The choreography proceeds through proposal (where the coordinator broadcasts the
//! upgrade), adoption (where devices opt in or reject), activation (where hard forks
//! with sufficient adoption and reached epoch fences are activated), and completion
//! (where all devices confirm upgrade application and emit cache invalidation events).
//! Epoch fences prevent split-brain scenarios by ensuring all devices in a threshold
//! account upgrade within the same epoch boundary.
//!
//! ## Guard Chain Integration
//!
//! All choreographic protocols automatically integrate with the complete guard chain
//! (CapGuard to FlowGuard to JournalCoupler) through the AuraHandlerAdapter. Each
//! send operation in a choreography triggers capability checking, budget charging,
//! and journal coupling without explicit guard invocations in protocol code. Message-
//! specific guard profiles control required capabilities, flow costs, leakage budgets,
//! and delta facts for journal updates. Receipts generated during budget charging
//! provide cryptographic proof for multi-hop scenarios.
//!
//! ## Execution Modes
//!
//! All protocols support three execution modes through the AuraHandlerAdapter. Testing
//! mode uses in-memory handlers with deterministic message delivery for unit tests.
//! Production mode connects to real network transports and persistent storage for
//! deployment. Simulation mode enables fault injection and controlled non-determinism
//! for property testing. Protocol implementations remain identical across modes with
//! behavior controlled by handler configuration.

pub mod anti_entropy;
// pub mod frost; // REMOVED: Superseded by aura-frost crate
pub mod ota;
pub mod snapshot;
// pub mod threshold_ceremony; // MOVED: Now implemented in aura-frost crate
pub mod tree_coordination;
pub mod tree_sync;

// Re-export specific protocol types to avoid macro-generated conflicts
// Note: choreography! macros generate types that conflict when re-exported with *
// so we only re-export the main types that are meant to be public

pub use anti_entropy::{AntiEntropyConfig, AntiEntropyError};
// Note: snapshot, threshold_ceremony, and tree_coordination choreographies are temporarily disabled
// due to macro conflicts. Only re-export types that don't depend on choreography! generated types.
// FROST removed: Use aura_frost crate instead
// ota::* exports omitted pending implementation
pub use snapshot::{SnapshotConfig, SnapshotProposal};
// pub use threshold_ceremony::{ThresholdCeremonyConfig, CeremonyError}; // Disabled - choreography types unavailable
// pub use tree_coordination::{TreeCoordinationConfig, TreeOperationRequest, TreeOperationError}; // Disabled - choreography types unavailable
