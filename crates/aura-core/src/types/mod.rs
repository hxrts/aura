//! Core Domain Types
//!
//! Foundational type definitions for the Aura system: identifiers, authority abstractions,
//! scoping constructs, flow budgets, epochs, sessions, and relationships.
//!
//! **Layer 1**: Pure type definitions with no implementations or business logic.

pub mod authority;
pub mod epochs;
pub mod facts;
pub mod flow;
pub mod identifiers;
pub mod participants;
pub mod relationships;
pub mod scope;
pub mod sessions;

// Re-export all public types for convenience
pub use authority::{Authority, AuthorityRef, AuthorityState, TreeStateSummary};
pub use epochs::*;
pub use flow::{FlowBudget, FlowCost, FlowNonce, Receipt, ReceiptSig};
pub use identifiers::{
    AccountId, AuthorityId, ChannelId, ContextId, DataId, DeviceId, DkdContextId, EventId,
    EventNonce, GroupId, GuardianId, HomeId, IndividualId, IndividualIdExt, MemberId,
    MessageContext, NeighborhoodId, OperationId, RelayId, SessionId,
};
pub use participants::{
    FrostThreshold, InvalidThresholdError, NetworkAddress, NetworkAddressError, ParticipantEndpoint,
    ParticipantIdentity, SignerIndexError, SigningParticipant,
};
pub use relationships::*;
pub use scope::{
    AuthorityOp, ContextOp, ResourceScope, ResourceScopeParseError, StoragePath, StoragePathError,
};
pub use sessions::*;

// Fact encoding types
pub use facts::{
    try_decode_fact, try_decode_fact_compatible, try_encode_fact, FactDelta, FactDeltaReducer,
    FactEncoding, FactEnvelope, FactError, FactTypeId,
};
