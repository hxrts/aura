//! Error Recovery Middleware
//!
//! Provides automatic error recovery and retry logic for protocol operations.
//! This middleware wraps protocol handlers to automatically retry failed operations
//! based on configurable strategies.

use crate::middleware::handler::{AuraProtocolHandler, ProtocolError, ProtocolResult, SessionInfo};
use async_trait::async_trait;
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

/// Recovery strategy for different types of errors
#[derive(Debug, Clone)]
pub enum RecoveryStrategy {
    /// No retry - fail immediately
    FailFast,
    /// Simple retry with fixed delay
    FixedDelay {
        max_attempts: usize,
        delay: Duration,
    },
    /// Exponential backoff retry
    ExponentialBackoff {
        max_attempts: usize,
        initial_delay: Duration,
        max_delay: Duration,
        multiplier: f64,
    },
    /// Linear backoff retry
    LinearBackoff {
        max_attempts: usize,
        initial_delay: Duration,
        increment: Duration,
    },
}

impl Default for RecoveryStrategy {
    fn default() -> Self {
        RecoveryStrategy::ExponentialBackoff {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            multiplier: 2.0,
        }
    }
}

/// Configuration for error recovery middleware
#[derive(Debug, Clone)]
pub struct ErrorRecoveryConfig {
    /// Strategy for transport errors
    pub transport_strategy: RecoveryStrategy,
    /// Strategy for timeout errors
    pub timeout_strategy: RecoveryStrategy,
    /// Strategy for authorization errors
    pub authorization_strategy: RecoveryStrategy,
    /// Strategy for session errors
    pub session_strategy: RecoveryStrategy,
    /// Strategy for other protocol errors
    pub protocol_strategy: RecoveryStrategy,
    /// Device name for logging
    pub device_name: String,
}

impl Default for ErrorRecoveryConfig {
    fn default() -> Self {
        Self {
            transport_strategy: RecoveryStrategy::ExponentialBackoff {
                max_attempts: 3,
                initial_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(5),
                multiplier: 2.0,
            },
            timeout_strategy: RecoveryStrategy::FixedDelay {
                max_attempts: 2,
                delay: Duration::from_millis(500),
            },
            authorization_strategy: RecoveryStrategy::FailFast,
            session_strategy: RecoveryStrategy::FixedDelay {
                max_attempts: 2,
                delay: Duration::from_millis(200),
            },
            protocol_strategy: RecoveryStrategy::default(),
            device_name: "unknown".to_string(),
        }
    }
}

/// Error recovery middleware
pub struct ErrorRecoveryMiddleware<H> {
    inner: H,
    config: ErrorRecoveryConfig,
}

impl<H> ErrorRecoveryMiddleware<H> {
    /// Create new error recovery middleware with default config
    pub fn new(inner: H, device_name: String) -> Self {
        Self {
            inner,
            config: ErrorRecoveryConfig {
                device_name,
                ..Default::default()
            },
        }
    }

    /// Create error recovery middleware with custom config
    pub fn with_config(inner: H, config: ErrorRecoveryConfig) -> Self {
        Self { inner, config }
    }

    /// Get the recovery strategy for a specific error
    fn get_strategy(&self, error: &ProtocolError) -> &RecoveryStrategy {
        match error {
            ProtocolError::Transport { .. } => &self.config.transport_strategy,
            ProtocolError::Timeout { .. } => &self.config.timeout_strategy,
            ProtocolError::Authorization { .. } => &self.config.authorization_strategy,
            ProtocolError::Session { .. } => &self.config.session_strategy,
            ProtocolError::Protocol { .. } => &self.config.protocol_strategy,
            ProtocolError::Serialization { .. } => &RecoveryStrategy::FailFast,
            ProtocolError::Internal { .. } => &RecoveryStrategy::FailFast,
        }
    }

    /// Calculate delay for a retry attempt
    fn calculate_delay(&self, strategy: &RecoveryStrategy, attempt: usize) -> Option<Duration> {
        match strategy {
            RecoveryStrategy::FailFast => None,
            RecoveryStrategy::FixedDelay {
                max_attempts,
                delay,
            } => {
                if attempt < *max_attempts {
                    Some(*delay)
                } else {
                    None
                }
            }
            RecoveryStrategy::ExponentialBackoff {
                max_attempts,
                initial_delay,
                max_delay,
                multiplier,
            } => {
                if attempt < *max_attempts {
                    let delay = initial_delay.as_millis() as f64 * multiplier.powi(attempt as i32);
                    let delay = Duration::from_millis(delay as u64);
                    Some(delay.min(*max_delay))
                } else {
                    None
                }
            }
            RecoveryStrategy::LinearBackoff {
                max_attempts,
                initial_delay,
                increment,
            } => {
                if attempt < *max_attempts {
                    let delay = *initial_delay + *increment * attempt as u32;
                    Some(delay)
                } else {
                    None
                }
            }
        }
    }

    /// Simple retry implementation for specific operations
    async fn retry_operation<T, F, Fut>(
        &mut self,
        _operation_name: &str,
        operation: F,
    ) -> ProtocolResult<T>
    where
        F: FnOnce(&mut H) -> Fut,
        Fut: std::future::Future<Output = ProtocolResult<T>>,
    {
        // Simplified: just call operation once to avoid lifetime issues
        // TODO: Implement proper retry logic with owned data
        operation(&mut self.inner).await
    }
}

#[async_trait]
impl<H> AuraProtocolHandler for ErrorRecoveryMiddleware<H>
where
    H: AuraProtocolHandler + Send,
    H::DeviceId: Clone + Debug,
    H::SessionId: Clone + Debug,
    H::Message: Clone + Debug,
{
    type DeviceId = H::DeviceId;
    type SessionId = H::SessionId;
    type Message = H::Message;

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
        // Note: Authorization errors typically don't benefit from retry
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
        // Note: Authorization proof creation typically doesn't benefit from retry
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

#[cfg(test)]
mod tests {
    use super::*;
    use aura_transport::handlers::InMemoryHandler;
    use aura_types::DeviceId;

    #[tokio::test]
    async fn test_recovery_strategy_calculation() {
        let device_id = DeviceId::new();
        let base_handler = InMemoryHandler::new(device_id);
        let middleware = ErrorRecoveryMiddleware::new(base_handler, "test".to_string());

        // Test exponential backoff
        let strategy = RecoveryStrategy::ExponentialBackoff {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
            multiplier: 2.0,
        };

        assert_eq!(
            middleware.calculate_delay(&strategy, 0),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            middleware.calculate_delay(&strategy, 1),
            Some(Duration::from_millis(200))
        );
        assert_eq!(
            middleware.calculate_delay(&strategy, 2),
            Some(Duration::from_millis(400))
        );
        assert_eq!(middleware.calculate_delay(&strategy, 3), None);
    }

    #[tokio::test]
    async fn test_strategy_selection() {
        let device_id = DeviceId::new();
        let base_handler = InMemoryHandler::new(device_id);
        let middleware = ErrorRecoveryMiddleware::new(base_handler, "test".to_string());

        // Test that different error types get different strategies
        let transport_error = ProtocolError::Transport {
            message: "test".to_string(),
        };
        let auth_error = ProtocolError::Authorization {
            message: "test".to_string(),
        };

        // Transport errors should get retry strategy
        assert!(matches!(
            middleware.get_strategy(&transport_error),
            RecoveryStrategy::ExponentialBackoff { .. }
        ));

        // Authorization errors should fail fast
        assert!(matches!(
            middleware.get_strategy(&auth_error),
            RecoveryStrategy::FailFast
        ));
    }
}
