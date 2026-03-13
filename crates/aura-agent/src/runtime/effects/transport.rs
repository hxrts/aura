use super::AuraEffectSystem;
use crate::core::default_context_id_for_authority;
use async_trait::async_trait;
use aura_core::effects::transport::{TransportEnvelope, TransportStats};
use aura_core::effects::{TransportEffects, TransportError};
use aura_core::{AuthorityId, ContextId};
#[cfg(not(target_arch = "wasm32"))]
use aura_effects::transport::TransportConfig;
use aura_rendezvous::TransportHint;
use cfg_if::cfg_if;
#[cfg(not(target_arch = "wasm32"))]
use futures::SinkExt;
#[cfg(target_arch = "wasm32")]
use futures::SinkExt;
#[cfg(target_arch = "wasm32")]
use gloo_net::websocket::{futures::WebSocket, Message};
#[cfg(target_arch = "wasm32")]
use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::AsyncWriteExt;
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::TcpStream;
#[cfg(not(target_arch = "wasm32"))]
use tokio::time::timeout;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::{connect_async, tungstenite::Message as TungsteniteMessage};
// Implementation of TransportEffects
#[async_trait]
impl TransportEffects for AuraEffectSystem {
    async fn send_envelope(&self, envelope: TransportEnvelope) -> Result<(), TransportError> {
        let payload_len = envelope.payload.len();
        if let Some(shared) = self.transport.shared_transport() {
            shared.route_envelope(envelope);
            self.transport.record_send(payload_len);
            return Ok(());
        }

        let self_device_id = self.config.device_id.to_string();
        let destination_device_id = envelope.metadata.get("aura-destination-device-id");
        let is_local = if envelope.destination == self.authority_id {
            match destination_device_id {
                Some(dst) => dst == &self_device_id,
                None => true,
            }
        } else {
            destination_device_id.is_some_and(|dst| dst == &self_device_id)
        };
        if is_local {
            self.queue_runtime_envelope(envelope);
            self.transport.record_send(payload_len);
            return Ok(());
        }

        let addr = resolve_peer_addr(self, envelope.context, envelope.destination)
            .await
            .ok_or(TransportError::DestinationUnreachable {
                destination: envelope.destination,
            })?;

        match send_envelope_tcp(&addr, &envelope).await {
            Ok(()) => {
                self.transport.record_send(payload_len);
                Ok(())
            }
            Err(err) => {
                self.transport.record_send_failure();
                Err(err)
            }
        }
    }

    async fn receive_envelope(&self) -> Result<TransportEnvelope, TransportError> {
        let self_device_id = self.config.device_id.to_string();
        let inbox = self.transport.inbox();
        let maybe = {
            let mut inbox = inbox.write();
            // In shared transport mode, filter by destination (this agent's authority ID)
            inbox
                .iter()
                .position(|env| {
                    let device_match = env
                        .metadata
                        .get("aura-destination-device-id")
                        .is_some_and(|dst| dst == &self_device_id);

                    if env.destination == self.authority_id {
                        return match env.metadata.get("aura-destination-device-id") {
                            Some(dst) => dst == &self_device_id,
                            None => true,
                        };
                    }

                    // Allow device-targeted envelopes for other authorities (multi-authority devices).
                    device_match
                })
                .map(|pos| inbox.remove(pos))
        };

        match maybe {
            Some(env) => {
                self.transport.record_receive();
                Ok(env)
            }
            None => Err(TransportError::NoMessage),
        }
    }

    async fn receive_envelope_from(
        &self,
        source: AuthorityId,
        context: ContextId,
    ) -> Result<TransportEnvelope, TransportError> {
        let self_device_id = self.config.device_id.to_string();
        let inbox = self.transport.inbox();
        let maybe = {
            let mut inbox = inbox.write();
            // In shared transport mode, filter by destination AND source/context
            inbox
                .iter()
                .position(|env| {
                    let device_match = env
                        .metadata
                        .get("aura-destination-device-id")
                        .is_some_and(|dst| dst == &self_device_id);

                    if env.destination == self.authority_id {
                        env.source == source
                            && env.context == context
                            && match env.metadata.get("aura-destination-device-id") {
                                Some(dst) => dst == &self_device_id,
                                None => true,
                            }
                    } else {
                        env.source == source && env.context == context && device_match
                    }
                })
                .map(|pos| inbox.remove(pos))
        };

        match maybe {
            Some(env) => {
                self.transport.record_receive();
                Ok(env)
            }
            None => Err(TransportError::NoMessage),
        }
    }

    async fn is_channel_established(&self, context: ContextId, peer: AuthorityId) -> bool {
        if let Some(shared) = self.transport.shared_transport() {
            return shared.is_peer_online(peer);
        }
        if let Some(manager) = self.rendezvous_manager() {
            return manager.get_descriptor(context, peer).await.is_some();
        }
        false
    }

    async fn get_transport_stats(&self) -> TransportStats {
        let mut stats = self.transport.stats_snapshot();

        if let Some(shared) = self.transport.shared_transport() {
            let active = shared.connected_peer_count(self.authority_id) as u32;
            self.transport.set_active_channels(active);
            stats.active_channels = active;
        }

        stats
    }
}

async fn resolve_peer_addr(
    effects: &AuraEffectSystem,
    context: ContextId,
    peer: AuthorityId,
) -> Option<String> {
    let manager = effects.rendezvous_manager()?;
    if let Some(addr) = manager
        .get_descriptor(context, peer)
        .await
        .and_then(descriptor_transport_addr)
    {
        return Some(addr);
    }

    let fallback_context = default_context_id_for_authority(peer);
    if fallback_context == context {
        return manager
            .get_any_descriptor_for_authority(peer)
            .await
            .and_then(descriptor_transport_addr);
    }

    if let Some(addr) = manager
        .get_descriptor(fallback_context, peer)
        .await
        .and_then(descriptor_transport_addr)
    {
        return Some(addr);
    }

    manager
        .get_any_descriptor_for_authority(peer)
        .await
        .and_then(descriptor_transport_addr)
}

fn descriptor_transport_addr(descriptor: aura_rendezvous::RendezvousDescriptor) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    for hint in &descriptor.transport_hints {
        if let TransportHint::WebSocketDirect { addr, .. } = hint {
            return Some(addr.to_string());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    for hint in &descriptor.transport_hints {
        if let TransportHint::WebSocketDirect { addr, .. } = hint {
            return Some(format!("ws://{}", addr));
        }
    }

    for hint in descriptor.transport_hints {
        if let TransportHint::TcpDirect { addr, .. } = hint {
            return Some(addr.to_string());
        }
    }
    None
}

async fn send_envelope_tcp(addr: &str, envelope: &TransportEnvelope) -> Result<(), TransportError> {
    cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            let payload = aura_core::util::serialization::to_vec(envelope).map_err(|e| {
                TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: format!("Envelope serialization failed: {e}"),
                }
            })?;
            let url = normalize_ws_url(addr);

            run_local_ws(move || async move {
                let mut ws = WebSocket::open(&url)
                    .map_err(|e| format!("WebSocket open failed ({url}): {e}"))?;
                ws.send(Message::Bytes(payload))
                    .await
                    .map_err(|e| format!("WebSocket send failed ({url}): {e}"))?;
                Ok(())
            })
            .await
            .map_err(|reason| TransportError::SendFailed {
                destination: envelope.destination,
                reason,
            })
        } else {
            let config = TransportConfig::default();
            if addr.starts_with("ws://") || addr.starts_with("wss://") {
                let (mut ws, _) = timeout(config.connect_timeout.get(), connect_async(addr))
                    .await
                    .map_err(|_| TransportError::SendFailed {
                        destination: envelope.destination,
                        reason: "WebSocket connect timeout".to_string(),
                    })?
                    .map_err(|e| TransportError::SendFailed {
                        destination: envelope.destination,
                        reason: format!("WebSocket connect failed: {e}"),
                    })?;

                let payload = aura_core::util::serialization::to_vec(envelope).map_err(|e| {
                    TransportError::SendFailed {
                        destination: envelope.destination,
                        reason: format!("Envelope serialization failed: {e}"),
                    }
                })?;

                timeout(
                    config.write_timeout.get(),
                    ws.send(TungsteniteMessage::Binary(payload)),
                )
                .await
                .map_err(|_| TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: "WebSocket write timeout".to_string(),
                })?
                .map_err(|e| TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: format!("WebSocket send failed: {e}"),
                })?;

                return Ok(());
            }

            let socket_addr: SocketAddr = addr.parse().map_err(|e| TransportError::SendFailed {
                destination: envelope.destination,
                reason: format!("Invalid transport address '{addr}': {e}"),
            })?;

            let mut stream = timeout(
                config.connect_timeout.get(),
                TcpStream::connect(socket_addr),
            )
            .await
            .map_err(|_| TransportError::SendFailed {
                destination: envelope.destination,
                reason: "TCP connect timeout".to_string(),
            })?
            .map_err(|e| TransportError::SendFailed {
                destination: envelope.destination,
                reason: format!("TCP connect failed: {e}"),
            })?;

            let payload = aura_core::util::serialization::to_vec(envelope).map_err(|e| {
                TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: format!("Envelope serialization failed: {e}"),
                }
            })?;

            let len = (payload.len() as u32).to_be_bytes();
            timeout(config.write_timeout.get(), stream.write_all(&len))
                .await
                .map_err(|_| TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: "TCP write timeout".to_string(),
                })?
                .map_err(|e| TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: format!("TCP write failed: {e}"),
                })?;
            timeout(config.write_timeout.get(), stream.write_all(&payload))
                .await
                .map_err(|_| TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: "TCP write timeout".to_string(),
                })?
                .map_err(|e| TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: format!("TCP write failed: {e}"),
                })?;
            timeout(config.write_timeout.get(), stream.flush())
                .await
                .map_err(|_| TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: "TCP flush timeout".to_string(),
                })?
                .map_err(|e| TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: format!("TCP flush failed: {e}"),
                })?;

            Ok(())
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn normalize_ws_url(addr: &str) -> String {
    if addr.starts_with("ws://") || addr.starts_with("wss://") {
        addr.to_string()
    } else {
        format!("ws://{addr}")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use crate::runtime::services::{RendezvousManager, RendezvousManagerConfig};
    use aura_rendezvous::RendezvousDescriptor;
    use std::sync::Arc;

    fn descriptor(
        authority_id: AuthorityId,
        context_id: ContextId,
        transport_hints: Vec<TransportHint>,
    ) -> RendezvousDescriptor {
        RendezvousDescriptor {
            authority_id,
            device_id: None,
            context_id,
            transport_hints,
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
            valid_from: 1,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        }
    }

    #[tokio::test]
    async fn resolve_peer_addr_falls_back_when_primary_descriptor_has_no_tcp_hint() {
        let authority = AuthorityId::new_from_entropy([210u8; 32]);
        let peer = AuthorityId::new_from_entropy([211u8; 32]);
        let primary_context = ContextId::new_from_entropy([212u8; 32]);
        let fallback_context = default_context_id_for_authority(peer);

        let config = AgentConfig::default();
        let effects =
            AuraEffectSystem::simulation_for_test_for_authority(&config, authority).unwrap();
        let manager = RendezvousManager::new_with_default_udp(
            authority,
            RendezvousManagerConfig::default(),
            Arc::new(effects.time_effects().clone()),
        );
        effects.attach_rendezvous_manager(manager.clone());

        manager
            .cache_descriptor(descriptor(
                peer,
                primary_context,
                vec![TransportHint::quic_direct("127.0.0.1:55001").unwrap()],
            ))
            .await
            .unwrap();

        manager
            .cache_descriptor(descriptor(
                peer,
                fallback_context,
                vec![TransportHint::tcp_direct("127.0.0.1:55002").unwrap()],
            ))
            .await
            .unwrap();

        let resolved = resolve_peer_addr(&effects, primary_context, peer).await;
        assert_eq!(resolved.as_deref(), Some("127.0.0.1:55002"));
    }
}
