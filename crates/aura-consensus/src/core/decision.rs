//! Allocation-free consensus decisions shared by production storage and proofs.
//!
//! Iterators expose identity only: share payloads are preserved by the storage
//! adapter and never participate in these decisions. No bounded proof model
//! substitutes for this production algorithm.

use super::state::ConsensusPhase;

pub(super) fn has_proposal<W: Copy + Eq, R, I>(proposals: I, witness: W) -> bool
where
    I: Iterator<Item = (W, R)>,
{
    proposals
        .into_iter()
        .any(|(candidate, _)| candidate == witness)
}

pub(super) fn count_result<W, R: Copy + Eq, I>(proposals: I, result: R) -> usize
where
    I: Iterator<Item = (W, R)>,
{
    proposals
        .filter(|(_, candidate)| *candidate == result)
        .count()
}

/// Select a maximal-count qualifying result. Equal counts retain the first
/// proposal's result. Neither wall time nor map iteration orders consensus.
pub(super) fn majority_result<W, R: Copy + Eq, I>(proposals: I, threshold: usize) -> Option<R>
where
    I: Clone + Iterator<Item = (W, R)>,
{
    let mut best = None;
    let mut best_count = 0;
    for (_, result) in proposals.clone() {
        let count = count_result(proposals.clone(), result);
        if count >= threshold && count > best_count {
            best = Some(result);
            best_count = count;
        }
    }
    best
}

pub(super) fn threshold_met<W, R: Copy + Eq, I>(proposals: I, threshold: usize) -> bool
where
    I: Clone + Iterator<Item = (W, R)>,
{
    majority_result(proposals, threshold).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShareRejection {
    NonWitness,
    AlreadyVoted,
    Inactive,
    Equivocator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ShareUpdate<R> {
    pub phase: ConsensusPhase,
    pub commit_result: Option<R>,
}

/// Decide the complete logical share transition before allocating new storage.
/// A successful decision appends the original proposal unchanged. The duplicate
/// guard makes the old post-guard equivocation branch unreachable.
pub(super) fn apply_share<W: Copy + Eq, R: Copy + Eq, I>(
    phase: ConsensusPhase,
    threshold: usize,
    proposals: I,
    witness_is_member: bool,
    witness_is_equivocator: bool,
    proposal: (W, R),
) -> Result<ShareUpdate<R>, ShareRejection>
where
    I: Clone + Iterator<Item = (W, R)>,
{
    if !witness_is_member {
        return Err(ShareRejection::NonWitness);
    }
    if has_proposal(proposals.clone(), proposal.0) {
        return Err(ShareRejection::AlreadyVoted);
    }
    if !matches!(
        phase,
        ConsensusPhase::FastPathActive | ConsensusPhase::FallbackActive
    ) {
        return Err(ShareRejection::Inactive);
    }
    if witness_is_equivocator {
        return Err(ShareRejection::Equivocator);
    }
    let commit_result = majority_result(proposals.chain(std::iter::once(proposal)), threshold);
    Ok(ShareUpdate {
        phase: if commit_result.is_some() {
            ConsensusPhase::Committed
        } else {
            phase
        },
        commit_result,
    })
}

pub(super) fn trigger_fallback(phase: ConsensusPhase) -> Option<ConsensusPhase> {
    (phase == ConsensusPhase::FastPathActive).then_some(ConsensusPhase::FallbackActive)
}

pub(super) fn fail_consensus(phase: ConsensusPhase) -> Option<ConsensusPhase> {
    (!matches!(phase, ConsensusPhase::Committed | ConsensusPhase::Failed))
        .then_some(ConsensusPhase::Failed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InvariantViolation<W> {
    ZeroThreshold,
    InsufficientWitnesses,
    NonWitnessProposal(W),
    NonWitnessEquivocator(W),
    MissingCommit,
}

pub(super) fn invariant_violation<W: Copy + Eq, R, P, S, E>(
    phase: ConsensusPhase,
    threshold: usize,
    proposals: P,
    witnesses: S,
    equivocators: E,
    has_commit: bool,
) -> Option<InvariantViolation<W>>
where
    P: Iterator<Item = (W, R)>,
    S: Clone + Iterator<Item = W>,
    E: Iterator<Item = W>,
{
    if threshold == 0 {
        return Some(InvariantViolation::ZeroThreshold);
    }
    if witnesses.clone().count() < threshold {
        return Some(InvariantViolation::InsufficientWitnesses);
    }
    for (witness, _) in proposals {
        if !witnesses.clone().any(|member| member == witness) {
            return Some(InvariantViolation::NonWitnessProposal(witness));
        }
    }
    for witness in equivocators {
        if !witnesses.clone().any(|member| member == witness) {
            return Some(InvariantViolation::NonWitnessEquivocator(witness));
        }
    }
    if phase == ConsensusPhase::Committed && !has_commit {
        return Some(InvariantViolation::MissingCommit);
    }
    None
}

pub(super) fn equivocators_excluded<W: Copy + Eq, R, P, E>(proposals: P, equivocators: E) -> bool
where
    P: Clone + Iterator<Item = (W, R)>,
    E: Iterator<Item = W>,
{
    equivocators
        .into_iter()
        .all(|witness| !has_proposal(proposals.clone(), witness))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximal_result_retains_first_encounter_on_equal_counts() {
        let proposals = [(1, 3), (2, 2), (3, 2), (4, 3)];
        assert_eq!(majority_result(proposals.into_iter(), 2), Some(3));
        assert_eq!(majority_result(proposals.into_iter().rev(), 2), Some(3));
        let reversed_groups = [(1, 2), (2, 3), (3, 3), (4, 2)];
        assert_eq!(majority_result(reversed_groups.into_iter(), 2), Some(2));
        assert_eq!(majority_result(proposals.into_iter(), 3), None);
    }

    #[test]
    fn production_wrapper_preserves_incoming_payload_and_selected_result() {
        use super::super::{
            state::{ConsensusState, ConsensusThreshold, PathSelection, ShareData, ShareProposal},
            transitions::{apply_share, TransitionResult},
        };
        use crate::types::ConsensusId;
        use aura_core::{AuthorityId, Hash32, OperationId};
        let witness = AuthorityId::new_from_entropy([1; 32]);
        let state = ConsensusState::new(
            ConsensusId(Hash32::new([2; 32])),
            OperationId::new_from_entropy([3; 32]),
            Hash32::new([4; 32]),
            ConsensusThreshold::new(1).unwrap(),
            [witness, AuthorityId::new_from_entropy([6; 32])]
                .into_iter()
                .collect(),
            witness,
            PathSelection::FastPath,
        );
        let proposal = ShareProposal {
            witness,
            result_id: Hash32::new([5; 32]),
            share: ShareData {
                share_value: "distinct-share".into(),
                nonce_binding: "distinct-nonce".into(),
                data_binding: "distinct-binding".into(),
            },
        };
        let TransitionResult::Ok(next) = apply_share(&state, proposal.clone()) else {
            panic!("valid share admitted")
        };
        assert_eq!(
            next.proposals[0].share.share_value,
            proposal.share.share_value
        );
        assert_eq!(
            next.proposals[0].share.nonce_binding,
            proposal.share.nonce_binding
        );
        assert_eq!(
            next.proposals[0].share.data_binding,
            proposal.share.data_binding
        );
        let commit = next.commit_fact.as_ref().unwrap();
        assert_eq!(commit.result_id, proposal.result_id);
        assert_eq!(commit.cid, state.cid);
        assert_eq!(commit.prestate_hash, state.prestate_hash);
        assert_eq!(
            commit.signature,
            format!(
                "pure-consensus:{}",
                hex::encode(aura_core::hash::hash(
                    &[
                        state.cid.0 .0.as_slice(),
                        commit.result_id.0.as_slice(),
                        state.prestate_hash.0.as_slice()
                    ]
                    .concat()
                ))
            )
        );
        let retained = commit.clone();
        let mut later = proposal;
        later.witness = AuthorityId::new_from_entropy([6; 32]);
        assert!(matches!(
            apply_share(&next, later),
            TransitionResult::NotEnabled(_)
        ));
        assert!(matches!(
            super::super::transitions::trigger_fallback(&next),
            TransitionResult::NotEnabled(_)
        ));
        assert!(matches!(
            super::super::transitions::fail_consensus(&next),
            TransitionResult::NotEnabled(_)
        ));
        let actual = next.commit_fact.as_ref().unwrap();
        assert_eq!(actual.cid, retained.cid);
        assert_eq!(actual.result_id, retained.result_id);
        assert_eq!(actual.prestate_hash, retained.prestate_hash);
        assert_eq!(actual.signature, retained.signature);
    }
    #[test]
    fn exhaustive_native_storage_refines_independent_decisions() {
        use super::super::{
            state::{
                ConsensusState, ConsensusThreshold, PathSelection, PureCommitFact, ShareData,
                ShareProposal,
            },
            transitions::{self, TransitionResult},
            validation::check_all_invariants,
        };
        use crate::types::ConsensusId;
        use aura_core::{AuthorityId, Hash32, OperationId};
        use std::collections::BTreeMap;
        let authority = |n: u8| AuthorityId::new_from_entropy([n; 32]);
        let result = |n: u8| Hash32::new([n; 32]);
        let proposal = |w: u8, r: u8| ShareProposal {
            witness: authority(w),
            result_id: result(r),
            share: ShareData {
                share_value: format!("share-{w}-{r}"),
                nonce_binding: format!("nonce-{w}"),
                data_binding: format!("binding-{r}"),
            },
        };
        let phases = [
            ConsensusPhase::Pending,
            ConsensusPhase::FastPathActive,
            ConsensusPhase::FallbackActive,
            ConsensusPhase::Committed,
            ConsensusPhase::Failed,
        ];
        let mut examined = 0usize;
        for witnesses in 2u8..=4 {
            for threshold in 1u16..=u16::from(witnesses) {
                for encoding in 0u32..4u32.pow(u32::from(witnesses)) {
                    let mut entries = Vec::new();
                    let mut encoded = encoding;
                    for witness in 1..=witnesses {
                        let r = (encoded % 4) as u8;
                        encoded /= 4;
                        if r != 0 {
                            entries.push(proposal(witness, r));
                        }
                    }
                    if entries.len() > 3 {
                        continue;
                    }
                    let mut counts = BTreeMap::new();
                    for p in &entries {
                        *counts.entry(p.result_id).or_insert(0usize) += 1;
                    }
                    let maximum = counts.values().copied().max().unwrap_or(0);
                    let qualified: Vec<_> = counts
                        .iter()
                        .filter(|(_, count)| {
                            **count >= usize::from(threshold) && **count == maximum
                        })
                        .map(|(r, _)| *r)
                        .collect();
                    for mask in 0u8..(1 << witnesses) {
                        if (1..=witnesses).any(|w| {
                            mask & (1 << (w - 1)) != 0
                                && entries.iter().any(|p| p.witness == authority(w))
                        }) {
                            continue;
                        }
                        for phase in phases {
                            if phase == ConsensusPhase::Committed && qualified.is_empty() {
                                continue;
                            }
                            let mut state = ConsensusState::new(
                                ConsensusId(result(21 + (encoding & 1) as u8)),
                                OperationId::new_from_entropy(
                                    [31 + ((encoding >> 1) & 1) as u8; 32],
                                ),
                                result(41 + ((encoding >> 2) & 1) as u8),
                                ConsensusThreshold::new(threshold).unwrap(),
                                (1..=witnesses).map(authority).collect(),
                                authority(1),
                                PathSelection::FastPath,
                            );
                            state.proposals = entries.clone();
                            state.phase = phase;
                            state.equivocators = (1..=witnesses)
                                .filter(|w| mask & (1 << (w - 1)) != 0)
                                .map(authority)
                                .collect();
                            state.fallback_timer_active = phase == ConsensusPhase::FallbackActive;
                            if phase == ConsensusPhase::Committed {
                                let winner = qualified[0];
                                state.commit_fact = Some(PureCommitFact {
                                    cid: state.cid,
                                    result_id: winner,
                                    prestate_hash: state.prestate_hash,
                                    signature: transitions::abstract_commit_signature(
                                        state.cid,
                                        winner,
                                        state.prestate_hash,
                                    ),
                                });
                            }
                            assert!(check_all_invariants(&state));
                            assert_eq!(state.threshold_met(), !qualified.is_empty());
                            let selected = state.majority_result();
                            assert_eq!(selected.is_some(), !qualified.is_empty());
                            if let Some(r) = selected {
                                assert!(qualified.contains(&r));
                                if qualified.len() == 1 {
                                    assert_eq!(r, qualified[0]);
                                }
                            }
                            for incoming_w in 1..=witnesses + 1 {
                                for incoming_r in 1..=3 {
                                    examined += 1;
                                    let incoming = proposal(incoming_w, incoming_r);
                                    let enabled = incoming_w <= witnesses
                                        && !entries.iter().any(|p| p.witness == incoming.witness)
                                        && matches!(
                                            phase,
                                            ConsensusPhase::FastPathActive
                                                | ConsensusPhase::FallbackActive
                                        )
                                        && mask & (1 << (incoming_w - 1)) == 0;
                                    let actual = transitions::apply_share(&state, incoming.clone());
                                    assert_eq!(actual.is_ok(), enabled);
                                    if let TransitionResult::Ok(next) = actual {
                                        let mut expected = entries.clone();
                                        expected.push(incoming);
                                        assert_eq!(next.cid, state.cid);
                                        assert_eq!(next.operation, state.operation);
                                        assert_eq!(next.prestate_hash, state.prestate_hash);
                                        assert_eq!(next.proposals.len(), expected.len());
                                        for (p, e) in next.proposals.iter().zip(&expected) {
                                            assert_eq!(p.witness, e.witness);
                                            assert_eq!(p.result_id, e.result_id);
                                            assert_eq!(p.share.share_value, e.share.share_value);
                                            assert_eq!(
                                                p.share.nonce_binding,
                                                e.share.nonce_binding
                                            );
                                            assert_eq!(p.share.data_binding, e.share.data_binding);
                                        }
                                        let mut reference = BTreeMap::new();
                                        for p in &expected {
                                            *reference.entry(p.result_id).or_insert(0usize) += 1;
                                        }
                                        let max = reference.values().copied().max().unwrap();
                                        let winners: Vec<_> = reference
                                            .iter()
                                            .filter(|(_, n)| {
                                                **n >= usize::from(threshold) && **n == max
                                            })
                                            .map(|(r, _)| *r)
                                            .collect();
                                        assert_eq!(
                                            next.phase,
                                            if winners.is_empty() {
                                                phase
                                            } else {
                                                ConsensusPhase::Committed
                                            }
                                        );
                                        if let Some(commit) = &next.commit_fact {
                                            assert!(winners.contains(&commit.result_id));
                                            assert_eq!(commit.cid, state.cid);
                                            assert_eq!(commit.prestate_hash, state.prestate_hash);
                                            assert_eq!(
                                                commit.signature,
                                                format!(
                                                    "pure-consensus:{}",
                                                    hex::encode(aura_core::hash::hash(
                                                        &[
                                                            state.cid.0 .0.as_slice(),
                                                            commit.result_id.0.as_slice(),
                                                            state.prestate_hash.0.as_slice()
                                                        ]
                                                        .concat()
                                                    ))
                                                )
                                            );
                                        }
                                        assert!(check_all_invariants(&next));
                                    }
                                }
                            }
                            assert_eq!(
                                transitions::trigger_fallback(&state).is_ok(),
                                phase == ConsensusPhase::FastPathActive
                            );
                            assert_eq!(
                                transitions::fail_consensus(&state).is_ok(),
                                !matches!(
                                    phase,
                                    ConsensusPhase::Committed | ConsensusPhase::Failed
                                )
                            );
                            if let Some(commit) = &state.commit_fact {
                                assert_eq!(
                                    commit.signature,
                                    transitions::abstract_commit_signature(
                                        state.cid,
                                        commit.result_id,
                                        state.prestate_hash
                                    )
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(
            examined > 10_000,
            "exhaustive fixture must traverse actual bounded storage states"
        );
    }
}
