//! Witness management for consensus
//!
//! This module handles witness state, nonce management, and quorum tracking
//! for consensus operations. It unifies the previous witness.rs and witness_state.rs
//! modules into a cohesive design.

use super::types::ConsensusId;
use aura_core::{
    crypto::tree_signing::NonceToken,
    epochs::Epoch,
    frost::{NonceCommitment, PartialSignature},
    AuthorityId, Hash32, Result,
};
use serde::{Deserialize, Serialize};
// use rand_chacha::rand_core::SeedableRng; // Used in tests
use async_lock::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Set of witnesses participating in consensus
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "NonEmptyWitnessSetSerde")]
pub struct NonEmptyWitnessSet {
    threshold: u16,
    witnesses: Vec<AuthorityId>,
}

#[derive(Debug, Clone, Deserialize)]
struct NonEmptyWitnessSetSerde {
    threshold: u16,
    witnesses: Vec<AuthorityId>,
}

impl TryFrom<NonEmptyWitnessSetSerde> for NonEmptyWitnessSet {
    type Error = aura_core::AuraError;

    fn try_from(value: NonEmptyWitnessSetSerde) -> std::result::Result<Self, Self::Error> {
        Self::new(value.threshold, value.witnesses)
    }
}

impl NonEmptyWitnessSet {
    /// Create a new non-empty witness set with threshold validation
    pub fn new(threshold: u16, witnesses: Vec<AuthorityId>) -> Result<Self> {
        if witnesses.is_empty() {
            return Err(aura_core::AuraError::invalid(
                "Consensus requires at least one witness",
            ));
        }

        if threshold == 0 {
            return Err(aura_core::AuraError::invalid(
                "Consensus threshold must be >= 1",
            ));
        }

        if witnesses.len() < threshold as usize {
            return Err(aura_core::AuraError::invalid(
                "Consensus threshold exceeds witness set size",
            ));
        }

        Ok(Self {
            threshold,
            witnesses,
        })
    }

    /// Required threshold for consensus
    pub fn threshold(&self) -> u16 {
        self.threshold
    }

    /// Borrow witnesses as a slice
    pub fn witnesses(&self) -> &[AuthorityId] {
        &self.witnesses
    }

    /// Consume into the witness vector
    pub fn into_witnesses(self) -> Vec<AuthorityId> {
        self.witnesses
    }

    /// Iterate over witnesses
    pub fn iter(&self) -> std::slice::Iter<'_, AuthorityId> {
        self.witnesses.iter()
    }

    /// Number of witnesses
    pub fn len(&self) -> usize {
        self.witnesses.len()
    }

    /// Always returns false since this set is guaranteed non-empty.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Check if we have sufficient witnesses for the threshold
    pub fn has_quorum(&self) -> bool {
        self.witnesses.len() >= self.threshold as usize
    }

    /// Build a runtime witness set with cached state
    pub fn to_runtime(&self) -> Result<WitnessSet> {
        WitnessSet::new(self.threshold, self.witnesses.clone())
    }
}

impl TryFrom<&NonEmptyWitnessSet> for WitnessSet {
    type Error = aura_core::AuraError;

    fn try_from(value: &NonEmptyWitnessSet) -> std::result::Result<Self, Self::Error> {
        WitnessSet::new(value.threshold, value.witnesses.clone())
    }
}

/// Set of witnesses participating in consensus
#[derive(Debug, Clone)]
pub struct WitnessSet {
    /// Required threshold for consensus
    pub threshold: u16,

    /// List of witness authorities
    pub witnesses: Vec<AuthorityId>,

    /// Cached witness states for pipelining optimization
    states: Arc<RwLock<HashMap<AuthorityId, WitnessState>>>,
}

impl WitnessSet {
    /// Create a new witness set
    pub fn new(threshold: u16, witnesses: Vec<AuthorityId>) -> Result<Self> {
        if witnesses.is_empty() {
            return Err(aura_core::AuraError::invalid(
                "Consensus requires at least one witness",
            ));
        }

        if threshold == 0 {
            return Err(aura_core::AuraError::invalid(
                "Consensus threshold must be >= 1",
            ));
        }

        if witnesses.len() < threshold as usize {
            return Err(aura_core::AuraError::invalid(
                "Consensus threshold exceeds witness set size",
            ));
        }

        Ok(Self {
            threshold,
            witnesses,
            states: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Check if we have sufficient witnesses for consensus
    pub fn has_quorum(&self) -> bool {
        self.witnesses.len() >= self.threshold as usize
    }

    /// Get or create witness state for a given authority
    pub async fn get_or_create_state(&self, witness_id: AuthorityId, epoch: Epoch) -> WitnessState {
        let mut states = self.states.write().await;

        states
            .entry(witness_id)
            .or_insert_with(|| WitnessState::new(witness_id, epoch))
            .clone()
    }

    /// Update witness state with a new cached nonce
    pub async fn update_witness_nonce(
        &self,
        witness_id: AuthorityId,
        commitment: NonceCommitment,
        token: NonceToken,
        epoch: Epoch,
    ) -> Result<()> {
        let mut states = self.states.write().await;

        let state = states
            .entry(witness_id)
            .or_insert_with(|| WitnessState::new(witness_id, epoch));

        state.set_next_nonce(commitment, token, epoch);
        Ok(())
    }

    /// Collect available next-round commitments for fast path
    pub async fn collect_cached_commitments(
        &self,
        epoch: Epoch,
    ) -> HashMap<AuthorityId, NonceCommitment> {
        let states = self.states.read().await;
        let mut commitments = HashMap::new();

        for (witness_id, state) in states.iter() {
            if let Some(commitment) = state.get_next_commitment(epoch) {
                commitments.insert(*witness_id, commitment.clone());
            }
        }

        commitments
    }

    /// Check if we have enough cached commitments for fast path (1 RTT)
    pub async fn has_fast_path_quorum(&self, epoch: Epoch) -> bool {
        let cached_count = self.collect_cached_commitments(epoch).await.len();
        cached_count >= self.threshold as usize
    }

    /// Invalidate all cached nonces (e.g., on epoch change)
    pub async fn invalidate_all_caches(&self) {
        let mut states = self.states.write().await;

        for state in states.values_mut() {
            state.invalidate();
        }
    }
}

/// State for a single witness across consensus rounds
#[derive(Debug, Clone)]
pub struct WitnessState {
    /// Witness identifier
    witness_id: AuthorityId,

    /// Current epoch
    epoch: Epoch,

    /// Cached nonce for next round (pipelining optimization)
    next_nonce: Option<(NonceCommitment, NonceToken)>,

    /// Active consensus instances this witness is participating in
    active_instances: HashMap<ConsensusId, WitnessInstance>,
}

impl WitnessState {
    /// Create a new witness state
    pub fn new(witness_id: AuthorityId, epoch: Epoch) -> Self {
        Self {
            witness_id,
            epoch,
            next_nonce: None,
            active_instances: HashMap::new(),
        }
    }

    /// Get the cached commitment for the next round if available
    pub fn get_next_commitment(&self, current_epoch: Epoch) -> Option<&NonceCommitment> {
        if self.epoch != current_epoch {
            // Epoch changed, cached commitment is stale
            return None;
        }

        self.next_nonce.as_ref().map(|(commitment, _)| commitment)
    }

    /// Take the cached nonce for use in the current round
    pub fn take_nonce(&mut self, current_epoch: Epoch) -> Option<(NonceCommitment, NonceToken)> {
        if self.epoch != current_epoch {
            // Epoch changed, invalidate cached nonce
            self.next_nonce = None;
            self.epoch = current_epoch;
            return None;
        }

        self.next_nonce.take()
    }

    /// Cache a new nonce for the next round
    pub fn set_next_nonce(&mut self, commitment: NonceCommitment, token: NonceToken, epoch: Epoch) {
        self.epoch = epoch;
        self.next_nonce = Some((commitment, token));
    }

    /// Check if we have a cached nonce ready
    pub fn has_cached_nonce(&self, current_epoch: Epoch) -> bool {
        self.epoch == current_epoch && self.next_nonce.is_some()
    }

    /// Invalidate cached nonce
    pub fn invalidate(&mut self) {
        self.next_nonce = None;
    }

    /// Start participating in a consensus instance
    pub fn start_instance(&mut self, instance: WitnessInstance) {
        self.active_instances
            .insert(instance.consensus_id, instance);
    }

    /// Get an active instance
    pub fn get_instance(&self, consensus_id: &ConsensusId) -> Option<&WitnessInstance> {
        self.active_instances.get(consensus_id)
    }

    /// Get a mutable reference to an active instance
    pub fn get_instance_mut(&mut self, consensus_id: &ConsensusId) -> Option<&mut WitnessInstance> {
        self.active_instances.get_mut(consensus_id)
    }

    /// Complete an instance and remove it
    pub fn complete_instance(&mut self, consensus_id: &ConsensusId) -> Option<WitnessInstance> {
        self.active_instances.remove(consensus_id)
    }

    /// Check if witness is participating in any active instances
    pub fn has_active_instances(&self) -> bool {
        !self.active_instances.is_empty()
    }
}

/// State for a witness within a single consensus instance
#[derive(Debug, Clone)]
pub struct WitnessInstance {
    /// Consensus instance ID
    pub consensus_id: ConsensusId,

    /// Prestate hash
    pub prestate_hash: Hash32,

    /// Operation being agreed upon
    pub operation_hash: Hash32,
    pub operation_bytes: Vec<u8>,

    /// This witness's nonce commitment (if generated)
    pub nonce_commitment: Option<NonceCommitment>,

    /// This witness's partial signature (if generated)
    pub partial_signature: Option<PartialSignature>,

    /// Aggregated nonces from all witnesses (for signing)
    pub aggregated_nonces: Vec<NonceCommitment>,

    /// Whether this instance used fast path
    pub fast_path: bool,
}

impl WitnessInstance {
    /// Create a new witness instance
    pub fn new(
        consensus_id: ConsensusId,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        operation_bytes: Vec<u8>,
        fast_path: bool,
    ) -> Self {
        Self {
            consensus_id,
            prestate_hash,
            operation_hash,
            operation_bytes,
            nonce_commitment: None,
            partial_signature: None,
            aggregated_nonces: Vec::new(),
            fast_path,
        }
    }

    /// Set the nonce commitment for this witness
    pub fn set_nonce_commitment(&mut self, commitment: NonceCommitment) {
        self.nonce_commitment = Some(commitment);
    }

    /// Set the aggregated nonces for signing
    pub fn set_aggregated_nonces(&mut self, nonces: Vec<NonceCommitment>) {
        self.aggregated_nonces = nonces;
    }

    /// Set the partial signature for this witness
    pub fn set_partial_signature(&mut self, signature: PartialSignature) {
        self.partial_signature = Some(signature);
    }

    /// Check if witness has completed all required steps
    pub fn is_complete(&self) -> bool {
        // Fast path doesn't need separate nonce commitment
        let nonce_ok = self.fast_path || self.nonce_commitment.is_some();
        nonce_ok && self.partial_signature.is_some()
    }
}

/// Tracks collected witness data during consensus
#[derive(Debug, Clone)]
pub struct WitnessTracker {
    /// Collected nonce commitments by witness
    pub nonce_commitments: HashMap<AuthorityId, NonceCommitment>,

    /// Share collector with type-safe threshold tracking
    share_collector: crate::shares::ShareCollector,

    /// Threshold for this consensus
    threshold: u32,

    /// Primary result_id (first one seen) for backward compatibility
    primary_result_id: Option<Hash32>,

    /// Witnesses that reported conflicts
    pub conflict_reporters: HashMap<AuthorityId, Vec<Hash32>>,

    /// Equivocation detector for generating proofs
    equivocation_detector: crate::core::validation::EquivocationDetector,

    /// Accumulated equivocation proofs detected during this consensus
    equivocation_proofs: Vec<crate::facts::ConsensusFact>,
}

impl Default for WitnessTracker {
    fn default() -> Self {
        Self::with_threshold(1)
    }
}

impl WitnessTracker {
    /// Create a new witness tracker with default threshold of 1
    pub fn new() -> Self {
        Self::with_threshold(1)
    }

    /// Create a new witness tracker with specified threshold
    pub fn with_threshold(threshold: u32) -> Self {
        Self {
            nonce_commitments: HashMap::new(),
            share_collector: crate::shares::ShareCollector::new(threshold),
            threshold,
            primary_result_id: None,
            conflict_reporters: HashMap::new(),
            equivocation_detector: crate::core::validation::EquivocationDetector::new(),
            equivocation_proofs: Vec::new(),
        }
    }

    /// Add a nonce commitment
    pub fn add_nonce(&mut self, witness: AuthorityId, commitment: NonceCommitment) {
        self.nonce_commitments.insert(witness, commitment);
    }

    /// Add a partial signature for a given result_id
    ///
    /// This method integrates with ShareCollector to track which result_id each witness voted for.
    /// Returns Ok(Some(threshold_set)) if this signature caused threshold to be reached for its result_id.
    pub fn add_signature(
        &mut self,
        witness: AuthorityId,
        signature: PartialSignature,
        result_id: Hash32,
    ) -> Result<Option<crate::shares::ThresholdShareSet>> {
        // Track primary result_id (first one seen)
        if self.primary_result_id.is_none() {
            self.primary_result_id = Some(result_id);
        }

        match self
            .share_collector
            .try_insert(result_id, witness, signature)?
        {
            crate::shares::InsertResult::Inserted { .. } => Ok(None),
            crate::shares::InsertResult::ThresholdReached(threshold_set) => Ok(Some(threshold_set)),
        }
    }

    /// Add a conflict report
    pub fn add_conflict(&mut self, witness: AuthorityId, conflicts: Vec<Hash32>) {
        self.conflict_reporters.insert(witness, conflicts);
    }

    /// Check if we have enough nonces for threshold
    pub fn has_nonce_threshold(&self, threshold: u16) -> bool {
        self.nonce_commitments.len() >= threshold as usize
    }

    /// Check if any result_id has reached threshold
    pub fn has_signature_threshold(&self, _threshold: u16) -> bool {
        // Check if any result_id has reached threshold
        self.share_collector
            .result_ids()
            .iter()
            .any(|rid| self.share_collector.has_threshold(rid))
    }

    /// Check if a specific result_id has reached threshold
    pub fn has_threshold_for_result(&self, result_id: &Hash32) -> bool {
        self.share_collector.has_threshold(result_id)
    }

    /// Get the result_id that reached threshold, if any
    pub fn get_threshold_result(&self) -> Option<Hash32> {
        self.share_collector
            .result_ids()
            .into_iter()
            .find(|rid| self.share_collector.has_threshold(rid))
    }

    /// Get all result_ids being tracked
    pub fn get_result_ids(&self) -> Vec<Hash32> {
        self.share_collector.result_ids()
    }

    /// Check if we have conflicts
    pub fn has_conflicts(&self) -> bool {
        !self.conflict_reporters.is_empty()
    }

    /// Get all collected nonces as a vector
    pub fn get_nonces(&self) -> Vec<NonceCommitment> {
        self.nonce_commitments.values().cloned().collect()
    }

    /// Get all collected signatures across all result_ids
    pub fn get_signatures(&self) -> Vec<PartialSignature> {
        self.share_collector
            .result_ids()
            .iter()
            .flat_map(|rid| self.share_collector.get_signatures_for_result(rid))
            .collect()
    }

    /// Get all signatures for a specific result_id
    pub fn get_signatures_for_result(&self, result_id: &Hash32) -> Vec<PartialSignature> {
        self.share_collector.get_signatures_for_result(result_id)
    }

    /// Get all participating witnesses across all result_ids
    pub fn get_participants(&self) -> Vec<AuthorityId> {
        self.share_collector
            .result_ids()
            .iter()
            .flat_map(|rid| self.share_collector.get_participants_for_result(rid))
            .collect()
    }

    /// Get participating witnesses for a specific result_id
    pub fn get_participants_for_result(&self, result_id: &Hash32) -> Vec<AuthorityId> {
        self.share_collector.get_participants_for_result(result_id)
    }

    /// Get accumulated equivocation proofs
    pub fn get_equivocation_proofs(&self) -> &[crate::facts::ConsensusFact] {
        &self.equivocation_proofs
    }

    /// Record a signature with equivocation detection
    ///
    /// This is an enhanced version of add_signature that checks for equivocation.
    /// Call this when you have full context available (consensus_id, prestate_hash, etc.)
    pub fn record_signature_with_detection(
        &mut self,
        context_id: aura_core::identifiers::ContextId,
        witness: AuthorityId,
        signature: PartialSignature,
        consensus_id: crate::ConsensusId,
        prestate_hash: Hash32,
        result_id: Hash32,
        timestamp_ms: u64,
    ) {
        // Check for equivocation before adding signature
        if let Some(proof) = self.equivocation_detector.check_share(
            context_id,
            witness,
            consensus_id,
            prestate_hash,
            result_id,
            timestamp_ms,
        ) {
            // Store proof for later retrieval
            self.equivocation_proofs.push(proof);
            // Don't add the equivocating signature
            tracing::warn!(
                witness = %witness,
                consensus_id = %consensus_id,
                "Equivocation detected and recorded"
            );
            return;
        }

        // No equivocation - add signature normally via ShareCollector
        let _ = self.add_signature(witness, signature, result_id);
    }

    /// Clear accumulated equivocation proofs
    ///
    /// Call this after extracting proofs to prevent duplicate emission
    pub fn clear_equivocation_proofs(&mut self) {
        self.equivocation_proofs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[tokio::test]
    async fn test_witness_set_fast_path() {
        let witnesses = vec![
            AuthorityId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
            AuthorityId::new_from_entropy([3u8; 32]),
        ];
        let witness_set = WitnessSet::new(2, witnesses.clone()).unwrap();

        // Initially no cached commitments
        assert!(!witness_set.has_fast_path_quorum(Epoch::from(1)).await);

        // Add cached commitment for one witness (not enough for quorum)
        witness_set
            .update_witness_nonce(
                witnesses[0],
                NonceCommitment {
                    signer: 1,
                    commitment: vec![1u8; 32],
                },
                // Note: In real usage, this would be a proper NonceToken
                NonceToken::from(frost_ed25519::round1::SigningNonces::new(
                    &frost_ed25519::keys::SigningShare::deserialize([1u8; 32]).unwrap(),
                    &mut rand_chacha::ChaCha20Rng::from_seed([7u8; 32]),
                )),
                Epoch::from(1),
            )
            .await
            .unwrap();

        assert!(!witness_set.has_fast_path_quorum(Epoch::from(1)).await);
    }

    #[test]
    fn test_witness_tracker() {
        let mut tracker = WitnessTracker::with_threshold(2);
        let witness1 = AuthorityId::new_from_entropy([1u8; 32]);
        let witness2 = AuthorityId::new_from_entropy([2u8; 32]);
        let result_id = Hash32::new([0u8; 32]);

        // Add nonces
        tracker.add_nonce(
            witness1,
            NonceCommitment {
                signer: 1,
                commitment: vec![1u8; 32],
            },
        );
        tracker.add_nonce(
            witness2,
            NonceCommitment {
                signer: 2,
                commitment: vec![2u8; 32],
            },
        );

        assert!(tracker.has_nonce_threshold(2));
        assert!(!tracker.has_signature_threshold(2));

        // Add signatures
        let _ = tracker.add_signature(
            witness1,
            PartialSignature {
                signer: 1,
                signature: vec![1u8; 64],
            },
            result_id,
        );
        let _ = tracker.add_signature(
            witness2,
            PartialSignature {
                signer: 2,
                signature: vec![2u8; 64],
            },
            result_id,
        );

        assert!(tracker.has_signature_threshold(2));
        assert_eq!(tracker.get_participants().len(), 2);
    }
}
