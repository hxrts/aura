use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::effects::transport::{TransportEnvelope, TransportStats};
use aura_core::effects::{TransportEffects, TransportError};
use aura_core::{AuthorityId, ContextId};
use aura_effects::transport::TransportConfig;
use aura_rendezvous::TransportHint;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::timeout;

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
        let is_local = envelope.destination == self.authority_id
            || envelope
                .metadata
                .get("aura-destination-device-id")
                .is_some_and(|dst| dst == &self_device_id);
        if is_local {
            self.transport.queue_envelope(envelope);
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
    let descriptor = manager.get_descriptor(context, peer).await?;
    for hint in descriptor.transport_hints {
        if let TransportHint::TcpDirect { addr } = hint {
            return Some(addr.to_string());
        }
    }
    None
}

async fn send_envelope_tcp(addr: &str, envelope: &TransportEnvelope) -> Result<(), TransportError> {
    let socket_addr: SocketAddr = addr.parse().map_err(|e| TransportError::SendFailed {
        destination: envelope.destination,
        reason: format!("Invalid transport address '{addr}': {e}"),
    })?;

    let config = TransportConfig::default();
    let mut stream = timeout(config.connect_timeout.get(), TcpStream::connect(socket_addr))
        .await
        .map_err(|_| TransportError::SendFailed {
            destination: envelope.destination,
            reason: "TCP connect timeout".to_string(),
        })?
        .map_err(|e| TransportError::SendFailed {
            destination: envelope.destination,
            reason: format!("TCP connect failed: {e}"),
        })?;

    let payload = aura_core::util::serialization::to_vec(envelope).map_err(|e| TransportError::SendFailed {
        destination: envelope.destination,
        reason: format!("Envelope serialization failed: {e}"),
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
