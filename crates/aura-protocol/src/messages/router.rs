//! Context-based message routing for privacy partition enforcement
//!
//! This module implements message routing that enforces the privacy partition invariant:
//! messages from different contexts cannot flow into each other without explicit bridge protocols.

use async_trait::async_trait;
use aura_core::{
    AuthStrength, MessageContext, MessageValidation, MessageValidator, SemanticVersion,
    TypedMessage,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur during message routing
#[derive(Debug, Error, Clone, PartialEq)]
pub enum RoutingError {
    /// Message context not registered
    #[error("Context not registered: {context}")]
    ContextNotRegistered { context: MessageContext },

    /// Message validation failed
    #[error("Message validation failed: {reason}")]
    ValidationFailed { reason: String },

    /// No route available for context
    #[error("No route available for context: {context}")]
    NoRouteAvailable { context: MessageContext },

    /// Context isolation violation
    #[error(
        "Context isolation violation: message from {from_context} cannot route to {to_context}"
    )]
    ContextIsolationViolation {
        from_context: MessageContext,
        to_context: MessageContext,
    },
}

/// Message handler trait for processing messages within a context
#[async_trait]
pub trait MessageHandler: Send + Sync {
    /// Process a message payload
    async fn handle_message(&self, message: &TypedMessage<Vec<u8>>) -> Result<(), RoutingError>;

    /// Get the context this handler processes
    fn context(&self) -> &MessageContext;

    /// Check if this handler can process messages with the given authentication strength
    fn accepts_auth_strength(&self, auth_strength: AuthStrength) -> bool;
}

/// Context configuration for message routing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// The message context
    pub context: MessageContext,
    /// Local protocol version
    pub local_version: SemanticVersion,
    /// Minimum required authentication strength
    pub min_auth_strength: AuthStrength,
    /// Whether this context accepts external messages
    pub accepts_external: bool,
}

impl ContextConfig {
    /// Create a new context configuration
    pub fn new(
        context: MessageContext,
        local_version: SemanticVersion,
        min_auth_strength: AuthStrength,
    ) -> Self {
        Self {
            context,
            local_version,
            min_auth_strength,
            accepts_external: true,
        }
    }

    /// Create a context configuration that only accepts internal messages
    pub fn internal_only(
        context: MessageContext,
        local_version: SemanticVersion,
        min_auth_strength: AuthStrength,
    ) -> Self {
        Self {
            context,
            local_version,
            min_auth_strength,
            accepts_external: false,
        }
    }

    /// Get a validator for this context
    pub fn validator(&self) -> MessageValidator {
        MessageValidator::new(
            self.context.clone(),
            self.local_version,
            self.min_auth_strength,
        )
    }
}

/// Message router that enforces context isolation and privacy partitions
pub struct MessageRouter {
    /// Registered context configurations
    contexts: HashMap<[u8; 32], ContextConfig>,
    /// Message handlers by context hash
    handlers: HashMap<[u8; 32], Box<dyn MessageHandler>>,
    /// Default protocol version
    default_version: SemanticVersion,
}

impl MessageRouter {
    /// Create a new message router
    pub fn new(default_version: SemanticVersion) -> Self {
        Self {
            contexts: HashMap::new(),
            handlers: HashMap::new(),
            default_version,
        }
    }

    /// Register a context configuration
    pub fn register_context(&mut self, config: ContextConfig) -> Result<(), RoutingError> {
        let context_hash = config.context.context_hash();
        self.contexts.insert(context_hash, config);
        Ok(())
    }

    /// Register a message handler for a context
    pub fn register_handler(
        &mut self,
        handler: Box<dyn MessageHandler>,
    ) -> Result<(), RoutingError> {
        let context_hash = handler.context().context_hash();

        // Ensure context is registered
        if !self.contexts.contains_key(&context_hash) {
            return Err(RoutingError::ContextNotRegistered {
                context: handler.context().clone(),
            });
        }

        self.handlers.insert(context_hash, handler);
        Ok(())
    }

    /// Route a message to the appropriate handler
    pub async fn route_message<P>(&self, message: &TypedMessage<P>) -> Result<(), RoutingError>
    where
        P: Clone + Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static,
    {
        let context_hash = message.context.context_hash();

        // Get context configuration
        let config =
            self.contexts
                .get(&context_hash)
                .ok_or_else(|| RoutingError::ContextNotRegistered {
                    context: message.context.clone(),
                })?;

        // Validate message against context policy
        let validator = config.validator();
        let validation = validator.validate(message);

        match validation {
            MessageValidation::Valid => {}
            MessageValidation::ContextMismatch { expected, actual } => {
                return Err(RoutingError::ContextIsolationViolation {
                    from_context: actual,
                    to_context: expected,
                });
            }
            other => {
                return Err(RoutingError::ValidationFailed {
                    reason: format!("{:?}", other),
                });
            }
        }

        // Get handler
        let handler =
            self.handlers
                .get(&context_hash)
                .ok_or_else(|| RoutingError::NoRouteAvailable {
                    context: message.context.clone(),
                })?;

        // Check authentication strength compatibility
        if !handler.accepts_auth_strength(message.auth.auth_strength()) {
            return Err(RoutingError::ValidationFailed {
                reason: format!(
                    "Handler does not accept auth strength: {:?}",
                    message.auth.auth_strength()
                ),
            });
        }

        // Serialize message payload for handler
        let payload_bytes =
            bincode::serialize(&message.payload).map_err(|e| RoutingError::ValidationFailed {
                reason: format!("Failed to serialize payload: {}", e),
            })?;

        let serialized_message = TypedMessage::new(
            message.context.clone(),
            payload_bytes,
            message.version,
            message.auth.clone(),
        );

        // Route to handler
        handler.handle_message(&serialized_message).await
    }

    /// Get all registered contexts
    pub fn registered_contexts(&self) -> Vec<&MessageContext> {
        self.contexts
            .values()
            .map(|config| &config.context)
            .collect()
    }

    /// Check if a context is registered
    pub fn has_context(&self, context: &MessageContext) -> bool {
        let context_hash = context.context_hash();
        self.contexts.contains_key(&context_hash)
    }

    /// Get context configuration
    pub fn get_context_config(&self, context: &MessageContext) -> Option<&ContextConfig> {
        let context_hash = context.context_hash();
        self.contexts.get(&context_hash)
    }

    /// Check if two contexts can communicate (same context only)
    pub fn can_communicate(&self, from: &MessageContext, to: &MessageContext) -> bool {
        // Enforce context isolation: only same contexts can communicate
        from.is_compatible_with(to)
    }

    /// Create a bridge between contexts (explicit cross-context communication)
    ///
    /// This would be used for explicit bridge protocols that allow controlled
    /// cross-context message flow. TODO fix - For now, returns an error to enforce isolation.
    pub fn create_bridge(
        &mut self,
        _from: &MessageContext,
        _to: &MessageContext,
    ) -> Result<(), RoutingError> {
        // Bridge protocols not implemented yet - enforce strict isolation
        Err(RoutingError::ValidationFailed {
            reason: "Bridge protocols not implemented - strict context isolation enforced"
                .to_string(),
        })
    }
}

/// Context-aware message dispatcher
pub struct MessageDispatcher {
    /// The message router
    router: MessageRouter,
}

impl MessageDispatcher {
    /// Create a new message dispatcher
    pub fn new(default_version: SemanticVersion) -> Self {
        Self {
            router: MessageRouter::new(default_version),
        }
    }

    /// Register a context and handler
    pub fn register_context_handler(
        &mut self,
        config: ContextConfig,
        handler: Box<dyn MessageHandler>,
    ) -> Result<(), RoutingError> {
        self.router.register_context(config)?;
        self.router.register_handler(handler)?;
        Ok(())
    }

    /// Send a message (routes internally based on context)
    pub async fn send_message<P>(&self, message: TypedMessage<P>) -> Result<(), RoutingError>
    where
        P: Clone + Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static,
    {
        // Route through the internal router
        self.router.route_message(&message).await
    }

    /// Check if dispatcher can handle a context
    pub fn can_handle(&self, context: &MessageContext) -> bool {
        self.router.has_context(context)
    }

    /// Get router reference for advanced operations
    pub fn router(&self) -> &MessageRouter {
        &self.router
    }
}

/// Example message handler implementation
pub struct EchoHandler {
    context: MessageContext,
    accepted_auth_strengths: Vec<AuthStrength>,
}

impl EchoHandler {
    /// Create a new echo handler
    pub fn new(context: MessageContext) -> Self {
        Self {
            context,
            accepted_auth_strengths: vec![
                AuthStrength::Unauthenticated,
                AuthStrength::MacAuthenticated,
                AuthStrength::AeadAuthenticated,
                AuthStrength::ThresholdSignature,
            ],
        }
    }

    /// Create an echo handler that only accepts authenticated messages
    pub fn authenticated_only(context: MessageContext) -> Self {
        Self {
            context,
            accepted_auth_strengths: vec![
                AuthStrength::MacAuthenticated,
                AuthStrength::AeadAuthenticated,
                AuthStrength::ThresholdSignature,
            ],
        }
    }
}

#[async_trait]
impl MessageHandler for EchoHandler {
    async fn handle_message(&self, message: &TypedMessage<Vec<u8>>) -> Result<(), RoutingError> {
        // Echo handler just logs the message (TODO fix - In a real implementation, this would
        // forward the message or process it appropriately)
        println!(
            "EchoHandler[{}]: Received message with {} bytes, version {}, auth {}",
            message.context,
            message.payload.len(),
            message.version,
            message.auth
        );
        Ok(())
    }

    fn context(&self) -> &MessageContext {
        &self.context
    }

    fn accepts_auth_strength(&self, auth_strength: AuthStrength) -> bool {
        self.accepted_auth_strengths.contains(&auth_strength)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AuthTag, DeviceId};

    #[tokio::test]
    async fn test_message_routing_basic() {
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();
        let context = MessageContext::relay_between(&device1, &device2);
        let version = SemanticVersion::new(1, 0, 0);

        let mut dispatcher = MessageDispatcher::new(version);

        // Register context and handler
        let config = ContextConfig::new(context.clone(), version, AuthStrength::Unauthenticated);
        let handler = Box::new(EchoHandler::new(context.clone()));

        dispatcher
            .register_context_handler(config, handler)
            .unwrap();

        // Send a message
        let message =
            TypedMessage::new(context, "test payload".to_string(), version, AuthTag::None);

        let result = dispatcher.send_message(message).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_context_isolation() {
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();
        let device3 = DeviceId::new();

        let context1 = MessageContext::relay_between(&device1, &device2);
        let context2 = MessageContext::relay_between(&device2, &device3);
        let version = SemanticVersion::new(1, 0, 0);

        let mut dispatcher = MessageDispatcher::new(version);

        // Register only context1
        let config1 = ContextConfig::new(context1.clone(), version, AuthStrength::Unauthenticated);
        let handler1 = Box::new(EchoHandler::new(context1.clone()));
        dispatcher
            .register_context_handler(config1, handler1)
            .unwrap();

        // Try to send message to context2 (not registered)
        let message = TypedMessage::new(
            context2, // Wrong context
            "test payload".to_string(),
            version,
            AuthTag::None,
        );

        let result = dispatcher.send_message(message).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RoutingError::ContextNotRegistered { .. }
        ));
    }

    #[tokio::test]
    async fn test_authentication_requirement() {
        let context = MessageContext::dkd_context("test", [0u8; 32]);
        let version = SemanticVersion::new(1, 0, 0);

        let mut dispatcher = MessageDispatcher::new(version);

        // Register context that requires threshold signatures
        let config = ContextConfig::new(context.clone(), version, AuthStrength::ThresholdSignature);
        let handler = Box::new(EchoHandler::authenticated_only(context.clone()));
        dispatcher
            .register_context_handler(config, handler)
            .unwrap();

        // Try to send unauthenticated message
        let message = TypedMessage::new(
            context.clone(),
            "test payload".to_string(),
            version,
            AuthTag::None, // Insufficient auth
        );

        let result = dispatcher.send_message(message).await;
        assert!(result.is_err());

        // Send properly authenticated message
        let auth = AuthTag::threshold_signature(vec![1, 2, 3], 3, 5);
        let auth_message = TypedMessage::new(context, "test payload".to_string(), version, auth);

        let result = dispatcher.send_message(auth_message).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_context_compatibility() {
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();

        let context1 = MessageContext::relay_between(&device1, &device2);
        let context2 = MessageContext::relay_between(&device1, &device2); // Same
        let context3 = MessageContext::dkd_context("app", [1u8; 32]);

        assert!(context1.is_compatible_with(&context2));
        assert!(!context1.is_compatible_with(&context3));
    }
}
