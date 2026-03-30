use super::AuraEffectSystem;
use async_trait::async_trait;
#[cfg(not(target_arch = "wasm32"))]
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::transport::{TransportEnvelope, TransportStats};
use aura_core::effects::{TransportEffects, TransportError};
#[cfg(not(target_arch = "wasm32"))]
use aura_core::{execute_with_timeout_budget, TimeoutBudget, TimeoutRunError};
use aura_core::{AuthorityId, ContextId};
#[cfg(not(target_arch = "wasm32"))]
use aura_effects::time::PhysicalTimeHandler;
#[cfg(not(target_arch = "wasm32"))]
use aura_effects::transport::TransportConfig;
#[cfg(target_arch = "wasm32")]
use base64::{engine::general_purpose::STANDARD, Engine};
use cfg_if::cfg_if;
#[cfg(not(target_arch = "wasm32"))]
use futures::SinkExt;
#[cfg(target_arch = "wasm32")]
use futures::SinkExt;
#[cfg(target_arch = "wasm32")]
use gloo_net::websocket::{futures::WebSocket, Message};
#[cfg(target_arch = "wasm32")]
use serde::Serialize;
#[cfg(target_arch = "wasm32")]
use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::AsyncWriteExt;
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::TcpStream;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::{connect_async, tungstenite::Message as TungsteniteMessage};
#[cfg(not(target_arch = "wasm32"))]
async fn execute_transport_timeout<F, Fut, T>(
    timeout: std::time::Duration,
    timeout_reason: impl Fn() -> TransportError + Copy,
    f: F,
) -> Result<T, TransportError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, TransportError>>,
{
    let time = PhysicalTimeHandler::new();
    let started_at = time.physical_time().await.map_err(|_| timeout_reason())?;
    let budget = TimeoutBudget::from_start_and_timeout(&started_at, timeout)
        .map_err(|_| timeout_reason())?;
    execute_with_timeout_budget(&time, &budget, f)
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => timeout_reason(),
            TimeoutRunError::Operation(error) => error,
        })
}
// Implementation of TransportEffects
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
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

        #[cfg(target_arch = "wasm32")]
        if let Some(url) = current_browser_harness_enqueue_url() {
            send_harness_browser_envelope(&url, &envelope)?;
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
    manager
        .get_descriptor(context, peer)
        .await
        .and_then(descriptor_transport_addr)
}

fn descriptor_transport_addr(descriptor: aura_rendezvous::RendezvousDescriptor) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    for endpoint in descriptor.advertised_link_endpoints() {
        if endpoint.protocol == aura_core::LinkProtocol::WebSocket {
            return endpoint.address;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    for endpoint in descriptor.advertised_link_endpoints() {
        if endpoint.protocol == aura_core::LinkProtocol::WebSocket {
            return endpoint.address.map(|addr| format!("ws://{}", addr));
        }
    }

    for endpoint in descriptor.advertised_link_endpoints() {
        if endpoint.protocol == aura_core::LinkProtocol::Tcp {
            return endpoint.address;
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
            let (url, use_harness_transport) = resolve_browser_transport_target(addr);
            log_harness_mailbox_send(envelope, &url, use_harness_transport);
            let wrapped_payload = if use_harness_transport {
                Some(
                    serde_json::to_string(
                        &HarnessBrowserTransportEnvelope::from_parts(envelope, &payload),
                    )
                    .map_err(|e| TransportError::SendFailed {
                        destination: envelope.destination,
                        reason: format!("Harness browser transport encode failed: {e}"),
                    })?,
                )
            } else {
                None
            };

            if let Some(wrapped) = wrapped_payload {
                let window = web_sys::window().ok_or_else(|| TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: "browser window unavailable for harness transport enqueue".to_string(),
                })?;
                let init = web_sys::RequestInit::new();
                init.set_method("POST");
                let body_value = wrapped.into();
                init.set_body(&body_value);
                let request = web_sys::Request::new_with_str_and_init(&url, &init).map_err(
                    |error| TransportError::SendFailed {
                        destination: envelope.destination,
                        reason: format!(
                            "Harness browser transport build failed ({url}): {error:?}"
                        ),
                    },
                )?;
                request
                    .headers()
                    .set("Content-Type", "application/json; charset=utf-8")
                    .map_err(|error| TransportError::SendFailed {
                        destination: envelope.destination,
                        reason: format!(
                            "Harness browser transport header failed ({url}): {error:?}"
                        ),
                    })?;
                let _ = window.fetch_with_request(&request);
                return Ok(());
            }

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
                let (mut ws, _) = execute_transport_timeout(
                    config.connect_timeout.get(),
                    || TransportError::SendFailed {
                        destination: envelope.destination,
                        reason: "WebSocket connect timeout".to_string(),
                    },
                    || async {
                        connect_async(addr).await.map_err(|e| TransportError::SendFailed {
                            destination: envelope.destination,
                            reason: format!("WebSocket connect failed: {e}"),
                        })
                    },
                )
                .await?;

                let payload = aura_core::util::serialization::to_vec(envelope).map_err(|e| {
                    TransportError::SendFailed {
                        destination: envelope.destination,
                        reason: format!("Envelope serialization failed: {e}"),
                    }
                })?;

                execute_transport_timeout(
                    config.write_timeout.get(),
                    || TransportError::SendFailed {
                        destination: envelope.destination,
                        reason: "WebSocket write timeout".to_string(),
                    },
                    || async {
                        ws.send(TungsteniteMessage::Binary(payload))
                            .await
                            .map_err(|e| TransportError::SendFailed {
                                destination: envelope.destination,
                                reason: format!("WebSocket send failed: {e}"),
                            })
                    },
                )
                .await?;

                return Ok(());
            }

            let socket_addr: SocketAddr = addr.parse().map_err(|e| TransportError::SendFailed {
                destination: envelope.destination,
                reason: format!("Invalid transport address '{addr}': {e}"),
            })?;

            let mut stream = execute_transport_timeout(
                config.connect_timeout.get(),
                || TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: "TCP connect timeout".to_string(),
                },
                || async {
                    TcpStream::connect(socket_addr)
                        .await
                        .map_err(|e| TransportError::SendFailed {
                            destination: envelope.destination,
                            reason: format!("TCP connect failed: {e}"),
                        })
                },
            )
            .await?;

            let payload = aura_core::util::serialization::to_vec(envelope).map_err(|e| {
                TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: format!("Envelope serialization failed: {e}"),
                }
            })?;

            let len = (payload.len() as u32).to_be_bytes();
            execute_transport_timeout(
                config.write_timeout.get(),
                || TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: "TCP write timeout".to_string(),
                },
                || async {
                    stream
                        .write_all(&len)
                        .await
                        .map_err(|e| TransportError::SendFailed {
                            destination: envelope.destination,
                            reason: format!("TCP write failed: {e}"),
                        })
                },
            )
            .await?;
            execute_transport_timeout(
                config.write_timeout.get(),
                || TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: "TCP write timeout".to_string(),
                },
                || async {
                    stream
                        .write_all(&payload)
                        .await
                        .map_err(|e| TransportError::SendFailed {
                            destination: envelope.destination,
                            reason: format!("TCP write failed: {e}"),
                        })
                },
            )
            .await?;
            execute_transport_timeout(
                config.write_timeout.get(),
                || TransportError::SendFailed {
                    destination: envelope.destination,
                    reason: "TCP flush timeout".to_string(),
                },
                || async {
                    stream.flush().await.map_err(|e| TransportError::SendFailed {
                        destination: envelope.destination,
                        reason: format!("TCP flush failed: {e}"),
                    })
                },
            )
            .await?;

            Ok(())
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Serialize)]
struct HarnessBrowserTransportEnvelope<'a> {
    kind: &'static str,
    destination: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    destination_device_id: Option<&'a str>,
    envelope_b64: String,
}

#[cfg(target_arch = "wasm32")]
const HARNESS_TRANSPORT_ENQUEUE_PATH: &str = "/__aura_harness_transport__/enqueue";

#[cfg(target_arch = "wasm32")]
impl<'a> HarnessBrowserTransportEnvelope<'a> {
    fn from_parts(envelope: &'a TransportEnvelope, payload: &[u8]) -> Self {
        Self {
            kind: "transport_envelope",
            destination: envelope.destination.to_string(),
            destination_device_id: envelope
                .metadata
                .get("aura-destination-device-id")
                .map(String::as_str),
            envelope_b64: STANDARD.encode(payload),
        }
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn normalize_ws_url(addr: &str) -> String {
    if addr.starts_with("ws://") || addr.starts_with("wss://") {
        addr.to_string()
    } else {
        format!("ws://{addr}")
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn harness_browser_transport_ws_url(current_host: &str, harness_mode: bool) -> Option<String> {
    if !harness_mode || current_host.is_empty() {
        return None;
    }

    Some(normalize_ws_url(current_host))
}

#[cfg(target_arch = "wasm32")]
fn current_browser_location_and_harness_mode() -> Option<(String, String, bool)> {
    let window = web_sys::window()?;
    let search = window.location().search().ok()?;
    let host = window.location().host().ok()?;
    let origin = window.location().origin().ok()?;
    let query = search.strip_prefix('?').unwrap_or(&search);
    let harness_mode = query.split('&').any(|pair: &str| {
        pair.split_once('=')
            .is_some_and(|(key, value)| key == "__aura_harness_instance" && !value.is_empty())
    });
    Some((host, origin, harness_mode))
}

#[cfg(target_arch = "wasm32")]
fn current_browser_harness_enqueue_url() -> Option<String> {
    let (_host, origin, harness_mode) = current_browser_location_and_harness_mode()?;
    if !harness_mode || origin.is_empty() {
        return None;
    }
    Some(format!("{origin}{HARNESS_TRANSPORT_ENQUEUE_PATH}"))
}

#[cfg(target_arch = "wasm32")]
fn send_harness_browser_envelope(
    url: &str,
    envelope: &TransportEnvelope,
) -> Result<(), TransportError> {
    let payload = aura_core::util::serialization::to_vec(envelope).map_err(|e| {
        TransportError::SendFailed {
            destination: envelope.destination,
            reason: format!("Envelope serialization failed: {e}"),
        }
    })?;
    let wrapped = serde_json::to_string(&HarnessBrowserTransportEnvelope::from_parts(
        envelope, &payload,
    ))
    .map_err(|e| TransportError::SendFailed {
        destination: envelope.destination,
        reason: format!("Harness browser transport encode failed: {e}"),
    })?;
    let window = web_sys::window().ok_or_else(|| TransportError::SendFailed {
        destination: envelope.destination,
        reason: "browser window unavailable for harness transport enqueue".to_string(),
    })?;
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    let body_value = wrapped.into();
    init.set_body(&body_value);
    let request = web_sys::Request::new_with_str_and_init(url, &init).map_err(|error| {
        TransportError::SendFailed {
            destination: envelope.destination,
            reason: format!("Harness browser transport build failed ({url}): {error:?}"),
        }
    })?;
    request
        .headers()
        .set("Content-Type", "application/json; charset=utf-8")
        .map_err(|error| TransportError::SendFailed {
            destination: envelope.destination,
            reason: format!("Harness browser transport header failed ({url}): {error:?}"),
        })?;
    log_harness_mailbox_send(envelope, url, true);
    let _ = window.fetch_with_request(&request);
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn log_harness_mailbox_send(envelope: &TransportEnvelope, url: &str, use_harness_transport: bool) {
    if !use_harness_transport {
        return;
    }

    let content_type = envelope
        .metadata
        .get("content-type")
        .map(String::as_str)
        .unwrap_or("<missing>");
    if content_type != "application/aura-invitation"
        && content_type != "application/aura-invitation-acceptance+json"
    {
        return;
    }

    web_sys::console::log_1(
        &format!(
            "[web-harness-transport] mailbox_send destination={} context={} content_type={} via={}",
            envelope.destination, envelope.context, content_type, url
        )
        .into(),
    );
}

#[cfg(target_arch = "wasm32")]
async fn run_local_ws<Mk, Fut>(make_fut: Mk) -> Result<(), String>
where
    Mk: FnOnce() -> Fut + 'static,
    Fut: Future<Output = Result<(), String>> + 'static,
{
    make_fut().await
}

#[cfg(target_arch = "wasm32")]
fn resolve_browser_transport_target(addr: &str) -> (String, bool) {
    if let Some((host, _origin, harness_mode)) = current_browser_location_and_harness_mode() {
        if let Some(enqueue_url) = current_browser_harness_enqueue_url() {
            return (enqueue_url, true);
        }
        if let Some(harness_url) = harness_browser_transport_ws_url(&host, harness_mode) {
            return (harness_url, true);
        }
    }

    (normalize_ws_url(addr), false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::default_context_id_for_authority;
    use crate::core::AgentConfig;
    use crate::runtime::services::{
        RendezvousManager, RendezvousManagerConfig, RuntimeService, RuntimeServiceContext,
    };
    use crate::runtime::TaskSupervisor;
    use aura_rendezvous::{RendezvousDescriptor, TransportHint};
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

    #[test]
    fn harness_browser_transport_uses_page_host_when_enabled() {
        assert_eq!(
            harness_browser_transport_ws_url("127.0.0.1:4173", true).as_deref(),
            Some("ws://127.0.0.1:4173")
        );
        assert_eq!(
            harness_browser_transport_ws_url("127.0.0.1:4173", false),
            None
        );
        assert_eq!(harness_browser_transport_ws_url("", true), None);
    }

    #[tokio::test]
    async fn resolve_peer_addr_does_not_fall_back_across_contexts() {
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
        let service_context = RuntimeServiceContext::new(
            Arc::new(TaskSupervisor::new()),
            Arc::new(effects.time_effects().clone()),
        );
        RuntimeService::start(&manager, &service_context)
            .await
            .unwrap();

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
        assert!(resolved.is_none());
        RuntimeService::stop(&manager).await.unwrap();
    }
}
