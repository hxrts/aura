//! AMP epoch-transition simulator scenarios and reducer/model conformance.
#![allow(clippy::expect_used)]

use aura_amp::core::{
    receive_ratchet_from_epoch_state, send_ratchet_from_epoch_state, sender_allowed_by_epoch_state,
};
use aura_core::time::{OrderTime, TimeStamp};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::Hash32;
use aura_journal::fact::{
    AmpEmergencyAlarm, AmpTransitionAbort, AmpTransitionConflict, AmpTransitionIdentity,
    AmpTransitionPolicy, AmpTransitionSupersession, AmpTransitionSuppressionScope,
    AmpTransitionWitnessSignature, CertifiedChannelEpochBump, ChannelBumpReason,
    FinalizedChannelEpochBump, ProposedChannelEpochBump, ProtocolRelationalFact,
};
use aura_journal::reduction::{
    reduce_context, AmpTransitionParentKey, AmpTransitionReduction, AmpTransitionReductionStatus,
    PendingBump,
};
use aura_journal::{
    ChannelEpochState, Fact, FactContent, FactJournal, JournalNamespace, RelationalFact,
};
use std::collections::BTreeSet;

#[test]
fn normal_transition_scenario_tolerates_delayed_witnesses_before_one_a2_live() {
    let scenario = [
        ModelEvent::Proposal(1, AmpTransitionPolicy::NormalTransition),
        ModelEvent::Certificate(1, AmpTransitionPolicy::NormalTransition),
    ];

    let before_witness = reduce_events(&scenario[..1]);
    assert_eq!(
        scenario_status(&before_witness),
        AmpTransitionReductionStatus::Observed
    );

    let after_witness = reduce_events(&scenario);
    assert_eq!(
        scenario_status(&after_witness),
        AmpTransitionReductionStatus::A2Live
    );
    assert_eq!(
        oracle_status(&scenario),
        AmpTransitionReductionStatus::A2Live
    );
}

#[test]
fn partitioned_conflicting_a2_certificates_suppress_live_successor() {
    let scenario = [
        ModelEvent::Certificate(1, AmpTransitionPolicy::NormalTransition),
        ModelEvent::Certificate(2, AmpTransitionPolicy::NormalTransition),
        ModelEvent::Conflict(1, 2),
    ];
    let state = reduce_events(&scenario);
    let transition = state.amp_transitions.values().next().expect("transition");

    assert_eq!(transition.status, AmpTransitionReductionStatus::A2Conflict);
    assert!(transition.live_transition_id.is_none());
    assert!(state.channel_epochs.get(&channel()).is_none_or(|channel| {
        channel.pending_bump.is_none()
            || channel
                .transition
                .as_ref()
                .is_some_and(|transition| transition.live_transition_id.is_none())
    }));
    assert_eq!(
        oracle_status(&scenario),
        AmpTransitionReductionStatus::A2Conflict
    );
}

#[test]
fn subtractive_transition_cuts_old_epoch_receive_boundary() {
    let state = channel_state_for_policy(
        AmpTransitionPolicy::SubtractiveTransition,
        AmpTransitionReductionStatus::A2Live,
        None,
    );
    let receive = receive_ratchet_from_epoch_state(&state);
    let send = send_ratchet_from_epoch_state(&state);

    assert_eq!(send.chan_epoch, 4);
    assert_eq!(receive.chan_epoch, 4);
    assert_eq!(receive.pending_epoch, None);
    assert_eq!(receive.skip_window, 0);
}

#[test]
fn emergency_quarantine_immediately_cuts_send_and_excludes_suspect() {
    let suspect = AuthorityId::new_from_entropy([91u8; 32]);
    let state = channel_state_for_policy(
        AmpTransitionPolicy::EmergencyQuarantineTransition,
        AmpTransitionReductionStatus::A2Live,
        Some(suspect),
    );
    let send = send_ratchet_from_epoch_state(&state);
    let receive = receive_ratchet_from_epoch_state(&state);

    assert_eq!(send.chan_epoch, 4);
    assert_eq!(receive.chan_epoch, 4);
    assert_eq!(receive.pending_epoch, Some(3));
    assert_eq!(receive.skip_window, 1);
    assert!(!sender_allowed_by_epoch_state(&state, suspect));
}

#[test]
fn emergency_cryptoshred_destroys_readable_state_at_a2_live_boundary() {
    let suspect = AuthorityId::new_from_entropy([92u8; 32]);
    let mut cert = certified_bump(
        identity(1, AmpTransitionPolicy::EmergencyCryptoshredTransition),
        [10u8; 32],
    );
    cert.excluded_authorities.insert(suspect);
    cert.readable_state_destroyed = true;
    let state = reduce_facts([ProtocolRelationalFact::AmpCertifiedChannelEpochBump(cert)]);
    let transition = state.amp_transitions.values().next().expect("transition");

    assert_eq!(transition.status, AmpTransitionReductionStatus::A2Live);
    assert!(transition.emergency_suspects.contains(&suspect));
    assert!(transition.prune_before_epochs.contains(&0));
}

#[test]
fn model_based_transition_cases_match_rust_reducer() {
    let cases: &[(&str, &[ModelEvent], AmpTransitionReductionStatus)] = &[
        (
            "delayed witnesses",
            &[
                ModelEvent::Proposal(1, AmpTransitionPolicy::NormalTransition),
                ModelEvent::Certificate(1, AmpTransitionPolicy::NormalTransition),
            ],
            AmpTransitionReductionStatus::A2Live,
        ),
        (
            "duplicate signing replay",
            &[
                ModelEvent::Certificate(1, AmpTransitionPolicy::NormalTransition),
                ModelEvent::Certificate(1, AmpTransitionPolicy::NormalTransition),
            ],
            AmpTransitionReductionStatus::A2Live,
        ),
        (
            "partition conflict",
            &[
                ModelEvent::Certificate(1, AmpTransitionPolicy::NormalTransition),
                ModelEvent::Certificate(2, AmpTransitionPolicy::NormalTransition),
            ],
            AmpTransitionReductionStatus::A2Conflict,
        ),
        (
            "explicit conflict evidence",
            &[
                ModelEvent::Certificate(1, AmpTransitionPolicy::NormalTransition),
                ModelEvent::Certificate(2, AmpTransitionPolicy::NormalTransition),
                ModelEvent::Conflict(1, 2),
            ],
            AmpTransitionReductionStatus::A2Conflict,
        ),
        (
            "abort evidence",
            &[
                ModelEvent::Proposal(1, AmpTransitionPolicy::NormalTransition),
                ModelEvent::Abort(1),
            ],
            AmpTransitionReductionStatus::Aborted,
        ),
        (
            "supersession",
            &[
                ModelEvent::Proposal(1, AmpTransitionPolicy::NormalTransition),
                ModelEvent::Proposal(2, AmpTransitionPolicy::EmergencyQuarantineTransition),
                ModelEvent::Supersede(1, 2),
            ],
            AmpTransitionReductionStatus::Superseded,
        ),
        (
            "recovery replay finalization",
            &[
                ModelEvent::Certificate(1, AmpTransitionPolicy::NormalTransition),
                ModelEvent::Finalize(1, AmpTransitionPolicy::NormalTransition),
                ModelEvent::Finalize(1, AmpTransitionPolicy::NormalTransition),
            ],
            AmpTransitionReductionStatus::A3Finalized,
        ),
        (
            "emergency alarm spam",
            &[
                ModelEvent::Alarm(1),
                ModelEvent::Alarm(2),
                ModelEvent::Certificate(1, AmpTransitionPolicy::EmergencyQuarantineTransition),
            ],
            AmpTransitionReductionStatus::A2Live,
        ),
    ];

    for (name, events, expected) in cases {
        let state = reduce_events(events);
        assert_eq!(scenario_status(&state), *expected, "{name}");
        assert_eq!(oracle_status(events), *expected, "{name} oracle");
    }
}

#[derive(Debug, Clone, Copy)]
enum ModelEvent {
    Proposal(u8, AmpTransitionPolicy),
    Certificate(u8, AmpTransitionPolicy),
    Finalize(u8, AmpTransitionPolicy),
    Conflict(u8, u8),
    Abort(u8),
    Supersede(u8, u8),
    Alarm(u8),
}

fn reduce_events(events: &[ModelEvent]) -> aura_journal::reduction::RelationalState {
    let mut facts = Vec::new();
    for event in events {
        match *event {
            ModelEvent::Proposal(seed, policy) => {
                facts.push(ProtocolRelationalFact::AmpProposedChannelEpochBump(
                    proposed_bump(seed, policy),
                ));
            }
            ModelEvent::Certificate(seed, policy) => {
                facts.push(ProtocolRelationalFact::AmpCertifiedChannelEpochBump(
                    certified_bump(identity(seed, policy), [seed; 32]),
                ));
            }
            ModelEvent::Finalize(seed, policy) => {
                facts.push(ProtocolRelationalFact::AmpFinalizedChannelEpochBump(
                    finalized_bump(identity(seed, policy), [seed; 32]),
                ));
            }
            ModelEvent::Conflict(left, right) => {
                facts.push(ProtocolRelationalFact::AmpTransitionConflict(
                    transition_conflict(left, right),
                ));
            }
            ModelEvent::Abort(seed) => {
                facts.push(ProtocolRelationalFact::AmpTransitionAbort(
                    transition_abort(seed),
                ));
            }
            ModelEvent::Supersede(old, new) => {
                facts.push(ProtocolRelationalFact::AmpTransitionSupersession(
                    transition_supersession(old, new),
                ));
            }
            ModelEvent::Alarm(seed) => {
                facts.push(ProtocolRelationalFact::AmpEmergencyAlarm(emergency_alarm(
                    seed,
                )));
            }
        }
    }
    reduce_facts(facts)
}

fn reduce_facts(
    facts: impl IntoIterator<Item = ProtocolRelationalFact>,
) -> aura_journal::reduction::RelationalState {
    let mut journal = FactJournal::new(JournalNamespace::Context(context()));
    for (index, fact) in facts.into_iter().enumerate() {
        let order = (index + 1) as u8;
        journal
            .add_fact(Fact::new(
                OrderTime([order; 32]),
                TimeStamp::OrderClock(OrderTime([order; 32])),
                FactContent::Relational(RelationalFact::Protocol(fact)),
            ))
            .expect("fact insert");
    }
    reduce_context(&journal).expect("context reduction")
}

fn oracle_status(events: &[ModelEvent]) -> AmpTransitionReductionStatus {
    let mut proposals = BTreeSet::new();
    let mut certs = BTreeSet::new();
    let mut finals = BTreeSet::new();
    let mut suppressed = BTreeSet::new();
    let mut conflict = false;

    for event in events {
        match *event {
            ModelEvent::Proposal(seed, _) => {
                proposals.insert(seed);
            }
            ModelEvent::Certificate(seed, _) => {
                certs.insert(seed);
            }
            ModelEvent::Finalize(seed, _) => {
                finals.insert(seed);
            }
            ModelEvent::Conflict(_, _) => conflict = true,
            ModelEvent::Abort(seed) => {
                suppressed.insert(seed);
            }
            ModelEvent::Supersede(seed, _) => {
                suppressed.insert(seed);
            }
            ModelEvent::Alarm(_) => {}
        }
    }

    let unsuppressed_certs = certs.difference(&suppressed).count();
    let unsuppressed_finals = finals.difference(&suppressed).count();

    if !suppressed.is_empty() && certs.is_empty() && finals.is_empty() {
        if events
            .iter()
            .any(|event| matches!(event, ModelEvent::Supersede(_, _)))
        {
            AmpTransitionReductionStatus::Superseded
        } else {
            AmpTransitionReductionStatus::Aborted
        }
    } else if conflict || unsuppressed_certs > 1 {
        AmpTransitionReductionStatus::A2Conflict
    } else if unsuppressed_finals > 1 {
        AmpTransitionReductionStatus::A3Conflict
    } else if unsuppressed_finals == 1 {
        AmpTransitionReductionStatus::A3Finalized
    } else if unsuppressed_certs == 1 {
        AmpTransitionReductionStatus::A2Live
    } else {
        let _ = proposals;
        AmpTransitionReductionStatus::Observed
    }
}

fn scenario_status(
    state: &aura_journal::reduction::RelationalState,
) -> AmpTransitionReductionStatus {
    state
        .amp_transitions
        .values()
        .next()
        .expect("transition")
        .status
}

fn context() -> ContextId {
    ContextId::new_from_entropy([71u8; 32])
}

fn channel() -> ChannelId {
    ChannelId::from_bytes([72u8; 32])
}

fn parent_commitment() -> Hash32 {
    Hash32::new([73u8; 32])
}

fn identity(seed: u8, transition_policy: AmpTransitionPolicy) -> AmpTransitionIdentity {
    AmpTransitionIdentity {
        context: context(),
        channel: channel(),
        parent_epoch: 0,
        parent_commitment: parent_commitment(),
        successor_epoch: 1,
        successor_commitment: Hash32::new([seed; 32]),
        membership_commitment: Hash32::new([74u8; 32]),
        transition_policy,
    }
}

fn proposed_bump(seed: u8, transition_policy: AmpTransitionPolicy) -> ProposedChannelEpochBump {
    let identity = identity(seed, transition_policy);
    ProposedChannelEpochBump {
        context: context(),
        channel: channel(),
        parent_epoch: identity.parent_epoch,
        new_epoch: identity.successor_epoch,
        bump_id: identity.successor_commitment,
        reason: ChannelBumpReason::Routine,
        parent_commitment: identity.parent_commitment,
        successor_commitment: identity.successor_commitment,
        membership_commitment: identity.membership_commitment,
        transition_policy: identity.transition_policy,
        transition_id: identity.transition_id(),
    }
}

fn certified_bump(
    identity: AmpTransitionIdentity,
    payload_seed: [u8; 32],
) -> CertifiedChannelEpochBump {
    CertifiedChannelEpochBump {
        transition_id: identity.transition_id(),
        identity,
        witness_payload_digest: Hash32::new(payload_seed),
        committee_digest: Hash32::new([75u8; 32]),
        threshold: 2,
        fault_bound: 1,
        coord_epoch: None,
        generation_min: None,
        generation_max: None,
        witness_signatures: vec![
            AmpTransitionWitnessSignature {
                witness: AuthorityId::new_from_entropy([76u8; 32]),
                signature: vec![1],
            },
            AmpTransitionWitnessSignature {
                witness: AuthorityId::new_from_entropy([77u8; 32]),
                signature: vec![2],
            },
        ],
        equivocation_refs: BTreeSet::new(),
        excluded_authorities: BTreeSet::new(),
        readable_state_destroyed: false,
    }
}

fn finalized_bump(
    identity: AmpTransitionIdentity,
    consensus_seed: [u8; 32],
) -> FinalizedChannelEpochBump {
    FinalizedChannelEpochBump {
        transition_id: identity.transition_id(),
        identity,
        consensus_id: Hash32::new(consensus_seed),
        transcript_ref: None,
        excluded_authorities: BTreeSet::new(),
        readable_state_destroyed: false,
    }
}

fn transition_conflict(left: u8, right: u8) -> AmpTransitionConflict {
    AmpTransitionConflict {
        context: context(),
        channel: channel(),
        parent_epoch: 0,
        parent_commitment: parent_commitment(),
        first_transition_id: identity(left, AmpTransitionPolicy::NormalTransition).transition_id(),
        second_transition_id: identity(right, AmpTransitionPolicy::NormalTransition)
            .transition_id(),
        equivocating_witness: Some(AuthorityId::new_from_entropy([78u8; 32])),
        evidence_id: Hash32::new([79u8; 32]),
    }
}

fn transition_abort(seed: u8) -> AmpTransitionAbort {
    AmpTransitionAbort {
        context: context(),
        channel: channel(),
        parent_epoch: 0,
        parent_commitment: parent_commitment(),
        transition_id: identity(seed, AmpTransitionPolicy::NormalTransition).transition_id(),
        evidence_id: Hash32::new([80u8; 32]),
        scope: AmpTransitionSuppressionScope::A2AndA3,
    }
}

fn transition_supersession(old: u8, new: u8) -> AmpTransitionSupersession {
    AmpTransitionSupersession {
        context: context(),
        channel: channel(),
        parent_epoch: 0,
        parent_commitment: parent_commitment(),
        superseded_transition_id: identity(old, AmpTransitionPolicy::NormalTransition)
            .transition_id(),
        superseding_transition_id: identity(
            new,
            AmpTransitionPolicy::EmergencyQuarantineTransition,
        )
        .transition_id(),
        evidence_id: Hash32::new([81u8; 32]),
        scope: AmpTransitionSuppressionScope::A2AndA3,
    }
}

fn emergency_alarm(seed: u8) -> AmpEmergencyAlarm {
    AmpEmergencyAlarm {
        context: context(),
        channel: channel(),
        parent_epoch: 0,
        parent_commitment: parent_commitment(),
        suspect: AuthorityId::new_from_entropy([seed; 32]),
        raised_by: AuthorityId::new_from_entropy([82u8; 32]),
        evidence_id: Hash32::new([seed; 32]),
    }
}

fn channel_state_for_policy(
    transition_policy: AmpTransitionPolicy,
    status: AmpTransitionReductionStatus,
    suspect: Option<AuthorityId>,
) -> ChannelEpochState {
    let transition_id = identity(1, transition_policy).transition_id();
    let mut emergency_suspects = BTreeSet::new();
    if let Some(suspect) = suspect {
        emergency_suspects.insert(suspect);
    }

    ChannelEpochState {
        chan_epoch: 3,
        current_gen: 10,
        last_checkpoint_gen: 8,
        skip_window: 64,
        pending_bump: Some(PendingBump {
            parent_epoch: 3,
            new_epoch: 4,
            bump_id: Hash32::new([83u8; 32]),
            reason: ChannelBumpReason::Routine,
            transition_id,
            transition_policy,
        }),
        bootstrap: None,
        transition: Some(AmpTransitionReduction {
            parent: AmpTransitionParentKey {
                context: context(),
                channel: channel(),
                parent_epoch: 3,
                parent_commitment: parent_commitment(),
            },
            status,
            observed_transition_ids: BTreeSet::new(),
            certified_transition_ids: BTreeSet::from([transition_id]),
            finalized_transition_ids: BTreeSet::new(),
            live_transition_id: (status == AmpTransitionReductionStatus::A2Live)
                .then_some(transition_id),
            finalized_transition_id: None,
            suppressed_transition_ids: BTreeSet::new(),
            conflict_evidence_ids: BTreeSet::new(),
            emergency_alarm_ids: BTreeSet::new(),
            emergency_suspects,
            quarantine_epochs: BTreeSet::new(),
            prune_before_epochs: BTreeSet::new(),
        }),
    }
}
