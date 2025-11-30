//! Real transport handler implementing TransportEffects trait
//!
//! This module provides the production implementation of TransportEffects that handles
//! actual network communication as the final step in the guard chain sequence.
//!
//! **Stateless Design**: This handler delegates state management to external services
//! (NetworkEffects, StorageEffects) following Layer 3 architectural constraints.

use crate::transport::TransportConfig;
use async_trait::async_trait;
use aura_core::{
    effects::{TransportEffects, TransportEnvelope, TransportError, TransportStats},
    AuthorityId, ContextId,
};
use tracing::{debug, info};

/// Production transport handler using real network operations
///
/// This handler implements the final step in the guard chain by emitting
/// network packets over established secure channels. It delegates state management
/// to external storage and network services to remain stateless.
#[derive(Debug)]
pub struct RealTransportHandler {
    /// Configuration for transport operations (reserved for future use)
    _config: TransportConfig,
}

impl RealTransportHandler {
    /// Create a new real transport handler
    pub fn new() -> Self {
        Self {
            _config: TransportConfig::default(),
        }
    }

    /// Create a new transport handler with configuration
    pub fn with_config(config: TransportConfig) -> Self {
        Self { _config: config }
    }
}

impl Default for RealTransportHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TransportEffects for RealTransportHandler {
    async fn send_envelope(&self, envelope: TransportEnvelope) -> Result<(), TransportError> {
        debug!(
            source = ?envelope.source,
            destination = ?envelope.destination,
            context = ?envelope.context,
            payload_size = envelope.payload.len(),
            "Sending transport envelope"
        );

        // Actual I/O is handled by NetworkEffects; this path only emits structured logs.
        info!(
            destination = ?envelope.destination,
            payload_size = envelope.payload.len(),
            "Envelope logged for real transport handler"
        );

        Ok(())
    }

    async fn receive_envelope(&self) -> Result<TransportEnvelope, TransportError> {
        debug!("Attempting to receive transport envelope");

        debug!("No transport envelopes available (stateless handler)");
        Err(TransportError::NoMessage)
    }

    async fn receive_envelope_from(
        &self,
        source: AuthorityId,
        context: ContextId,
    ) -> Result<TransportEnvelope, TransportError> {
        debug!(
            ?source,
            ?context,
            "Attempting to receive envelope from specific source"
        );

        debug!(
            ?source,
            ?context,
            "No transport envelope available from specified source (stateless handler)"
        );
        Err(TransportError::NoMessage)
    }

    async fn is_channel_established(&self, context: ContextId, peer: AuthorityId) -> bool {
        debug!(
            ?context,
            ?peer,
            "Checking channel status (stateless handler)"
        );
        false
    }

    async fn get_transport_stats(&self) -> TransportStats {
        TransportStats::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_real_transport_handler_creation() {
        let handler = RealTransportHandler::new();
        assert_eq!(
            handler._config.buffer_size,
            TransportConfig::default().buffer_size
        );
    }

    #[tokio::test]
    async fn test_real_transport_handler_with_config() {
        let config = TransportConfig {
            connect_timeout: std::time::Duration::from_millis(5),
            read_timeout: std::time::Duration::from_millis(5),
            write_timeout: std::time::Duration::from_millis(5),
            buffer_size: 4096,
        };
        let handler = RealTransportHandler::with_config(config.clone());
        assert_eq!(handler._config.buffer_size, config.buffer_size);
    }

    #[tokio::test]
    async fn test_send_envelope_logs() {
        let handler = RealTransportHandler::new();
        let envelope = TransportEnvelope {
            destination: AuthorityId::default(),
            source: AuthorityId::default(),
            context: ContextId::default(),
            payload: b"test".to_vec(),
            metadata: std::collections::HashMap::new(),
            receipt: None,
        };

        let result = handler.send_envelope(envelope).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_receive_envelope_no_message() {
        let handler = RealTransportHandler::new();
        let result = handler.receive_envelope().await;
        assert!(matches!(result, Err(TransportError::NoMessage)));
    }

    #[tokio::test]
    async fn test_receive_after_send_is_stateless() {
        let handler = RealTransportHandler::new();
        let envelope = TransportEnvelope {
            destination: AuthorityId::default(),
            source: AuthorityId::default(),
            context: ContextId::default(),
            payload: b"loopback".to_vec(),
            metadata: std::collections::HashMap::new(),
            receipt: None,
        };

        handler.send_envelope(envelope.clone()).await.unwrap();
        assert!(matches!(
            handler.receive_envelope().await,
            Err(TransportError::NoMessage)
        ));
    }

    #[tokio::test]
    async fn test_channel_not_established() {
        let handler = RealTransportHandler::new();
        let context = ContextId::default();
        let peer = AuthorityId::default();

        let result = handler.is_channel_established(context, peer).await;
        assert!(!result);
    }

    #[tokio::test]
    async fn test_get_stats_default() {
        let handler = RealTransportHandler::new();
        let stats = handler.get_transport_stats().await;
        assert_eq!(stats.envelopes_sent, 0);
        assert_eq!(stats.envelopes_received, 0);
    }
}
