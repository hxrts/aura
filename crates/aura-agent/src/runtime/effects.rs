//! Effect System Components
//!
//! Core effect system components per Layer-6 spec.

use crate::core::config::default_storage_path;
use crate::core::AgentConfig;
use crate::fact_registry::build_fact_registry;
use async_trait::async_trait;
use aura_composition::{CompositeHandlerAdapter, RegisterAllOptions};
use aura_core::crypto::single_signer::SigningMode;
use aura_core::effects::crypto::{FrostSigningPackage, SigningKeyGenResult};
use aura_core::effects::network::PeerEventStream;
use aura_core::effects::storage::{StorageError, StorageStats};
use aura_core::effects::transport::{TransportEnvelope, TransportStats};
use aura_core::effects::TransportEffects;
use aura_core::effects::*;
use aura_core::hash::hash;
use aura_core::Journal;
use aura_core::{
    AttestedOp, AuraError, AuthorityId, ChannelId, ContextId, DeviceId, FlowBudget, Hash32,
};
use aura_effects::{
    crypto::RealCryptoHandler,
    encrypted_storage::{EncryptedStorage, EncryptedStorageConfig},
    secure::RealSecureStorageHandler,
    storage::FilesystemStorageHandler,
    time::{OrderClockHandler, PhysicalTimeHandler},
};
use aura_app::ReactiveHandler;
use crate::database::IndexedJournalHandler;
use crate::handlers::logical_clock_service::LogicalClockService;
use aura_journal::commitment_tree::state::TreeState as JournalTreeState;
use aura_journal::extensibility::{DomainFact, FactRegistry};
use aura_journal::fact::{Fact as TypedFact, FactContent, RelationalFact};
use aura_protocol::amp::{AmpJournalEffects, ChannelMembershipFact, ChannelParticipantEvent};
use aura_protocol::effects::{
    AuraEffects, AuthorizationEffects, BloomDigest, ChoreographicEffects, ChoreographicRole,
    ChoreographyError, ChoreographyEvent, ChoreographyMetrics, EffectApiEffects, EffectApiError,
    EffectApiEventStream, LeakageEffects, SyncEffects, SyncError,
};
use aura_guards::GuardContextProvider;
use aura_protocol::handlers::{PersistentSyncHandler, PersistentTreeHandler};
use aura_authorization::{BiscuitAuthorizationBridge, FlowBudgetHandler};
use biscuit_auth::{Biscuit, KeyPair, PublicKey};
use rand::rngs::StdRng;
use rand::RngCore;
use rand::SeedableRng;
use std::collections::HashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::shared_transport::SharedTransport;

const DEFAULT_WINDOW: u32 = 1024;
const TYPED_FACT_STORAGE_PREFIX: &str = "journal/facts";

/// Concrete effect system combining all effects for runtime usage
///
/// Note: This wraps aura-composition infrastructure for Layer 6 runtime concerns.
pub struct AuraEffectSystem {
    config: AgentConfig,
    composite: CompositeHandlerAdapter,
    flow_budget: FlowBudgetHandler,
    crypto_handler: aura_effects::crypto::RealCryptoHandler,
    random_rng: parking_lot::Mutex<StdRng>,
    storage_handler: Arc<
        EncryptedStorage<FilesystemStorageHandler, RealCryptoHandler, RealSecureStorageHandler>,
    >,
    time_handler: PhysicalTimeHandler,
    logical_clock: LogicalClockService,
    order_clock: OrderClockHandler,
    authorization_handler:
        aura_authorization::effects::WotAuthorizationHandler<aura_effects::crypto::RealCryptoHandler>,
    leakage_handler: aura_effects::leakage::ProductionLeakageHandler<
        EncryptedStorage<FilesystemStorageHandler, RealCryptoHandler, RealSecureStorageHandler>,
    >,
    journal_policy: Option<(biscuit_auth::Biscuit, aura_authorization::BiscuitAuthorizationBridge)>,
    journal_verifying_key: Option<Vec<u8>>,
    authority_id: AuthorityId,
    tree_handler: PersistentTreeHandler,
    sync_handler: PersistentSyncHandler,
    transport_handler: aura_effects::transport::RealTransportHandler,
    transport_inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    shared_transport: Option<SharedTransport>,
    transport_stats: Arc<RwLock<TransportStats>>,
    fact_registry: Arc<FactRegistry>,
    /// Reactive signal graph for UI-facing state.
    ///
    /// This is the canonical ReactiveEffects surface for frontends when running
    /// with a full runtime. It is driven by the ReactiveScheduler pipeline.
    reactive_handler: ReactiveHandler,
    /// Secure storage for cryptographic key material (FROST keys, device keys)
    secure_storage_handler: Arc<RealSecureStorageHandler>,
    /// Indexed journal handler for efficient fact lookups (B-tree, Bloom, Merkle)
    indexed_journal: Arc<IndexedJournalHandler>,
    /// Test mode flag to bypass authorization guards
    test_mode: bool,
    /// Optional fact publication sink for the reactive scheduler.
    ///
    /// When configured, committed typed facts are sent to the reactive scheduler
    /// so UI signals can be updated via the canonical scheduler pipeline.
    fact_publish_tx: parking_lot::Mutex<Option<mpsc::Sender<crate::reactive::FactSource>>>,
}

#[derive(Clone, Default)]
struct NoopBiscuitAuthorizationHandler;

#[async_trait]
impl BiscuitAuthorizationEffects for NoopBiscuitAuthorizationHandler {
    async fn authorize_biscuit(
        &self,
        _token_data: &[u8],
        _operation: &str,
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
    fn build_internal(
        config: AgentConfig,
        composite: CompositeHandlerAdapter,
        test_mode: bool,
        crypto_seed: Option<[u8; 32]>,
        shared_transport: Option<SharedTransport>,
        authority_override: Option<AuthorityId>,
    ) -> Self {
        let device_id = config.device_id();
        let authority =
            authority_override.unwrap_or_else(|| AuthorityId::from_uuid(device_id.0));
        let (journal_policy, journal_verifying_key) = Self::init_journal_policy(authority);
        let crypto_handler = match crypto_seed {
            Some(seed) => RealCryptoHandler::seeded(seed),
            None => RealCryptoHandler::new(),
        };
        let random_rng = match crypto_seed {
            Some(seed) => StdRng::from_seed(seed),
            None => StdRng::from_entropy(),
        };
        let authorization_handler =
            Self::init_authorization_handler(authority, &crypto_handler, &journal_verifying_key);
        let secure_storage_handler = Arc::new(RealSecureStorageHandler::with_base_path(
            config.storage.base_path.clone(),
        ));
        let encrypted_storage_config = {
            let mut cfg = EncryptedStorageConfig::default()
                .with_encryption_enabled(config.storage.encryption_enabled);
            if config.storage.opaque_names {
                cfg = cfg.with_opaque_names();
            }
            cfg
        };
        let storage_handler = Arc::new(EncryptedStorage::new(
            FilesystemStorageHandler::new(config.storage.base_path.clone()),
            Arc::new(crypto_handler.clone()),
            secure_storage_handler.clone(),
            encrypted_storage_config,
        ));
        let leakage_handler =
            aura_effects::leakage::ProductionLeakageHandler::with_storage(storage_handler.clone());
        // Both tree and sync handlers share the same storage backend (no shared in-memory oplog)
        let tree_handler = PersistentTreeHandler::new(storage_handler.clone());
        let sync_handler = PersistentSyncHandler::new(storage_handler.clone());
        let transport_handler = aura_effects::transport::RealTransportHandler::default();
        // Use shared transport if provided (simulation mode), otherwise create new local inbox.
        // Also register the authority as "online" in the shared network so transport stats
        // can reflect currently running peer runtimes.
        if let Some(shared) = &shared_transport {
            shared.register(authority);
        }
        let transport_inbox = shared_transport
            .as_ref()
            .map(|shared| shared.inbox())
            .unwrap_or_else(|| Arc::new(RwLock::new(Vec::new())));
        let transport_stats = Arc::new(RwLock::new(TransportStats::default()));
        // Create indexed journal with capacity for 100k facts
        let indexed_journal = Arc::new(IndexedJournalHandler::with_capacity(100_000));

        Self {
            config,
            composite,
            flow_budget: FlowBudgetHandler::new(authority),
            crypto_handler,
            random_rng: parking_lot::Mutex::new(random_rng),
            storage_handler,
            time_handler: PhysicalTimeHandler::new(),
            logical_clock: LogicalClockService::new(Some(device_id)),
            order_clock: OrderClockHandler,
            authorization_handler,
            leakage_handler,
            journal_policy,
            journal_verifying_key,
            authority_id: authority,
            tree_handler,
            sync_handler,
            transport_handler,
            transport_inbox,
            shared_transport,
            transport_stats,
            fact_registry: Arc::new(build_fact_registry()),
            reactive_handler: ReactiveHandler::new(),
            secure_storage_handler,
            indexed_journal,
            test_mode,
            fact_publish_tx: parking_lot::Mutex::new(None),
        }
    }

    /// Check if the effect system is in test mode (bypasses authorization guards)
    pub fn is_testing(&self) -> bool {
        self.test_mode
    }

    /// Get the shared reactive handler (signal graph) for this runtime.
    pub fn reactive_handler(&self) -> ReactiveHandler {
        self.reactive_handler.clone()
    }

    /// Attach a fact sink for reactive scheduling (facts → scheduler ingestion).
    ///
    /// This is called during runtime startup when the ReactivePipeline is started.
    pub fn attach_fact_sink(&self, tx: mpsc::Sender<crate::reactive::FactSource>) {
        *self.fact_publish_tx.lock() = Some(tx);
    }

    pub(crate) fn requeue_envelope(&self, envelope: TransportEnvelope) {
        let mut inbox = self.transport_inbox.write();
        inbox.push(envelope);
    }

    async fn publish_typed_facts(&self, facts: Vec<TypedFact>) -> Result<(), AuraError> {
        let tx = self.fact_publish_tx.lock().clone();
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

            let fact = TypedFact {
                order: order.clone(),
                timestamp: aura_core::time::TimeStamp::OrderClock(order.clone()),
                content: FactContent::Relational(rel),
            };

            let key = Self::typed_fact_storage_key(self.authority_id, &order);
            let bytes = bincode::serialize(&fact)
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
        let rel = RelationalFact::Generic {
            context_id,
            binding_type: binding_type.to_string(),
            binding_data,
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

            let fact: TypedFact = bincode::deserialize(&bytes)
                .map_err(|e| AuraError::internal(format!("deserialize fact: {e}")))?;
            facts.push(fact);
        }

        facts.sort();
        Ok(facts)
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
            true,
            Some(Self::TEST_CRYPTO_SEED),
            None, // No shared transport
            None, // Default authority derivation (legacy)
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
            config, composite, false, None, None, None,
        ))
    }

    /// Create effect system for testing with default configuration.
    pub fn testing(config: &AgentConfig) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_testing(config.device_id());
        Ok(Self::build_internal(
            Self::normalize_test_config(config.clone()),
            composite,
            true,
            Some(Self::TEST_CRYPTO_SEED),
            None, // No shared transport
            None, // Default authority derivation (legacy)
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
            true,
            Some(Self::TEST_CRYPTO_SEED),
            Some(shared_transport),
            None, // Default authority derivation (legacy)
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
            true,
            Some(crypto_seed),
            None, // No shared transport
            None, // Default authority derivation (legacy)
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
            true,
            Some(crypto_seed),
            Some(shared_transport),
            None, // Default authority derivation (legacy)
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
            false,
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
            true,
            Some(Self::TEST_CRYPTO_SEED),
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
            true,
            Some(crypto_seed),
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
            true,
            Some(crypto_seed),
            Some(shared_transport),
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
    pub fn time_effects(&self) -> &PhysicalTimeHandler {
        &self.time_handler
    }

    /// Get the fact registry for domain-specific fact reduction.
    pub fn fact_registry(&self) -> &FactRegistry {
        &self.fact_registry
    }

    /// Get the indexed journal handler for efficient fact lookups.
    ///
    /// Provides O(log n) B-tree indexed lookups, O(1) Bloom filter membership tests,
    /// and Merkle tree integrity verification.
    pub fn indexed_journal(&self) -> &Arc<IndexedJournalHandler> {
        &self.indexed_journal
    }

    /// Build a permissive Biscuit policy/bridge pair for journal enforcement.
    fn init_journal_policy(
        authority_id: AuthorityId,
    ) -> (
        Option<(Biscuit, BiscuitAuthorizationBridge)>,
        Option<Vec<u8>>,
    ) {
        let keypair = KeyPair::new();
        match Biscuit::builder().build(&keypair) {
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
    ) -> aura_authorization::effects::WotAuthorizationHandler<RealCryptoHandler> {
        if let Some(bytes) = verifying_key {
            if let Ok(public_key) = PublicKey::from_bytes(bytes) {
                return aura_authorization::effects::WotAuthorizationHandler::new(
                    crypto_handler.clone(),
                    public_key,
                    authority,
                );
            }
        }

        aura_authorization::effects::WotAuthorizationHandler::new_mock(crypto_handler.clone())
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
        FlowBudgetHandler,
    > {
        let authorization = self
            .journal_policy
            .as_ref()
            .and_then(|(token, _bridge)| token.to_vec().ok())
            .map(|bytes| (bytes, NoopBiscuitAuthorizationHandler));

        aura_journal::JournalHandlerFactory::create(
            self.authority_id,
            self.crypto_handler.clone(),
            self.storage_handler.clone(),
            authorization,
            Some(self.flow_budget.clone()),
            self.journal_verifying_key.clone(),
            None, // Fact registry is accessed via AuraEffectSystem::fact_registry() instead
        )
    }
}

// Time effects backed by the production physical clock handler.
#[async_trait]
impl PhysicalTimeEffects for AuraEffectSystem {
    async fn physical_time(&self) -> Result<aura_core::time::PhysicalTime, TimeError> {
        self.time_handler.physical_time().await
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        self.time_handler.sleep_ms(ms).await
    }
}

#[async_trait]
impl LogicalClockEffects for AuraEffectSystem {
    async fn logical_advance(
        &self,
        observed: Option<&aura_core::time::VectorClock>,
    ) -> Result<aura_core::time::LogicalTime, TimeError> {
        self.logical_clock.logical_advance(observed).await
    }

    async fn logical_now(&self) -> Result<aura_core::time::LogicalTime, TimeError> {
        self.logical_clock.logical_now().await
    }
}

#[async_trait]
impl OrderClockEffects for AuraEffectSystem {
    async fn order_time(&self) -> Result<aura_core::time::OrderTime, TimeError> {
        self.order_clock.order_time().await
    }
}

#[async_trait::async_trait]
impl aura_protocol::effects::TreeEffects for AuraEffectSystem {
    async fn get_current_state(&self) -> Result<JournalTreeState, AuraError> {
        self.tree_handler.get_current_state().await
    }

    async fn get_current_commitment(&self) -> Result<aura_core::Hash32, AuraError> {
        self.tree_handler.get_current_commitment().await
    }

    async fn get_current_epoch(&self) -> Result<u64, AuraError> {
        self.tree_handler.get_current_epoch().await
    }

    async fn apply_attested_op(
        &self,
        op: aura_core::AttestedOp,
    ) -> Result<aura_core::Hash32, AuraError> {
        self.tree_handler.apply_attested_op(op).await
    }

    async fn verify_aggregate_sig(
        &self,
        op: &aura_core::AttestedOp,
        state: &JournalTreeState,
    ) -> Result<bool, AuraError> {
        self.tree_handler.verify_aggregate_sig(op, state).await
    }

    async fn add_leaf(
        &self,
        leaf: aura_core::LeafNode,
        under: aura_core::NodeIndex,
    ) -> Result<aura_core::TreeOpKind, AuraError> {
        self.tree_handler.add_leaf(leaf, under).await
    }

    async fn remove_leaf(
        &self,
        leaf_id: aura_core::LeafId,
        reason: u8,
    ) -> Result<aura_core::TreeOpKind, AuraError> {
        self.tree_handler.remove_leaf(leaf_id, reason).await
    }

    async fn change_policy(
        &self,
        node: aura_core::NodeIndex,
        policy: aura_core::Policy,
    ) -> Result<aura_core::TreeOpKind, AuraError> {
        self.tree_handler.change_policy(node, policy).await
    }

    async fn rotate_epoch(
        &self,
        affected: Vec<aura_core::NodeIndex>,
    ) -> Result<aura_core::TreeOpKind, AuraError> {
        self.tree_handler.rotate_epoch(affected).await
    }

    async fn propose_snapshot(
        &self,
        cut: aura_protocol::effects::tree::Cut,
    ) -> Result<aura_protocol::effects::tree::ProposalId, AuraError> {
        self.tree_handler.propose_snapshot(cut).await
    }

    async fn approve_snapshot(
        &self,
        proposal_id: aura_protocol::effects::tree::ProposalId,
    ) -> Result<aura_protocol::effects::tree::Partial, AuraError> {
        self.tree_handler.approve_snapshot(proposal_id).await
    }

    async fn finalize_snapshot(
        &self,
        proposal_id: aura_protocol::effects::tree::ProposalId,
    ) -> Result<aura_protocol::effects::tree::Snapshot, AuraError> {
        self.tree_handler.finalize_snapshot(proposal_id).await
    }

    async fn apply_snapshot(
        &self,
        snapshot: &aura_protocol::effects::tree::Snapshot,
    ) -> Result<(), AuraError> {
        self.tree_handler.apply_snapshot(snapshot).await
    }
}

// Implementation of RandomCoreEffects
#[async_trait]
impl RandomCoreEffects for AuraEffectSystem {
    #[allow(clippy::disallowed_methods)]
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        self.random_rng.lock().fill_bytes(&mut bytes);
        bytes
    }

    #[allow(clippy::disallowed_methods)]
    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        self.random_rng.lock().fill_bytes(&mut bytes);
        bytes
    }

    #[allow(clippy::disallowed_methods)]
    async fn random_u64(&self) -> u64 {
        self.random_rng.lock().next_u64()
    }

}

#[async_trait]
impl SyncEffects for AuraEffectSystem {
    async fn sync_with_peer(&self, peer_id: uuid::Uuid) -> Result<SyncMetrics, SyncError> {
        self.sync_handler.sync_with_peer(peer_id).await
    }

    async fn get_oplog_digest(&self) -> Result<BloomDigest, SyncError> {
        self.sync_handler.get_oplog_digest().await
    }

    async fn get_missing_ops(
        &self,
        remote_digest: &BloomDigest,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        self.sync_handler.get_missing_ops(remote_digest).await
    }

    async fn request_ops_from_peer(
        &self,
        peer_id: uuid::Uuid,
        cids: Vec<Hash32>,
    ) -> Result<Vec<AttestedOp>, SyncError> {
        self.sync_handler.request_ops_from_peer(peer_id, cids).await
    }

    async fn merge_remote_ops(&self, ops: Vec<AttestedOp>) -> Result<(), SyncError> {
        self.sync_handler.merge_remote_ops(ops).await
    }

    async fn announce_new_op(&self, cid: Hash32) -> Result<(), SyncError> {
        self.sync_handler.announce_new_op(cid).await
    }

    async fn request_op(&self, peer_id: uuid::Uuid, cid: Hash32) -> Result<AttestedOp, SyncError> {
        self.sync_handler.request_op(peer_id, cid).await
    }

    async fn push_op_to_peers(
        &self,
        op: AttestedOp,
        peers: Vec<uuid::Uuid>,
    ) -> Result<(), SyncError> {
        // Local handler has no network; treat push as merge then noop.
        self.sync_handler.merge_remote_ops(vec![op]).await?;
        let _ = peers;
        Ok(())
    }

    async fn get_connected_peers(&self) -> Result<Vec<uuid::Uuid>, SyncError> {
        Ok(Vec::new())
    }
}

// Implementation of TransportEffects
#[async_trait]
impl TransportEffects for AuraEffectSystem {
    async fn send_envelope(&self, envelope: TransportEnvelope) -> Result<(), TransportError> {
        {
            let mut inbox = self.transport_inbox.write();
            inbox.push(envelope.clone());
        }

        {
            let mut stats = self.transport_stats.write();
            stats.envelopes_sent = stats.envelopes_sent.saturating_add(1);
            let running_total = (stats.avg_envelope_size as u64)
                .saturating_mul(stats.envelopes_sent.saturating_sub(1))
                .saturating_add(envelope.payload.len() as u64);
            stats.avg_envelope_size = (running_total / stats.envelopes_sent.max(1)) as u32;
        }

        self.transport_handler.send_envelope(envelope).await
    }

    async fn receive_envelope(&self) -> Result<TransportEnvelope, TransportError> {
        let self_device_id = self.config.device_id.to_string();
        let maybe = {
            let mut inbox = self.transport_inbox.write();
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
                let mut stats = self.transport_stats.write();
                stats.envelopes_received = stats.envelopes_received.saturating_add(1);
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
        let maybe = {
            let mut inbox = self.transport_inbox.write();
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
                let mut stats = self.transport_stats.write();
                stats.envelopes_received = stats.envelopes_received.saturating_add(1);
                Ok(env)
            }
            None => Err(TransportError::NoMessage),
        }
    }

    async fn is_channel_established(&self, context: ContextId, peer: AuthorityId) -> bool {
        if let Some(shared) = &self.shared_transport {
            return shared.is_peer_online(peer);
        }

        self.transport_handler
            .is_channel_established(context, peer)
            .await
    }

    async fn get_transport_stats(&self) -> TransportStats {
        let mut stats = self.transport_stats.read().clone();

        if let Some(shared) = &self.shared_transport {
            stats.active_channels = shared.connected_peer_count(self.authority_id) as u32;
        }

        stats
    }
}

// Implementation of CryptoCoreEffects
#[async_trait]
impl CryptoCoreEffects for AuraEffectSystem {
    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler
            .hkdf_derive(ikm, salt, info, output_len)
            .await
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &crypto::KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler.derive_key(master_key, context).await
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        self.crypto_handler.ed25519_generate_keypair().await
    }

    async fn ed25519_sign(
        &self,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler.ed25519_sign(message, private_key).await
    }

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto_handler
            .ed25519_verify(message, signature, public_key)
            .await
    }

    fn is_simulated(&self) -> bool {
        self.crypto_handler.is_simulated()
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        self.crypto_handler.crypto_capabilities()
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.crypto_handler.constant_time_eq(a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        self.crypto_handler.secure_zero(data)
    }

}

// Implementation of CryptoExtendedEffects
#[async_trait]
impl CryptoExtendedEffects for AuraEffectSystem {
    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<crypto::FrostKeyGenResult, CryptoError> {
        self.crypto_handler
            .frost_generate_keys(threshold, max_signers)
            .await
    }

    async fn frost_generate_nonces(&self, key_package: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler.frost_generate_nonces(key_package).await
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
        public_key_package: &[u8],
    ) -> Result<FrostSigningPackage, CryptoError> {
        self.crypto_handler
            .frost_create_signing_package(message, nonces, participants, public_key_package)
            .await
    }

    async fn frost_sign_share(
        &self,
        signing_package: &crypto::FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler
            .frost_sign_share(signing_package, key_share, nonces)
            .await
    }

    async fn frost_aggregate_signatures(
        &self,
        signing_package: &crypto::FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler
            .frost_aggregate_signatures(signing_package, signature_shares)
            .await
    }

    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto_handler
            .frost_verify(message, signature, public_key)
            .await
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler.ed25519_public_key(private_key).await
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler
            .chacha20_encrypt(plaintext, key, nonce)
            .await
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler
            .chacha20_decrypt(ciphertext, key, nonce)
            .await
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler
            .aes_gcm_encrypt(plaintext, key, nonce)
            .await
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler
            .aes_gcm_decrypt(ciphertext, key, nonce)
            .await
    }

    async fn frost_rotate_keys(
        &self,
        old_shares: &[Vec<u8>],
        old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<crypto::FrostKeyGenResult, CryptoError> {
        self.crypto_handler
            .frost_rotate_keys(old_shares, old_threshold, new_threshold, new_max_signers)
            .await
    }

    async fn generate_signing_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        self.crypto_handler
            .generate_signing_keys(threshold, max_signers)
            .await
    }

    async fn sign_with_key(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler
            .sign_with_key(message, key_package, mode)
            .await
    }

    async fn verify_signature(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key_package: &[u8],
        mode: SigningMode,
    ) -> Result<bool, CryptoError> {
        self.crypto_handler
            .verify_signature(message, signature, public_key_package, mode)
            .await
    }
}

// Implementation of NetworkEffects
#[async_trait]
impl NetworkCoreEffects for AuraEffectSystem {
    async fn send_to_peer(
        &self,
        _peer_id: uuid::Uuid,
        _message: Vec<u8>,
    ) -> Result<(), NetworkError> {
        // Mock implementation
        Ok(())
    }

    async fn broadcast(&self, _message: Vec<u8>) -> Result<(), NetworkError> {
        // Mock implementation
        Ok(())
    }

    async fn receive(&self) -> Result<(uuid::Uuid, Vec<u8>), NetworkError> {
        // Mock implementation - return empty data
        Err(NetworkError::NoMessage)
    }
}

#[async_trait]
impl NetworkExtendedEffects for AuraEffectSystem {
    async fn receive_from(&self, _peer_id: uuid::Uuid) -> Result<Vec<u8>, NetworkError> {
        // Mock implementation
        Err(NetworkError::NoMessage)
    }

    async fn connected_peers(&self) -> Vec<uuid::Uuid> {
        // Mock implementation
        vec![]
    }

    async fn is_peer_connected(&self, _peer_id: uuid::Uuid) -> bool {
        // Mock implementation
        false
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        // Mock implementation
        Err(NetworkError::NotImplemented)
    }

    async fn open(&self, _address: &str) -> Result<String, NetworkError> {
        // Mock implementation
        Err(NetworkError::NotImplemented)
    }

    async fn send(&self, _connection_id: &str, _data: Vec<u8>) -> Result<(), NetworkError> {
        // Mock implementation
        Err(NetworkError::NotImplemented)
    }

    async fn close(&self, _connection_id: &str) -> Result<(), NetworkError> {
        // Mock implementation
        Ok(())
    }
}

// Implementation of StorageEffects
#[async_trait]
impl StorageCoreEffects for AuraEffectSystem {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        self.storage_handler.store(key, value).await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.storage_handler.retrieve(key).await
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        self.storage_handler.remove(key).await
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        self.storage_handler.list_keys(prefix).await
    }

}

#[async_trait]
impl StorageExtendedEffects for AuraEffectSystem {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        self.storage_handler.exists(key).await
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        self.storage_handler.store_batch(pairs).await
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        self.storage_handler.retrieve_batch(keys).await
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        self.storage_handler.clear_all().await
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        self.storage_handler.stats().await
    }
}

// Implementation of SecureStorageEffects
#[async_trait]
impl SecureStorageEffects for AuraEffectSystem {
    async fn secure_store(
        &self,
        location: &SecureStorageLocation,
        key: &[u8],
        caps: &[SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        self.secure_storage_handler
            .secure_store(location, key, caps)
            .await
    }

    async fn secure_retrieve(
        &self,
        location: &SecureStorageLocation,
        caps: &[SecureStorageCapability],
    ) -> Result<Vec<u8>, SecureStorageError> {
        self.secure_storage_handler
            .secure_retrieve(location, caps)
            .await
    }

    async fn secure_delete(
        &self,
        location: &SecureStorageLocation,
        caps: &[SecureStorageCapability],
    ) -> Result<(), SecureStorageError> {
        self.secure_storage_handler
            .secure_delete(location, caps)
            .await
    }

    async fn secure_exists(
        &self,
        location: &SecureStorageLocation,
    ) -> Result<bool, SecureStorageError> {
        self.secure_storage_handler.secure_exists(location).await
    }

    async fn secure_list_keys(
        &self,
        namespace: &str,
        caps: &[SecureStorageCapability],
    ) -> Result<Vec<String>, SecureStorageError> {
        self.secure_storage_handler
            .secure_list_keys(namespace, caps)
            .await
    }

    async fn secure_generate_key(
        &self,
        location: &SecureStorageLocation,
        context: &str,
        caps: &[SecureStorageCapability],
    ) -> Result<Option<Vec<u8>>, SecureStorageError> {
        self.secure_storage_handler
            .secure_generate_key(location, context, caps)
            .await
    }

    async fn secure_create_time_bound_token(
        &self,
        location: &SecureStorageLocation,
        caps: &[SecureStorageCapability],
        expires_at: &aura_core::time::PhysicalTime,
    ) -> Result<Vec<u8>, SecureStorageError> {
        self.secure_storage_handler
            .secure_create_time_bound_token(location, caps, expires_at)
            .await
    }

    async fn secure_access_with_token(
        &self,
        token: &[u8],
        location: &SecureStorageLocation,
    ) -> Result<Vec<u8>, SecureStorageError> {
        self.secure_storage_handler
            .secure_access_with_token(token, location)
            .await
    }

    async fn get_device_attestation(&self) -> Result<Vec<u8>, SecureStorageError> {
        self.secure_storage_handler.get_device_attestation().await
    }

    async fn is_secure_storage_available(&self) -> bool {
        self.secure_storage_handler
            .is_secure_storage_available()
            .await
    }

    fn get_secure_storage_capabilities(&self) -> Vec<String> {
        self.secure_storage_handler
            .get_secure_storage_capabilities()
    }
}

// Implementation of ThresholdSigningEffects
#[async_trait]
impl aura_core::effects::ThresholdSigningEffects for AuraEffectSystem {
    async fn bootstrap_authority(&self, authority: &AuthorityId) -> Result<Vec<u8>, AuraError> {
        // Generate 1-of-1 signing keys (uses Ed25519 for single-signer mode)
        let signing_keys = self.crypto_handler.generate_signing_keys(1, 1).await?;

        // Store key package in secure storage
        // Location varies by mode: signing_keys/ for Ed25519, frost_keys/ for FROST
        let key_prefix = match signing_keys.mode {
            SigningMode::SingleSigner => "signing_keys",
            SigningMode::Threshold => "frost_keys",
        };
        let location = SecureStorageLocation::with_sub_key(
            key_prefix,
            format!("{}/0", authority), // epoch 0
            "1",                        // signer index 1
        );
        let caps = vec![SecureStorageCapability::Write];
        self.secure_storage_handler
            .secure_store(&location, &signing_keys.key_packages[0], &caps)
            .await?;

        // Store public key package
        let pub_location = SecureStorageLocation::new(
            format!("{}_public", key_prefix),
            format!("{}/0", authority),
        );
        self.secure_storage_handler
            .secure_store(&pub_location, &signing_keys.public_key_package, &caps)
            .await?;

        // Store threshold metadata for epoch 0 (bootstrap case: 1-of-1 single signer)
        self.store_threshold_metadata(
            authority,
            0,   // epoch 0
            1,   // threshold
            1,   // total_participants
            &[], // 1-of-1 bootstrap: participant set is implicit (local signer)
        )
        .await?;

        Ok(signing_keys.public_key_package)
    }

    async fn sign(
        &self,
        context: aura_core::threshold::SigningContext,
    ) -> Result<aura_core::threshold::ThresholdSignature, AuraError> {
        // Serialize the operation for signing
        let message = serde_json::to_vec(&context.operation)
            .map_err(|e| AuraError::internal(format!("Failed to serialize operation: {}", e)))?;

        // Load key package from secure storage using tracked epoch
        let current_epoch = self.get_current_epoch(&context.authority).await;
        let location = SecureStorageLocation::with_sub_key(
            "frost_keys",
            format!("{}/{}", context.authority, current_epoch),
            "1",
        );
        let caps = vec![SecureStorageCapability::Read];
        let key_package = self
            .secure_storage_handler
            .secure_retrieve(&location, &caps)
            .await?;

        // Load public key package for current epoch
        let pub_location = SecureStorageLocation::new(
            "frost_public_keys",
            format!("{}/{}", context.authority, current_epoch),
        );
        let public_key_package = self
            .secure_storage_handler
            .secure_retrieve(&pub_location, &caps)
            .await
            .unwrap_or_else(|_| vec![0u8; 32]); // Fallback for bootstrapped authorities

        // Generate nonces
        let nonces = self
            .crypto_handler
            .frost_generate_nonces(&key_package)
            .await
            .map_err(|e| AuraError::internal(format!("Nonce generation failed: {}", e)))?;

        // Create signing package (single participant)
        let participants = vec![1u16];
        let signing_package = self
            .crypto_handler
            .frost_create_signing_package(
                &message,
                std::slice::from_ref(&nonces),
                &participants,
                &public_key_package,
            )
            .await
            .map_err(|e| AuraError::internal(format!("Signing package creation failed: {}", e)))?;

        // Sign
        let share = self
            .crypto_handler
            .frost_sign_share(&signing_package, &key_package, &nonces)
            .await
            .map_err(|e| AuraError::internal(format!("Signature share creation failed: {}", e)))?;

        // Aggregate (trivial for single signer)
        let signature = self
            .crypto_handler
            .frost_aggregate_signatures(&signing_package, &[share])
            .await
            .map_err(|e| AuraError::internal(format!("Signature aggregation failed: {}", e)))?;

        Ok(aura_core::threshold::ThresholdSignature::single_signer(
            signature,
            public_key_package,
            current_epoch,
        ))
    }

    async fn threshold_config(
        &self,
        authority: &AuthorityId,
    ) -> Option<aura_core::threshold::ThresholdConfig> {
        // Get current epoch for this authority
        let current_epoch = self.get_current_epoch(authority).await;

        // Try to retrieve stored threshold metadata for this epoch
        if let Some(metadata) = self.get_threshold_metadata(authority, current_epoch).await {
            return Some(aura_core::threshold::ThresholdConfig {
                threshold: metadata.threshold,
                total_participants: metadata.total_participants,
            });
        }

        // Fallback: check if we have keys but no metadata (legacy bootstrap case)
        let location = SecureStorageLocation::with_sub_key(
            "frost_keys",
            format!("{}/{}", authority, current_epoch),
            "1",
        );
        if self
            .secure_storage_handler
            .secure_exists(&location)
            .await
            .unwrap_or(false)
        {
            // Legacy case: keys exist but no metadata - assume 1-of-1
            Some(aura_core::threshold::ThresholdConfig {
                threshold: 1,
                total_participants: 1,
            })
        } else {
            None
        }
    }

    async fn threshold_state(
        &self,
        authority: &AuthorityId,
    ) -> Option<aura_core::threshold::ThresholdState> {
        // Get current epoch for this authority
        let current_epoch = self.get_current_epoch(authority).await;

        // Try to retrieve stored threshold metadata for this epoch
        if let Some(metadata) = self.get_threshold_metadata(authority, current_epoch).await {
            return Some(aura_core::threshold::ThresholdState {
                epoch: metadata.epoch,
                threshold: metadata.threshold,
                total_participants: metadata.total_participants,
                participants: metadata.resolved_participants(),
            });
        }

        // Fallback: check if we have keys but no metadata (legacy bootstrap case)
        let location = SecureStorageLocation::with_sub_key(
            "frost_keys",
            format!("{}/{}", authority, current_epoch),
            "1",
        );
        if self
            .secure_storage_handler
            .secure_exists(&location)
            .await
            .unwrap_or(false)
        {
            // Legacy case: keys exist but no metadata - return minimal state
            Some(aura_core::threshold::ThresholdState {
                epoch: current_epoch,
                threshold: 1,
                total_participants: 1,
                participants: Vec::new(),
            })
        } else {
            None
        }
    }

    async fn has_signing_capability(&self, authority: &AuthorityId) -> bool {
        let current_epoch = self.get_current_epoch(authority).await;
        let location = SecureStorageLocation::with_sub_key(
            "frost_keys",
            format!("{}/{}", authority, current_epoch),
            "1",
        );
        self.secure_storage_handler
            .secure_exists(&location)
            .await
            .unwrap_or(false)
    }

    async fn public_key_package(&self, authority: &AuthorityId) -> Option<Vec<u8>> {
        let location = SecureStorageLocation::new("frost_public_keys", format!("{}/0", authority));
        let caps = vec![SecureStorageCapability::Read];
        self.secure_storage_handler
            .secure_retrieve(&location, &caps)
            .await
            .ok()
    }

    async fn rotate_keys(
        &self,
        authority: &AuthorityId,
        new_threshold: u16,
        new_total_participants: u16,
        participants: &[aura_core::threshold::ParticipantIdentity],
    ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), AuraError> {
        tracing::info!(
            ?authority,
            new_threshold,
            new_total_participants,
            num_participants = participants.len(),
            "Rotating threshold keys via AuraEffectSystem"
        );

        // Validate inputs
        if participants.len() != new_total_participants as usize {
            return Err(AuraError::invalid(format!(
                "Participant count ({}) must match total_participants ({})",
                participants.len(),
                new_total_participants
            )));
        }

        // Get current epoch and calculate new epoch
        let current_epoch = self.get_current_epoch(authority).await;
        let new_epoch = current_epoch + 1;
        tracing::debug!(
            ?authority,
            current_epoch,
            new_epoch,
            "Rotating keys from epoch {} to {}",
            current_epoch,
            new_epoch
        );

        // Generate new threshold keys
        let key_result = if new_threshold >= 2 {
            self.crypto_handler
                .frost_rotate_keys(&[], 0, new_threshold, new_total_participants)
                .await?
        } else {
            let result = self
                .crypto_handler
                .generate_signing_keys(new_threshold, new_total_participants)
                .await?;
            aura_core::effects::crypto::FrostKeyGenResult {
                key_packages: result.key_packages,
                public_key_package: result.public_key_package,
            }
        };

        // Store guardian key packages
        let caps = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        for (participant, key_package) in participants.iter().zip(key_result.key_packages.iter()) {
            let location = SecureStorageLocation::with_sub_key(
                "participant_shares",
                format!("{}/{}", authority, new_epoch),
                participant.storage_key(),
            );
            self.secure_storage_handler
                .secure_store(&location, key_package, &caps)
                .await?;
        }

        // Store public key package
        let pub_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority),
            format!("{}", new_epoch),
        );
        self.secure_storage_handler
            .secure_store(&pub_location, &key_result.public_key_package, &caps)
            .await?;

        // Store threshold metadata for the new epoch
        self.store_threshold_metadata(
            authority,
            new_epoch,
            new_threshold,
            new_total_participants,
            participants,
        )
        .await?;

        Ok((
            new_epoch,
            key_result.key_packages,
            key_result.public_key_package,
        ))
    }

    async fn commit_key_rotation(
        &self,
        authority: &AuthorityId,
        new_epoch: u64,
    ) -> Result<(), AuraError> {
        tracing::info!(
            ?authority,
            new_epoch,
            "Committing key rotation via AuraEffectSystem"
        );
        // Activate the new epoch by updating the current epoch state
        self.set_current_epoch(authority, new_epoch).await?;
        tracing::debug!(
            ?authority,
            new_epoch,
            "Epoch state updated - new keys are now active"
        );
        Ok(())
    }

    async fn rollback_key_rotation(
        &self,
        authority: &AuthorityId,
        failed_epoch: u64,
    ) -> Result<(), AuraError> {
        tracing::warn!(
            ?authority,
            failed_epoch,
            "Rolling back key rotation via AuraEffectSystem"
        );
        // Delete orphaned keys from the failed epoch to prevent storage leakage
        self.delete_epoch_keys(authority, failed_epoch).await?;
        tracing::info!(
            ?authority,
            failed_epoch,
            "Successfully deleted orphaned keys from failed rotation"
        );
        Ok(())
    }
}

// Time helper implementations (compat)
#[async_trait]
// Implementation of ConsoleEffects
#[async_trait]
impl ConsoleEffects for AuraEffectSystem {
    async fn log_info(&self, message: &str) -> Result<(), AuraError> {
        tracing::info!("{}", message);
        Ok(())
    }

    async fn log_warn(&self, message: &str) -> Result<(), AuraError> {
        tracing::warn!("{}", message);
        Ok(())
    }

    async fn log_error(&self, message: &str) -> Result<(), AuraError> {
        tracing::error!("{}", message);
        Ok(())
    }

    async fn log_debug(&self, message: &str) -> Result<(), AuraError> {
        tracing::debug!("{}", message);
        Ok(())
    }
}

// Implementation of JournalEffects
#[async_trait]
impl JournalEffects for AuraEffectSystem {
    async fn merge_facts(&self, target: &Journal, delta: &Journal) -> Result<Journal, AuraError> {
        self.journal_handler().merge_facts(target, delta).await
    }

    async fn refine_caps(
        &self,
        target: &Journal,
        refinement: &Journal,
    ) -> Result<Journal, AuraError> {
        self.journal_handler().refine_caps(target, refinement).await
    }

    async fn get_journal(&self) -> Result<Journal, AuraError> {
        self.journal_handler().get_journal().await
    }

    async fn persist_journal(&self, journal: &Journal) -> Result<(), AuraError> {
        // Persist the journal to storage
        self.journal_handler().persist_journal(journal).await?;

        // Index all facts for efficient lookup (B-tree, Bloom filter, Merkle tree)
        let timestamp = aura_core::time::TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
            ts_ms: self.time_handler.physical_time_now_ms(),
            uncertainty: None,
        });
        for (predicate, value) in journal.facts.iter() {
            self.indexed_journal.add_fact(
                predicate.clone(),
                value.clone(),
                Some(self.authority_id),
                Some(timestamp.clone()),
            );
        }

        Ok(())
    }

    async fn get_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
    ) -> Result<FlowBudget, AuraError> {
        self.journal_handler()
            .get_flow_budget(_context, _peer)
            .await
    }

    async fn update_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError> {
        self.journal_handler()
            .update_flow_budget(_context, _peer, budget)
            .await
    }

    async fn charge_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
        _cost: u32,
    ) -> Result<FlowBudget, AuraError> {
        self.journal_handler()
            .charge_flow_budget(_context, _peer, _cost)
            .await
    }
}

// Implementation of IndexedJournalEffects - provides B-tree indexes, Bloom filters, Merkle trees
#[async_trait]
impl IndexedJournalEffects for AuraEffectSystem {
    fn watch_facts(&self) -> Box<dyn indexed::FactStreamReceiver> {
        self.indexed_journal.watch_facts()
    }

    async fn facts_by_predicate(
        &self,
        predicate: &str,
    ) -> Result<Vec<indexed::IndexedFact>, AuraError> {
        self.indexed_journal.facts_by_predicate(predicate).await
    }

    async fn facts_by_authority(
        &self,
        authority: &AuthorityId,
    ) -> Result<Vec<indexed::IndexedFact>, AuraError> {
        self.indexed_journal.facts_by_authority(authority).await
    }

    async fn facts_in_range(
        &self,
        start: aura_core::time::TimeStamp,
        end: aura_core::time::TimeStamp,
    ) -> Result<Vec<indexed::IndexedFact>, AuraError> {
        self.indexed_journal.facts_in_range(start, end).await
    }

    async fn all_facts(&self) -> Result<Vec<indexed::IndexedFact>, AuraError> {
        self.indexed_journal.all_facts().await
    }

    fn might_contain(
        &self,
        predicate: &str,
        value: &aura_core::domain::journal::FactValue,
    ) -> bool {
        self.indexed_journal.might_contain(predicate, value)
    }

    async fn merkle_root(&self) -> Result<[u8; 32], AuraError> {
        self.indexed_journal.merkle_root().await
    }

    async fn verify_fact_inclusion(&self, fact: &indexed::IndexedFact) -> Result<bool, AuraError> {
        self.indexed_journal.verify_fact_inclusion(fact).await
    }

    async fn get_bloom_filter(&self) -> Result<BloomFilter, AuraError> {
        self.indexed_journal.get_bloom_filter().await
    }

    async fn index_stats(&self) -> Result<indexed::IndexStats, AuraError> {
        self.indexed_journal.index_stats().await
    }
}

// Implementation of SystemEffects
#[async_trait]
impl SystemEffects for AuraEffectSystem {
    async fn shutdown(&self) -> Result<(), SystemError> {
        // Mock implementation
        Ok(())
    }

    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError> {
        // Mock implementation
        let mut info = HashMap::new();
        info.insert("version".to_string(), "0.1.0".to_string());
        info.insert("build_time".to_string(), "mock".to_string());
        info.insert("commit_hash".to_string(), "mock".to_string());
        info.insert("platform".to_string(), "test".to_string());
        Ok(info)
    }

    async fn log(&self, level: &str, component: &str, message: &str) -> Result<(), SystemError> {
        // Use tracing instead of println to avoid corrupting TUI
        match level.to_lowercase().as_str() {
            "error" => tracing::error!(component = component, "{}", message),
            "warn" => tracing::warn!(component = component, "{}", message),
            "debug" => tracing::debug!(component = component, "{}", message),
            "trace" => tracing::trace!(component = component, "{}", message),
            _ => tracing::info!(component = component, "{}", message),
        }
        Ok(())
    }

    async fn log_with_context(
        &self,
        level: &str,
        component: &str,
        message: &str,
        _context: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        // Use tracing instead of println to avoid corrupting TUI
        match level.to_lowercase().as_str() {
            "error" => tracing::error!(component = component, "{}", message),
            "warn" => tracing::warn!(component = component, "{}", message),
            "debug" => tracing::debug!(component = component, "{}", message),
            "trace" => tracing::trace!(component = component, "{}", message),
            _ => tracing::info!(component = component, "{}", message),
        }
        Ok(())
    }

    async fn set_config(&self, _key: &str, _value: &str) -> Result<(), SystemError> {
        // Mock implementation
        Ok(())
    }

    async fn get_config(&self, _key: &str) -> Result<String, SystemError> {
        // Mock implementation
        Ok("mock_value".to_string())
    }

    async fn health_check(&self) -> Result<bool, SystemError> {
        // Mock implementation
        Ok(true)
    }

    async fn get_metrics(&self) -> Result<HashMap<String, f64>, SystemError> {
        // Mock implementation
        Ok(HashMap::new())
    }

    async fn restart_component(&self, _component: &str) -> Result<(), SystemError> {
        // Mock implementation
        Ok(())
    }
}

// Implementation of ChoreographicEffects
#[async_trait]
impl ChoreographicEffects for AuraEffectSystem {
    async fn send_to_role_bytes(
        &self,
        _role: ChoreographicRole,
        _message: Vec<u8>,
    ) -> Result<(), ChoreographyError> {
        // Mock implementation
        Ok(())
    }

    async fn receive_from_role_bytes(
        &self,
        _role: ChoreographicRole,
    ) -> Result<Vec<u8>, ChoreographyError> {
        // Mock implementation
        Ok(vec![])
    }

    async fn broadcast_bytes(&self, _message: Vec<u8>) -> Result<(), ChoreographyError> {
        // Mock implementation
        Ok(())
    }

    #[allow(clippy::disallowed_methods)]
    fn current_role(&self) -> ChoreographicRole {
        // Mock implementation
        ChoreographicRole::new(uuid::Uuid::new_v4(), 0)
    }

    fn all_roles(&self) -> Vec<ChoreographicRole> {
        // Mock implementation
        vec![]
    }

    async fn is_role_active(&self, _role: ChoreographicRole) -> bool {
        // Mock implementation
        true
    }

    async fn start_session(
        &self,
        _session_id: uuid::Uuid,
        _roles: Vec<ChoreographicRole>,
    ) -> Result<(), ChoreographyError> {
        // Mock implementation
        Ok(())
    }

    async fn end_session(&self) -> Result<(), ChoreographyError> {
        // Mock implementation
        Ok(())
    }

    async fn emit_choreo_event(&self, _event: ChoreographyEvent) -> Result<(), ChoreographyError> {
        // Mock implementation
        Ok(())
    }

    async fn set_timeout(&self, _timeout_ms: u64) {
        // Mock implementation - no return value
    }

    async fn get_metrics(&self) -> ChoreographyMetrics {
        // Mock implementation
        ChoreographyMetrics {
            messages_sent: 0,
            messages_received: 0,
            avg_latency_ms: 0.0,
            timeout_count: 0,
            retry_count: 0,
            total_duration_ms: 0,
        }
    }
}

// Implementation of EffectApiEffects
#[async_trait]
impl EffectApiEffects for AuraEffectSystem {
    async fn append_event(&self, _event: Vec<u8>) -> Result<(), EffectApiError> {
        // Mock implementation
        Ok(())
    }

    async fn current_epoch(&self) -> Result<u64, EffectApiError> {
        // Mock implementation
        Ok(0)
    }

    async fn events_since(&self, _epoch: u64) -> Result<Vec<Vec<u8>>, EffectApiError> {
        // Mock implementation
        Ok(vec![])
    }

    async fn is_device_authorized(
        &self,
        _device_id: DeviceId,
        _operation: &str,
    ) -> Result<bool, EffectApiError> {
        // Mock implementation
        Ok(true)
    }

    async fn update_device_activity(&self, _device_id: DeviceId) -> Result<(), EffectApiError> {
        // Mock implementation
        Ok(())
    }

    async fn subscribe_to_events(&self) -> Result<EffectApiEventStream, EffectApiError> {
        // Mock implementation
        Err(EffectApiError::CryptoOperationFailed {
            message: "subscribe_to_events not implemented in mock".to_string(),
        })
    }

    async fn would_create_cycle(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _new_edge: (Vec<u8>, Vec<u8>),
    ) -> Result<bool, EffectApiError> {
        // Mock implementation
        Ok(false)
    }

    async fn find_connected_components(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<Vec<u8>>>, EffectApiError> {
        // Mock implementation
        Ok(vec![])
    }

    async fn topological_sort(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<u8>>, EffectApiError> {
        // Mock implementation
        Ok(vec![])
    }

    async fn shortest_path(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _start: Vec<u8>,
        _end: Vec<u8>,
    ) -> Result<Option<Vec<Vec<u8>>>, EffectApiError> {
        // Mock implementation
        Ok(None)
    }

    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, EffectApiError> {
        // Mock implementation
        Ok(vec![0u8; length])
    }

    async fn hash_data(&self, data: &[u8]) -> Result<[u8; 32], EffectApiError> {
        // Mock implementation - simple hash
        use aura_core::hash::hash;
        Ok(hash(data))
    }

    async fn current_timestamp(&self) -> Result<u64, EffectApiError> {
        // Use PhysicalTimeEffects instead of direct SystemTime
        let physical_time =
            self.time_handler
                .physical_time()
                .await
                .map_err(|e| EffectApiError::Backend {
                    error: format!("time unavailable: {e}"),
                })?;
        Ok(physical_time.ts_ms / 1000)
    }

    async fn effect_api_device_id(&self) -> Result<DeviceId, EffectApiError> {
        // Mock implementation
        Ok(DeviceId::new())
    }

    #[allow(clippy::disallowed_methods)]
    async fn new_uuid(&self) -> Result<uuid::Uuid, EffectApiError> {
        // Mock implementation
        Ok(uuid::Uuid::new_v4())
    }
}

// Implementation of FlowBudgetEffects
#[async_trait]
impl FlowBudgetEffects for AuraEffectSystem {
    async fn charge_flow(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> aura_core::AuraResult<aura_core::Receipt> {
        self.flow_budget.charge_flow(context, peer, cost).await
    }
}

// Implementation of AuraEffects (composite trait)
#[async_trait]
impl AuraEffects for AuraEffectSystem {
    fn execution_mode(&self) -> aura_core::effects::ExecutionMode {
        // Mock implementation based on configuration
        if self.config.is_simulation() {
            aura_core::effects::ExecutionMode::Simulation { seed: 42 }
        } else {
            aura_core::effects::ExecutionMode::Production
        }
    }
}

impl GuardContextProvider for AuraEffectSystem {
    fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    fn get_metadata(&self, key: &str) -> Option<String> {
        match key {
            "authority_id" => Some(self.authority_id.to_string()),
            "execution_mode" => Some(format!("{:?}", self.execution_mode())),
            "device_id" => Some(self.config.device_id().to_string()),
            _ => None,
        }
    }

    fn execution_mode(&self) -> aura_core::effects::ExecutionMode {
        AuraEffects::execution_mode(self)
    }

    fn can_perform_operation(&self, _operation: &str) -> bool {
        true
    }
}

#[async_trait::async_trait]
impl AmpChannelEffects for AuraEffectSystem {
    async fn create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> Result<ChannelId, AmpChannelError> {
        let channel = if let Some(id) = params.channel {
            id
        } else {
            let bytes = self.random_bytes(32).await;
            ChannelId::from_bytes(hash(&bytes))
        };

        let window = params.skip_window.unwrap_or(DEFAULT_WINDOW);

        let checkpoint = aura_journal::fact::ChannelCheckpoint {
            context: params.context,
            channel,
            chan_epoch: 0,
            base_gen: 0,
            window,
            ck_commitment: Default::default(),
            skip_window_override: Some(window),
        };

        self.insert_relational_fact(aura_journal::fact::RelationalFact::AmpChannelCheckpoint(
            checkpoint,
        ))
        .await
        .map_err(map_amp_err)?;

        if params.topic.is_some() || params.skip_window.is_some() {
            let policy = aura_journal::fact::ChannelPolicy {
                context: params.context,
                channel,
                skip_window: params.skip_window.or(Some(window)),
            };
            self.insert_relational_fact(aura_journal::fact::RelationalFact::AmpChannelPolicy(
                policy,
            ))
            .await
            .map_err(map_amp_err)?;
        }
        Ok(channel)
    }

    async fn close_channel(&self, params: ChannelCloseParams) -> Result<(), AmpChannelError> {
        let state = aura_protocol::amp::get_channel_state(self, params.context, params.channel)
            .await
            .map_err(map_amp_err)?;
        let committed = aura_journal::fact::CommittedChannelEpochBump {
            context: params.context,
            channel: params.channel,
            parent_epoch: state.chan_epoch,
            new_epoch: state.chan_epoch + 1,
            chosen_bump_id: Default::default(),
            consensus_id: Default::default(),
        };

        self.insert_relational_fact(
            aura_journal::fact::RelationalFact::AmpCommittedChannelEpochBump(committed),
        )
        .await
        .map_err(map_amp_err)?;

        let policy = aura_journal::fact::ChannelPolicy {
            context: params.context,
            channel: params.channel,
            skip_window: Some(0),
        };

        self.insert_relational_fact(aura_journal::fact::RelationalFact::AmpChannelPolicy(policy))
            .await
            .map_err(map_amp_err)?;

        Ok(())
    }

    async fn join_channel(&self, params: ChannelJoinParams) -> Result<(), AmpChannelError> {
        aura_protocol::amp::get_channel_state(self, params.context, params.channel)
            .await
            .map_err(map_amp_err)?;
        let timestamp = ChannelMembershipFact::random_timestamp(self).await;
        let membership = ChannelMembershipFact::new(
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Joined,
            timestamp,
        );
        self.insert_relational_fact(membership.to_generic())
            .await
            .map_err(map_amp_err)?;

        tracing::debug!(
            "Participant {:?} joined channel {:?} in context {:?}",
            params.participant,
            params.channel,
            params.context
        );

        Ok(())
    }

    async fn leave_channel(&self, params: ChannelLeaveParams) -> Result<(), AmpChannelError> {
        aura_protocol::amp::get_channel_state(self, params.context, params.channel)
            .await
            .map_err(map_amp_err)?;
        let timestamp = ChannelMembershipFact::random_timestamp(self).await;
        let membership = ChannelMembershipFact::new(
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Left,
            timestamp,
        );
        self.insert_relational_fact(membership.to_generic())
            .await
            .map_err(map_amp_err)?;

        tracing::debug!(
            "Participant {:?} left channel {:?} in context {:?}",
            params.participant,
            params.channel,
            params.context
        );

        Ok(())
    }

    async fn send_message(
        &self,
        params: ChannelSendParams,
    ) -> Result<AmpCiphertext, AmpChannelError> {
        let state = aura_protocol::amp::get_channel_state(self, params.context, params.channel)
            .await
            .map_err(map_amp_err)?;

        let header = AmpHeader {
            context: params.context,
            channel: params.channel,
            chan_epoch: state.chan_epoch,
            ratchet_gen: 0,
        };

        let cipher = AmpCiphertext {
            header,
            ciphertext: params.plaintext.clone(),
        };

        Ok(cipher)
    }
}

// AuthorizationEffects implementation delegating to the handler
#[async_trait]
impl AuthorizationEffects for AuraEffectSystem {
    async fn verify_capability(
        &self,
        capabilities: &aura_core::Cap,
        operation: &str,
        resource: &str,
    ) -> Result<bool, aura_core::effects::AuthorizationError> {
        self.authorization_handler
            .verify_capability(capabilities, operation, resource)
            .await
    }

    async fn delegate_capabilities(
        &self,
        source_capabilities: &aura_core::Cap,
        requested_capabilities: &aura_core::Cap,
        target_authority: &AuthorityId,
    ) -> Result<aura_core::Cap, aura_core::effects::AuthorizationError> {
        self.authorization_handler
            .delegate_capabilities(
                source_capabilities,
                requested_capabilities,
                target_authority,
            )
            .await
    }
}

// LeakageEffects implementation delegating to the handler
#[async_trait]
impl LeakageEffects for AuraEffectSystem {
    async fn record_leakage(
        &self,
        event: aura_core::effects::LeakageEvent,
    ) -> aura_core::Result<()> {
        self.leakage_handler.record_leakage(event).await
    }

    async fn get_leakage_budget(
        &self,
        context_id: aura_core::identifiers::ContextId,
    ) -> aura_core::Result<aura_core::effects::LeakageBudget> {
        self.leakage_handler.get_leakage_budget(context_id).await
    }

    async fn check_leakage_budget(
        &self,
        context_id: aura_core::identifiers::ContextId,
        observer: aura_core::effects::ObserverClass,
        amount: u64,
    ) -> aura_core::Result<bool> {
        self.leakage_handler
            .check_leakage_budget(context_id, observer, amount)
            .await
    }

    async fn get_leakage_history(
        &self,
        context_id: aura_core::identifiers::ContextId,
        since_timestamp: Option<&aura_core::time::PhysicalTime>,
    ) -> aura_core::Result<Vec<aura_core::effects::LeakageEvent>> {
        self.leakage_handler
            .get_leakage_history(context_id, since_timestamp)
            .await
    }
}

// ============================================================================
// RuntimeEffectsBundle Implementation (for simulator decoupling)
// ============================================================================

#[cfg(feature = "simulation")]
impl aura_core::effects::RuntimeEffectsBundle for AuraEffectSystem {
    fn is_simulation_mode(&self) -> bool {
        self.test_mode
    }

    fn simulation_seed(&self) -> Option<u64> {
        // The seed is not stored after construction, so we return None
        // In practice, determinism is achieved through seeded RNG at construction time
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::ContextId;
    use aura_protocol::amp::AmpJournalEffects;
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
        assert_eq!(state.epoch, 0); // fresh tree starts at epoch 0
        assert_eq!(state.root_commitment, [0u8; 32]);

        // Sync digest should not error
        let digest = effect_system.get_oplog_digest().await.unwrap();
        assert_eq!(digest.cids.len(), 0);
    }
}

// Note: RelationshipFormationEffects is a composite trait that is automatically implemented
// when all required component traits are implemented: ConsoleEffects, CryptoEffects,
// NetworkEffects, RandomEffects, and JournalEffects

/// Execution mode for the effect system
#[derive(Debug, Clone)]
pub enum ExecutionMode {
    /// Production mode with real implementations
    Production,
    /// Simulation mode with controllable behavior
    Simulation { seed: u64 },
    /// Test mode with mock implementations
    Test,
}

impl AuraEffectSystem {
    /// Determine execution mode based on configuration
    pub fn execution_mode(&self) -> ExecutionMode {
        if self.config.is_simulation() {
            ExecutionMode::Simulation { seed: 42 }
        } else {
            ExecutionMode::Production
        }
    }

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
            .secure_storage_handler
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
        self.secure_storage_handler
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
            .secure_storage_handler
            .secure_delete(&shares_location, &delete_caps)
            .await;

        // Best-effort cleanup of legacy guardian shares for this epoch
        let legacy_shares_location =
            SecureStorageLocation::new("guardian_shares", format!("{}/{}", authority, epoch));
        let _ = self
            .secure_storage_handler
            .secure_delete(&legacy_shares_location, &delete_caps)
            .await;

        // Delete public key for this epoch
        let pubkey_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority),
            format!("{}", epoch),
        );
        let _ = self
            .secure_storage_handler
            .secure_delete(&pubkey_location, &delete_caps)
            .await;

        // Delete threshold metadata for this epoch
        let metadata_location = SecureStorageLocation::with_sub_key(
            "threshold_metadata",
            format!("{}", authority),
            format!("{}", epoch),
        );
        let _ = self
            .secure_storage_handler
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
    ) -> Result<(), AuraError> {
        let metadata = ThresholdMetadata {
            epoch,
            threshold,
            total_participants,
            participants: participants.to_vec(),
            guardian_ids: Vec::new(),
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
        self.secure_storage_handler
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
            .secure_storage_handler
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
    /// Legacy guardian IDs (for backward-compatible deserialization)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    guardian_ids: Vec<String>,
}

impl ThresholdMetadata {
    fn resolved_participants(&self) -> Vec<aura_core::threshold::ParticipantIdentity> {
        if !self.participants.is_empty() {
            return self.participants.clone();
        }

        self.guardian_ids
            .iter()
            .filter_map(|s| s.parse::<AuthorityId>().ok())
            .map(aura_core::threshold::ParticipantIdentity::guardian)
            .collect()
    }
}

fn map_amp_err(e: aura_core::AuraError) -> AmpChannelError {
    match e {
        aura_core::AuraError::NotFound { .. } => AmpChannelError::NotFound,
        aura_core::AuraError::PermissionDenied { .. } => AmpChannelError::Unauthorized,
        aura_core::AuraError::Storage { message } => AmpChannelError::Storage(message),
        aura_core::AuraError::Crypto { message } => AmpChannelError::Crypto(message),
        aura_core::AuraError::Invalid { message } => AmpChannelError::InvalidState(message),
        other => AmpChannelError::Internal(other.to_string()),
    }
}

// Manual Debug implementation since some fields don't implement Debug
impl std::fmt::Debug for AuraEffectSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuraEffectSystem")
            .field("config", &self.config)
            .field("authority_id", &self.authority_id)
            .field("journal_policy", &self.journal_policy.is_some())
            .field(
                "journal_verifying_key",
                &self.journal_verifying_key.is_some(),
            )
            .finish_non_exhaustive()
    }
}
