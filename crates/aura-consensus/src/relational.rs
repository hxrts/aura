//! Relational consensus adapter for cross-authority coordination
//!
//! This module provides consensus mechanisms for relational contexts,
//! adapting the unified consensus protocol for cross-authority operations
//! like guardian bindings and recovery grants.

use super::{
    protocol::run_consensus as run_protocol_consensus,
    types::{CommitFact, ConsensusConfig},
};
use aura_core::{
    effects::{PhysicalTimeEffects, RandomEffects},
    epochs::Epoch,
    frost::{PublicKeyPackage, Share},
    relational::ConsensusProof,
    AuraError, AuthorityId, Prestate, Result,
};
use serde::Serialize;
use std::collections::HashMap;

/// Run consensus on an operation for relational contexts
///
/// Uses all authorities from the prestate as witnesses with a simple
/// majority threshold. This is suitable for most relational operations.
pub async fn run_consensus<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    epoch: Epoch,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<ConsensusProof> {
    // Extract witnesses from prestate
    let witnesses: Vec<_> = prestate
        .authority_commitments
        .iter()
        .map(|(id, _)| *id)
        .collect();

    // Simple majority threshold
    let threshold = (witnesses.len() as u16).div_ceil(2).max(1);

    let config = ConsensusConfig::new(threshold, witnesses, epoch)?;

    run_consensus_with_config(
        prestate,
        operation,
        config,
        key_packages,
        group_public_key,
        random,
        time,
    )
    .await
}

/// Run consensus on an operation and return both the proof and commit fact.
///
/// Uses all authorities from the prestate as witnesses with a simple
/// majority threshold. This is suitable for most relational operations.
pub async fn run_consensus_with_commit<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    epoch: Epoch,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<(ConsensusProof, CommitFact)> {
    let witnesses: Vec<_> = prestate
        .authority_commitments
        .iter()
        .map(|(id, _)| *id)
        .collect();

    let threshold = (witnesses.len() as u16).div_ceil(2).max(1);
    let config = ConsensusConfig::new(threshold, witnesses, epoch)?;

    run_consensus_with_config_and_commit(
        prestate,
        operation,
        config,
        key_packages,
        group_public_key,
        random,
        time,
    )
    .await
}

/// Run consensus with explicit configuration for relational contexts
///
/// Provides fine-grained control over witness selection and thresholds
/// for specialized relational operations.
pub async fn run_consensus_with_config<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    config: ConsensusConfig,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<ConsensusProof> {
    // Validate configuration
    if config.witness_set.is_empty() {
        return Err(AuraError::invalid(
            "Consensus requires at least one witness",
        ));
    }

    if !config.has_quorum() {
        return Err(AuraError::invalid(
            "Consensus threshold exceeds witness set size",
        ));
    }

    run_consensus_with_effects(
        prestate,
        operation,
        config,
        key_packages,
        group_public_key,
        random,
        time,
    )
    .await
}

/// Run consensus with explicit configuration and return proof + commit fact.
pub async fn run_consensus_with_config_and_commit<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    config: ConsensusConfig,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<(ConsensusProof, CommitFact)> {
    if config.witness_set.is_empty() {
        return Err(AuraError::invalid(
            "Consensus requires at least one witness",
        ));
    }

    if !config.has_quorum() {
        return Err(AuraError::invalid(
            "Consensus threshold exceeds witness set size",
        ));
    }

    run_consensus_with_effects_and_commit(
        prestate,
        operation,
        config,
        key_packages,
        group_public_key,
        random,
        time,
    )
    .await
}

/// Run consensus with custom effects (for testing)
pub async fn run_consensus_with_effects<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    config: ConsensusConfig,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<ConsensusProof> {
    // Run the unified consensus protocol
    let params = crate::protocol::ConsensusParams {
        witnesses: config.witness_set.clone(),
        threshold: config.threshold,
        key_packages,
        group_public_key,
        epoch: config.epoch,
    };
    let commit_fact = run_protocol_consensus(prestate, operation, params, random, time).await?;

    // Convert CommitFact to ConsensusProof for relational contexts
    commit_fact_to_consensus_proof(commit_fact)
}

/// Run consensus with custom effects and return proof + commit fact.
pub async fn run_consensus_with_effects_and_commit<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    config: ConsensusConfig,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<(ConsensusProof, CommitFact)> {
    let params = crate::protocol::ConsensusParams {
        witnesses: config.witness_set.clone(),
        threshold: config.threshold,
        key_packages,
        group_public_key,
        epoch: config.epoch,
    };
    let commit_fact = run_protocol_consensus(prestate, operation, params, random, time).await?;
    let proof = commit_fact_to_consensus_proof(commit_fact.clone())?;

    Ok((proof, commit_fact))
}

/// Convert a CommitFact to a ConsensusProof
fn commit_fact_to_consensus_proof(fact: CommitFact) -> Result<ConsensusProof> {
    // Extract the threshold signature
    let threshold_signature = Some(fact.threshold_signature);

    Ok(ConsensusProof::new(
        fact.prestate_hash,
        fact.operation_hash,
        threshold_signature,
        fact.participants,
        true, // threshold_met (always true for successful consensus)
    ))
}

/// Relational consensus configuration builder
pub struct RelationalConsensusBuilder {
    witnesses: Vec<AuthorityId>,
    threshold: Option<u16>,
    timeout_ms: Option<u64>,
    epoch: Epoch,
}

impl RelationalConsensusBuilder {
    /// Create a new builder with witnesses from a prestate
    pub fn from_prestate(prestate: &Prestate, epoch: Epoch) -> Self {
        let witnesses: Vec<_> = prestate
            .authority_commitments
            .iter()
            .map(|(id, _)| *id)
            .collect();

        Self {
            witnesses,
            threshold: None,
            timeout_ms: None,
            epoch,
        }
    }

    /// Set custom witnesses
    pub fn with_witnesses(mut self, witnesses: Vec<AuthorityId>) -> Self {
        self.witnesses = witnesses;
        self
    }

    /// Set custom threshold
    pub fn with_threshold(mut self, threshold: u16) -> Self {
        self.threshold = Some(threshold);
        self
    }

    /// Set custom timeout
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// Build the consensus configuration
    pub fn build(self) -> Result<ConsensusConfig> {
        let threshold = self.threshold.unwrap_or_else(|| {
            // Default to simple majority
            (self.witnesses.len() as u16).div_ceil(2).max(1)
        });

        let mut config = ConsensusConfig::new(threshold, self.witnesses, self.epoch)?;

        if let Some(timeout_ms) = self.timeout_ms {
            config.timeout_ms = timeout_ms;
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::Hash32;

    #[test]
    fn test_relational_builder() {
        let witnesses = vec![
            AuthorityId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
            AuthorityId::new_from_entropy([3u8; 32]),
        ];
        let epoch = Epoch::from(1);

        let prestate = Prestate::new(
            vec![(witnesses[0], Hash32::default())],
            Hash32::default(),
        )
        .unwrap();
        let config = RelationalConsensusBuilder::from_prestate(&prestate, epoch)
            .with_witnesses(witnesses.clone())
        .with_threshold(2)
        .with_timeout_ms(10000)
        .build()
        .unwrap();

        assert_eq!(config.witness_set, witnesses);
        assert_eq!(config.threshold, 2);
        assert_eq!(config.timeout_ms, 10000);
        assert_eq!(config.epoch, epoch);
    }
}
