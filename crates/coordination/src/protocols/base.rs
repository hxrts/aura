//! Base Protocol Implementation
//!
//! This module provides a base implementation that reduces boilerplate
//! across all protocol implementations.

use crate::execution::{
    EventAwaiter, EventBuilder, EventTypePattern, Instruction, InstructionResult, ProtocolContext,
    ProtocolError, ProtocolErrorType,
};
use aura_journal::{
    Event, EventType, OperationType, ParticipantId, ProtocolType, Session, SessionId,
};
use aura_types::DeviceId;
use std::collections::BTreeSet;
use uuid::Uuid;

/// Base protocol implementation with common functionality
pub struct ProtocolBase<'a> {
    pub ctx: &'a mut ProtocolContext,
    pub protocol_type: ProtocolType,
    pub operation_type: OperationType,
    pub ttl_epochs: u64,
}

impl<'a> ProtocolBase<'a> {
    /// Create a new protocol base
    pub fn new(
        ctx: &'a mut ProtocolContext,
        protocol_type: ProtocolType,
        operation_type: OperationType,
        ttl_epochs: u64,
    ) -> Self {
        Self {
            ctx,
            protocol_type,
            operation_type,
            ttl_epochs,
        }
    }

    /// Create a session for this protocol
    pub async fn create_session(&mut self) -> Result<Session, ProtocolError> {
        let participants = self
            .ctx
            .participants()
            .iter()
            .map(|device_id| ParticipantId::Device(*device_id))
            .collect();

        let current_epoch = self.get_current_epoch().await?;
        let timestamp = self.ctx.effects().now().map_err(|e| ProtocolError {
            session_id: self.ctx.session_id(),
            error_type: ProtocolErrorType::Other,
            message: format!("Failed to get timestamp: {:?}", e),
        })?;

        Ok(Session::new(
            SessionId(self.ctx.session_id()),
            self.protocol_type,
            participants,
            current_epoch,
            self.ttl_epochs,
            timestamp,
        ))
    }

    /// Get current epoch
    pub async fn get_current_epoch(&mut self) -> Result<u64, ProtocolError> {
        match self.ctx.execute(Instruction::GetCurrentEpoch).await? {
            InstructionResult::CurrentEpoch(epoch) => Ok(epoch),
            _ => Err(self.error(ProtocolErrorType::Other, "Failed to get current epoch")),
        }
    }

    /// Emit an event
    pub async fn emit_event(&mut self, event_type: EventType) -> Result<(), ProtocolError> {
        let _event = EventBuilder::new(self.ctx)
            .with_type(event_type)
            .with_device_auth()
            .build_sign_and_emit()
            .await?;
        Ok(())
    }

    /// Await threshold events
    pub async fn await_threshold(
        &mut self,
        event_patterns: Vec<EventTypePattern>,
        threshold: usize,
        timeout_epochs: u64,
    ) -> Result<Vec<Event>, ProtocolError> {
        let session_id = self.ctx.session_id();
        EventAwaiter::new(self.ctx)
            .for_session(session_id)
            .for_event_types(event_patterns)
            .await_threshold(threshold, timeout_epochs)
            .await
    }

    /// Await events from specific authors
    pub async fn await_from_authors(
        &mut self,
        event_patterns: Vec<EventTypePattern>,
        authors: BTreeSet<DeviceId>,
        timeout_epochs: u64,
    ) -> Result<Vec<Event>, ProtocolError> {
        let session_id = self.ctx.session_id();
        EventAwaiter::new(self.ctx)
            .for_session(session_id)
            .for_event_types(event_patterns)
            .from_authors(authors.clone())
            .await_threshold(authors.len(), timeout_epochs)
            .await
    }

    /// Await single event
    pub async fn await_single(
        &mut self,
        event_patterns: Vec<EventTypePattern>,
        timeout_epochs: u64,
    ) -> Result<Event, ProtocolError> {
        let session_id = self.ctx.session_id();
        EventAwaiter::new(self.ctx)
            .for_session(session_id)
            .for_event_types(event_patterns)
            .await_single(timeout_epochs)
            .await
    }

    /// Create a protocol error with context
    pub fn error(&self, error_type: ProtocolErrorType, message: &str) -> ProtocolError {
        ProtocolError {
            session_id: self.ctx.session_id(),
            error_type,
            message: message.to_string(),
        }
    }

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        DeviceId(self.ctx.device_id())
    }

    /// Get session ID
    pub fn session_id(&self) -> Uuid {
        self.ctx.session_id()
    }

    /// Get participants
    pub fn participants(&self) -> &[DeviceId] {
        self.ctx.participants()
    }

    /// Get threshold
    pub fn threshold(&self) -> Option<usize> {
        self.ctx.threshold()
    }

    /// Extract device IDs from events
    pub fn extract_device_ids(&self, events: &[Event]) -> BTreeSet<DeviceId> {
        events
            .iter()
            .filter_map(|e| match &e.authorization {
                aura_journal::EventAuthorization::DeviceCertificate { device_id, .. } => {
                    Some(*device_id)
                }
                _ => None,
            })
            .collect()
    }
}

/// Macro to simplify protocol phase implementation
#[macro_export]
macro_rules! protocol_phase {
    ($name:ident, $input:ty, $output:ty, $body:block) => {
        pub async fn $name(&mut self, input: $input) -> Result<$output, ProtocolError> {
            $body
        }
    };
}

/// Macro to simplify event emission
#[macro_export]
macro_rules! emit_event {
    ($base:expr, $event_type:expr) => {
        $base.emit_event($event_type).await?
    };
}

/// Macro to simplify event awaiting
#[macro_export]
macro_rules! await_events {
    ($base:expr, $patterns:expr, $threshold:expr, $timeout:expr) => {
        $base
            .await_threshold($patterns, $threshold, $timeout)
            .await?
    };
}

#[cfg(test)]
mod tests {
    // Add specific imports as needed for tests

    // Test that protocol base can be created
    #[test]
    fn test_protocol_base_creation() {
        // This would require a mock ProtocolContext
        // Just verify the struct compiles correctly
    }
}
