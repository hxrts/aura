use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::net::IpAddr;
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
use tokio::sync::{RwLock, Semaphore};
#[cfg(not(target_arch = "wasm32"))]
use tokio::time::{timeout, Instant};

const DEFAULT_LOOPBACK_BIND_ADDR: &str = "127.0.0.1:0";
#[cfg(not(target_arch = "wasm32"))]
const AUTHORIZATION_HEADER: &str = "authorization";
#[cfg(not(target_arch = "wasm32"))]
const INVITATION_RETRIEVAL_HEADER: &str = "x-aura-invitation-retrieval-token";
#[cfg(not(target_arch = "wasm32"))]
const BEARER_PREFIX: &str = "Bearer ";
#[cfg(not(target_arch = "wasm32"))]
const MAX_BOOTSTRAP_BODY_BYTES: usize = 16 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const MAX_BOOTSTRAP_CANDIDATES: usize = 256;
#[cfg(not(target_arch = "wasm32"))]
const MAX_PENDING_INVITATIONS: usize = 256;
#[cfg(not(target_arch = "wasm32"))]
const MAX_INVITATIONS_PER_RECIPIENT: usize = 16;
#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_MAX_BOOTSTRAP_CONNECTIONS: usize = 64;
#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_BOOTSTRAP_REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Resource limits for the local bootstrap broker HTTP service.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapBrokerLimits {
    /// Maximum number of in-flight HTTP connections served by the broker.
    pub max_connections: usize,
    /// Deadline for reading each request header and body chunk.
    pub request_read_timeout: Duration,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for BootstrapBrokerLimits {
    fn default() -> Self {
        Self {
            max_connections: DEFAULT_MAX_BOOTSTRAP_CONNECTIONS,
            request_read_timeout: DEFAULT_BOOTSTRAP_REQUEST_READ_TIMEOUT,
        }
    }
}

/// Explicit policy for native bootstrap broker bind exposure.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapBrokerLanBindPolicy {
    /// Broker binds must remain loopback-only.
    #[default]
    LoopbackOnly,
    /// Development-only exception allowing LAN-visible binds with bearer auth.
    AllowLanDevOnly,
}

/// Broker configuration for mixed native/browser bootstrap discovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapBrokerConfig {
    /// Whether broker-backed bootstrap discovery is enabled.
    pub enabled: bool,
    /// Native bind address for hosting a broker. Defaults to loopback when the
    /// native broker is enabled without a remote base URL.
    pub bind_addr: Option<String>,
    /// LAN-visible HTTP brokers are development-only until TLS is added. They
    /// require explicit opt-in plus bearer-token authentication.
    pub lan_bind_policy: BootstrapBrokerLanBindPolicy,
    /// Broker base URL used by runtimes that act as clients only.
    pub base_url: Option<String>,
    /// Bearer token required by all HTTP broker endpoints.
    pub auth_token: Option<String>,
    /// Unguessable one-time credential used by a recipient to drain queued
    /// invitations. It is registered with the broker but never returned by the
    /// candidate listing API.
    pub invitation_retrieval_token: Option<String>,
    /// Registration time-to-live in seconds.
    pub registration_ttl_secs: u64,
}

impl Default for BootstrapBrokerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: None,
            lan_bind_policy: BootstrapBrokerLanBindPolicy::LoopbackOnly,
            base_url: None,
            auth_token: None,
            invitation_retrieval_token: None,
            registration_ttl_secs: 120,
        }
    }
}

impl BootstrapBrokerConfig {
    pub fn with_bind_addr(mut self, bind_addr: impl Into<String>) -> Self {
        self.bind_addr = Some(bind_addr.into());
        self
    }

    pub fn with_lan_bind_policy(mut self, policy: BootstrapBrokerLanBindPolicy) -> Self {
        self.lan_bind_policy = policy;
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    pub fn with_auth_token(mut self, auth_token: impl Into<String>) -> Self {
        self.auth_token = Some(auth_token.into());
        self
    }

    pub fn with_invitation_retrieval_token(mut self, token: impl Into<String>) -> Self {
        self.invitation_retrieval_token = Some(token.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn registration_ttl(&self) -> Duration {
        Duration::from_secs(self.registration_ttl_secs.max(1))
    }

    pub fn resolved_bind_addr(&self) -> Option<&str> {
        if !self.enabled || self.base_url.is_some() {
            return self.bind_addr.as_deref();
        }
        Some(
            self.bind_addr
                .as_deref()
                .unwrap_or(DEFAULT_LOOPBACK_BIND_ADDR),
        )
    }
}

/// Wire-format registration payload stored by the bootstrap broker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapBrokerRegistration {
    pub authority_id: String,
    pub address: String,
    pub invitation_retrieval_token: String,
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
    invitation_credentials: HashMap<String, String>,
    started_at: Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl BootstrapBrokerState {
    fn new() -> Self {
        Self {
            candidates: HashMap::new(),
            invitations: HashMap::new(),
            invitation_credentials: HashMap::new(),
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
    auth_token: String,
    shared: Arc<RwLock<BootstrapBrokerState>>,
    limits: BootstrapBrokerLimits,
    connection_permits: Arc<Semaphore>,
}

#[cfg(not(target_arch = "wasm32"))]
impl LocalBootstrapBrokerService {
    pub async fn bind(
        bind_addr: &str,
        registration_ttl: Duration,
        auth_token: impl Into<String>,
        lan_bind_policy: BootstrapBrokerLanBindPolicy,
    ) -> Result<Self, String> {
        Self::bind_with_limits(
            bind_addr,
            registration_ttl,
            auth_token,
            lan_bind_policy,
            BootstrapBrokerLimits::default(),
        )
        .await
    }

    pub async fn bind_with_limits(
        bind_addr: &str,
        registration_ttl: Duration,
        auth_token: impl Into<String>,
        lan_bind_policy: BootstrapBrokerLanBindPolicy,
        limits: BootstrapBrokerLimits,
    ) -> Result<Self, String> {
        validate_bind_addr(bind_addr, lan_bind_policy)?;
        let auth_token = auth_token.into();
        if auth_token.is_empty() {
            return Err("bootstrap broker auth token must not be empty".to_string());
        }
        if limits.max_connections == 0 {
            return Err("bootstrap broker max connections must be greater than zero".to_string());
        }
        if limits.request_read_timeout.is_zero() {
            return Err("bootstrap broker request read timeout must be non-zero".to_string());
        }
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
            auth_token,
            shared: Arc::new(RwLock::new(BootstrapBrokerState::new())),
            limits,
            connection_permits: Arc::new(Semaphore::new(limits.max_connections)),
        })
    }

    pub fn public_url(&self) -> &str {
        &self.public_url
    }

    pub async fn register(&self, registration: BootstrapBrokerRegistration) -> Result<(), String> {
        let now_ms = broker_elapsed_ms(&self.shared).await;
        prune_candidates(&self.shared, now_ms, self.registration_ttl).await;
        insert_registration(&self.shared, registration, now_ms).await
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

    pub async fn queue_invitation(
        &self,
        invitation: BootstrapBrokerInvitation,
    ) -> Result<(), String> {
        queue_invitation(&self.shared, invitation).await
    }

    pub async fn take_invitations(
        &self,
        authority_id: &str,
        retrieval_token: &str,
    ) -> Result<Vec<String>, String> {
        take_invitations(&self.shared, authority_id, retrieval_token).await
    }

    pub fn start(&self, tasks: &TaskGroup) {
        let listener = self.listener.clone();
        let shared = self.shared.clone();
        let registration_ttl = self.registration_ttl;
        let auth_token = self.auth_token.clone();
        let limits = self.limits;
        let permits = self.connection_permits.clone();
        let accept_tasks = tasks.clone();
        let connection_tasks = tasks.clone();
        let _bootstrap_broker_handle =
            accept_tasks.spawn_named("bootstrap_broker_http", async move {
                loop {
                    let Ok((mut stream, _addr)) = listener.accept().await else {
                        break;
                    };
                    let Ok(permit) = permits.clone().try_acquire_owned() else {
                        let _ = write_http_response(
                            &mut stream,
                            429,
                            "text/plain; charset=utf-8",
                            b"too many bootstrap broker connections",
                        )
                        .await;
                        continue;
                    };
                    let shared = shared.clone();
                    let auth_token = auth_token.clone();
                    let connection_tasks = connection_tasks.clone();
                    let _conn_handle =
                        connection_tasks.spawn_named("bootstrap_broker_conn", async move {
                            let _permit = permit;
                            handle_http_connection(
                                stream,
                                shared,
                                registration_ttl,
                                auth_token,
                                limits.request_read_timeout,
                            )
                            .await;
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
fn validate_bind_addr(
    bind_addr: &str,
    lan_bind_policy: BootstrapBrokerLanBindPolicy,
) -> Result<(), String> {
    let host = bind_addr
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(bind_addr)
        .trim_matches(['[', ']']);
    let is_loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|addr| addr.is_loopback())
            .unwrap_or(false);
    if !is_loopback && lan_bind_policy != BootstrapBrokerLanBindPolicy::AllowLanDevOnly {
        return Err(
            "bootstrap broker LAN-visible bind requires explicit AllowLanDevOnly policy"
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
async fn insert_registration(
    shared: &Arc<RwLock<BootstrapBrokerState>>,
    registration: BootstrapBrokerRegistration,
    now_ms: u64,
) -> Result<(), String> {
    if registration.invitation_retrieval_token.is_empty() {
        return Err("bootstrap broker invitation retrieval token must not be empty".to_string());
    }
    let key = format!("{}@{}", registration.authority_id, registration.address);
    let mut state = shared.write().await;
    if !state.candidates.contains_key(&key) && state.candidates.len() >= MAX_BOOTSTRAP_CANDIDATES {
        if let Some(oldest_key) = state
            .candidates
            .iter()
            .min_by_key(|(_, candidate)| candidate.discovered_at_ms)
            .map(|(key, _)| key.clone())
        {
            state.candidates.remove(&oldest_key);
        }
    }
    state.invitation_credentials.insert(
        registration.authority_id.clone(),
        registration.invitation_retrieval_token,
    );
    state.candidates.insert(
        key,
        BootstrapBrokerCandidateRecord {
            authority_id: registration.authority_id,
            address: registration.address,
            nickname_suggestion: registration.nickname_suggestion,
            discovered_at_ms: now_ms,
        },
    );
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
async fn queue_invitation(
    shared: &Arc<RwLock<BootstrapBrokerState>>,
    invitation: BootstrapBrokerInvitation,
) -> Result<(), String> {
    let mut state = shared.write().await;
    let pending_total: usize = state.invitations.values().map(Vec::len).sum();
    let recipient_pending = state
        .invitations
        .get(&invitation.recipient_authority_id)
        .map(Vec::len)
        .unwrap_or(0);
    if pending_total >= MAX_PENDING_INVITATIONS {
        return Err("bootstrap broker pending invitation limit reached".to_string());
    }
    if recipient_pending >= MAX_INVITATIONS_PER_RECIPIENT {
        return Err("bootstrap broker per-recipient invitation limit reached".to_string());
    }
    state
        .invitations
        .entry(invitation.recipient_authority_id)
        .or_default()
        .push(invitation.invitation_code);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
async fn take_invitations(
    shared: &Arc<RwLock<BootstrapBrokerState>>,
    authority_id: &str,
    retrieval_token: &str,
) -> Result<Vec<String>, String> {
    let mut state = shared.write().await;
    match state.invitation_credentials.get(authority_id) {
        Some(expected) if constant_time_eq_str(expected, retrieval_token) => {}
        _ => return Err("bootstrap broker invitation credential rejected".to_string()),
    }
    let invitations = state.invitations.remove(authority_id).unwrap_or_default();
    if !invitations.is_empty() {
        state.invitation_credentials.remove(authority_id);
    }
    Ok(invitations)
}

#[cfg(not(target_arch = "wasm32"))]
fn request_is_authorized(request: &HttpRequest, auth_token: &str) -> bool {
    request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(AUTHORIZATION_HEADER))
        .map(|(_, value)| {
            let expected = format!("{BEARER_PREFIX}{auth_token}");
            constant_time_eq_str(value.trim(), &expected)
        })
        .unwrap_or(false)
}

#[cfg(not(target_arch = "wasm32"))]
fn constant_time_eq_str(candidate: &str, expected: &str) -> bool {
    let candidate = candidate.as_bytes();
    let expected = expected.as_bytes();
    let max_len = candidate.len().max(expected.len());
    let mut diff = candidate.len() ^ expected.len();
    for idx in 0..max_len {
        let left = candidate.get(idx).copied().unwrap_or(0);
        let right = expected.get(idx).copied().unwrap_or(0);
        diff |= usize::from(left ^ right);
    }
    diff == 0
}

#[cfg(not(target_arch = "wasm32"))]
async fn handle_http_connection(
    mut stream: TcpStream,
    shared: Arc<RwLock<BootstrapBrokerState>>,
    registration_ttl: Duration,
    auth_token: String,
    request_read_timeout: Duration,
) {
    let request = match read_http_request(&mut stream, request_read_timeout).await {
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
        _ if !request_is_authorized(&request, &auth_token) => {
            let _ = write_http_response(
                &mut stream,
                401,
                "text/plain; charset=utf-8",
                b"unauthorized",
            )
            .await;
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
            prune_candidates(&shared, now_ms, registration_ttl).await;
            if let Err(error) = insert_registration(&shared, registration, now_ms).await {
                let _ = write_http_response(
                    &mut stream,
                    429,
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                )
                .await;
                return;
            }
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
            if let Err(error) = queue_invitation(&shared, invitation).await {
                let _ = write_http_response(
                    &mut stream,
                    429,
                    "text/plain; charset=utf-8",
                    error.as_bytes(),
                )
                .await;
                return;
            }
            let _ = write_http_response(&mut stream, 204, "text/plain; charset=utf-8", b"").await;
        }
        ("GET", path) if path.starts_with("/v1/bootstrap/invitations/") => {
            let Some(authority_id) =
                invitation_request_authority(path.trim_start_matches("/v1/bootstrap/invitations/"))
            else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"missing invitation authority",
                )
                .await;
                return;
            };
            let Some(retrieval_token) = request_header(&request, INVITATION_RETRIEVAL_HEADER)
            else {
                let _ = write_http_response(
                    &mut stream,
                    401,
                    "text/plain; charset=utf-8",
                    b"missing invitation retrieval credential",
                )
                .await;
                return;
            };
            let invitations = match take_invitations(&shared, authority_id, retrieval_token).await {
                Ok(invitations) => invitations,
                Err(error) => {
                    let _ = write_http_response(
                        &mut stream,
                        401,
                        "text/plain; charset=utf-8",
                        error.as_bytes(),
                    )
                    .await;
                    return;
                }
            };
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
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[cfg(not(target_arch = "wasm32"))]
fn invitation_request_authority(path_tail: &str) -> Option<&str> {
    if path_tail.is_empty() || path_tail.contains('?') {
        return None;
    }
    Some(path_tail)
}

#[cfg(not(target_arch = "wasm32"))]
fn request_header<'a>(request: &'a HttpRequest, header_name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(header_name))
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
}

#[cfg(not(target_arch = "wasm32"))]
async fn read_http_request(
    stream: &mut TcpStream,
    request_read_timeout: Duration,
) -> Result<HttpRequest, String> {
    let mut buffer = Vec::new();
    let mut scratch = [0_u8; 2048];
    let header_end = loop {
        let read = timeout(request_read_timeout, stream.read(&mut scratch))
            .await
            .map_err(|_| "request header read timed out".to_string())?
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
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    let content_length = headers
        .iter()
        .find_map(|(name, value)| {
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    if content_length > MAX_BOOTSTRAP_BODY_BYTES {
        return Err("request body exceeds bootstrap broker limit".to_string());
    }

    let mut body = buffer[header_end..].to_vec();
    if body.len() > MAX_BOOTSTRAP_BODY_BYTES {
        return Err("request body exceeds bootstrap broker limit".to_string());
    }
    while body.len() < content_length {
        let read = timeout(request_read_timeout, stream.read(&mut scratch))
            .await
            .map_err(|_| "request body read timed out".to_string())?
            .map_err(|error| format!("body read failed: {error}"))?;
        if read == 0 {
            return Err("request body truncated".to_string());
        }
        body.extend_from_slice(&scratch[..read]);
        if body.len() > MAX_BOOTSTRAP_BODY_BYTES {
            return Err("request body exceeds bootstrap broker limit".to_string());
        }
    }
    body.truncate(content_length);

    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
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
        401 => "Unauthorized",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let headers = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\n\r\n",
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
    if url.starts_with("https://") {
        return Err(format!(
            "unsupported native bootstrap broker url (HTTPS client not available in this build): {url}"
        ));
    }
    let without_scheme = url.strip_prefix("http://").ok_or_else(|| {
        format!("unsupported bootstrap broker url (expected http:// loopback): {url}")
    })?;
    if !endpoint_is_loopback(url) {
        return Err(format!(
            "plain HTTP bootstrap broker URLs are restricted to loopback endpoints: {url}"
        ));
    }
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
    auth_token: &str,
    extra_headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let (host_port, path) = parse_http_endpoint(url)?;
    let mut stream = TcpStream::connect(&host_port)
        .await
        .map_err(|error| format!("bootstrap broker connect failed ({host_port}): {error}"))?;
    let body = body.unwrap_or_default();
    let mut extra_header_text = String::new();
    for (name, value) in extra_headers {
        extra_header_text.push_str(&format!("{name}: {value}\r\n"));
    }
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host_port}\r\nAuthorization: Bearer {auth_token}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/json\r\n{extra_header_text}\r\n",
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
    auth_token: &str,
    registration: &BootstrapBrokerRegistration,
) -> Result<(), String> {
    let body = serde_json::to_vec(registration)
        .map_err(|error| format!("serialize registration failed: {error}"))?;
    let _ = send_native_http_request(
        "POST",
        &register_endpoint(base_url),
        auth_token,
        &[],
        Some(&body),
    )
    .await?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_remote_candidates(
    base_url: &str,
    auth_token: &str,
) -> Result<Vec<BootstrapBrokerCandidateRecord>, String> {
    let body =
        send_native_http_request("GET", &candidates_endpoint(base_url), auth_token, &[], None)
            .await?;
    serde_json::from_slice(&body)
        .map_err(|error| format!("broker candidate decode failed: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn send_remote_invitation(
    base_url: &str,
    auth_token: &str,
    invitation: &BootstrapBrokerInvitation,
) -> Result<(), String> {
    let body = serde_json::to_vec(invitation)
        .map_err(|error| format!("serialize invitation failed: {error}"))?;
    let _ = send_native_http_request(
        "POST",
        &format!("{}/v1/bootstrap/invitations", normalize_base_url(base_url)),
        auth_token,
        &[],
        Some(&body),
    )
    .await?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn take_remote_invitations(
    base_url: &str,
    auth_token: &str,
    authority_id: &str,
    retrieval_token: &str,
) -> Result<Vec<String>, String> {
    let body = send_native_http_request(
        "GET",
        &invitations_endpoint(base_url, authority_id),
        auth_token,
        &[(INVITATION_RETRIEVAL_HEADER, retrieval_token)],
        None,
    )
    .await?;
    serde_json::from_slice(&body)
        .map_err(|error| format!("broker invitation decode failed: {error}"))
}

#[cfg(target_arch = "wasm32")]
pub async fn register_remote_candidate(
    base_url: &str,
    auth_token: &str,
    registration: &BootstrapBrokerRegistration,
) -> Result<(), String> {
    let body = serde_json::to_string(registration)
        .map_err(|error| format!("serialize registration failed: {error}"))?;
    let request = gloo_net::http::Request::post(&register_endpoint(base_url))
        .header("content-type", "application/json")
        .header("authorization", &format!("Bearer {auth_token}"))
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
    auth_token: &str,
) -> Result<Vec<BootstrapBrokerCandidateRecord>, String> {
    let request = gloo_net::http::Request::get(&candidates_endpoint(base_url))
        .header("authorization", &format!("Bearer {auth_token}"));
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
    auth_token: &str,
    invitation: &BootstrapBrokerInvitation,
) -> Result<(), String> {
    let body = serde_json::to_string(invitation)
        .map_err(|error| format!("serialize invitation failed: {error}"))?;
    gloo_net::http::Request::post(&format!(
        "{}/v1/bootstrap/invitations",
        normalize_base_url(base_url)
    ))
    .header("content-type", "application/json")
    .header("authorization", &format!("Bearer {auth_token}"))
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
    auth_token: &str,
    authority_id: &str,
    retrieval_token: &str,
) -> Result<Vec<String>, String> {
    let request = gloo_net::http::Request::get(&invitations_endpoint(base_url, authority_id))
        .header("authorization", &format!("Bearer {auth_token}"))
        .header("x-aura-invitation-retrieval-token", retrieval_token);
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

    const AUTH_TOKEN: &str = "test-broker-auth-token";
    const INVITE_TOKEN: &str = "test-invitation-retrieval-token";

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn local_bootstrap_broker_registers_and_lists_candidates() {
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("broker should bind");
        broker
            .register(BootstrapBrokerRegistration {
                authority_id: "01234567-89ab-cdef-0123-456789abcdef".to_string(),
                address: "127.0.0.1:40123".to_string(),
                invitation_retrieval_token: INVITE_TOKEN.to_string(),
                nickname_suggestion: Some("Alice".to_string()),
            })
            .await
            .expect("registration should be accepted");

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
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("broker should bind");
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_test"));
        tokio::task::yield_now().await;

        register_remote_candidate(
            broker.public_url(),
            AUTH_TOKEN,
            &BootstrapBrokerRegistration {
                authority_id: "89abcdef-0123-4567-89ab-cdef01234567".to_string(),
                address: "127.0.0.1:40124".to_string(),
                invitation_retrieval_token: INVITE_TOKEN.to_string(),
                nickname_suggestion: Some("Browser".to_string()),
            },
        )
        .await
        .expect("remote client should register");

        let candidates = fetch_remote_candidates(broker.public_url(), AUTH_TOKEN)
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
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("broker should bind");
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_invitation_test"));
        tokio::task::yield_now().await;

        register_remote_candidate(
            broker.public_url(),
            AUTH_TOKEN,
            &BootstrapBrokerRegistration {
                authority_id: "fedcba98-7654-3210-fedc-ba9876543210".to_string(),
                address: "127.0.0.1:40125".to_string(),
                invitation_retrieval_token: INVITE_TOKEN.to_string(),
                nickname_suggestion: None,
            },
        )
        .await
        .expect("recipient registration should succeed");

        send_remote_invitation(
            broker.public_url(),
            AUTH_TOKEN,
            &BootstrapBrokerInvitation {
                recipient_authority_id: "fedcba98-7654-3210-fedc-ba9876543210".to_string(),
                invitation_code: "invite-123".to_string(),
            },
        )
        .await
        .expect("remote invitation send should succeed");

        let invitations = take_remote_invitations(
            broker.public_url(),
            AUTH_TOKEN,
            "fedcba98-7654-3210-fedc-ba9876543210",
            INVITE_TOKEN,
        )
        .await
        .expect("remote invitation take should succeed");
        assert_eq!(invitations, vec!["invite-123".to_string()]);
        assert!(
            !invitations_endpoint(broker.public_url(), "fedcba98-7654-3210-fedc-ba9876543210")
                .contains(INVITE_TOKEN)
        );

        let replay = take_remote_invitations(
            broker.public_url(),
            AUTH_TOKEN,
            "fedcba98-7654-3210-fedc-ba9876543210",
            INVITE_TOKEN,
        )
        .await
        .expect_err("retrieval credential should be one-time after draining invitations");
        assert!(replay.contains("401"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn bootstrap_broker_rejects_query_string_invitation_credentials() {
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("broker should bind");
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_query_token_test"));
        tokio::task::yield_now().await;

        register_remote_candidate(
            broker.public_url(),
            AUTH_TOKEN,
            &BootstrapBrokerRegistration {
                authority_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
                address: "127.0.0.1:40126".to_string(),
                invitation_retrieval_token: INVITE_TOKEN.to_string(),
                nickname_suggestion: None,
            },
        )
        .await
        .expect("recipient registration should succeed");

        let query_url = format!(
            "{}/v1/bootstrap/invitations/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee?credential={}",
            normalize_base_url(broker.public_url()),
            INVITE_TOKEN
        );
        let error = send_native_http_request("GET", &query_url, AUTH_TOKEN, &[], None)
            .await
            .expect_err("query-string credential should be rejected");
        assert!(error.contains("400"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn bootstrap_broker_rejects_unauthorized_http_requests() {
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("broker should bind");
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_auth_test"));
        tokio::task::yield_now().await;

        let registration = serde_json::to_vec(&BootstrapBrokerRegistration {
            authority_id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_string(),
            address: "127.0.0.1:43000".to_string(),
            invitation_retrieval_token: INVITE_TOKEN.to_string(),
            nickname_suggestion: None,
        })
        .expect("registration should encode");
        let register_error = send_native_http_request(
            "POST",
            &register_endpoint(broker.public_url()),
            "wrong-token",
            &[],
            Some(&registration),
        )
        .await
        .expect_err("unauthorized register should be rejected");
        assert!(register_error.contains("401"));

        let list_error = fetch_remote_candidates(broker.public_url(), "wrong-token")
            .await
            .expect_err("unauthorized list should be rejected");
        assert!(list_error.contains("401"));

        let invitation = serde_json::to_vec(&BootstrapBrokerInvitation {
            recipient_authority_id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_string(),
            invitation_code: "invite-denied".to_string(),
        })
        .expect("invitation should encode");
        let queue_error = send_native_http_request(
            "POST",
            &format!(
                "{}/v1/bootstrap/invitations",
                normalize_base_url(broker.public_url())
            ),
            "wrong-token",
            &[],
            Some(&invitation),
        )
        .await
        .expect_err("unauthorized queue should be rejected");
        assert!(queue_error.contains("401"));

        let drain_error = take_remote_invitations(
            broker.public_url(),
            "wrong-token",
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            INVITE_TOKEN,
        )
        .await
        .expect_err("unauthorized drain should be rejected");
        assert!(drain_error.contains("401"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn bootstrap_broker_rejects_oversized_bodies() {
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("broker should bind");
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_body_limit_test"));
        tokio::task::yield_now().await;

        let (host_port, path) =
            parse_http_endpoint(&register_endpoint(broker.public_url())).expect("valid endpoint");
        let mut stream = TcpStream::connect(&host_port)
            .await
            .expect("broker should accept connections");
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nAuthorization: Bearer {AUTH_TOKEN}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n",
            MAX_BOOTSTRAP_BODY_BYTES + 1
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("request headers should write");
        stream.flush().await.expect("request should flush");
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .expect("response should read");
        let response = String::from_utf8_lossy(&response);
        assert!(response.starts_with("HTTP/1.1 400"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_bootstrap_broker_plain_http_is_loopback_only() {
        assert!(parse_http_endpoint("http://127.0.0.1:3000/v1/bootstrap").is_ok());
        assert!(parse_http_endpoint("http://localhost:3000/v1/bootstrap").is_ok());

        let error = parse_http_endpoint("http://192.0.2.10:3000/v1/bootstrap")
            .expect_err("non-loopback HTTP must fail closed");
        assert!(error.contains("plain HTTP bootstrap broker URLs are restricted to loopback"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_bootstrap_broker_https_is_not_downgraded_to_http() {
        let error = parse_http_endpoint("https://broker.example.test/v1/bootstrap")
            .expect_err("native HTTPS must not be parsed as plain HTTP");
        assert!(error.contains("HTTPS client not available"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn bootstrap_broker_bounds_concurrent_connections() {
        let broker = LocalBootstrapBrokerService::bind_with_limits(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
            BootstrapBrokerLimits {
                max_connections: 1,
                request_read_timeout: Duration::from_millis(250),
            },
        )
        .await
        .expect("broker should bind");
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_connection_limit_test"));
        tokio::task::yield_now().await;

        let (host_port, _) =
            parse_http_endpoint(&candidates_endpoint(broker.public_url())).expect("valid endpoint");
        let _held = TcpStream::connect(&host_port)
            .await
            .expect("first connection should be accepted and hold the permit");
        tokio::task::yield_now().await;

        let mut second = TcpStream::connect(&host_port)
            .await
            .expect("second connection should connect");
        let mut response = Vec::new();
        second
            .read_to_end(&mut response)
            .await
            .expect("limit response should read");
        let response = String::from_utf8_lossy(&response);
        assert!(response.starts_with("HTTP/1.1 429"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn bootstrap_broker_times_out_slow_headers_and_bodies() {
        let broker = LocalBootstrapBrokerService::bind_with_limits(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
            BootstrapBrokerLimits {
                max_connections: 4,
                request_read_timeout: Duration::from_millis(25),
            },
        )
        .await
        .expect("broker should bind");
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_read_timeout_test"));
        tokio::task::yield_now().await;

        let (host_port, path) =
            parse_http_endpoint(&register_endpoint(broker.public_url())).expect("valid endpoint");
        let mut header_stream = TcpStream::connect(&host_port)
            .await
            .expect("broker should accept slow header connection");
        header_stream
            .write_all(b"GET /v1/bootstrap/candidates HTTP/1.1\r\n")
            .await
            .expect("partial header should write");
        let mut response = Vec::new();
        header_stream
            .read_to_end(&mut response)
            .await
            .expect("timeout response should read");
        let response = String::from_utf8_lossy(&response);
        assert!(response.starts_with("HTTP/1.1 400"));
        assert!(response.contains("timed out"));

        let mut body_stream = TcpStream::connect(&host_port)
            .await
            .expect("broker should accept slow body connection");
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nAuthorization: Bearer {AUTH_TOKEN}\r\nConnection: close\r\nContent-Length: 4\r\nContent-Type: application/json\r\n\r\n{{"
        );
        body_stream
            .write_all(request.as_bytes())
            .await
            .expect("partial body should write");
        let mut response = Vec::new();
        body_stream
            .read_to_end(&mut response)
            .await
            .expect("timeout response should read");
        let response = String::from_utf8_lossy(&response);
        assert!(response.starts_with("HTTP/1.1 400"));
        assert!(response.contains("timed out"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn bootstrap_broker_prunes_stale_and_bounds_candidates() {
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_millis(1),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("broker should bind");
        broker
            .register(BootstrapBrokerRegistration {
                authority_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_string(),
                address: "127.0.0.1:41000".to_string(),
                invitation_retrieval_token: INVITE_TOKEN.to_string(),
                nickname_suggestion: None,
            })
            .await
            .expect("registration should be accepted");
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(broker.list_candidates().await.is_empty());

        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("broker should bind");
        for index in 0..(MAX_BOOTSTRAP_CANDIDATES + 8) {
            broker
                .register(BootstrapBrokerRegistration {
                    authority_id: format!("authority-{index}"),
                    address: format!("127.0.0.1:{}", 42000 + index),
                    invitation_retrieval_token: format!("invite-token-{index}"),
                    nickname_suggestion: None,
                })
                .await
                .expect("registration should stay bounded");
        }
        assert!(broker.list_candidates().await.len() <= MAX_BOOTSTRAP_CANDIDATES);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn bootstrap_broker_lan_bind_requires_explicit_opt_in() {
        let rejected = LocalBootstrapBrokerService::bind(
            "0.0.0.0:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect_err("LAN-visible bind should require explicit opt-in");
        assert!(rejected.contains("AllowLanDevOnly"));

        LocalBootstrapBrokerService::bind(
            "0.0.0.0:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::AllowLanDevOnly,
        )
        .await
        .expect("explicit LAN-visible bind should be accepted with auth");
    }
}
