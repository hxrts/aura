//! Core Domain Types
//!
//! Foundational type definitions for the Aura system: identifiers, authority abstractions,
//! scoping constructs, flow budgets, epochs, sessions, and relationships.
//!
//! **Layer 1**: Pure type definitions with no implementations or business logic.

pub mod authority;
pub mod epochs;
pub mod flow;
pub mod identifiers;
pub mod relationships;
pub mod scope;
pub mod sessions;

// Re-export all public types for convenience
pub use authority::{Authority, AuthorityRef, AuthorityState, TreeState};
pub use epochs::*;
pub use flow::{FlowBudget, Receipt};
pub use identifiers::{
    AccountId, AuthorityId, ChannelId, ContextId, DataId, DeviceId, DkdContextId, EventId,
    EventNonce, GroupId, GuardianId, IndividualId, IndividualIdExt, MemberId, MessageContext,
    OperationId, RelayId, SessionId,
};
pub use relationships::*;
pub use scope::{AuthorityOp, ContextOp, ResourceScope};
pub use sessions::*;
