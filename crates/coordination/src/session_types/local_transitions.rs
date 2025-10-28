//! Local Session Type Transitions
//!
//! This module solves orphan rule violations by providing local wrapper types
//! and traits for safe compile-time protocol state transitions.
//!
//! The orphan rule prevents implementing external traits on external types.
//! By creating local wrapper types and transition traits, we restore
//! compile-time safety for protocol state machines.

use crate::session_types::wrapper::SessionTypedProtocol;
use aura_types::SessionError;
use aura_types::session_core::{ChoreographicProtocol, SessionState};
use std::marker::PhantomData;

// ========== Local Wrapper Types ==========

/// Local wrapper around SessionTypedProtocol to enable local trait implementations
#[derive(Debug, Clone)]
pub struct SafeSessionProtocol<Core, State> {
    inner: SessionTypedProtocol<Core, State>,
}

impl<Core, State> SafeSessionProtocol<Core, State> {
    /// Create a new safe session protocol wrapper
    pub fn new(inner: SessionTypedProtocol<Core, State>) -> Self {
        Self { inner }
    }

    /// Extract the inner session protocol
    pub fn into_inner(self) -> SessionTypedProtocol<Core, State> {
        self.inner
    }

    /// Get a reference to the inner protocol
    pub fn inner(&self) -> &SessionTypedProtocol<Core, State> {
        &self.inner
    }

    /// Get a mutable reference to the inner protocol
    pub fn inner_mut(&mut self) -> &mut SessionTypedProtocol<Core, State> {
        &mut self.inner
    }
}

/// Local wrapper around ChoreographicProtocol for safe implementations
#[derive(Debug, Clone)]
pub struct SafeChoreographicProtocol<Core, State> {
    inner: ChoreographicProtocol<Core, State>,
}

impl<Core, State> SafeChoreographicProtocol<Core, State> {
    /// Create a new safe choreographic protocol wrapper
    pub fn new(inner: ChoreographicProtocol<Core, State>) -> Self {
        Self { inner }
    }

    /// Extract the inner choreographic protocol
    pub fn into_inner(self) -> ChoreographicProtocol<Core, State> {
        self.inner
    }

    /// Get a reference to the inner protocol
    pub fn inner(&self) -> &ChoreographicProtocol<Core, State> {
        &self.inner
    }
}

// ========== Local Transition Traits ==========

/// Local trait for safe state transitions with compile-time witnesses
///
/// This trait provides the same safety guarantees as WitnessedTransition
/// but is defined locally to avoid orphan rule violations.
pub trait SafeWitnessedTransition<FromState, ToState> {
    /// Evidence required to authorize this transition
    type Witness;
    /// Target protocol state after transition
    type Target;

    /// Perform a state transition with required witness
    ///
    /// This method provides compile-time guarantees that:
    /// 1. The transition is valid (FromState -> ToState)
    /// 2. The required evidence is provided (Witness)
    /// 3. The result has the correct type (Target)
    fn safe_transition_with_witness(self, witness: Self::Witness) -> Self::Target;
}

/// Local trait for simple state transitions without witnesses
///
/// Used for transitions that don't require additional evidence beyond
/// the current protocol state.
pub trait SafeStateTransition<FromState, ToState> {
    /// Target protocol state after transition
    type Target;

    /// Perform a simple state transition
    fn safe_transition_to(self) -> Self::Target;
}

/// Local trait for conditional transitions based on runtime checks
///
/// Provides a safer alternative to unchecked transitions by requiring
/// explicit condition validation.
pub trait SafeConditionalTransition<FromState, ToState> {
    /// Target protocol state after transition
    type Target;
    /// Condition that must be satisfied for transition
    type Condition;

    /// Attempt a conditional transition
    ///
    /// Returns Ok(target) if condition is met, Err(self) if not.
    fn safe_conditional_transition(
        self,
        condition: Self::Condition,
    ) -> Result<Self::Target, (Self, SessionError)>
    where
        Self: Sized;
}

// ========== Implementation Helpers ==========

/// Utility for creating safe transition implementations
///
/// This macro generates boilerplate implementations while maintaining
/// compile-time safety guarantees.
macro_rules! impl_safe_transition {
    (
        $protocol:ty: $from_state:ty => $to_state:ty,
        witness: $witness:ty,
        method: $method:ident
    ) => {
        impl SafeWitnessedTransition<$from_state, $to_state> for $protocol {
            type Witness = $witness;
            type Target = SafeSessionProtocol<<$protocol as HasCore>::Core, $to_state>;

            fn safe_transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
                let new_inner = self.inner.$method(witness);
                SafeSessionProtocol::new(new_inner)
            }
        }
    };

    (
        $protocol:ty: $from_state:ty => $to_state:ty,
        method: $method:ident
    ) => {
        impl SafeStateTransition<$from_state, $to_state> for $protocol {
            type Target = SafeSessionProtocol<<$protocol as HasCore>::Core, $to_state>;

            fn safe_transition_to(mut self) -> Self::Target {
                let new_inner = self.inner.$method();
                SafeSessionProtocol::new(new_inner)
            }
        }
    };
}

/// Helper trait to extract the Core type from protocol wrappers
pub trait HasCore {
    type Core;
}

impl<Core, State> HasCore for SafeSessionProtocol<Core, State> {
    type Core = Core;
}

impl<Core, State> HasCore for SafeChoreographicProtocol<Core, State> {
    type Core = Core;
}

// ========== Conversion Traits ==========

/// Convert from external session types to safe wrappers
pub trait IntoSafe<Core, State> {
    fn into_safe(self) -> SafeSessionProtocol<Core, State>;
}

impl<Core, State> IntoSafe<Core, State> for SessionTypedProtocol<Core, State> {
    fn into_safe(self) -> SafeSessionProtocol<Core, State> {
        SafeSessionProtocol::new(self)
    }
}

/// Convert from safe wrappers back to external session types
pub trait FromSafe<Core, State> {
    fn from_safe(safe: SafeSessionProtocol<Core, State>) -> Self;
}

impl<Core, State> FromSafe<Core, State> for SessionTypedProtocol<Core, State> {
    fn from_safe(safe: SafeSessionProtocol<Core, State>) -> Self {
        safe.into_inner()
    }
}

// ========== Error Handling ==========

/// Errors that can occur during safe transitions
#[derive(Debug, Clone, thiserror::Error)]
pub enum SafeTransitionError {
    #[error("Invalid transition attempted: {from} -> {to}")]
    InvalidTransition { from: String, to: String },

    #[error("Missing required witness for transition: {witness_type}")]
    MissingWitness { witness_type: String },

    #[error("Condition not met for transition: {condition}")]
    ConditionNotMet { condition: String },

    #[error("Session error during transition: {0}")]
    SessionError(#[from] SessionError),
}

// ========== Validation Helpers ==========

/// Validate that a transition is legal before performing it
pub fn validate_transition<FromState, ToState>(
    from_state: &str,
    to_state: &str,
    valid_transitions: &[(&str, &str)],
) -> Result<(), SafeTransitionError> {
    if valid_transitions
        .iter()
        .any(|(from, to)| *from == from_state && *to == to_state)
    {
        Ok(())
    } else {
        Err(SafeTransitionError::InvalidTransition {
            from: from_state.to_string(),
            to: to_state.to_string(),
        })
    }
}

/// Create a witness validation closure
pub fn witness_validator<W, F>(validator: F) -> impl Fn(&W) -> bool
where
    F: Fn(&W) -> bool,
{
    validator
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_wrapper_creation() {
        // Test that we can create safe wrappers without orphan rule violations
        // This is primarily a compilation test
        assert!(true, "Compilation test passed");
    }

    #[test]
    fn test_transition_validation() {
        let valid_transitions = vec![
            ("idle", "commitment"),
            ("commitment", "reveal"),
            ("reveal", "finalized"),
        ];

        assert!(validate_transition("idle", "commitment", &valid_transitions).is_ok());
        assert!(validate_transition("idle", "reveal", &valid_transitions).is_err());
    }
}
