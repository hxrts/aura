use super::AuraEffectSystem;
use crate::core::default_context_id_for_authority;
use async_trait::async_trait;
use aura_core::effects::network::PeerEventStream;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{
    NetworkCoreEffects, NetworkError, NetworkExtendedEffects, RandomExtendedEffects,
    TransportEffects, TransportError,
};
use aura_core::{execute_with_timeout_budget, TimeoutBudget, TimeoutRunError};
use aura_effects::time::PhysicalTimeHandler;
use aura_core::types::identifiers::AuthorityId;
use aura_protocol::amp::deserialize_amp_message;
use cfg_if::cfg_if;
#[cfg(target_arch = "wasm32")]
use futures::SinkExt;
#[cfg(target_arch = "wasm32")]
use gloo_net::websocket::{futures::WebSocket, Message};
use std::collections::HashMap;
use std::collections::HashSet;
#[cfg(target_arch = "wasm32")]
use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::AsyncWriteExt;
const NETWORK_CONTENT_TYPE: &str = "application/aura-network";
const CONNECTION_ID_PREFIX: &str = "conn-";

#[cfg(not(target_arch = "wasm32"))]
async fn execute_network_timeout<F, Fut, T>(
    timeout: std::time::Duration,
    timeout_error: impl Fn() -> NetworkError + Copy,
    f: F,
) -> Result<T, NetworkError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, NetworkError>>,
{
    let time = PhysicalTimeHandler::new();
    let started_at = time.physical_time().await.map_err(|_| timeout_error())?;
    let budget =
        TimeoutBudget::from_start_and_timeout(&started_at, timeout).map_err(|_| timeout_error())?;
    Ok(execute_with_timeout_budget(&time, &budget, f)
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => timeout_error(),
            TimeoutRunError::Operation(error) => error,
        })?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ConnectionId(uuid::Uuid);

impl ConnectionId {
    fn new(id: uuid::Uuid) -> Self {
        Self(id)
    }

    fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }

    fn to_wire(self) -> String {
        format!("{CONNECTION_ID_PREFIX}{}", self.0)
    }

    fn parse_wire(value: &str) -> Result<Self, NetworkError> {
        let raw = value.strip_prefix(CONNECTION_ID_PREFIX).unwrap_or(value);
        let id = uuid::Uuid::parse_str(raw).map_err(|e| {
            NetworkError::ConnectionFailed(format!("invalid connection id `{value}`: {e}"))
        })?;
        Ok(Self(id))
    }
}

// Implementation of NetworkEffects
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
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

            let wire = deserialize_amp_message(&message).map_err(|e| {
                NetworkError::SerializationFailed {
                    error: e.to_string(),
                }
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

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
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
            return shared
                .online_peers()
                .into_iter()
                .map(|peer| peer.uuid())
                .collect();
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

        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let connection_id = ConnectionId::new(self.random_uuid().await);
                let ws_url = normalize_ws_url(_address);

                run_local_ws(move || async move {
                    let ws = WebSocket::open(&ws_url)
                        .map_err(|e| format!("WebSocket open failed ({ws_url}): {e}"))?;
                    ws.close(None, None)
                        .map_err(|e| format!("WebSocket close failed ({ws_url}): {e}"))?;
                    Ok(())
                })
                .await
                .map_err(NetworkError::ConnectionFailed)?;

                self.network_connections
                    .write()
                    .insert(connection_id.as_uuid(), normalize_ws_url(_address));
                Ok(connection_id.to_wire())
            } else {
                let socket_addr = _address
                    .parse::<SocketAddr>()
                    .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
                let _stream = tokio::net::TcpStream::connect(socket_addr)
                    .await
                    .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

                let connection_id = ConnectionId::new(self.random_uuid().await);
                self.network_connections
                    .write()
                    .insert(connection_id.as_uuid(), socket_addr);
                Ok(connection_id.to_wire())
            }
        }
    }

    async fn send(&self, _connection_id: &str, _data: Vec<u8>) -> Result<(), NetworkError> {
        if self.execution_mode.is_deterministic() {
            self.ensure_mock_network()?;
            return Err(NetworkError::NotImplemented);
        }

        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let connection_id = ConnectionId::parse_wire(_connection_id)?;
                let ws_url = self
                    .network_connections
                    .read()
                    .get(&connection_id.as_uuid())
                    .cloned()
                    .ok_or_else(|| NetworkError::SendFailed {
                        peer_id: None,
                        reason: format!("Unknown connection id `{_connection_id}`"),
                    })?;

                run_local_ws(move || async move {
                    let mut ws = WebSocket::open(&ws_url)
                        .map_err(|e| format!("WebSocket open failed ({ws_url}): {e}"))?;
                    ws.send(Message::Bytes(_data))
                        .await
                        .map_err(|e| format!("WebSocket send failed ({ws_url}): {e}"))?;
                    Ok(())
                })
                .await
                .map_err(|reason| NetworkError::SendFailed {
                    peer_id: None,
                    reason,
                })?;
                Ok(())
            } else {
                let connection_id = ConnectionId::parse_wire(_connection_id)?;
                let socket_addr = self
                    .network_connections
                    .read()
                    .get(&connection_id.as_uuid())
                    .copied()
                    .ok_or_else(|| NetworkError::SendFailed {
                        peer_id: None,
                        reason: format!("Unknown connection id `{_connection_id}`"),
                    })?;

                let config = aura_effects::transport::TransportConfig::default();
                let mut stream = execute_network_timeout(
                    config.connect_timeout.get(),
                    || NetworkError::OperationTimeout {
                        operation: "network_send_connect".to_string(),
                        timeout_ms: config.connect_timeout.get().as_millis() as u64,
                    },
                    || async {
                        tokio::net::TcpStream::connect(socket_addr)
                            .await
                            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))
                    },
                )
                .await?;

                let len = (_data.len() as u32).to_be_bytes();
                execute_network_timeout(
                    config.write_timeout.get(),
                    || NetworkError::OperationTimeout {
                        operation: "network_send_len".to_string(),
                        timeout_ms: config.write_timeout.get().as_millis() as u64,
                    },
                    || async {
                        stream.write_all(&len).await.map_err(|e| NetworkError::SendFailed {
                            peer_id: None,
                            reason: e.to_string(),
                        })
                    },
                )
                .await?;
                execute_network_timeout(
                    config.write_timeout.get(),
                    || NetworkError::OperationTimeout {
                        operation: "network_send_payload".to_string(),
                        timeout_ms: config.write_timeout.get().as_millis() as u64,
                    },
                    || async {
                        stream.write_all(&_data).await.map_err(|e| NetworkError::SendFailed {
                            peer_id: None,
                            reason: e.to_string(),
                        })
                    },
                )
                .await?;

                Ok(())
            }
        }
    }

    async fn close(&self, _connection_id: &str) -> Result<(), NetworkError> {
        if self.execution_mode.is_deterministic() {
            self.ensure_mock_network()?;
        }
        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let connection_id = ConnectionId::parse_wire(_connection_id)?;
                let removed = self
                    .network_connections
                    .write()
                    .remove(&connection_id.as_uuid());
                if removed.is_none() {
                    return Err(NetworkError::ConnectionFailed(format!(
                        "unknown connection id `{_connection_id}`"
                    )));
                }
            } else {
                let connection_id = ConnectionId::parse_wire(_connection_id)?;
                let removed = self
                    .network_connections
                    .write()
                    .remove(&connection_id.as_uuid());
                if removed.is_none() {
                    return Err(NetworkError::ConnectionFailed(format!(
                        "unknown connection id `{_connection_id}`"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
fn normalize_ws_url(address: &str) -> String {
    if address.starts_with("ws://") || address.starts_with("wss://") {
        address.to_string()
    } else {
        format!("ws://{address}")
    }
}

#[cfg(target_arch = "wasm32")]
async fn run_local_ws<Mk, Fut>(make_fut: Mk) -> Result<(), String>
where
    Mk: FnOnce() -> Fut + 'static,
    Fut: Future<Output = Result<(), String>> + 'static,
{
    make_fut().await
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use tokio::io::AsyncReadExt;

    fn production_config_for_tests() -> AgentConfig {
        let mut config = AgentConfig::default();
        let path =
            std::env::temp_dir().join(format!("aura-agent-network-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&path);
        config.storage.base_path = path;
        config
    }

    #[test]
    fn connection_id_round_trip() {
        let original = ConnectionId::new(uuid::Uuid::from_u128(
            0x1234_5678_9abc_def0_1234_5678_9abc_def0,
        ));
        let wire = original.to_wire();
        let parsed = ConnectionId::parse_wire(&wire).expect("parse connection id");
        assert_eq!(parsed, original);
    }

    #[tokio::test]
    async fn send_rejects_invalid_connection_handle() {
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
        let effects = AuraEffectSystem::production(production_config_for_tests(), authority_id)
            .expect("create production effects");
        let err = effects
            .send("not-a-connection-id", vec![1, 2, 3])
            .await
            .expect_err("invalid connection handle should fail");
        assert!(matches!(err, NetworkError::ConnectionFailed(_)));
    }

    #[tokio::test]
    async fn typed_connection_lifecycle_open_send_close() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");

        let receiver = tokio::spawn(async move {
            // open() does a connectivity check, consuming one accept.
            let (_warmup, _) = listener.accept().await.expect("accept warmup");
            let (mut stream, _) = listener.accept().await.expect("accept payload");
            let mut len = [0u8; 4];
            stream.read_exact(&mut len).await.expect("read length");
            let payload_len = u32::from_be_bytes(len) as usize;
            let mut payload = vec![0u8; payload_len];
            stream.read_exact(&mut payload).await.expect("read payload");
            payload
        });

        let authority_id = AuthorityId::new_from_entropy([2u8; 32]);
        let effects = AuraEffectSystem::production(production_config_for_tests(), authority_id)
            .expect("create production effects");
        let connection_id = effects
            .open(&addr.to_string())
            .await
            .expect("open connection");
        assert!(
            connection_id.starts_with(CONNECTION_ID_PREFIX),
            "open should return opaque connection handle"
        );

        let payload = b"typed-handle-payload".to_vec();
        effects
            .send(&connection_id, payload.clone())
            .await
            .expect("send payload");
        effects
            .close(&connection_id)
            .await
            .expect("close connection");

        let received = receiver.await.expect("join receiver");
        assert_eq!(received, payload);

        let close_err = effects
            .close(&connection_id)
            .await
            .expect_err("closing an already closed handle should fail");
        assert!(matches!(close_err, NetworkError::ConnectionFailed(_)));
    }
}
