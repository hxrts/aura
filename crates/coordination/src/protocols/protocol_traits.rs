//! Protocol Traits
//!
//! This module provides a protocol trait system that reduces boilerplate
//! and consolidates common patterns across protocol implementations.

use crate::execution::{
    EventAwaiter, EventTypePattern, ProtocolContext, ProtocolContextExt, ProtocolError,
};
use async_trait::async_trait;
use aura_journal::{Event, OperationType, ProtocolType, Session};

/// Protocol trait that combines session lifecycle with protocol execution
#[async_trait]
pub trait Protocol: Send + Sync {
    /// The result type returned by this protocol
    type Result: Clone + Send + Sync;

    /// Get the protocol type
    fn protocol_type(&self) -> ProtocolType;

    /// Get the operation type
    fn operation_type(&self) -> OperationType;

    /// Execute the full protocol lifecycle
    async fn execute(&mut self, ctx: &mut ProtocolContext) -> Result<Self::Result, ProtocolError>;
}

/// Protocol phase trait for modular protocol composition
#[async_trait]
pub trait ProtocolPhase: Send + Sync {
    /// Input type for this phase
    type Input: Clone + Send + Sync;
    /// Output type for this phase
    type Output: Clone + Send + Sync;

    /// Execute this phase of the protocol
    async fn execute_phase(
        &mut self,
        ctx: &mut ProtocolContext,
        input: Self::Input,
    ) -> Result<Self::Output, ProtocolError>;
}

/// Common protocol utilities
pub struct ProtocolUtils;

impl ProtocolUtils {
    /// Create a standard session for a protocol
    pub async fn create_standard_session(
        ctx: &mut ProtocolContext,
        protocol_type: ProtocolType,
        ttl_epochs: u64,
    ) -> Result<Session, ProtocolError> {
        let ledger_context = ctx.fetch_ledger_context().await?;
        let session_participants = ctx
            .participants()
            .iter()
            .map(|&device_id| aura_journal::ParticipantId::Device(device_id))
            .collect();

        Ok(Session::new(
            aura_journal::SessionId(ctx.session_id()),
            protocol_type,
            session_participants,
            ledger_context.epoch,
            ttl_epochs,
            ctx.effects().now().map_err(|e| ProtocolError {
                session_id: ctx.session_id(),
                error_type: crate::execution::ProtocolErrorType::Other,
                message: format!("Failed to get timestamp: {:?}", e),
            })?,
        ))
    }

    /// Await events with standard timeout handling
    pub async fn await_threshold_events(
        ctx: &mut ProtocolContext,
        event_types: Vec<EventTypePattern>,
        threshold: usize,
        timeout_epochs: u64,
    ) -> Result<Vec<Event>, ProtocolError> {
        let session_id = ctx.session_id();
        EventAwaiter::new(ctx)
            .for_session(session_id)
            .for_event_types(event_types)
            .await_threshold(threshold, timeout_epochs)
            .await
    }

    /// Build and emit a standard event
    pub async fn emit_event(
        ctx: &mut ProtocolContext,
        event_type: aura_journal::EventType,
    ) -> Result<(), ProtocolError> {
        use crate::execution::EventBuilder;

        let _event = EventBuilder::new(ctx)
            .with_type(event_type)
            .with_device_auth()
            .build_sign_and_emit()
            .await?;
        Ok(())
    }
}

/// Macro to reduce boilerplate in protocol implementations
#[macro_export]
macro_rules! define_protocol_phases {
    ($protocol:ident, $($phase:ident => $phase_type:ty),*) => {
        impl $protocol {
            $(
                async fn $phase(
                    &mut self,
                    ctx: &mut ProtocolContext,
                ) -> Result<<$phase_type as ProtocolPhase>::Output, ProtocolError> {
                    let phase_impl = <$phase_type>::new(self.config.clone());
                    phase_impl.execute_phase(ctx, self.state.clone()).await
                }
            )*
        }
    };
}

/// Protocol builder for protocol creation
pub struct ProtocolBuilder<T> {
    protocol_type: ProtocolType,
    operation_type: OperationType,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> ProtocolBuilder<T> {
    pub fn new(protocol_type: ProtocolType, operation_type: OperationType) -> Self {
        Self {
            protocol_type,
            operation_type,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Example protocol implementation
    struct ExampleProtocol {
        data: Vec<u8>,
    }

    #[async_trait]
    impl Protocol for ExampleProtocol {
        type Result = Vec<u8>;

        fn protocol_type(&self) -> ProtocolType {
            ProtocolType::Dkd
        }

        fn operation_type(&self) -> OperationType {
            OperationType::Dkd
        }

        async fn execute(
            &mut self,
            ctx: &mut ProtocolContext,
        ) -> Result<Self::Result, ProtocolError> {
            // Create session
            let session =
                ProtocolUtils::create_standard_session(ctx, self.protocol_type(), 50).await?;

            // Execute protocol logic here

            Ok(self.data.clone())
        }
    }
}
