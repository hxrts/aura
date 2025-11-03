//! Example demonstrating clean middleware composition for choreographic protocols

use aura_choreography::threshold_crypto::{DkdProtocol, FrostSigningProtocol};
use aura_protocol::{
    choreographic::{ChoreographicRole, ChoreographicHandlerBuilder, ChoreographyMiddlewareConfig},
    context::BaseContext,
    effects::AuraEffectsAdapter,
};
use aura_types::{effects::Effects, DeviceId};
use rumpsteak_choreography::ChoreoHandler;
use uuid::Uuid;

/// Example: Create a fully middleware-wrapped DKD choreography handler
pub async fn create_dkd_handler_with_middleware(
    device_id: DeviceId,
    context: BaseContext,
) -> impl ChoreoHandler<Role = ChoreographicRole> {
    // Create effects adapter
    let effects = Effects::test(42);
    let effects_adapter = AuraEffectsAdapter::new(device_id.into(), effects);

    // Configure middleware stack
    let config = ChoreographyMiddlewareConfig {
        device_name: format!("dkd-{}", device_id),
        enable_observability: true,
        enable_capabilities: true,
        enable_error_recovery: true,
        max_retries: 3,
    };

    // Build handler with full middleware stack
    ChoreographicHandlerBuilder::new(effects_adapter)
        .with_config(config)
        .build_in_memory(device_id, context)
}

/// Example: Create a FROST signing handler with custom middleware configuration
pub async fn create_frost_handler_with_custom_middleware(
    device_id: DeviceId,
    context: BaseContext,
) -> impl ChoreoHandler<Role = ChoreographicRole> {
    // Create production effects
    let effects = Effects::production();
    let effects_adapter = AuraEffectsAdapter::new(device_id.into(), effects);

    // Configure only essential middleware for performance
    let config = ChoreographyMiddlewareConfig {
        device_name: format!("frost-{}", device_id),
        enable_tracing: true,      // Keep tracing for debugging
        enable_metrics: false,      // Disable metrics for performance
        enable_capabilities: true,   // Always check capabilities
        enable_error_recovery: true, // Essential for network resilience
        max_retries: 5,             // More retries for critical signing
    };

    // Build handler
    ChoreographicHandlerBuilder::new(effects_adapter)
        .with_config(config)
        .build_in_memory(device_id, context)
}

/// Example: Compose multiple protocols with shared middleware
pub struct ComposedProtocolHandler<H: ChoreoHandler> {
    handler: H,
    protocols_executed: Vec<String>,
}

impl<H: ChoreoHandler<Role = ChoreographicRole>> ComposedProtocolHandler<H> {
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            protocols_executed: Vec::new(),
        }
    }

    /// Execute DKD followed by FROST signing
    pub async fn execute_dkd_then_frost(
        &mut self,
        participants: Vec<ChoreographicRole>,
    ) -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
        // Record protocol execution
        self.protocols_executed.push("DKD".to_string());
        
        // Execute DKD protocol
        // In real implementation, this would use the actual protocol execution
        let derived_key = vec![1, 2, 3, 4]; // Placeholder
        
        // Record protocol execution
        self.protocols_executed.push("FROST".to_string());
        
        // Execute FROST signing protocol
        let signature = vec![5, 6, 7, 8]; // Placeholder
        
        Ok((derived_key, signature))
    }

    /// Get execution history
    pub fn execution_history(&self) -> &[String] {
        &self.protocols_executed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::MemoryTransport;
    use aura_journal::AccountLedger;
    use ed25519_dalek::SigningKey;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn create_test_context(device_id: Uuid) -> BaseContext {
        let session_id = Uuid::new_v4();
        let participants = vec![DeviceId::from(device_id)];
        let ledger = Arc::new(RwLock::new(AccountLedger::new(vec![])));
        let transport = Arc::new(MemoryTransport::new());
        let effects = Effects::test(42);
        let device_key = SigningKey::from_bytes(&[1u8; 32]);
        let time_source = Box::new(crate::effects::SimulatedTimeSource::new());

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
    async fn test_middleware_composition_example() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let context = create_test_context(device_id.into());

        // Create handler with full middleware
        let _handler = create_dkd_handler_with_middleware(device_id, context).await;

        // Handler is ready to execute choreographic protocols with:
        // - Tracing for debugging
        // - Metrics for monitoring  
        // - Capability checking for authorization
        // - Error recovery for resilience
    }

    #[tokio::test]
    async fn test_custom_middleware_configuration() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let context = create_test_context(device_id.into());

        // Create handler with custom middleware
        let _handler = create_frost_handler_with_custom_middleware(device_id, context).await;

        // Handler optimized for performance with only essential middleware
    }

    #[tokio::test]
    async fn test_protocol_composition() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let context = create_test_context(device_id.into());

        // Create base handler with middleware
        let handler = create_dkd_handler_with_middleware(device_id, context).await;

        // Wrap in composition handler
        let mut composed = ComposedProtocolHandler::new(handler);

        // Execute composed protocols
        let participants = vec![
            ChoreographicRole { device_id: device_id.into(), role_index: 0 },
            ChoreographicRole { device_id: Uuid::new_v4(), role_index: 1 },
        ];
        
        let result = composed.execute_dkd_then_frost(participants).await;
        assert!(result.is_ok());

        // Check execution history
        assert_eq!(composed.execution_history(), &["DKD", "FROST"]);
    }
}