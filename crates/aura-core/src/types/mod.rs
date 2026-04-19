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

// Re-export all public types for convenience
pub use authority::{Authority, AuthorityRef, AuthorityState, TreeStateSummary};
pub use epochs::{Epoch, ParticipantId};
pub use flow::{FlowBudget, FlowCost, FlowNonce, Receipt, ReceiptSig};
pub use identifiers::{
    derive_legacy_authority_from_device, AccountId, AuthorityId, ChannelId, ContextId, DataId,
    DeviceId, DkdContextId, EventId, EventNonce, GroupId, GuardianId, HomeId, IndividualId,
    IndividualIdExt, LegacyAuthorityFromDeviceDerivation, LegacyAuthorityFromDeviceError,
    LegacyAuthorityFromDeviceReason, LegacyAuthorityFromDeviceRequest, MemberId, MessageContext,
    NeighborhoodId, OperationId, RelayId, SessionId,
};
pub use participants::{
    FrostThreshold, InvalidThresholdError, NetworkAddress, NetworkAddressError,
    ParticipantEndpoint, ParticipantIdentity, SignerIndexError, SigningParticipant,
};
pub use relationships::*;
pub use scope::{
    AuthorityOp, ContextOp, ResourceScope, ResourceScopeParseError, StoragePath, StoragePathError,
};

// Fact encoding types
pub use facts::{
    try_decode_fact, try_decode_fact_compatible, try_encode_fact, FactDelta, FactDeltaReducer,
    FactEncoding, FactEnvelope, FactError, FactSchemaCompatibility, FactTypeId,
};
