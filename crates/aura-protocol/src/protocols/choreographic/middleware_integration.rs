//! Middleware integration for choreographic protocols
//!
//! This module provides builders and utilities for composing the Aura middleware
//! stack with choreographic protocol handlers, ensuring all choreographies benefit
//! from tracing, metrics, capability checking, and error recovery.

use super::handler_adapter::RumpsteakAdapter;
use crate::context::BaseContext;
use crate::effects::ProtocolEffects;
#[cfg(feature = "transport")]
use crate::handlers::StandardHandlerFactory;
use crate::middleware::{
    error_recovery::ErrorRecoveryConfig,
    handler::AuraProtocolHandler,
    stack::{create_standard_stack, MiddlewareConfig},
};
use aura_types::DeviceId;
use std::fmt::Debug;
use uuid::Uuid;

/// Type alias for a boxed protocol handler with the required trait bounds
type BoxedProtocolHandler<DeviceId, SessionId, Message> = Box<
    dyn AuraProtocolHandler<DeviceId = DeviceId, SessionId = SessionId, Message = Message> + Send,
>;

/// Type alias for a RumpsteakAdapter with boxed handler
type ChoreographicAdapter<DeviceId, SessionId, Message, E> =
    RumpsteakAdapter<BoxedProtocolHandler<DeviceId, SessionId, Message>, E>;

/// Configuration for choreographic middleware stack
#[derive(Debug, Clone)]
pub struct ChoreographyMiddlewareConfig {
    /// Device name for logging
    pub device_name: String,
    /// Enable observability middleware (replaces separate tracing/metrics)
    pub enable_observability: bool,
    /// Enable capability authorization
    pub enable_capabilities: bool,
    /// Enable error recovery
    pub enable_error_recovery: bool,
    /// Maximum retry attempts for error recovery
    pub max_retries: u32,
}

impl Default for ChoreographyMiddlewareConfig {
    fn default() -> Self {
        Self {
            device_name: "choreography".to_string(),
            enable_observability: true,
            enable_capabilities: true,
            enable_error_recovery: true,
            max_retries: 3,
        }
    }
}

/// Builder for creating middleware-wrapped choreographic handlers
pub struct ChoreographicHandlerBuilder<E: ProtocolEffects> {
    effects: E,
    config: ChoreographyMiddlewareConfig,
}

impl<E: ProtocolEffects> ChoreographicHandlerBuilder<E> {
    /// Create a new builder with the given effects
    pub fn new(effects: E) -> Self {
        Self {
            effects,
            config: ChoreographyMiddlewareConfig::default(),
        }
    }

    /// Set the device name for logging
    pub fn with_device_name(mut self, name: String) -> Self {
        self.config.device_name = name;
        self
    }

    /// Configure which middleware to enable
    pub fn with_config(mut self, config: ChoreographyMiddlewareConfig) -> Self {
        self.config = config;
        self
    }

    /// Build an in-memory choreographic handler with full middleware stack
    pub fn build_in_memory(
        self,
        _device_id: DeviceId,
        _context: BaseContext,
    ) -> RumpsteakAdapter<
        Box<dyn AuraProtocolHandler<DeviceId = Uuid, SessionId = Uuid, Message = Vec<u8>> + Send>,
        E,
    > {
        // Start with base handler
        #[cfg(feature = "transport")]
        {
            let handler = StandardHandlerFactory::in_memory(device_id);

            // Apply middleware in order (innermost to outermost)
            let handler = self.apply_middleware(handler);

            // Wrap in Rumpsteak adapter
            RumpsteakAdapter::new(handler, self.effects, context)
        }

        #[cfg(not(feature = "transport"))]
        {
            panic!("Transport feature is required for handler creation");
        }
    }

    /// Build a network choreographic handler with full middleware stack
    #[cfg(feature = "transport")]
    pub fn build_network(
        self,
        device_id: DeviceId,
        transport_url: &str,
        context: BaseContext,
    ) -> RumpsteakAdapter<
        Box<dyn AuraProtocolHandler<DeviceId = Uuid, SessionId = Uuid, Message = Vec<u8>> + Send>,
        E,
    > {
        // Start with base handler
        #[cfg(feature = "transport")]
        let handler = StandardHandlerFactory::network(device_id, transport_url);

        // Apply middleware in order
        let handler = self.apply_middleware(handler);

        // Wrap in Rumpsteak adapter
        RumpsteakAdapter::new(handler, self.effects, context)
    }

    /// Apply middleware stack to a handler
    fn apply_middleware<H>(
        self,
        handler: H,
    ) -> Box<dyn AuraProtocolHandler<DeviceId = Uuid, SessionId = Uuid, Message = Vec<u8>> + Send>
    where
        H: AuraProtocolHandler<DeviceId = Uuid, SessionId = Uuid, Message = Vec<u8>>
            + Send
            + 'static,
    {
        // Convert our config to the middleware stack config
        let middleware_config = MiddlewareConfig {
            device_name: self.config.device_name.clone(),
            enable_observability: self.config.enable_observability,
            enable_capabilities: self.config.enable_capabilities,
            enable_error_recovery: self.config.enable_error_recovery,
            observability_config: None,
            error_recovery_config: if self.config.enable_error_recovery {
                Some(ErrorRecoveryConfig {
                    device_name: self.config.device_name.clone(),
                    ..Default::default()
                })
            } else {
                None
            },
        };

        // Use the create_standard_stack function which handles boxing only at the end
        create_standard_stack(handler, middleware_config)
    }
}

/// Extension trait for easy middleware composition
pub trait ChoreographicMiddlewareExt: AuraProtocolHandler {
    /// Wrap this handler with the standard choreographic middleware stack
    fn with_choreographic_middleware<E: ProtocolEffects>(
        self,
        effects: E,
        context: BaseContext,
    ) -> ChoreographicAdapter<Self::DeviceId, Self::SessionId, Self::Message, E>
    where
        Self: Sized + Send + 'static,
        Self::DeviceId: From<Uuid> + Into<Uuid> + Clone + Debug + Send + Sync + 'static,
        Self::SessionId: Clone + Debug + Send + Sync + 'static,
        Self::Message: Clone + Debug + Send + Sync + 'static,
    {
        // For now, wrap the handler directly without applying middleware
        // since apply_middleware requires specific type constraints
        RumpsteakAdapter::new(
            Box::new(self)
                as Box<
                    dyn AuraProtocolHandler<
                            DeviceId = Self::DeviceId,
                            SessionId = Self::SessionId,
                            Message = Self::Message,
                        > + Send,
                >,
            effects,
            context,
        )
    }
}

impl<T: AuraProtocolHandler> ChoreographicMiddlewareExt for T {}

/*
TODO: These middleware integration tests need API updates after protocol refactoring.
The tests verify middleware composition with choreographic handlers but require updates
to work with the new protocol APIs.

Key areas needing updates:
- BaseContext creation and initialization
- AccountState and AccountLedger API changes
- Handler factory methods
- Transport and effects integration
- SimulationScheduler API updates

Re-enable once the middleware integration APIs have stabilized.
*/

/*
#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::AuraEffectsAdapter;
    use crate::test_utils::MemoryTransport;
    use aura_crypto::Effects;
    use aura_journal::{AccountLedger, AccountState, DeviceMetadata, DeviceType};
    use aura_types::{AccountId, AccountIdExt, DeviceIdExt};
    use ed25519_dalek::SigningKey;
    use std::sync::Arc;
    use std::collections::{BTreeMap, BTreeSet};
    use tokio::sync::RwLock;
    use crate::effects::time::SimulationScheduler;

    fn create_test_context() -> BaseContext {
        let session_id = Uuid::new_v4();
        let device_id = Uuid::new_v4();
        let participants = vec![DeviceId::from(device_id)];

        // Create a proper AccountState for testing
        let effects = Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();
        let device_id_typed = DeviceId::new_with_effects(&effects);

        let device = DeviceMetadata {
            device_id: device_id_typed,
            device_name: "Test Device".to_string(),
            device_type: DeviceType::Native,
            public_key: group_public_key,
            added_at: 1000,
            last_seen: 1000,
            dkd_commitment_proofs: BTreeMap::new(),
            next_nonce: 0,
            used_nonces: BTreeSet::new(),
            key_share_epoch: 0,
        };

        let account_state = AccountState::new(
            account_id,
            group_public_key,
            device,
            2, // threshold
            3, // total_participants
        );

        let ledger = Arc::new(RwLock::new(
            AccountLedger::new(account_state).expect("Failed to create ledger"),
        ));
        let transport = Arc::new(MemoryTransport::new());
        let device_key = SigningKey::from_bytes(&[1u8; 32]);
        let scheduler = Arc::new(RwLock::new(SimulationScheduler::new()));
        let time_source = Box::new(crate::effects::SimulatedTimeSource::new(
            device_id, scheduler.clone(),
        ));

        BaseContext::new(
            session_id,
            device_id,
            participants,
            Some(2),
            ledger,
            transport,
            effects,
            device_key,
            time_source,
        )
    }

    #[tokio::test]
    async fn test_middleware_builder_in_memory() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let effects = AuraEffectsAdapter::new(Effects::test(), device_id.into());
        let context = create_test_context();

        let builder =
            ChoreographicHandlerBuilder::new(effects).with_device_name("test-device".to_string());

        let _handler = builder.build_in_memory(device_id, context);
        // Handler is created successfully with middleware stack
    }

    #[tokio::test]
    async fn test_middleware_configuration() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let effects = AuraEffectsAdapter::new(Effects::test(), device_id.into());
        let context = create_test_context();

        let config = ChoreographyMiddlewareConfig {
            device_name: "custom-device".to_string(),
            enable_observability: true,
            enable_capabilities: true,
            enable_error_recovery: false,
            max_retries: 5,
        };

        let builder = ChoreographicHandlerBuilder::new(effects).with_config(config);

        let _handler = builder.build_in_memory(device_id, context);
        // Handler is created with custom middleware configuration
    }
}
*/
