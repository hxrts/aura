use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::effects::network::PeerEventStream;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{NetworkCoreEffects, NetworkError, NetworkExtendedEffects};
use aura_protocol::amp::deserialize_amp_message;
use std::collections::HashMap;

// Implementation of NetworkEffects
#[async_trait]
impl NetworkCoreEffects for AuraEffectSystem {
    async fn send_to_peer(
        &self,
        _peer_id: uuid::Uuid,
        _message: Vec<u8>,
    ) -> Result<(), NetworkError> {
        self.ensure_mock_network()?;
        Ok(())
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        self.ensure_mock_network()?;
        let Some(shared) = self.transport.shared_transport() else {
            return Err(NetworkError::BroadcastFailed {
                reason: "shared transport not configured".to_string(),
            });
        };

        let wire =
            deserialize_amp_message(&message).map_err(|e| NetworkError::SerializationFailed {
                error: e.to_string(),
            })?;

        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            super::AMP_CONTENT_TYPE.to_string(),
        );

        let source = self.authority_id;
        let context = wire.header.context;

        for peer in shared.online_peers() {
            if peer == source {
                continue;
            }

            let envelope = TransportEnvelope {
                destination: peer,
                source,
                context,
                payload: message.clone(),
                metadata: metadata.clone(),
                receipt: None,
            };

            shared.route_envelope(envelope);
        }

        Ok(())
    }

    async fn receive(&self) -> Result<(uuid::Uuid, Vec<u8>), NetworkError> {
        self.ensure_mock_network()?;
        Err(NetworkError::NoMessage)
    }
}

#[async_trait]
impl NetworkExtendedEffects for AuraEffectSystem {
    async fn receive_from(&self, _peer_id: uuid::Uuid) -> Result<Vec<u8>, NetworkError> {
        self.ensure_mock_network()?;
        Err(NetworkError::NoMessage)
    }

    async fn connected_peers(&self) -> Vec<uuid::Uuid> {
        if self.execution_mode.is_production() {
            tracing::error!("NetworkEffects::connected_peers not implemented for production");
        }
        vec![]
    }

    async fn is_peer_connected(&self, _peer_id: uuid::Uuid) -> bool {
        if self.execution_mode.is_production() {
            tracing::error!("NetworkEffects::is_peer_connected not implemented for production");
        }
        false
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        self.ensure_mock_network()?;
        Err(NetworkError::NotImplemented)
    }

    async fn open(&self, _address: &str) -> Result<String, NetworkError> {
        self.ensure_mock_network()?;
        Err(NetworkError::NotImplemented)
    }

    async fn send(&self, _connection_id: &str, _data: Vec<u8>) -> Result<(), NetworkError> {
        self.ensure_mock_network()?;
        Err(NetworkError::NotImplemented)
    }

    async fn close(&self, _connection_id: &str) -> Result<(), NetworkError> {
        self.ensure_mock_network()?;
        Ok(())
    }
}
