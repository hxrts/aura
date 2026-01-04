//! Delivery and consistency policy framework.
//!
//! This module provides policies for controlling when acknowledgment
//! tracking should be dropped and how consistency metadata is interpreted.
//!
//! # Architecture
//!
//! ```text
//! Journal Layer                    App Layer
//! ┌──────────────────┐            ┌─────────────────────┐
//! │ Fact + AckStorage│            │ DeliveryPolicy      │
//! │                  │◄───────────│  - expected_peers() │
//! │ gc_ack_tracking()│            │  - should_drop()    │
//! └──────────────────┘            └─────────────────────┘
//!                                         │
//!                                         ▼
//!                                 ┌─────────────────────┐
//!                                 │ PolicyRegistry      │
//!                                 │  - register<F>()    │
//!                                 │  - get_policy()     │
//!                                 └─────────────────────┘
//! ```

pub mod delivery;
pub mod registry;
pub mod status_interpreter;

// Re-export key types
pub use delivery::{
    boxed, BoxedPolicy, ChannelMembersPolicy, DeliveryPolicy, DropWhenFinalized,
    DropWhenFinalizedAndFullyAcked, DropWhenFullyAcked, DropWhenSafeAndFullyAcked,
    NoOpPolicyContext, PolicyContext,
};

pub use registry::{PolicyRegistry, TypedPolicyRegistry};

pub use status_interpreter::{
    CategoryKind, CeremonyDetails, NoOpStatusContext, ProposalDetails, StatusContext,
    StatusInterpreter, StatusResult,
};
