//! Capability Authorization Middleware
//!
//! Provides capability-based authorization for protocol operations.

use crate::middleware::handler::{AuraProtocolHandler, ProtocolResult, SessionInfo};
use async_trait::async_trait;
use std::collections::HashMap;

/// Capability authorization middleware
pub struct CapabilityMiddleware<H> {
    inner: H,
}

impl<H> CapabilityMiddleware<H> {
    /// Create new capability middleware
    pub fn new(inner: H) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<H> AuraProtocolHandler for CapabilityMiddleware<H>
where
    H: AuraProtocolHandler + Send,
{
    type DeviceId = H::DeviceId;
    type SessionId = H::SessionId;
    type Message = H::Message;

    async fn send_message(&mut self, to: Self::DeviceId, msg: Self::Message) -> ProtocolResult<()> {
        // TODO: Implement capability check before sending
        self.inner.send_message(to, msg).await
    }

    async fn receive_message(&mut self, from: Self::DeviceId) -> ProtocolResult<Self::Message> {
        self.inner.receive_message(from).await
    }

    async fn broadcast(
        &mut self,
        recipients: &[Self::DeviceId],
        msg: Self::Message,
    ) -> ProtocolResult<()> {
        self.inner.broadcast(recipients, msg).await
    }

    async fn parallel_send(
        &mut self,
        sends: &[(Self::DeviceId, Self::Message)],
    ) -> ProtocolResult<()> {
        self.inner.parallel_send(sends).await
    }

    async fn start_session(
        &mut self,
        participants: Vec<Self::DeviceId>,
        protocol_type: String,
        metadata: HashMap<String, String>,
    ) -> ProtocolResult<Self::SessionId> {
        self.inner
            .start_session(participants, protocol_type, metadata)
            .await
    }

    async fn end_session(&mut self, session_id: Self::SessionId) -> ProtocolResult<()> {
        self.inner.end_session(session_id).await
    }

    async fn get_session_info(
        &mut self,
        session_id: Self::SessionId,
    ) -> ProtocolResult<SessionInfo> {
        self.inner.get_session_info(session_id).await
    }

    async fn list_sessions(&mut self) -> ProtocolResult<Vec<SessionInfo>> {
        self.inner.list_sessions().await
    }

    async fn verify_capability(
        &mut self,
        operation: &str,
        resource: &str,
        context: HashMap<String, String>,
    ) -> ProtocolResult<bool> {
        self.inner
            .verify_capability(operation, resource, context)
            .await
    }

    async fn create_authorization_proof(
        &mut self,
        operation: &str,
        resource: &str,
        context: HashMap<String, String>,
    ) -> ProtocolResult<Vec<u8>> {
        self.inner
            .create_authorization_proof(operation, resource, context)
            .await
    }

    fn device_id(&self) -> Self::DeviceId {
        self.inner.device_id()
    }

    async fn setup(&mut self) -> ProtocolResult<()> {
        self.inner.setup().await
    }

    async fn teardown(&mut self) -> ProtocolResult<()> {
        self.inner.teardown().await
    }

    async fn health_check(&mut self) -> ProtocolResult<bool> {
        self.inner.health_check().await
    }

    async fn is_peer_reachable(&mut self, peer: Self::DeviceId) -> ProtocolResult<bool> {
        self.inner.is_peer_reachable(peer).await
    }
}
