//! Aura protocol prelude.
//!
//! Curated re-exports for protocol orchestration.

pub use crate::error::ProtocolError;
pub use crate::handlers::*;
pub use crate::session::{SessionOutcome, SessionStatus};
pub use crate::types::{
    ProtocolDuration, ProtocolMode, ProtocolPriority, ProtocolSessionStatus, ProtocolType,
};

/// Composite effect requirements for protocol orchestration (excludes StorageEffects).
pub trait ProtocolEffects:
    aura_guards::GuardEffects
    + aura_guards::GuardContextProvider
    + aura_core::effects::time::PhysicalTimeEffects
    + aura_core::effects::FlowBudgetEffects
    + aura_core::effects::JournalEffects
    + aura_core::effects::LeakageEffects
    + aura_core::effects::TransportEffects
{
}

impl<T> ProtocolEffects for T where
    T: aura_guards::GuardEffects
        + aura_guards::GuardContextProvider
        + aura_core::effects::time::PhysicalTimeEffects
        + aura_core::effects::FlowBudgetEffects
        + aura_core::effects::JournalEffects
        + aura_core::effects::LeakageEffects
        + aura_core::effects::TransportEffects
{
}
