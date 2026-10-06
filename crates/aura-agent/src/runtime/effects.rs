//! Effect System Components
//!
//! Core effect system components per Layer-6 spec.
//!
//! # Blocking Lock Usage
//!
//! This module uses `parking_lot::Mutex` and `parking_lot::RwLock` for several fields.
//! This is acceptable because:
//! 1. This is Layer 6 runtime assembly code (aura-agent/src/runtime/) explicitly allowed per clippy.toml
//! 2. Locks protect synchronous state (RNG, channel senders) never held across .await points
//! 3. Lock operations are brief with no async work inside the critical sections

#![allow(clippy::disallowed_types)]

use crate::core::config::{default_storage_path, SecureStorageBackend};
use crate::core::AgentConfig;
use crate::database::IndexedJournalHandler;
use crate::fact_registry::build_fact_registry;
use crate::reactive::{MessageDrop, MessageDropLog};
use crate::runtime::services::{
    LanTransportService, LogicalClockManager, MoveManager, RendezvousManager,
};
use crate::runtime::subsystems::choreography::RuntimeChoreographySessionId;
use crate::runtime::subsystems::{
    crypto::CryptoRng, ChoreographyState, CryptoSubsystem, JournalSubsystem, TransportSubsystem,
    VmFragmentId, VmFragmentRegistry,
};
use crate::runtime::time_handler::EnhancedTimeHandler;
use async_trait::async_trait;
use aura_app::ReactiveHandler;
use aura_authorization::{BiscuitAuthorizationBridge, VerifiedBiscuitToken};
use aura_composition::{CompositeHandlerAdapter, RegisterAllOptions};
use aura_core::crypto::single_signer::SigningMode;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::*;
use aura_core::hash::hash as aura_hash;
use aura_core::types::scope::AuthorizationOp;
use aura_core::TimeoutRunError;
use aura_core::{
    execute_with_timeout_budget, AuraError, AuthorityId, ContextId, Ed25519SigningKey,
    TimeoutBudget,
};
use aura_effects::{
    crypto::RealCryptoHandler,
    encrypted_storage::{EncryptedStorage, EncryptedStorageConfig},
    secure::ProductionSecureStorageHandler,
    storage::FilesystemStorageHandler,
    time::{OrderClockHandler, PhysicalTimeHandler},
};
use aura_journal::extensibility::FactRegistry;
use aura_journal::fact::ProtocolRelationalFact;
use aura_journal::fact::{
    DkgTranscriptCommit, Fact as TypedFact, FactContent, FactOptions, RelationalFact,
};
use aura_mpst::CompositionManifest;
use aura_protocol::handlers::{PersistentSyncHandler, PersistentTreeHandler};
use biscuit_auth::{Biscuit, PublicKey};
use parking_lot::RwLock;
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;
use std::panic::Location;
use std::sync::Arc;
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
use std::sync::Once;
use std::sync::OnceLock;
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
use std::time::Duration;

use super::shared_transport::SharedTransport;

#[cfg(target_arch = "wasm32")]
const HARNESS_INSTANCE_QUERY_KEY: &str = "__aura_harness_instance";
#[cfg(target_arch = "wasm32")]
const HARNESS_TOKEN_QUERY_KEY: &str = "__aura_harness_token";
#[cfg(target_arch = "wasm32")]
const MIN_HARNESS_TOKEN_LEN: usize = 16;

/// Cached Biscuit token and root public key for guard chain authorization.
///
/// Populated during `bootstrap_authority()` and loaded from secure storage
/// on subsequent startups via `initialize_biscuit_cache()`.
#[derive(Clone, Debug)]
pub struct BiscuitCache {
    /// Base64-encoded Biscuit token bytes
    pub token_b64: String,
    /// Trusted issuer metadata carried alongside cached token bytes.
    pub issuer_authority: AuthorityId,
    /// Base64-encoded root public key bytes
    ///
    /// Retained as cache consistency metadata only. Verification must use the
    /// trusted runtime root public key, not this cached value.
    pub root_pk_b64: String,
}

#[derive(Debug, thiserror::Error)]
pub enum BiscuitStartupRecordError {
    #[error("persisted Biscuit exceeds its record budget")]
    Oversized,
    #[error("persisted Biscuit record is too short: {0} bytes")]
    Truncated(usize),
}

impl From<BiscuitStartupRecordError> for AuraError {
    fn from(source: BiscuitStartupRecordError) -> Self {
        AuraError::Serialization {
            message: "invalid persisted authorization record".into(),
            source: Some(Arc::new(source)),
        }
    }
}

mod amp;
mod aura;
mod choreography;
mod crypto;
#[cfg(test)]
pub(crate) use crypto::EnrollmentGenerationHistoryError;
#[cfg(test)]
pub(crate) use crypto::ParticipantEnvelopeBoundsError;
pub(crate) use crypto::{
    held_registration_error, EnrollmentFinalInventoryError,
    EnrollmentFinalVerifierInventoryCapability, EnrollmentGenerationCustodyCapability,
    EnrollmentGenerationReservation, EnrollmentResponsePolicy, EnrollmentTranscriptTreeOwner,
    HeldEnrollmentRegistrationError, RegisteredEnrollmentGenerationCapability,
    RequiredSigningParticipantError,
};

pub(in crate::runtime) use crypto::{
    OwnedSecretBirthCapability, OwnedSecretNegativeCapability, OwnedSecretPositiveCapability,
    OwnedSecretReadCapability,
};

mod effect_api;
mod flow;
mod guard;
mod journal;
mod network;
mod noise;
mod storage;
mod sync;
mod system;
mod time;
mod transport;
mod tree;

const DEFAULT_WINDOW: u32 = 1024;
const TYPED_FACT_STORAGE_PREFIX: &str = "journal/facts";
const DEFAULT_CHOREO_FLOW_COST: u32 = 1;
const CHOREO_FLOW_COST_PER_KB: u32 = 1;
const AMP_CONTENT_TYPE: &str = "application/aura-amp";
const TEST_SEED_DERIVATION_DOMAIN: &str = "aura:test-seed:v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthorizationRuntimeMode {
    Production,
    Testing,
    Harness,
    Simulation,
}

impl AuthorizationRuntimeMode {
    fn from_runtime(execution_mode: ExecutionMode, harness_mode_enabled: bool) -> Self {
        if harness_mode_enabled {
            return Self::Harness;
        }
        match execution_mode {
            ExecutionMode::Production => Self::Production,
            ExecutionMode::Testing => Self::Testing,
            ExecutionMode::Simulation { .. } => Self::Simulation,
        }
    }
}

#[derive(Clone, Debug)]
struct AuthorizationRuntimeConfig {
    mode: AuthorizationRuntimeMode,
    authority_id: AuthorityId,
    root_public_key: PublicKey,
}

impl AuthorizationRuntimeConfig {
    fn from_verifying_key(
        authority_id: AuthorityId,
        execution_mode: ExecutionMode,
        harness_mode_enabled: bool,
        verifying_key: &[u8],
    ) -> Result<Self, AuraError> {
        if verifying_key.is_empty() {
            return Err(AuraError::invalid(
                "Biscuit authorization root public key is required",
            ));
        }
        if verifying_key.iter().all(|byte| *byte == 0) {
            return Err(AuraError::invalid(
                "Biscuit authorization root public key must not be all zero",
            ));
        }
        let root_public_key = PublicKey::from_bytes(verifying_key).map_err(|error| {
            AuraError::invalid(format!(
                "Biscuit authorization root public key is invalid: {error}"
            ))
        })?;
        Ok(Self {
            mode: AuthorizationRuntimeMode::from_runtime(execution_mode, harness_mode_enabled),
            authority_id,
            root_public_key,
        })
    }
}

#[derive(Clone, Debug)]
struct TestSeedUsage {
    identity: String,
    location: String,
}

static TEST_SEED_REGISTRY: OnceLock<parking_lot::Mutex<HashMap<u64, TestSeedUsage>>> =
    OnceLock::new();

/// Providers selected by the complete custom typestate builder before subsystem assembly.
/// Ordinary storage remains wrapped by the selected profile's encrypted/secure owner.
pub(crate) struct SelectedCustomProviders {
    pub(crate) crypto: Arc<dyn CryptoEffects>,
    pub(crate) storage: Arc<dyn StorageEffects>,
    pub(crate) random: Arc<dyn RandomEffects>,
    pub(crate) console: Arc<dyn ConsoleEffects>,
    pub(crate) transports: Vec<Arc<dyn TransportEffects>>,
}

type RuntimeEncryptedStorage = EncryptedStorage<
    Arc<dyn StorageEffects>,
    Arc<dyn CryptoEffects>,
    ProductionSecureStorageHandler,
>;
type RuntimeJournalHandler = aura_journal::JournalHandler<
    Arc<dyn CryptoEffects>,
    Arc<RuntimeEncryptedStorage>,
    JournalBiscuitAuthorizationHandler,
>;

/// A fact received from `source` and rejected at ingress because the required
/// views cannot decode it (`cause` keeps the codec or schema fault).
#[derive(Debug, thiserror::Error)]
#[error("peer fact {type_id} (schema {schema_version}) from {source_authority} rejected: {cause}")]
pub struct PeerFactRejection {
    pub source_authority: AuthorityId,
    pub type_id: String,
    pub schema_version: u16,
    #[source]
    pub cause: AuraError,
}

impl PeerFactRejection {
    fn new(source_authority: AuthorityId, fact: &RelationalFact, cause: AuraError) -> Self {
        let (type_id, schema_version) = match fact {
            RelationalFact::Generic { envelope, .. } => (
                envelope.type_id.as_str().to_owned(),
                envelope.schema_version,
            ),
            _ => (String::from("protocol"), 0),
        };
        Self {
            source_authority,
            type_id,
            schema_version,
            cause,
        }
    }
}

/// Concrete effect system combining all effects for runtime usage
///
/// Note: This wraps aura-composition infrastructure for Layer 6 runtime concerns.
///
/// ## Subsystem Organization
///
/// Related fields are grouped into subsystems for better organization:
/// - `crypto`: Cryptographic operations, RNG, secure key storage
/// - `transport`: Network transport, inbox management, statistics
/// - `journal`: Indexed journal, fact registry, publication channel
///
/// Remaining fields are core infrastructure used across subsystems.
pub struct AuraEffectSystem {
    // One actual operation admission owner shared by runtime and service clones.
    public_operation_activity: Arc<crate::runtime::system::RuntimeActivityGate>,
    // === Core Configuration ===
    config: AgentConfig,
    authority_id: AuthorityId,
    execution_mode: ExecutionMode,
    harness_mode_enabled: bool,

    // === Subsystems (grouped related fields) ===
    /// Cryptographic operations subsystem
    crypto: CryptoSubsystem,
    enrollment_generation_gate: tokio::sync::Mutex<()>,
    /// Facts received from peers and rejected at ingress (see
    /// [`AuraEffectSystem::admit_peer_fact`]).
    rejected_peer_facts: std::sync::atomic::AtomicU64,
    /// Dropped chat messages, inbound and outbound (see
    /// [`AuraEffectSystem::record_message_drop`]).
    message_drops: std::sync::Mutex<MessageDropLog>,
    #[cfg(all(test, not(target_arch = "wasm32")))]
    enrollment_retirement_fault: std::sync::Mutex<Option<u64>>,
    /// Network transport subsystem
    transport: TransportSubsystem,
    /// Journal and fact management subsystem
    journal: JournalSubsystem,

    // === Composition & Handlers ===
    composite: CompositeHandlerAdapter,

    // === Storage Infrastructure ===
    storage_handler: Arc<RuntimeEncryptedStorage>,
    tree_handler: PersistentTreeHandler,
    sync_handler: PersistentSyncHandler,

    // === Time Services ===
    time_handler: EnhancedTimeHandler,
    logical_clock: Arc<LogicalClockManager>,
    order_clock: OrderClockHandler,

    // === Authorization & Flow Control ===
    authorization_handler:
        aura_authorization::effects::WotAuthorizationHandler<Arc<dyn CryptoEffects>>,
    leakage_handler: aura_effects::leakage::ProductionLeakageHandler<RuntimeEncryptedStorage>,

    // === Reactive System ===
    /// Reactive signal graph for UI-facing state.
    reactive_handler: ReactiveHandler,

    // === Choreography State ===
    /// In-memory choreography session state for runtime coordination.
    choreography_state: parking_lot::RwLock<ChoreographyState>,

    /// Fragment-scoped local ownership registry for admitted VM sessions.
    vm_fragment_registry: parking_lot::RwLock<VmFragmentRegistry>,

    /// LAN transport service (optional, for TCP envelope delivery)
    lan_transport: parking_lot::RwLock<Option<Arc<LanTransportService>>>,

    /// Rendezvous manager (optional, for address resolution)
    rendezvous_manager: parking_lot::RwLock<Option<RendezvousManager>>,

    /// Move manager (optional, for bounded movement planning and replay windows).
    move_manager: parking_lot::RwLock<Option<MoveManager>>,

    /// Cached Biscuit token for guard chain authorization.
    biscuit_cache: parking_lot::RwLock<Option<BiscuitCache>>,

    /// Runtime-local key used to sign flow receipts and their transport binding.
    receipt_signing_key: tokio::sync::OnceCell<Ed25519SigningKey>,
    custom_random: Option<Arc<dyn RandomEffects>>,
    custom_console: Option<Arc<dyn ConsoleEffects>>,
    custom_transports: Vec<Arc<dyn TransportEffects>>,

    /// Runtime-owned in-memory ledger surface for EffectApiEffects consumers.
    effect_api_ledger: parking_lot::Mutex<EffectApiLedgerState>,
    imported_invitation_decision_gate: tokio::sync::Mutex<()>,
    guardian_recovery_keypair_gate: tokio::sync::Mutex<()>,
    enrollment_manifest_admission_gate: tokio::sync::Mutex<()>,
    enrollment_profile_handoff_gate: tokio::sync::Mutex<()>,
    enrollment_invitee_window_owner: Arc<tokio::sync::Semaphore>,

    /// Runtime-owned config overrides exposed through SystemEffects.
    system_config: parking_lot::RwLock<HashMap<String, String>>,

    /// Runtime-managed connection handles for `NetworkExtendedEffects::open/send/close`.
    ///
    /// The key is an opaque connection handle UUID, not the remote address.
    /// Addresses are parsed and validated once during `open`.
    #[cfg(not(target_arch = "wasm32"))]
    network_connections: parking_lot::RwLock<HashMap<uuid::Uuid, SocketAddr>>,
    /// Browser websocket endpoints keyed by opaque connection handle UUID.
    #[cfg(target_arch = "wasm32")]
    network_connections: parking_lot::RwLock<HashMap<uuid::Uuid, String>>,
    // Declared last: storage subsystems drop before the actual profile owner.
    profile_owner: Option<std::sync::Arc<aura_effects::profile_storage::OwnedProfileLease>>,
}

#[derive(Default)]
struct EffectApiLedgerState {
    epoch: u64,
    events: Vec<(u64, Vec<u8>)>,
    device_activity: HashMap<aura_core::DeviceId, u64>,
    subscribers: Vec<futures::channel::mpsc::Sender<aura_protocol::effects::EffectApiEvent>>,
}

#[derive(Clone)]
struct JournalBiscuitAuthorizationHandler {
    bridge: BiscuitAuthorizationBridge,
    time_handler: PhysicalTimeHandler,
}

#[async_trait]
impl BiscuitAuthorizationEffects for JournalBiscuitAuthorizationHandler {
    async fn authorize_biscuit(
        &self,
        token_data: &[u8],
        operation: AuthorizationOp,
        scope: &aura_core::types::scope::ResourceScope,
    ) -> Result<AuthorizationDecision, AuthorizationError> {
        let token = VerifiedBiscuitToken::from_bytes(token_data, self.bridge.root_public_key())
            .map_err(|error| AuthorizationError::InvalidToken {
                reason: error.to_string(),
            })?;
        let now = self.time_handler.physical_time_now_ms() / 1000;
        let result = self
            .bridge
            .authorize_with_time(&token, operation, scope, Some(now))
            .map_err(|error| AuthorizationError::InvalidToken {
                reason: error.to_string(),
            })?;
        Ok(AuthorizationDecision {
            authorized: result.authorized,
            reason: (!result.authorized).then(|| "Biscuit policy denied operation".to_string()),
        })
    }

    async fn authorize_fact(
        &self,
        token_data: &[u8],
        _fact_type: &str,
        scope: &aura_core::types::scope::ResourceScope,
    ) -> Result<bool, AuthorizationError> {
        Ok(self
            .authorize_biscuit(token_data, AuthorizationOp::Update, scope)
            .await?
            .authorized)
    }
}

pub(crate) struct AdmittedEnrollmentWindowLeaseCapability<'a> {
    effects: &'a AuraEffectSystem,
    permit: tokio::sync::OwnedSemaphorePermit,
}
impl AdmittedEnrollmentWindowLeaseCapability<'_> {
    pub(crate) fn require_effects(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<(), aura_core::AuraError> {
        if !std::ptr::eq(self.effects, effects) {
            return Err(aura_core::AuraError::from(
                aura_core::TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "admitted lease belongs to another runtime owner".into(),
                },
            ));
        }
        Ok(())
    }
    pub(crate) fn into_permit(self) -> tokio::sync::OwnedSemaphorePermit {
        self.permit
    }
}
/// Exclusive original-runtime custody for required contact import reads and
/// decision publication. Raw locks and separately instantiated handler caches
/// cannot issue or replace this lease.
#[derive(Debug, thiserror::Error)]
enum ImportedInvitationDecisionOwnerError {
    #[error("imported invitation decision belongs to another runtime owner")]
    ForeignRuntime,
}

/// Private runtime-issued custody for Guardian key initialization.
pub(crate) struct GuardianRecoveryKeypairLeaseCapability<'runtime> {
    effects: &'runtime AuraEffectSystem,
    _guard: tokio::sync::MutexGuard<'runtime, ()>,
}
impl<'runtime> GuardianRecoveryKeypairLeaseCapability<'runtime> {
    pub(crate) fn effects(&self) -> &'runtime AuraEffectSystem {
        self.effects
    }
}

pub(crate) struct ImportedInvitationDecisionLeaseCapability<'runtime> {
    effects: &'runtime AuraEffectSystem,
    _guard: tokio::sync::MutexGuard<'runtime, ()>,
}
impl ImportedInvitationDecisionLeaseCapability<'_> {
    pub(crate) fn require_runtime_owner(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<(), AuraError> {
        if !std::ptr::eq(self.effects, effects) {
            let cause = ImportedInvitationDecisionOwnerError::ForeignRuntime;
            return Err(AuraError::Invalid {
                message: cause.to_string(),
                source: Some(Arc::new(cause)),
            });
        }
        Ok(())
    }
}

/// One admitted runtime operation with its original effect-backed window.
/// The admission lease is retained through every mutation and observation await.
#[must_use = "retain the admitted operation through its original window"]
pub(crate) struct RuntimeBoundedOperationCapability<'runtime> {
    effects: &'runtime AuraEffectSystem,
    lease: crate::runtime::system::RuntimeOperationLease,
    window: TimeoutBudget,
}
impl RuntimeBoundedOperationCapability<'_> {
    pub(crate) fn original_window(&self) -> &TimeoutBudget {
        &self.window
    }
    pub(crate) fn require_runtime_owner(
        &self,
        effects: &AuraEffectSystem,
    ) -> crate::core::AgentResult<()> {
        self.lease
            .require_gate(&effects.public_operation_activity)?;
        if !std::ptr::eq(self.effects, effects) {
            return Err(crate::core::AgentError::from(
                crate::runtime::system::RuntimePublicOperationError::ForeignHandoff,
            ));
        }
        Ok(())
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "runtime_bounded_operation", capability_type = RuntimeBoundedOperationCapability, receiver_type = RuntimeBoundedOperationCapability<'_>, family = "runtime_helper")]
    pub(crate) async fn execute<F, Fut, T>(&self, operation: F) -> crate::core::AgentResult<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = crate::core::AgentResult<T>>,
    {
        match execute_with_timeout_budget(self.effects, &self.window, operation).await {
            Ok(value) => Ok(value),
            Err(TimeoutRunError::Operation(source)) => Err(source),
            Err(TimeoutRunError::Timeout(
                source @ aura_core::TimeoutBudgetError::DeadlineExceeded { .. },
            )) => {
                let message = source.to_string();
                Err(crate::core::AgentError::TimeoutWithSource {
                    message: message.clone(),
                    source: AuraError::Internal {
                        message,
                        source: Some(Arc::new(source)),
                    },
                })
            }
            Err(TimeoutRunError::Timeout(source)) => {
                Err(crate::core::AgentError::Aura(AuraError::Internal {
                    message: "required original runtime operation window failed".into(),
                    source: Some(Arc::new(source)),
                }))
            }
        }
    }
}

impl AuraEffectSystem {
    /// Admission and its window are born once, before any operation mutation.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "runtime_bounded_operation", capability_type = RuntimeBoundedOperationCapability, family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub(crate) async fn admit_bounded_runtime_operation(
        &self,
    ) -> crate::core::AgentResult<RuntimeBoundedOperationCapability<'_>> {
        let lease = self.admit_public_operation()?;
        self.bound_admitted_runtime_operation(lease).await
    }

    /// Continue an actual admitted lease into its one original resource window.
    /// No second admission or caller-provided clock/deadline is accepted.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "runtime_bounded_operation", capability_type = RuntimeBoundedOperationCapability, family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub(crate) async fn bound_admitted_runtime_operation(
        &self,
        lease: crate::runtime::system::RuntimeOperationLease,
    ) -> crate::core::AgentResult<RuntimeBoundedOperationCapability<'_>> {
        lease.require_gate(&self.public_operation_activity())?;
        let started = self.physical_time().await.map_err(|source| {
            crate::core::AgentError::Aura(AuraError::Internal {
                message: "read original runtime operation admission time".into(),
                source: Some(Arc::new(source)),
            })
        })?;
        // Public operation policy, not a helper/retry-renewed duration.
        let window =
            TimeoutBudget::from_start_and_timeout(&started, std::time::Duration::from_secs(30))
                .map_err(|source| {
                    crate::core::AgentError::Aura(AuraError::Internal {
                        message: "validate original runtime operation window".into(),
                        source: Some(Arc::new(source)),
                    })
                })?;
        Ok(RuntimeBoundedOperationCapability {
            effects: self,
            lease,
            window,
        })
    }
}

/// Exact canonical commit and processing owner, minted only by the runtime commit path.
#[must_use = "await exact processing under original operation custody"]
pub(crate) struct RequiredReactiveFactCommitCapability<'runtime> {
    effects: &'runtime AuraEffectSystem,
    committed: Vec<TypedFact>,
    target: crate::reactive::FactProcessingTargetCapability,
}
impl RequiredReactiveFactCommitCapability<'_> {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "required_reactive_fact_commit", capability_type = RequiredReactiveFactCommitCapability, receiver_type = RequiredReactiveFactCommitCapability<'_>, family = "runtime_helper")]
    pub(crate) async fn await_processed(
        self,
        operation: &RuntimeBoundedOperationCapability<'_>,
    ) -> crate::core::AgentResult<Vec<TypedFact>> {
        operation.require_runtime_owner(self.effects)?;
        self.await_processed_in_original_window(operation.original_window())
            .await
    }

    /// Observation under an already held protocol/startup resource window.
    /// The retained canonical target remains mutation provenance; the borrowed
    /// window cannot authorize, renew, or reconstruct the original operation.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "required_reactive_fact_commit", capability_type = RequiredReactiveFactCommitCapability, receiver_type = RequiredReactiveFactCommitCapability<'_>, family = "runtime_helper")]
    pub(crate) async fn await_processed_in_original_window(
        self,
        original_window: &TimeoutBudget,
    ) -> crate::core::AgentResult<Vec<TypedFact>> {
        match self
            .effects
            .journal
            .await_required(self.target, self.effects, original_window)
            .await
        {
            Ok(()) => Ok(self.committed),
            Err(aura_core::time::timeout::TimeoutRunError::Timeout(
                source @ aura_core::TimeoutBudgetError::DeadlineExceeded { .. },
            )) => {
                let message = source.to_string();
                Err(crate::core::AgentError::TimeoutWithSource {
                    message: message.clone(),
                    source: AuraError::Internal {
                        message,
                        source: Some(Arc::new(source)),
                    },
                })
            }
            Err(source) => Err(crate::core::AgentError::Aura(AuraError::Internal {
                message: "required original canonical fact processing failed".into(),
                source: Some(Arc::new(source)),
            })),
        }
    }
}

impl AuraEffectSystem {
    #[aura_macros::capability_boundary(category = "capability_gated",
        capability = "runtime_public_operation", capability_type = crate::runtime::system::RuntimeOperationLease,
        family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub(crate) fn admit_public_operation(
        &self,
    ) -> crate::core::AgentResult<crate::runtime::system::RuntimeOperationLease> {
        self.public_operation_activity
            .admit()
            .map_err(crate::core::AgentError::from)
    }

    pub(crate) fn public_operation_activity(
        &self,
    ) -> Arc<crate::runtime::system::RuntimeActivityGate> {
        self.public_operation_activity.clone()
    }

    /// Serialize the original local Guardian key birth and required pair reads.
    #[aura_macros::capability_boundary(category = "capability_gated",
        capability = "guardian_recovery_keypair", capability_type = GuardianRecoveryKeypairLeaseCapability,
        family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub(crate) async fn acquire_guardian_recovery_keypair(
        &self,
    ) -> GuardianRecoveryKeypairLeaseCapability<'_> {
        GuardianRecoveryKeypairLeaseCapability {
            effects: self,
            _guard: self.guardian_recovery_keypair_gate.lock().await,
        }
    }

    #[aura_macros::capability_boundary(category = "capability_gated",
    capability = "imported_invitation_decision", capability_type = ImportedInvitationDecisionLeaseCapability,
    family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub(crate) async fn acquire_imported_invitation_decision(
        &self,
    ) -> ImportedInvitationDecisionLeaseCapability<'_> {
        ImportedInvitationDecisionLeaseCapability {
            effects: self,
            _guard: self.imported_invitation_decision_gate.lock().await,
        }
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "admitted_enrollment_execution_window",
        capability_type = AdmittedEnrollmentWindowLeaseCapability,
        family = "runtime_helper"
    )]
    pub(crate) fn acquire_admitted_enrollment_window_owner(
        &self,
        admitted: &crate::handlers::invitation::enrollment_manifest_admission::AdmittedEnrollmentManifest,
    ) -> Result<AdmittedEnrollmentWindowLeaseCapability<'_>, aura_core::AuraError> {
        if admitted.manifest().invitee_device != self.device_id() {
            return Err(aura_core::AuraError::invalid(
                "enrollment window must name actual physical device",
            ));
        }
        self.enrollment_invitee_window_owner
            .clone()
            .try_acquire_owned()
            .map(|permit| AdmittedEnrollmentWindowLeaseCapability {
                effects: self,
                permit,
            })
            .map_err(|source| aura_core::AuraError::Internal {
                message: "admitted enrollment execution already has a window owner".into(),
                source: Some(Arc::new(source)),
            })
    }

    fn create_test_storage_namespace(
        root: &std::path::Path,
        label: &str,
        counter: &std::sync::atomic::AtomicUsize,
        maximum_attempts: usize,
    ) -> std::io::Result<std::path::PathBuf> {
        let mut collision = None;
        for _ in 0..maximum_attempts {
            let attempt = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // A versioned namespace never selects the old unchecked fallback.
            let candidate = root.join(format!("aura-agent-isolated-v2-{label}-{attempt}"));
            match std::fs::create_dir(&candidate) {
                Ok(()) => return Ok(candidate),
                Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                    collision = Some(source);
                }
                Err(source) => return Err(source),
            }
        }
        Err(collision.unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "test namespace requires a positive attempt budget",
            )
        }))
    }
    fn unique_test_storage_path(label: &str) -> std::io::Result<std::path::PathBuf> {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        Self::create_test_storage_namespace(&std::env::temp_dir(), label, &COUNTER, 4096)
    }

    #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
    fn maybe_start_deadlock_detector() {
        static START: Once = Once::new();
        START.call_once(|| {
            std::thread::Builder::new()
                .name("aura-deadlock-detector".to_string())
                .spawn(|| {
                    loop {
                        std::thread::park_timeout(Duration::from_secs(10));
                        let deadlocks = parking_lot::deadlock::check_deadlock();
                        if !deadlocks.is_empty() {
                            // Note: DeadlockedThread doesn't implement Debug, so we log count only
                            tracing::error!(
                                count = deadlocks.len(),
                                "Detected parking_lot deadlock(s)"
                            );
                        }
                    }
                })
                .expect("failed to spawn deadlock detector thread");
        });
    }

    #[cfg(any(not(debug_assertions), target_arch = "wasm32"))]
    fn maybe_start_deadlock_detector() {}

    fn normalize_test_config(
        mut config: AgentConfig,
    ) -> Result<AgentConfig, crate::core::AgentError> {
        if config.storage.base_path == default_storage_path() {
            config.storage.base_path =
                Self::unique_test_storage_path("test").map_err(|source| {
                    crate::core::AgentError::from(aura_core::AuraError::Storage {
                        message: "allocate isolated test profile".into(),
                        source: Some(Arc::new(source)),
                    })
                })?;
        }
        Ok(config)
    }

    fn secure_storage_handler_for_config(
        config: &AgentConfig,
        execution_mode: ExecutionMode,
        harness_mode_enabled: bool,
        test_filesystem_secure_storage_allowed: bool,
    ) -> Result<ProductionSecureStorageHandler, crate::core::AgentError> {
        if !execution_mode.is_production() {
            return Ok(
                ProductionSecureStorageHandler::filesystem_fallback_for_non_production(
                    config.storage.base_path.clone(),
                ),
            );
        }

        match config.storage.secure_storage_backend {
            SecureStorageBackend::FilesystemFallback
                if harness_mode_enabled || test_filesystem_secure_storage_allowed =>
            {
                Ok(
                    ProductionSecureStorageHandler::filesystem_fallback_for_non_production(
                        config.storage.base_path.clone(),
                    ),
                )
            }
            SecureStorageBackend::FilesystemFallback => Err(crate::core::AgentError::config(
                "production runtime rejects filesystem secure-storage fallback; use platform credential storage or an explicit test/harness/simulation constructor",
            )),
            SecureStorageBackend::PlatformCredentialStore => {
                // Preserve the configured backend without migrating keyring state.
                Ok(ProductionSecureStorageHandler::for_production(
                    config.storage.base_path.clone(),
                ))
            }
        }
    }

    /// Internal helper that builds the effect system with the given composite handler.
    ///
    /// All factory methods delegate to this to avoid code duplication.
    ///
    /// When `crypto_seed` is provided, the crypto handler will use deterministic
    /// randomness for reproducible tests and simulations.
    ///
    /// When `shared_transport` is provided (for simulation/demo mode), all agents
    /// share a common in-memory transport network for routing.
    ///
    /// When `shared_inbox` is provided, all agents share a single inbox queue and
    /// filter envelopes by destination on receive.
    fn build_internal(
        config: AgentConfig,
        composite: CompositeHandlerAdapter,
        execution_mode: ExecutionMode,
        crypto_seed: Option<[u8; 32]>,
        shared_transport: Option<SharedTransport>,
        shared_inbox: Option<Arc<RwLock<Vec<TransportEnvelope>>>>,
        authority_id: AuthorityId,
        test_filesystem_secure_storage_allowed: bool,
    ) -> Result<Self, crate::core::AgentError> {
        Self::build_internal_owned(
            config,
            composite,
            execution_mode,
            crypto_seed,
            shared_transport,
            shared_inbox,
            authority_id,
            test_filesystem_secure_storage_allowed,
            None,
            None,
            None,
        )
    }

    fn build_internal_owned(
        config: AgentConfig,
        composite: CompositeHandlerAdapter,
        execution_mode: ExecutionMode,
        crypto_seed: Option<[u8; 32]>,
        shared_transport: Option<SharedTransport>,
        shared_inbox: Option<Arc<RwLock<Vec<TransportEnvelope>>>>,
        authority_id: AuthorityId,
        test_filesystem_secure_storage_allowed: bool,
        selected_profile_owner: Option<Arc<aura_effects::profile_storage::OwnedProfileLease>>,
        custom: Option<SelectedCustomProviders>,
        testing_profile: Option<super::builder::TestingOwnedProfileCapability>,
    ) -> Result<Self, crate::core::AgentError> {
        let entropy = super::entropy::NonProductionEntropySeed::admit(execution_mode, crypto_seed)
            .map_err(crate::core::AgentError::from)?;
        if execution_mode.is_production()
            && matches!(
                config.storage.encryption_policy,
                crate::core::config::StorageEncryptionPolicy::PlaintextForTests
            )
        {
            return Err(crate::core::AgentError::config(
                "production runtime rejects plaintext storage policy",
            ));
        }
        let profile_error = |source: aura_core::effects::profile_storage::ProfileStorageError| {
            crate::core::AgentError::from(AuraError::Storage {
                message: "runtime profile ownership failed".into(),
                source: Some(std::sync::Arc::new(source)),
            })
        };
        if !execution_mode.is_production() && selected_profile_owner.is_some() {
            return Err(profile_error(
                aura_core::effects::profile_storage::ProfileStorageError::Invalid(
                    "production profile lease supplied to a nonproduction runtime".into(),
                ),
            ));
        }
        let owned_testing = testing_profile.is_some();
        let selected_profile_owner =
            match testing_profile {
                Some(profile)
                    if matches!(
                        execution_mode,
                        ExecutionMode::Testing | ExecutionMode::Simulation { .. }
                    ) =>
                {
                    Some(profile.into_owner())
                }
                Some(_) => return Err(profile_error(
                    aura_core::effects::profile_storage::ProfileStorageError::Invalid(
                        "owned testing profile capability supplied outside nonproduction assembly"
                            .into(),
                    ),
                )),
                None => selected_profile_owner,
            };
        let profile_owner = if execution_mode.is_production() || owned_testing {
            let owned = match selected_profile_owner {
                Some(owned) => owned,
                None => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        aura_effects::profile_storage::FilesystemProfileStorageHandler::new(
                            config.storage.base_path.clone(),
                        )
                        .acquire_owned_native()
                        .map(Arc::new)
                        .map_err(profile_error)?
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        return Err(profile_error(
                            aura_core::effects::profile_storage::ProfileStorageError::Unsupported,
                        ));
                    }
                }
            };
            if !owned
                .matches_profile(&config.storage.base_path)
                .map_err(profile_error)?
            {
                return Err(profile_error(
                    aura_core::effects::profile_storage::ProfileStorageError::Invalid(
                        "runtime configuration differs from selected physical profile".into(),
                    ),
                ));
            }
            Some(owned)
        } else {
            None
        };
        Self::maybe_start_deadlock_detector();
        let authority = authority_id;
        let device_id = config.device_id();
        let (journal_policy, journal_verifying_key) = Self::init_journal_policy(authority);
        let test_mode = execution_mode.is_deterministic();
        let harness_mode_enabled =
            std::env::var_os("AURA_HARNESS_MODE").is_some() || authenticated_browser_harness_mode();

        // === Build CryptoSubsystem ===
        let crypto_handler: Arc<dyn CryptoEffects> = match custom.as_ref() {
            Some(providers) => providers.crypto.clone(),
            None => Arc::new(match entropy.as_ref() {
                Some(seed) => seed.crypto_handler(),
                None => RealCryptoHandler::new(),
            }),
        };
        if execution_mode.is_production() && crypto_handler.is_simulated() {
            return Err(crate::core::AgentError::config(
                "production custom crypto provider must not be simulated",
            ));
        }
        let random_rng = match entropy.as_ref() {
            Some(seed) => seed.random_stream(),
            None => CryptoRng::thread_local(),
        };
        // The actual filesystem provider must receive the owner before touching
        // its wrapping key. Platform providers retain their separate namespace.
        #[cfg(unix)]
        let owned_filesystem_backend = match profile_owner.as_ref() {
            Some(owner)
                if config.storage.secure_storage_backend
                    == SecureStorageBackend::FilesystemFallback =>
            {
                if execution_mode.is_production()
                    && !harness_mode_enabled
                    && !test_filesystem_secure_storage_allowed
                {
                    return Err(crate::core::AgentError::config(
                        "production runtime rejects filesystem secure-storage fallback",
                    ));
                }
                Some(
                    ProductionSecureStorageHandler::filesystem_fallback_with_profile_owner(
                        owner.clone(),
                    )
                    .map_err(crate::core::AgentError::from)?,
                )
            }
            _ => None,
        };
        #[cfg(unix)]
        let already_owned = owned_filesystem_backend.is_some();
        #[cfg(not(unix))]
        let already_owned = false;
        #[cfg(unix)]
        let secure_storage_backend = if let Some(backend) = owned_filesystem_backend {
            backend
        } else {
            Self::secure_storage_handler_for_config(
                &config,
                execution_mode,
                harness_mode_enabled,
                test_filesystem_secure_storage_allowed,
            )?
        };
        #[cfg(not(unix))]
        let secure_storage_backend = Self::secure_storage_handler_for_config(
            &config,
            execution_mode,
            harness_mode_enabled,
            test_filesystem_secure_storage_allowed,
        )?;
        let secure_storage_backend = match profile_owner.as_ref() {
            Some(owner) if !already_owned => secure_storage_backend
                .retain_profile_owner(owner.clone())
                .map_err(profile_error)?,
            _ => secure_storage_backend,
        };
        #[cfg(unix)]
        let (secure_storage_backend, allocation_lifetime_root) = if profile_owner.is_some()
            && matches!(&secure_storage_backend, ProductionSecureStorageHandler::ProfileOwned(owned) if owned.uses_filesystem_fallback())
        {
            let (ordinary, root) = secure_storage_backend
                .into_selected_profile_lifetime_channel()
                .map_err(crate::core::AgentError::from)?;
            (ordinary, Some(root))
        } else {
            (secure_storage_backend, None)
        };
        #[cfg(not(unix))]
        let allocation_lifetime_root = None;
        let secure_storage_handler = Arc::new(secure_storage_backend);

        let mut crypto = CryptoSubsystem::from_parts(
            crypto_handler.clone(),
            random_rng,
            secure_storage_handler.clone(),
        );
        if let Some(root) = allocation_lifetime_root {
            crypto
                .retain_selected_secret_lifetimes(root)
                .map_err(crate::core::AgentError::from)?;
        }
        let receipt_signing_key = match custom.as_ref() {
            Some(_) => tokio::sync::OnceCell::new(),
            None => tokio::sync::OnceCell::new_with(Some(Ed25519SigningKey::from_bytes(
                crypto.random_32_bytes(),
            ))),
        };

        // === Build Storage Infrastructure ===
        let auth_time = PhysicalTimeHandler::new();
        let time_handler = EnhancedTimeHandler::new();
        let authorization_handler = Self::init_authorization_handler(
            authority,
            &crypto_handler,
            &journal_verifying_key,
            &auth_time,
            execution_mode,
            harness_mode_enabled,
        );
        let encrypted_storage_config = {
            let mut cfg = match config.storage.encryption_policy {
                crate::core::config::StorageEncryptionPolicy::Required => {
                    EncryptedStorageConfig::production_required()
                }
                crate::core::config::StorageEncryptionPolicy::PlaintextForTests => {
                    if execution_mode.is_production() {
                        return Err(crate::core::AgentError::config(
                            "production runtime rejects plaintext storage policy; use encrypted storage or an explicit test/simulation constructor",
                        ));
                    }
                    #[cfg(any(test, feature = "simulation"))]
                    {
                        EncryptedStorageConfig::testing_plaintext()
                    }
                    #[cfg(not(any(test, feature = "simulation")))]
                    {
                        return Err(crate::core::AgentError::config(
                            "plaintext storage policy requires a test build or the simulation feature",
                        ));
                    }
                }
            };
            if config.storage.opaque_names {
                cfg = cfg.with_opaque_names();
            }

            let _ = test_mode; // Suppress unused warning
            cfg
        };
        let plain_storage = FilesystemStorageHandler::new(config.storage.base_path.clone());
        let plain_storage = match profile_owner.as_ref() {
            Some(owner) => plain_storage
                .retain_profile_owner(owner.clone())
                .map_err(profile_error)?,
            None => plain_storage,
        };
        let selected_storage: Arc<dyn StorageEffects> = match custom.as_ref() {
            Some(providers) => providers.storage.clone(),
            None => Arc::new(plain_storage),
        };
        let storage_handler = Arc::new(EncryptedStorage::new(
            selected_storage,
            Arc::new(crypto_handler),
            secure_storage_handler,
            encrypted_storage_config,
        ));
        let leakage_handler =
            aura_effects::leakage::ProductionLeakageHandler::with_storage(storage_handler.clone());
        let tree_handler = PersistentTreeHandler::new(storage_handler.clone());
        let sync_handler = PersistentSyncHandler::new(storage_handler.clone());

        // === Build TransportSubsystem ===
        let transport_handler = aura_effects::transport::RealTransportHandler::default();
        // Use shared transport if provided (simulation mode), otherwise create new local inbox.
        if shared_transport.is_some() && shared_inbox.is_some() {
            tracing::warn!(
                "Shared transport and shared inbox both provided; using shared transport"
            );
        }
        if let Some(shared) = &shared_transport {
            shared.register(authority);
            shared.register_device(config.device_id, authority);
        }
        let transport_inbox = shared_inbox.unwrap_or_else(|| {
            shared_transport
                .as_ref()
                .map(|shared| shared.inbox_for(authority))
                .unwrap_or_else(|| Arc::new(RwLock::new(Vec::new())))
        });
        let transport =
            TransportSubsystem::from_parts(transport_handler, transport_inbox, shared_transport);

        // === Build JournalSubsystem ===
        let indexed_journal = Arc::new(IndexedJournalHandler::with_capacity(100_000));
        let fact_registry = Arc::new(build_fact_registry());
        let journal = JournalSubsystem::from_parts(
            indexed_journal,
            fact_registry,
            None, // fact_publish_tx attached later via attach_fact_sink
            Some(journal_policy),
            Some(journal_verifying_key),
        );

        // Pre-populate Biscuit cache so the guard chain works immediately.
        // For production, initialize_biscuit_cache() or bootstrap_biscuit_tokens()
        // will overwrite this with the persisted/real token later.
        let initial_biscuit_cache = {
            use base64::Engine;
            let engine = base64::engine::general_purpose::STANDARD;
            let token_authority = aura_authorization::TokenAuthority::new(authority);
            match token_authority.create_token(
                authority,
                crate::token_profiles::TokenCapabilityProfile::StandardDevice,
            ) {
                Ok(biscuit) => match biscuit.to_vec() {
                    Ok(token_bytes) => {
                        let root_pk_bytes = token_authority.root_public_key().to_bytes();
                        Some(BiscuitCache {
                            token_b64: engine.encode(&token_bytes),
                            issuer_authority: authority,
                            root_pk_b64: engine.encode(root_pk_bytes),
                        })
                    }
                    Err(_) => None,
                },
                Err(_) => None,
            }
        };

        let effect_system = Self {
            public_operation_activity: Arc::new(crate::runtime::system::RuntimeActivityGate::new()),
            config,
            authority_id: authority,
            execution_mode,
            harness_mode_enabled,
            crypto,
            enrollment_generation_gate: tokio::sync::Mutex::new(()),
            rejected_peer_facts: std::sync::atomic::AtomicU64::new(0),
            message_drops: std::sync::Mutex::new(MessageDropLog::default()),
            #[cfg(all(test, not(target_arch = "wasm32")))]
            enrollment_retirement_fault: std::sync::Mutex::new(None),
            transport,
            journal,
            composite,
            storage_handler,
            tree_handler,
            sync_handler,
            time_handler,
            logical_clock: Arc::new(LogicalClockManager::new(Some(device_id))),
            order_clock: OrderClockHandler,
            authorization_handler,
            leakage_handler,
            reactive_handler: ReactiveHandler::new(),
            choreography_state: parking_lot::RwLock::new(ChoreographyState::default()),
            vm_fragment_registry: parking_lot::RwLock::new(VmFragmentRegistry::default()),
            lan_transport: parking_lot::RwLock::new(None),
            rendezvous_manager: parking_lot::RwLock::new(None),
            move_manager: parking_lot::RwLock::new(None),
            biscuit_cache: parking_lot::RwLock::new(initial_biscuit_cache),
            receipt_signing_key,
            custom_random: custom.as_ref().map(|providers| providers.random.clone()),
            custom_console: custom.as_ref().map(|providers| providers.console.clone()),
            custom_transports: custom
                .map(|providers| providers.transports)
                .unwrap_or_default(),
            effect_api_ledger: parking_lot::Mutex::new(EffectApiLedgerState::default()),
            imported_invitation_decision_gate: tokio::sync::Mutex::new(()),
            guardian_recovery_keypair_gate: tokio::sync::Mutex::new(()),
            enrollment_manifest_admission_gate: tokio::sync::Mutex::new(()),
            enrollment_profile_handoff_gate: tokio::sync::Mutex::new(()),
            enrollment_invitee_window_owner: Arc::new(tokio::sync::Semaphore::new(1)),
            system_config: parking_lot::RwLock::new(HashMap::new()),
            #[cfg(not(target_arch = "wasm32"))]
            network_connections: parking_lot::RwLock::new(HashMap::new()),
            #[cfg(target_arch = "wasm32")]
            network_connections: parking_lot::RwLock::new(HashMap::new()),
            profile_owner,
        };

        tracing::info!(
            authority_id = %effect_system.authority_id,
            execution_mode = ?effect_system.execution_mode,
            choreography_backend = crate::CHOREO_BACKEND,
            choreography_session_mode = "per-session-task-bound",
            active_choreography_sessions = effect_system.choreography_state.read().active_session_count(),
            "initialized aura runtime effect system"
        );

        Ok(effect_system)
    }

    /// Check if the effect system is in test mode (bypasses authorization guards)
    pub fn is_testing(&self) -> bool {
        self.execution_mode.is_deterministic()
    }

    pub(crate) async fn initialize_selected_receipt_key(&self) {
        if self.custom_random.is_some() {
            let _ = self.receipt_signing_key().await;
        }
    }

    async fn receipt_signing_key(&self) -> &Ed25519SigningKey {
        self.receipt_signing_key
            .get_or_init(|| async {
                Ed25519SigningKey::from_bytes(RandomCoreEffects::random_bytes_32(self).await)
            })
            .await
    }

    pub(crate) async fn sign_flow_receipt(
        &self,
        receipt: &mut aura_core::Receipt,
    ) -> Result<(), AuraError> {
        crate::runtime::receipt_model::sign_flow_receipt(receipt, self.receipt_signing_key().await)
    }

    pub(crate) fn verify_transport_flow_receipt(
        &self,
        receipt: &aura_core::effects::transport::TransportReceipt,
    ) -> Result<(), aura_core::effects::transport::TransportError> {
        crate::runtime::receipt_model::verify_transport_flow_receipt(receipt)
    }

    pub(crate) async fn bind_transport_receipt_to_envelope(
        &self,
        receipt: &mut aura_core::effects::transport::TransportReceipt,
        envelope: &aura_core::effects::transport::TransportEnvelope,
    ) -> Result<(), aura_core::effects::transport::TransportError> {
        crate::runtime::receipt_model::sign_transport_receipt_for_envelope(
            receipt,
            envelope,
            self.receipt_signing_key().await,
        )
    }

    /// Check if the effect system is in explicit test mode (not simulation).
    pub fn is_test_mode(&self) -> bool {
        matches!(self.execution_mode, ExecutionMode::Testing)
    }

    /// Check whether harness diagnostics are enabled for this runtime instance.
    pub fn harness_mode_enabled(&self) -> bool {
        self.harness_mode_enabled
    }

    fn ensure_mock_network(&self) -> Result<(), NetworkError> {
        if self.execution_mode.is_deterministic() {
            Ok(())
        } else {
            Err(NetworkError::NotImplemented)
        }
    }

    fn effect_api_append(&self, event: Vec<u8>, epoch: u64) {
        let subscribers = {
            let mut ledger = self.effect_api_ledger.lock();
            ledger.events.push((epoch, event.clone()));
            std::mem::take(&mut ledger.subscribers)
        };
        let mut retained = Vec::with_capacity(subscribers.len());
        for mut sender in subscribers {
            if sender
                .try_send(aura_protocol::effects::EffectApiEvent::EventAppended {
                    epoch,
                    event: event.clone(),
                })
                .is_ok()
            {
                retained.push(sender);
            }
        }
        self.effect_api_ledger.lock().subscribers = retained;
    }

    fn effect_api_publish_device_activity(&self, device_id: aura_core::DeviceId, last_seen: u64) {
        let subscribers = {
            let mut ledger = self.effect_api_ledger.lock();
            std::mem::take(&mut ledger.subscribers)
        };
        let mut retained = Vec::with_capacity(subscribers.len());
        for mut sender in subscribers {
            if sender
                .try_send(aura_protocol::effects::EffectApiEvent::DeviceActivity {
                    device_id,
                    last_seen,
                })
                .is_ok()
            {
                retained.push(sender);
            }
        }
        self.effect_api_ledger.lock().subscribers = retained;
    }

    /// Get the shared reactive handler (signal graph) for this runtime.
    pub fn reactive_handler(&self) -> ReactiveHandler {
        self.reactive_handler.clone()
    }

    /// Retain actual local tree mutation custody through a checked decision.
    pub(crate) async fn lock_tree_decision(
        &self,
    ) -> aura_protocol::handlers::tree::TreeDecisionLease<'_> {
        self.tree_handler.lock_decision().await
    }

    /// Install the exact signed post-commit extension under the actual tree gate.
    /// Historical evidence cannot overwrite a later current local tree decision.
    pub(crate) async fn install_confirmed_enrollment_transition(
        &self,
        confirmed: &crate::handlers::invitation::enrollment_manifest_admission::DurableConfirmedEnrollmentCapability,
    ) -> Result<aura_protocol::handlers::tree::TreeDecisionLease<'_>, AuraError> {
        #[derive(Debug, thiserror::Error)]
        #[error(
            "current local tree evidence does not authorize the retained enrollment activation"
        )]
        struct LaterEnrollmentEvidence;
        let proof = confirmed.confirmation();
        let manifest = proof.manifest();
        let transition = proof.committed_transition();
        if manifest.invitee_device != self.device_id() {
            return Err(AuraError::PermissionDenied {
                message: "committed tree belongs to another physical device".into(),
                source: Some(Arc::new(LaterEnrollmentEvidence)),
            });
        }
        let lease = self.tree_handler.lock_decision().await;
        let current = lease
            .install_authenticated_extension(manifest.baseline_count as usize, transition.ops())
            .await?;
        let verified =
            proof
                .verify_local_extension(&current)
                .map_err(|source| AuraError::Internal {
                    message: "authenticate current local enrollment tree extension".into(),
                    source: Some(Arc::new(source)),
                })?;
        let state = verified.state();
        if state.epoch.value() != manifest.pending_epoch
            || ![manifest.initiator_device, manifest.invitee_device]
                .iter()
                .all(|device| {
                    state.leaves.values().any(|leaf| {
                        leaf.device_id == *device && leaf.role == aura_core::LeafRole::Device
                    })
                })
        {
            return Err(AuraError::PermissionDenied {
                message: "verified committed tree lacks required device membership".into(),
                source: Some(Arc::new(LaterEnrollmentEvidence)),
            });
        }
        Ok(lease)
    }

    pub async fn export_tree_ops(
        &self,
    ) -> Result<Vec<aura_core::AttestedOp>, crate::core::AgentError> {
        self.tree_handler
            .export_ops()
            .await
            .map_err(crate::core::AgentError::from)
    }

    /// Import tree ops replicated from another device of this authority.
    ///
    /// Every new operation must extend the local tree and verify under an
    /// independently stored parent-epoch key. The complete extension is
    /// checked before any operation is persisted. Duplicate operations are
    /// idempotent; invalid or divergent operations reject the exchange.
    pub async fn import_verified_tree_ops(
        &self,
        ops: &[aura_core::AttestedOp],
    ) -> Result<usize, crate::core::AgentError> {
        use aura_core::tree::verification::extract_target_node;
        let encode = |op: &aura_core::AttestedOp| {
            aura_core::util::serialization::to_vec(op)
                .map_err(|error| crate::core::AgentError::internal(error.to_string()))
        };
        let mut staged = self.export_tree_ops().await?;
        let mut known = std::collections::BTreeSet::new();
        for op in &staged {
            known.insert(encode(op)?);
        }
        let mut additions = Vec::new();
        for op in ops {
            if !known.insert(encode(op)?) {
                continue;
            }
            let state = aura_journal::commitment_tree::reduce(&staged).map_err(|error| {
                crate::core::AgentError::Aura(AuraError::crypto(format!(
                    "Cannot verify sibling tree against invalid local log: {error}"
                )))
            })?;
            if op.op.parent_epoch != state.epoch || op.op.parent_commitment != state.root_commitment
            {
                return Err(AuraError::crypto(
                    "Sibling tree operation does not extend the current parent epoch",
                )
                .into());
            }
            let target = extract_target_node(&op.op.op).or_else(|| match &op.op.op {
                aura_core::TreeOpKind::RemoveLeaf { leaf, .. } => {
                    state.get_remove_leaf_affected_parent(leaf)
                }
                _ => None,
            });
            let target = target.ok_or_else(|| {
                AuraError::crypto("Sibling tree operation has no verifiable signing node")
            })?;
            let (key, threshold) = self
                .trusted_tree_parent_verifier(&self.authority_id, op.op.parent_epoch.value())
                .await
                .map_err(crate::core::AgentError::from)?;
            if let Some(branch_key) = state.get_signing_key(&target) {
                if branch_key != &key {
                    return Err(AuraError::crypto(
                        "Trusted parent-epoch key conflicts with committed tree state",
                    )
                    .into());
                }
            }
            aura_core::tree::verify_attested_op(op, &key, threshold, state.epoch).map_err(
                |error| AuraError::crypto(format!("Invalid sibling tree signature: {error}")),
            )?;
            staged.push(op.clone());
            aura_journal::commitment_tree::reduce(&staged).map_err(|error| {
                AuraError::crypto(format!("Invalid sibling tree transition: {error}"))
            })?;
            additions.push(op.clone());
        }
        self.tree_handler
            .import_ops(&additions)
            .await
            .map_err(crate::core::AgentError::from)?;
        Ok(additions.len())
    }

    /// Adopt `ops` as this device's whole tree OpLog (a device joining an
    /// existing account drops any provisional history; see `replace_ops`).
    pub async fn replace_tree_ops(
        &self,
        ops: &[aura_core::AttestedOp],
    ) -> Result<(), crate::core::AgentError> {
        self.tree_handler
            .replace_ops(ops)
            .await
            .map_err(crate::core::AgentError::from)
    }

    /// The strong independently admitted baseline is required on this owner path.
    pub(crate) async fn install_admitted_enrollment_baseline(
        &self,
        admitted:&crate::handlers::invitation::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        original: [u8; 32],
    ) -> Result<(), AuraError> {
        self.tree_handler
            .install_baseline_if_original(original, admitted.baseline().ops())
            .await
    }

    /// Attach a fact sink for reactive scheduling (facts → scheduler ingestion).
    ///
    /// This is called during runtime startup when the ReactivePipeline is started.
    pub(crate) fn attach_fact_sink(
        &self,
        tx: crate::reactive::FactIngress,
    ) -> Result<(), crate::reactive::FactProcessingError> {
        tx.require_runtime_owner(self)?;
        self.journal.attach_fact_sink(tx);
        Ok(())
    }

    /// Snapshot the runtime choreography session bound to the current task.
    pub(crate) fn current_runtime_choreography_session_id(
        &self,
    ) -> Option<crate::runtime::RuntimeChoreographySessionId> {
        self.choreography_state.read().current_session_id()
    }

    /// Attach the admitted protocol identifier to the current task-bound runtime session.
    pub(crate) fn set_current_runtime_choreography_protocol_id(
        &self,
        protocol_id: impl Into<String>,
    ) -> Result<(), String> {
        let session_id = self
            .current_runtime_choreography_session_id()
            .ok_or_else(|| {
                "cannot attach a protocol id without an active choreography session".to_string()
            })?;
        self.choreography_state
            .write()
            .set_session_protocol_id(session_id, protocol_id)
    }

    /// Claim authoritative ownership for one active runtime choreography session.
    pub(crate) fn claim_runtime_choreography_session_owner(
        &self,
        session_id: crate::runtime::RuntimeChoreographySessionId,
        owner_label: impl Into<String>,
    ) -> Result<crate::runtime::subsystems::choreography::SessionOwnerCapability, String> {
        self.choreography_state
            .write()
            .claim_session_owner(session_id, owner_label)
            .map_err(|error| error.to_string())
    }

    /// Ensure the current owner record still matches the expected local owner.
    pub(crate) fn ensure_runtime_choreography_session_owner_capability(
        &self,
        session_id: crate::runtime::RuntimeChoreographySessionId,
        expected_capability: &crate::runtime::subsystems::choreography::SessionOwnerCapability,
    ) -> Result<(), String> {
        self.choreography_state
            .read()
            .ensure_session_owner(session_id, expected_capability)
            .map_err(|error| error.to_string())
    }

    /// Snapshot the authoritative local owner label for the current bound choreography session.
    pub(crate) fn current_runtime_choreography_session_owner_label(
        &self,
    ) -> Result<String, String> {
        let session_id = self
            .current_runtime_choreography_session_id()
            .ok_or_else(|| {
                "cannot resolve runtime session owner label without an active choreography session"
                    .to_string()
            })?;
        self.choreography_state
            .read()
            .session_owner(session_id)
            .map(|owner| owner.owner_label.clone())
            .ok_or_else(|| format!("runtime session {session_id} has no owner record"))
    }

    /// Retire exactly the registered owner held by a dropped VM handle. Validation
    /// and cleanup share the same lock ordering as ownership transfer. This is
    /// resource cancellation, not a terminal decision or required async close.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "SessionOwnerCapability",
        family = "runtime_helper"
    )]
    pub(crate) fn retire_dropped_vm_owner(
        &self,
        capability: &crate::runtime::subsystems::choreography::SessionOwnerCapability,
    ) -> Result<(), crate::runtime::subsystems::choreography::SessionOwnershipError> {
        use crate::runtime::subsystems::choreography::SessionOwnershipError;
        let mut state = self.choreography_state.write();
        match state.ensure_session_owner(capability.session_id(), capability) {
            Ok(()) => {}
            // Required close already retired this owner before reporting a clock
            // failure. Do not invent another cleanup failure or touch newer state.
            Err(SessionOwnershipError::MissingOwner { .. })
                if !state.contains_registered_session(capability.session_id()) =>
            {
                return Ok(())
            }
            Err(error) => return Err(error),
        }
        let mut fragments = self.vm_fragment_registry.write();
        state.cancel_session(capability.session_id());
        fragments.release_session(capability.session_id());
        Ok(())
    }

    /// Atomically transfer authoritative ownership for one active runtime choreography session.
    pub(crate) fn transfer_runtime_choreography_session_owner(
        &self,
        session_id: crate::runtime::RuntimeChoreographySessionId,
        expected_capability: &crate::runtime::subsystems::choreography::SessionOwnerCapability,
        next_owner_label: impl Into<String>,
        next_scope: crate::runtime::subsystems::choreography::SessionOwnerCapabilityScope,
    ) -> Result<crate::runtime::subsystems::choreography::SessionOwnerCapability, String> {
        let next_owner_label = next_owner_label.into();
        let mut choreography_state = self.choreography_state.write();
        choreography_state
            .ensure_session_owner(session_id, expected_capability)
            .map_err(|error| error.to_string())?;

        let transferred_fragments = self
            .vm_fragment_registry
            .write()
            .transfer_session_if_present(
                session_id,
                expected_capability.owner_label(),
                &next_owner_label,
            )
            .map_err(|error| error.to_string())?;

        let next_capability = choreography_state
            .transfer_session_owner(
                session_id,
                expected_capability,
                next_owner_label.clone(),
                next_scope,
            )
            .map_err(|error| error.to_string())?;

        if transferred_fragments > 0 {
            tracing::info!(
                session_id = %session_id,
                from_owner = %expected_capability.owner_label(),
                to_owner = %next_owner_label,
                fragment_count = transferred_fragments,
                "transferred runtime choreography session owner and fragment ownership together"
            );
        }

        Ok(next_capability)
    }

    /// Claim local ownership for every fragment described by one manifest in the current session.
    pub(crate) fn claim_vm_fragments_for_manifest(
        &self,
        owner_label: impl Into<String>,
        manifest: &CompositionManifest,
    ) -> Result<Vec<VmFragmentId>, String> {
        let session_id = self
            .current_runtime_choreography_session_id()
            .ok_or_else(|| {
                format!(
                "cannot claim protocol fragments for protocol {} without an active choreography session",
                manifest.protocol_id
            )
            })?;
        let owner_label = owner_label.into();
        let claimed = self
            .vm_fragment_registry
            .write()
            .claim_manifest(session_id, owner_label.clone(), manifest)
            .map_err(|error| error.to_string())?;
        tracing::debug!(
            session_id = %session_id,
            protocol_id = %manifest.protocol_id,
            owner_label = %owner_label,
            fragment_count = claimed.len(),
            "claimed local protocol fragment ownership"
        );
        Ok(claimed)
    }

    /// Release all locally owned fragments for one runtime session.
    pub(crate) fn release_vm_fragments_for_session(
        &self,
        session_id: crate::runtime::RuntimeChoreographySessionId,
    ) -> Vec<VmFragmentId> {
        let released = self
            .vm_fragment_registry
            .write()
            .release_session(session_id);
        if !released.is_empty() {
            tracing::debug!(
                session_id = %session_id,
                fragment_count = released.len(),
                "released local protocol fragment ownership"
            );
        }
        released
    }

    /// Release one explicit set of locally owned fragments.
    pub(crate) fn release_vm_fragments(&self, fragment_ids: &[VmFragmentId]) -> Vec<VmFragmentId> {
        let released = self
            .vm_fragment_registry
            .write()
            .release_fragments(fragment_ids);
        if !released.is_empty() {
            tracing::debug!(
                fragment_count = released.len(),
                "released explicitly claimed local protocol fragments"
            );
        }
        released
    }

    #[cfg(test)]
    pub fn vm_fragment_snapshot(
        &self,
    ) -> Vec<(
        VmFragmentId,
        crate::runtime::subsystems::VmFragmentOwnerRecord,
    )> {
        self.vm_fragment_registry.read().snapshot()
    }

    /// Observe processing of publications accepted before this original ingress
    /// barrier. This grants no journal commit or canonical entity evidence.
    /// Required mutation paths retain their exact commit target instead.
    pub(crate) async fn await_reactive_publications_in_original_window(
        &self,
        original: &TimeoutBudget,
    ) -> Result<(), AuraError> {
        execute_with_timeout_budget(self, original, || async {
            let target = self
                .journal
                .publish_required(Vec::new())
                .await
                .map_err(|source| AuraError::Internal {
                    message: "required original processing barrier enqueue failed".into(),
                    source: Some(Arc::new(source)),
                })?;
            self.journal
                .await_required(target, self, original)
                .await
                .map_err(|source| AuraError::Internal {
                    message: "required original processing barrier failed".into(),
                    source: Some(Arc::new(source)),
                })
        })
        .await
        .map_err(|source| AuraError::Internal {
            message: "required original processing barrier window failed".into(),
            source: Some(Arc::new(source)),
        })
    }

    pub fn requeue_envelope(
        &self,
        envelope: TransportEnvelope,
    ) -> crate::runtime::subsystems::transport::QueueEnvelopeOutcome {
        // The envelope was already admitted against its flow window when it
        // was taken; putting it back must not make it look like a replay.
        if let Some(receipt) = envelope.receipt.as_ref() {
            let device = envelope
                .metadata
                .get("aura-source-device-id")
                .map(String::as_str);
            self.transport.flow().allow_readmit(receipt, device);
        }
        self.queue_runtime_envelope(envelope)
    }

    /// Take a buffered choreography envelope for a session this device has not
    /// opened (see `ChoreographyState::take_unclaimed_session_envelope`).
    pub(crate) fn take_unclaimed_choreography_envelope(
        &self,
        accept: impl Fn(&TransportEnvelope) -> bool,
    ) -> Option<TransportEnvelope> {
        self.choreography_state
            .write()
            .take_unclaimed_session_envelope(accept)
    }

    /// Record that `peer` was verified reachable at `now_ms`.
    // Only the native LAN ingress path records reachability.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn record_peer_reachable(&self, peer: AuthorityId, now_ms: u64) {
        if peer != self.authority_id {
            self.transport.record_peer_reachable(peer, now_ms);
        }
    }

    /// Distinct peers verified reachable within `window_ms` of `now_ms`.
    pub(crate) fn reachable_peer_count(&self, now_ms: u64, window_ms: u64) -> usize {
        self.transport.reachable_peer_count(now_ms, window_ms)
    }

    pub(crate) fn queue_runtime_envelope(
        &self,
        envelope: TransportEnvelope,
    ) -> crate::runtime::subsystems::transport::QueueEnvelopeOutcome {
        if let Some(session_id) = Self::choreography_session_id_from_envelope(&envelope) {
            self.choreography_state
                .write()
                .queue_session_envelope(session_id, envelope);
            return crate::runtime::subsystems::transport::QueueEnvelopeOutcome::Queued;
        }

        self.transport.queue_envelope(envelope)
    }

    /// Clone queued choreography envelopes addressed to this runtime without consuming them.
    ///
    /// Covers both session-local inboxes (network ingress) and the transport inbox
    /// (shared in-memory transport), so callers can discover sessions they have not opened.
    pub(crate) fn peek_queued_choreography_envelopes(&self) -> Vec<TransportEnvelope> {
        let mut envelopes = self
            .choreography_state
            .read()
            .queued_session_envelopes_snapshot();
        let inbox = self.transport.inbox();
        envelopes.extend(
            inbox
                .read()
                .iter()
                .filter(|envelope| {
                    envelope.destination == self.authority_id
                        && Self::choreography_session_id_from_envelope(envelope).is_some()
                })
                .cloned(),
        );
        envelopes
    }

    fn choreography_session_id_from_envelope(
        envelope: &TransportEnvelope,
    ) -> Option<RuntimeChoreographySessionId> {
        let is_choreography = envelope
            .metadata
            .get("content-type")
            .is_some_and(|value| value == "application/aura-choreography");
        if !is_choreography {
            return None;
        }
        let session_id = envelope.metadata.get("session-id")?;
        let Ok(session_uuid) = uuid::Uuid::parse_str(session_id) else {
            return None;
        };
        Some(RuntimeChoreographySessionId::from_uuid(session_uuid))
    }

    pub fn attach_lan_transport(&self, service: Arc<LanTransportService>) {
        *self.lan_transport.write() = Some(service);
    }

    pub fn lan_transport(&self) -> Option<Arc<LanTransportService>> {
        self.lan_transport.read().clone()
    }

    pub fn attach_rendezvous_manager(&self, manager: RendezvousManager) {
        *self.rendezvous_manager.write() = Some(manager);
    }

    pub fn rendezvous_manager(&self) -> Option<RendezvousManager> {
        self.rendezvous_manager.read().clone()
    }

    pub fn attach_move_manager(&self, manager: MoveManager) {
        *self.move_manager.write() = Some(manager);
    }

    pub fn move_manager(&self) -> Option<MoveManager> {
        self.move_manager.read().clone()
    }

    /// Load persisted Biscuit tokens from secure storage into the in-memory cache.
    ///
    /// Called during startup (builder) to restore tokens for returning users.
    /// For new users the cache remains empty until `bootstrap_authority()` creates tokens.
    ///
    /// Storage format: `[32 bytes root public key][N bytes biscuit token]`
    pub async fn initialize_biscuit_cache(&self) -> Result<(), AuraError> {
        use aura_core::effects::secure::{SecureStorageCapability, SecureStorageLocation};
        use aura_core::effects::SecureStorageEffects;
        use base64::Engine;

        let location = SecureStorageLocation::biscuit_authority(&self.authority_id);
        let caps = [SecureStorageCapability::Read];

        match self.secure_retrieve(&location, &caps).await {
            Ok(bytes) if bytes.len() > 32 => {
                if bytes.len() > 1_048_576 {
                    return Err(BiscuitStartupRecordError::Oversized.into());
                }
                let root =
                    aura_authorization::PublicKey::from_bytes(&bytes[..32]).map_err(|source| {
                        AuraError::Crypto {
                            message: "decode persisted Biscuit root key".into(),
                            source: Some(Arc::new(source)),
                        }
                    })?;
                aura_authorization::VerifiedBiscuitToken::from_bytes(&bytes[32..], root).map_err(
                    |source| AuraError::Crypto {
                        message: "verify persisted Biscuit token".into(),
                        source: Some(Arc::new(source)),
                    },
                )?;
                let engine = base64::engine::general_purpose::STANDARD;
                let root_pk_b64 = engine.encode(&bytes[..32]);
                let token_b64 = engine.encode(&bytes[32..]);

                *self.biscuit_cache.write() = Some(BiscuitCache {
                    token_b64,
                    issuer_authority: self.authority_id,
                    root_pk_b64,
                });
                tracing::info!("Biscuit cache initialized from secure storage");
            }
            Ok(bytes) => {
                return Err(BiscuitStartupRecordError::Truncated(bytes.len()).into());
            }
            Err(error)
                if std::error::Error::source(&error).is_some_and(|source| {
                    source
                        .downcast_ref::<aura_core::effects::secure::SecureStorageRecordMissing>()
                        .is_some_and(|missing| missing.location() == &location)
                }) =>
            {
                tracing::debug!("No biscuit found in secure storage (new account)");
            }
            Err(error) => return Err(error),
        }
        Ok(())
    }

    /// Runtime-private publication after required authorization validation.
    fn publish_biscuit_cache(&self, cache: BiscuitCache) {
        *self.biscuit_cache.write() = Some(cache);
    }

    #[cfg(test)]
    pub(crate) fn set_biscuit_cache(&self, cache: BiscuitCache) {
        self.publish_biscuit_cache(cache);
    }

    #[cfg(test)]
    pub(crate) fn clear_biscuit_cache(&self) {
        *self.biscuit_cache.write() = None;
    }

    /// Get the current biscuit cache (for guard chain metadata).
    pub fn biscuit_cache(&self) -> Option<BiscuitCache> {
        self.biscuit_cache.read().clone()
    }

    /// Build journal-sync Biscuit authorization from the verified frontier.
    ///
    /// Returns `None` until the authority has a Biscuit frontier (before
    /// account bootstrap).
    pub fn sync_biscuit_authorization(
        &self,
    ) -> Result<
        Option<(
            aura_authorization::BiscuitTokenManager,
            aura_guards::BiscuitGuardEvaluator,
        )>,
        AuraError,
    > {
        Ok(self.verified_biscuit_frontier()?.map(|(token, bridge)| {
            (
                aura_authorization::BiscuitTokenManager::new(
                    self.authorization_handler.authority_id(),
                    token.token().clone(),
                ),
                aura_guards::BiscuitGuardEvaluator::new(
                    aura_guards::BiscuitAuthorizationBridge::new(
                        bridge.root_public_key(),
                        bridge.authority_id(),
                    ),
                ),
            )
        }))
    }

    /// Verify the cached Biscuit frontier against the runtime's trusted root key.
    pub fn verified_biscuit_frontier(
        &self,
    ) -> Result<
        Option<(
            aura_authorization::VerifiedBiscuitToken,
            aura_authorization::BiscuitAuthorizationBridge,
        )>,
        AuraError,
    > {
        use base64::Engine;

        let Some(cache) = self.biscuit_cache() else {
            return Ok(None);
        };

        let trusted_authority = self.authorization_handler.authority_id();
        if cache.issuer_authority != trusted_authority {
            return Err(AuraError::invalid(format!(
                "cached Biscuit issuer {} does not match trusted authority {}",
                cache.issuer_authority, trusted_authority
            )));
        }

        let engine = base64::engine::general_purpose::STANDARD;
        let token_bytes =
            engine
                .decode(cache.token_b64)
                .map_err(|error| AuraError::Serialization {
                    message: "decode cached Biscuit token bytes".into(),
                    source: Some(Arc::new(error)),
                })?;

        let cached_root_bytes =
            engine
                .decode(cache.root_pk_b64)
                .map_err(|error| AuraError::Serialization {
                    message: "decode cached Biscuit root public key".into(),
                    source: Some(Arc::new(error)),
                })?;
        let root_public_key = aura_authorization::PublicKey::from_bytes(&cached_root_bytes)
            .map_err(|error| AuraError::Crypto {
                message: "parse cached Biscuit root public key".into(),
                source: Some(Arc::new(error)),
            })?;

        let token =
            aura_authorization::VerifiedBiscuitToken::from_bytes(&token_bytes, root_public_key)
                .map_err(|error| AuraError::Crypto {
                    message: "verify cached Biscuit token".into(),
                    source: Some(Arc::new(error)),
                })?;
        let bridge =
            aura_authorization::BiscuitAuthorizationBridge::new(root_public_key, trusted_authority);

        Ok(Some((token, bridge)))
    }

    /// Create and persist Biscuit authorization tokens during account bootstrap.
    ///
    /// Creates a `TokenAuthority`, mints a self-token with full capabilities,
    /// persists `[32 bytes root PK][N bytes token]` to secure storage, and
    /// populates the in-memory `BiscuitCache` so the guard chain works immediately.
    pub async fn bootstrap_biscuit_tokens(&self, authority: &AuthorityId) -> Result<(), AuraError> {
        use aura_core::effects::secure::{SecureStorageCapability, SecureStorageLocation};
        use aura_core::effects::SecureStorageEffects;
        use base64::Engine;

        let token_authority = aura_authorization::TokenAuthority::new(*authority);
        let biscuit = token_authority
            .create_token(
                *authority,
                crate::token_profiles::TokenCapabilityProfile::StandardDevice,
            )
            .map_err(|e| AuraError::internal(format!("Failed to create Biscuit token: {e}")))?;

        let token_bytes = biscuit
            .to_vec()
            .map_err(|e| AuraError::internal(format!("Failed to serialize Biscuit: {e}")))?;
        let root_pk_bytes = token_authority.root_public_key().to_bytes();

        // Persist as [32 bytes root PK][N bytes token]
        let mut storage_bytes = Vec::with_capacity(32 + token_bytes.len());
        storage_bytes.extend_from_slice(&root_pk_bytes);
        storage_bytes.extend_from_slice(&token_bytes);

        let location = SecureStorageLocation::biscuit_authority(authority);
        let caps = vec![SecureStorageCapability::Write];
        self.secure_store(&location, &storage_bytes, &caps).await?;

        // Populate in-memory cache immediately
        let engine = base64::engine::general_purpose::STANDARD;
        self.publish_biscuit_cache(BiscuitCache {
            token_b64: engine.encode(&token_bytes),
            issuer_authority: *authority,
            root_pk_b64: engine.encode(root_pk_bytes),
        });

        tracing::info!(%authority, "Biscuit authorization tokens bootstrapped");
        Ok(())
    }

    async fn publish_typed_facts(&self, facts: Vec<TypedFact>) -> Result<(), AuraError> {
        if !self.journal.has_fact_sink() {
            return Ok(());
        }

        self.journal
            .publish_facts(crate::reactive::FactSource::Journal(facts))
            .await
            .map_err(|source| AuraError::Internal {
                message: source.to_string(),
                source: Some(Arc::new(source)),
            })?;

        Ok(())
    }

    fn typed_fact_storage_prefix(authority_id: AuthorityId) -> String {
        format!("{}/{}/", TYPED_FACT_STORAGE_PREFIX, authority_id)
    }

    fn typed_fact_storage_key(
        authority_id: AuthorityId,
        order: &aura_core::time::OrderTime,
    ) -> String {
        format!(
            "{}{}",
            Self::typed_fact_storage_prefix(authority_id),
            hex::encode(order.0)
        )
    }

    /// Commit a batch of typed relational facts into the canonical fact store and publish them.
    ///
    /// This is the single write path for UI-facing facts in the runtime.
    pub async fn commit_relational_facts(
        &self,
        facts: Vec<RelationalFact>,
    ) -> Result<Vec<TypedFact>, AuraError> {
        let committed = self.persist_relational_facts(facts).await?;
        self.publish_typed_facts(committed.clone()).await?;
        Ok(committed)
    }

    /// Commit canonical facts and retain exact original scheduler processing custody.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "required_reactive_fact_commit", capability_type = RequiredReactiveFactCommitCapability, family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub(crate) async fn commit_relational_facts_required(
        &self,
        facts: Vec<RelationalFact>,
    ) -> Result<RequiredReactiveFactCommitCapability<'_>, AuraError> {
        let committed = self.persist_relational_facts(facts).await?;
        let target = self
            .journal
            .publish_required(committed.clone())
            .await
            .map_err(|source| AuraError::Internal {
                message: "required canonical fact publication failed after persistence".into(),
                source: Some(Arc::new(source)),
            })?;
        Ok(RequiredReactiveFactCommitCapability {
            effects: self,
            committed,
            target,
        })
    }

    async fn persist_relational_facts(
        &self,
        facts: Vec<RelationalFact>,
    ) -> Result<Vec<TypedFact>, AuraError> {
        if facts.is_empty() {
            return Ok(vec![]);
        }

        let mut committed: Vec<TypedFact> = Vec::with_capacity(facts.len());
        for rel in facts {
            let order = self
                .order_time()
                .await
                .map_err(|source| AuraError::Internal {
                    message: format!("order_time: {source}"),
                    source: Some(Arc::new(source)),
                })?;

            let fact = TypedFact::new(
                order.clone(),
                aura_core::time::TimeStamp::OrderClock(order.clone()),
                FactContent::Relational(rel),
            );

            let key = Self::typed_fact_storage_key(self.authority_id, &order);
            let bytes = aura_core::util::serialization::to_vec(&fact).map_err(|source| {
                AuraError::Serialization {
                    message: format!("serialize fact: {source}"),
                    source: Some(Arc::new(source)),
                }
            })?;
            self.store(&key, bytes)
                .await
                .map_err(|source| AuraError::Storage {
                    message: format!("persist fact: {source}"),
                    source: Some(Arc::new(source)),
                })?;

            committed.push(fact);
        }

        // Publish after persistence so subscribers can always recover from storage.

        Ok(committed)
    }

    /// Commit a batch of typed relational facts with options.
    ///
    /// Same as `commit_relational_facts` but allows specifying options like ack tracking.
    pub async fn commit_relational_facts_with_options(
        &self,
        facts: Vec<RelationalFact>,
        options: FactOptions,
    ) -> Result<Vec<TypedFact>, AuraError> {
        if facts.is_empty() {
            return Ok(vec![]);
        }

        let mut committed: Vec<TypedFact> = Vec::with_capacity(facts.len());
        for rel in facts {
            let order = self
                .order_time()
                .await
                .map_err(|source| AuraError::Internal {
                    message: format!("order_time: {source}"),
                    source: Some(Arc::new(source)),
                })?;

            let mut fact = TypedFact::new(
                order.clone(),
                aura_core::time::TimeStamp::OrderClock(order.clone()),
                FactContent::Relational(rel),
            );

            // Apply options
            if options.request_acks {
                fact = fact.with_ack_tracking();
            }
            if let Some(agreement) = &options.initial_agreement {
                fact = fact.with_agreement(agreement.clone());
            }

            let key = Self::typed_fact_storage_key(self.authority_id, &order);
            let bytes = aura_core::util::serialization::to_vec(&fact).map_err(|source| {
                AuraError::Serialization {
                    message: format!("serialize fact: {source}"),
                    source: Some(Arc::new(source)),
                }
            })?;
            self.store(&key, bytes)
                .await
                .map_err(|source| AuraError::Storage {
                    message: format!("persist fact: {source}"),
                    source: Some(Arc::new(source)),
                })?;

            committed.push(fact);
        }

        // Publish after persistence so subscribers can always recover from storage.
        self.publish_typed_facts(committed.clone()).await?;

        Ok(committed)
    }

    /// Commit a domain fact under its own envelope, keeping its declared
    /// type id, schema version and encoding.
    pub async fn commit_domain_fact<F: aura_journal::DomainFact>(
        &self,
        context_id: ContextId,
        fact: &F,
    ) -> Result<TypedFact, AuraError> {
        self.commit_generic_envelope(context_id, fact.to_envelope())
            .await
    }

    /// Commit a caller-built envelope. The caller owns its type id, schema
    /// version and encoding; domain facts go through [`Self::commit_domain_fact`].
    pub(crate) async fn commit_generic_envelope(
        &self,
        context_id: ContextId,
        envelope: aura_core::types::facts::FactEnvelope,
    ) -> Result<TypedFact, AuraError> {
        let rel = RelationalFact::Generic {
            context_id,
            envelope,
        };
        let mut committed = self.commit_relational_facts(vec![rel]).await?;
        Ok(committed
            .pop()
            .unwrap_or_else(|| unreachable!("commit_relational_facts committed exactly one")))
    }

    /// Admit a relational fact received from a peer (or a sibling device, which
    /// may run another build) before it is committed. Required views treat a
    /// decode fault of a committed fact as terminal, so a fact they cannot
    /// decode (an unsupported schema, a malformed payload) is rejected here,
    /// per fact: logged, counted in [`Self::rejected_peer_fact_count`], and
    /// never committed. Locally committed facts keep the terminal contract.
    pub(crate) fn admit_peer_fact(
        &self,
        source: AuthorityId,
        fact: &RelationalFact,
    ) -> Result<(), PeerFactRejection> {
        crate::reactive::app_signal_views::check_required_projection_fact(fact).map_err(|cause| {
            let rejection = PeerFactRejection::new(source, fact, cause);
            self.rejected_peer_facts
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tracing::warn!(error = %rejection, "rejected peer fact at ingress");
            rejection
        })
    }

    /// How many peer facts [`Self::admit_peer_fact`] has rejected.
    pub fn rejected_peer_fact_count(&self) -> u64 {
        self.rejected_peer_facts
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Record a dropped chat message (an inbound intake or receive-gate
    /// refusal, or an outbound delivery failure): logged at warn with its
    /// typed reason, counted, and kept (bounded, newest last) for
    /// diagnostics. Observation only; never parity-critical.
    pub(crate) fn record_message_drop(&self, drop: MessageDrop) {
        tracing::warn!(
            direction = ?drop.reason.direction(),
            context_id = ?drop.context_id,
            channel_id = ?drop.channel_id,
            peer_id = ?drop.peer_id,
            message_id = ?drop.message_id,
            reason = %drop.reason,
            "chat message dropped"
        );
        self.message_drops
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(drop);
    }

    /// Recent dropped chat messages, both directions (newest last), and the
    /// total dropped since startup.
    pub fn message_drops(&self) -> (Vec<MessageDrop>, u64) {
        let log = self
            .message_drops
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (log.recent.iter().cloned().collect(), log.total)
    }

    /// Import committed facts replicated from another device of this authority.
    ///
    /// Facts keep their original order key, so importing is idempotent; only
    /// facts not already stored are persisted and published. A fact the
    /// required views cannot decode is rejected per fact
    /// ([`Self::admit_peer_fact`]). Returns how many were new.
    pub async fn import_committed_facts(&self, facts: Vec<TypedFact>) -> Result<usize, AuraError> {
        let mut imported = Vec::new();
        for fact in facts {
            if let FactContent::Relational(relational) = &fact.content {
                if self.admit_peer_fact(self.authority_id, relational).is_err() {
                    continue;
                }
            }
            let key = Self::typed_fact_storage_key(self.authority_id, &fact.order);
            let present = self
                .retrieve(&key)
                .await
                .map_err(|e| AuraError::storage(format!("retrieve: {e}")))?
                .is_some();
            if present {
                continue;
            }
            let bytes = aura_core::util::serialization::to_vec(&fact).map_err(|source| {
                AuraError::Serialization {
                    message: format!("serialize fact: {source}"),
                    source: Some(Arc::new(source)),
                }
            })?;
            self.store(&key, bytes)
                .await
                .map_err(|source| AuraError::Storage {
                    message: format!("persist fact: {source}"),
                    source: Some(Arc::new(source)),
                })?;
            imported.push(fact);
        }
        let count = imported.len();
        if count > 0 {
            self.publish_typed_facts(imported).await?;
        }
        Ok(count)
    }

    /// Re-publish committed facts in `contexts` so views re-derive them, e.g.
    /// after a channel key arrives for messages that were rendered sealed.
    /// Views apply facts idempotently. Returns how many were re-published.
    pub async fn republish_committed_facts_for_contexts(
        &self,
        contexts: &std::collections::BTreeSet<ContextId>,
    ) -> Result<usize, AuraError> {
        let facts: Vec<TypedFact> = self
            .load_committed_facts(self.authority_id)
            .await?
            .into_iter()
            .filter(|fact| match &fact.content {
                FactContent::Relational(relational) => contexts.contains(&relational.context_id()),
                _ => false,
            })
            .collect();
        let count = facts.len();
        if count > 0 {
            self.publish_typed_facts(facts).await?;
        }
        Ok(count)
    }

    /// Load all committed typed facts for the given authority from storage.
    pub async fn load_committed_facts(
        &self,
        authority_id: AuthorityId,
    ) -> Result<Vec<TypedFact>, AuraError> {
        let prefix = Self::typed_fact_storage_prefix(authority_id);
        let mut keys = self
            .list_keys(Some(&prefix))
            .await
            .map_err(|error| AuraError::Storage {
                message: "list committed fact keys".into(),
                source: Some(Arc::new(error)),
            })?;
        keys.sort();

        let mut facts = Vec::new();
        for key in keys {
            let Some(bytes) = self
                .retrieve(&key)
                .await
                .map_err(|error| AuraError::Storage {
                    message: "read committed fact".into(),
                    source: Some(Arc::new(error)),
                })?
            else {
                return Err(AuraError::invalid(
                    "committed fact index references absent storage",
                ));
            };

            let fact: TypedFact =
                aura_core::util::serialization::from_slice(&bytes).map_err(|error| {
                    AuraError::Serialization {
                        message: "decode committed fact".into(),
                        source: Some(Arc::new(error)),
                    }
                })?;
            facts.push(fact);
        }

        facts.sort();
        Ok(facts)
    }

    /// Check whether a consensus-finalized DKG transcript commit exists for an epoch.
    pub async fn has_dkg_transcript_commit(
        &self,
        authority_id: AuthorityId,
        context_id: ContextId,
        epoch: u64,
    ) -> Result<bool, AuraError> {
        let facts = self.load_committed_facts(authority_id).await?;
        for fact in facts {
            let FactContent::Relational(RelationalFact::Protocol(
                ProtocolRelationalFact::DkgTranscriptCommit(commit),
            )) = &fact.content
            else {
                continue;
            };

            if commit.context == context_id && commit.epoch == epoch {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Return the latest DKG transcript commit for a context, if any.
    pub async fn latest_dkg_transcript_commit(
        &self,
        authority_id: AuthorityId,
        context_id: ContextId,
    ) -> Result<Option<DkgTranscriptCommit>, AuraError> {
        let facts = self.load_committed_facts(authority_id).await?;
        let mut latest: Option<DkgTranscriptCommit> = None;
        for fact in facts {
            let FactContent::Relational(RelationalFact::Protocol(
                ProtocolRelationalFact::DkgTranscriptCommit(commit),
            )) = &fact.content
            else {
                continue;
            };

            if commit.context != context_id {
                continue;
            }

            match &latest {
                Some(existing) if existing.epoch >= commit.epoch => {}
                _ => latest = Some(commit.clone()),
            }
        }
        Ok(latest)
    }

    /// Default crypto seed for deterministic testing.
    /// Uses a fixed seed to ensure reproducible FROST key generation and crypto operations.
    const TEST_CRYPTO_SEED: [u8; 32] = [42u8; 32];

    /// Create new effect system with configuration (testing mode).
    pub fn new(
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Result<Self, crate::core::AgentError> {
        let config = Self::normalize_test_config(config)?;
        let composite = CompositeHandlerAdapter::for_testing(config.device_id());
        Self::build_internal(
            config,
            composite,
            ExecutionMode::Testing,
            Some(Self::TEST_CRYPTO_SEED),
            None, // No shared transport
            None, // No shared inbox
            authority_id,
            false,
        )
    }

    /// Create effect system for production.
    pub fn production(
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Result<Self, crate::core::AgentError> {
        let mut composite = CompositeHandlerAdapter::for_production(config.device_id());
        composite
            .composite_mut()
            .register_all(RegisterAllOptions::allow_impure())
            .map_err(|e| crate::core::AgentError::effects(e.to_string()))?;
        // Production uses OS entropy, no seed
        Self::build_internal(
            config,
            composite,
            ExecutionMode::Production,
            None,
            None,
            None,
            authority_id,
            false,
        )
    }

    #[cfg(test)]
    pub(crate) fn production_for_test_for_authority(
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Result<Self, crate::core::AgentError> {
        let mut composite = CompositeHandlerAdapter::for_production(config.device_id());
        composite
            .composite_mut()
            .register_all(RegisterAllOptions::allow_impure())
            .map_err(|e| crate::core::AgentError::effects(e.to_string()))?;
        Self::build_internal(
            config,
            composite,
            ExecutionMode::Production,
            None,
            None,
            None,
            authority_id,
            true,
        )
    }

    fn identity_from_location(location: &Location<'_>) -> String {
        format!(
            "{}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        )
    }

    fn derive_test_seed(identity: &str, extra_salt: u64) -> u64 {
        let seed_material = format!("{TEST_SEED_DERIVATION_DOMAIN}:{identity}:{extra_salt}");
        let digest = aura_hash(seed_material.as_bytes());
        let mut seed_bytes = [0u8; 8];
        seed_bytes.copy_from_slice(&digest[..8]);
        u64::from_le_bytes(seed_bytes)
    }

    fn derive_test_authority(seed: u64) -> AuthorityId {
        let authority_material = format!("{TEST_SEED_DERIVATION_DOMAIN}:authority:{seed}");
        AuthorityId::new_from_entropy(aura_hash(authority_material.as_bytes()))
    }

    fn register_test_seed(
        seed: u64,
        identity: &str,
        location: &Location<'_>,
    ) -> Result<(), crate::core::AgentError> {
        let registry = TEST_SEED_REGISTRY.get_or_init(|| parking_lot::Mutex::new(HashMap::new()));
        let usage = TestSeedUsage {
            identity: identity.to_string(),
            location: Self::identity_from_location(location),
        };
        let mut guard = registry.lock();
        if let Some(existing) = guard.get(&seed) {
            return Err(crate::core::AgentError::effects(format!(
                "duplicate deterministic test seed {} detected (first: {} @ {}, second: {} @ {}). \
                 Use unique test identities or simulation_for_test_with_salt(...) to disambiguate.",
                seed, existing.identity, existing.location, usage.identity, usage.location
            )));
        }
        guard.insert(seed, usage);
        Ok(())
    }

    #[track_caller]
    fn allocate_test_seed_with_identity(
        test_identity: &str,
        extra_salt: u64,
    ) -> Result<u64, crate::core::AgentError> {
        let location = Location::caller();
        let scoped_identity = format!(
            "{}::{}",
            test_identity,
            Self::identity_from_location(location)
        );
        let seed = Self::derive_test_seed(&scoped_identity, extra_salt);
        Self::register_test_seed(seed, &scoped_identity, location)?;
        Ok(seed)
    }

    #[track_caller]
    fn allocate_test_seed(extra_salt: u64) -> Result<u64, crate::core::AgentError> {
        let location = Location::caller();
        let identity = Self::identity_from_location(location);
        let seed = Self::derive_test_seed(&identity, extra_salt);
        Self::register_test_seed(seed, &identity, location)?;
        Ok(seed)
    }

    /// Canonical deterministic constructor for tests.
    ///
    /// Seed derivation is deterministic from callsite location, and duplicate
    /// seeds in-process are rejected to prevent hidden test coupling.
    #[track_caller]
    #[allow(clippy::disallowed_methods)]
    pub fn simulation_for_test(config: &AgentConfig) -> Result<Self, crate::core::AgentError> {
        let seed = Self::allocate_test_seed(0)?;
        Self::simulation(config, seed, Self::derive_test_authority(seed))
    }

    /// Deterministic test constructor with extra salt for multi-instance setups
    /// from the same callsite.
    #[track_caller]
    #[allow(clippy::disallowed_methods)]
    pub fn simulation_for_test_with_salt(
        config: &AgentConfig,
        extra_salt: u64,
    ) -> Result<Self, crate::core::AgentError> {
        let seed = Self::allocate_test_seed(extra_salt)?;
        Self::simulation(config, seed, Self::derive_test_authority(seed))
    }

    /// Deterministic test constructor using explicit test identity plus callsite.
    #[track_caller]
    #[allow(clippy::disallowed_methods)]
    pub fn simulation_for_named_test(
        config: &AgentConfig,
        test_identity: &str,
    ) -> Result<Self, crate::core::AgentError> {
        let seed = Self::allocate_test_seed_with_identity(test_identity, 0)?;
        Self::simulation(config, seed, Self::derive_test_authority(seed))
    }

    /// Deterministic test constructor with explicit test identity and salt.
    #[track_caller]
    #[allow(clippy::disallowed_methods)]
    pub fn simulation_for_named_test_with_salt(
        config: &AgentConfig,
        test_identity: &str,
        extra_salt: u64,
    ) -> Result<Self, crate::core::AgentError> {
        let seed = Self::allocate_test_seed_with_identity(test_identity, extra_salt)?;
        Self::simulation(config, seed, Self::derive_test_authority(seed))
    }

    /// Deterministic authority-aware constructor for tests.
    #[track_caller]
    #[allow(clippy::disallowed_methods)]
    pub fn simulation_for_test_for_authority(
        config: &AgentConfig,
        authority_id: AuthorityId,
    ) -> Result<Self, crate::core::AgentError> {
        let seed = Self::allocate_test_seed(0)?;
        Self::simulation_for_authority(config, seed, authority_id)
    }

    /// Deterministic authority-aware constructor for tests with salt.
    #[track_caller]
    #[allow(clippy::disallowed_methods)]
    pub fn simulation_for_test_for_authority_with_salt(
        config: &AgentConfig,
        authority_id: AuthorityId,
        extra_salt: u64,
    ) -> Result<Self, crate::core::AgentError> {
        let seed = Self::allocate_test_seed(extra_salt)?;
        Self::simulation_for_authority(config, seed, authority_id)
    }

    /// Deterministic shared-transport constructor for tests.
    #[track_caller]
    #[allow(clippy::disallowed_methods)]
    pub fn simulation_for_test_with_shared_transport(
        config: &AgentConfig,
        shared_transport: SharedTransport,
    ) -> Result<Self, crate::core::AgentError> {
        let seed = Self::allocate_test_seed(0)?;
        Self::simulation_with_shared_transport(
            config,
            seed,
            Self::derive_test_authority(seed),
            shared_transport,
        )
    }

    /// Deterministic shared-transport constructor for tests with explicit authority.
    #[track_caller]
    #[allow(clippy::disallowed_methods)]
    pub fn simulation_for_test_with_shared_transport_for_authority(
        config: &AgentConfig,
        authority_id: AuthorityId,
        shared_transport: SharedTransport,
    ) -> Result<Self, crate::core::AgentError> {
        let seed = Self::allocate_test_seed(0)?;
        Self::simulation_with_shared_transport_for_authority(
            config,
            seed,
            authority_id,
            shared_transport,
        )
    }

    /// Deterministic shared-transport constructor with an explicit test
    /// identity, for helpers that build several instances from one callsite.
    #[track_caller]
    #[allow(clippy::disallowed_methods)]
    pub fn simulation_for_named_test_with_shared_transport_for_authority(
        config: &AgentConfig,
        test_identity: &str,
        authority_id: AuthorityId,
        shared_transport: SharedTransport,
    ) -> Result<Self, crate::core::AgentError> {
        let seed = Self::allocate_test_seed_with_identity(test_identity, 0)?;
        Self::simulation_with_shared_transport_for_authority(
            config,
            seed,
            authority_id,
            shared_transport,
        )
    }

    /// Retains the actual isolated lease before selected-provider construction.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "TestingOwnedProfileCapability",
        family = "runtime_helper"
    )]
    pub(crate) fn testing_with_owned_profile(
        config: &AgentConfig,
        authority_id: AuthorityId,
        shared_transport: Option<SharedTransport>,
        profile: super::builder::TestingOwnedProfileCapability,
        custom: Option<SelectedCustomProviders>,
    ) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_testing(config.device_id());
        Self::build_internal_owned(
            Self::normalize_test_config(config.clone())?,
            composite,
            ExecutionMode::Testing,
            Some(Self::TEST_CRYPTO_SEED),
            shared_transport,
            None,
            authority_id,
            false,
            None,
            custom,
            Some(profile),
        )
    }

    /// Simulation runtime that retains an actual isolated profile lease, so
    /// enrollment and key rotation have original selected secret custody (the
    /// demo and simulation fixtures). Unowned simulation constructors keep
    /// returning typed `MissingSelectedCustody`.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "TestingOwnedProfileCapability",
        family = "runtime_helper"
    )]
    pub(crate) fn simulation_with_owned_profile(
        config: &AgentConfig,
        seed: u64,
        authority_id: AuthorityId,
        shared_transport: Option<SharedTransport>,
        profile: super::builder::TestingOwnedProfileCapability,
    ) -> Result<Self, crate::core::AgentError> {
        let config = Self::normalize_test_config(config.clone())?;
        let composite = CompositeHandlerAdapter::for_simulation(config.device_id(), seed);
        let mut crypto_seed = [0u8; 32];
        crypto_seed[0..8].copy_from_slice(&seed.to_le_bytes());
        Self::build_internal_owned(
            config,
            composite,
            ExecutionMode::Simulation { seed },
            Some(crypto_seed),
            shared_transport,
            None,
            authority_id,
            false,
            None,
            None,
            Some(profile),
        )
    }

    /// Normalize a nonproduction configuration before acquiring its profile
    /// lease, so the lease and the runtime select the same directory.
    pub(crate) fn normalized_nonproduction_config(
        config: AgentConfig,
    ) -> Result<AgentConfig, crate::core::AgentError> {
        Self::normalize_test_config(config)
    }

    /// Create effect system for testing with default configuration.
    ///
    /// Prefer `simulation_for_test(...)` for deterministic per-test seeding.
    pub fn testing(
        config: &AgentConfig,
        authority_id: AuthorityId,
    ) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_testing(config.device_id());
        Self::build_internal(
            Self::normalize_test_config(config.clone())?,
            composite,
            ExecutionMode::Testing,
            Some(Self::TEST_CRYPTO_SEED),
            None, // No shared transport
            None, // No shared inbox
            authority_id,
            false,
        )
    }

    /// Create effect system for testing with shared transport.
    ///
    /// This factory is used for tests that need to verify transport envelope routing,
    /// enabling loopback testing where an agent can send and receive messages from itself.
    pub fn testing_with_shared_transport(
        config: &AgentConfig,
        authority_id: AuthorityId,
        shared_transport: SharedTransport,
    ) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_testing(config.device_id());
        Self::build_internal(
            Self::normalize_test_config(config.clone())?,
            composite,
            ExecutionMode::Testing,
            Some(Self::TEST_CRYPTO_SEED),
            Some(shared_transport),
            None, // No shared inbox
            authority_id,
            false,
        )
    }

    /// Create effect system for simulation with controlled seed.
    pub fn simulation(
        config: &AgentConfig,
        seed: u64,
        authority_id: AuthorityId,
    ) -> Result<Self, crate::core::AgentError> {
        let config = Self::normalize_test_config(config.clone())?;
        let composite = CompositeHandlerAdapter::for_simulation(config.device_id(), seed);
        // Convert u64 seed to [u8; 32] for crypto handler
        let mut crypto_seed = [0u8; 32];
        crypto_seed[0..8].copy_from_slice(&seed.to_le_bytes());
        Self::build_internal(
            config,
            composite,
            ExecutionMode::Simulation { seed },
            Some(crypto_seed),
            None, // No shared transport
            None, // No shared inbox
            authority_id,
            false,
        )
    }

    /// Create effect system for simulation with shared transport.
    ///
    /// This factory is used for multi-agent simulations where all agents need to
    /// communicate through a shared transport layer. The shared transport enables
    /// message routing between Bob, Alice, and Carol in demo mode.
    pub fn simulation_with_shared_transport(
        config: &AgentConfig,
        seed: u64,
        authority_id: AuthorityId,
        shared_transport: SharedTransport,
    ) -> Result<Self, crate::core::AgentError> {
        let config = Self::normalize_test_config(config.clone())?;
        let composite = CompositeHandlerAdapter::for_simulation(config.device_id(), seed);
        // Convert u64 seed to [u8; 32] for crypto handler
        let mut crypto_seed = [0u8; 32];
        crypto_seed[0..8].copy_from_slice(&seed.to_le_bytes());
        Self::build_internal(
            config,
            composite,
            ExecutionMode::Simulation { seed },
            Some(crypto_seed),
            Some(shared_transport),
            None, // No shared inbox
            authority_id,
            false,
        )
    }

    /// Create effect system for simulation with a shared inbox.
    ///
    /// This variant matches the aura-core simulation factory contract and uses
    /// a single shared inbox for all agents. Receivers filter by destination.
    pub fn simulation_with_shared_inbox(
        config: &AgentConfig,
        seed: u64,
        authority_id: AuthorityId,
        shared_inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    ) -> Result<Self, crate::core::AgentError> {
        let config = Self::normalize_test_config(config.clone())?;
        let composite = CompositeHandlerAdapter::for_simulation(config.device_id(), seed);
        let mut crypto_seed = [0u8; 32];
        crypto_seed[0..8].copy_from_slice(&seed.to_le_bytes());
        Self::build_internal(
            config,
            composite,
            ExecutionMode::Simulation { seed },
            Some(crypto_seed),
            None, // No shared transport
            Some(shared_inbox),
            authority_id,
            false,
        )
    }

    /// Create effect system for production, overriding the authority identity.
    pub fn production_for_authority(
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Result<Self, crate::core::AgentError> {
        Self::production(config, authority_id)
    }

    /// Assemble selected handlers before any persistent handler or service can retain defaults.
    pub(crate) fn custom_for_authority(
        config: AgentConfig,
        authority: AuthorityId,
        mode: ExecutionMode,
        providers: SelectedCustomProviders,
        owner: Option<Arc<aura_effects::profile_storage::OwnedProfileLease>>,
        shared: Option<SharedTransport>,
    ) -> Result<Self, crate::core::AgentError> {
        if providers.transports.len() > 16 {
            return Err(crate::core::AgentError::config(
                "custom transport inventory exceeds 16 providers",
            ));
        }
        let config = if mode.is_production() {
            config
        } else {
            Self::normalize_test_config(config)?
        };
        let mut composite = match mode {
            ExecutionMode::Production => {
                CompositeHandlerAdapter::for_production(config.device_id())
            }
            ExecutionMode::Testing => CompositeHandlerAdapter::for_testing(config.device_id()),
            ExecutionMode::Simulation { seed } => {
                CompositeHandlerAdapter::for_simulation(config.device_id(), seed)
            }
        };
        if mode.is_production() {
            composite
                .composite_mut()
                .register_all(RegisterAllOptions::allow_impure())
                .map_err(|source| {
                    crate::core::AgentError::from(AuraError::Internal {
                        message: "assemble selected custom providers".into(),
                        source: Some(Arc::new(source)),
                    })
                })?;
        }
        Self::build_internal_owned(
            config,
            composite,
            mode,
            None,
            shared,
            None,
            authority,
            false,
            owner,
            Some(providers),
            None,
        )
    }

    /// Owned production assembly accepts only the concrete audited adapter token.
    /// Retain the exact concrete preassembly lease across bootstrap/runtime owners.
    /// A caller-provided boxed core lease cannot manufacture this provider resource.
    pub(crate) fn production_for_authority_shared_profile(
        config: AgentConfig,
        authority_id: AuthorityId,
        owner: Arc<aura_effects::profile_storage::OwnedProfileLease>,
    ) -> Result<Self, crate::core::AgentError> {
        let mut composite = CompositeHandlerAdapter::for_production(config.device_id());
        composite
            .composite_mut()
            .register_all(RegisterAllOptions::allow_impure())
            .map_err(|source| {
                crate::core::AgentError::from(AuraError::Internal {
                    message: "assemble owned production effect handlers".into(),
                    source: Some(Arc::new(source)),
                })
            })?;
        Self::build_internal_owned(
            config,
            composite,
            ExecutionMode::Production,
            None,
            None,
            None,
            authority_id,
            false,
            Some(owner),
            None,
            None,
        )
    }

    /// Create effect system for testing, overriding the authority identity.
    ///
    /// Prefer `simulation_for_test_for_authority(...)` for deterministic per-test seeding.
    #[allow(clippy::disallowed_methods)]
    pub fn testing_for_authority(
        config: &AgentConfig,
        authority_id: AuthorityId,
    ) -> Result<Self, crate::core::AgentError> {
        Self::testing(config, authority_id)
    }

    /// Create effect system for simulation, overriding the authority identity.
    #[allow(clippy::disallowed_methods)]
    pub fn simulation_for_authority(
        config: &AgentConfig,
        seed: u64,
        authority_id: AuthorityId,
    ) -> Result<Self, crate::core::AgentError> {
        Self::simulation(config, seed, authority_id)
    }

    /// Create effect system for simulation with shared transport, overriding authority.
    pub fn simulation_with_shared_transport_for_authority(
        config: &AgentConfig,
        seed: u64,
        authority_id: AuthorityId,
        shared_transport: SharedTransport,
    ) -> Result<Self, crate::core::AgentError> {
        Self::simulation_with_shared_transport(config, seed, authority_id, shared_transport)
    }

    /// Create effect system for simulation with a shared inbox, overriding authority.
    pub fn simulation_with_shared_inbox_for_authority(
        config: &AgentConfig,
        seed: u64,
        authority_id: AuthorityId,
        shared_inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    ) -> Result<Self, crate::core::AgentError> {
        Self::simulation_with_shared_inbox(config, seed, authority_id, shared_inbox)
    }

    /// Get configuration
    pub(crate) async fn lock_enrollment_manifest_admission(
        &self,
    ) -> tokio::sync::MutexGuard<'_, ()> {
        self.enrollment_manifest_admission_gate.lock().await
    }

    /// Serialize account projection decisions with sealed enrollment handoff.
    /// The production adapters additionally retain their cross-process profile lease.
    pub(crate) async fn enrollment_profile_handoff_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.enrollment_profile_handoff_gate.lock().await
    }

    /// The authority actually selected by this runtime's construction owner.
    pub(crate) fn runtime_authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// Get composite handler
    pub fn composite(&self) -> &CompositeHandlerAdapter {
        &self.composite
    }

    /// Get access to time effects
    /// Configure the physical-time owner before runtime service assembly.
    pub(crate) fn with_physical_time_provider(
        mut self,
        provider: Arc<dyn aura_core::effects::PhysicalTimeEffects>,
    ) -> Self {
        self.time_handler = EnhancedTimeHandler::with_provider(provider);
        self
    }

    /// Corrupt the encrypted backing bytes of an immutable record (fixtures only;
    /// the API never mutates immutable records).
    #[cfg(test)]
    pub(crate) async fn fault_corrupt_secure_record_for_test(
        &self,
        location: &aura_core::effects::SecureStorageLocation,
    ) -> Result<(), aura_core::AuraError> {
        self.crypto
            .secure_storage()
            .fault_corrupt_selected_record_for_test(location)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn fault_remove_secure_record_for_test(
        &self,
        location: &aura_core::effects::SecureStorageLocation,
    ) -> Result<bool, aura_core::AuraError> {
        self.crypto
            .secure_storage()
            .fault_remove_selected_record_for_test(location)
            .await
    }

    pub fn time_effects(&self) -> &EnhancedTimeHandler {
        &self.time_handler
    }

    /// Get the fact registry for domain-specific fact reduction.
    pub fn fact_registry(&self) -> Arc<FactRegistry> {
        self.journal.fact_registry()
    }

    /// Get the indexed journal handler for efficient fact lookups.
    ///
    /// Provides O(log n) B-tree indexed lookups, O(1) Bloom filter membership tests,
    /// and Merkle tree integrity verification.
    pub fn indexed_journal(&self) -> Arc<IndexedJournalHandler> {
        self.journal.indexed_journal()
    }

    /// Build a permissive Biscuit policy/bridge pair for journal enforcement.
    fn init_journal_policy(
        authority_id: AuthorityId,
    ) -> ((Biscuit, BiscuitAuthorizationBridge), Vec<u8>) {
        let issuer = aura_authorization::TokenAuthority::new(authority_id);
        let token = issuer
            .create_token(
                authority_id,
                crate::token_profiles::TokenCapabilityProfile::StandardDevice,
            )
            .expect("journal authorization policy token creation must succeed");
        let bridge = BiscuitAuthorizationBridge::new(issuer.root_public_key(), authority_id);
        let verifying_key = issuer.root_public_key().to_bytes().to_vec();
        ((token, bridge), verifying_key)
    }

    /// Build the Biscuit-backed authorization handler.
    fn init_authorization_handler(
        authority: AuthorityId,
        crypto_handler: &Arc<dyn CryptoEffects>,
        verifying_key: &[u8],
        time_handler: &PhysicalTimeHandler,
        execution_mode: ExecutionMode,
        harness_mode_enabled: bool,
    ) -> aura_authorization::effects::WotAuthorizationHandler<Arc<dyn CryptoEffects>> {
        let runtime_config = AuthorizationRuntimeConfig::from_verifying_key(
            authority,
            execution_mode,
            harness_mode_enabled,
            verifying_key,
        )
        .expect("Biscuit authorization requires a valid configured root public key");
        tracing::debug!(
            authorization_mode = ?runtime_config.mode,
            authority = %runtime_config.authority_id,
            "Initialized Biscuit authorization runtime"
        );
        let handler = aura_authorization::effects::WotAuthorizationHandler::new(
            crypto_handler.clone(),
            runtime_config.root_public_key,
            runtime_config.authority_id,
        );
        let time_handler = time_handler.clone();
        handler.with_time_provider(Arc::new(move || time_handler.physical_time_now_ms() / 1000))
    }

    /// Construct a journal handler with current policy hooks.
    fn journal_handler(&self) -> RuntimeJournalHandler {
        let (token, bridge) = self
            .journal
            .journal_policy()
            .expect("journal handler requires a Biscuit authorization policy");
        let authorization = (
            token
                .to_vec()
                .expect("journal authorization policy token must serialize"),
            JournalBiscuitAuthorizationHandler {
                bridge: BiscuitAuthorizationBridge::new(
                    bridge.root_public_key(),
                    bridge.authority_id(),
                ),
                time_handler: PhysicalTimeHandler::new(),
            },
        );

        let handler = aura_journal::JournalHandlerFactory::create(
            self.authority_id,
            self.crypto.handler().clone(),
            self.storage_handler.clone(),
            authorization,
            Some(
                self.journal
                    .journal_verifying_key()
                    .expect("journal handler requires a verifying key")
                    .to_vec(),
            ),
            None, // Fact registry is accessed via AuraEffectSystem::fact_registry() instead
        );
        if self.execution_mode.is_deterministic() {
            #[cfg(feature = "simulation")]
            {
                return handler.with_unsigned_receipt_bypass_for_simulation(
                    aura_journal::effects::UnsignedReceiptBypassToken::for_simulation(
                        "deterministic runtime accepts unsigned simulation receipts explicitly",
                    ),
                );
            }
        }
        handler
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{SecureStorageBackend, StorageConfig};
    use crate::runtime::services::threshold_signing::ThresholdSigningService;
    use aura_core::effects::{FlowBudgetEffects, ThresholdSigningEffects};
    use aura_core::types::identifiers::ContextId;
    use aura_guards::GuardContextProvider;
    use aura_protocol::amp::AmpJournalEffects;
    use aura_protocol::effects::SyncEffects;
    use aura_protocol::effects::TreeEffects;

    #[tokio::test]
    async fn simulation_constructor_preserves_seeded_stream_across_actual_subsystem_clones(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use aura_core::effects::RandomCoreEffects;
        use rand::{rngs::StdRng, RngCore, SeedableRng};

        let temporary = tempfile::tempdir()?;
        let mut first_draws = Vec::new();
        for salt in [0x31, 0x32] {
            let config = AgentConfig {
                storage: StorageConfig {
                    base_path: temporary.path().join(format!("simulation-{salt}")),
                    ..Default::default()
                },
                ..Default::default()
            };
            let effects = AuraEffectSystem::simulation_for_named_test_with_salt(
                &config,
                "actual constructor entropy continuation",
                salt,
            )?;
            let ExecutionMode::Simulation { seed } = effects.execution_mode else {
                panic!("simulation constructor must retain its actual mode");
            };
            let mut seed_bytes = [0; 32];
            seed_bytes[..8].copy_from_slice(&seed.to_le_bytes());
            let mut reference = StdRng::from_seed(seed_bytes);
            // Construction consumes the actual receipt signing key first.
            let mut receipt_key_draw = [0; 32];
            reference.fill_bytes(&mut receipt_key_draw);
            let cloned_crypto = effects.crypto.clone();
            let mut draws = Vec::new();
            for (index, length) in [17, 29, 11, 43].into_iter().enumerate() {
                let actual = if index % 2 == 0 {
                    RandomCoreEffects::random_bytes(&effects, length).await
                } else {
                    cloned_crypto.random_bytes(length)
                };
                let mut expected = vec![0; length];
                reference.fill_bytes(&mut expected);
                assert_eq!(
                    actual, expected,
                    "the constructed owner must share one stream"
                );
                draws.push(actual);
            }
            // Replaying the independently seeded reference reproduces every draw.
            let mut replay = StdRng::from_seed(seed_bytes);
            replay.fill_bytes(&mut receipt_key_draw);
            for actual in &draws {
                let mut expected = vec![0; actual.len()];
                replay.fill_bytes(&mut expected);
                assert_eq!(actual, &expected);
            }
            first_draws.push(draws.remove(0));
        }
        assert_ne!(first_draws[0], first_draws[1]);
        Ok(())
    }

    #[tokio::test]
    async fn seeded_simulation_constructor_retains_configured_crypto_and_random_dispatch(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use aura_core::effects::{CryptoCoreEffects, RandomCoreEffects};
        use aura_testkit::stateful_effects::custom_provider::{
            CustomCryptoProbe, CustomProviderOutage, CustomProviderProbe,
        };
        use std::error::Error;
        let temporary = tempfile::tempdir()?;
        let config = AgentConfig {
            storage: StorageConfig {
                base_path: temporary.path().join("configured-simulation"),
                ..Default::default()
            },
            ..Default::default()
        };
        let crypto = Arc::new(CustomCryptoProbe::default());
        let probe = Arc::new(CustomProviderProbe::default());
        let effects = AuraEffectSystem::build_internal_owned(
            config.clone(),
            CompositeHandlerAdapter::for_simulation(config.device_id(), 0x58),
            ExecutionMode::Simulation { seed: 0x58 },
            Some([0x58; 32]),
            None,
            None,
            AuthorityId::new_from_entropy([0x59; 32]),
            false,
            None,
            Some(SelectedCustomProviders {
                crypto: crypto.clone(),
                storage: probe.clone(),
                random: probe.clone(),
                console: probe.clone(),
                transports: vec![probe.clone()],
            }),
            None,
        )?;
        assert!(!effects.crypto.handler().is_simulated());
        assert!(effects
            .crypto
            .handler()
            .crypto_capabilities()
            .iter()
            .any(|name| name == "configured-custom-probe"));
        let initial_draws = probe.random_draws();
        assert_eq!(
            RandomCoreEffects::random_bytes(&effects, 13).await,
            vec![0x93; 13]
        );
        assert_eq!(
            RandomCoreEffects::random_bytes_32(&effects).await,
            [0x93; 32]
        );
        assert_eq!(probe.random_draws(), initial_draws + 2);
        crypto.set_fault(true);
        let error = CryptoCoreEffects::kdf_derive(&effects, &[0x61; 32], b"salt", b"context", 32)
            .await
            .expect_err("actual configured outage must survive seeded construction");
        let mut source: Option<&dyn Error> = Some(&error);
        let mut retained = false;
        while let Some(actual) = source {
            retained |= actual.is::<CustomProviderOutage>();
            source = actual.source();
        }
        assert!(retained);
        Ok(())
    }

    #[tokio::test]
    async fn production_seed_rejection_precedes_profile_io_with_custom_real_crypto(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::error::Error;
        let temporary = tempfile::tempdir()?;
        let profile = temporary.path().join("seeded-production-must-stay-absent");
        let config = AgentConfig {
            storage: StorageConfig {
                base_path: profile.clone(),
                ..Default::default()
            },
            ..Default::default()
        };
        let probe = Arc::new(
            aura_testkit::stateful_effects::custom_provider::CustomProviderProbe::default(),
        );
        let crypto: Arc<dyn CryptoEffects> = Arc::new(RealCryptoHandler::new());
        assert!(!crypto.is_simulated());
        let composite = CompositeHandlerAdapter::for_production(config.device_id());
        let result = AuraEffectSystem::build_internal_owned(
            config,
            composite,
            ExecutionMode::Production,
            Some([0x74; 32]),
            None,
            None,
            AuthorityId::new_from_entropy([0x75; 32]),
            false,
            None,
            Some(SelectedCustomProviders {
                crypto,
                storage: probe.clone(),
                random: probe.clone(),
                console: probe.clone(),
                transports: vec![probe.clone()],
            }),
            None,
        );
        let error = result
            .err()
            .ok_or("seeded production assembly was admitted")?;
        let mut source: Option<&dyn Error> = Some(&error);
        let mut retained = false;
        while let Some(actual) = source {
            retained |= actual.is::<super::super::entropy::ProductionSeededEntropyError>();
            source = actual.source();
        }
        assert!(retained);
        assert!(!profile.exists());
        assert_eq!(probe.random_draws(), 0);
        assert_eq!(probe.console_calls(), 0);
        assert_eq!(probe.sends(), 0);
        assert!(probe.stored_bytes().await.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn required_biscuit_hydration_distinguishes_absence_corruption_and_valid_restore() {
        use aura_core::effects::secure::{SecureStorageCapability, SecureStorageLocation};
        use aura_core::effects::SecureStorageEffects;
        use std::error::Error;
        let authority = AuthorityId::new_from_entropy([241; 32]);
        let profile = tempfile::tempdir().expect("isolated authorization profile");
        let config = crate::AgentConfig {
            storage: StorageConfig {
                base_path: profile.path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let context = aura_core::context::EffectContext::new(
            authority,
            ContextId::new_from_entropy([242; 32]),
            ExecutionMode::Testing,
        );
        let agent = crate::AgentBuilder::new()
            .with_authority(authority)
            .with_config(config.clone())
            .build_testing_async(&context)
            .await
            .expect("genuine fresh runtime permits confirmed absence");
        let effects = agent.runtime().effects();
        effects.clear_biscuit_cache();
        effects
            .initialize_biscuit_cache()
            .await
            .expect("confirmed absence");
        assert!(effects.biscuit_cache().is_none());
        let location = SecureStorageLocation::biscuit_authority(&authority);
        effects
            .secure_store(&location, &[1; 32], &[SecureStorageCapability::Write])
            .await
            .expect("publish malformed mutable authorization record");
        let truncated = effects
            .initialize_biscuit_cache()
            .await
            .expect_err("persisted truncation cannot mean new account");
        assert!(matches!(
            truncated
                .source()
                .and_then(|source| source.downcast_ref::<BiscuitStartupRecordError>()),
            Some(BiscuitStartupRecordError::Truncated(32))
        ));
        assert!(effects.biscuit_cache().is_none());
        effects
            .bootstrap_biscuit_tokens(&authority)
            .await
            .expect("real token producer");
        let original = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .expect("retain actual token bytes");
        effects.clear_biscuit_cache();
        effects
            .initialize_biscuit_cache()
            .await
            .expect("verify valid retained token");
        assert!(effects
            .verified_biscuit_frontier()
            .expect("verified restored frontier")
            .is_some());
        effects.clear_biscuit_cache();
        let mut corrupt = original;
        corrupt.truncate(33);
        effects
            .secure_store(&location, &corrupt, &[SecureStorageCapability::Write])
            .await
            .expect("actual encoded token corruption");
        let failure = effects
            .initialize_biscuit_cache()
            .await
            .expect_err("malformed token cannot publish a cache");
        assert!(matches!(failure, AuraError::Crypto { .. }));
        assert!(failure.source().is_some());
        assert!(effects.biscuit_cache().is_none());
        // An owned profile stays leased until its runtime shuts down.
        drop(effects);
        agent
            .shutdown(&context)
            .await
            .expect("acknowledge original runtime shutdown before reopen");
        let returning = crate::AgentBuilder::new()
            .with_authority(authority)
            .with_config(config)
            .build_testing_async(&context)
            .await
            .err()
            .expect("returning builder cannot admit corrupt persisted authorization");
        let mut cause: Option<&(dyn Error + 'static)> = Some(&returning);
        let mut native_crypto = false;
        while let Some(error) = cause {
            if matches!(
                error.downcast_ref::<AuraError>(),
                Some(AuraError::Crypto { .. })
            ) {
                native_crypto = true;
            }
            cause = error.source();
        }
        assert!(
            native_crypto,
            "returning builder retains actual verification cause"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn required_biscuit_hydration_retains_actual_secure_decryption_failure() {
        use aura_core::effects::secure::SecureStorageLocation;
        use std::error::Error;
        let authority = AuthorityId::new_from_entropy([243; 32]);
        let profile = tempfile::tempdir().expect("isolated backing fault profile");
        let config = crate::AgentConfig {
            storage: StorageConfig {
                base_path: profile.path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let context = aura_core::context::EffectContext::new(
            authority,
            ContextId::new_from_entropy([244; 32]),
            ExecutionMode::Testing,
        );
        let agent = crate::AgentBuilder::new()
            .with_authority(authority)
            .with_config(config)
            .build_testing_async(&context)
            .await
            .expect("actual runtime");
        let effects = agent.runtime().effects();
        effects
            .bootstrap_biscuit_tokens(&authority)
            .await
            .expect("actual persisted token");
        effects.clear_biscuit_cache();
        effects
            .crypto
            .secure_storage()
            .fault_corrupt_selected_record_for_test(&SecureStorageLocation::biscuit_authority(
                &authority,
            ))
            .await
            .expect("mutate selected actual encrypted backing bytes");
        let failure = effects
            .initialize_biscuit_cache()
            .await
            .expect_err("authenticated decryption failure cannot mean credential absence");
        assert!(matches!(failure, AuraError::Storage { .. }));
        let mut cause: Option<&(dyn Error + 'static)> = Some(&failure);
        let mut native_aead = false;
        while let Some(error) = cause {
            native_aead |= error
                .downcast_ref::<chacha20poly1305::aead::Error>()
                .is_some();
            cause = error.source();
        }
        assert!(
            native_aead,
            "retain actual secure-provider authentication cause"
        );
        assert!(effects.biscuit_cache().is_none());
    }

    #[test]
    fn required_parent_metadata_keeps_injected_io_failure_as_storage() {
        use std::error::Error;
        let error = AuraEffectSystem::decode_required_threshold_metadata(Err(AuraError::Storage {
            message: "injected metadata read fault".into(),
            source: Some(std::sync::Arc::new(std::io::Error::from(
                std::io::ErrorKind::PermissionDenied,
            ))),
        }))
        .expect_err("storage failure must not become absent metadata");
        assert!(matches!(error, AuraError::Storage { .. }));
        assert_eq!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<std::io::Error>())
                .map(std::io::Error::kind),
            Some(std::io::ErrorKind::PermissionDenied)
        );
    }

    #[tokio::test]
    async fn actual_retained_parent_metadata_rejects_corruption_and_duplicate_roster() {
        use std::error::Error;
        let (issuer, _invitee, invitation, _start, _accept, _proof) = Box::pin(
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                "parent-metadata-faults",
            ),
        )
        .await;
        let effects = issuer.runtime().effects();
        let retained =
            crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl::load(
                effects.clone(),
                &invitation,
            )
            .await
            .expect("actual retained signed issuer manifest");
        let authority = retained.manifest().subject;
        let epoch = retained.manifest().starting_epoch;
        let location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            authority.to_string(),
            epoch.to_string(),
        );
        let caps = [
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        let original = effects
            .secure_retrieve(&location, &caps)
            .await
            .expect("actual retained parent policy");
        effects
            .require_threshold_config_metadata(&authority, epoch)
            .await
            .expect("original policy valid");
        effects
            .secure_store(&location, b"{", &caps)
            .await
            .expect("inject stored codec corruption");
        let error = effects
            .require_threshold_config_metadata(&authority, epoch)
            .await
            .expect_err("corrupt secure policy fails");
        assert!(matches!(error, AuraError::Serialization { .. }));
        assert!(error
            .source()
            .and_then(|source| source.downcast_ref::<serde_json::Error>())
            .is_some());
        let mut duplicate: ThresholdConfigMetadata =
            serde_json::from_slice(&original).expect("original exact policy");
        duplicate
            .participants
            .push(duplicate.participants[0].clone());
        duplicate.total_n += 1;
        let duplicate = serde_json::to_vec(&duplicate).expect("encode actual duplicated roster");
        effects
            .secure_store(&location, &duplicate, &caps)
            .await
            .expect("inject duplicate roster");
        let error = effects
            .require_threshold_config_metadata(&authority, epoch)
            .await
            .expect_err("duplicate roster cannot supply threshold cardinality");
        assert!(matches!(error, AuraError::Crypto { .. }));
        effects
            .secure_store(&location, &original, &caps)
            .await
            .expect("restore exact secure policy");
        effects
            .require_threshold_config_metadata(&authority, epoch)
            .await
            .expect("original policy remains valid");
    }

    #[test]
    fn authorization_runtime_config_rejects_missing_or_invalid_root_keys() {
        let authority = AuthorityId::new_from_entropy([0xA5; 32]);

        assert!(AuthorizationRuntimeConfig::from_verifying_key(
            authority,
            ExecutionMode::Production,
            false,
            &[],
        )
        .is_err());
        assert!(AuthorizationRuntimeConfig::from_verifying_key(
            authority,
            ExecutionMode::Production,
            false,
            &[0; 32],
        )
        .is_err());
        assert!(AuthorizationRuntimeConfig::from_verifying_key(
            authority,
            ExecutionMode::Production,
            false,
            &[1, 2, 3],
        )
        .is_err());
    }

    #[test]
    fn authorization_runtime_config_records_runtime_mode() {
        let authority = AuthorityId::new_from_entropy([0xA6; 32]);
        let issuer = aura_authorization::TokenAuthority::new(authority);
        let root_key = issuer.root_public_key().to_bytes();

        let production = AuthorizationRuntimeConfig::from_verifying_key(
            authority,
            ExecutionMode::Production,
            false,
            &root_key,
        )
        .expect("production root key should parse");
        assert_eq!(production.mode, AuthorizationRuntimeMode::Production);

        let harness = AuthorizationRuntimeConfig::from_verifying_key(
            authority,
            ExecutionMode::Testing,
            true,
            &root_key,
        )
        .expect("harness root key should parse");
        assert_eq!(harness.mode, AuthorizationRuntimeMode::Harness);

        let simulation = AuthorizationRuntimeConfig::from_verifying_key(
            authority,
            ExecutionMode::Simulation { seed: 7 },
            false,
            &root_key,
        )
        .expect("simulation root key should parse");
        assert_eq!(simulation.mode, AuthorizationRuntimeMode::Simulation);
    }

    #[tokio::test]
    async fn commit_domain_fact_stores_the_fact_schema_version_and_decodes() {
        use aura_journal::DomainFact;
        let authority_id = AuthorityId::new_from_entropy([0xD1; 32]);
        let context = ContextId::new_from_entropy([0xD2; 32]);
        let effects = AuraEffectSystem::simulation_for_test_for_authority(
            &AgentConfig::default(),
            authority_id,
        )
        .expect("effect system should build");
        let fact = aura_social::SocialFact::home_created_ms(
            aura_social::HomeId::from_bytes([0xD3; 32]),
            context,
            1,
            authority_id,
            "schema-home".to_string(),
        );
        let declared = fact.to_envelope().schema_version;
        assert!(declared > 1, "fixture must sit above schema 1");

        effects
            .commit_domain_fact(context, &fact)
            .await
            .expect("domain fact commits");

        let stored = effects
            .load_committed_facts(authority_id)
            .await
            .expect("load committed facts");
        let envelope = stored
            .iter()
            .find_map(|typed| match &typed.content {
                FactContent::Relational(RelationalFact::Generic {
                    context_id,
                    envelope,
                }) if *context_id == context => Some(envelope.clone()),
                _ => None,
            })
            .expect("committed generic fact is stored");
        assert_eq!(envelope.schema_version, declared);
        assert_eq!(
            aura_social::SocialFact::from_envelope(&envelope),
            Some(fact)
        );
    }

    #[tokio::test]
    async fn charge_flow_returns_verifiable_receipt_signature() {
        let authority_id = AuthorityId::new_from_entropy([0xC1; 32]);
        let peer = AuthorityId::new_from_entropy([0xC2; 32]);
        let context = ContextId::new_from_entropy([0xC3; 32]);
        let effects = AuraEffectSystem::simulation_for_test_for_authority(
            &AgentConfig::default(),
            authority_id,
        )
        .expect("effect system should build");

        let receipt =
            FlowBudgetEffects::charge_flow(&effects, &context, &peer, aura_core::FlowCost::new(1))
                .await
                .expect("flow charge should produce a receipt");
        let transport_receipt = aura_core::effects::transport::TransportReceipt {
            context: receipt.ctx,
            src: receipt.src,
            dst: receipt.dst,
            epoch: receipt.epoch.value(),
            cost: receipt.cost.value(),
            nonce: receipt.nonce.value(),
            prev: receipt.prev.0,
            sig: receipt.sig.into_bytes(),
        };

        assert!(
            crate::runtime::receipt_model::verify_transport_flow_receipt(&transport_receipt)
                .is_ok()
        );
    }

    /// Regression (work/8.md task 3): journal anti-entropy must be authorizable
    /// from the runtime Biscuit frontier; without it every sync was denied.
    #[test]
    fn sync_biscuit_authorization_grants_request_digest() {
        let authority_id = AuthorityId::new_from_entropy([0xB5; 32]);
        let effects = AuraEffectSystem::simulation_for_test_for_authority(
            &AgentConfig::default(),
            authority_id,
        )
        .expect("effect system should build");

        let (token_manager, evaluator) = effects
            .sync_biscuit_authorization()
            .expect("frontier should verify")
            .expect("test runtime has a Biscuit frontier");
        let token = aura_authorization::VerifiedBiscuitToken::from_token(
            token_manager.current_token(),
            evaluator.root_public_key(),
        )
        .expect("token should verify");
        let resource = aura_core::types::scope::ResourceScope::Authority {
            authority_id,
            operation: aura_core::types::scope::AuthorityOp::UpdateTree,
        };
        let mut budget = aura_core::FlowBudget::new(1000, aura_core::Epoch::new(0));
        let result = evaluator
            .evaluate_guard(
                &token,
                &aura_sync::capabilities::SyncCapability::RequestDigest.as_name(),
                &resource,
                aura_core::FlowCost::new(100),
                &mut budget,
                1,
            )
            .expect("guard evaluation should run");
        assert!(result.authorized, "sync:request_digest must be granted");
    }

    #[test]
    fn verified_biscuit_frontier_rejects_mismatched_cached_issuer() {
        use base64::Engine;

        let authority_id = AuthorityId::new_from_entropy([0xB1; 32]);
        let effects = AuraEffectSystem::simulation_for_test_for_authority(
            &AgentConfig::default(),
            authority_id,
        )
        .expect("effect system should build");
        let issuer = aura_authorization::TokenAuthority::new(authority_id);
        let token = issuer
            .create_token(
                authority_id,
                crate::token_profiles::TokenCapabilityProfile::StandardDevice,
            )
            .expect("test token should build");
        let engine = base64::engine::general_purpose::STANDARD;
        effects.set_biscuit_cache(BiscuitCache {
            token_b64: engine.encode(token.to_vec().expect("token should serialize")),
            issuer_authority: AuthorityId::new_from_entropy([0xB2; 32]),
            root_pk_b64: engine.encode(issuer.root_public_key().to_bytes()),
        });

        assert!(effects.verified_biscuit_frontier().is_err());
    }

    #[test]
    fn verified_biscuit_frontier_rejects_mismatched_cached_root_key() {
        use base64::Engine;
        use std::error::Error;

        let authority_id = AuthorityId::new_from_entropy([0xB3; 32]);
        let effects = AuraEffectSystem::simulation_for_test_for_authority(
            &AgentConfig::default(),
            authority_id,
        )
        .expect("effect system should build");
        let issuer = aura_authorization::TokenAuthority::new(authority_id);
        let wrong_root = aura_authorization::TokenAuthority::new(authority_id);
        let token = issuer
            .create_token(
                authority_id,
                crate::token_profiles::TokenCapabilityProfile::StandardDevice,
            )
            .expect("test token should build");
        let engine = base64::engine::general_purpose::STANDARD;
        effects.set_biscuit_cache(BiscuitCache {
            token_b64: engine.encode(token.to_vec().expect("token should serialize")),
            issuer_authority: authority_id,
            root_pk_b64: engine.encode(wrong_root.root_public_key().to_bytes()),
        });

        let failure = effects
            .verified_biscuit_frontier()
            .expect_err("actual token signature rejects a different root");
        assert!(matches!(failure, AuraError::Crypto { .. }));
        assert!(
            failure.source().is_some(),
            "retain native verification cause"
        );
    }

    #[test]
    fn production_platform_storage_requires_its_own_namespace_owner() {
        let _env_guard = crate::testing::harness_env_lock().lock_blocking();
        let authority = AuthorityId::new_from_entropy([0xA7; 32]);
        let temp = tempfile::tempdir().expect("tempdir should build");
        let config = AgentConfig {
            storage: StorageConfig {
                base_path: temp.path().join("aura"),
                secure_storage_backend: SecureStorageBackend::PlatformCredentialStore,
                ..Default::default()
            },
            ..Default::default()
        };
        // Platform storage assembles with its own profile and namespace lease;
        // while it lives, no second writer can share that namespace.
        let first = AuraEffectSystem::production(config.clone(), authority)
            .expect("platform production retains its own namespace owner");
        let error = AuraEffectSystem::production(config.clone(), authority)
            .expect_err("a second writer cannot share the owned platform namespace");
        let mut source: &dyn std::error::Error = &error;
        let ownership = loop {
            if let Some(ownership) =
                source.downcast_ref::<aura_core::effects::profile_storage::ProfileStorageError>()
            {
                break ownership;
            }
            source = source.source().expect("typed ownership source retained");
        };
        assert!(matches!(
            ownership,
            aura_core::effects::profile_storage::ProfileStorageError::Busy
        ));
        assert!(!config.storage.base_path.join("secure_store").exists());
        drop(first);
    }

    #[test]
    fn explicit_test_filesystem_production_retains_profile_owner() {
        let authority = AuthorityId::new_from_entropy([0xA7; 32]);
        let temp = tempfile::tempdir().expect("tempdir should build");
        let config = AgentConfig {
            storage: StorageConfig {
                base_path: temp.path().join("aura"),
                secure_storage_backend: SecureStorageBackend::FilesystemFallback,
                ..Default::default()
            },
            ..Default::default()
        };
        let effect_system = AuraEffectSystem::production_for_test_for_authority(config, authority)
            .expect("explicit test filesystem runtime builds");
        match effect_system.crypto.secure_storage().as_ref() {
            ProductionSecureStorageHandler::ProfileOwned(owned) => {
                assert!(owned.uses_filesystem_fallback());
            }
            _ => panic!("production writer must retain actual profile owner"),
        }
    }

    #[test]
    fn production_rejects_filesystem_secure_storage_fallback() {
        let _env_guard = crate::testing::harness_env_lock().lock_blocking();
        let authority = AuthorityId::new_from_entropy([0xA8; 32]);
        let temp = tempfile::tempdir().expect("tempdir should build");
        let config = AgentConfig {
            storage: StorageConfig {
                base_path: temp.path().join("aura"),
                secure_storage_backend: SecureStorageBackend::FilesystemFallback,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = match AuraEffectSystem::production(config, authority) {
            Ok(_) => panic!("production runtime must reject filesystem fallback"),
            Err(error) => error,
        };

        assert!(error
            .to_string()
            .contains("production runtime rejects filesystem secure-storage fallback"));
    }

    #[test]
    fn testing_can_use_filesystem_secure_storage_fallback() {
        let authority = AuthorityId::new_from_entropy([0xA9; 32]);
        let temp = tempfile::tempdir().expect("tempdir should build");
        let config = AgentConfig {
            storage: StorageConfig {
                base_path: temp.path().join("aura"),
                secure_storage_backend: SecureStorageBackend::FilesystemFallback,
                ..Default::default()
            },
            ..Default::default()
        };

        let effect_system = AuraEffectSystem::simulation_for_test_for_authority(&config, authority)
            .expect("simulation runtime builds");

        assert!(matches!(
            effect_system.crypto.secure_storage().as_ref(),
            ProductionSecureStorageHandler::FilesystemFallback(_)
        ));
    }

    #[test]
    fn production_rejects_plaintext_storage_policy() {
        let authority = AuthorityId::new_from_entropy([0xA9; 32]);
        let temp = tempfile::tempdir().expect("tempdir should build");
        let config = AgentConfig {
            storage: StorageConfig {
                base_path: temp.path().join("aura"),
                encryption_policy: crate::core::config::StorageEncryptionPolicy::PlaintextForTests,
                ..Default::default()
            },
            ..Default::default()
        };

        let error = AuraEffectSystem::production(config, authority)
            .expect_err("production runtime must reject plaintext storage policy");

        assert!(matches!(error, crate::core::AgentError::Config(_)));
    }

    #[tokio::test]
    async fn test_frost_integration_through_effect_system() {
        let config = AgentConfig::default();
        let effect_system = crate::testing::simulation_effect_system(&config);

        // Generate 2-of-3 FROST keys through the effect system
        let result = effect_system.frost_generate_keys(2, 3).await;
        assert!(result.is_ok(), "FROST key generation should succeed");

        let key_gen_result = result.unwrap();
        assert_eq!(
            key_gen_result.key_packages.len(),
            3,
            "Should have 3 key packages for 3 signers"
        );
        assert!(
            !key_gen_result.public_key_package.is_empty(),
            "Public key package should not be empty"
        );

        // Generate nonces using the first key package
        let first_key_package = &key_gen_result.key_packages[0];
        let nonces_result = effect_system.frost_generate_nonces(first_key_package).await;
        assert!(
            nonces_result.is_ok(),
            "FROST nonce generation should succeed: {:?}",
            nonces_result.err()
        );

        let nonces = nonces_result.unwrap();
        assert!(!nonces.is_empty(), "Nonces should not be empty");
    }

    #[tokio::test]
    async fn test_frost_seeded_determinism() {
        let identity = "runtime/effects:test_frost_seeded_determinism";
        let seed_a = AuraEffectSystem::derive_test_seed(identity, 0);
        let seed_b = AuraEffectSystem::derive_test_seed(identity, 0);
        let seed_c = AuraEffectSystem::derive_test_seed(identity, 1);
        assert_eq!(seed_a, seed_b, "same identity/salt must be deterministic");
        assert_ne!(
            seed_a, seed_c,
            "different salt must produce a different seed"
        );
    }

    #[tokio::test]
    async fn test_duplicate_seed_registration_is_rejected() {
        let config = AgentConfig::default();
        let mut attempts = Vec::new();
        for _ in 0..2 {
            attempts.push(AuraEffectSystem::simulation_for_named_test_with_salt(
                &config, "dup-seed", 7,
            ));
        }
        assert!(
            attempts[0].is_ok(),
            "first deterministic allocation should succeed"
        );
        assert!(
            attempts[1].is_err(),
            "duplicate deterministic seed must be rejected"
        );
    }

    #[tokio::test]
    async fn test_guard_effect_system_enables_amp_journal_effects() {
        let config = AgentConfig::default();
        let effect_system = crate::testing::simulation_effect_system(&config);

        // Pure guards + EffectInterpreter are used; legacy bridges removed.
        let context = ContextId::new_from_entropy([1u8; 32]);
        let _journal = effect_system.fetch_context_journal(context).await.unwrap();

        // Test that metadata works
        assert!(effect_system.get_metadata("authority_id").is_some());
        assert!(effect_system.get_metadata("execution_mode").is_some());
        assert!(effect_system.get_metadata("device_id").is_some());

        // Test operation permissions
        assert!(effect_system.can_perform_operation("test_operation"));
    }

    #[test]
    fn test_simulation_uses_isolated_storage_for_default_config() {
        let config = AgentConfig::default();
        let effect_system = crate::testing::simulation_effect_system(&config);

        assert_ne!(
            effect_system.config().storage.base_path,
            default_storage_path()
        );
    }

    #[tokio::test]
    async fn test_tree_and_sync_handlers_are_wired() {
        let config = AgentConfig::default();
        let effect_system = crate::testing::simulation_effect_system(&config);

        // Tree state should be retrievable (empty but deterministic)
        let state = effect_system.get_current_state().await.unwrap();
        assert_eq!(state.epoch, aura_core::Epoch::initial()); // fresh tree starts at epoch 0
        let commitment = effect_system.get_current_commitment().await.unwrap();
        // The exact empty-tree commitment is handler-defined; the important
        // invariant is that state and point queries agree before any ops exist.
        assert_eq!(
            state.root_commitment,
            *commitment.as_bytes(),
            "empty tree state should agree with current commitment query"
        );

        // Sync state should be internally consistent even when the simulation
        // storage already contains baseline or previously materialized local ops.
        let digest = effect_system.get_oplog_digest().await.unwrap();
        let missing_from_empty = effect_system
            .get_missing_ops(&aura_protocol::effects::BloomDigest::empty())
            .await
            .unwrap();
        assert_eq!(
            digest.cids.len(),
            missing_from_empty.len(),
            "sync digest should match missing-op projection from an empty remote"
        );
    }

    #[tokio::test]
    async fn test_threshold_queries_use_threshold_config_metadata() {
        let config = AgentConfig::default();
        let effect_system = crate::testing::simulation_effect_system(&config);
        let authority = AuthorityId::new_from_entropy([33u8; 32]);

        effect_system
            .bootstrap_authority(&authority)
            .await
            .expect("bootstrap should persist threshold config metadata");

        let threshold_config = effect_system
            .threshold_config(&authority)
            .await
            .expect("threshold config should be readable");
        assert_eq!(threshold_config.threshold, 1);
        assert_eq!(threshold_config.total_participants, 1);

        let threshold_state = effect_system
            .threshold_state(&authority)
            .await
            .expect("threshold state should be readable");
        assert_eq!(threshold_state.epoch, 0);
        assert_eq!(
            threshold_state.agreement_mode,
            aura_core::threshold::AgreementMode::Provisional
        );
    }

    #[tokio::test]
    async fn test_bootstrapped_authority_signs_and_exposes_public_key_package() {
        let config = AgentConfig::default();
        let authority = AuthorityId::new_from_entropy([34u8; 32]);
        // Signing requires the runtime authority to match the signing authority.
        let effect_system =
            crate::testing::simulation_effect_system_for_authority_arc(&config, authority);

        // The signing service writes the canonical epoch metadata signing reads.
        let bootstrapped_public_key =
            crate::runtime::services::ThresholdSigningService::new(effect_system.clone())
                .bootstrap_authority(&authority)
                .await
                .expect("bootstrap should succeed");

        assert!(effect_system.has_signing_capability(&authority).await);
        assert_eq!(
            effect_system.public_key_package(&authority).await,
            Some(bootstrapped_public_key.clone())
        );

        let signature = effect_system
            .sign(aura_core::threshold::SigningContext {
                authority,
                operation: aura_core::threshold::SignableOperation::Message {
                    domain: "aura.test.threshold-signing".to_string(),
                    payload: b"bootstrap-signature".to_vec(),
                },
                approval_context: aura_core::threshold::ApprovalContext::SelfOperation,
            })
            .await
            .expect("bootstrapped authority should sign");

        assert!(signature.is_single_signer());
        assert_eq!(signature.public_key_package, bootstrapped_public_key);
    }

    #[tokio::test]
    async fn test_sign_uses_threshold_pubkey_fallback_for_service_bootstrap() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = AgentConfig {
            storage: StorageConfig {
                base_path: temp.path().join("aura"),
                ..Default::default()
            },
            ..Default::default()
        };
        let effect_system = crate::testing::simulation_effect_system_arc(&config);
        let service = ThresholdSigningService::new(effect_system.clone());
        let authority = AuthorityId::new_from_entropy([35u8; 32]);

        let bootstrapped_public_key = service
            .bootstrap_authority(&authority)
            .await
            .expect("service bootstrap should succeed");

        assert_eq!(
            effect_system.public_key_package(&authority).await,
            Some(bootstrapped_public_key.clone())
        );

        let signature = effect_system
            .sign(aura_core::threshold::SigningContext {
                authority,
                operation: aura_core::threshold::SignableOperation::Message {
                    domain: "aura.test.threshold-signing-fallback".to_string(),
                    payload: b"bootstrap-signature-fallback".to_vec(),
                },
                approval_context: aura_core::threshold::ApprovalContext::SelfOperation,
            })
            .await
            .expect("effect-system signing should accept service bootstrap layout");

        assert!(signature.is_single_signer());
        assert_eq!(signature.public_key_package, bootstrapped_public_key);
    }
}

// Note: RelationshipFormationEffects is a composite trait that is automatically implemented
// when all required component traits are implemented: ConsoleEffects, CryptoEffects,
// NetworkEffects, RandomEffects, and JournalEffects

impl AuraEffectSystem {
    pub fn device_id(&self) -> aura_core::DeviceId {
        self.config.device_id
    }

    /// Get the current active epoch for an authority's threshold keys
    ///
    /// Returns 0 if no epoch has been stored (bootstrap case).
    async fn get_current_epoch(&self, authority: &AuthorityId) -> u64 {
        let location = SecureStorageLocation::new("epoch_state", format!("{}", authority));
        let caps = vec![SecureStorageCapability::Read];

        match self
            .crypto
            .secure_storage()
            .secure_retrieve(&location, &caps)
            .await
        {
            Ok(data) if data.len() >= 8 => {
                let bytes: [u8; 8] = data[..8].try_into().unwrap_or([0u8; 8]);
                u64::from_le_bytes(bytes)
            }
            _ => 0, // Default to epoch 0 for bootstrap
        }
    }

    /// Set the current active epoch for an authority's threshold keys
    async fn set_current_epoch(
        &self,
        authority: &AuthorityId,
        epoch: u64,
    ) -> Result<(), AuraError> {
        let location = SecureStorageLocation::new("epoch_state", format!("{}", authority));
        let caps = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];

        let data = epoch.to_le_bytes().to_vec();
        self.crypto
            .secure_storage()
            .secure_store(&location, &data, &caps)
            .await
            .map_err(|e| AuraError::storage(format!("Failed to store epoch state: {}", e)))
    }

    /// Delete keys for a specific epoch (used during rollback)
    async fn delete_epoch_keys(
        &self,
        authority: &AuthorityId,
        epoch: u64,
    ) -> Result<(), AuraError> {
        let delete_caps = vec![SecureStorageCapability::Delete];

        // Delete participant shares for this epoch
        let shares_location =
            SecureStorageLocation::new("participant_shares", format!("{}:{}", authority, epoch));
        let _ = self
            .crypto
            .secure_storage()
            .secure_delete(&shares_location, &delete_caps)
            .await;

        // Delete public key for this epoch
        let pubkey_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority),
            format!("{}", epoch),
        );
        let _ = self
            .crypto
            .secure_storage()
            .secure_delete(&pubkey_location, &delete_caps)
            .await;

        // Delete threshold config metadata for this epoch
        let metadata_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            format!("{}", authority),
            format!("{}", epoch),
        );
        let _ = self
            .crypto
            .secure_storage()
            .secure_delete(&metadata_location, &delete_caps)
            .await;

        tracing::debug!(?authority, epoch, "Deleted keys for epoch");
        Ok(())
    }

    /// Store threshold configuration metadata for an epoch
    ///
    /// This stores the threshold, total participants, and guardian IDs alongside
    /// the actual cryptographic keys. This metadata is used by the recovery system
    /// to understand the current guardian configuration.
    async fn store_threshold_config_metadata(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        threshold: u16,
        total_participants: u16,
        participants: &[aura_core::threshold::ParticipantIdentity],
        agreement_mode: aura_core::threshold::AgreementMode,
    ) -> Result<(), AuraError> {
        let metadata = ThresholdConfigMetadata {
            bootstrap_migration_origin: None,
            threshold_k: threshold,
            total_n: total_participants,
            participants: participants.to_vec(),
            mode: if threshold >= 2 {
                SigningMode::Threshold
            } else {
                SigningMode::SingleSigner
            },
            agreement_mode,
        };

        let location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            format!("{}", authority),
            format!("{}", epoch),
        );
        let caps = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];

        let data = serde_json::to_vec(&metadata).map_err(|e| {
            AuraError::storage(format!("Failed to serialize threshold config: {}", e))
        })?;
        self.crypto
            .secure_storage()
            .secure_store(&location, &data, &caps)
            .await
            .map_err(|e| AuraError::storage(format!("Failed to store threshold config: {}", e)))?;

        tracing::debug!(
            ?authority,
            epoch,
            threshold,
            total_participants,
            num_participants = participants.len(),
            "Stored threshold config metadata"
        );
        Ok(())
    }

    /// Required trusted-parent reader: storage and codec faults are not absence.
    pub(crate) async fn require_threshold_config_metadata(
        &self,
        authority: &AuthorityId,
        epoch: u64,
    ) -> Result<ThresholdConfigMetadata, AuraError> {
        let location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            authority.to_string(),
            epoch.to_string(),
        );
        let data = self
            .crypto
            .secure_storage()
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await;
        let mut metadata = Self::decode_required_threshold_metadata(data)?;
        if let Some(origin) = metadata.bootstrap_migration_origin {
            crate::runtime::services::threshold_signing::validate_bootstrap_migration_origin(
                self, authority, epoch, origin,
            )
            .await?;
        }
        if metadata.agreement_mode != aura_core::threshold::AgreementMode::ConsensusFinalized {
            if let Some(archive) =
                crate::runtime::services::enrollment_profile::load_original_active_profile_archive(
                    self,
                )
                .await?
            {
                if archive.manifest().subject == *authority
                    && archive.manifest().pending_epoch == epoch
                {
                    let owner = self
                        .load_confirmed_activation_envelope(archive.confirmed())
                        .await?;
                    metadata = self.confirmed_activation_finalized_config(&owner).await?;
                }
            }
        }
        Ok(metadata)
    }

    fn decode_required_threshold_metadata(
        read: Result<Vec<u8>, AuraError>,
    ) -> Result<ThresholdConfigMetadata, AuraError> {
        let data = read?;
        if data.len() > 131_072 {
            return Err(AuraError::crypto("trusted parent metadata exceeds bounds"));
        }
        let metadata: ThresholdConfigMetadata =
            serde_json::from_slice(&data).map_err(|error| AuraError::Serialization {
                message: "decode required trusted parent threshold metadata".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        let distinct: std::collections::HashSet<_> = metadata.participants.iter().collect();
        if metadata.total_n == 0
            || metadata.total_n > 1024
            || metadata.threshold_k == 0
            || metadata.threshold_k > metadata.total_n
            || metadata.participants.len() != usize::from(metadata.total_n)
            || distinct.len() != metadata.participants.len()
        {
            return Err(AuraError::crypto(
                "invalid required trusted parent participant policy",
            ));
        }
        if metadata.mode == SigningMode::SingleSigner
            && (metadata.threshold_k != 1 || metadata.total_n != 1)
        {
            return Err(AuraError::crypto(
                "invalid required trusted parent single-signer policy",
            ));
        }
        Ok(metadata)
    }

    /// Retrieve threshold configuration metadata for an epoch
    ///
    /// Returns None if no metadata exists for the epoch.
    async fn get_threshold_config_metadata(
        &self,
        authority: &AuthorityId,
        epoch: u64,
    ) -> Option<ThresholdConfigMetadata> {
        let location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            format!("{}", authority),
            format!("{}", epoch),
        );
        let caps = vec![SecureStorageCapability::Read];

        match self
            .crypto
            .secure_storage()
            .secure_retrieve(&location, &caps)
            .await
        {
            Ok(data) => match serde_json::from_slice(&data) {
                Ok(metadata) => Some(metadata),
                Err(e) => {
                    tracing::warn!(
                        ?authority,
                        epoch,
                        error = %e,
                        "Failed to deserialize threshold config"
                    );
                    None
                }
            },
            Err(_) => None,
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn authenticated_browser_harness_mode() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let Ok(search) = window.location().search() else {
        return false;
    };
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
    has_instance && has_token
}

#[cfg(not(target_arch = "wasm32"))]
fn authenticated_browser_harness_mode() -> bool {
    false
}

/// Threshold configuration metadata stored alongside keys
///
/// This structure captures the full threshold configuration for an epoch,
/// including the guardian IDs which are needed for recovery operations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ThresholdConfigMetadata {
    /// Minimum signers required (k in k-of-n)
    pub(crate) threshold_k: u16,
    /// Total number of participants (n in k-of-n)
    pub(crate) total_n: u16,
    /// Participants (in protocol participant order)
    #[serde(default)]
    pub(crate) participants: Vec<aura_core::threshold::ParticipantIdentity>,
    /// Signing mode for the stored epoch.
    pub(crate) mode: SigningMode,
    /// Agreement mode (A1/A2/A3) for the stored epoch
    #[serde(default)]
    pub(crate) agreement_mode: aura_core::threshold::AgreementMode,
    /// Exact protected original bootstrap migration decision, absent for fresh keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) bootstrap_migration_origin: Option<[u8; 32]>,
}

impl ThresholdConfigMetadata {
    pub(crate) fn contains_participant(
        &self,
        participant: &aura_core::threshold::ParticipantIdentity,
    ) -> bool {
        self.participants.contains(participant)
    }
}

impl ThresholdConfigMetadata {
    pub(crate) fn resolved_participants(&self) -> Vec<aura_core::threshold::ParticipantIdentity> {
        self.participants.clone()
    }
}

// Manual Debug implementation since some fields don't implement Debug
impl std::fmt::Debug for AuraEffectSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuraEffectSystem")
            .field("profile_owned", &self.profile_owner.is_some())
            .field("config", &self.config)
            .field("authority_id", &self.authority_id)
            .field("journal_policy", &self.journal.journal_policy().is_some())
            .field(
                "journal_verifying_key",
                &self.journal.journal_verifying_key().is_some(),
            )
            .finish_non_exhaustive()
    }
}

pub(crate) fn enrollment_generation_profile_location(
    authority: &aura_core::AuthorityId,
    epoch: u64,
) -> aura_core::effects::SecureStorageLocation {
    aura_core::effects::SecureStorageLocation::with_sub_key(
        "device_enrollment_generation_live_slot_v2",
        authority.to_string(),
        epoch.to_string(),
    )
}

pub(crate) fn legacy_enrollment_generation_profile_location(
    authority: &aura_core::AuthorityId,
    epoch: u64,
) -> aura_core::effects::SecureStorageLocation {
    aura_core::effects::SecureStorageLocation::with_sub_key(
        "device_enrollment_generation_profile_v1",
        authority.to_string(),
        epoch.to_string(),
    )
}

#[cfg(all(test, unix))]
mod test_namespace_tests {
    use super::AuraEffectSystem;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn collision_exhaustion_never_returns_or_modifies_a_preexisting_profile(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        for suffix in ["0", "1", "2-fallback"] {
            let name = root
                .path()
                .join(format!("aura-agent-isolated-v2-fixture-{suffix}"));
            std::fs::create_dir(&name)?;
            std::fs::set_permissions(&name, std::fs::Permissions::from_mode(0o755))?;
        }
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let failure =
            AuraEffectSystem::create_test_storage_namespace(root.path(), "fixture", &counter, 2)
                .expect_err(
                    "collision budget cannot authorize unchecked fallback or existing owner",
                );
        assert_eq!(failure.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::metadata(root.path().join("aura-agent-isolated-v2-fixture-0"))?
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        let fresh =
            AuraEffectSystem::create_test_storage_namespace(root.path(), "fixture", &counter, 2)?;
        assert_eq!(fresh, root.path().join("aura-agent-isolated-v2-fixture-2"));
        assert!(fresh.is_dir());
        Ok(())
    }
    #[test]
    fn namespace_creation_fault_preserves_native_io_instead_of_returning_a_locator(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let file = root.path().join("not-a-directory");
        std::fs::write(&file, b"fixture")?;
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let failure =
            AuraEffectSystem::create_test_storage_namespace(&file, "fixture", &counter, 2)
                .expect_err("native creation fault cannot be swallowed");
        assert!(failure.raw_os_error().is_some());
        assert_eq!(std::fs::read(file)?, b"fixture");
        Ok(())
    }
}
