//! Aura Core Prelude
//!
//! This module provides convenient re-exports of the most commonly used types
//! from `aura-core`. Import with:
//!
//! ```rust
//! use aura_core::prelude::*;
//! ```
//!
//! # Included Types
//!
//! ## Identifiers
//! - `AuthorityId`, `ContextId`, `SessionId`, `DeviceId`, `ChannelId`
//! - `AccountId`, `GroupId`, `RelayId`, `GuardianId`
//!
//! ## Core Types
//! - `AuraError`, `AuraResult` - Unified error handling
//! - `Journal`, `Fact`, `Cap` - CRDT types
//! - `EffectContext` - Operation context for effects
//!
//! ## Semilattice Traits
//! - `JoinSemilattice`, `MeetSemiLattice` - Algebraic operations
//!
//! ## Time Types
//! - `TimeStamp`, `TimeDomain` - Unified time system

// === Error Types ===
pub use crate::errors::{AuraError, ProtocolErrorCode, Result as AuraResult};
pub use crate::faults::{AuraFault, AuraFaultKind, CorruptionMode, FaultEdge};

// === Core Identifiers ===
pub use crate::types::identifiers::{
    derive_legacy_authority_from_device, AccountId, AuthorityId, ChannelId, ContextId, DeviceId,
    GroupId, GuardianId, LegacyAuthorityFromDeviceReason, LegacyAuthorityFromDeviceRequest,
    RelayId, SessionId,
};

// === Journal & CRDT Types ===
pub use crate::domain::journal::{Cap, Fact, FactKey, Journal};

// === Semilattice Traits ===
pub use crate::semilattice::{JoinSemilattice, MeetSemiLattice};

// === Context ===
pub use crate::context::{EffectContext, OperationSessionId};

// === Time ===
pub use crate::time::{TimeDomain, TimeStamp};

// === Authority ===
pub use crate::types::authority::{Authority, AuthorityState};
/// Alias for AuthorityId (for backwards compatibility)
pub use crate::types::identifiers::AuthorityId as AuthId;

// === Tree Types ===
pub use crate::tree::{Epoch, LeafId, NodeIndex, Policy};

// === Common Hash Type ===
pub use crate::domain::content::Hash32;
