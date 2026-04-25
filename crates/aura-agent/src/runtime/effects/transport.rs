use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::transport::{
    TransportEnvelope, TransportError, TransportStats, MAX_TRANSPORT_SIGNATURE_BYTES,
};
use aura_core::effects::TransportEffects;
use aura_core::service::{LinkEndpoint, LinkProtocol, Route};
#[cfg(not(target_arch = "wasm32"))]
use aura_core::{execute_with_timeout_budget, TimeoutBudget, TimeoutRunError};
use aura_core::{AuthorityId, ContextId};
#[cfg(not(target_arch = "wasm32"))]
use aura_effects::time::PhysicalTimeHandler;
#[cfg(not(target_arch = "wasm32"))]
use aura_effects::transport::TransportConfig;
use aura_rendezvous::RendezvousDescriptor;
use base64::{engine::general_purpose::STANDARD, Engine};
use cfg_if::cfg_if;
#[cfg(not(target_arch = "wasm32"))]
use futures::SinkExt;
#[cfg(target_arch = "wasm32")]
use futures::SinkExt;
#[cfg(target_arch = "wasm32")]
use gloo_net::websocket::{futures::WebSocket, Message};
use serde::Serialize;
#[cfg(target_arch = "wasm32")]
use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::AsyncWriteExt;
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::TcpStream;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::{connect_async, tungstenite::Message as TungsteniteMessage};

#[cfg(target_arch = "wasm32")]
const HARNESS_INSTANCE_QUERY_KEY: &str = "__aura_harness_instance";
#[cfg(target_arch = "wasm32")]
const HARNESS_TOKEN_QUERY_KEY: &str = "__aura_harness_token";
#[cfg(target_arch = "wasm32")]
const MIN_HARNESS_TOKEN_LEN: usize = 16;
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
        let now_ms = self
            .time_effects()
            .physical_time()
            .await
            .map(|time| time.ts_ms)
            .unwrap_or(0);

        let route = resolve_move_route(self, envelope.context, envelope.destination)
            .await
            .unwrap_or_else(|| fallback_direct_route(&envelope));

        if let Some(move_manager) = self.move_manager() {
            let batch = move_manager
                .enqueue_for_delivery(envelope, route, now_ms, self)
                .await
                .map_err(|error| TransportError::ProtocolError {
                    details: error.to_string(),
                })?;

            for plan in batch {
                let payload_len = plan.envelope.payload.len();
                let context = plan.envelope.context;
                let destination = plan.envelope.destination;
                match send_planned_envelope(self, plan.envelope, &plan.route).await {
                    Ok(()) => {
                        self.transport.record_send(payload_len);
                        move_manager
                            .record_delivery_result(
                                plan.replay_marker,
                                context,
                                destination,
                                true,
                                now_ms,
                            )
                            .await;
                    }
                    Err(error) => {
                        self.transport.record_send_failure();
                        move_manager
                            .record_delivery_result(
                                plan.replay_marker,
                                context,
                                destination,
                                false,
                                now_ms,
                            )
                            .await;
                        return Err(error);
                    }
                }
            }
            return Ok(());
        }

        let payload_len = envelope.payload.len();
        let fallback_route = fallback_direct_route(&envelope);
        match send_planned_envelope(self, envelope, &fallback_route).await {
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
                validate_inbound_transport_receipt(&env)?;
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
                validate_inbound_transport_receipt(&env)?;
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
    resolve_move_route(effects, context, peer)
        .await
        .and_then(|route| route_destination_addr(&route.destination))
}

async fn resolve_move_route(
    effects: &AuraEffectSystem,
    context: ContextId,
    peer: AuthorityId,
) -> Option<Route> {
    let manager = effects.rendezvous_manager()?;
    let descriptor = manager.get_descriptor(context, peer).await?;
    if descriptor_has_placeholder_crypto(&descriptor)
        && !(effects.is_testing() || effects.harness_mode_enabled())
    {
        tracing::warn!(
            peer = %peer,
            context = %context,
            "Rejecting placeholder rendezvous descriptor for transport route resolution"
        );
        return None;
    }
    let paths = descriptor.advertised_move_paths();
    #[cfg(not(target_arch = "wasm32"))]
    let paths = {
        let mut paths = paths;
        paths.sort_by_key(|path| match path.route.destination.protocol {
            LinkProtocol::Tcp => 0u8,
            LinkProtocol::WebSocket => 1u8,
            _ => 2u8,
        });
        paths
    };
    paths
        .into_iter()
        .map(|path| path.route)
        .find(|route| direct_route_allowed(effects, route))
}

fn descriptor_has_placeholder_crypto(descriptor: &RendezvousDescriptor) -> bool {
    descriptor.public_key == [0u8; 32] || descriptor.handshake_psk_commitment == [0u8; 32]
}

fn direct_route_allowed(effects: &AuraEffectSystem, route: &Route) -> bool {
    if effects.is_testing() || effects.harness_mode_enabled() || !route.is_direct() {
        return true;
    }

    let Some(addr) = route_destination_addr(&route.destination) else {
        return false;
    };
    direct_addr_allowed_in_production(&addr)
}

#[cfg(not(target_arch = "wasm32"))]
fn direct_addr_allowed_in_production(addr: &str) -> bool {
    match addr.parse::<SocketAddr>() {
        Ok(socket) => ip_allowed_for_production_direct_egress(socket.ip()),
        Err(_) => false,
    }
}

#[cfg(target_arch = "wasm32")]
fn direct_addr_allowed_in_production(_addr: &str) -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn ip_allowed_for_production_direct_egress(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.is_broadcast()
                || (a == 192 && b == 0 && c == 2)
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113)
                || ip.is_unspecified())
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || is_ipv6_unicast_link_local(ip)
                || is_ipv6_unique_local(ip)
                || (segments[0] == 0x2001 && segments[1] == 0x0db8))
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn is_ipv6_unicast_link_local(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    (first & 0xffc0) == 0xfe80
}

#[cfg(not(target_arch = "wasm32"))]
fn is_ipv6_unique_local(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    (first & 0xfe00) == 0xfc00
}

async fn send_planned_envelope(
    effects: &AuraEffectSystem,
    envelope: TransportEnvelope,
    _route: &Route,
) -> Result<(), TransportError> {
    if let Some(shared) = effects.transport.shared_transport() {
        shared.route_envelope(envelope);
        return Ok(());
    }

    let self_device_id = effects.config.device_id.to_string();
    let destination_device_id = envelope.metadata.get("aura-destination-device-id");
    let is_local = if envelope.destination == effects.authority_id {
        match destination_device_id {
            Some(dst) => dst == &self_device_id,
            None => true,
        }
    } else {
        destination_device_id.is_some_and(|dst| dst == &self_device_id)
    };
    if is_local {
        effects.queue_runtime_envelope(envelope);
        return Ok(());
    }

    let addr = resolve_peer_addr(effects, envelope.context, envelope.destination)
        .await
        .ok_or(TransportError::DestinationUnreachable {
            destination: envelope.destination,
        })?;
    if envelope
        .metadata
        .get("content-type")
        .is_some_and(|value| value == "application/aura-invitation")
    {
        tracing::info!(
            destination = %envelope.destination,
            context = %envelope.context,
            addr = %addr,
            "Resolved invitation transport target"
        );
    }
    send_envelope_tcp(&addr, &envelope).await
}

fn fallback_direct_route(envelope: &TransportEnvelope) -> Route {
    Route::direct(LinkEndpoint::direct(
        LinkProtocol::Tcp,
        format!("runtime://{}", envelope.destination),
    ))
}

fn validate_inbound_transport_receipt(envelope: &TransportEnvelope) -> Result<(), TransportError> {
    let Some(receipt) = envelope.receipt.as_ref() else {
        return Ok(());
    };

    if receipt.context != envelope.context {
        return Err(TransportError::ReceiptValidationFailed {
            reason: "receipt context does not match envelope context".to_string(),
        });
    }
    if receipt.src != envelope.source {
        return Err(TransportError::ReceiptValidationFailed {
            reason: "receipt source does not match envelope source".to_string(),
        });
    }
    if receipt.dst != envelope.destination {
        return Err(TransportError::ReceiptValidationFailed {
            reason: "receipt destination does not match envelope destination".to_string(),
        });
    }
    if receipt.sig.is_empty() {
        return Err(TransportError::ReceiptValidationFailed {
            reason: "receipt signature is missing".to_string(),
        });
    }
    if receipt.sig.len() > MAX_TRANSPORT_SIGNATURE_BYTES {
        return Err(TransportError::ReceiptValidationFailed {
            reason: "receipt signature exceeds transport bound".to_string(),
        });
    }

    Ok(())
}

fn route_destination_addr(endpoint: &LinkEndpoint) -> Option<String> {
    match endpoint.protocol {
        LinkProtocol::Tcp | LinkProtocol::WebSocket => {}
        _ => return None,
    }

    #[cfg(target_arch = "wasm32")]
    if endpoint.protocol == LinkProtocol::WebSocket {
        return endpoint.address.clone();
    }

    #[cfg(not(target_arch = "wasm32"))]
    if endpoint.protocol == LinkProtocol::WebSocket {
        return endpoint
            .address
            .as_ref()
            .map(|addr| format!("ws://{}", addr));
    }

    endpoint.address.clone()
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
                Some(encode_harness_browser_transport_envelope(envelope, &payload)?)
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
                let wrapped_payload = if use_native_harness_browser_transport(addr) {
                    Some(encode_harness_browser_transport_envelope(envelope, &payload)?)
                } else {
                    None
                };

                execute_transport_timeout(
                    config.write_timeout.get(),
                    || TransportError::SendFailed {
                        destination: envelope.destination,
                        reason: "WebSocket write timeout".to_string(),
                    },
                    || async {
                        let message = match wrapped_payload {
                            Some(ref wrapped) => TungsteniteMessage::Text(wrapped.clone()),
                            None => TungsteniteMessage::Binary(payload),
                        };
                        ws.send(message)
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

fn encode_harness_browser_transport_envelope(
    envelope: &TransportEnvelope,
    payload: &[u8],
) -> Result<String, TransportError> {
    serde_json::to_string(&HarnessBrowserTransportEnvelope::from_parts(
        envelope, payload,
    ))
    .map_err(|e| TransportError::SendFailed {
        destination: envelope.destination,
        reason: format!("Harness browser transport encode failed: {e}"),
    })
}

#[cfg(any(test, target_arch = "wasm32"))]
fn normalize_ws_url(addr: &str) -> String {
    if addr.starts_with("ws://") || addr.starts_with("wss://") {
        addr.to_string()
    } else {
        format!("ws://{addr}")
    }
}

#[cfg(test)]
fn harness_browser_transport_ws_url(current_host: &str, harness_mode: bool) -> Option<String> {
    if !harness_mode || current_host.is_empty() {
        return None;
    }

    Some(normalize_ws_url(current_host))
}

#[cfg(target_arch = "wasm32")]
fn current_browser_location_and_authenticated_harness_mode() -> Option<(String, String, bool)> {
    let window = web_sys::window()?;
    let search = window.location().search().ok()?;
    let host = window.location().host().ok()?;
    let origin = window.location().origin().ok()?;
    let query = search.strip_prefix('?').unwrap_or(&search);
    let mut has_instance = false;
    let mut has_token = false;
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key == HARNESS_INSTANCE_QUERY_KEY && !value.is_empty() {
            has_instance = true;
        } else if key == HARNESS_TOKEN_QUERY_KEY && value.len() >= MIN_HARNESS_TOKEN_LEN {
            has_token = true;
        }
    }
    let harness_mode = has_instance && has_token;
    Some((host, origin, harness_mode))
}

#[cfg(target_arch = "wasm32")]
fn current_browser_harness_enqueue_url() -> Option<String> {
    let (_host, origin, harness_mode) = current_browser_location_and_authenticated_harness_mode()?;
    if !harness_mode || origin.is_empty() {
        return None;
    }
    Some(format!("{origin}{HARNESS_TRANSPORT_ENQUEUE_PATH}"))
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

#[cfg(not(target_arch = "wasm32"))]
fn use_native_harness_browser_transport(addr: &str) -> bool {
    addr.starts_with("ws://") || addr.starts_with("wss://")
}

#[cfg(target_arch = "wasm32")]
fn resolve_browser_transport_target(addr: &str) -> (String, bool) {
    let normalized_target = normalize_ws_url(addr);
    if let Some((host, _origin, harness_mode)) =
        current_browser_location_and_authenticated_harness_mode()
    {
        if harness_mode && browser_target_uses_harness_transport(&host, &normalized_target) {
            if let Some(enqueue_url) = current_browser_harness_enqueue_url() {
                return (enqueue_url, true);
            }
        }
    }

    (normalized_target, false)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_target_uses_harness_transport(current_host: &str, normalized_target: &str) -> bool {
    if current_host.is_empty() {
        return false;
    }
    normalized_target == normalize_ws_url(current_host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::default_context_id_for_authority;
    use crate::core::AgentConfig;
    use crate::runtime::services::{
        MoveManager, MoveManagerConfig, RendezvousManager, RendezvousManagerConfig, RuntimeService,
        RuntimeServiceContext, ServiceRegistry,
    };
    use crate::runtime::TaskSupervisor;
    use aura_core::effects::transport::TransportEnvelope;
    use aura_core::effects::TransportEffects;
    use aura_rendezvous::{RendezvousDescriptor, TransportHint};
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::sync::Arc;

    struct EnvRestore {
        key: &'static str,
        value: Option<OsString>,
    }

    impl EnvRestore {
        fn capture(key: &'static str) -> Self {
            Self {
                key,
                value: std::env::var_os(key),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            if let Some(value) = self.value.take() {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

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

    #[test]
    fn browser_target_uses_harness_transport_only_for_page_host() {
        assert!(browser_target_uses_harness_transport(
            "127.0.0.1:4173",
            "ws://127.0.0.1:4173"
        ));
        assert!(!browser_target_uses_harness_transport(
            "127.0.0.1:4173",
            "ws://127.0.0.1:51628"
        ));
        assert!(!browser_target_uses_harness_transport(
            "127.0.0.1:4173",
            "ws://127.0.0.1:21628"
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_harness_browser_transport_wraps_websocket_targets() {
        assert!(use_native_harness_browser_transport("ws://127.0.0.1:4173"));
        assert!(use_native_harness_browser_transport(
            "wss://example.test/socket"
        ));
        assert!(!use_native_harness_browser_transport("127.0.0.1:4173"));
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

    #[tokio::test]
    async fn resolve_peer_addr_allows_placeholder_direct_descriptor_in_harness_mode() {
        let harness_mode_env = crate::runtime_bridge::harness_mode_env_key_for_tests();
        let _env_restore = EnvRestore::capture(harness_mode_env);
        std::env::set_var(harness_mode_env, "1");

        let authority = AuthorityId::new_from_entropy([213u8; 32]);
        let peer = AuthorityId::new_from_entropy([214u8; 32]);
        let context = default_context_id_for_authority(peer);

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
                context,
                vec![TransportHint::tcp_direct("127.0.0.1:55003").unwrap()],
            ))
            .await
            .unwrap();

        let resolved = resolve_peer_addr(&effects, context, peer).await;
        assert_eq!(resolved.as_deref(), Some("127.0.0.1:55003"));
        RuntimeService::stop(&manager).await.unwrap();
    }

    #[test]
    fn production_direct_egress_rejects_local_and_private_ip_literals() {
        assert!(!direct_addr_allowed_in_production("127.0.0.1:55001"));
        assert!(!direct_addr_allowed_in_production("10.0.0.1:55001"));
        assert!(!direct_addr_allowed_in_production("172.16.0.1:55001"));
        assert!(!direct_addr_allowed_in_production("192.168.0.1:55001"));
        assert!(!direct_addr_allowed_in_production("169.254.1.1:55001"));
        assert!(!direct_addr_allowed_in_production("[::1]:55001"));
        assert!(!direct_addr_allowed_in_production("[fc00::1]:55001"));
        assert!(!direct_addr_allowed_in_production("[fe80::1]:55001"));
    }

    #[test]
    fn production_direct_egress_allows_public_ip_literals() {
        assert!(direct_addr_allowed_in_production("8.8.8.8:443"));
        assert!(direct_addr_allowed_in_production(
            "[2001:4860:4860::8888]:443"
        ));
    }

    #[test]
    fn harness_mode_allows_loopback_direct_routes() {
        let harness_mode_env = crate::runtime_bridge::harness_mode_env_key_for_tests();
        let _env_restore = EnvRestore::capture(harness_mode_env);
        std::env::set_var(harness_mode_env, "1");

        let config = AgentConfig::default();
        let authority = AuthorityId::new_from_entropy([229u8; 32]);
        let effects = AuraEffectSystem::simulation_for_test_for_authority(&config, authority)
            .expect("transport harness effects should build");
        let route = Route::direct(LinkEndpoint::direct(
            LinkProtocol::Tcp,
            "127.0.0.1:55001".to_string(),
        ));

        assert!(direct_route_allowed(&effects, &route));
    }

    #[tokio::test]
    async fn move_passthrough_preserves_opaque_envelope_delivery() {
        let config = AgentConfig::default();
        let sender = AuthorityId::new_from_entropy([220u8; 32]);
        let receiver = AuthorityId::new_from_entropy([221u8; 32]);
        let context = ContextId::new_from_entropy([222u8; 32]);

        let plain_shared = crate::runtime::SharedTransport::new();
        let plain_sender =
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                sender,
                plain_shared.clone(),
            )
            .unwrap();
        let plain_receiver =
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                receiver,
                plain_shared,
            )
            .unwrap();

        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            "application/aura-opaque-object".to_string(),
        );
        let envelope = TransportEnvelope {
            destination: receiver,
            source: sender,
            context,
            payload: vec![9, 4, 2, 7, 1],
            metadata,
            receipt: None,
        };

        plain_sender.send_envelope(envelope.clone()).await.unwrap();
        let baseline = plain_receiver.receive_envelope().await.unwrap();

        let move_shared = crate::runtime::SharedTransport::new();
        let move_sender =
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                sender,
                move_shared.clone(),
            )
            .unwrap();
        let move_receiver =
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                receiver,
                move_shared,
            )
            .unwrap();
        let move_manager = MoveManager::new(
            MoveManagerConfig::for_testing(),
            Arc::new(ServiceRegistry::new()),
        );
        move_sender.attach_move_manager(move_manager.clone());

        move_sender.send_envelope(envelope.clone()).await.unwrap();
        let migrated = move_receiver.receive_envelope().await.unwrap();

        assert_eq!(baseline.destination, envelope.destination);
        assert_eq!(baseline.source, envelope.source);
        assert_eq!(baseline.context, envelope.context);
        assert_eq!(baseline.payload, envelope.payload);
        assert_eq!(baseline.metadata, envelope.metadata);
        assert!(baseline.receipt.is_none());

        assert_eq!(migrated.destination, baseline.destination);
        assert_eq!(migrated.source, baseline.source);
        assert_eq!(migrated.context, baseline.context);
        assert_eq!(migrated.payload, baseline.payload);
        assert_eq!(migrated.metadata, baseline.metadata);
        assert!(migrated.receipt.is_none());

        let projection = move_manager.projection().await;
        assert_eq!(projection.queued_envelopes, 0);
        assert_eq!(projection.replay_window_entries, 0);
    }

    #[tokio::test]
    async fn receive_envelope_rejects_receipt_binding_mismatch() {
        let config = AgentConfig::default();
        let sender = AuthorityId::new_from_entropy([230u8; 32]);
        let receiver = AuthorityId::new_from_entropy([231u8; 32]);
        let context = ContextId::new_from_entropy([232u8; 32]);

        let shared = crate::runtime::SharedTransport::new();
        let sender_effects =
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config,
                sender,
                shared.clone(),
            )
            .unwrap();
        let receiver_effects =
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &config, receiver, shared,
            )
            .unwrap();

        let envelope = TransportEnvelope {
            destination: receiver,
            source: sender,
            context,
            payload: vec![1, 2, 3],
            metadata: HashMap::new(),
            receipt: Some(aura_core::effects::transport::TransportReceipt {
                context: ContextId::new_from_entropy([233u8; 32]),
                src: sender,
                dst: receiver,
                epoch: 1,
                cost: 1,
                nonce: 7,
                prev: [0u8; 32],
                sig: vec![0xAA],
            }),
        };

        sender_effects.send_envelope(envelope).await.unwrap();
        let error = receiver_effects
            .receive_envelope()
            .await
            .expect_err("receipt binding mismatch must fail closed");
        assert!(matches!(
            error,
            TransportError::ReceiptValidationFailed { .. }
        ));
    }
}
