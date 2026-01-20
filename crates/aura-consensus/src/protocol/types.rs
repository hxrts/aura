//! Protocol types and parameters
//!
//! This module contains types used for consensus protocol execution.

use super::ConsensusProtocol;
use crate::types::{CommitFact, ConsensusConfig};
use aura_core::{
    effects::{PhysicalTimeEffects, RandomEffects},
    epochs::Epoch,
    frost::{PublicKeyPackage, Share},
    AuraError, AuthorityId, ContextId, Prestate, Result,
};
use std::collections::HashMap;

/// Protocol statistics
#[derive(Debug, Clone)]
pub struct ProtocolStats {
    pub active_instances: usize,
    pub epoch: Epoch,
    pub threshold: u16,
    pub witness_count: usize,
}

/// Parameters for consensus execution
pub struct ConsensusParams {
    pub context_id: ContextId,
    pub witnesses: Vec<AuthorityId>,
    pub threshold: u16,
    pub key_packages: HashMap<AuthorityId, Share>,
    pub group_public_key: PublicKeyPackage,
    pub epoch: Epoch,
}

/// Run consensus with default configuration
pub async fn run_consensus<T: serde::Serialize>(
    prestate: &Prestate,
    operation: &T,
    params: ConsensusParams,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<CommitFact> {
    let config = ConsensusConfig::new(params.threshold, params.witnesses, params.epoch)?;
    // Derive coordinator ID deterministically from the prestate hash to keep coordination scoped to the instance.
    let prestate_hash = prestate.compute_hash();
    let mut entropy = [0u8; 32];
    entropy.copy_from_slice(&prestate_hash.0);
    let authority_id = AuthorityId::new_from_entropy(entropy);

    let protocol = ConsensusProtocol::new(
        authority_id,
        params.context_id,
        config,
        params.key_packages,
        params.group_public_key,
    )?;

    let response = protocol
        .run_consensus(prestate, operation, random, time)
        .await?;

    match response.result {
        Ok(commit_fact) => Ok(commit_fact),
        Err(e) => Err(AuraError::internal(e.to_string())),
    }
}
