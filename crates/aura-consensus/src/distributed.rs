//! Consensus whose witnesses are separate runtimes.
//!
//! The in-process orchestrator (`frost.rs`) holds every witness share. Here
//! each witness keeps its own share: the coordinator distributes a
//! [`ConsensusRound`], collects each witness's nonce commitment and signature
//! share over its transport, and assembles the [`CommitFact`]. The
//! cryptography is the same: witnesses sign
//! [`consensus_commit_transcript_bytes`] with FROST, and the commit carries
//! the aggregate signature verified against the group key.

use crate::types::{consensus_commit_transcript_bytes, CommitFact, ConsensusId};
use aura_core::crypto::tree_signing::{
    frost_aggregate, FrostNonces, NonceCommitment, PartialSignature, PublicKeyPackage,
    RetiredFrostNonces, Share,
};
use aura_core::frost::ThresholdSignature;
use aura_core::time::ProvenancedTime;
use aura_core::{AuraError, AuthorityId, Hash32, Prestate, Result};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One consensus instance as every witness sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsensusRound {
    pub consensus_id: ConsensusId,
    pub prestate_hash: Hash32,
    pub operation_hash: Hash32,
    pub operation_bytes: Vec<u8>,
    pub threshold: u16,
}

impl ConsensusRound {
    /// The round agreeing on `operation` over `prestate`; `nonce` makes the
    /// instance unique.
    pub fn new<T: Serialize>(
        prestate: &Prestate,
        operation: &T,
        nonce: u64,
        threshold: u16,
    ) -> Result<Self> {
        let operation_bytes =
            serde_json::to_vec(operation).map_err(|e| AuraError::serialization(e.to_string()))?;
        let prestate_hash = prestate.compute_hash();
        let operation_hash = crate::hash_operation(&operation_bytes)?;
        Ok(Self {
            consensus_id: ConsensusId::new(prestate_hash, operation_hash, nonce),
            prestate_hash,
            operation_hash,
            operation_bytes,
            threshold,
        })
    }

    /// Whether this round's identity and hashes match its operation bytes.
    pub fn is_well_formed(&self) -> bool {
        crate::hash_operation(&self.operation_bytes).is_ok_and(|hash| hash == self.operation_hash)
            && self.threshold > 0
    }

    /// The bytes each witness signs.
    pub fn transcript(&self) -> Result<Vec<u8>> {
        consensus_commit_transcript_bytes(
            self.consensus_id,
            self.prestate_hash,
            self.operation_hash,
            &self.operation_bytes,
            self.threshold,
        )
    }
}

/// A witness's single-use nonces for a round; only their public
/// commitment leaves the witness.
pub async fn witness_commit(
    share: &Share,
    random: &(impl aura_core::effects::RandomEffects + ?Sized),
) -> Result<FrostNonces> {
    crate::frost::witness_nonce(share, random).await
}

/// A witness's signature share over `round`, given every participating
/// witness's commitment. The retired nonces are consumed.
pub fn witness_sign(
    round: &ConsensusRound,
    share: &Share,
    nonces: RetiredFrostNonces,
    commitments: &[NonceCommitment],
    group_public_key: &PublicKeyPackage,
) -> Result<PartialSignature> {
    let frost_group: frost_ed25519::keys::PublicKeyPackage = group_public_key
        .clone()
        .try_into()
        .map_err(|e: AuraError| AuraError::crypto(format!("Invalid group public key: {e}")))?;
    let identifier = frost_ed25519::Identifier::try_from(share.identifier)
        .map_err(|e| AuraError::crypto(format!("Invalid signer identifier: {e}")))?;
    let verifying_share = *frost_group
        .verifying_shares()
        .get(&identifier)
        .ok_or_else(|| AuraError::crypto("no verifying share for this witness"))?;
    let key_package = frost_ed25519::keys::KeyPackage::new(
        identifier,
        share
            .to_frost()
            .map_err(|e| AuraError::crypto(format!("Invalid signing share: {e}")))?,
        verifying_share,
        *frost_group.verifying_key(),
        round.threshold,
    );
    let mut frost_commitments = BTreeMap::new();
    for commitment in commitments {
        frost_commitments.insert(
            commitment.frost_identifier()?,
            commitment
                .to_frost()
                .map_err(|e| AuraError::crypto(format!("Invalid commitment: {e}")))?,
        );
    }
    let package = frost_ed25519::SigningPackage::new(frost_commitments, &round.transcript()?);
    let signature = nonces.sign(&package, &key_package)?;
    Ok(PartialSignature::from_frost(identifier, signature))
}

/// Aggregate the witnesses' shares into a verified commit. `participants`
/// are the authorities whose shares are included.
pub fn assemble_commit_fact(
    round: &ConsensusRound,
    commitments: &[NonceCommitment],
    partials: &[PartialSignature],
    group_public_key: &PublicKeyPackage,
    participants: Vec<AuthorityId>,
    timestamp: ProvenancedTime,
) -> Result<CommitFact> {
    if partials.len() < usize::from(round.threshold) || participants.len() != partials.len() {
        return Err(AuraError::invalid(
            "insufficient consensus signature shares",
        ));
    }
    let frost_group: frost_ed25519::keys::PublicKeyPackage = group_public_key
        .clone()
        .try_into()
        .map_err(|e: AuraError| AuraError::crypto(format!("Invalid group public key: {e}")))?;
    let commitments: BTreeMap<u16, NonceCommitment> = commitments
        .iter()
        .map(|commitment| (commitment.signer, commitment.clone()))
        .collect();
    let transcript = round.transcript()?;
    let signature = frost_aggregate(partials, &transcript, &commitments, &frost_group)
        .map_err(|e| AuraError::crypto(format!("FROST aggregation failed: {e}")))?;
    let commit = CommitFact::new(
        round.consensus_id,
        round.prestate_hash,
        round.operation_hash,
        round.operation_bytes.clone(),
        ThresholdSignature {
            signature,
            signers: partials.iter().map(|partial| partial.signer).collect(),
        },
        Some(group_public_key.clone()),
        participants,
        round.threshold,
        false,
        timestamp,
    );
    commit.verify()?;
    Ok(commit)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn dealt(n: u16, t: u16) -> (Vec<Share>, PublicKeyPackage) {
        let mut rng = rand::rngs::StdRng::from_seed([5; 32]);
        let (shares, public) = frost_ed25519::keys::generate_with_dealer(
            n,
            t,
            frost_ed25519::keys::IdentifierList::Default,
            &mut rng,
        )
        .unwrap();
        let shares = shares
            .into_iter()
            .map(|(id, secret)| {
                let key = frost_ed25519::keys::KeyPackage::try_from(secret).unwrap();
                Share::from_frost(id, *key.signing_share())
            })
            .collect();
        (shares, public.into())
    }

    fn timestamp() -> ProvenancedTime {
        ProvenancedTime {
            stamp: aura_core::time::TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
                ts_ms: 1,
                uncertainty: None,
            }),
            proofs: vec![],
            origin: None,
        }
    }

    /// Witnesses holding separate shares produce a commit that verifies
    /// against the group key; a share over a different round does not
    /// aggregate.
    /// Test entropy: a counter, so each call yields fresh bytes.
    struct CounterRandom(std::sync::atomic::AtomicU8);

    #[async_trait::async_trait]
    impl aura_core::effects::RandomCoreEffects for CounterRandom {
        async fn random_bytes(&self, len: usize) -> Vec<u8> {
            vec![self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed); len]
        }
        async fn random_bytes_32(&self) -> [u8; 32] {
            [self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed); 32]
        }
        async fn random_u64(&self) -> u64 {
            u64::from(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
        }
    }

    async fn commit_all(
        witnesses: &[Share],
        random: &CounterRandom,
    ) -> (Vec<NonceCommitment>, Vec<RetiredFrostNonces>) {
        let mut commitments = Vec::new();
        let mut tokens = Vec::new();
        for share in witnesses {
            let (commitment, token) = commit_one(share, random).await;
            commitments.push(commitment);
            tokens.push(token);
        }
        (commitments, tokens)
    }

    async fn commit_one(
        share: &Share,
        random: &CounterRandom,
    ) -> (NonceCommitment, RetiredFrostNonces) {
        let nonces = witness_commit(share, random).await.unwrap();
        let commitment = nonces.commitment().clone();
        let log = aura_core::crypto::tree_signing::ProcessFrostNonceRetirement::default();
        (commitment, nonces.retire(&log).await.unwrap())
    }

    #[tokio::test]
    async fn separately_held_shares_assemble_a_verified_commit() {
        let random = CounterRandom(std::sync::atomic::AtomicU8::new(1));
        let (shares, group) = dealt(3, 2);
        let prestate = Prestate::new(
            vec![(AuthorityId::new_from_entropy([3; 32]), Hash32([4; 32]))],
            Hash32::default(),
        )
        .unwrap();
        let round = ConsensusRound::new(&prestate, &"bump", 7, 2).unwrap();
        assert!(round.is_well_formed());
        let witnesses = &shares[..2];
        let (commitments, tokens) = commit_all(witnesses, &random).await;
        let partials: Vec<_> = witnesses
            .iter()
            .zip(tokens)
            .map(|(share, token)| witness_sign(&round, share, token, &commitments, &group).unwrap())
            .collect();
        let authorities = vec![
            AuthorityId::new_from_entropy([1; 32]),
            AuthorityId::new_from_entropy([2; 32]),
        ];
        let commit = assemble_commit_fact(
            &round,
            &commitments,
            &partials,
            &group,
            authorities.clone(),
            timestamp(),
        )
        .unwrap();
        assert_eq!(commit.consensus_id, round.consensus_id);

        let other = ConsensusRound::new(&prestate, &"other", 7, 2).unwrap();
        let (commitments, tokens) = commit_all(witnesses, &random).await;
        let mut mixed: Vec<_> = witnesses
            .iter()
            .zip(tokens)
            .map(|(share, token)| witness_sign(&round, share, token, &commitments, &group).unwrap())
            .collect();
        let (c2, t2) = commit_one(&witnesses[1], &random).await;
        mixed[1] = witness_sign(
            &other,
            &witnesses[1],
            t2,
            &[commitments[0].clone(), c2],
            &group,
        )
        .unwrap();
        assert!(
            assemble_commit_fact(
                &round,
                &commitments,
                &mixed,
                &group,
                authorities,
                timestamp()
            )
            .is_err(),
            "a share over another round does not aggregate"
        );
    }
}
