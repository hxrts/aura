//! Aura Guards prelude.
//!
//! Curated re-exports for guard-chain usage without pulling in extra modules.

pub use crate::guards::chain::{create_send_guard, create_send_guard_op, SendGuardChain, SendGuardResult};
pub use crate::guards::executor::{execute_guarded_choreography, BorrowedEffectInterpreter};
pub use crate::guards::{
    GuardContextProvider, GuardEffects, GuardOperation, GuardedExecutionResult, JournalCoupler,
    LeakageBudget, ProtocolGuard,
};
pub use crate::guards::execution::execute_guarded_operation;

/// Composite effect requirements for guard-chain usage.
pub trait GuardsEffects: GuardEffects + GuardContextProvider + aura_core::effects::time::PhysicalTimeEffects {}

impl<T> GuardsEffects for T where
    T: GuardEffects + GuardContextProvider + aura_core::effects::time::PhysicalTimeEffects
{
}
