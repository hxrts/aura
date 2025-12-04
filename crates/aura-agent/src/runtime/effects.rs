//! Effect System Components
//!
//! Core effect system components per Layer-6 spec.

use crate::core::{AgentConfig, AgentResult};
use crate::fact_registry::build_fact_registry;
use async_trait::async_trait;
use aura_composition::CompositeHandlerAdapter;
use aura_core::effects::crypto::FrostSigningPackage;
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
    storage::FilesystemStorageHandler,
    time::{LogicalClockHandler, OrderClockHandler, PhysicalTimeHandler},
};
use aura_journal::commitment_tree::state::TreeState as JournalTreeState;
use aura_journal::extensibility::{DomainFact, FactRegistry};
use aura_protocol::amp::{AmpJournalEffects, ChannelMembershipFact, ChannelParticipantEvent};
use aura_protocol::effects::{
    AuraEffects, AuthorizationEffects, BloomDigest, ChoreographicEffects, ChoreographicRole,
    ChoreographyError, ChoreographyEvent, ChoreographyMetrics, EffectApiEffects, EffectApiError,
    EffectApiEventStream, LeakageEffects, SyncEffects, SyncError,
};
use aura_protocol::guards::GuardContextProvider;
use aura_protocol::handlers::{InMemoryTreeHandler, LocalSyncHandler};
use aura_wot::{BiscuitAuthorizationBridge, FlowBudgetHandler};
use biscuit_auth::{Biscuit, KeyPair, PublicKey};
use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

const DEFAULT_WINDOW: u32 = 1024;

/// Effect executor for dispatching effect calls
///
/// Note: This wraps aura-composition infrastructure for Layer 6 runtime concerns.
#[allow(dead_code)] // Part of future effect system API
pub struct EffectExecutor {
    config: AgentConfig,
    composite: CompositeHandlerAdapter,
}

impl EffectExecutor {
    /// Create new effect executor
    #[allow(dead_code)] // Part of future effect system API
    pub fn new(config: AgentConfig) -> Result<Self, crate::core::AgentError> {
        let device_id = config.device_id();
        let composite = CompositeHandlerAdapter::for_testing(device_id);
        Ok(Self { config, composite })
    }

    /// Create production effect executor
    #[allow(dead_code)] // Part of future effect system API
    pub fn production(config: AgentConfig) -> Result<Self, crate::core::AgentError> {
        let device_id = config.device_id();
        let composite = CompositeHandlerAdapter::for_production(device_id);
        Ok(Self { config, composite })
    }

    /// Create testing effect executor
    #[allow(dead_code)] // Part of future effect system API
    pub fn testing(config: AgentConfig) -> Result<Self, crate::core::AgentError> {
        let device_id = config.device_id();
        let composite = CompositeHandlerAdapter::for_testing(device_id);
        Ok(Self { config, composite })
    }

    /// Create simulation effect executor
    #[allow(dead_code)] // Part of future effect system API
    pub fn simulation(config: AgentConfig, seed: u64) -> Result<Self, crate::core::AgentError> {
        let device_id = config.device_id();
        let composite = CompositeHandlerAdapter::for_simulation(device_id, seed);
        Ok(Self { config, composite })
    }

    /// Dispatch effect call
    #[allow(dead_code)] // Part of future effect system API
    pub async fn execute<T>(&self, effect_call: T) -> AgentResult<T::Output>
    where
        T: EffectCall,
    {
        effect_call.execute(&self.config).await
    }
}

/// Trait for effect calls that can be executed
#[async_trait]
#[allow(dead_code)] // Part of future effect system API
pub trait EffectCall: Send + Sync {
    type Output;

    async fn execute(&self, config: &AgentConfig) -> AgentResult<Self::Output>;
}

/// Concrete effect system combining all effects for runtime usage
///
/// Note: This wraps aura-composition infrastructure for Layer 6 runtime concerns.
pub struct AuraEffectSystem {
    config: AgentConfig,
    composite: CompositeHandlerAdapter,
    flow_budget: FlowBudgetHandler,
    crypto_handler: aura_effects::crypto::RealCryptoHandler,
    storage_handler: aura_effects::storage::FilesystemStorageHandler,
    time_handler: PhysicalTimeHandler,
    logical_clock: LogicalClockHandler,
    order_clock: OrderClockHandler,
    authorization_handler:
        aura_wot::effects::WotAuthorizationHandler<aura_effects::crypto::RealCryptoHandler>,
    leakage_handler:
        aura_effects::leakage_handler::ProductionLeakageHandler<FilesystemStorageHandler>,
    journal_policy: Option<(biscuit_auth::Biscuit, aura_wot::BiscuitAuthorizationBridge)>,
    journal_verifying_key: Option<Vec<u8>>,
    authority_id: AuthorityId,
    tree_handler: InMemoryTreeHandler,
    sync_handler: LocalSyncHandler,
    transport_handler: aura_effects::transport::RealTransportHandler,
    transport_inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    transport_stats: Arc<RwLock<TransportStats>>,
    fact_registry: Arc<FactRegistry>,
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
    /// Internal helper that builds the effect system with the given composite handler.
    ///
    /// All factory methods delegate to this to avoid code duplication.
    fn build_internal(config: AgentConfig, composite: CompositeHandlerAdapter) -> Self {
        let authority = AuthorityId::from_uuid(config.device_id().0);
        let (journal_policy, journal_verifying_key) = Self::init_journal_policy(authority);
        let crypto_handler = RealCryptoHandler::new();
        let authorization_handler =
            Self::init_authorization_handler(authority, &crypto_handler, &journal_verifying_key);
        let storage_handler = FilesystemStorageHandler::new(config.storage.base_path.clone());
        let leakage_storage =
            FilesystemStorageHandler::new(config.storage.base_path.join("leakage"));
        let leakage_handler = aura_effects::leakage_handler::ProductionLeakageHandler::with_storage(
            Arc::new(leakage_storage),
        );
        let oplog = Arc::new(RwLock::new(Vec::new()));
        let tree_handler = InMemoryTreeHandler::new(oplog.clone());
        let sync_handler = LocalSyncHandler::new(oplog);
        let transport_handler = aura_effects::transport::RealTransportHandler::default();
        let transport_inbox = Arc::new(RwLock::new(Vec::new()));
        let transport_stats = Arc::new(RwLock::new(TransportStats::default()));

        Self {
            config,
            composite,
            flow_budget: FlowBudgetHandler::new(authority),
            crypto_handler,
            storage_handler,
            time_handler: PhysicalTimeHandler::new(),
            logical_clock: LogicalClockHandler::new(),
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
            transport_stats,
            fact_registry: Arc::new(build_fact_registry()),
        }
    }

    /// Create new effect system with configuration (testing mode).
    pub fn new(config: AgentConfig) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_testing(config.device_id());
        Ok(Self::build_internal(config, composite))
    }

    /// Create effect system for production.
    pub fn production(config: AgentConfig) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_production(config.device_id());
        Ok(Self::build_internal(config, composite))
    }

    /// Create effect system for testing with default configuration.
    pub fn testing(config: &AgentConfig) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_testing(config.device_id());
        Ok(Self::build_internal(config.clone(), composite))
    }

    /// Create effect system for simulation with controlled seed.
    pub fn simulation(config: &AgentConfig, seed: u64) -> Result<Self, crate::core::AgentError> {
        let composite = CompositeHandlerAdapter::for_simulation(config.device_id(), seed);
        Ok(Self::build_internal(config.clone(), composite))
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
    ) -> aura_wot::effects::WotAuthorizationHandler<RealCryptoHandler> {
        if let Some(bytes) = verifying_key {
            if let Ok(public_key) = PublicKey::from_bytes(bytes) {
                return aura_wot::effects::WotAuthorizationHandler::new(
                    crypto_handler.clone(),
                    public_key,
                    authority,
                );
            }
        }

        aura_wot::effects::WotAuthorizationHandler::new_mock(crypto_handler.clone())
    }

    /// Construct a journal handler with current policy hooks.
    fn journal_handler(
        &self,
    ) -> aura_journal::JournalHandler<
        RealCryptoHandler,
        FilesystemStorageHandler,
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

// Implementation of RandomEffects
#[async_trait]
impl RandomEffects for AuraEffectSystem {
    #[allow(clippy::disallowed_methods)]
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut rng = StdRng::from_seed([7u8; 32]);
        let mut bytes = vec![0u8; len];
        rng.fill_bytes(&mut bytes);
        bytes
    }

    #[allow(clippy::disallowed_methods)]
    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut rng = StdRng::from_seed([11u8; 32]);
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        bytes
    }

    #[allow(clippy::disallowed_methods)]
    async fn random_u64(&self) -> u64 {
        let mut rng = StdRng::from_seed([19u8; 32]);
        rng.gen()
    }

    #[allow(clippy::disallowed_methods)]
    async fn random_range(&self, min: u64, max: u64) -> u64 {
        let mut rng = StdRng::from_seed([23u8; 32]);
        rng.gen_range(min..=max)
    }

    #[allow(clippy::disallowed_methods)]
    async fn random_uuid(&self) -> uuid::Uuid {
        uuid::Uuid::new_v4()
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
            let mut inbox = self
                .transport_inbox
                .write()
                .expect("transport inbox poisoned");
            inbox.push(envelope.clone());
        }

        {
            let mut stats = self
                .transport_stats
                .write()
                .expect("transport stats poisoned");
            stats.envelopes_sent = stats.envelopes_sent.saturating_add(1);
            let running_total = (stats.avg_envelope_size as u64)
                .saturating_mul(stats.envelopes_sent.saturating_sub(1))
                .saturating_add(envelope.payload.len() as u64);
            stats.avg_envelope_size = (running_total / stats.envelopes_sent.max(1)) as u32;
        }

        self.transport_handler.send_envelope(envelope).await
    }

    async fn receive_envelope(&self) -> Result<TransportEnvelope, TransportError> {
        let maybe = {
            let mut inbox = self
                .transport_inbox
                .write()
                .expect("transport inbox poisoned");
            if inbox.is_empty() {
                None
            } else {
                Some(inbox.remove(0))
            }
        };

        match maybe {
            Some(env) => {
                let mut stats = self
                    .transport_stats
                    .write()
                    .expect("transport stats poisoned");
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
        let maybe = {
            let mut inbox = self
                .transport_inbox
                .write()
                .expect("transport inbox poisoned");
            inbox
                .iter()
                .position(|env| env.source == source && env.context == context)
                .map(|pos| inbox.remove(pos))
        };

        match maybe {
            Some(env) => {
                let mut stats = self
                    .transport_stats
                    .write()
                    .expect("transport stats poisoned");
                stats.envelopes_received = stats.envelopes_received.saturating_add(1);
                Ok(env)
            }
            None => Err(TransportError::NoMessage),
        }
    }

    async fn is_channel_established(&self, context: ContextId, peer: AuthorityId) -> bool {
        let has_local = {
            let inbox = self
                .transport_inbox
                .read()
                .expect("transport inbox poisoned");
            inbox
                .iter()
                .any(|env| env.context == context && env.destination == peer)
        };

        if has_local {
            return true;
        }

        self.transport_handler
            .is_channel_established(context, peer)
            .await
    }

    async fn get_transport_stats(&self) -> TransportStats {
        self.transport_stats
            .read()
            .expect("transport stats poisoned")
            .clone()
    }
}

// Implementation of CryptoEffects
#[async_trait]
impl CryptoEffects for AuraEffectSystem {
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

    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<crypto::FrostKeyGenResult, CryptoError> {
        self.crypto_handler
            .frost_generate_keys(threshold, max_signers)
            .await
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError> {
        self.crypto_handler.frost_generate_nonces().await
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

    fn is_simulated(&self) -> bool {
        aura_core::CryptoEffects::is_simulated(&self.crypto_handler)
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
}

// Implementation of NetworkEffects
#[async_trait]
impl NetworkEffects for AuraEffectSystem {
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
impl StorageEffects for AuraEffectSystem {
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

// Time helper implementations (compat)
#[async_trait]
// Implementation of ConsoleEffects
#[async_trait]
impl ConsoleEffects for AuraEffectSystem {
    async fn log_info(&self, message: &str) -> Result<(), AuraError> {
        println!("INFO: {}", message);
        Ok(())
    }

    async fn log_warn(&self, message: &str) -> Result<(), AuraError> {
        println!("WARN: {}", message);
        Ok(())
    }

    async fn log_error(&self, message: &str) -> Result<(), AuraError> {
        eprintln!("ERROR: {}", message);
        Ok(())
    }

    async fn log_debug(&self, message: &str) -> Result<(), AuraError> {
        println!("DEBUG: {}", message);
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

    async fn persist_journal(&self, _journal: &Journal) -> Result<(), AuraError> {
        self.journal_handler().persist_journal(_journal).await
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
        // Mock implementation
        println!("[{}] {}: {}", level.to_uppercase(), component, message);
        Ok(())
    }

    async fn log_with_context(
        &self,
        level: &str,
        component: &str,
        message: &str,
        _context: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        // Mock implementation that logs without additional context data
        println!("[{}] {}: {}", level.to_uppercase(), component, message);
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
        since_timestamp: Option<u64>,
    ) -> aura_core::Result<Vec<aura_core::effects::LeakageEvent>> {
        self.leakage_handler
            .get_leakage_history(context_id, since_timestamp)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::ContextId;
    use aura_protocol::amp::AmpJournalEffects;
    use aura_protocol::effects::TreeEffects;

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
