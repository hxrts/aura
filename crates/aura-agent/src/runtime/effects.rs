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

use crate::core::config::default_storage_path;
use crate::core::AgentConfig;
use crate::database::IndexedJournalHandler;
use crate::fact_registry::build_fact_registry;
use crate::runtime::services::{LanTransportService, LogicalClockManager, RendezvousManager};
use crate::runtime::subsystems::{
    crypto::CryptoRng, ChoreographyState, CryptoSubsystem, JournalSubsystem, TransportSubsystem,
};
use crate::runtime::time_handler::EnhancedTimeHandler;
use async_trait::async_trait;
use aura_app::ReactiveHandler;
use aura_authorization::BiscuitAuthorizationBridge;
use aura_composition::{CompositeHandlerAdapter, RegisterAllOptions};
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::*;
use aura_core::scope::AuthorizationOp;
use aura_core::{AuraError, AuthorityId, ContextId};
use aura_effects::{
    crypto::RealCryptoHandler,
    encrypted_storage::{EncryptedStorage, EncryptedStorageConfig},
    secure::RealSecureStorageHandler,
    storage::FilesystemStorageHandler,
    time::{OrderClockHandler, PhysicalTimeHandler},
};
use aura_journal::extensibility::FactRegistry;
use aura_journal::fact::ProtocolRelationalFact;
use aura_journal::fact::{
    DkgTranscriptCommit, Fact as TypedFact, FactContent, FactOptions, RelationalFact,
};
use aura_protocol::handlers::{PersistentSyncHandler, PersistentTreeHandler};
use biscuit_auth::{macros::*, Biscuit, KeyPair, PublicKey};
use parking_lot::RwLock;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::sync::Arc;
#[cfg(debug_assertions)]
use std::sync::Once;
#[cfg(debug_assertions)]
use std::time::Duration;
use tokio::sync::mpsc;

use super::shared_transport::SharedTransport;

mod amp;
mod aura;
mod choreography;
mod crypto;
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
    // === Core Configuration ===
    config: AgentConfig,
    authority_id: AuthorityId,
    execution_mode: ExecutionMode,

    // === Subsystems (grouped related fields) ===
    /// Cryptographic operations subsystem
    crypto: CryptoSubsystem,
    /// Network transport subsystem
    transport: TransportSubsystem,
    /// Journal and fact management subsystem
    journal: JournalSubsystem,

    // === Composition & Handlers ===
    composite: CompositeHandlerAdapter,

    // === Storage Infrastructure ===
    storage_handler: Arc<
        EncryptedStorage<FilesystemStorageHandler, RealCryptoHandler, RealSecureStorageHandler>,
    >,
    tree_handler: PersistentTreeHandler,
    sync_handler: PersistentSyncHandler,

    // === Time Services ===
    time_handler: EnhancedTimeHandler,
    logical_clock: LogicalClockManager,
    order_clock: OrderClockHandler,

    // === Authorization & Flow Control ===
    authorization_handler: aura_authorization::effects::WotAuthorizationHandler<
        aura_effects::crypto::RealCryptoHandler,
    >,
    leakage_handler: aura_effects::leakage::ProductionLeakageHandler<
        EncryptedStorage<FilesystemStorageHandler, RealCryptoHandler, RealSecureStorageHandler>,
    >,

    // === Reactive System ===
    /// Reactive signal graph for UI-facing state.
    reactive_handler: ReactiveHandler,

    // === Choreography State ===
    /// In-memory choreography session state for runtime coordination.
    choreography_state: parking_lot::RwLock<ChoreographyState>,

    /// LAN transport service (optional, for TCP envelope delivery)
    lan_transport: parking_lot::RwLock<Option<Arc<LanTransportService>>>,

    /// Rendezvous manager (optional, for address resolution)
    rendezvous_manager: parking_lot::RwLock<Option<RendezvousManager>>,
}

#[derive(Clone, Default)]
struct NoopBiscuitAuthorizationHandler;

#[async_trait]
impl BiscuitAuthorizationEffects for NoopBiscuitAuthorizationHandler {
    async fn authorize_biscuit(
        &self,
        _token_data: &[u8],
        _operation: AuthorizationOp,
        _scope: &aura_core::scope::ResourceScope,
    ) -> Result<AuthorizationDecision, AuthorizationError> {
        Ok(AuthorizationDecision {
            authorized: true,
            reason: None,
        })
    }

    async fn authorize_fact(
        &self,
        _token_data: &[u8],
        _fact_type: &str,
        _scope: &aura_core::scope::ResourceScope,
    ) -> Result<bool, AuthorizationError> {
        Ok(true)
    }
}

impl AuraEffectSystem {
    #[cfg(debug_assertions)]
    fn maybe_start_deadlock_detector() {
        static START: Once = Once::new();
        START.call_once(|| {
            std::thread::Builder::new()
                .name("aura-deadlock-detector".to_string())
                .spawn(|| loop {
                    std::thread::park_timeout(Duration::from_secs(10));
                    let deadlocks = parking_lot::deadlock::check_deadlock();
                    if !deadlocks.is_empty() {
                        // Note: DeadlockedThread doesn't implement Debug, so we log count only
                        tracing::error!(
                            count = deadlocks.len(),
                            "Detected parking_lot deadlock(s)"
                        );
                    }
                })
                .expect("failed to spawn deadlock detector thread");
        });
    }

    #[cfg(not(debug_assertions))]
    fn maybe_start_deadlock_detector() {}

    fn normalize_test_config(mut config: AgentConfig) -> AgentConfig {
        // Avoid writing test data into the user's real data directory (e.g. `~/.aura`).
        //
        // Tests that require a specific persistent directory should override
        // `config.storage.base_path` explicitly.
        if config.storage.base_path == default_storage_path() {
            static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

            let temp_root = std::env::temp_dir();
            let mut attempt = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            let mut selected: Option<std::path::PathBuf> = None;
            for _ in 0..256 {
                let candidate = temp_root.join(format!("aura-agent-test-{attempt}"));
                match std::fs::create_dir(&candidate) {
                    Ok(()) => {
                        selected = Some(candidate);
                        break;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                        attempt = attempt.wrapping_add(1);
                        continue;
                    }
                    Err(_) => {
                        selected = Some(candidate);
                        break;
                    }
                }
            }

            config.storage.base_path =
                selected.unwrap_or_else(|| temp_root.join("aura-agent-test-fallback"));
        }
        config
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
        authority_override: Option<AuthorityId>,
    ) -> Self {
        Self::maybe_start_deadlock_detector();
        let device_id = config.device_id();
        let authority = authority_override.unwrap_or_else(|| AuthorityId::from_uuid(device_id.0));
        let (journal_policy, journal_verifying_key) = Self::init_journal_policy(authority);
        let test_mode = execution_mode.is_deterministic();

        // === Build CryptoSubsystem ===
        let crypto_handler = match crypto_seed {
            Some(seed) => RealCryptoHandler::seeded(seed),
            None => RealCryptoHandler::new(),
        };
        let random_rng = match crypto_seed {
            Some(seed) => CryptoRng::deterministic(StdRng::from_seed(seed)),
            None => CryptoRng::thread_local(),
        };
        let secure_storage_handler = Arc::new(RealSecureStorageHandler::with_base_path(
            config.storage.base_path.clone(),
        ));
        let crypto = CryptoSubsystem::from_parts(
            crypto_handler.clone(),
            random_rng,
            secure_storage_handler.clone(),
        );

        // === Build Storage Infrastructure ===
        let auth_time = PhysicalTimeHandler::new();
        let time_handler = EnhancedTimeHandler::new();
        let authorization_handler = Self::init_authorization_handler(
            authority,
            &crypto_handler,
            &journal_verifying_key,
            &auth_time,
        );
        let encrypted_storage_config = {
            let mut cfg = EncryptedStorageConfig::default()
                .with_encryption_enabled(config.storage.encryption_enabled);
            if config.storage.opaque_names {
                cfg = cfg.with_opaque_names();
            }
            // Note: Legacy plaintext migration support removed - all data must be encrypted
            let _ = test_mode; // Suppress unused warning
            cfg
        };
        let storage_handler = Arc::new(EncryptedStorage::new(
            FilesystemStorageHandler::new(config.storage.base_path.clone()),
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
            journal_policy,
            journal_verifying_key,
        );

        Self {
            config,
            authority_id: authority,
            execution_mode,
            crypto,
            transport,
            journal,
            composite,
            storage_handler,
            tree_handler,
            sync_handler,
            time_handler,
            logical_clock: LogicalClockManager::new(Some(device_id)),
            order_clock: OrderClockHandler,
            authorization_handler,
            leakage_handler,
            reactive_handler: ReactiveHandler::new(),
            choreography_state: parking_lot::RwLock::new(ChoreographyState::default()),
            lan_transport: parking_lot::RwLock::new(None),
            rendezvous_manager: parking_lot::RwLock::new(None),
        }
    }

    /// Check if the effect system is in test mode (bypasses authorization guards)
    pub fn is_testing(&self) -> bool {
        self.execution_mode.is_deterministic()
    }

    /// Check if the effect system is in explicit test mode (not simulation).
    pub fn is_test_mode(&self) -> bool {
        matches!(self.execution_mode, ExecutionMode::Testing)
    }

    fn ensure_mock_network(&self) -> Result<(), NetworkError> {
        if self.execution_mode.is_deterministic() {
            Ok(())
        } else {
            Err(NetworkError::NotImplemented)
        }
    }

    fn ensure_mock_system(&self, operation: &str) -> Result<(), SystemError> {
        if self.execution_mode.is_deterministic() {
            Ok(())
        } else {
            Err(SystemError::OperationFailed {
                message: format!("SystemEffects::{operation} not implemented for production"),
            })
        }
    }

    fn ensure_mock_effect_api(
        &self,
        operation: &str,
    ) -> Result<(), aura_protocol::effects::EffectApiError> {
        if self.execution_mode.is_deterministic() {
            Ok(())
        } else {
            Err(aura_protocol::effects::EffectApiError::Backend {
                error: format!("EffectApi::{operation} not implemented for production"),
            })
        }
    }

    /// Get the shared reactive handler (signal graph) for this runtime.
    pub fn reactive_handler(&self) -> ReactiveHandler {
        self.reactive_handler.clone()
    }

    /// Attach a fact sink for reactive scheduling (facts → scheduler ingestion).
    ///
    /// This is called during runtime startup when the ReactivePipeline is started.
    pub fn attach_fact_sink(&self, tx: mpsc::Sender<crate::reactive::FactSource>) {
        self.journal.attach_fact_sink(tx);
    }

    /// Attach a view update sender for awaiting fact processing.
    ///
    /// This is called during runtime startup when the ReactivePipeline is started.
    pub fn attach_view_update_sender(
        &self,
        tx: tokio::sync::broadcast::Sender<crate::reactive::ViewUpdate>,
    ) {
        self.journal.attach_view_update_sender(tx);
    }

    /// Wait for the reactive scheduler to process the next batch of facts.
    ///
    /// This is useful after committing facts to ensure the reactive views
    /// have been updated before continuing. Returns immediately if no
    /// view update subscription is available (e.g., in tests).
    ///
    /// # Example
    ///
    /// ```ignore
    /// effects.commit_generic_fact_bytes(context, type_id, bytes).await?;
    /// effects.await_next_view_update().await; // Ensure views are updated
    /// ```
    pub async fn await_next_view_update(&self) {
        use crate::reactive::ViewUpdate;

        let Some(mut rx) = self.journal.subscribe_view_updates() else {
            return;
        };

        loop {
            match rx.recv().await {
                Ok(ViewUpdate::Batch { .. }) => return,
                Ok(_) => continue,
                Err(_) => return, // Channel closed or lagged, just return
            }
        }
    }

    pub fn requeue_envelope(&self, envelope: TransportEnvelope) {
        self.transport.queue_envelope(envelope);
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

    async fn publish_typed_facts(&self, facts: Vec<TypedFact>) -> Result<(), AuraError> {
        let tx = self.journal.fact_publisher();
        let Some(tx) = tx else {
            return Ok(());
        };

        tx.send(crate::reactive::FactSource::Journal(facts))
            .await
            .map_err(|_| AuraError::internal("Reactive fact sink dropped"))?;

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
        if facts.is_empty() {
            return Ok(vec![]);
        }

        let mut committed: Vec<TypedFact> = Vec::with_capacity(facts.len());
        for rel in facts {
            let order = self
                .order_time()
                .await
                .map_err(|e| AuraError::internal(format!("order_time: {e}")))?;

            let fact = TypedFact::new(
                order.clone(),
                aura_core::time::TimeStamp::OrderClock(order.clone()),
                FactContent::Relational(rel),
            );

            let key = Self::typed_fact_storage_key(self.authority_id, &order);
            let bytes = aura_core::util::serialization::to_vec(&fact)
                .map_err(|e| AuraError::internal(format!("serialize fact: {e}")))?;
            self.store(&key, bytes)
                .await
                .map_err(|e| AuraError::storage(format!("persist fact: {e}")))?;

            committed.push(fact);
        }

        // Publish after persistence so subscribers can always recover from storage.
        self.publish_typed_facts(committed.clone()).await?;

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
                .map_err(|e| AuraError::internal(format!("order_time: {e}")))?;

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
            let bytes = aura_core::util::serialization::to_vec(&fact)
                .map_err(|e| AuraError::internal(format!("serialize fact: {e}")))?;
            self.store(&key, bytes)
                .await
                .map_err(|e| AuraError::storage(format!("persist fact: {e}")))?;

            committed.push(fact);
        }

        // Publish after persistence so subscribers can always recover from storage.
        self.publish_typed_facts(committed.clone()).await?;

        Ok(committed)
    }

    /// Commit a single generic domain fact (binding_type + bytes) into the canonical fact store.
    pub async fn commit_generic_fact_bytes(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: Vec<u8>,
    ) -> Result<TypedFact, AuraError> {
        let envelope = aura_core::types::facts::FactEnvelope {
            type_id: aura_core::types::facts::FactTypeId::from(binding_type),
            schema_version: 1,
            encoding: aura_core::types::facts::FactEncoding::DagCbor,
            payload: binding_data,
        };
        let rel = RelationalFact::Generic {
            context_id,
            envelope,
        };
        let mut committed = self.commit_relational_facts(vec![rel]).await?;
        Ok(committed
            .pop()
            .unwrap_or_else(|| unreachable!("commit_relational_facts committed exactly one")))
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
            .map_err(|e| AuraError::storage(format!("list_keys: {e}")))?;
        keys.sort();

        let mut facts = Vec::new();
        for key in keys {
            let Some(bytes) = self
                .retrieve(&key)
                .await
                .map_err(|e| AuraError::storage(format!("retrieve: {e}")))?
            else {
                continue;
            };

            let fact: TypedFact = aura_core::util::serialization::from_slice(&bytes)
                .map_err(|e| AuraError::internal(format!("deserialize fact: {e}")))?;
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
    pub fn new(config: AgentConfig) -> Result<Self, crate::core::AgentError> {
        let config = Self::normalize_test_config(config);
        let composite = CompositeHandlerAdapter::for_testing(config.device_id());
        Ok(Self::build_internal(
            config,
            composite,
            ExecutionMode::Testing,
            Some(Self::TEST_CRYPTO_SEED),
            None, // No shared transport
            None, // No shared inbox
            None, // Derive authority from device_id
        ))
    }

    /// Create effect system for production.
    pub fn production(config: AgentConfig) -> Result<Self, crate::core::AgentError> {
        let mut composite = CompositeHandlerAdapter::for_production(config.device_id());
        composite
            .composite_mut()
            .register_all(RegisterAllOptions::allow_impure())
            .map_err(|e| crate::core::AgentError::effects(e.to_string()))?;
        // Production uses OS entropy, no seed
        Ok(Self::build_internal(
            config,
            composite,
            ExecutionMode::Production,
            None,
            None,
            None,
            None,
        ))
    }

    /// Create effect system for testing with default configuration.
    pub fn testing(config: &AgentConfig) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_testing(config.device_id());
        Ok(Self::build_internal(
            Self::normalize_test_config(config.clone()),
            composite,
            ExecutionMode::Testing,
            Some(Self::TEST_CRYPTO_SEED),
            None, // No shared transport
            None, // No shared inbox
            None, // Derive authority from device_id
        ))
    }

    /// Create effect system for testing with shared transport.
    ///
    /// This factory is used for tests that need to verify transport envelope routing,
    /// enabling loopback testing where an agent can send and receive messages from itself.
    pub fn testing_with_shared_transport(
        config: &AgentConfig,
        shared_transport: SharedTransport,
    ) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_testing(config.device_id());
        Ok(Self::build_internal(
            Self::normalize_test_config(config.clone()),
            composite,
            ExecutionMode::Testing,
            Some(Self::TEST_CRYPTO_SEED),
            Some(shared_transport),
            None, // No shared inbox
            None, // Derive authority from device_id
        ))
    }

    /// Create effect system for simulation with controlled seed.
    pub fn simulation(config: &AgentConfig, seed: u64) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_simulation(config.device_id(), seed);
        // Convert u64 seed to [u8; 32] for crypto handler
        let mut crypto_seed = [0u8; 32];
        crypto_seed[0..8].copy_from_slice(&seed.to_le_bytes());
        Ok(Self::build_internal(
            config.clone(),
            composite,
            ExecutionMode::Simulation { seed },
            Some(crypto_seed),
            None, // No shared transport
            None, // No shared inbox
            None, // Derive authority from device_id
        ))
    }

    /// Create effect system for simulation with shared transport.
    ///
    /// This factory is used for multi-agent simulations where all agents need to
    /// communicate through a shared transport layer. The shared transport enables
    /// message routing between Bob, Alice, and Carol in demo mode.
    pub fn simulation_with_shared_transport(
        config: &AgentConfig,
        seed: u64,
        shared_transport: SharedTransport,
    ) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_simulation(config.device_id(), seed);
        // Convert u64 seed to [u8; 32] for crypto handler
        let mut crypto_seed = [0u8; 32];
        crypto_seed[0..8].copy_from_slice(&seed.to_le_bytes());
        Ok(Self::build_internal(
            config.clone(),
            composite,
            ExecutionMode::Simulation { seed },
            Some(crypto_seed),
            Some(shared_transport),
            None, // No shared inbox
            None, // Derive authority from device_id
        ))
    }

    /// Create effect system for simulation with a shared inbox.
    ///
    /// This variant matches the aura-core simulation factory contract and uses
    /// a single shared inbox for all agents. Receivers filter by destination.
    pub fn simulation_with_shared_inbox(
        config: &AgentConfig,
        seed: u64,
        shared_inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    ) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_simulation(config.device_id(), seed);
        let mut crypto_seed = [0u8; 32];
        crypto_seed[0..8].copy_from_slice(&seed.to_le_bytes());
        Ok(Self::build_internal(
            config.clone(),
            composite,
            ExecutionMode::Simulation { seed },
            Some(crypto_seed),
            None, // No shared transport
            Some(shared_inbox),
            None, // Derive authority from device_id
        ))
    }

    /// Create effect system for production, overriding the authority identity.
    pub fn production_for_authority(
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Result<Self, crate::core::AgentError> {
        let mut composite = CompositeHandlerAdapter::for_production(config.device_id());
        composite
            .composite_mut()
            .register_all(RegisterAllOptions::allow_impure())
            .map_err(|e| crate::core::AgentError::effects(e.to_string()))?;
        Ok(Self::build_internal(
            config,
            composite,
            ExecutionMode::Production,
            None,
            None,
            None,
            Some(authority_id),
        ))
    }

    /// Create effect system for testing, overriding the authority identity.
    pub fn testing_for_authority(
        config: &AgentConfig,
        authority_id: AuthorityId,
    ) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_testing(config.device_id());
        Ok(Self::build_internal(
            Self::normalize_test_config(config.clone()),
            composite,
            ExecutionMode::Testing,
            Some(Self::TEST_CRYPTO_SEED),
            None,
            None,
            Some(authority_id),
        ))
    }

    /// Create effect system for simulation, overriding the authority identity.
    pub fn simulation_for_authority(
        config: &AgentConfig,
        seed: u64,
        authority_id: AuthorityId,
    ) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_simulation(config.device_id(), seed);
        let mut crypto_seed = [0u8; 32];
        crypto_seed[0..8].copy_from_slice(&seed.to_le_bytes());
        Ok(Self::build_internal(
            config.clone(),
            composite,
            ExecutionMode::Simulation { seed },
            Some(crypto_seed),
            None,
            None,
            Some(authority_id),
        ))
    }

    /// Create effect system for simulation with shared transport, overriding authority.
    pub fn simulation_with_shared_transport_for_authority(
        config: &AgentConfig,
        seed: u64,
        authority_id: AuthorityId,
        shared_transport: SharedTransport,
    ) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_simulation(config.device_id(), seed);
        let mut crypto_seed = [0u8; 32];
        crypto_seed[0..8].copy_from_slice(&seed.to_le_bytes());
        Ok(Self::build_internal(
            config.clone(),
            composite,
            ExecutionMode::Simulation { seed },
            Some(crypto_seed),
            Some(shared_transport),
            None,
            Some(authority_id),
        ))
    }

    /// Create effect system for simulation with a shared inbox, overriding authority.
    pub fn simulation_with_shared_inbox_for_authority(
        config: &AgentConfig,
        seed: u64,
        authority_id: AuthorityId,
        shared_inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    ) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_simulation(config.device_id(), seed);
        let mut crypto_seed = [0u8; 32];
        crypto_seed[0..8].copy_from_slice(&seed.to_le_bytes());
        Ok(Self::build_internal(
            config.clone(),
            composite,
            ExecutionMode::Simulation { seed },
            Some(crypto_seed),
            None,
            Some(shared_inbox),
            Some(authority_id),
        ))
    }

    /// Get configuration
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// Get composite handler
    pub fn composite(&self) -> &CompositeHandlerAdapter {
        &self.composite
    }

    /// Get access to time effects
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
    ) -> (
        Option<(Biscuit, BiscuitAuthorizationBridge)>,
        Option<Vec<u8>>,
    ) {
        let keypair = KeyPair::new();
        let authority = authority_id.to_string();
        let token = biscuit!(
            r#"
            authority({authority});
            role("owner");
            capability("read");
            capability("write");
            capability("execute");
            capability("delegate");
            capability("admin");
            capability("flow_charge");
        "#
        )
        .build(&keypair);

        match token {
            Ok(token) => {
                let bridge = BiscuitAuthorizationBridge::new(keypair.public(), authority_id);
                let verifying_key = keypair.public().to_bytes().to_vec();
                (Some((token, bridge)), Some(verifying_key))
            }
            Err(_) => (None, None),
        }
    }

    /// Build the Biscuit-backed authorization handler. Falls back to mock when no key is available.
    fn init_authorization_handler(
        authority: AuthorityId,
        crypto_handler: &RealCryptoHandler,
        verifying_key: &Option<Vec<u8>>,
        time_handler: &PhysicalTimeHandler,
    ) -> aura_authorization::effects::WotAuthorizationHandler<RealCryptoHandler> {
        if let Some(bytes) = verifying_key {
            if let Ok(public_key) = PublicKey::from_bytes(bytes) {
                let handler = aura_authorization::effects::WotAuthorizationHandler::new(
                    crypto_handler.clone(),
                    public_key,
                    authority,
                );
                let time_handler = time_handler.clone();
                return handler.with_time_provider(Arc::new(move || {
                    time_handler.physical_time_now_ms() / 1000
                }));
            }
        }

        let handler =
            aura_authorization::effects::WotAuthorizationHandler::new_mock(crypto_handler.clone());
        let time_handler = time_handler.clone();
        handler.with_time_provider(Arc::new(move || time_handler.physical_time_now_ms() / 1000))
    }

    /// Construct a journal handler with current policy hooks.
    fn journal_handler(
        &self,
    ) -> aura_journal::JournalHandler<
        RealCryptoHandler,
        Arc<
            EncryptedStorage<FilesystemStorageHandler, RealCryptoHandler, RealSecureStorageHandler>,
        >,
        NoopBiscuitAuthorizationHandler,
    > {
        let authorization = self
            .journal
            .journal_policy()
            .and_then(|(token, _bridge)| token.to_vec().ok())
            .map(|bytes| (bytes, NoopBiscuitAuthorizationHandler));

        aura_journal::JournalHandlerFactory::create(
            self.authority_id,
            self.crypto.handler().clone(),
            self.storage_handler.clone(),
            authorization,
            self.journal.journal_verifying_key().map(|s| s.to_vec()),
            None, // Fact registry is accessed via AuraEffectSystem::fact_registry() instead
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::ContextId;
    use aura_guards::GuardContextProvider;
    use aura_protocol::amp::AmpJournalEffects;
    use aura_protocol::effects::SyncEffects;
    use aura_protocol::effects::TreeEffects;

    #[tokio::test]
    async fn test_frost_integration_through_effect_system() {
        let config = AgentConfig::default();
        let effect_system = AuraEffectSystem::testing(&config).unwrap();

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
        // Create two effect systems with the same seed
        let config = AgentConfig::default();
        let effect_system1 = AuraEffectSystem::testing(&config).unwrap();
        let effect_system2 = AuraEffectSystem::testing(&config).unwrap();

        // Generate keys from both - they should produce identical results
        // because testing mode uses the same TEST_CRYPTO_SEED
        let result1 = effect_system1.frost_generate_keys(2, 3).await.unwrap();
        let result2 = effect_system2.frost_generate_keys(2, 3).await.unwrap();

        assert_eq!(
            result1.public_key_package, result2.public_key_package,
            "Seeded crypto should produce deterministic public key packages"
        );
    }

    #[tokio::test]
    async fn test_guard_effect_system_enables_amp_journal_effects() {
        let config = AgentConfig::default();
        let effect_system = AuraEffectSystem::testing(&config).unwrap();

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

    #[tokio::test]
    async fn test_tree_and_sync_handlers_are_wired() {
        let config = AgentConfig::default();
        let effect_system = AuraEffectSystem::testing(&config).unwrap();

        // Tree state should be retrievable (empty but deterministic)
        let state = effect_system.get_current_state().await.unwrap();
        assert_eq!(state.epoch, aura_core::Epoch::initial()); // fresh tree starts at epoch 0
        assert_eq!(state.root_commitment, [0u8; 32]);

        // Sync digest should not error
        let digest = effect_system.get_oplog_digest().await.unwrap();
        assert_eq!(digest.cids.len(), 0);
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
            SecureStorageLocation::new("participant_shares", format!("{}/{}", authority, epoch));
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

        // Delete threshold metadata for this epoch
        let metadata_location = SecureStorageLocation::with_sub_key(
            "threshold_metadata",
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
    async fn store_threshold_metadata(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        threshold: u16,
        total_participants: u16,
        participants: &[aura_core::threshold::ParticipantIdentity],
        agreement_mode: aura_core::threshold::AgreementMode,
    ) -> Result<(), AuraError> {
        let metadata = ThresholdMetadata {
            epoch,
            threshold,
            total_participants,
            participants: participants.to_vec(),
            agreement_mode,
        };

        let location = SecureStorageLocation::with_sub_key(
            "threshold_metadata",
            format!("{}", authority),
            format!("{}", epoch),
        );
        let caps = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];

        let data = serde_json::to_vec(&metadata).map_err(|e| {
            AuraError::storage(format!("Failed to serialize threshold metadata: {}", e))
        })?;
        self.crypto
            .secure_storage()
            .secure_store(&location, &data, &caps)
            .await
            .map_err(|e| {
                AuraError::storage(format!("Failed to store threshold metadata: {}", e))
            })?;

        tracing::debug!(
            ?authority,
            epoch,
            threshold,
            total_participants,
            num_participants = participants.len(),
            "Stored threshold metadata"
        );
        Ok(())
    }

    /// Retrieve threshold configuration metadata for an epoch
    ///
    /// Returns None if no metadata exists for the epoch.
    async fn get_threshold_metadata(
        &self,
        authority: &AuthorityId,
        epoch: u64,
    ) -> Option<ThresholdMetadata> {
        let location = SecureStorageLocation::with_sub_key(
            "threshold_metadata",
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
                        "Failed to deserialize threshold metadata"
                    );
                    None
                }
            },
            Err(_) => None,
        }
    }
}

/// Threshold configuration metadata stored alongside keys
///
/// This structure captures the full threshold configuration for an epoch,
/// including the guardian IDs which are needed for recovery operations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ThresholdMetadata {
    /// The epoch this configuration applies to
    epoch: u64,
    /// Minimum signers required (k in k-of-n)
    threshold: u16,
    /// Total number of participants (n in k-of-n)
    total_participants: u16,
    /// Participants (in protocol participant order)
    #[serde(default)]
    participants: Vec<aura_core::threshold::ParticipantIdentity>,
    /// Agreement mode (A1/A2/A3) for the stored epoch
    #[serde(default)]
    agreement_mode: aura_core::threshold::AgreementMode,
}

impl ThresholdMetadata {
    fn resolved_participants(&self) -> Vec<aura_core::threshold::ParticipantIdentity> {
        self.participants.clone()
    }
}

// Manual Debug implementation since some fields don't implement Debug
impl std::fmt::Debug for AuraEffectSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuraEffectSystem")
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
