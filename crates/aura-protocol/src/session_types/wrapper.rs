//! Session-typed protocol wrapper for compile-time state machine safety
//!
//! This module provides the `SessionTypedProtocol` wrapper that enables
//! the typestate pattern for protocols, ensuring compile-time safety
//! for state transitions.

use std::marker::PhantomData;
use uuid::Uuid;

/// A session-typed protocol wrapper for typestate pattern
///
/// This wrapper uses phantom types to track protocol state at compile time,
/// preventing invalid state transitions and ensuring type-safe protocol execution.
///
/// # Type Parameters
/// - `Core`: The protocol-specific core data
/// - `State`: Phantom type representing the current protocol state
#[derive(Debug, Clone)]
pub struct SessionTypedProtocol<Core, State> {
    /// The protocol-specific core data
    pub inner: Core,
    /// The current state (phantom type for compile-time safety)
    _state: PhantomData<State>,
}

impl<Core, State> SessionTypedProtocol<Core, State> {
    /// Create a new session-typed protocol instance
    pub fn new(inner: Core) -> Self {
        Self {
            inner,
            _state: PhantomData,
        }
    }

    /// Get reference to the inner core
    pub fn core(&self) -> &Core {
        &self.inner
    }

    /// Get mutable reference to the inner core
    pub fn core_mut(&mut self) -> &mut Core {
        &mut self.inner
    }

    /// Extract the inner core, consuming self
    pub fn into_core(self) -> Core {
        self.inner
    }

    /// Transition to a new state (type-safe state change)
    ///
    /// This allows transitioning between protocol states while maintaining
    /// compile-time type safety.
    pub fn transition_to<NewState>(self) -> SessionTypedProtocol<Core, NewState> {
        SessionTypedProtocol::new(self.inner)
    }
}

/// Protocol trait providing common protocol operations
///
/// This trait defines the interface that all protocols must implement,
/// allowing uniform handling of different protocol types.
pub trait SessionProtocol: Send + Sync + std::fmt::Debug {
    /// Get the protocol's session ID
    fn session_id(&self) -> Uuid;

    /// Get the current state name
    fn state_name(&self) -> &'static str;

    /// Check if the protocol is in a final state
    fn is_final(&self) -> bool;

    /// Check if the protocol can terminate
    fn can_terminate(&self) -> bool;

    /// Get the protocol ID (may differ from session ID for some protocols)
    fn protocol_id(&self) -> Uuid;

    /// Get the device ID for this protocol instance
    fn device_id(&self) -> Uuid;
}
