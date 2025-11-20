//! Stub Coordinator for compilation
//!
//! This is a minimal stub to allow aura-agent to compile while the full
//! coordinator is being refactored to use the new authority-centric architecture.

use aura_core::effects::*;
use aura_core::effects::crypto::{KeyDerivationContext, FrostKeyGenResult, FrostSigningPackage};
use aura_core::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::{AuraError, Cap, FlowBudget, Journal, Hash32, Policy, Receipt};
use aura_effects::*;
use aura_protocol::effects::ledger::{LedgerEffects, LedgerError, DeviceMetadata, LedgerEventStream};
use aura_protocol::effects::tree::TreeEffects;
use aura_protocol::effects::choreographic::{ChoreographicEffects, ChoreographicRole, ChoreographyError, ChoreographyEvent, ChoreographyMetrics};
use aura_journal::ratchet_tree::{TreeState, AttestedOp, LeafNode, NodeIndex, LeafId, TreeOpKind};
use aura_protocol::effects::tree::{Cut, ProposalId, Partial, Snapshot};
use aura_protocol::guards::flow::FlowBudgetEffects;
use aura_protocol::guards::GuardEffectSystem;
use aura_protocol::handlers::tree::DummyTreeHandler;
use aura_protocol::handlers::memory::MemoryChoreographicHandler;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Minimal stub effect system that composes handlers from aura-effects
#[derive(Clone)]
pub struct AuraEffectSystem {
    device_id: DeviceId,
    console: Arc<RealConsoleHandler>,
    crypto: Arc<RealCryptoHandler>,
    random: Arc<RealRandomHandler>,
    time: Arc<RealTimeHandler>,
    storage: Arc<MemoryStorageHandler>,
    journal: Arc<MockJournalHandler>,
    tree: Arc<DummyTreeHandler>,
    choreographic: Arc<MemoryChoreographicHandler>,
}

impl std::fmt::Debug for AuraEffectSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuraEffectSystem")
            .field("console", &"<RealConsoleHandler>")
            .field("crypto", &"<RealCryptoHandler>")
            .field("random", &"<RealRandomHandler>")
            .field("time", &"<RealTimeHandler>")
            .field("storage", &"<MemoryStorageHandler>")
            .field("journal", &"<MockJournalHandler>")
            .field("tree", &"<DummyTreeHandler>")
            .field("choreographic", &"<MemoryChoreographicHandler>")
            .finish()
    }
}

impl AuraEffectSystem {
    /// Create a new stub effect system
    pub fn new() -> Self {
        Self {
            device_id: DeviceId::new(),
            console: Arc::new(RealConsoleHandler::new()),
            crypto: Arc::new(RealCryptoHandler::new()),
            random: Arc::new(RealRandomHandler::new()),
            time: Arc::new(RealTimeHandler::new()),
            storage: Arc::new(MemoryStorageHandler::new()),
            journal: Arc::new(MockJournalHandler::new()),
            tree: Arc::new(DummyTreeHandler::new()),
            choreographic: Arc::new(MemoryChoreographicHandler::new(Uuid::new_v4())),
        }
    }

    /// Create a stub effect system seeded with a specific device ID
    pub fn with_device_id(device_id: DeviceId) -> Self {
        Self {
            device_id,
            console: Arc::new(RealConsoleHandler::new()),
            crypto: Arc::new(RealCryptoHandler::new()),
            random: Arc::new(RealRandomHandler::new()),
            time: Arc::new(RealTimeHandler::new()),
            storage: Arc::new(MemoryStorageHandler::new()),
            journal: Arc::new(MockJournalHandler::new()),
            tree: Arc::new(DummyTreeHandler::new()),
            choreographic: Arc::new(MemoryChoreographicHandler::new(Uuid::new_v4())),
        }
    }
}

impl Default for AuraEffectSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FlowBudgetEffects for AuraEffectSystem {
    async fn charge_flow(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> Result<Receipt, AuraError> {
        // Use the journal handler to perform an atomic charge
        let updated_budget = JournalEffects::charge_flow_budget(self.journal.as_ref(), context, peer, cost).await?;
        let nonce = updated_budget.spent;
        Ok(Receipt::new(
            context.clone(),
            self.device_id,
            *peer,
            updated_budget.epoch,
            cost,
            nonce,
            Hash32::default(),
            Vec::new(),
        ))
    }
}

// Guard integration for send guard chain / leakage tracking
impl GuardEffectSystem for AuraEffectSystem {
    fn device_id(&self) -> DeviceId {
        self.device_id
    }

    fn execution_mode(&self) -> aura_core::effects::ExecutionMode {
        aura_core::effects::ExecutionMode::Testing
    }

    fn get_metadata(&self, _key: &str) -> Option<String> {
        None
    }

    fn can_perform_operation(&self, _operation: &str) -> bool {
        true
    }
}

// Implement ConsoleEffects by delegating to the console handler
#[async_trait]
impl ConsoleEffects for AuraEffectSystem {
    async fn log_info(&self, message: &str) -> Result<(), AuraError> {
        ConsoleEffects::log_info(self.console.as_ref(), message).await
    }

    async fn log_warn(&self, message: &str) -> Result<(), AuraError> {
        ConsoleEffects::log_warn(self.console.as_ref(), message).await
    }

    async fn log_error(&self, message: &str) -> Result<(), AuraError> {
        ConsoleEffects::log_error(self.console.as_ref(), message).await
    }

    async fn log_debug(&self, message: &str) -> Result<(), AuraError> {
        ConsoleEffects::log_debug(self.console.as_ref(), message).await
    }
}

// Implement RandomEffects by delegating to the random handler
#[async_trait]
impl RandomEffects for AuraEffectSystem {
    async fn random_bytes(&self, count: usize) -> Vec<u8> {
        RandomEffects::random_bytes(self.random.as_ref(), count).await
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        RandomEffects::random_bytes_32(self.random.as_ref()).await
    }

    async fn random_u64(&self) -> u64 {
        RandomEffects::random_u64(self.random.as_ref()).await
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        RandomEffects::random_range(self.random.as_ref(), min, max).await
    }

    async fn random_uuid(&self) -> Uuid {
        RandomEffects::random_uuid(self.random.as_ref()).await
    }
}

// Implement CryptoEffects by delegating to the crypto handler
#[async_trait]
impl CryptoEffects for AuraEffectSystem {
    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        CryptoEffects::hkdf_derive(self.crypto.as_ref(), ikm, salt, info, output_len).await
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        CryptoEffects::derive_key(self.crypto.as_ref(), master_key, context).await
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        CryptoEffects::ed25519_generate_keypair(self.crypto.as_ref()).await
    }

    async fn ed25519_sign(
        &self,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        CryptoEffects::ed25519_sign(self.crypto.as_ref(), message, private_key).await
    }

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        CryptoEffects::ed25519_verify(self.crypto.as_ref(), message, signature, public_key).await
    }

    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        CryptoEffects::frost_generate_keys(self.crypto.as_ref(), threshold, max_signers).await
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError> {
        CryptoEffects::frost_generate_nonces(self.crypto.as_ref()).await
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
        public_key_package: &[u8],
    ) -> Result<FrostSigningPackage, CryptoError> {
        CryptoEffects::frost_create_signing_package(
            self.crypto.as_ref(),
            message,
            nonces,
            participants,
            public_key_package,
        )
        .await
    }

    async fn frost_sign_share(
        &self,
        signing_package: &FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        CryptoEffects::frost_sign_share(self.crypto.as_ref(), signing_package, key_share, nonces).await
    }

    async fn frost_aggregate_signatures(
        &self,
        signing_package: &FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        CryptoEffects::frost_aggregate_signatures(self.crypto.as_ref(), signing_package, signature_shares).await
    }

    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        CryptoEffects::frost_verify(self.crypto.as_ref(), message, signature, group_public_key).await
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        CryptoEffects::ed25519_public_key(self.crypto.as_ref(), private_key).await
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        CryptoEffects::chacha20_encrypt(self.crypto.as_ref(), plaintext, key, nonce).await
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        CryptoEffects::chacha20_decrypt(self.crypto.as_ref(), ciphertext, key, nonce).await
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        CryptoEffects::aes_gcm_encrypt(self.crypto.as_ref(), plaintext, key, nonce).await
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        CryptoEffects::aes_gcm_decrypt(self.crypto.as_ref(), ciphertext, key, nonce).await
    }

    async fn frost_rotate_keys(
        &self,
        old_shares: &[Vec<u8>],
        old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        CryptoEffects::frost_rotate_keys(
            self.crypto.as_ref(),
            old_shares,
            old_threshold,
            new_threshold,
            new_max_signers,
        )
        .await
    }

    fn is_simulated(&self) -> bool {
        CryptoEffects::is_simulated(self.crypto.as_ref())
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        CryptoEffects::crypto_capabilities(self.crypto.as_ref())
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        CryptoEffects::constant_time_eq(self.crypto.as_ref(), a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        CryptoEffects::secure_zero(self.crypto.as_ref(), data)
    }
}

// Implement TimeEffects by delegating to the time handler
#[async_trait]
impl TimeEffects for AuraEffectSystem {
    async fn current_epoch(&self) -> u64 {
        TimeEffects::current_timestamp_millis(self.time.as_ref()).await
    }

    async fn current_timestamp(&self) -> u64 {
        TimeEffects::current_timestamp(self.time.as_ref()).await
    }

    async fn current_timestamp_millis(&self) -> u64 {
        TimeEffects::current_timestamp_millis(self.time.as_ref()).await
    }

    async fn now_instant(&self) -> Instant {
        // Stub: return current instant
        Instant::now()
    }

    async fn sleep_ms(&self, ms: u64) {
        TimeEffects::sleep(self.time.as_ref(), ms).await.ok();
    }

    async fn sleep_until(&self, _epoch: u64) {
        // Stub: no-op
    }

    async fn delay(&self, duration: Duration) {
        TimeEffects::sleep(self.time.as_ref(), duration.as_millis() as u64).await.ok();
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError> {
        TimeEffects::sleep(self.time.as_ref(), duration_ms).await
    }

    async fn yield_until(&self, _condition: WakeCondition) -> Result<(), TimeError> {
        // Stub: return immediately
        Ok(())
    }

    async fn wait_until(&self, _condition: WakeCondition) -> Result<(), AuraError> {
        // Stub: return immediately
        Ok(())
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        TimeEffects::set_timeout(self.time.as_ref(), timeout_ms).await
    }

    async fn cancel_timeout(&self, _handle: TimeoutHandle) -> Result<(), TimeError> {
        // Stub: always succeed
        Ok(())
    }

    fn is_simulated(&self) -> bool {
        false
    }

    fn register_context(&self, _context_id: Uuid) {
        // Stub: no-op
    }

    fn unregister_context(&self, _context_id: Uuid) {
        // Stub: no-op
    }

    async fn notify_events_available(&self) {
        // Stub: no-op
    }

    fn resolution_ms(&self) -> u64 {
        1
    }
}

// Implement StorageEffects by delegating to the storage handler
#[async_trait]
impl StorageEffects for AuraEffectSystem {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        StorageEffects::store(self.storage.as_ref(), key, value).await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        StorageEffects::retrieve(self.storage.as_ref(), key).await
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        StorageEffects::remove(self.storage.as_ref(), key).await
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        StorageEffects::list_keys(self.storage.as_ref(), prefix).await
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        StorageEffects::exists(self.storage.as_ref(), key).await
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        StorageEffects::store_batch(self.storage.as_ref(), pairs).await
    }

    async fn retrieve_batch(&self, keys: &[String]) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        StorageEffects::retrieve_batch(self.storage.as_ref(), keys).await
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        StorageEffects::clear_all(self.storage.as_ref()).await
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        StorageEffects::stats(self.storage.as_ref()).await
    }
}

// Implement JournalEffects by delegating to the journal handler
#[async_trait]
impl JournalEffects for AuraEffectSystem {
    async fn merge_facts(&self, target: &Journal, delta: &Journal) -> Result<Journal, AuraError> {
        JournalEffects::merge_facts(self.journal.as_ref(), target, delta).await
    }

    async fn refine_caps(&self, target: &Journal, refinement: &Journal) -> Result<Journal, AuraError> {
        JournalEffects::refine_caps(self.journal.as_ref(), target, refinement).await
    }

    async fn get_journal(&self) -> Result<Journal, AuraError> {
        JournalEffects::get_journal(self.journal.as_ref()).await
    }

    async fn persist_journal(&self, journal: &Journal) -> Result<(), AuraError> {
        JournalEffects::persist_journal(self.journal.as_ref(), journal).await
    }

    async fn get_flow_budget(&self, context: &ContextId, peer: &AuthorityId) -> Result<FlowBudget, AuraError> {
        JournalEffects::get_flow_budget(self.journal.as_ref(), context, peer).await
    }

    async fn update_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError> {
        JournalEffects::update_flow_budget(self.journal.as_ref(), context, peer, budget).await
    }

    async fn charge_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> Result<FlowBudget, AuraError> {
        JournalEffects::charge_flow_budget(self.journal.as_ref(), context, peer, cost).await
    }
}

// Implement AuthorizationEffects with stub implementations
#[async_trait]
impl AuthorizationEffects for AuraEffectSystem {
    async fn verify_capability(
        &self,
        _capabilities: &Cap,
        _operation: &str,
        _resource: &str,
    ) -> Result<bool, AuthorizationError> {
        // Stub: always permit
        Ok(true)
    }

    async fn delegate_capabilities(
        &self,
        _source_capabilities: &Cap,
        _requested_capabilities: &Cap,
        _target_device: &DeviceId,
    ) -> Result<Cap, AuthorizationError> {
        Err(AuthorizationError::SystemError(
            AuraError::internal("AuthorizationEffects::delegate_capabilities not implemented in stub")
        ))
    }
}

// Implement LeakageEffects with stub implementations
#[async_trait]
impl LeakageEffects for AuraEffectSystem {
    async fn record_leakage(&self, _event: LeakageEvent) -> Result<(), AuraError> {
        // Stub: no-op (accept all leakage)
        Ok(())
    }

    async fn get_leakage_budget(&self, _context_id: ContextId) -> Result<LeakageBudget, AuraError> {
        // Stub: return zero budget
        Ok(LeakageBudget::zero())
    }

    async fn check_leakage_budget(
        &self,
        _context_id: ContextId,
        _observer: ObserverClass,
        _amount: u64,
    ) -> Result<bool, AuraError> {
        // Stub: always allow
        Ok(true)
    }

    async fn get_leakage_history(
        &self,
        _context_id: ContextId,
        _since_timestamp: Option<u64>,
    ) -> Result<Vec<LeakageEvent>, AuraError> {
        // Stub: return empty history
        Ok(Vec::new())
    }
}

// Implement NetworkEffects with stub implementations
#[async_trait]
impl NetworkEffects for AuraEffectSystem {
    async fn send_to_peer(&self, _peer_id: Uuid, _message: Vec<u8>) -> Result<(), NetworkError> {
        Err(NetworkError::NotImplemented)
    }

    async fn broadcast(&self, _message: Vec<u8>) -> Result<(), NetworkError> {
        Err(NetworkError::NotImplemented)
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        Err(NetworkError::NoMessage)
    }

    async fn receive_from(&self, _peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        Err(NetworkError::NotImplemented)
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        Vec::new()
    }

    async fn is_peer_connected(&self, _peer_id: Uuid) -> bool {
        false
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        Err(NetworkError::NotImplemented)
    }
}

// Implement LedgerEffects with stub implementations
#[async_trait]
impl LedgerEffects for AuraEffectSystem {
    async fn append_event(&self, _event: Vec<u8>) -> Result<(), LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn current_epoch(&self) -> Result<u64, LedgerError> {
        Ok(0)
    }

    async fn events_since(&self, _epoch: u64) -> Result<Vec<Vec<u8>>, LedgerError> {
        Ok(Vec::new())
    }

    async fn is_device_authorized(&self, _device_id: DeviceId, _operation: &str) -> Result<bool, LedgerError> {
        Ok(true)
    }

    async fn get_device_metadata(&self, _device_id: DeviceId) -> Result<Option<DeviceMetadata>, LedgerError> {
        Ok(None)
    }

    async fn update_device_activity(&self, _device_id: DeviceId) -> Result<(), LedgerError> {
        Ok(())
    }

    async fn subscribe_to_events(&self) -> Result<LedgerEventStream, LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn would_create_cycle(&self, _edges: &[(Vec<u8>, Vec<u8>)], _new_edge: (Vec<u8>, Vec<u8>)) -> Result<bool, LedgerError> {
        Ok(false)
    }

    async fn find_connected_components(&self, _edges: &[(Vec<u8>, Vec<u8>)]) -> Result<Vec<Vec<Vec<u8>>>, LedgerError> {
        Ok(Vec::new())
    }

    async fn topological_sort(&self, _edges: &[(Vec<u8>, Vec<u8>)]) -> Result<Vec<Vec<u8>>, LedgerError> {
        Ok(Vec::new())
    }

    async fn shortest_path(&self, _edges: &[(Vec<u8>, Vec<u8>)], _start: Vec<u8>, _end: Vec<u8>) -> Result<Option<Vec<Vec<u8>>>, LedgerError> {
        Ok(None)
    }

    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, LedgerError> {
        Ok(RandomEffects::random_bytes(self, length).await)
    }

    async fn hash_data(&self, data: &[u8]) -> Result<[u8; 32], LedgerError> {
        Ok(aura_core::hash::hash(data))
    }

    async fn current_timestamp(&self) -> Result<u64, LedgerError> {
        Ok(TimeEffects::current_timestamp(self).await)
    }

    async fn ledger_device_id(&self) -> Result<DeviceId, LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn new_uuid(&self) -> Result<Uuid, LedgerError> {
        Ok(Uuid::new_v4())
    }
}

// Implement TreeEffects by delegating to the tree handler
#[async_trait]
impl TreeEffects for AuraEffectSystem {
    async fn get_current_state(&self) -> Result<TreeState, AuraError> {
        TreeEffects::get_current_state(self.tree.as_ref()).await
    }

    async fn get_current_commitment(&self) -> Result<Hash32, AuraError> {
        TreeEffects::get_current_commitment(self.tree.as_ref()).await
    }

    async fn get_current_epoch(&self) -> Result<u64, AuraError> {
        TreeEffects::get_current_epoch(self.tree.as_ref()).await
    }

    async fn apply_attested_op(&self, op: AttestedOp) -> Result<Hash32, AuraError> {
        TreeEffects::apply_attested_op(self.tree.as_ref(), op).await
    }

    async fn verify_aggregate_sig(
        &self,
        op: &AttestedOp,
        state: &TreeState,
    ) -> Result<bool, AuraError> {
        TreeEffects::verify_aggregate_sig(self.tree.as_ref(), op, state).await
    }

    async fn add_leaf(&self, leaf: LeafNode, under: NodeIndex) -> Result<TreeOpKind, AuraError> {
        TreeEffects::add_leaf(self.tree.as_ref(), leaf, under).await
    }

    async fn remove_leaf(&self, leaf_id: LeafId, reason: u8) -> Result<TreeOpKind, AuraError> {
        TreeEffects::remove_leaf(self.tree.as_ref(), leaf_id, reason).await
    }

    async fn change_policy(
        &self,
        node: NodeIndex,
        new_policy: Policy,
    ) -> Result<TreeOpKind, AuraError> {
        TreeEffects::change_policy(self.tree.as_ref(), node, new_policy).await
    }

    async fn rotate_epoch(&self, affected: Vec<NodeIndex>) -> Result<TreeOpKind, AuraError> {
        TreeEffects::rotate_epoch(self.tree.as_ref(), affected).await
    }

    async fn propose_snapshot(&self, cut: Cut) -> Result<ProposalId, AuraError> {
        TreeEffects::propose_snapshot(self.tree.as_ref(), cut).await
    }

    async fn approve_snapshot(&self, proposal_id: ProposalId) -> Result<Partial, AuraError> {
        TreeEffects::approve_snapshot(self.tree.as_ref(), proposal_id).await
    }

    async fn finalize_snapshot(&self, proposal_id: ProposalId) -> Result<Snapshot, AuraError> {
        TreeEffects::finalize_snapshot(self.tree.as_ref(), proposal_id).await
    }

    async fn apply_snapshot(&self, snapshot: &Snapshot) -> Result<(), AuraError> {
        TreeEffects::apply_snapshot(self.tree.as_ref(), snapshot).await
    }
}

// Implement ChoreographicEffects by delegating to the choreographic handler
#[async_trait]
impl ChoreographicEffects for AuraEffectSystem {
    async fn send_to_role_bytes(
        &self,
        role: ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), ChoreographyError> {
        ChoreographicEffects::send_to_role_bytes(self.choreographic.as_ref(), role, message).await
    }

    async fn receive_from_role_bytes(
        &self,
        role: ChoreographicRole,
    ) -> Result<Vec<u8>, ChoreographyError> {
        ChoreographicEffects::receive_from_role_bytes(self.choreographic.as_ref(), role).await
    }

    async fn broadcast_bytes(&self, message: Vec<u8>) -> Result<(), ChoreographyError> {
        ChoreographicEffects::broadcast_bytes(self.choreographic.as_ref(), message).await
    }

    fn current_role(&self) -> ChoreographicRole {
        ChoreographicEffects::current_role(self.choreographic.as_ref())
    }

    fn all_roles(&self) -> Vec<ChoreographicRole> {
        ChoreographicEffects::all_roles(self.choreographic.as_ref())
    }

    async fn is_role_active(&self, role: ChoreographicRole) -> bool {
        ChoreographicEffects::is_role_active(self.choreographic.as_ref(), role).await
    }

    async fn start_session(
        &self,
        session_id: Uuid,
        roles: Vec<ChoreographicRole>,
    ) -> Result<(), ChoreographyError> {
        ChoreographicEffects::start_session(self.choreographic.as_ref(), session_id, roles).await
    }

    async fn end_session(&self) -> Result<(), ChoreographyError> {
        ChoreographicEffects::end_session(self.choreographic.as_ref()).await
    }

    async fn emit_choreo_event(&self, event: ChoreographyEvent) -> Result<(), ChoreographyError> {
        ChoreographicEffects::emit_choreo_event(self.choreographic.as_ref(), event).await
    }

    async fn set_timeout(&self, timeout_ms: u64) {
        ChoreographicEffects::set_timeout(self.choreographic.as_ref(), timeout_ms).await
    }

    async fn get_metrics(&self) -> ChoreographyMetrics {
        ChoreographicEffects::get_metrics(self.choreographic.as_ref()).await
    }
}

// Implement SystemEffects with stub implementations
#[async_trait]
impl SystemEffects for AuraEffectSystem {
    async fn log(&self, level: &str, component: &str, message: &str) -> Result<(), SystemError> {
        // Stub: log to console
        let _ = ConsoleEffects::log_info(self, &format!("[{}] {}: {}", level, component, message)).await;
        Ok(())
    }

    async fn log_with_context(
        &self,
        level: &str,
        component: &str,
        message: &str,
        context: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        // Stub: log to console with context
        let context_str = context
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = ConsoleEffects::log_info(self, &format!("[{}] {}: {} [{}]", level, component, message, context_str)).await;
        Ok(())
    }

    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError> {
        let mut info = HashMap::new();
        info.insert("version".to_string(), env!("CARGO_PKG_VERSION").to_string());
        info.insert("mode".to_string(), "stub".to_string());
        Ok(info)
    }

    async fn set_config(&self, _key: &str, _value: &str) -> Result<(), SystemError> {
        Err(SystemError::OperationFailed {
            message: "SystemEffects::set_config not implemented in stub".to_string(),
        })
    }

    async fn get_config(&self, _key: &str) -> Result<String, SystemError> {
        Err(SystemError::ResourceNotFound {
            resource: "config".to_string(),
        })
    }

    async fn health_check(&self) -> Result<bool, SystemError> {
        Ok(true)
    }

    async fn get_metrics(&self) -> Result<HashMap<String, f64>, SystemError> {
        Ok(HashMap::new())
    }

    async fn restart_component(&self, _component: &str) -> Result<(), SystemError> {
        Err(SystemError::OperationFailed {
            message: "SystemEffects::restart_component not implemented in stub".to_string(),
        })
    }

    async fn shutdown(&self) -> Result<(), SystemError> {
        Ok(()) // Stub: no-op
    }
}

// Implement the composite AuraEffects trait
impl aura_protocol::effects::AuraEffects for AuraEffectSystem {
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Testing // Stub coordinator is for testing
    }
}
