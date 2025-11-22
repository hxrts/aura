//! Authority state derivation from journal facts
//!
//! This module provides the implementation for deriving authority state
//! from the fact-based journal, implementing the Authority trait.

use crate::{fact::Journal, reduction::reduce_account_facts};
use async_trait::async_trait;
use aura_core::{authority::TreeState, Authority, AuthorityId, Hash32, Result};
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
    pub async fn sign_with_threshold(&self, data: &[u8]) -> Result<Signature> {
        // Get the root public key for signing context
        let _public_key = self.tree_state.root_key();

        // TODO: Implement actual FROST threshold signing coordination
        // This would:
        // 1. Coordinate nonce generation across threshold devices
        // 2. Distribute partial signatures
        // 3. Aggregate into final signature
        // For now, create a deterministic placeholder signature for testing

        use ed25519_dalek::Signer;
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
        let signature = signing_key.sign(data);

        Ok(signature)
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

    async fn sign_operation(&self, operation: &[u8]) -> Result<Signature> {
        // Delegate to internal threshold signing
        self.state.sign_with_threshold(operation).await
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
