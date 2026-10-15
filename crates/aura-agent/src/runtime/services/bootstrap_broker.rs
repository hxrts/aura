use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::net::IpAddr;
#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
use std::path::{Path, PathBuf};
use std::str::FromStr;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
mod mailbox;
#[cfg(not(target_arch = "wasm32"))]
mod return_mailbox;
#[cfg(not(target_arch = "wasm32"))]
use aura_core::effects::time::PhysicalTimeEffects;
#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
use mailbox::QuiescentPendingRegistration;
#[cfg(not(target_arch = "wasm32"))]
use mailbox::{PendingEnrollmentMailboxes, PendingMailboxKey};
#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
use return_mailbox::QuiescentReturnRegistration;
#[cfg(not(target_arch = "wasm32"))]
use return_mailbox::{EnrollmentReturnMailboxes, ReturnMailboxKey};

#[cfg(test)]
use aura_core::invitation::InvitationPurpose;
use aura_core::types::identifiers::{AuthorityId, DeviceId};

#[cfg(not(target_arch = "wasm32"))]
use crate::runtime::TaskGroup;

#[cfg(not(target_arch = "wasm32"))]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::{TcpListener, TcpStream};
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::{RwLock, Semaphore};
#[cfg(not(target_arch = "wasm32"))]
use tokio::time::timeout;

const DEFAULT_LOOPBACK_BIND_ADDR: &str = "127.0.0.1:0";
#[cfg(not(target_arch = "wasm32"))]
const AUTHORIZATION_HEADER: &str = "authorization";
const INVITATION_RETRIEVAL_HEADER: &str = "x-aura-invitation-retrieval-token";
const ENROLLMENT_MAILBOX_RETRIEVAL_HEADER: &str = "x-aura-enrollment-mailbox-retrieval-token";
const ENROLLMENT_RETURN_MAILBOX_RETRIEVAL_HEADER: &str =
    "x-aura-enrollment-return-mailbox-retrieval-token";
#[cfg(not(target_arch = "wasm32"))]
const BEARER_PREFIX: &str = "Bearer ";
#[cfg(not(target_arch = "wasm32"))]
const MAX_BOOTSTRAP_BODY_BYTES: usize = 16 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const MAX_ENROLLMENT_MAILBOX_BODY_BYTES: usize = 1_500_000;
#[cfg(not(target_arch = "wasm32"))]
const MAX_BOOTSTRAP_CANDIDATES: usize = 256;
#[cfg(not(target_arch = "wasm32"))]
const MAX_PENDING_INVITATIONS: usize = 256;
#[cfg(not(target_arch = "wasm32"))]
const MAX_INVITATIONS_PER_RECIPIENT: usize =
    aura_core::invitation::MAX_BOOTSTRAP_INVITATIONS_PER_RECIPIENT_COUNT;
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

/// Native broker client policy. Non-loopback plain HTTP is development-only.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapBrokerClientPolicy {
    /// Production default: only the local machine may receive plain HTTP.
    #[default]
    LoopbackOnly,
    /// Explicit test/LAN opt-in, restricted to numeric private IPv4 hosts.
    #[cfg(feature = "lan-http-dev")]
    AllowPrivateLanDevOnly,
}

impl BootstrapBrokerClientPolicy {
    #[cfg(not(target_arch = "wasm32"))]
    fn allows_private_lan(self) -> bool {
        #[cfg(feature = "lan-http-dev")]
        {
            matches!(self, Self::AllowPrivateLanDevOnly)
        }
        #[cfg(not(feature = "lan-http-dev"))]
        {
            false
        }
    }
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
    /// Explicit development-only opt-in for native clients using private LAN HTTP.
    #[serde(default)]
    pub client_policy: BootstrapBrokerClientPolicy,
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
            client_policy: BootstrapBrokerClientPolicy::LoopbackOnly,
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

    pub fn with_client_policy(mut self, policy: BootstrapBrokerClientPolicy) -> Self {
        self.client_policy = policy;
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
    pub device_id: DeviceId,
    pub address: String,
    pub invitation_retrieval_token: String,
    pub nickname_suggestion: Option<String>,
}

/// Typed broker candidate record returned by discovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapBrokerCandidateRecord {
    pub authority_id: String,
    pub device_id: DeviceId,
    pub address: String,
    pub nickname_suggestion: Option<String>,
    pub discovered_at_ms: u64,
}

/// Untrusted wire registration plus Bob's scoped retrieval credential. The
/// broker stores only its hash; Alice verifies the signature with the retained
/// original setup verifier before any VM frame is published.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapPendingEnrollmentMailboxRegistration {
    pub registration: aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxRegistration,
    pub retrieval_credential: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapPendingEnrollmentMailboxEnvelope {
    pub manifest_digest: [u8; 32],
    pub envelope: aura_core::effects::transport::TransportEnvelope,
}

/// Untrusted public issuer proof plus an independent Alice-only retrieval
/// credential. Neither broker registration nor its receipt grants membership.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapIssuerReturnMailboxRegistration {
    pub registration: aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration,
    pub retrieval_credential: String,
}

/// The original Bob setup request proves possession only; Alice still checks
/// it against protected admission and verifies the resulting VM frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapIssuerReturnMailboxEnvelope {
    pub manifest_digest: [u8; 32],
    pub setup_request: aura_invitation::enrollment_setup::DeviceEnrollmentSetupRequest,
    pub authorization: aura_invitation::enrollment_mailbox::EnrollmentReturnEnvelopeAuthorization,
    pub envelope: aura_core::effects::transport::TransportEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IssuerReturnMailboxDelivery {
    pub(crate) message_id: String,
    pub(crate) setup_request: aura_invitation::enrollment_setup::DeviceEnrollmentSetupRequest,
    pub(crate) authorization:
        aura_invitation::enrollment_mailbox::EnrollmentReturnEnvelopeAuthorization,
    pub(crate) envelope: aura_core::effects::transport::TransportEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingMailboxDelivery {
    pub(crate) message_id: String,
    pub(crate) envelope: aura_core::effects::transport::TransportEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapPendingEnrollmentMailboxAck {
    message_id: String,
}

impl BootstrapBrokerCandidateRecord {
    pub fn authority_id(&self) -> Option<AuthorityId> {
        AuthorityId::from_str(&self.authority_id).ok()
    }
}

pub use aura_core::invitation::BootstrapBrokerInvitation;

#[cfg(not(target_arch = "wasm32"))]
struct BootstrapBrokerState {
    candidates: HashMap<String, BootstrapBrokerCandidateRecord>,
    invitations: HashMap<(String, DeviceId), Vec<BootstrapBrokerInvitation>>,
    invitation_credentials: HashMap<(String, DeviceId), String>,
    pending_enrollment_mailboxes: PendingEnrollmentMailboxes,
    #[cfg(test)]
    pending_registration_miss: Option<tokio::sync::oneshot::Sender<()>>,
    #[cfg(test)]
    pending_registration_pause: Option<(
        tokio::sync::oneshot::Sender<()>,
        tokio::sync::oneshot::Receiver<()>,
    )>,
    enrollment_return_mailboxes: EnrollmentReturnMailboxes,
    candidate_time_high_water_ms: Option<u64>,
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for BootstrapBrokerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BootstrapBrokerState")
            .field("pending_mailboxes", &self.pending_enrollment_mailboxes)
            .finish_non_exhaustive()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl BootstrapBrokerState {
    #[cfg(test)]
    fn new() -> Self {
        Self::with_retention(Duration::from_secs(120))
    }

    fn with_retention(retention: Duration) -> Self {
        Self {
            candidates: HashMap::new(),
            invitations: HashMap::new(),
            invitation_credentials: HashMap::new(),
            pending_enrollment_mailboxes: PendingEnrollmentMailboxes::new(retention),
            #[cfg(test)]
            pending_registration_miss: None,
            #[cfg(test)]
            pending_registration_pause: None,
            enrollment_return_mailboxes: EnrollmentReturnMailboxes::new(retention),
            candidate_time_high_water_ms: None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
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
    mailbox_time: Option<Arc<dyn PhysicalTimeEffects>>,
    #[cfg(feature = "simulator-checkpoint")]
    checkpoint_group: OnceLock<TaskGroup>,
    #[cfg(feature = "simulator-checkpoint")]
    checkpoint_start_conflict: AtomicBool,
}

/// Private stopped broker owner; it cannot authorize a profile or restore a
/// continuation. Only a later effect-owned co-checkpoint may consume its state.
#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
pub struct StoppedBrokerCheckpointOwner {
    _state: BootstrapBrokerState,
    retention: Duration,
}

/// A one-use simulator artifact owned by the selected filesystem provider.
/// The broker bytes cannot be installed without the effect-owned V2 restore.
#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
pub struct SealedBrokerCheckpoint {
    profile: aura_effects::profile_checkpoint::QuiescentFilesystemProfileCheckpoint,
    original_profile: PathBuf,
    revision: [u8; 32],
    participant: [u8; 32],
    context: aura_effects::profile_checkpoint::AuxiliaryCheckpointContext,
    retention: Duration,
    used: bool,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
#[derive(Debug, thiserror::Error)]
pub enum BrokerCheckpointRestoreError {
    #[error("broker checkpoint selected profile failed: {0}")]
    Profile(#[from] aura_effects::profile_checkpoint::ProfileCheckpointError),
    #[error("broker checkpoint fresh owner is already active or has state")]
    Busy,
    #[error("broker checkpoint selected clock is missing, stale, or moved backward")]
    Clock,
    #[error("broker checkpoint authenticated registration state is invalid")]
    InvalidState,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
const BROKER_AUXILIARY_KIND: [u8; 16] = *b"aura-broker-v1\0\0";

#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
impl SealedBrokerCheckpoint {
    /// Only the original sealed artifact can install broker state, and only
    /// into a new unstarted owner. An attempted effect restore consumes the
    /// handle even on failure; context/clock preflight can be corrected.
    pub async fn restore_into_fresh_broker(
        &mut self,
        broker: LocalBootstrapBrokerService,
        expected: aura_effects::profile_checkpoint::AuxiliaryCheckpointContext,
        expected_original_profile: &Path,
    ) -> Result<LocalBootstrapBrokerService, BrokerCheckpointRestoreError> {
        use BrokerCheckpointRestoreError as Refusal;
        if self.used {
            return Err(Refusal::Busy);
        }
        if expected != self.context
            || expected.kind != BROKER_AUXILIARY_KIND
            || expected.version != 1
            || expected.epoch != 0
            || expected_original_profile != self.original_profile.as_path()
        {
            return Err(Refusal::InvalidState);
        }
        if broker.checkpoint_group.get().is_some()
            || Arc::strong_count(&broker.shared) != 1
            || Arc::strong_count(&broker.listener) != 1
            || broker.registration_ttl != self.retention
        {
            return Err(Refusal::Busy);
        }
        let now_ms = broker.mailbox_now_ms().await.map_err(|_| Refusal::Clock)?;
        if now_ms < expected.selected_clock_ms {
            return Err(Refusal::Clock);
        }
        {
            let state = broker.shared.read().await;
            if !state.candidates.is_empty()
                || !state.invitations.is_empty()
                || !state.invitation_credentials.is_empty()
                || state.candidate_time_high_water_ms.is_some()
                || !state.pending_enrollment_mailboxes.is_empty()
                || !state.enrollment_return_mailboxes.is_empty()
            {
                return Err(Refusal::Busy);
            }
        }
        self.used = true;
        let bytes = self
            .profile
            .restore_with_auxiliary(
                &self.original_profile,
                self.revision,
                self.participant,
                expected,
            )
            .await?;
        let restored = decode_quiescent_broker_registrations(&bytes, self.retention, now_ms)
            .ok_or(Refusal::InvalidState)?;
        *broker.shared.write().await = restored;
        Ok(broker)
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuiescentBrokerRegistrationsV1 {
    version: u8,
    candidates: Vec<BootstrapBrokerCandidateRecord>,
    pending: Vec<QuiescentPendingRegistration>,
    returns: Vec<QuiescentReturnRegistration>,
    candidate_time_high_water_ms: Option<u64>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
impl StoppedBrokerCheckpointOwner {
    /// Fixed broker payload domain; caller metadata never chooses the codec.
    pub fn context_for_simulator(
        owner: [u8; 32],
        epoch: u64,
        fixture: [u8; 32],
        selected_clock_ms: u64,
    ) -> aura_effects::profile_checkpoint::AuxiliaryCheckpointContext {
        aura_effects::profile_checkpoint::AuxiliaryCheckpointContext {
            kind: BROKER_AUXILIARY_KIND,
            version: 1,
            owner,
            epoch,
            fixture,
            selected_clock_ms,
        }
    }
    /// Encode only the original stopped owner's quiescent availability data.
    /// The bytes confer no restore authority and must be sealed by the selected
    /// profile checkpoint owner before any fresh broker accepts them.
    fn into_unsealed_registrations(self) -> Result<Vec<u8>, BrokerCheckpointQuiescenceError> {
        let state = self._state;
        let mut candidates: Vec<_> = state.candidates.into_values().collect();
        candidates.sort_by(|left, right| left.authority_id.cmp(&right.authority_id));
        let pending = state
            .pending_enrollment_mailboxes
            .quiescent_registrations()
            .ok_or(BrokerCheckpointQuiescenceError::UnsupportedState)?;
        let returns = state
            .enrollment_return_mailboxes
            .quiescent_registrations()
            .ok_or(BrokerCheckpointQuiescenceError::UnsupportedState)?;
        let bytes = serde_json::to_vec(&QuiescentBrokerRegistrationsV1 {
            version: 1,
            candidates,
            pending,
            returns,
            candidate_time_high_water_ms: state.candidate_time_high_water_ms,
        })
        .map_err(|_| BrokerCheckpointQuiescenceError::UnsupportedState)?;
        if decode_quiescent_broker_registrations(&bytes, self.retention, 0).is_none() {
            return Err(BrokerCheckpointQuiescenceError::UnsupportedState);
        }
        Ok(bytes)
    }

    /// Capture the stopped original broker together with an exclusively
    /// quiescent profile selected by its real runtime provider.
    pub fn seal_with_selected_profile(
        self,
        selected: aura_effects::profile_checkpoint::SelectedFilesystemCheckpointSource,
        original_profile: &Path,
        artifact: &Path,
        revision: [u8; 32],
        participant: [u8; 32],
        context: aura_effects::profile_checkpoint::AuxiliaryCheckpointContext,
    ) -> Result<SealedBrokerCheckpoint, BrokerCheckpointRestoreError> {
        use aura_effects::profile_checkpoint::QuiescentFilesystemProfileCheckpoint;
        if context.kind != BROKER_AUXILIARY_KIND || context.version != 1 || context.epoch != 0 {
            return Err(BrokerCheckpointRestoreError::InvalidState);
        }
        let retention = self.retention;
        let bytes = self
            .into_unsealed_registrations()
            .map_err(|_| BrokerCheckpointRestoreError::InvalidState)?;
        let profile = QuiescentFilesystemProfileCheckpoint::capture_with_auxiliary(
            selected,
            original_profile,
            artifact,
            revision,
            participant,
            context,
            &bytes,
        )?;
        Ok(SealedBrokerCheckpoint {
            profile,
            original_profile: original_profile.to_owned(),
            revision,
            participant,
            context,
            retention,
            used: false,
        })
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
fn decode_quiescent_broker_registrations(
    bytes: &[u8],
    retention: Duration,
    now_ms: u64,
) -> Option<BootstrapBrokerState> {
    if bytes.len() > 256 * 1024 {
        return None;
    }
    let snapshot: QuiescentBrokerRegistrationsV1 = serde_json::from_slice(bytes).ok()?;
    if snapshot.version != 1
        || snapshot.candidates.len() > MAX_BOOTSTRAP_CANDIDATES
        || (now_ms != 0
            && snapshot
                .candidate_time_high_water_ms
                .is_some_and(|observed| now_ms < observed))
        || serde_json::to_vec(&snapshot).ok()? != bytes
    {
        return None;
    }
    let mut state = BootstrapBrokerState::with_retention(retention);
    for candidate in snapshot.candidates {
        let key = candidate.authority_id.clone();
        if candidate.authority_id().is_none()
            || (now_ms != 0 && candidate.discovered_at_ms > now_ms)
            || state.candidates.insert(key, candidate).is_some()
        {
            return None;
        }
    }
    state.pending_enrollment_mailboxes = PendingEnrollmentMailboxes::from_quiescent_registrations(
        retention,
        snapshot.pending,
        now_ms,
    )?;
    state.enrollment_return_mailboxes = EnrollmentReturnMailboxes::from_quiescent_registrations(
        retention,
        snapshot.returns,
        now_ms,
    )?;
    state.candidate_time_high_water_ms = snapshot.candidate_time_high_water_ms;
    Some(state)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "simulator-checkpoint"))]
#[derive(Debug, thiserror::Error)]
pub enum BrokerCheckpointQuiescenceError {
    #[error("broker checkpoint requires its original started task group")]
    NotStarted,
    #[error("broker checkpoint original task group was started more than once")]
    RepeatedStart,
    #[error("broker checkpoint group contains another live task")]
    ForeignTask,
    #[error("broker checkpoint has no selected mailbox time provider")]
    MissingSelectedClock,
    #[error("broker checkpoint task disposal failed: {0}")]
    Disposal(#[from] crate::task_registry::TaskSupervisionError),
    #[error("broker checkpoint original task group retained a terminal failure")]
    FailedTask,
    #[error("broker checkpoint original task disposal belongs to another group")]
    ForeignDisposal,
    #[error("broker checkpoint retains a live listener or connection owner")]
    LiveOwner,
    #[error("broker checkpoint has unsettled invitation or mailbox delivery")]
    UnsettledDelivery,
    #[error("broker checkpoint retains state outside its quiescent registration contract")]
    UnsupportedState,
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for LocalBootstrapBrokerService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalBootstrapBrokerService")
            .field("public_url", &self.public_url)
            .field("mailbox_time_selected", &self.mailbox_time.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl LocalBootstrapBrokerService {
    async fn mailbox_now_ms(&self) -> Result<u64, String> {
        selected_mailbox_now_ms(self.mailbox_time.as_deref()).await
    }
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
            shared: Arc::new(RwLock::new(BootstrapBrokerState::with_retention(
                registration_ttl,
            ))),
            limits,
            connection_permits: Arc::new(Semaphore::new(limits.max_connections)),
            mailbox_time: None,
            #[cfg(feature = "simulator-checkpoint")]
            checkpoint_group: OnceLock::new(),
            #[cfg(feature = "simulator-checkpoint")]
            checkpoint_start_conflict: AtomicBool::new(false),
        })
    }

    /// Bind mailbox expiry to the selected runtime provider. Ordinary broker
    /// discovery can run without it, but mailbox endpoints fail closed.
    pub fn with_mailbox_time_provider(mut self, provider: Arc<dyn PhysicalTimeEffects>) -> Self {
        self.mailbox_time = Some(provider);
        self
    }

    pub fn public_url(&self) -> &str {
        &self.public_url
    }

    #[cfg(test)]
    pub(crate) async fn observe_next_pending_registration_miss_for_test(
        &self,
    ) -> tokio::sync::oneshot::Receiver<()> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.shared.write().await.pending_registration_miss = Some(sender);
        receiver
    }

    #[cfg(test)]
    pub(crate) async fn pause_next_pending_registration_for_test(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered_sender, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
        self.shared.write().await.pending_registration_pause =
            Some((entered_sender, release_receiver));
        (entered_receiver, release_sender)
    }

    #[cfg(test)]
    pub(crate) async fn pending_ack_witnesses_for_test(
        &self,
        subject: AuthorityId,
        device: DeviceId,
        manifest_digest: [u8; 32],
    ) -> Result<Vec<(String, String, String, String)>, String> {
        let now_ms = self.mailbox_now_ms().await?;
        Ok(self
            .shared
            .write()
            .await
            .pending_enrollment_mailboxes
            .acknowledged_witnesses(
                PendingMailboxKey::new(subject, device, manifest_digest),
                now_ms,
            )
            .into_iter()
            .map(|witness| {
                (
                    witness.message_id,
                    witness.subject,
                    witness.device,
                    witness.manifest_digest,
                )
            })
            .collect())
    }

    pub async fn register(&self, registration: BootstrapBrokerRegistration) -> Result<(), String> {
        with_candidate_time(
            &self.shared,
            self.mailbox_time.as_deref(),
            |state, now_ms| {
                prune_candidates(state, now_ms, self.registration_ttl);
                insert_registration(state, registration, now_ms)
            },
        )
        .await?
    }

    pub async fn list_candidates(&self) -> Result<Vec<BootstrapBrokerCandidateRecord>, String> {
        with_candidate_time(
            &self.shared,
            self.mailbox_time.as_deref(),
            |state, now_ms| {
                prune_candidates(state, now_ms, self.registration_ttl);
                state.candidates.values().cloned().collect()
            },
        )
        .await
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
        device_id: DeviceId,
        retrieval_token: &str,
    ) -> Result<Vec<BootstrapBrokerInvitation>, String> {
        take_invitations(&self.shared, authority_id, device_id, retrieval_token).await
    }

    pub(crate) async fn register_pending_enrollment_mailbox(
        &self,
        registration: BootstrapPendingEnrollmentMailboxRegistration,
    ) -> Result<(), String> {
        if constant_time_eq_str(&registration.retrieval_credential, &self.auth_token) {
            return Err("pending mailbox credential must be scoped independently".into());
        }
        let now_ms = self.mailbox_now_ms().await?;
        self.shared
            .write()
            .await
            .pending_enrollment_mailboxes
            .register(
                registration.registration,
                &registration.retrieval_credential,
                now_ms,
            )
            .map_err(str::to_string)
    }

    pub(crate) async fn pending_enrollment_mailbox_registration(
        &self,
        subject: AuthorityId,
        device: DeviceId,
        manifest_digest: [u8; 32],
    ) -> Result<
        Option<aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxRegistration>,
        String,
    > {
        let now_ms = self.mailbox_now_ms().await?;
        let mut state = self.shared.write().await;
        let registration = state.pending_enrollment_mailboxes.registration(
            PendingMailboxKey::new(subject, device, manifest_digest),
            now_ms,
        );
        #[cfg(test)]
        if registration.is_none() {
            if let Some(sender) = state.pending_registration_miss.take() {
                let _ = sender.send(());
            }
        }
        Ok(registration)
    }

    pub(crate) async fn queue_pending_enrollment_vm_envelope(
        &self,
        manifest_digest: [u8; 32],
        envelope: aura_core::effects::transport::TransportEnvelope,
    ) -> Result<(), String> {
        let device = envelope
            .metadata
            .get("aura-destination-device-id")
            .and_then(|value| DeviceId::from_str(value).ok())
            .ok_or("pending enrollment VM envelope has no exact device")?;
        let key = PendingMailboxKey::new(envelope.destination, device, manifest_digest);
        let now_ms = self.mailbox_now_ms().await?;
        let mut state = self.shared.write().await;
        state
            .pending_enrollment_mailboxes
            .enqueue(key, envelope, now_ms)
            .map_err(str::to_string)?;
        Ok(())
    }

    pub(crate) async fn take_pending_enrollment_vm_envelopes(
        &self,
        subject: AuthorityId,
        device: DeviceId,
        manifest_digest: [u8; 32],
        credential: &str,
    ) -> Result<Vec<PendingMailboxDelivery>, String> {
        let now_ms = self.mailbox_now_ms().await?;
        self.shared
            .write()
            .await
            .pending_enrollment_mailboxes
            .take(
                PendingMailboxKey::new(subject, device, manifest_digest),
                credential,
                now_ms,
            )
            .map_err(str::to_string)
    }

    pub(crate) async fn acknowledge_pending_enrollment_vm_envelope(
        &self,
        subject: AuthorityId,
        device: DeviceId,
        manifest_digest: [u8; 32],
        credential: &str,
        message_id: &str,
    ) -> Result<(), String> {
        let now_ms = self.mailbox_now_ms().await?;
        self.shared
            .write()
            .await
            .pending_enrollment_mailboxes
            .acknowledge(
                PendingMailboxKey::new(subject, device, manifest_digest),
                credential,
                message_id,
                now_ms,
            )
            .map_err(str::to_string)
    }

    #[cfg(test)]
    pub(crate) async fn interrupt_next_pending_enrollment_ack_for_test(&self) {
        self.shared
            .write()
            .await
            .pending_enrollment_mailboxes
            .fail_next_ack_for_test();
    }

    pub(crate) async fn register_issuer_return_mailbox(
        &self,
        payload: BootstrapIssuerReturnMailboxRegistration,
    ) -> Result<(), String> {
        if constant_time_eq_str(&payload.retrieval_credential, &self.auth_token) {
            return Err("return mailbox credential must be scoped independently".into());
        }
        let provider = self
            .mailbox_time
            .as_deref()
            .ok_or("return mailbox selected physical-time provider is absent")?;
        payload
            .registration
            .verify_untrusted_broker_registration(
                &aura_effects::crypto::RealCryptoHandler::new(),
                provider,
            )
            .await
            .map_err(|source| source.to_string())?;
        let now_ms = self.mailbox_now_ms().await?;
        self.shared
            .write()
            .await
            .enrollment_return_mailboxes
            .register(payload.registration, &payload.retrieval_credential, now_ms)
            .map_err(str::to_string)
    }

    pub(crate) async fn issuer_return_mailbox_registration(
        &self,
        subject: AuthorityId,
        issuer_device: DeviceId,
        manifest_digest: [u8; 32],
    ) -> Result<
        Option<aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration>,
        String,
    > {
        let now_ms = self.mailbox_now_ms().await?;
        Ok(self
            .shared
            .write()
            .await
            .enrollment_return_mailboxes
            .registration(
                ReturnMailboxKey::new(subject, issuer_device, manifest_digest),
                now_ms,
            ))
    }

    pub(crate) async fn queue_issuer_return_vm_envelope(
        &self,
        payload: BootstrapIssuerReturnMailboxEnvelope,
    ) -> Result<(), String> {
        let issuer_device = payload
            .envelope
            .metadata
            .get("aura-destination-device-id")
            .and_then(|raw| DeviceId::from_str(raw).ok())
            .ok_or("return VM envelope has no exact issuer device")?;
        let key = ReturnMailboxKey::new(
            payload.envelope.destination,
            issuer_device,
            payload.manifest_digest,
        );
        let now_ms = self.mailbox_now_ms().await?;
        let registration = self
            .shared
            .write()
            .await
            .enrollment_return_mailboxes
            .registration(key, now_ms)
            .ok_or("return mailbox registration is absent")?;
        let provider = self
            .mailbox_time
            .as_deref()
            .ok_or("return mailbox selected physical-time provider is absent")?;
        payload
            .authorization
            .verify_untrusted_broker_admission(
                &aura_effects::crypto::RealCryptoHandler::new(),
                provider,
                &registration,
                &payload.setup_request,
                &payload.envelope,
            )
            .await
            .map_err(|source| source.to_string())?;
        let now_ms = self.mailbox_now_ms().await?;
        self.shared
            .write()
            .await
            .enrollment_return_mailboxes
            .enqueue_verified(key, &registration, payload, now_ms)
            .map_err(str::to_string)
    }

    pub(crate) async fn take_issuer_return_vm_envelopes(
        &self,
        subject: AuthorityId,
        issuer_device: DeviceId,
        manifest_digest: [u8; 32],
        credential: &str,
    ) -> Result<Vec<IssuerReturnMailboxDelivery>, String> {
        let now_ms = self.mailbox_now_ms().await?;
        self.shared
            .write()
            .await
            .enrollment_return_mailboxes
            .take(
                ReturnMailboxKey::new(subject, issuer_device, manifest_digest),
                credential,
                now_ms,
            )
            .map_err(str::to_string)
    }

    pub(crate) async fn acknowledge_issuer_return_vm_envelope(
        &self,
        subject: AuthorityId,
        issuer_device: DeviceId,
        manifest_digest: [u8; 32],
        credential: &str,
        message_id: &str,
    ) -> Result<(), String> {
        let now_ms = self.mailbox_now_ms().await?;
        self.shared
            .write()
            .await
            .enrollment_return_mailboxes
            .acknowledge(
                ReturnMailboxKey::new(subject, issuer_device, manifest_digest),
                credential,
                message_id,
                now_ms,
            )
            .map_err(str::to_string)
    }

    #[cfg(test)]
    pub(crate) async fn interrupt_next_issuer_return_ack_for_test(&self) {
        self.shared
            .write()
            .await
            .enrollment_return_mailboxes
            .fail_next_ack_for_test();
    }

    pub fn start(&self, tasks: &TaskGroup) {
        #[cfg(feature = "simulator-checkpoint")]
        if self.checkpoint_group.set(tasks.clone()).is_err() {
            self.checkpoint_start_conflict
                .store(true, Ordering::Release);
        }
        let listener = self.listener.clone();
        let shared = self.shared.clone();
        let mailbox_time = self.mailbox_time.clone();
        let registration_ttl = self.registration_ttl;
        let auth_token = self.auth_token.clone();
        let limits = self.limits;
        let permits = self.connection_permits.clone();
        let accept_tasks = tasks.clone();
        let connection_tasks = tasks.clone();
        let _bootstrap_broker_handle =
            // The accept loop runs for the runtime's life; a listener failure
            // ends it as a supervised task failure (Task 117).
            accept_tasks.spawn_try_named("bootstrap_broker_http", async move {
                loop {
                    let (mut stream, _addr) =
                        listener.accept().await.map_err(|error| aura_core::AuraError::Internal {
                            message: "bootstrap broker listener accept failed".to_string(),
                            source: Some(std::sync::Arc::new(error)),
                        })?;
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
                    let mailbox_time = mailbox_time.clone();
                    let auth_token = auth_token.clone();
                    let connection_tasks = connection_tasks.clone();
                    let _conn_handle =
                        connection_tasks.spawn_named("bootstrap_broker_conn", async move {
                            let _permit = permit;
                            handle_http_connection(
                                stream,
                                shared,
                                mailbox_time,
                                registration_ttl,
                                auth_token,
                                limits.request_read_timeout,
                            )
                            .await;
                        });
                }
            });
    }

    /// Stop exactly the broker's original supervised subtree before any
    /// simulator checkpoint can inspect it. No bytes or restore authority leave
    /// this private method. A failure consumes the service and remains closed.
    #[cfg(feature = "simulator-checkpoint")]
    pub async fn stop_for_simulator_checkpoint(
        self,
        timeout: Duration,
    ) -> Result<StoppedBrokerCheckpointOwner, BrokerCheckpointQuiescenceError> {
        use BrokerCheckpointQuiescenceError as Refusal;
        let group = self.checkpoint_group.get().ok_or(Refusal::NotStarted)?;
        let repeated = self.checkpoint_start_conflict.load(Ordering::Acquire);
        let foreign = group
            .active_tasks()
            .iter()
            .any(|task| task != "bootstrap_broker_http" && task != "bootstrap_broker_conn");
        let sealed = group.seal_admission();
        group.shutdown_with_timeout(timeout).await?;
        let disposed = sealed.await_disposal().await;
        disposed
            .require_original_group(group)
            .map_err(|_| Refusal::ForeignDisposal)?;
        if disposed.failure().is_some() {
            return Err(Refusal::FailedTask);
        }
        if repeated {
            return Err(Refusal::RepeatedStart);
        }
        if foreign {
            return Err(Refusal::ForeignTask);
        }
        if self.mailbox_time.is_none() {
            return Err(Refusal::MissingSelectedClock);
        }
        if Arc::strong_count(&self.listener) != 1
            || Arc::strong_count(&self.shared) != 1
            || self.connection_permits.available_permits() != self.limits.max_connections
        {
            return Err(Refusal::LiveOwner);
        }
        let state = Arc::try_unwrap(self.shared)
            .map_err(|_| Refusal::LiveOwner)?
            .into_inner();
        if state.invitations.values().any(|items| !items.is_empty())
            || state.pending_enrollment_mailboxes.has_unsettled_delivery()
            || state.enrollment_return_mailboxes.has_unsettled_delivery()
        {
            return Err(Refusal::UnsettledDelivery);
        }
        if !state.invitation_credentials.is_empty()
            || state
                .pending_enrollment_mailboxes
                .has_uncheckpointable_state()
            || state
                .enrollment_return_mailboxes
                .has_uncheckpointable_state()
            || {
                #[cfg(test)]
                {
                    state.pending_registration_miss.is_some()
                        || state.pending_registration_pause.is_some()
                }
                #[cfg(not(test))]
                {
                    false
                }
            }
        {
            return Err(Refusal::UnsupportedState);
        }
        Ok(StoppedBrokerCheckpointOwner {
            _state: state,
            retention: self.registration_ttl,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn prune_candidates(state: &mut BootstrapBrokerState, now_ms: u64, registration_ttl: Duration) {
    let max_age_ms = registration_ttl.as_millis() as u64;
    state
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
fn insert_registration(
    state: &mut BootstrapBrokerState,
    registration: BootstrapBrokerRegistration,
    now_ms: u64,
) -> Result<(), String> {
    if registration.invitation_retrieval_token.is_empty() {
        return Err("bootstrap broker invitation retrieval token must not be empty".to_string());
    }
    let key = format!(
        "{}:{}@{}",
        registration.authority_id, registration.device_id, registration.address
    );
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
        (registration.authority_id.clone(), registration.device_id),
        registration.invitation_retrieval_token,
    );
    state.candidates.insert(
        key,
        BootstrapBrokerCandidateRecord {
            authority_id: registration.authority_id,
            device_id: registration.device_id,
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
    let recipient = (
        invitation.recipient_authority_id.clone(),
        invitation.recipient_device_id,
    );
    let pending_total: usize = state.invitations.values().map(Vec::len).sum();
    let recipient_pending = state.invitations.get(&recipient).map(Vec::len).unwrap_or(0);
    if pending_total >= MAX_PENDING_INVITATIONS {
        return Err("bootstrap broker pending invitation limit reached".to_string());
    }
    if recipient_pending >= MAX_INVITATIONS_PER_RECIPIENT {
        return Err("bootstrap broker per-recipient invitation limit reached".to_string());
    }
    state
        .invitations
        .entry(recipient)
        .or_default()
        .push(invitation);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
async fn take_invitations(
    shared: &Arc<RwLock<BootstrapBrokerState>>,
    authority_id: &str,
    device_id: DeviceId,
    retrieval_token: &str,
) -> Result<Vec<BootstrapBrokerInvitation>, String> {
    let mut state = shared.write().await;
    let recipient = (authority_id.to_owned(), device_id);
    match state.invitation_credentials.get(&recipient) {
        Some(expected) if constant_time_eq_str(expected, retrieval_token) => {}
        _ => return Err("bootstrap broker invitation credential rejected".to_string()),
    }
    let invitations = state.invitations.remove(&recipient).unwrap_or_default();
    if !invitations.is_empty() {
        state.invitation_credentials.remove(&recipient);
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
async fn selected_mailbox_now_ms(
    provider: Option<&dyn PhysicalTimeEffects>,
) -> Result<u64, String> {
    provider
        .ok_or_else(|| "pending mailbox selected physical-time provider is absent".to_string())?
        .physical_time()
        .await
        .map(|time| time.ts_ms)
        .map_err(|source| format!("pending mailbox selected physical-time read failed: {source}"))
}

#[cfg(not(target_arch = "wasm32"))]
async fn handle_http_connection(
    mut stream: TcpStream,
    shared: Arc<RwLock<BootstrapBrokerState>>,
    mailbox_time: Option<Arc<dyn PhysicalTimeEffects>>,
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

    macro_rules! mailbox_now {
        () => {
            match selected_mailbox_now_ms(mailbox_time.as_deref()).await {
                Ok(now_ms) => now_ms,
                Err(_) => {
                    let _ = write_http_response(
                        &mut stream,
                        503,
                        "text/plain; charset=utf-8",
                        b"pending mailbox selected physical-time provider unavailable",
                    )
                    .await;
                    return;
                }
            }
        };
    }

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
        ("POST", "/v2/bootstrap/register") => {
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

            let admission =
                match with_candidate_time(&shared, mailbox_time.as_deref(), |state, now_ms| {
                    prune_candidates(state, now_ms, registration_ttl);
                    insert_registration(state, registration, now_ms)
                })
                .await
                {
                    Ok(admission) => admission,
                    Err(_) => {
                        let _ = write_http_response(
                            &mut stream,
                            503,
                            "text/plain; charset=utf-8",
                            b"bootstrap broker selected physical-time provider unavailable",
                        )
                        .await;
                        return;
                    }
                };
            if let Err(error) = admission {
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
        ("GET", "/v2/bootstrap/candidates") => {
            let candidates: Vec<_> =
                match with_candidate_time(&shared, mailbox_time.as_deref(), |state, now_ms| {
                    prune_candidates(state, now_ms, registration_ttl);
                    state.candidates.values().cloned().collect()
                })
                .await
                {
                    Ok(candidates) => candidates,
                    Err(_) => {
                        let _ = write_http_response(
                            &mut stream,
                            503,
                            "text/plain; charset=utf-8",
                            b"bootstrap broker selected physical-time provider unavailable",
                        )
                        .await;
                        return;
                    }
                };
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
        ("POST", "/v2/bootstrap/invitations") => {
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
        ("GET", path) if path.starts_with("/v2/bootstrap/invitations/") => {
            let Some((authority_id, device_id)) =
                invitation_request_recipient(path.trim_start_matches("/v2/bootstrap/invitations/"))
            else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"missing or invalid invitation authority/device",
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
            let invitations =
                match take_invitations(&shared, authority_id, device_id, retrieval_token).await {
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
        ("POST", "/v2/bootstrap/enrollment-mailbox/register") => {
            let payload: BootstrapPendingEnrollmentMailboxRegistration =
                match serde_json::from_slice(&request.body) {
                    Ok(value) => value,
                    Err(_) => {
                        let _ = write_http_response(
                            &mut stream,
                            400,
                            "text/plain; charset=utf-8",
                            b"invalid pending enrollment mailbox registration",
                        )
                        .await;
                        return;
                    }
                };
            if constant_time_eq_str(&payload.retrieval_credential, &auth_token) {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"pending mailbox credential must be scoped independently",
                )
                .await;
                return;
            }
            #[cfg(test)]
            let registration_pause = { shared.write().await.pending_registration_pause.take() };
            #[cfg(test)]
            if let Some((entered, release)) = registration_pause {
                let _ = entered.send(());
                let _ = release.await;
            }
            let now_ms = mailbox_now!();
            let result = shared.write().await.pending_enrollment_mailboxes.register(
                payload.registration,
                &payload.retrieval_credential,
                now_ms,
            );
            let (status, body) = match result {
                Ok(()) => (204, ""),
                Err(reason) => (429, reason),
            };
            let _ = write_http_response(
                &mut stream,
                status,
                "text/plain; charset=utf-8",
                body.as_bytes(),
            )
            .await;
        }
        ("GET", path) if path.starts_with("/v2/bootstrap/enrollment-mailbox/register/") => {
            let Some(key) = pending_enrollment_mailbox_key(
                path.trim_start_matches("/v2/bootstrap/enrollment-mailbox/register/"),
            ) else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"invalid pending enrollment mailbox selector",
                )
                .await;
                return;
            };
            let now_ms = mailbox_now!();
            let mut state = shared.write().await;
            let found = state.pending_enrollment_mailboxes.registration(key, now_ms);
            #[cfg(test)]
            if found.is_none() {
                if let Some(sender) = state.pending_registration_miss.take() {
                    let _ = sender.send(());
                }
            }
            drop(state);
            match serde_json::to_vec(&found) {
                Ok(body) => {
                    let _ = write_http_response(&mut stream, 200, "application/json", &body).await;
                }
                Err(_) => {
                    let _ = write_http_response(
                        &mut stream,
                        500,
                        "text/plain; charset=utf-8",
                        b"pending enrollment mailbox registration encoding failed",
                    )
                    .await;
                }
            }
        }
        ("POST", "/v2/bootstrap/enrollment-mailbox/envelopes") => {
            let payload: BootstrapPendingEnrollmentMailboxEnvelope =
                match serde_json::from_slice(&request.body) {
                    Ok(value) => value,
                    Err(_) => {
                        let _ = write_http_response(
                            &mut stream,
                            400,
                            "text/plain; charset=utf-8",
                            b"invalid pending enrollment VM envelope",
                        )
                        .await;
                        return;
                    }
                };
            let device = payload
                .envelope
                .metadata
                .get("aura-destination-device-id")
                .and_then(|value| DeviceId::from_str(value).ok());
            let Some(device) = device else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"pending enrollment VM envelope has no exact device",
                )
                .await;
                return;
            };
            let key = PendingMailboxKey::new(
                payload.envelope.destination,
                device,
                payload.manifest_digest,
            );
            let now_ms = mailbox_now!();
            let mut state = shared.write().await;
            let result = state
                .pending_enrollment_mailboxes
                .enqueue(key, payload.envelope, now_ms);
            drop(state);
            let (status, body) = match result {
                Ok(()) => (204, ""),
                Err(reason) => (429, reason),
            };
            let _ = write_http_response(
                &mut stream,
                status,
                "text/plain; charset=utf-8",
                body.as_bytes(),
            )
            .await;
        }
        ("POST", path) if path.starts_with("/v2/bootstrap/enrollment-mailbox/ack/") => {
            let Some(key) = pending_enrollment_mailbox_key(
                path.trim_start_matches("/v2/bootstrap/enrollment-mailbox/ack/"),
            ) else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"invalid pending enrollment mailbox selector",
                )
                .await;
                return;
            };
            let Some(credential) = request_header(&request, ENROLLMENT_MAILBOX_RETRIEVAL_HEADER)
            else {
                let _ = write_http_response(
                    &mut stream,
                    401,
                    "text/plain; charset=utf-8",
                    b"missing pending enrollment mailbox retrieval credential",
                )
                .await;
                return;
            };
            let Ok(ack) =
                serde_json::from_slice::<BootstrapPendingEnrollmentMailboxAck>(&request.body)
            else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"invalid pending enrollment mailbox acknowledgment",
                )
                .await;
                return;
            };
            let now_ms = mailbox_now!();
            let result = shared
                .write()
                .await
                .pending_enrollment_mailboxes
                .acknowledge(key, credential, &ack.message_id, now_ms);
            let status = if result.is_ok() { 204 } else { 401 };
            let _ =
                write_http_response(&mut stream, status, "text/plain; charset=utf-8", b"").await;
        }
        ("GET", path) if path.starts_with("/v2/bootstrap/enrollment-mailbox/progress-witness/") => {
            let Some(key) = pending_enrollment_mailbox_key(
                path.trim_start_matches("/v2/bootstrap/enrollment-mailbox/progress-witness/"),
            ) else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"invalid pending enrollment mailbox selector",
                )
                .await;
                return;
            };
            let now_ms = mailbox_now!();
            let result = shared
                .write()
                .await
                .pending_enrollment_mailboxes
                .progress_witness(key, now_ms)
                .and_then(|witness| {
                    serde_json::to_vec(&witness).map_err(|_| "progress witness encoding failed")
                });
            match result {
                Ok(body) => {
                    let _ = write_http_response(&mut stream, 200, "application/json", &body).await;
                }
                Err(_) => {
                    let _ = write_http_response(
                        &mut stream,
                        500,
                        "text/plain; charset=utf-8",
                        b"pending enrollment progress witness encoding failed",
                    )
                    .await;
                }
            }
        }
        ("GET", path) if path.starts_with("/v2/bootstrap/enrollment-mailbox/ack-witness/") => {
            let Some(key) = pending_enrollment_mailbox_key(
                path.trim_start_matches("/v2/bootstrap/enrollment-mailbox/ack-witness/"),
            ) else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"invalid pending enrollment mailbox selector",
                )
                .await;
                return;
            };
            let now_ms = mailbox_now!();
            let witnesses = shared
                .write()
                .await
                .pending_enrollment_mailboxes
                .acknowledged_witnesses(key, now_ms);
            match serde_json::to_vec(&witnesses) {
                Ok(body) => {
                    let _ = write_http_response(&mut stream, 200, "application/json", &body).await;
                }
                Err(_) => {
                    let _ = write_http_response(
                        &mut stream,
                        500,
                        "text/plain; charset=utf-8",
                        b"pending enrollment ACK witness encoding failed",
                    )
                    .await;
                }
            }
        }
        ("GET", path) if path.starts_with("/v2/bootstrap/enrollment-mailbox/envelopes/") => {
            let Some(key) = pending_enrollment_mailbox_key(
                path.trim_start_matches("/v2/bootstrap/enrollment-mailbox/envelopes/"),
            ) else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"invalid pending enrollment mailbox selector",
                )
                .await;
                return;
            };
            let Some(credential) = request_header(&request, ENROLLMENT_MAILBOX_RETRIEVAL_HEADER)
            else {
                let _ = write_http_response(
                    &mut stream,
                    401,
                    "text/plain; charset=utf-8",
                    b"missing pending enrollment mailbox retrieval credential",
                )
                .await;
                return;
            };
            let now_ms = mailbox_now!();
            let result = shared
                .write()
                .await
                .pending_enrollment_mailboxes
                .take(key, credential, now_ms);
            match result {
                Ok(envelopes) => match serde_json::to_vec(&envelopes) {
                    Ok(body) => {
                        let _ =
                            write_http_response(&mut stream, 200, "application/json", &body).await;
                    }
                    Err(_) => {
                        let _ = write_http_response(
                            &mut stream,
                            500,
                            "text/plain; charset=utf-8",
                            b"pending enrollment mailbox encoding failed",
                        )
                        .await;
                    }
                },
                Err(_) => {
                    let _ = write_http_response(
                        &mut stream,
                        401,
                        "text/plain; charset=utf-8",
                        b"pending enrollment mailbox retrieval refused",
                    )
                    .await;
                }
            }
        }
        ("POST", "/v2/bootstrap/enrollment-return-mailbox/register") => {
            let Ok(payload) =
                serde_json::from_slice::<BootstrapIssuerReturnMailboxRegistration>(&request.body)
            else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"invalid return mailbox registration",
                )
                .await;
                return;
            };
            if constant_time_eq_str(&payload.retrieval_credential, &auth_token) {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"return credential must be scoped independently",
                )
                .await;
                return;
            }
            let Some(provider) = mailbox_time.as_deref() else {
                let _ = write_http_response(
                    &mut stream,
                    503,
                    "text/plain; charset=utf-8",
                    b"selected physical-time provider unavailable",
                )
                .await;
                return;
            };
            if payload
                .registration
                .verify_untrusted_broker_registration(
                    &aura_effects::crypto::RealCryptoHandler::new(),
                    provider,
                )
                .await
                .is_err()
            {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"return issuer proof rejected",
                )
                .await;
                return;
            }
            let now_ms = mailbox_now!();
            let result = shared.write().await.enrollment_return_mailboxes.register(
                payload.registration,
                &payload.retrieval_credential,
                now_ms,
            );
            let status = if result.is_ok() { 204 } else { 429 };
            let _ =
                write_http_response(&mut stream, status, "text/plain; charset=utf-8", b"").await;
        }
        ("GET", path) if path.starts_with("/v2/bootstrap/enrollment-return-mailbox/register/") => {
            let Some(key) = enrollment_return_mailbox_key(
                path.trim_start_matches("/v2/bootstrap/enrollment-return-mailbox/register/"),
            ) else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"invalid return mailbox selector",
                )
                .await;
                return;
            };
            let now_ms = mailbox_now!();
            let found = shared
                .write()
                .await
                .enrollment_return_mailboxes
                .registration(key, now_ms);
            match serde_json::to_vec(&found) {
                Ok(body) => {
                    let _ = write_http_response(&mut stream, 200, "application/json", &body).await;
                }
                Err(_) => {
                    let _ = write_http_response(
                        &mut stream,
                        500,
                        "text/plain; charset=utf-8",
                        b"return registration encoding failed",
                    )
                    .await;
                }
            }
        }
        ("POST", "/v2/bootstrap/enrollment-return-mailbox/envelopes") => {
            let Ok(payload) =
                serde_json::from_slice::<BootstrapIssuerReturnMailboxEnvelope>(&request.body)
            else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"invalid return VM envelope",
                )
                .await;
                return;
            };
            let Some(issuer_device) = payload
                .envelope
                .metadata
                .get("aura-destination-device-id")
                .and_then(|raw| DeviceId::from_str(raw).ok())
            else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"return VM envelope lacks issuer device",
                )
                .await;
                return;
            };
            let key = ReturnMailboxKey::new(
                payload.envelope.destination,
                issuer_device,
                payload.manifest_digest,
            );
            let now_ms = mailbox_now!();
            let Some(registration) = shared
                .write()
                .await
                .enrollment_return_mailboxes
                .registration(key, now_ms)
            else {
                let _ = write_http_response(
                    &mut stream,
                    404,
                    "text/plain; charset=utf-8",
                    b"return mailbox registration absent",
                )
                .await;
                return;
            };
            let Some(provider) = mailbox_time.as_deref() else {
                let _ = write_http_response(
                    &mut stream,
                    503,
                    "text/plain; charset=utf-8",
                    b"selected physical-time provider unavailable",
                )
                .await;
                return;
            };
            if payload
                .authorization
                .verify_untrusted_broker_admission(
                    &aura_effects::crypto::RealCryptoHandler::new(),
                    provider,
                    &registration,
                    &payload.setup_request,
                    &payload.envelope,
                )
                .await
                .is_err()
            {
                let _ = write_http_response(
                    &mut stream,
                    403,
                    "text/plain; charset=utf-8",
                    b"return setup-key envelope authorization rejected",
                )
                .await;
                return;
            }
            let now_ms = mailbox_now!();
            let result = shared
                .write()
                .await
                .enrollment_return_mailboxes
                .enqueue_verified(key, &registration, payload, now_ms);
            let status = if result.is_ok() { 204 } else { 429 };
            let _ =
                write_http_response(&mut stream, status, "text/plain; charset=utf-8", b"").await;
        }
        ("GET", path) if path.starts_with("/v2/bootstrap/enrollment-return-mailbox/envelopes/") => {
            let Some(key) = enrollment_return_mailbox_key(
                path.trim_start_matches("/v2/bootstrap/enrollment-return-mailbox/envelopes/"),
            ) else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"invalid return mailbox selector",
                )
                .await;
                return;
            };
            let Some(credential) =
                request_header(&request, ENROLLMENT_RETURN_MAILBOX_RETRIEVAL_HEADER)
            else {
                let _ = write_http_response(
                    &mut stream,
                    401,
                    "text/plain; charset=utf-8",
                    b"missing return retrieval credential",
                )
                .await;
                return;
            };
            let now_ms = mailbox_now!();
            let result = shared
                .write()
                .await
                .enrollment_return_mailboxes
                .take(key, credential, now_ms);
            match result.and_then(|deliveries| {
                serde_json::to_vec(&deliveries).map_err(|_| "return envelope encoding failed")
            }) {
                Ok(body) => {
                    let _ = write_http_response(&mut stream, 200, "application/json", &body).await;
                }
                Err(_) => {
                    let _ = write_http_response(
                        &mut stream,
                        401,
                        "text/plain; charset=utf-8",
                        b"return retrieval refused",
                    )
                    .await;
                }
            }
        }
        ("POST", path) if path.starts_with("/v2/bootstrap/enrollment-return-mailbox/ack/") => {
            let Some(key) = enrollment_return_mailbox_key(
                path.trim_start_matches("/v2/bootstrap/enrollment-return-mailbox/ack/"),
            ) else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"invalid return mailbox selector",
                )
                .await;
                return;
            };
            let Some(credential) =
                request_header(&request, ENROLLMENT_RETURN_MAILBOX_RETRIEVAL_HEADER)
            else {
                let _ = write_http_response(
                    &mut stream,
                    401,
                    "text/plain; charset=utf-8",
                    b"missing return retrieval credential",
                )
                .await;
                return;
            };
            let Ok(ack) =
                serde_json::from_slice::<BootstrapPendingEnrollmentMailboxAck>(&request.body)
            else {
                let _ = write_http_response(
                    &mut stream,
                    400,
                    "text/plain; charset=utf-8",
                    b"invalid return mailbox acknowledgment",
                )
                .await;
                return;
            };
            let now_ms = mailbox_now!();
            let result = shared
                .write()
                .await
                .enrollment_return_mailboxes
                .acknowledge(key, credential, &ack.message_id, now_ms);
            let status = if result.is_ok() { 204 } else { 401 };
            let _ =
                write_http_response(&mut stream, status, "text/plain; charset=utf-8", b"").await;
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
fn invitation_request_recipient(path_tail: &str) -> Option<(&str, DeviceId)> {
    if path_tail.is_empty() || path_tail.contains('?') {
        return None;
    }
    let (authority, device) = path_tail.split_once('/')?;
    AuthorityId::from_str(authority).ok()?;
    Some((authority, DeviceId::from_str(device).ok()?))
}

#[cfg(not(target_arch = "wasm32"))]
fn pending_enrollment_mailbox_key(path_tail: &str) -> Option<PendingMailboxKey> {
    if path_tail.contains('?') {
        return None;
    }
    let mut parts = path_tail.split('/');
    let subject = AuthorityId::from_str(parts.next()?).ok()?;
    let device = DeviceId::from_str(parts.next()?).ok()?;
    let digest_bytes = hex::decode(parts.next()?).ok()?;
    if parts.next().is_some() || digest_bytes.len() != 32 {
        return None;
    }
    let mut digest = [0; 32];
    digest.copy_from_slice(&digest_bytes);
    Some(PendingMailboxKey::new(subject, device, digest))
}

#[cfg(not(target_arch = "wasm32"))]
fn enrollment_return_mailbox_key(path_tail: &str) -> Option<ReturnMailboxKey> {
    if path_tail.contains('?') {
        return None;
    }
    let mut parts = path_tail.split('/');
    let subject = AuthorityId::from_str(parts.next()?).ok()?;
    let issuer_device = DeviceId::from_str(parts.next()?).ok()?;
    let digest_bytes = hex::decode(parts.next()?).ok()?;
    if parts.next().is_some() || digest_bytes.len() != 32 {
        return None;
    }
    let mut digest = [0; 32];
    digest.copy_from_slice(&digest_bytes);
    Some(ReturnMailboxKey::new(subject, issuer_device, digest))
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
    let body_limit = if method == "POST"
        && (path == "/v2/bootstrap/enrollment-mailbox/envelopes"
            || path == "/v2/bootstrap/enrollment-return-mailbox/envelopes")
    {
        MAX_ENROLLMENT_MAILBOX_BODY_BYTES
    } else {
        MAX_BOOTSTRAP_BODY_BYTES
    };
    if content_length > body_limit {
        return Err("request body exceeds bootstrap broker limit".to_string());
    }

    let mut body = buffer[header_end..].to_vec();
    if body.len() > body_limit {
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
        if body.len() > body_limit {
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
        503 => "Service Unavailable",
        _ => "OK",
    };
    let headers = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization, {INVITATION_RETRIEVAL_HEADER}, {ENROLLMENT_MAILBOX_RETRIEVAL_HEADER}, {ENROLLMENT_RETURN_MAILBOX_RETRIEVAL_HEADER}\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

#[cfg(not(target_arch = "wasm32"))]
async fn with_candidate_time<T>(
    shared: &Arc<RwLock<BootstrapBrokerState>>,
    provider: Option<&dyn PhysicalTimeEffects>,
    action: impl FnOnce(&mut BootstrapBrokerState, u64) -> T,
) -> Result<T, String> {
    let now_ms = selected_mailbox_now_ms(provider).await?;
    let mut state = shared.write().await;
    if state
        .candidate_time_high_water_ms
        .is_some_and(|observed| now_ms < observed)
    {
        return Err("bootstrap broker selected physical time moved backward".to_string());
    }
    state.candidate_time_high_water_ms = Some(now_ms);
    Ok(action(&mut state, now_ms))
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}

pub fn register_endpoint(base_url: &str) -> String {
    format!("{}/v2/bootstrap/register", normalize_base_url(base_url))
}

pub fn candidates_endpoint(base_url: &str) -> String {
    format!("{}/v2/bootstrap/candidates", normalize_base_url(base_url))
}

pub fn invitations_endpoint(base_url: &str, authority_id: &str, device_id: DeviceId) -> String {
    format!(
        "{}/v2/bootstrap/invitations/{}/{}",
        normalize_base_url(base_url),
        authority_id,
        device_id
    )
}

pub fn pending_enrollment_mailbox_endpoint(
    base_url: &str,
    resource: &str,
    subject: AuthorityId,
    device: DeviceId,
    manifest_digest: [u8; 32],
) -> String {
    format!(
        "{}/v2/bootstrap/enrollment-mailbox/{resource}/{subject}/{device}/{}",
        normalize_base_url(base_url),
        hex::encode(manifest_digest)
    )
}

#[cfg(test)]
pub(crate) async fn fetch_pending_progress_witness_for_test(
    base_url: &str,
    auth_token: &str,
    subject: AuthorityId,
    device: DeviceId,
    manifest_digest: [u8; 32],
) -> Result<serde_json::Value, aura_core::AuraError> {
    let body = send_http_request(
        "GET",
        &pending_enrollment_mailbox_endpoint(
            base_url,
            "progress-witness",
            subject,
            device,
            manifest_digest,
        ),
        auth_token,
        &[],
        None,
    )
    .await?;
    serde_json::from_slice(&body).map_err(aura_core::AuraError::from)
}

pub fn issuer_return_mailbox_endpoint(
    base_url: &str,
    resource: &str,
    subject: AuthorityId,
    issuer_device: DeviceId,
    manifest_digest: [u8; 32],
) -> String {
    format!(
        "{}/v2/bootstrap/enrollment-return-mailbox/{resource}/{subject}/{issuer_device}/{}",
        normalize_base_url(base_url),
        hex::encode(manifest_digest)
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

/// Actual unsuccessful HTTP status observed at the broker client boundary.
#[derive(Debug, thiserror::Error)]
#[error("bootstrap broker returned HTTP {status}")]
pub struct BootstrapBrokerHttpStatus {
    status: u16,
}
impl BootstrapBrokerHttpStatus {
    #[cfg(test)]
    /// Status from the actual broker response.
    pub fn status(&self) -> u16 {
        self.status
    }
}
fn broker_client_failure(
    stage: &'static str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> aura_core::AuraError {
    aura_core::AuraError::Network {
        message: stage.into(),
        source: Some(std::sync::Arc::new(source)),
    }
}
fn require_success_status(status: u16) -> Result<(), aura_core::AuraError> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(broker_client_failure(
            "bootstrap broker rejected request",
            BootstrapBrokerHttpStatus { status },
        ))
    }
}

#[cfg(all(not(target_arch = "wasm32"), test))]
fn parse_http_endpoint(url: &str) -> Result<(String, String), aura_core::AuraError> {
    parse_http_endpoint_with_policy(url, BootstrapBrokerClientPolicy::LoopbackOnly)
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_http_endpoint_with_policy(
    url: &str,
    policy: BootstrapBrokerClientPolicy,
) -> Result<(String, String), aura_core::AuraError> {
    if url.starts_with("https://") {
        return Err(aura_core::AuraError::invalid(
            "HTTPS client not available in this native build",
        ));
    }
    let without_scheme = url
        .strip_prefix("http://")
        .ok_or_else(|| aura_core::AuraError::invalid("bootstrap broker URL requires http://"))?;
    let (host_port, path) = match without_scheme.split_once('/') {
        Some((host_port, path)) => (host_port.to_string(), format!("/{path}")),
        None => (without_scheme.to_string(), "/".to_string()),
    };
    if host_port.is_empty()
        || host_port.contains('@')
        || host_port.contains('?')
        || host_port.contains('#')
        || path.contains('\r')
        || path.contains('\n')
    {
        return Err(aura_core::AuraError::invalid(
            "invalid bootstrap broker endpoint",
        ));
    }
    let socket = host_port.parse::<std::net::SocketAddr>().ok();
    let localhost_port = host_port
        .strip_prefix("localhost:")
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0);
    if socket.is_none_or(|addr| addr.port() == 0) && localhost_port.is_none() {
        return Err(aura_core::AuraError::invalid(
            "invalid bootstrap broker endpoint",
        ));
    }
    let loopback = localhost_port.is_some() || socket.is_some_and(|addr| addr.ip().is_loopback());
    let private_lan =
        socket.is_some_and(|addr| matches!(addr.ip(), std::net::IpAddr::V4(ip) if ip.is_private()));
    if !loopback && !(policy.allows_private_lan() && private_lan) {
        return Err(aura_core::AuraError::invalid(
            "plain HTTP bootstrap broker URLs are restricted to loopback endpoints",
        ));
    }
    Ok((host_port, path))
}

#[cfg(not(target_arch = "wasm32"))]
async fn send_http_request(
    method: &str,
    url: &str,
    auth_token: &str,
    extra_headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<Vec<u8>, aura_core::AuraError> {
    send_http_request_with_policy(
        method,
        url,
        auth_token,
        extra_headers,
        body,
        BootstrapBrokerClientPolicy::LoopbackOnly,
    )
    .await
}

#[cfg(not(target_arch = "wasm32"))]
async fn send_http_request_with_policy(
    method: &str,
    url: &str,
    auth_token: &str,
    extra_headers: &[(&str, &str)],
    body: Option<&[u8]>,
    policy: BootstrapBrokerClientPolicy,
) -> Result<Vec<u8>, aura_core::AuraError> {
    let (host_port, path) = parse_http_endpoint_with_policy(url, policy)?;
    let mut stream = TcpStream::connect(&host_port)
        .await
        .map_err(|source| broker_client_failure("bootstrap broker connect failed", source))?;
    let body = body.unwrap_or_default();
    let mut extra_header_text = String::new();
    for (name, value) in extra_headers {
        extra_header_text.push_str(&format!("{name}: {value}\r\n"));
    }
    let request = format!("{method} {path} HTTP/1.1\r\nHost: {host_port}\r\nAuthorization: Bearer {auth_token}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/json\r\n{extra_header_text}\r\n", body.len());
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|source| broker_client_failure("bootstrap broker request write failed", source))?;
    if !body.is_empty() {
        stream.write_all(body).await.map_err(|source| {
            broker_client_failure("bootstrap broker body write failed", source)
        })?;
    }
    stream
        .flush()
        .await
        .map_err(|source| broker_client_failure("bootstrap broker flush failed", source))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .map_err(|source| broker_client_failure("bootstrap broker response read failed", source))?;
    let header_end = response
        .windows(4)
        .position(|chunk| chunk == b"\r\n\r\n")
        .map(|offset| offset + 4)
        .ok_or_else(|| {
            aura_core::AuraError::invalid("bootstrap broker response missing headers")
        })?;
    let header_text = std::str::from_utf8(&response[..header_end])
        .map_err(|source| broker_client_failure("bootstrap broker header decode failed", source))?;
    let status_text = header_text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| aura_core::AuraError::invalid("bootstrap broker response missing status"))?;
    let status = status_text
        .parse::<u16>()
        .map_err(|source| broker_client_failure("bootstrap broker status decode failed", source))?;
    require_success_status(status)?;
    Ok(response[header_end..].to_vec())
}

#[cfg(target_arch = "wasm32")]
async fn send_http_request(
    method: &str,
    url: &str,
    auth_token: &str,
    extra_headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<Vec<u8>, aura_core::AuraError> {
    let mut builder = match method {
        "GET" => gloo_net::http::Request::get(url),
        "POST" => gloo_net::http::Request::post(url),
        _ => {
            return Err(aura_core::AuraError::invalid(
                "unsupported bootstrap broker request method",
            ))
        }
    }
    .header("authorization", &format!("Bearer {auth_token}"));
    for (name, value) in extra_headers {
        builder = builder.header(name, value);
    }
    let request = match body {
        Some(body) => builder.header("content-type", "application/json").body(
            std::str::from_utf8(body).map_err(|source| {
                broker_client_failure("bootstrap broker body decode failed", source)
            })?,
        ),
        None => builder.build(),
    }
    .map_err(|source| broker_client_failure("bootstrap broker request build failed", source))?;
    let response = request
        .send()
        .await
        .map_err(|source| broker_client_failure("bootstrap broker request failed", source))?;
    require_success_status(response.status())?;
    response
        .binary()
        .await
        .map_err(|source| broker_client_failure("bootstrap broker response read failed", source))
}

#[cfg(target_arch = "wasm32")]
async fn send_http_request_with_policy(
    method: &str,
    url: &str,
    auth_token: &str,
    extra_headers: &[(&str, &str)],
    body: Option<&[u8]>,
    _policy: BootstrapBrokerClientPolicy,
) -> Result<Vec<u8>, aura_core::AuraError> {
    send_http_request(method, url, auth_token, extra_headers, body).await
}

pub async fn register_remote_candidate(
    base_url: &str,
    auth_token: &str,
    registration: &BootstrapBrokerRegistration,
) -> Result<(), aura_core::AuraError> {
    let body = serde_json::to_vec(registration).map_err(aura_core::AuraError::from)?;
    send_http_request(
        "POST",
        &register_endpoint(base_url),
        auth_token,
        &[],
        Some(&body),
    )
    .await?;
    Ok(())
}
pub async fn fetch_remote_candidates(
    base_url: &str,
    auth_token: &str,
) -> Result<Vec<BootstrapBrokerCandidateRecord>, aura_core::AuraError> {
    let body =
        send_http_request("GET", &candidates_endpoint(base_url), auth_token, &[], None).await?;
    serde_json::from_slice(&body).map_err(aura_core::AuraError::from)
}
pub async fn send_remote_invitation(
    base_url: &str,
    auth_token: &str,
    invitation: &BootstrapBrokerInvitation,
) -> Result<(), aura_core::AuraError> {
    let body = serde_json::to_vec(invitation).map_err(aura_core::AuraError::from)?;
    send_http_request(
        "POST",
        &format!("{}/v2/bootstrap/invitations", normalize_base_url(base_url)),
        auth_token,
        &[],
        Some(&body),
    )
    .await?;
    Ok(())
}
pub async fn take_remote_invitations(
    base_url: &str,
    auth_token: &str,
    authority_id: &str,
    device_id: DeviceId,
    retrieval_token: &str,
) -> Result<Vec<BootstrapBrokerInvitation>, aura_core::AuraError> {
    let body = send_http_request(
        "GET",
        &invitations_endpoint(base_url, authority_id, device_id),
        auth_token,
        &[(INVITATION_RETRIEVAL_HEADER, retrieval_token)],
        None,
    )
    .await?;
    serde_json::from_slice(&body).map_err(aura_core::AuraError::from)
}

pub(crate) async fn register_remote_pending_enrollment_mailbox(
    base_url: &str,
    auth_token: &str,
    registration: &BootstrapPendingEnrollmentMailboxRegistration,
    policy: BootstrapBrokerClientPolicy,
) -> Result<(), aura_core::AuraError> {
    let body = serde_json::to_vec(registration).map_err(aura_core::AuraError::from)?;
    send_http_request_with_policy(
        "POST",
        &format!(
            "{}/v2/bootstrap/enrollment-mailbox/register",
            normalize_base_url(base_url)
        ),
        auth_token,
        &[],
        Some(&body),
        policy,
    )
    .await?;
    Ok(())
}

pub(crate) async fn fetch_remote_pending_enrollment_mailbox_registration(
    base_url: &str,
    auth_token: &str,
    subject: AuthorityId,
    device: DeviceId,
    manifest_digest: [u8; 32],
    policy: BootstrapBrokerClientPolicy,
) -> Result<
    Option<aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxRegistration>,
    aura_core::AuraError,
> {
    let body = send_http_request_with_policy(
        "GET",
        &pending_enrollment_mailbox_endpoint(
            base_url,
            "register",
            subject,
            device,
            manifest_digest,
        ),
        auth_token,
        &[],
        None,
        policy,
    )
    .await?;
    serde_json::from_slice(&body).map_err(aura_core::AuraError::from)
}

pub(crate) async fn send_remote_pending_enrollment_vm_envelope(
    base_url: &str,
    auth_token: &str,
    payload: &BootstrapPendingEnrollmentMailboxEnvelope,
    policy: BootstrapBrokerClientPolicy,
) -> Result<(), aura_core::AuraError> {
    let body = serde_json::to_vec(payload).map_err(aura_core::AuraError::from)?;
    send_http_request_with_policy(
        "POST",
        &format!(
            "{}/v2/bootstrap/enrollment-mailbox/envelopes",
            normalize_base_url(base_url)
        ),
        auth_token,
        &[],
        Some(&body),
        policy,
    )
    .await?;
    Ok(())
}

pub(crate) async fn take_remote_pending_enrollment_vm_envelopes(
    base_url: &str,
    auth_token: &str,
    subject: AuthorityId,
    device: DeviceId,
    manifest_digest: [u8; 32],
    retrieval_credential: &str,
    policy: BootstrapBrokerClientPolicy,
) -> Result<Vec<PendingMailboxDelivery>, aura_core::AuraError> {
    let body = send_http_request_with_policy(
        "GET",
        &pending_enrollment_mailbox_endpoint(
            base_url,
            "envelopes",
            subject,
            device,
            manifest_digest,
        ),
        auth_token,
        &[(ENROLLMENT_MAILBOX_RETRIEVAL_HEADER, retrieval_credential)],
        None,
        policy,
    )
    .await?;
    serde_json::from_slice(&body).map_err(aura_core::AuraError::from)
}

pub(crate) async fn acknowledge_remote_pending_enrollment_vm_envelope(
    base_url: &str,
    auth_token: &str,
    subject: AuthorityId,
    device: DeviceId,
    manifest_digest: [u8; 32],
    retrieval_credential: &str,
    message_id: &str,
    policy: BootstrapBrokerClientPolicy,
) -> Result<(), aura_core::AuraError> {
    let body = serde_json::to_vec(&BootstrapPendingEnrollmentMailboxAck {
        message_id: message_id.to_owned(),
    })
    .map_err(aura_core::AuraError::from)?;
    send_http_request_with_policy(
        "POST",
        &pending_enrollment_mailbox_endpoint(base_url, "ack", subject, device, manifest_digest),
        auth_token,
        &[(ENROLLMENT_MAILBOX_RETRIEVAL_HEADER, retrieval_credential)],
        Some(&body),
        policy,
    )
    .await?;
    Ok(())
}

pub(crate) async fn register_remote_issuer_return_mailbox(
    base_url: &str,
    auth_token: &str,
    payload: &BootstrapIssuerReturnMailboxRegistration,
    policy: BootstrapBrokerClientPolicy,
) -> Result<(), aura_core::AuraError> {
    let body = serde_json::to_vec(payload).map_err(aura_core::AuraError::from)?;
    send_http_request_with_policy(
        "POST",
        &format!(
            "{}/v2/bootstrap/enrollment-return-mailbox/register",
            normalize_base_url(base_url)
        ),
        auth_token,
        &[],
        Some(&body),
        policy,
    )
    .await?;
    Ok(())
}

pub(crate) async fn fetch_remote_issuer_return_mailbox_registration(
    base_url: &str,
    auth_token: &str,
    subject: AuthorityId,
    issuer_device: DeviceId,
    manifest_digest: [u8; 32],
    policy: BootstrapBrokerClientPolicy,
) -> Result<
    Option<aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration>,
    aura_core::AuraError,
> {
    let body = send_http_request_with_policy(
        "GET",
        &issuer_return_mailbox_endpoint(
            base_url,
            "register",
            subject,
            issuer_device,
            manifest_digest,
        ),
        auth_token,
        &[],
        None,
        policy,
    )
    .await?;
    serde_json::from_slice(&body).map_err(aura_core::AuraError::from)
}

pub(crate) async fn send_remote_issuer_return_vm_envelope(
    base_url: &str,
    auth_token: &str,
    payload: &BootstrapIssuerReturnMailboxEnvelope,
    policy: BootstrapBrokerClientPolicy,
) -> Result<(), aura_core::AuraError> {
    let body = serde_json::to_vec(payload).map_err(aura_core::AuraError::from)?;
    send_http_request_with_policy(
        "POST",
        &format!(
            "{}/v2/bootstrap/enrollment-return-mailbox/envelopes",
            normalize_base_url(base_url)
        ),
        auth_token,
        &[],
        Some(&body),
        policy,
    )
    .await?;
    Ok(())
}

pub(crate) async fn take_remote_issuer_return_vm_envelopes(
    base_url: &str,
    auth_token: &str,
    subject: AuthorityId,
    issuer_device: DeviceId,
    manifest_digest: [u8; 32],
    retrieval_credential: &str,
    policy: BootstrapBrokerClientPolicy,
) -> Result<Vec<IssuerReturnMailboxDelivery>, aura_core::AuraError> {
    let body = send_http_request_with_policy(
        "GET",
        &issuer_return_mailbox_endpoint(
            base_url,
            "envelopes",
            subject,
            issuer_device,
            manifest_digest,
        ),
        auth_token,
        &[(
            ENROLLMENT_RETURN_MAILBOX_RETRIEVAL_HEADER,
            retrieval_credential,
        )],
        None,
        policy,
    )
    .await?;
    serde_json::from_slice(&body).map_err(aura_core::AuraError::from)
}

pub(crate) async fn acknowledge_remote_issuer_return_vm_envelope(
    base_url: &str,
    auth_token: &str,
    subject: AuthorityId,
    issuer_device: DeviceId,
    manifest_digest: [u8; 32],
    retrieval_credential: &str,
    message_id: &str,
    policy: BootstrapBrokerClientPolicy,
) -> Result<(), aura_core::AuraError> {
    let body = serde_json::to_vec(&BootstrapPendingEnrollmentMailboxAck {
        message_id: message_id.to_owned(),
    })
    .map_err(aura_core::AuraError::from)?;
    send_http_request_with_policy(
        "POST",
        &issuer_return_mailbox_endpoint(base_url, "ack", subject, issuer_device, manifest_digest),
        auth_token,
        &[(
            ENROLLMENT_RETURN_MAILBOX_RETRIEVAL_HEADER,
            retrieval_credential,
        )],
        Some(&body),
        policy,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    use crate::runtime::TaskSupervisor;

    const AUTH_TOKEN: &str = "test-broker-auth-token";
    const INVITE_TOKEN: &str = "test-invitation-retrieval-token";

    #[cfg(all(feature = "simulator-checkpoint", not(target_arch = "wasm32")))]
    async fn checkpoint_broker() -> LocalBootstrapBrokerService {
        LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("original broker bind")
        .with_mailbox_time_provider(Arc::new(aura_effects::time::PhysicalTimeHandler::new()))
    }

    #[cfg(all(feature = "simulator-checkpoint", not(target_arch = "wasm32")))]
    #[tokio::test]
    async fn simulator_broker_quiescence_stops_original_owned_group() {
        let broker = checkpoint_broker().await;
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("checkpoint_original_broker");
        broker.start(&group);
        assert_eq!(group.active_tasks(), ["bootstrap_broker_http"]);
        let stopped = broker
            .stop_for_simulator_checkpoint(Duration::from_secs(5))
            .await
            .expect("original broker subtree disposed");
        assert!(group.active_tasks().is_empty());
        assert!(stopped._state.invitations.is_empty());
        let bytes = stopped
            .into_unsealed_registrations()
            .expect("bounded quiescent codec");
        let decoded = decode_quiescent_broker_registrations(&bytes, Duration::from_secs(60), 1)
            .expect("fresh private state reconstruction");
        assert!(decoded.candidates.is_empty());
        assert!(
            decode_quiescent_broker_registrations(&bytes, Duration::from_secs(60), 0).is_some()
        );
        let mut changed = bytes.clone();
        changed[0] ^= 1;
        assert!(
            decode_quiescent_broker_registrations(&changed, Duration::from_secs(60), 1).is_none()
        );
        let mut version: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        version["version"] = serde_json::json!(2);
        assert!(decode_quiescent_broker_registrations(
            &serde_json::to_vec(&version).unwrap(),
            Duration::from_secs(60),
            1
        )
        .is_none());

        let mut stale: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        stale["candidate_time_high_water_ms"] = serde_json::json!(42);
        assert!(decode_quiescent_broker_registrations(
            &serde_json::to_vec(&stale).unwrap(),
            Duration::from_secs(60),
            41
        )
        .is_none());
        let candidate = BootstrapBrokerCandidateRecord {
            authority_id: AuthorityId::new_from_entropy([4; 32]).to_string(),
            device_id: DeviceId::from_bytes([5; 32]),
            address: "127.0.0.1:5000".to_owned(),
            nickname_suggestion: None,
            discovered_at_ms: 1,
        };
        let mut duplicate: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        duplicate["candidates"] = serde_json::json!([candidate.clone(), candidate]);
        assert!(decode_quiescent_broker_registrations(
            &serde_json::to_vec(&duplicate).unwrap(),
            Duration::from_secs(60),
            2
        )
        .is_none());
    }

    #[cfg(all(feature = "simulator-checkpoint", not(target_arch = "wasm32")))]
    #[tokio::test]
    async fn simulator_broker_quiescence_refuses_unstarted_and_repeated_start() {
        let unstarted = checkpoint_broker().await;
        assert!(matches!(
            unstarted
                .stop_for_simulator_checkpoint(Duration::from_secs(5))
                .await,
            Err(BrokerCheckpointQuiescenceError::NotStarted)
        ));
        let repeated = checkpoint_broker().await;
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("checkpoint_repeated_broker");
        repeated.start(&group);
        repeated.start(&group);
        assert!(matches!(
            repeated
                .stop_for_simulator_checkpoint(Duration::from_secs(5))
                .await,
            Err(BrokerCheckpointQuiescenceError::RepeatedStart)
        ));
        assert!(group.active_tasks().is_empty());
    }

    #[cfg(all(feature = "simulator-checkpoint", not(target_arch = "wasm32")))]
    #[tokio::test]
    async fn simulator_broker_quiescence_refuses_foreign_task_and_pending_invitation() {
        let broker = checkpoint_broker().await;
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("checkpoint_foreign_task");
        broker.start(&group);
        let _foreign = group.spawn_named("foreign_runtime_owner", std::future::pending());
        assert!(matches!(
            broker
                .stop_for_simulator_checkpoint(Duration::from_secs(5))
                .await,
            Err(BrokerCheckpointQuiescenceError::ForeignTask)
        ));
        assert!(group.active_tasks().is_empty());

        let broker = checkpoint_broker().await;
        let group = supervisor.group("checkpoint_pending_invitation");
        broker.start(&group);
        broker
            .queue_invitation(BootstrapBrokerInvitation {
                recipient_authority_id: "11111111-1111-1111-1111-111111111111".into(),
                recipient_device_id: DeviceId::from_bytes([7; 32]),
                purpose: InvitationPurpose::DeviceEnrollment,
                invitation_code: "pending-private-code".into(),
            })
            .await
            .expect("pending delivery retained");
        assert!(matches!(
            broker
                .stop_for_simulator_checkpoint(Duration::from_secs(5))
                .await,
            Err(BrokerCheckpointQuiescenceError::UnsettledDelivery)
        ));
        assert!(group.active_tasks().is_empty());
    }

    #[cfg(all(feature = "simulator-checkpoint", not(target_arch = "wasm32")))]
    #[tokio::test]
    async fn simulator_broker_quiescence_refuses_unserializable_owner_state() {
        let broker = checkpoint_broker().await;
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("checkpoint_unserializable_owner");
        broker.start(&group);
        broker.shared.write().await.invitation_credentials.insert(
            (
                "orphaned credential".to_owned(),
                DeviceId::from_bytes([3; 32]),
            ),
            "private token".to_owned(),
        );
        assert!(matches!(
            broker
                .stop_for_simulator_checkpoint(Duration::from_secs(5))
                .await,
            Err(BrokerCheckpointQuiescenceError::UnsupportedState)
        ));
        assert!(group.active_tasks().is_empty());

        let broker = checkpoint_broker().await;
        let group = supervisor.group("checkpoint_unserializable_mailbox");
        broker.start(&group);
        broker
            .shared
            .write()
            .await
            .pending_enrollment_mailboxes
            .fail_next_ack_for_test();
        assert!(matches!(
            broker
                .stop_for_simulator_checkpoint(Duration::from_secs(5))
                .await,
            Err(BrokerCheckpointQuiescenceError::UnsupportedState)
        ));
        assert!(group.active_tasks().is_empty());

        let broker = checkpoint_broker().await;
        let group = supervisor.group("checkpoint_unserializable_pause");
        broker.start(&group);
        let (_entered, _release) = broker.pause_next_pending_registration_for_test().await;
        assert!(matches!(
            broker
                .stop_for_simulator_checkpoint(Duration::from_secs(5))
                .await,
            Err(BrokerCheckpointQuiescenceError::UnsupportedState)
        ));
        assert!(group.active_tasks().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn return_mailbox_http_requires_selected_physical_time_provider() {
        let without_time = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("broker without selected time should bind");
        let with_time = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("broker with selected time should bind")
        .with_mailbox_time_provider(Arc::new(aura_effects::time::PhysicalTimeHandler::new()));
        let supervisor = TaskSupervisor::new();
        without_time.start(&supervisor.group("return_mailbox_without_time"));
        with_time.start(&supervisor.group("return_mailbox_with_time"));
        tokio::task::yield_now().await;

        let subject: AuthorityId = uuid::Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
            .expect("fixed test authority id")
            .into();
        let issuer_device = DeviceId::from_bytes([7; 32]);
        let digest = [9; 32];
        let error = fetch_remote_issuer_return_mailbox_registration(
            without_time.public_url(),
            AUTH_TOKEN,
            subject,
            issuer_device,
            digest,
            BootstrapBrokerClientPolicy::LoopbackOnly,
        )
        .await
        .expect_err("missing selected provider must refuse return-mailbox reads");
        assert_eq!(
            std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<BootstrapBrokerHttpStatus>()
                .unwrap()
                .status(),
            503
        );
        assert!(fetch_remote_issuer_return_mailbox_registration(
            with_time.public_url(),
            AUTH_TOKEN,
            subject,
            issuer_device,
            digest,
            BootstrapBrokerClientPolicy::LoopbackOnly,
        )
        .await
        .expect("selected provider permits a valid empty result")
        .is_none());
        let error = fetch_remote_issuer_return_mailbox_registration(
            with_time.public_url(),
            "wrong-token",
            subject,
            issuer_device,
            digest,
            BootstrapBrokerClientPolicy::LoopbackOnly,
        )
        .await
        .expect_err("selected provider must not bypass bearer authorization");
        assert_eq!(
            std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<BootstrapBrokerHttpStatus>()
                .unwrap()
                .status(),
            401
        );
        supervisor
            .shutdown_with_timeout(Duration::from_secs(5))
            .await
            .expect("broker shutdown");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn broker_client_mixed_purposes_preserve_every_original_delivery() {
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("actual broker bind")
        .with_mailbox_time_provider(Arc::new(aura_effects::time::PhysicalTimeHandler::new()));
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("broker_mixed_purposes"));
        let authority = "fedcba98-7654-3210-fedc-ba9876543210";
        let device = DeviceId::from_bytes([7; 32]);
        register_remote_candidate(
            broker.public_url(),
            AUTH_TOKEN,
            &BootstrapBrokerRegistration {
                authority_id: authority.into(),
                device_id: device,
                address: "observed-browser-device".into(),
                invitation_retrieval_token: INVITE_TOKEN.into(),
                nickname_suggestion: None,
            },
        )
        .await
        .expect("actual registration receipt");
        let deliveries = [
            (InvitationPurpose::Contact, "original-contact"),
            (InvitationPurpose::DeviceEnrollment, "original-enrollment"),
            (InvitationPurpose::Guardian, "original-guardian"),
            (InvitationPurpose::Channel, "original-channel"),
        ]
        .map(|(purpose, code)| BootstrapBrokerInvitation {
            recipient_authority_id: authority.into(),
            recipient_device_id: device,
            purpose,
            invitation_code: code.into(),
        });
        for delivery in &deliveries {
            send_remote_invitation(broker.public_url(), AUTH_TOKEN, delivery)
                .await
                .expect("actual queue receipt");
        }
        let received = take_remote_invitations(
            broker.public_url(),
            AUTH_TOKEN,
            authority,
            device,
            INVITE_TOKEN,
        )
        .await
        .expect("actual one-use batch retrieval");
        assert_eq!(received, deliveries);
        let missing_purpose = serde_json::json!({
            "recipient_authority_id": authority,
            "recipient_device_id": device,
            "invitation_code": "untyped-code"
        });
        assert!(serde_json::from_value::<BootstrapBrokerInvitation>(missing_purpose).is_err());
        let replay = take_remote_invitations(
            broker.public_url(),
            AUTH_TOKEN,
            authority,
            device,
            INVITE_TOKEN,
        )
        .await
        .expect_err("nonempty retrieval consumes only its original credential");
        let status = std::error::Error::source(&replay)
            .and_then(|source| source.downcast_ref::<BootstrapBrokerHttpStatus>())
            .expect("actual HTTP rejection retained");
        assert_eq!(status.status(), 401);
        supervisor
            .shutdown_with_timeout(Duration::from_secs(5))
            .await
            .expect("actual broker task disposal");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn broker_client_same_authority_devices_keep_independent_delivery_custody() {
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(60),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .unwrap()
        .with_mailbox_time_provider(Arc::new(aura_effects::time::PhysicalTimeHandler::new()));
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("broker_device_isolation"));
        let authority = "fedcba98-7654-3210-fedc-ba9876543210";
        let first = DeviceId::from_bytes([1; 32]);
        let second = DeviceId::from_bytes([2; 32]);
        for (device, token) in [(first, "first-token"), (second, "second-token")] {
            register_remote_candidate(
                broker.public_url(),
                AUTH_TOKEN,
                &BootstrapBrokerRegistration {
                    authority_id: authority.into(),
                    device_id: device,
                    address: format!("observed-device-{device}"),
                    invitation_retrieval_token: token.into(),
                    nickname_suggestion: None,
                },
            )
            .await
            .unwrap();
        }
        let candidates = fetch_remote_candidates(broker.public_url(), AUTH_TOKEN)
            .await
            .unwrap();
        assert_eq!(candidates.len(), 2);
        assert!(candidates
            .iter()
            .any(|candidate| candidate.device_id == first));
        assert!(candidates
            .iter()
            .any(|candidate| candidate.device_id == second));
        assert!(take_remote_invitations(
            broker.public_url(),
            AUTH_TOKEN,
            authority,
            first,
            "first-token"
        )
        .await
        .unwrap()
        .is_empty());
        for (device, code) in [(first, "first-code"), (second, "second-code")] {
            send_remote_invitation(
                broker.public_url(),
                AUTH_TOKEN,
                &BootstrapBrokerInvitation {
                    recipient_authority_id: authority.into(),
                    recipient_device_id: device,
                    purpose: InvitationPurpose::Contact,
                    invitation_code: code.into(),
                },
            )
            .await
            .unwrap();
        }
        register_remote_candidate(
            broker.public_url(),
            AUTH_TOKEN,
            &BootstrapBrokerRegistration {
                authority_id: authority.into(),
                device_id: second,
                address: format!("observed-device-{second}"),
                invitation_retrieval_token: "refreshed-second-token".into(),
                nickname_suggestion: None,
            },
        )
        .await
        .unwrap();
        for (device, wrong) in [
            (first, "refreshed-second-token"),
            (second, "first-token"),
            (second, "second-token"),
        ] {
            let error =
                take_remote_invitations(broker.public_url(), AUTH_TOKEN, authority, device, wrong)
                    .await
                    .unwrap_err();
            assert_eq!(
                std::error::Error::source(&error)
                    .unwrap()
                    .downcast_ref::<BootstrapBrokerHttpStatus>()
                    .unwrap()
                    .status(),
                401
            );
        }
        for (device, token, expected) in [
            (first, "first-token", "first-code"),
            (second, "refreshed-second-token", "second-code"),
        ] {
            assert_eq!(
                take_remote_invitations(broker.public_url(), AUTH_TOKEN, authority, device, token)
                    .await
                    .unwrap(),
                [BootstrapBrokerInvitation {
                    recipient_authority_id: authority.into(),
                    recipient_device_id: device,
                    purpose: InvitationPurpose::Contact,
                    invitation_code: expected.into()
                }]
            );
            let error =
                take_remote_invitations(broker.public_url(), AUTH_TOKEN, authority, device, token)
                    .await
                    .unwrap_err();
            assert_eq!(
                std::error::Error::source(&error)
                    .unwrap()
                    .downcast_ref::<BootstrapBrokerHttpStatus>()
                    .unwrap()
                    .status(),
                401
            );
        }
        supervisor
            .shutdown_with_timeout(Duration::from_secs(5))
            .await
            .unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn broker_client_preflight_response_allows_invitation_retrieval_header() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let serve = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await.unwrap();
                headers.push(byte[0]);
                assert!(headers.len() < 8192);
            }
            assert!(headers.starts_with(b"OPTIONS "));
            write_http_response(&mut stream, 204, "text/plain; charset=utf-8", b"")
                .await
                .unwrap();
        };
        let request = async {
            let mut stream = TcpStream::connect(address).await.unwrap();
            let request = format!("OPTIONS /v2/bootstrap/invitations/test HTTP/1.1\r\nHost: {address}\r\nOrigin: http://localhost\r\nAccess-Control-Request-Method: GET\r\nAccess-Control-Request-Headers: authorization,{INVITATION_RETRIEVAL_HEADER}\r\nContent-Length: 0\r\n\r\n");
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await.unwrap();
            String::from_utf8(response).unwrap()
        };
        let ((), response) = futures::join!(serve, request);
        assert!(response.starts_with("HTTP/1.1 204 "));
        let allowed = response
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("access-control-allow-headers")
                    .then_some(value)
            })
            .expect("preflight must advertise allowed headers");
        for required in ["authorization", "content-type", INVITATION_RETRIEVAL_HEADER] {
            assert!(
                allowed
                    .split(',')
                    .any(|header| header.trim().eq_ignore_ascii_case(required)),
                "preflight must allow actual client header {required}"
            );
        }
    }

    #[test]
    fn broker_client_status_requires_actual_success() {
        for status in [200, 204, 299] {
            require_success_status(status).expect("actual successful status");
        }
        for status in [199, 300, 401, 500] {
            let error = require_success_status(status).expect_err("non-success must fail");
            let observed = std::error::Error::source(&error)
                .and_then(|source| source.downcast_ref::<BootstrapBrokerHttpStatus>())
                .expect("retain actual observed HTTP status");
            assert_eq!(observed.status(), status);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn broker_client_loopback_transport_failure_is_not_success() {
        let endpoint = "http://127.0.0.1:0";
        let registration = BootstrapBrokerRegistration {
            authority_id: "fedcba98-7654-3210-fedc-ba9876543210".into(),
            device_id: DeviceId::from_bytes([7; 32]),
            address: "127.0.0.1:40125".into(),
            invitation_retrieval_token: INVITE_TOKEN.into(),
            nickname_suggestion: None,
        };
        let invitation = BootstrapBrokerInvitation {
            recipient_authority_id: registration.authority_id.clone(),
            recipient_device_id: DeviceId::from_bytes([7; 32]),
            purpose: InvitationPurpose::Contact,
            invitation_code: "actual-test-invitation".into(),
        };
        let errors = [
            register_remote_candidate(endpoint, AUTH_TOKEN, &registration)
                .await
                .unwrap_err(),
            fetch_remote_candidates(endpoint, AUTH_TOKEN)
                .await
                .unwrap_err(),
            send_remote_invitation(endpoint, AUTH_TOKEN, &invitation)
                .await
                .unwrap_err(),
            take_remote_invitations(
                endpoint,
                AUTH_TOKEN,
                &registration.authority_id,
                DeviceId::from_bytes([7; 32]),
                INVITE_TOKEN,
            )
            .await
            .unwrap_err(),
        ];
        for error in errors {
            assert!(
                std::error::Error::source(&error)
                    .and_then(|source| source.downcast_ref::<std::io::Error>())
                    .is_some(),
                "retain concrete connect failure: {error:?}"
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn broker_client_successful_http_preserves_json_decode_failure() {
        for invitation_list in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let serve = async {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut headers = Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).await.unwrap();
                    headers.push(byte[0]);
                    assert!(headers.len() < 8192);
                }
                assert!(headers.starts_with(b"GET "));
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\n{",
                    )
                    .await
                    .unwrap();
                stream.shutdown().await.unwrap();
            };
            let query = async {
                if invitation_list {
                    take_remote_invitations(
                        &endpoint,
                        AUTH_TOKEN,
                        "fedcba98-7654-3210-fedc-ba9876543210",
                        DeviceId::from_bytes([7; 32]),
                        INVITE_TOKEN,
                    )
                    .await
                    .unwrap_err()
                } else {
                    fetch_remote_candidates(&endpoint, AUTH_TOKEN)
                        .await
                        .unwrap_err()
                }
            };
            let ((), error) = futures::join!(serve, query);
            assert!(
                std::error::Error::source(&error)
                    .and_then(|source| source.downcast_ref::<serde_json::Error>())
                    .is_some(),
                "retain concrete JSON failure after real HTTP success: {error:?}"
            );
        }
    }

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
        .expect("broker should bind")
        .with_mailbox_time_provider(Arc::new(aura_effects::time::PhysicalTimeHandler::new()));
        broker
            .register(BootstrapBrokerRegistration {
                authority_id: "01234567-89ab-cdef-0123-456789abcdef".to_string(),
                device_id: DeviceId::from_bytes([7; 32]),
                address: "127.0.0.1:40123".to_string(),
                invitation_retrieval_token: INVITE_TOKEN.to_string(),
                nickname_suggestion: Some("Alice".to_string()),
            })
            .await
            .expect("registration should be accepted");

        let candidates = broker.list_candidates().await.expect("selected clock");
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
    async fn broker_candidates_use_selected_clock_and_refuse_backward_observation() {
        let without_time = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_millis(10),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("broker bind");
        assert!(without_time.list_candidates().await.is_err());

        let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(100));
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_millis(10),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("broker bind")
        .with_mailbox_time_provider(clock.clone());
        let registration = BootstrapBrokerRegistration {
            authority_id: "01234567-89ab-cdef-0123-456789abcdef".to_string(),
            device_id: DeviceId::from_bytes([7; 32]),
            address: "127.0.0.1:40123".to_string(),
            invitation_retrieval_token: INVITE_TOKEN.to_string(),
            nickname_suggestion: None,
        };
        broker
            .register(registration.clone())
            .await
            .expect("first registration");
        assert_eq!(broker.list_candidates().await.unwrap().len(), 1);
        clock.set_time(99);
        assert!(broker.register(registration).await.is_err());
        assert!(broker.list_candidates().await.is_err());
        clock.set_time(100);
        assert_eq!(broker.list_candidates().await.unwrap().len(), 1);
        clock.set_time(110);
        assert!(broker.list_candidates().await.unwrap().is_empty());
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
        .expect("broker should bind")
        .with_mailbox_time_provider(Arc::new(aura_effects::time::PhysicalTimeHandler::new()));
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_test"));
        tokio::task::yield_now().await;

        register_remote_candidate(
            broker.public_url(),
            AUTH_TOKEN,
            &BootstrapBrokerRegistration {
                authority_id: "89abcdef-0123-4567-89ab-cdef01234567".to_string(),
                device_id: DeviceId::from_bytes([7; 32]),
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
        .expect("broker should bind")
        .with_mailbox_time_provider(Arc::new(aura_effects::time::PhysicalTimeHandler::new()));
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_invitation_test"));
        tokio::task::yield_now().await;

        register_remote_candidate(
            broker.public_url(),
            AUTH_TOKEN,
            &BootstrapBrokerRegistration {
                authority_id: "fedcba98-7654-3210-fedc-ba9876543210".to_string(),
                device_id: DeviceId::from_bytes([7; 32]),
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
                recipient_device_id: DeviceId::from_bytes([7; 32]),
                purpose: InvitationPurpose::Contact,
                invitation_code: "invite-123".to_string(),
            },
        )
        .await
        .expect("remote invitation send should succeed");

        let invitations = take_remote_invitations(
            broker.public_url(),
            AUTH_TOKEN,
            "fedcba98-7654-3210-fedc-ba9876543210",
            DeviceId::from_bytes([7; 32]),
            INVITE_TOKEN,
        )
        .await
        .expect("remote invitation take should succeed");
        assert_eq!(invitations.len(), 1);
        assert_eq!(invitations[0].invitation_code, "invite-123");
        assert_eq!(invitations[0].purpose, InvitationPurpose::Contact);
        assert!(!invitations_endpoint(
            broker.public_url(),
            "fedcba98-7654-3210-fedc-ba9876543210",
            DeviceId::from_bytes([7; 32])
        )
        .contains(INVITE_TOKEN));

        let replay = take_remote_invitations(
            broker.public_url(),
            AUTH_TOKEN,
            "fedcba98-7654-3210-fedc-ba9876543210",
            DeviceId::from_bytes([7; 32]),
            INVITE_TOKEN,
        )
        .await
        .expect_err("retrieval credential should be one-time after draining invitations");
        assert_eq!(
            std::error::Error::source(&replay)
                .unwrap()
                .downcast_ref::<BootstrapBrokerHttpStatus>()
                .unwrap()
                .status(),
            401
        );
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
        .expect("broker should bind")
        .with_mailbox_time_provider(Arc::new(aura_effects::time::PhysicalTimeHandler::new()));
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_query_token_test"));
        tokio::task::yield_now().await;

        register_remote_candidate(
            broker.public_url(),
            AUTH_TOKEN,
            &BootstrapBrokerRegistration {
                authority_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
                device_id: DeviceId::from_bytes([7; 32]),
                address: "127.0.0.1:40126".to_string(),
                invitation_retrieval_token: INVITE_TOKEN.to_string(),
                nickname_suggestion: None,
            },
        )
        .await
        .expect("recipient registration should succeed");

        let query_url = format!(
            "{}/v2/bootstrap/invitations/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee?credential={}",
            normalize_base_url(broker.public_url()),
            INVITE_TOKEN
        );
        let error = send_http_request("GET", &query_url, AUTH_TOKEN, &[], None)
            .await
            .expect_err("query-string credential should be rejected");
        assert_eq!(
            std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<BootstrapBrokerHttpStatus>()
                .unwrap()
                .status(),
            400
        );
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
        .expect("broker should bind")
        .with_mailbox_time_provider(Arc::new(aura_effects::time::PhysicalTimeHandler::new()));
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_broker_auth_test"));
        tokio::task::yield_now().await;

        let registration = serde_json::to_vec(&BootstrapBrokerRegistration {
            authority_id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_string(),
            device_id: DeviceId::from_bytes([7; 32]),
            address: "127.0.0.1:43000".to_string(),
            invitation_retrieval_token: INVITE_TOKEN.to_string(),
            nickname_suggestion: None,
        })
        .expect("registration should encode");
        let register_error = send_http_request(
            "POST",
            &register_endpoint(broker.public_url()),
            "wrong-token",
            &[],
            Some(&registration),
        )
        .await
        .expect_err("unauthorized register should be rejected");
        assert_eq!(
            std::error::Error::source(&register_error)
                .unwrap()
                .downcast_ref::<BootstrapBrokerHttpStatus>()
                .unwrap()
                .status(),
            401
        );

        let list_error = fetch_remote_candidates(broker.public_url(), "wrong-token")
            .await
            .expect_err("unauthorized list should be rejected");
        assert_eq!(
            std::error::Error::source(&list_error)
                .unwrap()
                .downcast_ref::<BootstrapBrokerHttpStatus>()
                .unwrap()
                .status(),
            401
        );

        let progress_url = pending_enrollment_mailbox_endpoint(
            broker.public_url(),
            "progress-witness",
            AuthorityId::new_from_entropy([8; 32]),
            DeviceId::from_bytes([7; 32]),
            [3; 32],
        );
        let progress_error = send_http_request("GET", &progress_url, "wrong-token", &[], None)
            .await
            .expect_err("unauthorized progress witness should be rejected");
        assert_eq!(
            std::error::Error::source(&progress_error)
                .unwrap()
                .downcast_ref::<BootstrapBrokerHttpStatus>()
                .unwrap()
                .status(),
            401
        );
        let progress = send_http_request("GET", &progress_url, AUTH_TOKEN, &[], None)
            .await
            .expect("authorized read-only progress witness");
        let progress: serde_json::Value = serde_json::from_slice(&progress).unwrap();
        assert_eq!(progress["registration_present"], false);
        assert!(progress["first_message_id"].is_null());
        assert_eq!(progress["messages"].as_array().unwrap().len(), 0);
        assert_eq!(progress.as_object().unwrap().len(), 6);

        let invitation = serde_json::to_vec(&BootstrapBrokerInvitation {
            recipient_authority_id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_string(),
            recipient_device_id: DeviceId::from_bytes([7; 32]),
            purpose: InvitationPurpose::Contact,
            invitation_code: "invite-denied".to_string(),
        })
        .expect("invitation should encode");
        let queue_error = send_http_request(
            "POST",
            &format!(
                "{}/v2/bootstrap/invitations",
                normalize_base_url(broker.public_url())
            ),
            "wrong-token",
            &[],
            Some(&invitation),
        )
        .await
        .expect_err("unauthorized queue should be rejected");
        assert_eq!(
            std::error::Error::source(&queue_error)
                .unwrap()
                .downcast_ref::<BootstrapBrokerHttpStatus>()
                .unwrap()
                .status(),
            401
        );

        let drain_error = take_remote_invitations(
            broker.public_url(),
            "wrong-token",
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            DeviceId::from_bytes([7; 32]),
            INVITE_TOKEN,
        )
        .await
        .expect_err("unauthorized drain should be rejected");
        assert_eq!(
            std::error::Error::source(&drain_error)
                .unwrap()
                .downcast_ref::<BootstrapBrokerHttpStatus>()
                .unwrap()
                .status(),
            401
        );
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
        assert!(parse_http_endpoint("http://127.0.0.1:3000/v2/bootstrap").is_ok());
        assert!(parse_http_endpoint("http://localhost:3000/v2/bootstrap").is_ok());

        let error = parse_http_endpoint("http://192.0.2.10:3000/v2/bootstrap")
            .expect_err("non-loopback HTTP must fail closed");
        assert!(error
            .to_string()
            .contains("plain HTTP bootstrap broker URLs are restricted to loopback"));
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "lan-http-dev"))]
    #[test]
    fn native_broker_private_lan_http_requires_explicit_client_policy() {
        use BootstrapBrokerClientPolicy::{AllowPrivateLanDevOnly, LoopbackOnly};
        let endpoint = "http://192.168.0.199:43190/v2/bootstrap/enrollment-mailbox/register";
        assert!(parse_http_endpoint_with_policy(endpoint, LoopbackOnly).is_err());
        assert_eq!(
            parse_http_endpoint_with_policy(endpoint, AllowPrivateLanDevOnly)
                .expect("explicit development LAN route"),
            (
                "192.168.0.199:43190".into(),
                "/v2/bootstrap/enrollment-mailbox/register".into()
            )
        );
        for bad in [
            "http://192.0.2.10:43190/v2/bootstrap",
            "http://8.8.8.8:43190/v2/bootstrap",
            "http://broker.example.test:43190/v2/bootstrap",
            "http://192.168.0.199@evil.test:43190/v2/bootstrap",
            "http://localhost:bad/v2/bootstrap",
            "http://192.168.0.199:0/v2/bootstrap",
            "https://192.168.0.199:43190/v2/bootstrap",
        ] {
            assert!(
                parse_http_endpoint_with_policy(bad, AllowPrivateLanDevOnly).is_err(),
                "foreign or malformed endpoint accepted"
            );
        }
        assert_eq!(BootstrapBrokerConfig::default().client_policy, LoopbackOnly);
    }

    #[cfg(all(not(target_arch = "wasm32"), not(feature = "lan-http-dev")))]
    #[test]
    fn production_broker_client_refuses_private_lan_http() {
        assert!(parse_http_endpoint_with_policy(
            "http://192.168.0.199:43190/v2/bootstrap/enrollment-mailbox/register",
            BootstrapBrokerClientPolicy::LoopbackOnly,
        )
        .is_err());
        assert_eq!(
            BootstrapBrokerConfig::default().client_policy,
            BootstrapBrokerClientPolicy::LoopbackOnly
        );
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "lan-http-dev"))]
    #[tokio::test]
    async fn development_client_registers_original_mailbox_over_authenticated_http() {
        let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(100));
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(120),
            AUTH_TOKEN,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("local broker bind")
        .with_mailbox_time_provider(clock);
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("development_client_registration"));
        tokio::task::yield_now().await;
        let subject = AuthorityId::new_from_entropy([11; 32]);
        let provisional = AuthorityId::new_from_entropy([12; 32]);
        let device = DeviceId::from_bytes([13; 32]);
        let manifest_digest = [14u8; 32];
        let setup_digest = [15u8; 32];
        let credential = "receiver-scoped-credential-000000000000000000000000".to_owned();
        let registration = serde_json::from_value(serde_json::json!({
            "statement": {
                "version": 1,
                "invitation": aura_core::InvitationId::new("development-client-invitation"),
                "ceremony": aura_core::CeremonyId::new("development-client-ceremony"),
                "manifest_digest": manifest_digest,
                "setup_digest": setup_digest,
                "subject": subject,
                "provisional": provisional,
                "invitee_device": device,
                "expires_at_ms": 1000,
                "retrieval_credential_commitment": aura_core::hash::hash(credential.as_bytes())
            },
            "proof": aura_core::threshold::ThresholdSignature::new(
                vec![0; 64], 1, vec![1], vec![0; 32], 0)
        }))
        .expect("transport fixture has canonical untrusted registration shape");
        let payload = BootstrapPendingEnrollmentMailboxRegistration {
            registration,
            retrieval_credential: credential,
        };
        let policy = BootstrapBrokerClientPolicy::AllowPrivateLanDevOnly;
        register_remote_pending_enrollment_mailbox(
            broker.public_url(),
            AUTH_TOKEN,
            &payload,
            policy,
        )
        .await
        .expect("explicit development policy retains original mailbox registration");
        let stored = fetch_remote_pending_enrollment_mailbox_registration(
            broker.public_url(),
            AUTH_TOKEN,
            subject,
            device,
            [14; 32],
            policy,
        )
        .await
        .expect("authenticated broker read")
        .expect("registration was retained");
        assert_eq!(stored, payload.registration);
        assert!(fetch_remote_pending_enrollment_mailbox_registration(
            broker.public_url(),
            "wrong-bearer",
            subject,
            device,
            [14; 32],
            policy,
        )
        .await
        .is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_bootstrap_broker_https_is_not_downgraded_to_http() {
        let error = parse_http_endpoint("https://broker.example.test/v2/bootstrap")
            .expect_err("native HTTPS must not be parsed as plain HTTP");
        assert!(error.to_string().contains("HTTPS client not available"));
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
            .write_all(b"GET /v2/bootstrap/candidates HTTP/1.1\r\n")
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
        let mut state = BootstrapBrokerState::new();
        insert_registration(
            &mut state,
            BootstrapBrokerRegistration {
                authority_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_string(),
                device_id: DeviceId::from_bytes([7; 32]),
                address: "127.0.0.1:41000".to_string(),
                invitation_retrieval_token: INVITE_TOKEN.to_string(),
                nickname_suggestion: None,
            },
            1000,
        )
        .expect("registration should be accepted");
        prune_candidates(&mut state, 1000, Duration::from_millis(1));
        assert_eq!(state.candidates.len(), 1);
        prune_candidates(&mut state, 1001, Duration::from_millis(1));
        assert!(state.candidates.is_empty());

        for index in 0..(MAX_BOOTSTRAP_CANDIDATES + 8) {
            insert_registration(
                &mut state,
                BootstrapBrokerRegistration {
                    authority_id: format!("authority-{index}"),
                    device_id: DeviceId::from_bytes([7; 32]),
                    address: format!("127.0.0.1:{}", 42000 + index),
                    invitation_retrieval_token: format!("invite-token-{index}"),
                    nickname_suggestion: None,
                },
                2000 + index as u64,
            )
            .expect("registration should stay bounded");
        }
        assert_eq!(state.candidates.len(), MAX_BOOTSTRAP_CANDIDATES);
        assert!(!state
            .candidates
            .values()
            .any(|candidate| candidate.authority_id == "authority-0"));
        assert!(state
            .candidates
            .values()
            .any(|candidate| candidate.authority_id
                == format!("authority-{}", MAX_BOOTSTRAP_CANDIDATES + 7)));
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
