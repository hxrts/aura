use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
use std::str::FromStr;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
use std::time::Duration;

use aura_core::types::identifiers::AuthorityId;

#[cfg(not(target_arch = "wasm32"))]
use crate::runtime::TaskGroup;

#[cfg(not(target_arch = "wasm32"))]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::{TcpListener, TcpStream};
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::RwLock;
#[cfg(not(target_arch = "wasm32"))]
use tokio::time::Instant;

/// Broker configuration for mixed native/browser bootstrap discovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapBrokerConfig {
    /// Whether broker-backed bootstrap discovery is enabled.
    pub enabled: bool,
    /// Native bind address for hosting a localhost or LAN-visible broker.
    pub bind_addr: Option<String>,
    /// Broker base URL used by runtimes that act as clients only.
    pub base_url: Option<String>,
    /// Registration time-to-live in seconds.
    pub registration_ttl_secs: u64,
}

impl Default for BootstrapBrokerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: None,
            base_url: None,
            registration_ttl_secs: 120,
        }
    }
}

impl BootstrapBrokerConfig {
    pub fn with_bind_addr(mut self, bind_addr: impl Into<String>) -> Self {
        self.bind_addr = Some(bind_addr.into());
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn registration_ttl(&self) -> Duration {
        Duration::from_secs(self.registration_ttl_secs.max(1))
    }
}

/// Wire-format registration payload stored by the bootstrap broker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapBrokerRegistration {
    pub authority_id: String,
    pub address: String,
    pub nickname_suggestion: Option<String>,
}

/// Typed broker candidate record returned by discovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapBrokerCandidateRecord {
    pub authority_id: String,
    pub address: String,
    pub nickname_suggestion: Option<String>,
    pub discovered_at_ms: u64,
}

impl BootstrapBrokerCandidateRecord {
    pub fn authority_id(&self) -> Option<AuthorityId> {
        AuthorityId::from_str(&self.authority_id).ok()
    }
}

/// Wire-format invitation payload relayed by the bootstrap broker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapBrokerInvitation {
    pub recipient_authority_id: String,
    pub invitation_code: String,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct BootstrapBrokerState {
    candidates: HashMap<String, BootstrapBrokerCandidateRecord>,
    invitations: HashMap<String, Vec<String>>,
    started_at: Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl BootstrapBrokerState {
    fn new() -> Self {
        Self {
            candidates: HashMap::new(),
            invitations: HashMap::new(),
            started_at: Instant::now(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
#[aura_macros::actor_root(
    owner = "bootstrap_broker_service",
    domain = "bootstrap_discovery",
    supervision = "bootstrap_broker_task_root",
    category = "actor_owned"
)]
pub struct LocalBootstrapBrokerService {
    listener: Arc<TcpListener>,
    public_url: String,
    registration_ttl: Duration,
    shared: Arc<RwLock<BootstrapBrokerState>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl LocalBootstrapBrokerService {
    pub async fn bind(bind_addr: &str, registration_ttl: Duration) -> Result<Self, String> {
        let listener = TcpListener::bind(bind_addr)
            .await
            .map_err(|error| format!("bootstrap broker bind failed ({bind_addr}): {error}"))?;
        let local_addr = listener
            .local_addr()
            .map_err(|error| format!("bootstrap broker local addr lookup failed: {error}"))?;
        let public_url = format!("http://{}", local_addr);
        Ok(Self {
            listener: Arc::new(listener),
            public_url,
            registration_ttl,
            shared: Arc::new(RwLock::new(BootstrapBrokerState::new())),
        })
    }

    pub fn public_url(&self) -> &str {
        &self.public_url
    }

    pub async fn register(&self, registration: BootstrapBrokerRegistration) {
        let now_ms = broker_elapsed_ms(&self.shared).await;
        let key = format!("{}@{}", registration.authority_id, registration.address);
        self.shared.write().await.candidates.insert(
            key,
            BootstrapBrokerCandidateRecord {
                authority_id: registration.authority_id,
                address: registration.address,
                nickname_suggestion: registration.nickname_suggestion,
                discovered_at_ms: now_ms,
            },
        );
    }

    pub async fn list_candidates(&self) -> Vec<BootstrapBrokerCandidateRecord> {
        let now_ms = broker_elapsed_ms(&self.shared).await;
        prune_candidates(&self.shared, now_ms, self.registration_ttl).await;
        self.shared
            .read()
            .await
            .candidates
            .values()
            .cloned()
            .collect()
    }

    pub async fn queue_invitation(&self, invitation: BootstrapBrokerInvitation) {
        self.shared
            .write()
            .await
            .invitations
            .entry(invitation.recipient_authority_id)
            .or_default()
            .push(invitation.invitation_code);
    }

    pub async fn take_invitations(&self, authority_id: &str) -> Vec<String> {
        self.shared
            .write()
            .await
            .invitations
            .remove(authority_id)
            .unwrap_or_default()
    }

    pub fn start(&self, tasks: &TaskGroup) {
        let listener = self.listener.clone();
        let shared = self.shared.clone();
        let registration_ttl = self.registration_ttl;
        let accept_tasks = tasks.clone();
        let connection_tasks = tasks.clone();
        let _bootstrap_broker_handle =
            accept_tasks.spawn_named("bootstrap_broker_http", async move {
                loop {
                    let Ok((stream, _addr)) = listener.accept().await else {
                        break;
                    };
                    let shared = shared.clone();
                    let connection_tasks = connection_tasks.clone();
                    let _conn_handle =
                        connection_tasks.spawn_named("bootstrap_broker_conn", async move {
                            handle_http_connection(stream, shared, registration_ttl).await;
                        });
                }
            });
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn prune_candidates(
    shared: &Arc<RwLock<BootstrapBrokerState>>,
    now_ms: u64,
    registration_ttl: Duration,
) {
    let max_age_ms = registration_ttl.as_millis() as u64;
    shared
        .write()
        .await
        .candidates
        .retain(|_, candidate| now_ms.saturating_sub(candidate.discovered_at_ms) < max_age_ms);
}

#[cfg(not(target_arch = "wasm32"))]
async fn handle_http_connection(
    mut stream: TcpStream,
    shared: Arc<RwLock<BootstrapBrokerState>>,
    registration_ttl: Duration,
) {
    let request = match read_http_request(&mut stream).await {
        Ok(request) => request,
        Err(error) => {
            let _ = write_http_response(
                &mut stream,
                400,
                "text/plain; charset=utf-8",
                error.as_bytes(),
            )
            .await;
            return;
        }
    };

    match (request.method.as_str(), request.path.as_str()) {
        ("OPTIONS", _) => {
            let _ = write_http_response(&mut stream, 204, "text/plain; charset=utf-8", b"").await;
        }
        ("POST", "/v1/bootstrap/register") => {
            let registration: BootstrapBrokerRegistration =
                match serde_json::from_slice(&request.body) {
                    Ok(registration) => registration,
                    Err(error) => {
                        let body = format!("invalid registration payload: {error}");
                        let _ = write_http_response(
                            &mut stream,
                            400,
                            "text/plain; charset=utf-8",
                            body.as_bytes(),
                        )
                        .await;
                        return;
                    }
                };

            let now_ms = broker_elapsed_ms(&shared).await;
            let key = format!("{}@{}", registration.authority_id, registration.address);
            shared.write().await.candidates.insert(
                key,
                BootstrapBrokerCandidateRecord {
                    authority_id: registration.authority_id,
                    address: registration.address,
                    nickname_suggestion: registration.nickname_suggestion,
                    discovered_at_ms: now_ms,
                },
            );
            let _ = write_http_response(&mut stream, 204, "text/plain; charset=utf-8", b"").await;
        }
        ("GET", "/v1/bootstrap/candidates") => {
            let now_ms = broker_elapsed_ms(&shared).await;
            prune_candidates(&shared, now_ms, registration_ttl).await;
            let candidates: Vec<_> = shared.read().await.candidates.values().cloned().collect();
            match serde_json::to_vec(&candidates) {
                Ok(body) => {
                    let _ = write_http_response(&mut stream, 200, "application/json", &body).await;
                }
                Err(error) => {
                    let body = format!("failed to encode candidates: {error}");
                    let _ = write_http_response(
                        &mut stream,
                        500,
                        "text/plain; charset=utf-8",
                        body.as_bytes(),
                    )
                    .await;
                }
            }
        }
        ("POST", "/v1/bootstrap/invitations") => {
            let invitation: BootstrapBrokerInvitation = match serde_json::from_slice(&request.body)
            {
                Ok(invitation) => invitation,
                Err(error) => {
                    let body = format!("invalid invitation payload: {error}");
                    let _ = write_http_response(
                        &mut stream,
                        400,
                        "text/plain; charset=utf-8",
                        body.as_bytes(),
                    )
                    .await;
                    return;
                }
            };
            shared
                .write()
                .await
                .invitations
                .entry(invitation.recipient_authority_id)
                .or_default()
                .push(invitation.invitation_code);
            let _ = write_http_response(&mut stream, 204, "text/plain; charset=utf-8", b"").await;
        }
        ("GET", path) if path.starts_with("/v1/bootstrap/invitations/") => {
            let authority_id = path.trim_start_matches("/v1/bootstrap/invitations/");
            let invitations = shared
                .write()
                .await
                .invitations
                .remove(authority_id)
                .unwrap_or_default();
            match serde_json::to_vec(&invitations) {
                Ok(body) => {
                    let _ = write_http_response(&mut stream, 200, "application/json", &body).await;
                }
                Err(error) => {
                    let body = format!("failed to encode invitations: {error}");
                    let _ = write_http_response(
                        &mut stream,
                        500,
                        "text/plain; charset=utf-8",
                        body.as_bytes(),
                    )
                    .await;
                }
            }
        }
        _ => {
            let _ =
                write_http_response(&mut stream, 404, "text/plain; charset=utf-8", b"not found")
                    .await;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

#[cfg(not(target_arch = "wasm32"))]
async fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut buffer = Vec::new();
    let mut scratch = [0_u8; 2048];
    let header_end = loop {
        let read = stream
            .read(&mut scratch)
            .await
            .map_err(|error| format!("read failed: {error}"))?;
        if read == 0 {
            return Err("request ended before headers were complete".to_string());
        }
        buffer.extend_from_slice(&scratch[..read]);
        if let Some(end) = buffer.windows(4).position(|chunk| chunk == b"\r\n\r\n") {
            break end + 4;
        }
        if buffer.len() > 64 * 1024 {
            return Err("request headers exceed 64KiB".to_string());
        }
    };

    let header_text = std::str::from_utf8(&buffer[..header_end])
        .map_err(|error| format!("header utf-8 decode failed: {error}"))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| "missing request line".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "missing request method".to_string())?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| "missing request path".to_string())?
        .to_string();
    let content_length = lines
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    let mut body = buffer[header_end..].to_vec();
    while body.len() < content_length {
        let read = stream
            .read(&mut scratch)
            .await
            .map_err(|error| format!("body read failed: {error}"))?;
        if read == 0 {
            return Err("request body truncated".to_string());
        }
        body.extend_from_slice(&scratch[..read]);
    }
    body.truncate(content_length);

    Ok(HttpRequest { method, path, body })
}

#[cfg(not(target_arch = "wasm32"))]
async fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), std::io::Error> {
    let status_text = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let headers = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

#[cfg(not(target_arch = "wasm32"))]
async fn broker_elapsed_ms(shared: &Arc<RwLock<BootstrapBrokerState>>) -> u64 {
    shared.read().await.started_at.elapsed().as_millis() as u64
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}

pub fn register_endpoint(base_url: &str) -> String {
    format!("{}/v1/bootstrap/register", normalize_base_url(base_url))
}

pub fn candidates_endpoint(base_url: &str) -> String {
    format!("{}/v1/bootstrap/candidates", normalize_base_url(base_url))
}

pub fn invitations_endpoint(base_url: &str, authority_id: &str) -> String {
    format!(
        "{}/v1/bootstrap/invitations/{}",
        normalize_base_url(base_url),
        authority_id
    )
}

pub fn endpoint_is_loopback(raw: &str) -> bool {
    let raw = raw
        .strip_prefix("http://")
        .or_else(|| raw.strip_prefix("https://"))
        .unwrap_or(raw);
    let host = raw
        .split('/')
        .next()
        .unwrap_or(raw)
        .split(':')
        .next()
        .unwrap_or(raw);
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|addr| addr.is_loopback())
            .unwrap_or(false)
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_http_endpoint(url: &str) -> Result<(String, String), String> {
    let without_scheme = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("unsupported bootstrap broker url (expected http://): {url}"))?;
    let (host_port, path) = match without_scheme.split_once('/') {
        Some((host_port, path)) => (host_port.to_string(), format!("/{}", path)),
        None => (without_scheme.to_string(), "/".to_string()),
    };
    if host_port.is_empty() {
        return Err(format!("bootstrap broker url missing host: {url}"));
    }
    Ok((host_port, path))
}

#[cfg(not(target_arch = "wasm32"))]
async fn send_native_http_request(
    method: &str,
    url: &str,
    body: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let (host_port, path) = parse_http_endpoint(url)?;
    let mut stream = TcpStream::connect(&host_port)
        .await
        .map_err(|error| format!("bootstrap broker connect failed ({host_port}): {error}"))?;
    let body = body.unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| format!("bootstrap broker request write failed: {error}"))?;
    if !body.is_empty() {
        stream
            .write_all(body)
            .await
            .map_err(|error| format!("bootstrap broker body write failed: {error}"))?;
    }
    stream
        .flush()
        .await
        .map_err(|error| format!("bootstrap broker flush failed: {error}"))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .map_err(|error| format!("bootstrap broker response read failed: {error}"))?;
    let header_end = response
        .windows(4)
        .position(|chunk| chunk == b"\r\n\r\n")
        .map(|offset| offset + 4)
        .ok_or_else(|| "bootstrap broker response missing headers".to_string())?;
    let header_text = std::str::from_utf8(&response[..header_end])
        .map_err(|error| format!("bootstrap broker header decode failed: {error}"))?;
    let status = header_text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "bootstrap broker response missing status".to_string())?;
    if !(200..300).contains(&status) {
        let body_text = String::from_utf8_lossy(&response[header_end..]).to_string();
        return Err(format!("bootstrap broker returned {status}: {body_text}"));
    }
    Ok(response[header_end..].to_vec())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn register_remote_candidate(
    base_url: &str,
    registration: &BootstrapBrokerRegistration,
) -> Result<(), String> {
    let body = serde_json::to_vec(registration)
        .map_err(|error| format!("serialize registration failed: {error}"))?;
    let _ = send_native_http_request("POST", &register_endpoint(base_url), Some(&body)).await?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_remote_candidates(
    base_url: &str,
) -> Result<Vec<BootstrapBrokerCandidateRecord>, String> {
    let body = send_native_http_request("GET", &candidates_endpoint(base_url), None).await?;
    serde_json::from_slice(&body)
        .map_err(|error| format!("broker candidate decode failed: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn send_remote_invitation(
    base_url: &str,
    invitation: &BootstrapBrokerInvitation,
) -> Result<(), String> {
    let body = serde_json::to_vec(invitation)
        .map_err(|error| format!("serialize invitation failed: {error}"))?;
    let _ = send_native_http_request(
        "POST",
        &format!("{}/v1/bootstrap/invitations", normalize_base_url(base_url)),
        Some(&body),
    )
    .await?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn take_remote_invitations(
    base_url: &str,
    authority_id: &str,
) -> Result<Vec<String>, String> {
    let body = send_native_http_request("GET", &invitations_endpoint(base_url, authority_id), None)
        .await?;
    serde_json::from_slice(&body)
        .map_err(|error| format!("broker invitation decode failed: {error}"))
}

#[cfg(target_arch = "wasm32")]
pub async fn register_remote_candidate(
    base_url: &str,
    registration: &BootstrapBrokerRegistration,
) -> Result<(), String> {
    let body = serde_json::to_string(registration)
        .map_err(|error| format!("serialize registration failed: {error}"))?;
    let request = gloo_net::http::Request::post(&register_endpoint(base_url))
        .header("content-type", "application/json")
        .body(body)
        .map_err(|error| format!("build broker register request failed: {error}"))?;
    match request.send().await {
        Ok(_) => Ok(()),
        Err(error) if endpoint_is_loopback(base_url) => Ok(()),
        Err(error) => Err(format!("broker register request failed: {error}")),
    }
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_remote_candidates(
    base_url: &str,
) -> Result<Vec<BootstrapBrokerCandidateRecord>, String> {
    let request = gloo_net::http::Request::get(&candidates_endpoint(base_url));
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) if endpoint_is_loopback(base_url) => return Ok(Vec::new()),
        Err(error) => return Err(format!("broker candidate request failed: {error}")),
    };
    response
        .json::<Vec<BootstrapBrokerCandidateRecord>>()
        .await
        .map_err(|error| format!("broker candidate decode failed: {error}"))
}

#[cfg(target_arch = "wasm32")]
pub async fn send_remote_invitation(
    base_url: &str,
    invitation: &BootstrapBrokerInvitation,
) -> Result<(), String> {
    let body = serde_json::to_string(invitation)
        .map_err(|error| format!("serialize invitation failed: {error}"))?;
    gloo_net::http::Request::post(&format!(
        "{}/v1/bootstrap/invitations",
        normalize_base_url(base_url)
    ))
    .header("content-type", "application/json")
    .body(body)
    .map_err(|error| format!("build broker invitation request failed: {error}"))?
    .send()
    .await
    .map_err(|error| format!("broker invitation request failed: {error}"))?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub async fn take_remote_invitations(
    base_url: &str,
    authority_id: &str,
) -> Result<Vec<String>, String> {
    let request = gloo_net::http::Request::get(&invitations_endpoint(base_url, authority_id));
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) if endpoint_is_loopback(base_url) => return Ok(Vec::new()),
        Err(error) => return Err(format!("broker invitation request failed: {error}")),
    };
    response
        .json::<Vec<String>>()
        .await
        .map_err(|error| format!("broker invitation decode failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    use crate::runtime::TaskSupervisor;

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn local_bootstrap_broker_registers_and_lists_candidates() {
        let broker = LocalBootstrapBrokerService::bind("127.0.0.1:0", Duration::from_secs(60))
            .await
            .expect("broker should bind");
        broker
            .register(BootstrapBrokerRegistration {
                authority_id: "01234567-89ab-cdef-0123-456789abcdef".to_string(),
                address: "127.0.0.1:40123".to_string(),
                nickname_suggestion: Some("Alice".to_string()),
            })
            .await;

        let candidates = broker.list_candidates().await;
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].authority_id,
            "01234567-89ab-cdef-0123-456789abcdef"
        );
        assert_eq!(candidates[0].address, "127.0.0.1:40123");
        assert_eq!(candidates[0].nickname_suggestion.as_deref(), Some("Alice"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn native_client_round_trips_against_local_broker_http() {
        let broker = LocalBootstrapBrokerService::bind("127.0.0.1:0", Duration::from_secs(60))
            .await
            .expect("broker should bind");
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_test"));
        tokio::task::yield_now().await;

        register_remote_candidate(
            broker.public_url(),
            &BootstrapBrokerRegistration {
                authority_id: "89abcdef-0123-4567-89ab-cdef01234567".to_string(),
                address: "127.0.0.1:40124".to_string(),
                nickname_suggestion: Some("Browser".to_string()),
            },
        )
        .await
        .expect("remote client should register");

        let candidates = fetch_remote_candidates(broker.public_url())
            .await
            .expect("remote client should list");
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].authority_id,
            "89abcdef-0123-4567-89ab-cdef01234567"
        );
        assert_eq!(candidates[0].address, "127.0.0.1:40124");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn native_client_round_trips_broker_invitations() {
        let broker = LocalBootstrapBrokerService::bind("127.0.0.1:0", Duration::from_secs(60))
            .await
            .expect("broker should bind");
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_invitation_test"));
        tokio::task::yield_now().await;

        send_remote_invitation(
            broker.public_url(),
            &BootstrapBrokerInvitation {
                recipient_authority_id: "fedcba98-7654-3210-fedc-ba9876543210".to_string(),
                invitation_code: "invite-123".to_string(),
            },
        )
        .await
        .expect("remote invitation send should succeed");

        let invitations =
            take_remote_invitations(broker.public_url(), "fedcba98-7654-3210-fedc-ba9876543210")
                .await
                .expect("remote invitation take should succeed");
        assert_eq!(invitations, vec!["invite-123".to_string()]);

        let emptied =
            take_remote_invitations(broker.public_url(), "fedcba98-7654-3210-fedc-ba9876543210")
                .await
                .expect("second take should succeed");
        assert!(emptied.is_empty());
    }
}
