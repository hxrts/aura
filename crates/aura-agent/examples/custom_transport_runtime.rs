//! Custom Transport Runtime Example
//!
//! This example demonstrates how to create an Aura agent with a custom
//! transport handler. This is useful when you need to support non-standard
//! network transports like Bluetooth Low Energy, custom protocols, or
//! specialized network infrastructure.
//!
//! # Running
//!
//! ```bash
//! cargo run --package aura-agent --example custom_transport_runtime
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use aura_agent::{AgentBuilder, EffectContext, ExecutionMode};
use aura_core::effects::{TransportEffects, TransportEnvelope, TransportError, TransportStats};
use aura_core::hash::hash;
use aura_core::{AuthorityId, ContextId};
use aura_effects::{
    FilesystemStorageHandler, PhysicalTimeHandler, RealConsoleHandler, RealCryptoHandler,
    RealRandomHandler, RealTransportHandler,
};

/// Example custom transport handler.
///
/// This demonstrates how to implement a custom transport for specialized
/// network requirements. In a real implementation, this might be:
/// - Bluetooth Low Energy for mobile device communication
/// - A custom UDP-based protocol for low-latency messaging
/// - An overlay network transport for privacy
/// - A relay-based transport for NAT traversal
#[derive(Debug)]
struct LoggingTransportWrapper {
    /// Underlying transport to delegate to
    inner: RealTransportHandler,
    /// Name for logging
    name: String,
}

impl LoggingTransportWrapper {
    fn new(name: impl Into<String>) -> Self {
        Self {
            inner: RealTransportHandler::new(),
            name: name.into(),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl TransportEffects for LoggingTransportWrapper {
    async fn send_envelope(&self, envelope: TransportEnvelope) -> Result<(), TransportError> {
        tracing::info!(
            transport = %self.name,
            source = ?envelope.source,
            destination = ?envelope.destination,
            context = ?envelope.context,
            size = envelope.payload.len(),
            "Custom transport: sending envelope"
        );
        self.inner.send_envelope(envelope).await
    }

    async fn receive_envelope(&self) -> Result<TransportEnvelope, TransportError> {
        let result = self.inner.receive_envelope().await;
        match &result {
            Ok(envelope) => {
                tracing::info!(
                    transport = %self.name,
                    source = ?envelope.source,
                    size = envelope.payload.len(),
                    "Custom transport: received envelope"
                );
            }
            Err(TransportError::NoMessage) => {
                tracing::debug!(transport = %self.name, "Custom transport: no message available");
            }
            Err(e) => {
                tracing::warn!(transport = %self.name, error = ?e, "Custom transport: receive error");
            }
        }
        result
    }

    async fn receive_envelope_from(
        &self,
        source: AuthorityId,
        context: ContextId,
    ) -> Result<TransportEnvelope, TransportError> {
        tracing::debug!(
            transport = %self.name,
            ?source,
            ?context,
            "Custom transport: receiving from specific source"
        );
        self.inner.receive_envelope_from(source, context).await
    }

    async fn is_channel_established(&self, context: ContextId, peer: AuthorityId) -> bool {
        let established = self.inner.is_channel_established(context, peer).await;
        tracing::debug!(
            transport = %self.name,
            ?context,
            ?peer,
            established,
            "Custom transport: channel status check"
        );
        established
    }

    async fn get_transport_stats(&self) -> TransportStats {
        self.inner.get_transport_stats().await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for visibility
    tracing_subscriber::fmt()
        .with_env_filter("info,aura_agent::task_registry=error")
        .init();

    println!("Creating Aura agent with custom transport handlers...\n");

    // Create the standard effect handlers
    let crypto = Arc::new(RealCryptoHandler::new());
    let storage = Arc::new(FilesystemStorageHandler::new(
        std::env::temp_dir().join("aura-transport-example"),
    ));
    let time = Arc::new(PhysicalTimeHandler);
    let random = Arc::new(RealRandomHandler);
    let console = Arc::new(RealConsoleHandler);
    let authority_id = AuthorityId::new_from_entropy([0xA1; 32]);

    // Create custom transport handlers
    // Multiple transports can be added for different network protocols
    let primary_transport: Arc<dyn TransportEffects> =
        Arc::new(LoggingTransportWrapper::new("primary-tcp"));
    let backup_transport: Arc<dyn TransportEffects> =
        Arc::new(LoggingTransportWrapper::new("backup-relay"));

    // Build the agent with custom transports
    let agent = AgentBuilder::custom()
        .with_crypto(crypto)
        .with_storage(storage)
        .with_time(time)
        .with_random(random)
        .with_console(console)
        // Add multiple transport handlers for different network protocols
        .with_transport(primary_transport)
        .with_transport(backup_transport)
        .authority(authority_id)
        .testing_mode()
        .build()
        .await?;

    println!("Agent created with custom transports!");
    println!("Authority ID: {:?}", agent.authority_id());

    // The agent now supports multiple transport protocols:
    // - Primary TCP transport with logging
    // - Backup relay transport with logging
    //
    // In a real application, you might have:
    // - A direct TCP transport for local network communication
    // - A relay transport for NAT traversal
    // - A BLE transport for nearby device discovery
    let context_id = ContextId::new_from_entropy(hash(&authority_id.to_bytes()));
    let shutdown_ctx = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);
    agent.shutdown(&shutdown_ctx).await?;

    Ok(())
}
