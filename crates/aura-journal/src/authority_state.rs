//! Authority state derivation from journal facts
//!
//! This module provides the implementation for deriving authority state
//! from the fact-based journal, implementing the Authority trait.

use crate::{fact::Journal, reduction::reduce_account_facts};
use async_trait::async_trait;
use aura_core::effects::CryptoEffects;
use aura_core::{authority::TreeState, AuraError, Authority, AuthorityId, Hash32, Result};
use ed25519_dalek::{Signature, VerifyingKey as PublicKey};

/// Authority state derived from facts
#[derive(Debug, Clone)]
pub struct AuthorityState {
    /// Tree state derived from attested operations
    pub tree_state: TreeState,

    /// Threshold signing context (placeholder)
    pub threshold_context: Option<Vec<u8>>,
}

impl AuthorityState {
    /// Sign with threshold - internal implementation
    pub async fn sign_with_threshold<E: CryptoEffects>(
        &self,
        effects: &E,
        data: &[u8],
    ) -> Result<Signature> {
        // Get the root public key for signing context
        let _public_key = self.tree_state.root_key();

        // Delegate to FROST threshold signing coordination via effects
        self.coordinate_frost_threshold_signing(effects, data).await
    }

    /// Coordinate FROST threshold signing across devices via effects delegation
    ///
    /// This properly delegates FROST operations to the effects system, following
    /// the architectural pattern: Layer 2 (journal) → Layer 3 (effects) → Layer 5 (frost).
    async fn coordinate_frost_threshold_signing<E: CryptoEffects>(
        &self,
        _effects: &E,
        _data: &[u8],
    ) -> Result<Signature> {
        // Get threshold requirements from tree state
        let threshold = self.tree_state.threshold();
        let device_count = self.tree_state.device_count();

        // Validate we have sufficient devices for threshold
        if device_count < threshold as u32 {
            return Err(AuraError::invalid(format!(
                "Insufficient devices for threshold signing: have {}, need {}",
                device_count, threshold
            )));
        }

        // For now, return an error indicating this requires full FROST protocol implementation
        // This is a proper architectural boundary - Layer 2 should delegate but not implement
        Err(AuraError::internal(
            "FROST threshold signing requires full protocol implementation. \
             This should be coordinated through aura-frost crate via the orchestration layer.",
        ))

        // TODO: When FROST architecture is fully implemented, this should:
        // 1. Extract device key shares from secure storage via effects
        // 2. Coordinate FROST signing session via transport effects
        // 3. Collect nonce commitments from threshold participants
        // 4. Create signing package with message and commitments
        // 5. Collect partial signatures from participants
        // 6. Aggregate partial signatures using effects.frost_aggregate_signatures()
        // 7. Verify final signature using effects.frost_verify()
        // 8. Return the aggregated signature as ed25519_dalek::Signature
    }
}

/// Derived authority implementation
///
/// This implements the Authority trait by deriving all state from
/// the journal facts. No device information is exposed externally.
#[derive(Debug, Clone)]
pub struct DerivedAuthority {
    /// Authority identifier
    id: AuthorityId,

    /// Derived state
    state: AuthorityState,
}

impl DerivedAuthority {
    /// Create from journal by reduction
    pub fn from_journal(authority_id: AuthorityId, journal: &Journal) -> Result<Self> {
        // Reduce journal facts to authority state
        let state = reduce_authority_state(authority_id, journal)?;

        Ok(Self {
            id: authority_id,
            state,
        })
    }

    /// Sign operation with proper effects delegation
    ///
    /// This method provides the correct architectural pattern for threshold signing:
    /// Layer 2 (journal) → Layer 3 (effects) → Layer 5 (frost) → Layer 4 (orchestration)
    pub async fn sign_operation_with_effects<E: CryptoEffects>(
        &self,
        effects: &E,
        operation: &[u8],
    ) -> Result<Signature> {
        // Delegate to internal threshold signing with effects
        self.state.sign_with_threshold(effects, operation).await
    }
}

#[async_trait]
impl Authority for DerivedAuthority {
    fn authority_id(&self) -> AuthorityId {
        self.id
    }

    fn public_key(&self) -> PublicKey {
        // Get root public key from tree state
        self.state.tree_state.root_key()
    }

    fn root_commitment(&self) -> Hash32 {
        self.state.tree_state.root_commitment()
    }

    async fn sign_operation(&self, _operation: &[u8]) -> Result<Signature> {
        // TODO: The Authority trait needs to be updated to accept effects parameter
        // For now, return an error that shows the proper delegation pattern
        Err(AuraError::internal(
            "Authority trait sign_operation requires effects parameter for FROST delegation. \
             Use DerivedAuthority::sign_operation_with_effects() instead.",
        ))
    }

    fn get_threshold(&self) -> u16 {
        self.state.tree_state.threshold()
    }

    fn active_device_count(&self) -> usize {
        self.state.tree_state.device_count() as usize
    }
}

/// Reduce journal facts to authority state
pub fn reduce_authority_state(
    _authority_id: AuthorityId,
    journal: &Journal,
) -> Result<AuthorityState> {
    // Use the reduction function to get tree state from facts
    let tree_state = reduce_account_facts(journal);

    Ok(AuthorityState {
        tree_state,
        threshold_context: None,
    })
}
