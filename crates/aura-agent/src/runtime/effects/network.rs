use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::effects::network::PeerEventStream;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{NetworkCoreEffects, NetworkError, NetworkExtendedEffects, TransportEffects, TransportError};
use aura_protocol::amp::deserialize_amp_message;
use std::collections::HashMap;
use crate::core::default_context_id_for_authority;
use aura_core::identifiers::AuthorityId;
use std::collections::HashSet;
use tokio::io::AsyncWriteExt;
use std::net::SocketAddr;
use std::str::FromStr;

const NETWORK_CONTENT_TYPE: &str = "application/aura-network";

// Implementation of NetworkEffects
#[async_trait]
impl NetworkCoreEffects for AuraEffectSystem {
    async fn send_to_peer(
        &self,
        peer_id: uuid::Uuid,
        message: Vec<u8>,
    ) -> Result<(), NetworkError> {
        if self.execution_mode.is_deterministic() {
            if let Some(shared) = self.transport.shared_transport() {
                let peer = AuthorityId::from_uuid(peer_id);
                let mut metadata = HashMap::new();
                metadata.insert("content-type".to_string(), NETWORK_CONTENT_TYPE.to_string());
                let envelope = TransportEnvelope {
                    destination: peer,
                    source: self.authority_id,
                    context: default_context_id_for_authority(peer),
                    payload: message,
                    metadata,
                    receipt: None,
                };
                shared.route_envelope(envelope);
                return Ok(());
            }
            self.ensure_mock_network()?;
            return Ok(());
        }

        let peer = AuthorityId::from_uuid(peer_id);
        let mut metadata = HashMap::new();
        metadata.insert("content-type".to_string(), NETWORK_CONTENT_TYPE.to_string());
        let envelope = TransportEnvelope {
            destination: peer,
            source: self.authority_id,
            context: default_context_id_for_authority(peer),
            payload: message,
            metadata,
            receipt: None,
        };

        TransportEffects::send_envelope(self, envelope)
            .await
            .map_err(|e| NetworkError::SendFailed {
                peer_id: Some(peer_id),
                reason: e.to_string(),
            })?;
        Ok(())
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        if self.execution_mode.is_deterministic() {
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

            return Ok(());
        }

        let peers: HashSet<uuid::Uuid> = self.connected_peers().await.into_iter().collect();
        for peer in peers {
            let _ = self.send_to_peer(peer, message.clone()).await;
        }
        Ok(())
    }

    async fn receive(&self) -> Result<(uuid::Uuid, Vec<u8>), NetworkError> {
        let envelope = match TransportEffects::receive_envelope(self).await {
            Ok(env) => env,
            Err(TransportError::NoMessage) => return Err(NetworkError::NoMessage),
            Err(e) => {
                return Err(NetworkError::ReceiveFailed {
                    reason: e.to_string(),
                })
            }
        };

        let Some(content_type) = envelope.metadata.get("content-type") else {
            self.requeue_envelope(envelope);
            return Err(NetworkError::NoMessage);
        };

        if content_type != NETWORK_CONTENT_TYPE {
            self.requeue_envelope(envelope);
            return Err(NetworkError::NoMessage);
        }

        Ok((envelope.source.uuid(), envelope.payload))
    }
}

#[async_trait]
impl NetworkExtendedEffects for AuraEffectSystem {
    async fn receive_from(&self, _peer_id: uuid::Uuid) -> Result<Vec<u8>, NetworkError> {
        let peer_id = _peer_id;
        let envelope = match TransportEffects::receive_envelope(self).await {
            Ok(env) => env,
            Err(TransportError::NoMessage) => return Err(NetworkError::NoMessage),
            Err(e) => {
                return Err(NetworkError::ReceiveFailed {
                    reason: e.to_string(),
                })
            }
        };

        let Some(content_type) = envelope.metadata.get("content-type") else {
            self.requeue_envelope(envelope);
            return Err(NetworkError::NoMessage);
        };

        if content_type != NETWORK_CONTENT_TYPE || envelope.source.uuid() != peer_id {
            self.requeue_envelope(envelope);
            return Err(NetworkError::NoMessage);
        }

        Ok(envelope.payload)
    }

    async fn connected_peers(&self) -> Vec<uuid::Uuid> {
        if let Some(shared) = self.transport.shared_transport() {
            return shared.online_peers().into_iter().map(|peer| peer.uuid()).collect();
        }

        if let Some(manager) = self.rendezvous_manager() {
            return manager
                .list_cached_peers()
                .await
                .into_iter()
                .map(|peer| peer.uuid())
                .collect();
        }

        vec![]
    }

    async fn is_peer_connected(&self, _peer_id: uuid::Uuid) -> bool {
        if let Some(shared) = self.transport.shared_transport() {
            return shared.is_peer_online(AuthorityId::from_uuid(_peer_id));
        }

        if let Some(manager) = self.rendezvous_manager() {
            let peer = AuthorityId::from_uuid(_peer_id);
            let context = default_context_id_for_authority(peer);
            return manager.get_descriptor(context, peer).await.is_some();
        }

        false
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        self.ensure_mock_network()?;
        Err(NetworkError::NotImplemented)
    }

    async fn open(&self, _address: &str) -> Result<String, NetworkError> {
        if self.execution_mode.is_deterministic() {
            self.ensure_mock_network()?;
            return Err(NetworkError::NotImplemented);
        }

        // For now, treat the address as a TCP endpoint and validate connectivity.
        let addr = _address.to_string();
        let socket_addr = SocketAddr::from_str(&addr)
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        let _stream = tokio::net::TcpStream::connect(socket_addr)
            .await
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        Ok(addr)
    }

    async fn send(&self, _connection_id: &str, _data: Vec<u8>) -> Result<(), NetworkError> {
        if self.execution_mode.is_deterministic() {
            self.ensure_mock_network()?;
            return Err(NetworkError::NotImplemented);
        }

        let socket_addr = SocketAddr::from_str(_connection_id)
            .map_err(|e| NetworkError::SendFailed {
                peer_id: None,
                reason: format!("Invalid connection address: {e}"),
            })?;

        let config = aura_effects::transport::TransportConfig::default();
        let mut stream = tokio::time::timeout(config.connect_timeout.get(), tokio::net::TcpStream::connect(socket_addr))
            .await
            .map_err(|_| NetworkError::OperationTimeout {
                operation: "network_send_connect".to_string(),
                timeout_ms: config.connect_timeout.get().as_millis() as u64,
            })?
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        let len = (_data.len() as u32).to_be_bytes();
        tokio::time::timeout(config.write_timeout.get(), stream.write_all(&len))
            .await
            .map_err(|_| NetworkError::OperationTimeout {
                operation: "network_send_len".to_string(),
                timeout_ms: config.write_timeout.get().as_millis() as u64,
            })?
            .map_err(|e| NetworkError::SendFailed {
                peer_id: None,
                reason: e.to_string(),
            })?;
        tokio::time::timeout(config.write_timeout.get(), stream.write_all(&_data))
            .await
            .map_err(|_| NetworkError::OperationTimeout {
                operation: "network_send_payload".to_string(),
                timeout_ms: config.write_timeout.get().as_millis() as u64,
            })?
            .map_err(|e| NetworkError::SendFailed {
                peer_id: None,
                reason: e.to_string(),
            })?;

        Ok(())
    }

    async fn close(&self, _connection_id: &str) -> Result<(), NetworkError> {
        if self.execution_mode.is_deterministic() {
            self.ensure_mock_network()?;
        }
        Ok(())
    }
}
