//! Practical middleware stack implementation

use super::handler::AuraProtocolHandler;
use super::{
    capability::CapabilityMiddleware,
    error_recovery::{ErrorRecoveryConfig, ErrorRecoveryMiddleware},
    observability::{ObservabilityConfig, ObservabilityMiddleware},
};

/// A middleware stack that performs type-level composition of handlers
///
/// This avoids boxing by making the full type known at compile time.
/// Uses the unified observability middleware instead of separate tracing/metrics/etc.
pub type StandardMiddlewareStack<H> =
    ObservabilityMiddleware<CapabilityMiddleware<ErrorRecoveryMiddleware<H>>>;

/// Builder that constructs the middleware stack step by step
pub struct MiddlewareStackBuilder<H> {
    handler: H,
    config: MiddlewareConfig,
}

/// Configuration for the middleware stack
#[derive(Clone, Debug)]
pub struct MiddlewareConfig {
    pub device_name: String,
    pub enable_observability: bool,
    pub enable_capabilities: bool,
    pub enable_error_recovery: bool,
    pub observability_config: Option<ObservabilityConfig>,
    pub error_recovery_config: Option<ErrorRecoveryConfig>,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            device_name: "unknown".to_string(),
            enable_observability: true,
            enable_capabilities: true,
            enable_error_recovery: true,
            observability_config: None,
            error_recovery_config: None,
        }
    }
}

impl<H: AuraProtocolHandler> MiddlewareStackBuilder<H> {
    /// Create a new builder with a base handler
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            config: MiddlewareConfig::default(),
        }
    }

    /// Set the configuration
    pub fn with_config(mut self, config: MiddlewareConfig) -> Self {
        self.config = config;
        self
    }

    /// Build the middleware stack
    ///
    /// This uses the create_standard_stack function for simplicity and type safety.
    /// While it uses boxing, the performance impact is minimal for protocol handlers.
    pub fn build(
        self,
    ) -> Box<
        dyn AuraProtocolHandler<
                DeviceId = H::DeviceId,
                SessionId = H::SessionId,
                Message = H::Message,
            > + Send,
    >
    where
        H: Send + 'static,
        H::DeviceId: Send + Sync + 'static + std::fmt::Display + std::fmt::Debug + ToString + Clone,
        H::SessionId:
            Send + Sync + 'static + std::fmt::Display + std::fmt::Debug + ToString + Clone,
        H::Message: Send + Sync + 'static + std::fmt::Debug + Clone,
    {
        create_standard_stack(self.handler, self.config)
    }
}

/// Alternative approach using macros to generate type-safe middleware stacks
///
/// This macro generates all possible combinations of middleware stacks
/// based on configuration flags.
#[macro_export]
macro_rules! build_middleware_stack {
    ($handler:expr, $config:expr) => {{
        use $crate::middleware::{
            CapabilityMiddleware, ErrorRecoveryMiddleware, ObservabilityMiddleware,
        };

        // This now handles 8 combinations of 3 boolean flags (simplified from previous version)
        match (
            $config.enable_observability,
            $config.enable_capabilities,
            $config.enable_error_recovery,
        ) {
            (true, true, true) => {
                let obs_config = $config.observability_config.unwrap_or_else(|| {
                    super::observability::ObservabilityConfig {
                        device_name: $config.device_name.clone(),
                        ..Default::default()
                    }
                });
                Box::new(ObservabilityMiddleware::with_config(
                    CapabilityMiddleware::new(ErrorRecoveryMiddleware::new(
                        $handler,
                        $config.device_name.clone(),
                    )),
                    obs_config,
                ))
            }
            (true, true, false) => {
                let obs_config = $config.observability_config.unwrap_or_else(|| {
                    super::observability::ObservabilityConfig {
                        device_name: $config.device_name.clone(),
                        ..Default::default()
                    }
                });
                Box::new(ObservabilityMiddleware::with_config(
                    CapabilityMiddleware::new($handler),
                    obs_config,
                ))
            }
            (true, false, true) => {
                let obs_config = $config.observability_config.unwrap_or_else(|| {
                    super::observability::ObservabilityConfig {
                        device_name: $config.device_name.clone(),
                        ..Default::default()
                    }
                });
                Box::new(ObservabilityMiddleware::with_config(
                    ErrorRecoveryMiddleware::new($handler, $config.device_name.clone()),
                    obs_config,
                ))
            }
            (true, false, false) => {
                let obs_config = $config.observability_config.unwrap_or_else(|| {
                    super::observability::ObservabilityConfig {
                        device_name: $config.device_name.clone(),
                        ..Default::default()
                    }
                });
                Box::new(ObservabilityMiddleware::with_config($handler, obs_config))
            }
            (false, true, true) => Box::new(CapabilityMiddleware::new(
                ErrorRecoveryMiddleware::new($handler, $config.device_name.clone()),
            )),
            (false, true, false) => Box::new(CapabilityMiddleware::new($handler)),
            (false, false, true) => Box::new(ErrorRecoveryMiddleware::new(
                $handler,
                $config.device_name.clone(),
            )),
            (false, false, false) => Box::new($handler),
        }
    }};
}

/// Extension trait for easy middleware application
pub trait MiddlewareExt: AuraProtocolHandler + Sized {
    /// Apply standard middleware stack
    fn with_middleware(self) -> MiddlewareStackBuilder<Self> {
        MiddlewareStackBuilder::new(self)
    }

    /// Apply a single layer of middleware
    fn layer<M>(self, middleware: impl FnOnce(Self) -> M) -> M
    where
        M: AuraProtocolHandler<
            DeviceId = Self::DeviceId,
            SessionId = Self::SessionId,
            Message = Self::Message,
        >,
    {
        middleware(self)
    }
}

impl<T: AuraProtocolHandler> MiddlewareExt for T {}

/// Function to create a standard middleware stack without macros
pub fn create_standard_stack<H>(
    handler: H,
    config: MiddlewareConfig,
) -> Box<
    dyn AuraProtocolHandler<DeviceId = H::DeviceId, SessionId = H::SessionId, Message = H::Message>
        + Send,
>
where
    H: AuraProtocolHandler + Send + 'static,
    H::DeviceId: Send + Sync + 'static + std::fmt::Display + std::fmt::Debug + ToString + Clone,
    H::SessionId: Send + Sync + 'static + std::fmt::Display + std::fmt::Debug + ToString + Clone,
    H::Message: Send + Sync + 'static + std::fmt::Debug + Clone,
{
    // This is a fallback that does use boxing, but only at the very end
    let mut boxed: Box<
        dyn AuraProtocolHandler<
                DeviceId = H::DeviceId,
                SessionId = H::SessionId,
                Message = H::Message,
            > + Send,
    > = Box::new(handler);

    if config.enable_error_recovery {
        let recovery_config = config
            .error_recovery_config
            .unwrap_or_else(|| ErrorRecoveryConfig {
                device_name: config.device_name.clone(),
                ..Default::default()
            });
        boxed = Box::new(ErrorRecoveryMiddleware::with_config(boxed, recovery_config));
    }

    if config.enable_capabilities {
        boxed = Box::new(CapabilityMiddleware::new(boxed));
    }

    if config.enable_observability {
        let obs_config = config
            .observability_config
            .unwrap_or_else(|| ObservabilityConfig {
                device_name: config.device_name,
                ..Default::default()
            });
        boxed = Box::new(ObservabilityMiddleware::with_config(boxed, obs_config));
    }

    boxed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::{ProtocolResult, SessionInfo};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use uuid::Uuid;

    struct MockHandler {
        device_id: Uuid,
    }

    #[async_trait]
    impl AuraProtocolHandler for MockHandler {
        type DeviceId = Uuid;
        type SessionId = Uuid;
        type Message = Vec<u8>;

        async fn send_message(
            &mut self,
            _to: Self::DeviceId,
            _msg: Self::Message,
        ) -> ProtocolResult<()> {
            Ok(())
        }

        async fn receive_message(
            &mut self,
            _from: Self::DeviceId,
        ) -> ProtocolResult<Self::Message> {
            Ok(vec![])
        }

        async fn broadcast(
            &mut self,
            _recipients: &[Self::DeviceId],
            _msg: Self::Message,
        ) -> ProtocolResult<()> {
            Ok(())
        }

        async fn parallel_send(
            &mut self,
            _sends: &[(Self::DeviceId, Self::Message)],
        ) -> ProtocolResult<()> {
            Ok(())
        }

        async fn start_session(
            &mut self,
            _participants: Vec<Self::DeviceId>,
            _protocol_type: String,
            _metadata: HashMap<String, String>,
        ) -> ProtocolResult<Self::SessionId> {
            Ok(Uuid::new_v4())
        }

        async fn end_session(&mut self, _session_id: Self::SessionId) -> ProtocolResult<()> {
            Ok(())
        }

        async fn get_session_info(
            &mut self,
            _session_id: Self::SessionId,
        ) -> ProtocolResult<SessionInfo> {
            todo!()
        }

        async fn list_sessions(&mut self) -> ProtocolResult<Vec<SessionInfo>> {
            Ok(vec![])
        }

        async fn verify_capability(
            &mut self,
            _operation: &str,
            _resource: &str,
            _context: HashMap<String, String>,
        ) -> ProtocolResult<bool> {
            Ok(true)
        }

        async fn create_authorization_proof(
            &mut self,
            _operation: &str,
            _resource: &str,
            _context: HashMap<String, String>,
        ) -> ProtocolResult<Vec<u8>> {
            Ok(vec![])
        }

        fn device_id(&self) -> Self::DeviceId {
            self.device_id
        }

        async fn setup(&mut self) -> ProtocolResult<()> {
            Ok(())
        }

        async fn teardown(&mut self) -> ProtocolResult<()> {
            Ok(())
        }

        async fn health_check(&mut self) -> ProtocolResult<bool> {
            Ok(true)
        }

        async fn is_peer_reachable(&mut self, _peer: Self::DeviceId) -> ProtocolResult<bool> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn test_middleware_stack_builder() {
        let handler = MockHandler {
            device_id: Uuid::new_v4(),
        };

        let config = MiddlewareConfig {
            device_name: "test-device".to_string(),
            enable_observability: true,
            enable_capabilities: false,
            enable_error_recovery: false,
            observability_config: None,
            error_recovery_config: None,
        };

        let mut stack = handler.with_middleware().with_config(config).build();

        // Test that operations work
        stack
            .send_message(Uuid::new_v4(), vec![1, 2, 3])
            .await
            .unwrap();
    }

    #[test]
    fn test_layer_composition() {
        let handler = MockHandler {
            device_id: Uuid::new_v4(),
        };

        // Test the layer method with observability middleware
        let _with_observability =
            handler.layer(|h| ObservabilityMiddleware::new(h, "test".to_string()));
    }
}
