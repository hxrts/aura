//! Core Protocol Handler Trait
//!
//! Defines the fundamental interface for Aura protocol handlers, providing the foundation
//! for the middleware system. This trait abstracts over different transport mechanisms
//! and allows middleware to compose cross-cutting concerns.

use async_trait::async_trait;
use aura_types::{AuraError, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use uuid::Uuid;

/// Result type for protocol operations
pub type ProtocolResult<T> = Result<T, ProtocolError>;

/// Errors that can occur during protocol execution
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProtocolError {
    #[error("Transport error: {message}")]
    Transport { message: String },

    #[error("Serialization error: {message}")]
    Serialization { message: String },

    #[error("Authorization error: {message}")]
    Authorization { message: String },

    #[error("Session error: {message}")]
    Session { message: String },

    #[error("Timeout error: operation timed out after {duration_ms}ms")]
    Timeout { duration_ms: u64 },

    #[error("Protocol error: {message}")]
    Protocol { message: String },

    #[error("Internal error: {message}")]
    Internal { message: String },
}

impl From<AuraError> for ProtocolError {
    fn from(error: AuraError) -> Self {
        ProtocolError::Protocol {
            message: format!("{:?}", error),
        }
    }
}

/// Session information for active protocol sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: Uuid,
    pub participants: Vec<DeviceId>,
    pub protocol_type: String,
    pub started_at: u64,
    pub metadata: HashMap<String, String>,
}

/// Core protocol handler trait
///
/// This trait defines the interface for handling protocol operations. Middleware
/// implements this trait and wraps other handlers to add cross-cutting functionality.
#[async_trait]
pub trait AuraProtocolHandler: Send + Sync {
    /// Device identifier type
    type DeviceId: Clone + Debug + Send + Sync + 'static;

    /// Session identifier type  
    type SessionId: Clone + Debug + Send + Sync + 'static;

    /// Message type for protocol communication
    type Message: Clone + Debug + Send + Sync + 'static;

    // ========== Core Communication Operations ==========

    /// Send a message to a specific device
    async fn send_message(&mut self, to: Self::DeviceId, msg: Self::Message) -> ProtocolResult<()>;

    /// Receive a message from a specific device
    async fn receive_message(&mut self, from: Self::DeviceId) -> ProtocolResult<Self::Message>;

    /// Broadcast a message to multiple devices
    async fn broadcast(
        &mut self,
        recipients: &[Self::DeviceId],
        msg: Self::Message,
    ) -> ProtocolResult<()> {
        // Default implementation: send to each recipient sequentially
        for recipient in recipients {
            self.send_message(recipient.clone(), msg.clone()).await?;
        }
        Ok(())
    }

    /// Send messages to multiple recipients in parallel
    async fn parallel_send(
        &mut self,
        sends: &[(Self::DeviceId, Self::Message)],
    ) -> ProtocolResult<()> {
        // Default implementation: send sequentially
        // Middleware can override for true parallel behavior
        for (recipient, msg) in sends {
            self.send_message(recipient.clone(), msg.clone()).await?;
        }
        Ok(())
    }

    // ========== Session Management ==========

    /// Start a new protocol session
    async fn start_session(
        &mut self,
        participants: Vec<Self::DeviceId>,
        protocol_type: String,
        metadata: HashMap<String, String>,
    ) -> ProtocolResult<Self::SessionId>;

    /// End an active protocol session
    async fn end_session(&mut self, session_id: Self::SessionId) -> ProtocolResult<()>;

    /// Get information about an active session
    async fn get_session_info(
        &mut self,
        session_id: Self::SessionId,
    ) -> ProtocolResult<SessionInfo>;

    /// List all active sessions
    async fn list_sessions(&mut self) -> ProtocolResult<Vec<SessionInfo>>;

    // ========== Authorization ==========

    /// Verify capability for a specific operation
    async fn verify_capability(
        &mut self,
        operation: &str,
        resource: &str,
        context: HashMap<String, String>,
    ) -> ProtocolResult<bool>;

    /// Create an authorization proof for an operation
    async fn create_authorization_proof(
        &mut self,
        operation: &str,
        resource: &str,
        context: HashMap<String, String>,
    ) -> ProtocolResult<Vec<u8>>;

    // ========== Lifecycle Management ==========

    /// Setup the handler (called before protocol execution)
    async fn setup(&mut self) -> ProtocolResult<()> {
        // Default: no-op
        Ok(())
    }

    /// Teardown the handler (called after protocol execution)
    async fn teardown(&mut self) -> ProtocolResult<()> {
        // Default: no-op
        Ok(())
    }

    /// Health check for the handler
    async fn health_check(&mut self) -> ProtocolResult<bool> {
        // Default: always healthy
        Ok(true)
    }

    // ========== Utility Methods ==========

    /// Get the device ID for this handler
    fn device_id(&self) -> Self::DeviceId;

    /// Check if a peer is reachable
    async fn is_peer_reachable(&mut self, peer: Self::DeviceId) -> ProtocolResult<bool> {
        // Default: assume all peers are reachable
        // Transport-specific implementations can provide real connectivity checks
        let _ = peer;
        Ok(true)
    }
}

/// Extension trait for handler utilities
#[async_trait]
pub trait AuraProtocolHandlerExt: AuraProtocolHandler {
    /// Send a message with timeout
    async fn send_message_with_timeout(
        &mut self,
        to: Self::DeviceId,
        msg: Self::Message,
        timeout_ms: u64,
    ) -> ProtocolResult<()> {
        use tokio::time::{timeout, Duration};

        timeout(
            Duration::from_millis(timeout_ms),
            self.send_message(to, msg),
        )
        .await
        .map_err(|_| ProtocolError::Timeout {
            duration_ms: timeout_ms,
        })?
    }

    /// Receive a message with timeout
    async fn receive_message_with_timeout(
        &mut self,
        from: Self::DeviceId,
        timeout_ms: u64,
    ) -> ProtocolResult<Self::Message> {
        use tokio::time::{timeout, Duration};

        timeout(
            Duration::from_millis(timeout_ms),
            self.receive_message(from),
        )
        .await
        .map_err(|_| ProtocolError::Timeout {
            duration_ms: timeout_ms,
        })?
    }
}

// Automatically implement the extension trait for all handlers
impl<T: AuraProtocolHandler> AuraProtocolHandlerExt for T {}

// Blanket implementation for boxed trait objects
#[async_trait]
impl<DeviceId, SessionId, Message> AuraProtocolHandler
    for Box<
        dyn AuraProtocolHandler<DeviceId = DeviceId, SessionId = SessionId, Message = Message>
            + Send,
    >
where
    DeviceId: Clone + Debug + Send + Sync + 'static,
    SessionId: Clone + Debug + Send + Sync + 'static,
    Message: Clone + Debug + Send + Sync + 'static,
{
    type DeviceId = DeviceId;
    type SessionId = SessionId;
    type Message = Message;

    async fn send_message(&mut self, to: Self::DeviceId, msg: Self::Message) -> ProtocolResult<()> {
        (**self).send_message(to, msg).await
    }

    async fn receive_message(&mut self, from: Self::DeviceId) -> ProtocolResult<Self::Message> {
        (**self).receive_message(from).await
    }

    async fn broadcast(
        &mut self,
        recipients: &[Self::DeviceId],
        msg: Self::Message,
    ) -> ProtocolResult<()> {
        (**self).broadcast(recipients, msg).await
    }

    async fn parallel_send(
        &mut self,
        sends: &[(Self::DeviceId, Self::Message)],
    ) -> ProtocolResult<()> {
        (**self).parallel_send(sends).await
    }

    async fn start_session(
        &mut self,
        participants: Vec<Self::DeviceId>,
        protocol_type: String,
        metadata: HashMap<String, String>,
    ) -> ProtocolResult<Self::SessionId> {
        (**self)
            .start_session(participants, protocol_type, metadata)
            .await
    }

    async fn end_session(&mut self, session_id: Self::SessionId) -> ProtocolResult<()> {
        (**self).end_session(session_id).await
    }

    async fn get_session_info(
        &mut self,
        session_id: Self::SessionId,
    ) -> ProtocolResult<SessionInfo> {
        (**self).get_session_info(session_id).await
    }

    async fn list_sessions(&mut self) -> ProtocolResult<Vec<SessionInfo>> {
        (**self).list_sessions().await
    }

    async fn verify_capability(
        &mut self,
        operation: &str,
        resource: &str,
        context: HashMap<String, String>,
    ) -> ProtocolResult<bool> {
        (**self)
            .verify_capability(operation, resource, context)
            .await
    }

    async fn health_check(&mut self) -> ProtocolResult<bool> {
        (**self).health_check().await
    }

    async fn is_peer_reachable(&mut self, peer: Self::DeviceId) -> ProtocolResult<bool> {
        (**self).is_peer_reachable(peer).await
    }

    async fn create_authorization_proof(
        &mut self,
        operation: &str,
        resource: &str,
        context: HashMap<String, String>,
    ) -> ProtocolResult<Vec<u8>> {
        (**self)
            .create_authorization_proof(operation, resource, context)
            .await
    }

    fn device_id(&self) -> Self::DeviceId {
        (**self).device_id()
    }
}

/// Helper trait for creating typed protocol handlers
pub trait TypedHandler {
    type DeviceId: Clone + Debug + Send + Sync + 'static;
    type SessionId: Clone + Debug + Send + Sync + 'static;
    type Message: Clone + Debug + Send + Sync + 'static;
}

/// Convenience type alias for handlers with Aura types
pub type AuraHandler<H> =
    dyn AuraProtocolHandler<DeviceId = DeviceId, SessionId = Uuid, Message = H>;
