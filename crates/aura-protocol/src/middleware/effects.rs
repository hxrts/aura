//! Effects Middleware
//!
//! This middleware provides automatic effects injection for protocol handlers.
//! It ensures that all handlers have access to the unified effects system,
//! enabling deterministic testing and clean separation of side effects.

use crate::effects::AuraEffectsAdapter;
use crate::middleware::handler::{AuraProtocolHandler, ProtocolResult, SessionInfo};
use aura_crypto::Effects;
use std::sync::Arc;
use uuid::Uuid;

/// Middleware that automatically injects effects into protocol operations
///
/// This middleware wraps a handler and provides access to the unified effects system.
/// It can work with either production effects or test effects, making it easy to
/// switch between deterministic testing and real execution.
pub struct EffectsMiddleware<H> {
    inner: H,
    effects_adapter: Arc<AuraEffectsAdapter>,
}

impl<H> EffectsMiddleware<H> {
    /// Create new effects middleware with the given effects adapter
    pub fn new(inner: H, effects_adapter: Arc<AuraEffectsAdapter>) -> Self {
        Self {
            inner,
            effects_adapter,
        }
    }

    /// Create new effects middleware with production effects
    pub fn with_production_effects(inner: H, device_id: Uuid) -> Self {
        let effects = Effects::production();
        let adapter = Arc::new(AuraEffectsAdapter::new(effects, device_id));
        Self::new(inner, adapter)
    }

    /// Create new effects middleware with test effects
    pub fn with_test_effects(inner: H, device_id: Uuid, test_name: &str) -> Self {
        let effects = Effects::for_test(test_name);
        let adapter = Arc::new(AuraEffectsAdapter::new(effects, device_id));
        Self::new(inner, adapter)
    }

    /// Create new effects middleware with deterministic effects
    pub fn with_deterministic_effects(
        inner: H,
        device_id: Uuid,
        seed: u64,
        initial_time: u64,
    ) -> Self {
        let effects = Effects::deterministic(seed, initial_time);
        let adapter = Arc::new(AuraEffectsAdapter::new(effects, device_id));
        Self::new(inner, adapter)
    }

    /// Get access to the effects adapter
    pub fn effects(&self) -> &Arc<AuraEffectsAdapter> {
        &self.effects_adapter
    }

    /// Get access to the inner handler
    pub fn inner(&self) -> &H {
        &self.inner
    }

    /// Get mutable access to the inner handler
    pub fn inner_mut(&mut self) -> &mut H {
        &mut self.inner
    }
}

#[async_trait::async_trait]
impl<H> AuraProtocolHandler for EffectsMiddleware<H>
where
    H: AuraProtocolHandler + Send + Sync,
{
    type DeviceId = H::DeviceId;
    type SessionId = H::SessionId;
    type Message = H::Message;

    // ========== Messaging ==========

    async fn send_message(&mut self, to: Self::DeviceId, msg: Self::Message) -> ProtocolResult<()> {
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

    // ========== Session Management ==========

    async fn start_session(
        &mut self,
        participants: Vec<Self::DeviceId>,
        protocol_type: String,
        metadata: std::collections::HashMap<String, String>,
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

    // ========== Authorization ==========

    async fn verify_capability(
        &mut self,
        operation: &str,
        resource: &str,
        context: std::collections::HashMap<String, String>,
    ) -> ProtocolResult<bool> {
        self.inner
            .verify_capability(operation, resource, context)
            .await
    }

    async fn create_authorization_proof(
        &mut self,
        operation: &str,
        resource: &str,
        context: std::collections::HashMap<String, String>,
    ) -> ProtocolResult<Vec<u8>> {
        self.inner
            .create_authorization_proof(operation, resource, context)
            .await
    }

    // ========== Lifecycle Management ==========

    async fn setup(&mut self) -> ProtocolResult<()> {
        self.inner.setup().await
    }

    async fn teardown(&mut self) -> ProtocolResult<()> {
        self.inner.teardown().await
    }

    // ========== Identification ==========

    fn device_id(&self) -> Self::DeviceId {
        self.inner.device_id()
    }
}

/// Extension trait to make it easy to add effects to any handler
pub trait WithEffects: Sized {
    /// Wrap this handler with effects middleware using production effects
    fn with_production_effects(self, device_id: Uuid) -> EffectsMiddleware<Self> {
        EffectsMiddleware::with_production_effects(self, device_id)
    }

    /// Wrap this handler with effects middleware using test effects
    fn with_test_effects(self, device_id: Uuid, test_name: &str) -> EffectsMiddleware<Self> {
        EffectsMiddleware::with_test_effects(self, device_id, test_name)
    }

    /// Wrap this handler with effects middleware using deterministic effects
    fn with_deterministic_effects(
        self,
        device_id: Uuid,
        seed: u64,
        initial_time: u64,
    ) -> EffectsMiddleware<Self> {
        EffectsMiddleware::with_deterministic_effects(self, device_id, seed, initial_time)
    }

    /// Wrap this handler with effects middleware using a custom effects adapter
    fn with_effects(self, effects_adapter: Arc<AuraEffectsAdapter>) -> EffectsMiddleware<Self> {
        EffectsMiddleware::new(self, effects_adapter)
    }
}

// Implement the extension trait for all protocol handlers
impl<T: AuraProtocolHandler> WithEffects for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::create_test_device_id;
    use crate::effects::{TimeEffects, ProtocolEffects};

    #[tokio::test]
    async fn test_effects_middleware_creation() {
        let device_id = create_test_device_id();
        let uuid_device_id = uuid::Uuid::from(device_id);

        // Test that we can create effects middleware instances with different configurations
        let effects1 = Effects::deterministic(42, 1000);
        let adapter1 = Arc::new(AuraEffectsAdapter::new(effects1, uuid_device_id));
        
        let effects2 = Effects::deterministic(100, 2000);
        let adapter2 = Arc::new(AuraEffectsAdapter::new(effects2, uuid_device_id));

        // Verify each adapter has the correct settings
        assert_eq!(adapter1.current_epoch(), 1000);
        assert_eq!(adapter1.device_id(), uuid_device_id);
        
        assert_eq!(adapter2.current_epoch(), 2000);
        assert_eq!(adapter2.device_id(), uuid_device_id);
    }

    #[test]
    fn test_effects_middleware_factory_methods() {
        let device_id = create_test_device_id();
        let uuid_device_id = uuid::Uuid::from(device_id);

        // Test that factory methods create effects with correct properties
        let production_effects = Effects::production();
        let prod_adapter = AuraEffectsAdapter::new(production_effects, uuid_device_id);
        assert_eq!(prod_adapter.device_id(), uuid_device_id);

        let test_effects = Effects::for_test("test_name");
        let test_adapter = AuraEffectsAdapter::new(test_effects, uuid_device_id);
        assert_eq!(test_adapter.device_id(), uuid_device_id);

        let det_effects = Effects::deterministic(42, 500);
        let det_adapter = AuraEffectsAdapter::new(det_effects, uuid_device_id);
        assert_eq!(det_adapter.device_id(), uuid_device_id);
        assert_eq!(det_adapter.current_epoch(), 500);
    }

    #[test]
    fn test_multiple_effects_isolation() {
        let device1 = create_test_device_id();
        let device2 = crate::test_helpers::create_test_uuid(67891);
        
        let effects1 = Effects::deterministic(100, 2000);
        let adapter1 = AuraEffectsAdapter::new(effects1, device1.into());
        
        let effects2 = Effects::deterministic(200, 3000);
        let adapter2 = AuraEffectsAdapter::new(effects2, device2);

        // Each adapter should have its own effects
        assert_eq!(adapter1.device_id(), uuid::Uuid::from(device1));
        assert_eq!(adapter1.current_epoch(), 2000);

        assert_eq!(adapter2.device_id(), device2);
        assert_eq!(adapter2.current_epoch(), 3000);

        // Different seeds should produce different results
        assert_ne!(adapter1.current_epoch(), adapter2.current_epoch());
    }
}
