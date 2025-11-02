//! Fault Injection Middleware
//!
//! Provides fault injection capabilities for testing error handling and resilience.

use crate::middleware::handler::{AuraProtocolHandler, ProtocolError, ProtocolResult, SessionInfo};
use async_trait::async_trait;
use rand::{thread_rng, Rng};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;

/// Fault injection configuration
#[derive(Debug, Clone)]
pub struct FaultConfig {
    /// Probability of injecting a failure (0.0 = never, 1.0 = always)
    pub failure_rate: f64,
    /// Range of random delays to inject
    pub delay_range: (Duration, Duration),
    /// Probability of injecting a delay (0.0 = never, 1.0 = always)
    pub delay_rate: f64,
}

impl Default for FaultConfig {
    fn default() -> Self {
        Self {
            failure_rate: 0.1, // 10% failure rate
            delay_range: (Duration::from_millis(10), Duration::from_millis(100)),
            delay_rate: 0.2, // 20% delay rate
        }
    }
}

/// Fault injection middleware for testing
pub struct FaultInjectionMiddleware<H> {
    inner: H,
    config: FaultConfig,
}

impl<H> FaultInjectionMiddleware<H> {
    /// Create new fault injection middleware
    pub fn new(inner: H) -> Self {
        Self {
            inner,
            config: FaultConfig::default(),
        }
    }

    /// Create fault injection middleware with custom config
    pub fn with_config(inner: H, config: FaultConfig) -> Self {
        Self { inner, config }
    }

    /// Set failure rate
    pub fn with_failure_rate(mut self, rate: f64) -> Self {
        self.config.failure_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Set delay rate and range
    pub fn with_delay(mut self, rate: f64, min: Duration, max: Duration) -> Self {
        self.config.delay_rate = rate.clamp(0.0, 1.0);
        self.config.delay_range = (min, max);
        self
    }

    /// Inject a random delay if configured
    async fn maybe_inject_delay(&self) {
        if thread_rng().gen::<f64>() < self.config.delay_rate {
            let min_ms = self.config.delay_range.0.as_millis() as u64;
            let max_ms = self.config.delay_range.1.as_millis() as u64;
            let delay_ms = thread_rng().gen_range(min_ms..=max_ms);
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    /// Inject a random failure if configured
    fn maybe_inject_failure(&self, operation: &str) -> ProtocolResult<()> {
        if thread_rng().gen::<f64>() < self.config.failure_rate {
            Err(ProtocolError::Transport {
                message: format!("Injected failure in {}", operation),
            })
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl<H> AuraProtocolHandler for FaultInjectionMiddleware<H>
where
    H: AuraProtocolHandler + Send,
{
    type DeviceId = H::DeviceId;
    type SessionId = H::SessionId;
    type Message = H::Message;

    async fn send_message(&mut self, to: Self::DeviceId, msg: Self::Message) -> ProtocolResult<()> {
        self.maybe_inject_failure("send_message")?;
        self.maybe_inject_delay().await;
        self.inner.send_message(to, msg).await
    }

    async fn receive_message(&mut self, from: Self::DeviceId) -> ProtocolResult<Self::Message> {
        self.maybe_inject_failure("receive_message")?;
        self.maybe_inject_delay().await;
        self.inner.receive_message(from).await
    }

    async fn broadcast(
        &mut self,
        recipients: &[Self::DeviceId],
        msg: Self::Message,
    ) -> ProtocolResult<()> {
        self.maybe_inject_failure("broadcast")?;
        self.maybe_inject_delay().await;
        self.inner.broadcast(recipients, msg).await
    }

    async fn parallel_send(
        &mut self,
        sends: &[(Self::DeviceId, Self::Message)],
    ) -> ProtocolResult<()> {
        self.maybe_inject_failure("parallel_send")?;
        self.maybe_inject_delay().await;
        self.inner.parallel_send(sends).await
    }

    async fn start_session(
        &mut self,
        participants: Vec<Self::DeviceId>,
        protocol_type: String,
        metadata: HashMap<String, String>,
    ) -> ProtocolResult<Self::SessionId> {
        self.maybe_inject_failure("start_session")?;
        self.maybe_inject_delay().await;
        self.inner
            .start_session(participants, protocol_type, metadata)
            .await
    }

    async fn end_session(&mut self, session_id: Self::SessionId) -> ProtocolResult<()> {
        self.maybe_inject_delay().await;
        self.inner.end_session(session_id).await
    }

    async fn get_session_info(
        &mut self,
        session_id: Self::SessionId,
    ) -> ProtocolResult<SessionInfo> {
        self.maybe_inject_delay().await;
        self.inner.get_session_info(session_id).await
    }

    async fn list_sessions(&mut self) -> ProtocolResult<Vec<SessionInfo>> {
        self.maybe_inject_delay().await;
        self.inner.list_sessions().await
    }

    async fn verify_capability(
        &mut self,
        operation: &str,
        resource: &str,
        context: HashMap<String, String>,
    ) -> ProtocolResult<bool> {
        self.maybe_inject_delay().await;
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
        self.maybe_inject_delay().await;
        self.inner
            .create_authorization_proof(operation, resource, context)
            .await
    }

    fn device_id(&self) -> Self::DeviceId {
        self.inner.device_id()
    }

    async fn setup(&mut self) -> ProtocolResult<()> {
        self.maybe_inject_delay().await;
        self.inner.setup().await
    }

    async fn teardown(&mut self) -> ProtocolResult<()> {
        self.maybe_inject_delay().await;
        self.inner.teardown().await
    }

    async fn health_check(&mut self) -> ProtocolResult<bool> {
        self.maybe_inject_delay().await;
        self.inner.health_check().await
    }

    async fn is_peer_reachable(&mut self, peer: Self::DeviceId) -> ProtocolResult<bool> {
        self.maybe_inject_delay().await;
        self.inner.is_peer_reachable(peer).await
    }
}
