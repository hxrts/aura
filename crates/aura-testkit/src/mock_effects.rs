//! Mock effects implementation for comprehensive testing
//!
//! This module provides MockEffects - a complete implementation of all Aura effect traits
//! with deterministic, predictable behavior suitable for testing.
//!
//! Key features:
//! - Deterministic randomness using seeded ChaCha20 RNG
//! - In-memory storage with full CRUD operations
//! - Mock crypto operations with consistent return values
//! - Controllable time advancement for testing
//! - Complete coverage of all effect traits
//!
//! # Blocking Lock Usage
//!
//! Uses `std::sync::Mutex` because this is Layer 8 test infrastructure where:
//! 1. Tests run in controlled single-threaded contexts
//! 2. Lock contention is not a concern in test scenarios
//! 3. Simpler synchronous API is preferred for test clarity

#![allow(clippy::disallowed_types)]

use async_trait::async_trait;
use aura_core::crypto::single_signer::{
    SigningMode, SingleSignerKeyPackage, SingleSignerPublicKeyPackage,
};
use aura_core::effects::authorization::{AuthorizationDecision, AuthorizationError};
use aura_core::effects::crypto::SigningKeyGenResult;
use aura_core::effects::network::{
    NetworkError, PeerEventStream, UdpEffects, UdpEndpoint, UdpEndpointEffects,
};
use aura_core::effects::storage::{StorageError, StorageStats};
use aura_core::effects::time::{
    LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects, TimeError,
};
use aura_core::effects::{
    amp::{
        AmpChannelEffects, AmpChannelError, AmpCiphertext, AmpHeader, ChannelCloseParams,
        ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
    },
    BiscuitAuthorizationEffects, CryptoCoreEffects, CryptoExtendedEffects, FlowBudgetEffects,
    JournalEffects, NetworkCoreEffects, NetworkExtendedEffects, RandomCoreEffects,
    StorageCoreEffects, StorageExtendedEffects,
};
use aura_core::flow::{FlowBudget, Receipt};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::scope::{AuthorizationOp, ResourceScope};
use aura_core::time::{LogicalTime, OrderTime, PhysicalTime, VectorClock};
use aura_core::types::Epoch;
use aura_core::{AuraError, ChannelId, Hash32, Journal};
use rand_chacha::{rand_core::SeedableRng, ChaCha20Rng};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Mock effects implementation for deterministic testing
///
/// This provides a complete implementation of all effect traits with
/// deterministic, predictable behavior suitable for comprehensive testing.
#[derive(Debug, Clone)]
pub struct MockEffects {
    /// Internal state for deterministic behavior
    state: Arc<Mutex<MockState>>,
}

#[derive(Debug)]
struct MockState {
    /// Deterministic RNG for reproducible tests
    rng: ChaCha20Rng,
    /// Mock storage backend
    storage: HashMap<String, Vec<u8>>,
    /// Logical clock state
    logical_clock: LogicalTime,
    /// Physical time counter (deterministic)
    physical_time_ms: u64,
    /// Flow receipts per context/authority
    flow_receipts: HashMap<(ContextId, AuthorityId), Receipt>,
    /// AMP channel state
    amp_channels: HashMap<(ContextId, ChannelId), AmpChanState>,
}

#[derive(Debug, Clone)]
struct AmpChanState {
    epoch: u64,
    gen: u64,
    closed: bool,
}

impl MockEffects {
    /// Create deterministic mock effects with fixed seed
    pub fn deterministic() -> Self {
        Self::with_seed([42; 32])
    }

    /// Create mock effects with specific seed for reproducible tests
    pub fn with_seed(seed: [u8; 32]) -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState {
                rng: ChaCha20Rng::from_seed(seed),
                storage: HashMap::new(),
                logical_clock: LogicalTime {
                    vector: VectorClock::default(),
                    lamport: 0,
                },
                physical_time_ms: 1640995200000, // Fixed: 2022-01-01 00:00:00 UTC
                flow_receipts: HashMap::new(),
                amp_channels: HashMap::new(),
            })),
        }
    }

    /// Reset internal state while preserving deterministic seed
    pub fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        state.storage.clear();
        state.logical_clock = LogicalTime {
            vector: VectorClock::default(),
            lamport: 0,
        };
        state.flow_receipts.clear();
        state.amp_channels.clear();
    }

    /// Get current storage state for inspection
    pub fn storage_keys(&self) -> Vec<String> {
        let state = self.state.lock().unwrap();
        state.storage.keys().cloned().collect()
    }
}

#[async_trait]
impl AmpChannelEffects for MockEffects {
    async fn create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> Result<ChannelId, AmpChannelError> {
        let ChannelCreateParams {
            context, channel, ..
        } = params;

        let channel = match channel {
            Some(channel) => channel,
            None => {
                let bytes = self.random_bytes(32).await;
                ChannelId::from_bytes(aura_core::hash::hash(&bytes))
            }
        };

        let mut state = self.state.lock().unwrap();
        let entry = state
            .amp_channels
            .entry((context, channel))
            .or_insert(AmpChanState {
                epoch: 0,
                gen: 0,
                closed: false,
            });
        entry.closed = false;
        Ok(channel)
    }

    async fn close_channel(&self, params: ChannelCloseParams) -> Result<(), AmpChannelError> {
        let mut state = self.state.lock().unwrap();
        let key = (params.context, params.channel);
        let chan = state
            .amp_channels
            .get_mut(&key)
            .ok_or(AmpChannelError::NotFound)?;
        chan.closed = true;
        chan.epoch += 1;
        Ok(())
    }

    async fn join_channel(&self, params: ChannelJoinParams) -> Result<(), AmpChannelError> {
        let state = self.state.lock().unwrap();
        let key = (params.context, params.channel);
        if !state.amp_channels.contains_key(&key) {
            return Err(AmpChannelError::NotFound);
        }
        // Mock join - just verify channel exists
        Ok(())
    }

    async fn leave_channel(&self, params: ChannelLeaveParams) -> Result<(), AmpChannelError> {
        let state = self.state.lock().unwrap();
        let key = (params.context, params.channel);
        if !state.amp_channels.contains_key(&key) {
            return Err(AmpChannelError::NotFound);
        }
        // Mock leave - just verify channel exists
        Ok(())
    }

    async fn send_message(
        &self,
        params: ChannelSendParams,
    ) -> Result<AmpCiphertext, AmpChannelError> {
        let mut state = self.state.lock().unwrap();
        let key = (params.context, params.channel);
        let chan = state
            .amp_channels
            .get_mut(&key)
            .ok_or(AmpChannelError::NotFound)?;
        if chan.closed {
            return Err(AmpChannelError::InvalidState("channel closed".into()));
        }
        let header = AmpHeader {
            context: params.context,
            channel: params.channel,
            chan_epoch: chan.epoch,
            ratchet_gen: chan.gen,
        };
        chan.gen += 1;
        Ok(AmpCiphertext {
            header,
            ciphertext: params.plaintext,
        })
    }
}

#[async_trait]
impl RandomCoreEffects for MockEffects {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        use rand::RngCore;
        let mut state = self.state.lock().unwrap();
        let mut bytes = vec![0u8; len];
        state.rng.fill_bytes(&mut bytes);
        bytes
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        use rand::RngCore;
        let mut state = self.state.lock().unwrap();
        let mut bytes = [0u8; 32];
        state.rng.fill_bytes(&mut bytes);
        bytes
    }

    async fn random_u64(&self) -> u64 {
        use rand::Rng;
        let mut state = self.state.lock().unwrap();
        state.rng.gen()
    }
}

#[async_trait]
impl CryptoCoreEffects for MockEffects {
    async fn hkdf_derive(
        &self,
        _ikm: &[u8],
        _salt: &[u8],
        _info: &[u8],
        output_len: u32,
    ) -> Result<Vec<u8>, aura_core::effects::crypto::CryptoError> {
        Ok(vec![0x42; output_len as usize])
    }

    async fn derive_key(
        &self,
        _master_key: &[u8],
        _context: &aura_core::effects::crypto::KeyDerivationContext,
    ) -> Result<Vec<u8>, aura_core::effects::crypto::CryptoError> {
        Ok(vec![0x33; 32])
    }

    async fn ed25519_generate_keypair(
        &self,
    ) -> Result<(Vec<u8>, Vec<u8>), aura_core::effects::crypto::CryptoError> {
        let private_key = self.random_bytes_32().await;
        let public_key = vec![0x44; 32];
        Ok((private_key.to_vec(), public_key))
    }

    async fn ed25519_sign(
        &self,
        _message: &[u8],
        _private_key: &[u8],
    ) -> Result<Vec<u8>, aura_core::effects::crypto::CryptoError> {
        Ok(vec![0x55; 64])
    }

    async fn ed25519_verify(
        &self,
        _message: &[u8],
        _signature: &[u8],
        _public_key: &[u8],
    ) -> Result<bool, aura_core::effects::crypto::CryptoError> {
        Ok(true)
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        vec![
            "ed25519".to_string(),
            "frost".to_string(),
            "chacha20-poly1305".to_string(),
            "aes-gcm".to_string(),
        ]
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        use subtle::ConstantTimeEq;
        a.ct_eq(b).into()
    }

    fn secure_zero(&self, data: &mut [u8]) {
        use zeroize::Zeroize;
        data.zeroize();
    }
}

#[async_trait]
impl CryptoExtendedEffects for MockEffects {
    async fn frost_generate_keys(
        &self,
        _threshold: u16,
        max_signers: u16,
    ) -> Result<
        aura_core::effects::crypto::FrostKeyGenResult,
        aura_core::effects::crypto::CryptoError,
    > {
        let key_packages = (0..max_signers).map(|i| vec![0x66 + i as u8; 32]).collect();

        Ok(aura_core::effects::crypto::FrostKeyGenResult {
            key_packages,
            public_key_package: vec![0x77; 32],
        })
    }

    async fn frost_generate_nonces(
        &self,
        _key_package: &[u8],
    ) -> Result<Vec<u8>, aura_core::effects::crypto::CryptoError> {
        Ok(vec![0x88; 32])
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        _nonces: &[Vec<u8>],
        _participants: &[u16],
        public_key_package: &[u8],
    ) -> Result<
        aura_core::effects::crypto::FrostSigningPackage,
        aura_core::effects::crypto::CryptoError,
    > {
        Ok(aura_core::effects::crypto::FrostSigningPackage {
            message: message.to_vec(),
            package: vec![0x99; 64],
            participants: vec![1, 2],
            public_key_package: public_key_package.to_vec(),
        })
    }

    async fn frost_sign_share(
        &self,
        _signing_package: &aura_core::effects::crypto::FrostSigningPackage,
        _key_share: &[u8],
        _nonces: &[u8],
    ) -> Result<Vec<u8>, aura_core::effects::crypto::CryptoError> {
        Ok(vec![0xAA; 32])
    }

    async fn frost_aggregate_signatures(
        &self,
        _signing_package: &aura_core::effects::crypto::FrostSigningPackage,
        _signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, aura_core::effects::crypto::CryptoError> {
        Ok(vec![0xBB; 64])
    }

    async fn frost_verify(
        &self,
        _message: &[u8],
        _signature: &[u8],
        _group_public_key: &[u8],
    ) -> Result<bool, aura_core::effects::crypto::CryptoError> {
        Ok(true)
    }

    async fn ed25519_public_key(
        &self,
        _private_key: &[u8],
    ) -> Result<Vec<u8>, aura_core::effects::crypto::CryptoError> {
        Ok(vec![0xCC; 32])
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, aura_core::effects::crypto::CryptoError> {
        let mut result = plaintext.to_vec();
        for byte in &mut result {
            *byte ^= 0xDD;
        }
        Ok(result)
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, aura_core::effects::crypto::CryptoError> {
        let mut result = ciphertext.to_vec();
        for byte in &mut result {
            *byte ^= 0xDD;
        }
        Ok(result)
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, aura_core::effects::crypto::CryptoError> {
        let mut result = plaintext.to_vec();
        for byte in &mut result {
            *byte ^= 0xEE;
        }
        Ok(result)
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, aura_core::effects::crypto::CryptoError> {
        let mut result = ciphertext.to_vec();
        for byte in &mut result {
            *byte ^= 0xEE;
        }
        Ok(result)
    }

    async fn frost_rotate_keys(
        &self,
        _old_shares: &[Vec<u8>],
        _old_threshold: u16,
        _new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<
        aura_core::effects::crypto::FrostKeyGenResult,
        aura_core::effects::crypto::CryptoError,
    > {
        self.frost_generate_keys(_new_threshold, new_max_signers)
            .await
    }

    async fn generate_signing_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, aura_core::effects::crypto::CryptoError> {
        if threshold == 1 && max_signers == 1 {
            let (signing_key, verifying_key) = self.ed25519_generate_keypair().await?;
            let key_package = SingleSignerKeyPackage::new(signing_key, verifying_key.clone());
            let public_package = SingleSignerPublicKeyPackage::new(verifying_key);
            Ok(SigningKeyGenResult {
                key_packages: vec![key_package.to_bytes().map_err(|e| {
                    aura_core::effects::crypto::CryptoError::invalid(format!(
                        "key package serialization: {e}"
                    ))
                })?],
                public_key_package: public_package.to_bytes().map_err(|e| {
                    aura_core::effects::crypto::CryptoError::invalid(format!(
                        "public package serialization: {e}"
                    ))
                })?,
                mode: SigningMode::SingleSigner,
            })
        } else if threshold >= 2 {
            let frost_result = self.frost_generate_keys(threshold, max_signers).await?;
            Ok(SigningKeyGenResult {
                key_packages: frost_result.key_packages,
                public_key_package: frost_result.public_key_package,
                mode: SigningMode::Threshold,
            })
        } else {
            Err(aura_core::AuraError::invalid(format!(
                "Invalid signing configuration: threshold={threshold}, max_signers={max_signers}. \
                 Use 1-of-1 for single-signer or threshold>=2 for multi-party."
            )))
        }
    }

    async fn sign_with_key(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, aura_core::effects::crypto::CryptoError> {
        match mode {
            SigningMode::SingleSigner => {
                let package = SingleSignerKeyPackage::from_bytes(key_package).map_err(|e| {
                    aura_core::AuraError::invalid(format!(
                        "Invalid single-signer key package: {e}"
                    ))
                })?;
                self.ed25519_sign(message, package.signing_key()).await
            }
            SigningMode::Threshold => Err(aura_core::AuraError::invalid(
                "sign_with_key() does not support Threshold mode. Use the full FROST protocol flow.",
            )),
        }
    }

    async fn verify_signature(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key_package: &[u8],
        mode: SigningMode,
    ) -> Result<bool, aura_core::effects::crypto::CryptoError> {
        match mode {
            SigningMode::SingleSigner => {
                let package = SingleSignerPublicKeyPackage::from_bytes(public_key_package)
                    .map_err(|e| {
                        aura_core::AuraError::invalid(format!(
                            "Invalid single-signer public key package: {e}"
                        ))
                    })?;
                self.ed25519_verify(message, signature, package.verifying_key())
                    .await
            }
            SigningMode::Threshold => {
                self.frost_verify(message, signature, public_key_package)
                    .await
            }
        }
    }
}

#[async_trait]
impl StorageCoreEffects for MockEffects {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let mut state = self.state.lock().unwrap();
        state.storage.insert(key.to_string(), value);
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let state = self.state.lock().unwrap();
        Ok(state.storage.get(key).cloned())
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let mut state = self.state.lock().unwrap();
        Ok(state.storage.remove(key).is_some())
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let state = self.state.lock().unwrap();
        match prefix {
            Some(p) => Ok(state
                .storage
                .keys()
                .filter(|k| k.starts_with(p))
                .cloned()
                .collect()),
            None => Ok(state.storage.keys().cloned().collect()),
        }
    }
}

#[async_trait]
impl StorageExtendedEffects for MockEffects {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let state = self.state.lock().unwrap();
        Ok(state.storage.contains_key(key))
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        let mut state = self.state.lock().unwrap();
        for (key, value) in pairs {
            state.storage.insert(key, value);
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        let state = self.state.lock().unwrap();
        let mut result = HashMap::new();
        for key in keys {
            if let Some(value) = state.storage.get(key) {
                result.insert(key.clone(), value.clone());
            }
        }
        Ok(result)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        let mut state = self.state.lock().unwrap();
        state.storage.clear();
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let state = self.state.lock().unwrap();
        Ok(StorageStats {
            key_count: state.storage.len() as u64,
            total_size: state.storage.values().map(|v| v.len() as u64).sum(),
            available_space: Some(u64::MAX),
            backend_type: "mock".to_string(),
        })
    }
}

#[async_trait]
impl PhysicalTimeEffects for MockEffects {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
        let state = self.state.lock().unwrap();
        Ok(PhysicalTime {
            ts_ms: state.physical_time_ms,
            uncertainty: None,
        })
    }

    async fn sleep_ms(&self, duration_ms: u64) -> Result<(), TimeError> {
        let mut state = self.state.lock().unwrap();
        state.physical_time_ms += duration_ms;
        Ok(())
    }
}

#[async_trait]
impl LogicalClockEffects for MockEffects {
    async fn logical_advance(
        &self,
        _observed: Option<&VectorClock>,
    ) -> Result<LogicalTime, TimeError> {
        let mut state = self.state.lock().unwrap();
        // Simple increment for mock implementation
        state.logical_clock = LogicalTime {
            vector: VectorClock::default(),
            lamport: 0,
        }; // Reset for deterministic behavior
        Ok(state.logical_clock.clone())
    }

    async fn logical_now(&self) -> Result<LogicalTime, TimeError> {
        let state = self.state.lock().unwrap();
        Ok(state.logical_clock.clone())
    }
}

#[async_trait]
impl OrderClockEffects for MockEffects {
    async fn order_time(&self) -> Result<OrderTime, TimeError> {
        let bytes = self.random_bytes_32().await;
        Ok(OrderTime(bytes))
    }
}

#[async_trait]
impl BiscuitAuthorizationEffects for MockEffects {
    async fn authorize_biscuit(
        &self,
        _token_data: &[u8],
        _operation: AuthorizationOp,
        _scope: &ResourceScope,
    ) -> Result<AuthorizationDecision, AuthorizationError> {
        Ok(AuthorizationDecision {
            authorized: true,
            reason: Some("Mock authorization".to_string()),
        })
    }

    async fn authorize_fact(
        &self,
        _token_data: &[u8],
        _fact_type: &str,
        _scope: &ResourceScope,
    ) -> Result<bool, AuthorizationError> {
        Ok(true)
    }
}

#[async_trait]
impl FlowBudgetEffects for MockEffects {
    async fn charge_flow(
        &self,
        context: &ContextId,
        authority: &AuthorityId,
        cost: aura_core::FlowCost,
    ) -> aura_core::Result<Receipt> {
        let key = (*context, *authority);
        let mut state = self.state.lock().unwrap();

        // Create a new receipt for this flow charge
        let receipt = Receipt {
            ctx: *context,
            src: *authority,
            dst: *authority, // Self-charge for simplicity
            epoch: Epoch(0),
            cost,
            nonce: aura_core::FlowNonce::new(state.flow_receipts.len() as u64),
            prev: Hash32::new([0; 32]),
            sig: aura_core::ReceiptSig::new(vec![0xAB; 64])?,
        };

        state.flow_receipts.insert(key, receipt.clone());
        Ok(receipt)
    }
}

// Simplified implementations for completeness
#[async_trait]
impl JournalEffects for MockEffects {
    async fn merge_facts(&self, mut target: Journal, delta: Journal) -> Result<Journal, AuraError> {
        target.merge_facts(delta.facts);
        Ok(target)
    }

    async fn refine_caps(
        &self,
        mut target: Journal,
        refinement: Journal,
    ) -> Result<Journal, AuraError> {
        target.refine_caps(refinement.caps);
        Ok(target)
    }

    async fn get_journal(&self) -> Result<Journal, AuraError> {
        Ok(Journal::new())
    }

    async fn persist_journal(&self, _journal: &Journal) -> Result<(), AuraError> {
        Ok(())
    }

    async fn get_flow_budget(
        &self,
        context: &ContextId,
        authority: &AuthorityId,
    ) -> Result<FlowBudget, AuraError> {
        let key = (*context, *authority);
        let state = self.state.lock().unwrap();

        // Derive budget from receipts for mock implementation
        let total_spent = state
            .flow_receipts
            .get(&key)
            .map(|r| u64::from(r.cost))
            .unwrap_or(0);
        Ok(FlowBudget {
            limit: 1000,
            spent: total_spent,
            epoch: Epoch(0),
        })
    }

    async fn update_flow_budget(
        &self,
        _context: &ContextId,
        _authority: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError> {
        Ok(*budget)
    }

    async fn charge_flow_budget(
        &self,
        context: &ContextId,
        authority: &AuthorityId,
        cost: aura_core::FlowCost,
    ) -> Result<FlowBudget, AuraError> {
        let _receipt =
            <Self as FlowBudgetEffects>::charge_flow(self, context, authority, cost).await?;
        self.get_flow_budget(context, authority).await
    }
}

#[async_trait]
impl NetworkCoreEffects for MockEffects {
    async fn send_to_peer(&self, _peer_id: Uuid, _message: Vec<u8>) -> Result<(), NetworkError> {
        Ok(())
    }

    async fn broadcast(&self, _message: Vec<u8>) -> Result<(), NetworkError> {
        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        // Mock: return empty message with deterministic peer id
        use rand::Rng;
        let mut state = self.state.lock().unwrap();
        let bytes: [u8; 16] = state.rng.gen();
        Ok((Uuid::from_bytes(bytes), vec![]))
    }
}

#[async_trait]
impl NetworkExtendedEffects for MockEffects {
    async fn receive_from(&self, _peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        Ok(vec![])
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        vec![]
    }

    async fn is_peer_connected(&self, _peer_id: Uuid) -> bool {
        false
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        // Mock implementation - create a dummy stream
        use futures::stream;
        Ok(Box::pin(stream::empty()))
    }

    async fn open(&self, _address: &str) -> Result<String, NetworkError> {
        Err(NetworkError::NotImplemented)
    }

    async fn send(&self, _connection_id: &str, _data: Vec<u8>) -> Result<(), NetworkError> {
        Err(NetworkError::NotImplemented)
    }

    async fn close(&self, _connection_id: &str) -> Result<(), NetworkError> {
        Err(NetworkError::NotImplemented)
    }
}

#[derive(Debug, Default)]
struct MockUdpSocket;

#[async_trait]
impl UdpEndpointEffects for MockUdpSocket {
    async fn set_broadcast(&self, _enabled: bool) -> Result<(), NetworkError> {
        Err(NetworkError::NotImplemented)
    }

    async fn send_to(&self, _payload: &[u8], _addr: &UdpEndpoint) -> Result<usize, NetworkError> {
        Err(NetworkError::NotImplemented)
    }

    async fn recv_from(&self, _buffer: &mut [u8]) -> Result<(usize, UdpEndpoint), NetworkError> {
        Err(NetworkError::NotImplemented)
    }
}

#[async_trait]
impl UdpEffects for MockEffects {
    async fn udp_bind(
        &self,
        _addr: UdpEndpoint,
    ) -> Result<std::sync::Arc<dyn UdpEndpointEffects>, NetworkError> {
        Ok(std::sync::Arc::new(MockUdpSocket))
    }
}

// ThresholdSigningEffects implementation
#[async_trait]
impl aura_core::effects::ThresholdSigningEffects for MockEffects {
    async fn bootstrap_authority(&self, _authority: &AuthorityId) -> Result<Vec<u8>, AuraError> {
        // Return a mock public key package (32 bytes of zeros)
        Ok(vec![0u8; 32])
    }

    async fn sign(
        &self,
        _context: aura_core::threshold::SigningContext,
    ) -> Result<aura_core::threshold::ThresholdSignature, AuraError> {
        // Return a mock signature
        Ok(aura_core::threshold::ThresholdSignature {
            signature: vec![0u8; 64],
            signer_count: 1,
            signers: vec![1],
            public_key_package: vec![0u8; 32],
            epoch: 0,
        })
    }

    async fn threshold_config(
        &self,
        _authority: &AuthorityId,
    ) -> Option<aura_core::threshold::ThresholdConfig> {
        // Default 1-of-1 threshold
        Some(aura_core::threshold::ThresholdConfig {
            threshold: 1,
            total_participants: 1,
        })
    }

    async fn threshold_state(
        &self,
        authority: &AuthorityId,
    ) -> Option<aura_core::threshold::ThresholdState> {
        // Default 1-of-1 threshold state for mock
        Some(aura_core::threshold::ThresholdState {
            epoch: 0,
            threshold: 1,
            total_participants: 1,
            participants: vec![aura_core::threshold::ParticipantIdentity::guardian(
                *authority,
            )],
            agreement_mode: aura_core::threshold::AgreementMode::Provisional,
        })
    }

    async fn has_signing_capability(&self, _authority: &AuthorityId) -> bool {
        true
    }

    async fn public_key_package(&self, _authority: &AuthorityId) -> Option<Vec<u8>> {
        Some(vec![0u8; 32])
    }

    async fn rotate_keys(
        &self,
        _authority: &AuthorityId,
        new_threshold: u16,
        new_total_participants: u16,
        participants: &[aura_core::threshold::ParticipantIdentity],
    ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), AuraError> {
        // Return mock key packages for each participant
        let key_packages: Vec<Vec<u8>> = participants
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let mut pkg = vec![0u8; 64];
                pkg[0] = i as u8; // Make each package unique
                pkg[1] = new_threshold as u8;
                pkg[2] = new_total_participants as u8;
                pkg
            })
            .collect();

        let public_key_package = vec![0xAB; 32]; // Mock group public key
        let new_epoch = 1u64;

        Ok((new_epoch, key_packages, public_key_package))
    }

    async fn commit_key_rotation(
        &self,
        _authority: &AuthorityId,
        _new_epoch: u64,
    ) -> Result<(), AuraError> {
        // Mock commit - no-op
        Ok(())
    }

    async fn rollback_key_rotation(
        &self,
        _authority: &AuthorityId,
        _failed_epoch: u64,
    ) -> Result<(), AuraError> {
        // Mock rollback - no-op
        Ok(())
    }
}

// SecureStorageEffects implementation
#[async_trait]
impl aura_core::effects::SecureStorageEffects for MockEffects {
    async fn secure_store(
        &self,
        location: &aura_core::effects::SecureStorageLocation,
        data: &[u8],
        _capabilities: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<(), AuraError> {
        // Use regular storage with a secure_ prefix
        let key = format!("secure_{}", location.full_path());
        let mut state = self.state.lock().unwrap();
        state.storage.insert(key, data.to_vec());
        Ok(())
    }

    async fn secure_retrieve(
        &self,
        location: &aura_core::effects::SecureStorageLocation,
        _required_capabilities: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<Vec<u8>, AuraError> {
        let key = format!("secure_{}", location.full_path());
        let state = self.state.lock().unwrap();
        state
            .storage
            .get(&key)
            .cloned()
            .ok_or_else(|| AuraError::storage(format!("Secure key not found: {key}")))
    }

    async fn secure_delete(
        &self,
        location: &aura_core::effects::SecureStorageLocation,
        _required_capabilities: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<(), AuraError> {
        let key = format!("secure_{}", location.full_path());
        let mut state = self.state.lock().unwrap();
        state.storage.remove(&key);
        Ok(())
    }

    async fn secure_exists(
        &self,
        location: &aura_core::effects::SecureStorageLocation,
    ) -> Result<bool, AuraError> {
        let key = format!("secure_{}", location.full_path());
        let state = self.state.lock().unwrap();
        Ok(state.storage.contains_key(&key))
    }

    async fn secure_list_keys(
        &self,
        namespace: &str,
        _required_capabilities: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<Vec<String>, AuraError> {
        let prefix = format!("secure_{namespace}/");
        let state = self.state.lock().unwrap();
        let keys: Vec<String> = state
            .storage
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .map(|k| k.strip_prefix(&prefix).unwrap_or(k).to_string())
            .collect();
        Ok(keys)
    }

    async fn secure_generate_key(
        &self,
        location: &aura_core::effects::SecureStorageLocation,
        key_type: &str,
        _capabilities: &[aura_core::effects::SecureStorageCapability],
    ) -> Result<Option<Vec<u8>>, AuraError> {
        // Generate a deterministic key based on location and type
        let key_bytes: Vec<u8> = match key_type {
            "ed25519" => vec![1u8; 32],
            "frost-share" => vec![2u8; 64],
            _ => vec![0u8; 32],
        };
        // Store private key
        let key = format!("secure_{}", location.full_path());
        let mut state = self.state.lock().unwrap();
        state.storage.insert(key, key_bytes.clone());
        // Return public key (mock: same as private for testing)
        Ok(Some(key_bytes))
    }

    async fn secure_create_time_bound_token(
        &self,
        location: &aura_core::effects::SecureStorageLocation,
        _capabilities: &[aura_core::effects::SecureStorageCapability],
        expires_at: &aura_core::time::PhysicalTime,
    ) -> Result<Vec<u8>, AuraError> {
        // Create a simple mock token (location hash + expiry)
        let mut token = location.full_path().as_bytes().to_vec();
        token.extend_from_slice(&expires_at.ts_ms.to_le_bytes());
        Ok(token)
    }

    async fn secure_access_with_token(
        &self,
        _token: &[u8],
        location: &aura_core::effects::SecureStorageLocation,
    ) -> Result<Vec<u8>, AuraError> {
        // Simply retrieve the data (mock doesn't validate token)
        self.secure_retrieve(location, &[]).await
    }

    async fn get_device_attestation(&self) -> Result<Vec<u8>, AuraError> {
        // Return mock attestation
        Ok(b"mock_device_attestation".to_vec())
    }

    async fn is_secure_storage_available(&self) -> bool {
        true
    }

    fn get_secure_storage_capabilities(&self) -> Vec<String> {
        vec![
            "read".to_string(),
            "write".to_string(),
            "delete".to_string(),
            "list".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_effects_deterministic() {
        let effects1 = MockEffects::deterministic();
        let effects2 = MockEffects::deterministic();

        let bytes1 = effects1.random_bytes_32().await;
        let bytes2 = effects2.random_bytes_32().await;

        assert_eq!(bytes1, bytes2);
    }

    #[tokio::test]
    async fn test_mock_storage() {
        let effects = MockEffects::deterministic();

        effects
            .store("test_key", b"test_value".to_vec())
            .await
            .unwrap();
        let value = effects.retrieve("test_key").await.unwrap();

        assert_eq!(value, Some(b"test_value".to_vec()));
    }

    #[tokio::test]
    async fn test_mock_crypto() {
        let effects = MockEffects::deterministic();

        let (priv_key, pub_key) = effects.ed25519_generate_keypair().await.unwrap();
        let signature = effects.ed25519_sign(b"message", &priv_key).await.unwrap();
        let verified = effects
            .ed25519_verify(b"message", &signature, &pub_key)
            .await
            .unwrap();

        assert!(verified);
    }

    #[tokio::test]
    async fn test_mock_time() {
        let effects = MockEffects::deterministic();

        let time1 = effects.physical_time().await.unwrap();
        effects.sleep_ms(1000).await.unwrap();
        let time2 = effects.physical_time().await.unwrap();

        assert_eq!(time2.ts_ms - time1.ts_ms, 1000);
    }
}
