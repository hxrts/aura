//! Aura Consensus among a channel's members (work/8.md Task 164 step 2).
//!
//! A channel epoch bump for a membership change is agreed by the members of
//! the new epoch: the witnesses are the epoch's key roster, each signing with
//! its own share of the epoch's channel key (from the channel key ceremony).
//! The coordinator sends each witness the [`ConsensusRound`] (prestate and
//! the proposed bump); a witness admits it only from a coordinator with
//! standing, for a bump of the scoped channel to exactly the epoch whose key
//! it holds, and answers with a nonce commitment and then a signature share.
//! The coordinator assembles a [`CommitFact`] whose aggregate signature
//! verifies against the epoch's group key, so the commit also attests that
//! the key ceremony produced that key (the DKG transcript is finalized by the
//! same consensus). No witness share leaves its runtime.

use super::channel_key_ceremony::verified_device_key;
use super::context_dkg::{load_context_key_package, load_roster, ChannelKeyScope};
use super::AuraEffectSystem;
use aura_consensus::distributed::{
    assemble_commit_fact, witness_commit, witness_sign, ConsensusRound,
};
use aura_consensus::CommitFact;
use aura_core::crypto::tree_signing::{
    FrostNonces, NonceCommitment, PartialSignature, ProcessFrostNonceRetirement, PublicKeyPackage,
    Share,
};
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{PhysicalTimeEffects, RandomCoreEffects};
use aura_core::types::identifiers::AuthorityId;
use aura_core::{AuraError, Hash32, Prestate};
use aura_journal::fact::ProposedChannelEpochBump;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Content type of channel consensus messages.
pub(crate) const CHANNEL_CONSENSUS_CONTENT_TYPE: &str = "application/aura-channel-consensus";
const RESPONSE_POLL_MS: u64 = 50;
const MAX_MESSAGE_BYTES: usize = 262_144;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum ChannelConsensusMessage {
    /// Coordinator to witness: agree on `round` with the `key_epoch` key.
    Execute {
        scope: ChannelKeyScope,
        key_epoch: u64,
        round: ConsensusRound,
    },
    /// Witness to coordinator: its nonce commitment.
    NonceCommit {
        consensus_id: aura_consensus::ConsensusId,
        commitment: NonceCommitment,
    },
    /// Coordinator to witness: every participating commitment.
    SignRequest {
        consensus_id: aura_consensus::ConsensusId,
        commitments: Vec<NonceCommitment>,
    },
    /// Witness to coordinator: its signature share.
    SignShare {
        consensus_id: aura_consensus::ConsensusId,
        partial: PartialSignature,
    },
}

fn decode(envelope: &TransportEnvelope) -> Option<ChannelConsensusMessage> {
    if envelope.metadata.get("content-type").map(String::as_str)
        != Some(CHANNEL_CONSENSUS_CONTENT_TYPE)
        || envelope.payload.len() > MAX_MESSAGE_BYTES
    {
        return None;
    }
    aura_core::util::serialization::from_slice(&envelope.payload).ok()
}

async fn send(
    effects: &AuraEffectSystem,
    to: AuthorityId,
    message: &ChannelConsensusMessage,
) -> Result<(), AuraError> {
    let device = verified_device_key(effects, to)
        .await?
        .ok_or_else(|| AuraError::permission_denied(format!("no verified device for {to}")))?
        .device;
    let bytes = aura_core::util::serialization::to_vec(message)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    effects
        .send_device_payload(device.uuid(), CHANNEL_CONSENSUS_CONTENT_TYPE, bytes)
        .await
        .map_err(|error| AuraError::network(error.to_string()))
}

/// This authority's share and the group key of the `key_epoch` channel key.
async fn epoch_key(
    effects: &AuraEffectSystem,
    scope: ChannelKeyScope,
    key_epoch: u64,
) -> Result<(Share, PublicKeyPackage, Vec<AuthorityId>), AuraError> {
    let key_package = load_context_key_package(effects, scope, key_epoch).await?;
    let (roster, public) = load_roster(effects, scope, key_epoch).await?;
    let share = Share::from_frost(*key_package.identifier(), *key_package.signing_share());
    Ok((share, PublicKeyPackage::from(public), roster.participants))
}

/// The bump a round agrees on, if `round` is one: well formed, for the
/// scoped channel, to exactly `key_epoch`.
fn decode_bump(
    round: &ConsensusRound,
    scope: ChannelKeyScope,
    key_epoch: u64,
) -> Option<ProposedChannelEpochBump> {
    if !round.is_well_formed() {
        return None;
    }
    let bump: ProposedChannelEpochBump = serde_json::from_slice(&round.operation_bytes).ok()?;
    (bump.context == scope.context
        && bump.channel == scope.channel
        && bump.new_epoch == key_epoch
        && bump.new_epoch == bump.parent_epoch + 1
        && bump.transition_id == bump.transition_identity().transition_id())
    .then_some(bump)
}

/// The proposed bump of `scope` from `parent_epoch` committing the new
/// epoch's roster.
pub(crate) fn membership_bump(
    scope: ChannelKeyScope,
    parent_epoch: u64,
    roster: &[AuthorityId],
    bump_id: Hash32,
) -> Result<ProposedChannelEpochBump, AuraError> {
    let mut bump = ProposedChannelEpochBump::new(
        scope.context,
        scope.channel,
        parent_epoch,
        parent_epoch + 1,
        bump_id,
        aura_journal::fact::ChannelBumpReason::Routine,
    );
    let roster = aura_core::util::serialization::to_vec(&roster)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    bump.membership_commitment = Hash32::from_bytes(&roster);
    bump.transition_id = bump.transition_identity().transition_id();
    Ok(bump)
}

/// Run consensus on `bump` among the roster of its new epoch's channel key,
/// coordinated by this authority (which holds a share of that key).
pub(crate) async fn coordinate_channel_consensus(
    effects: &AuraEffectSystem,
    scope: ChannelKeyScope,
    prestate: &Prestate,
    bump: &ProposedChannelEpochBump,
    max_polls: u32,
) -> Result<CommitFact, AuraError> {
    let me = aura_guards::GuardContextProvider::authority_id(effects);
    let key_epoch = bump.new_epoch;
    let (share, group, roster) = epoch_key(effects, scope, key_epoch).await?;
    let threshold = group.threshold;
    let round = ConsensusRound::new(prestate, bump, effects.random_u64().await, threshold)?;
    if decode_bump(&round, scope, key_epoch).is_none() {
        return Err(AuraError::invalid(
            "bump does not move this channel to its key epoch",
        ));
    }
    let witnesses: Vec<AuthorityId> = roster.iter().copied().filter(|a| *a != me).collect();
    let execute = ChannelConsensusMessage::Execute {
        scope,
        key_epoch,
        round: round.clone(),
    };
    for witness in &witnesses {
        send(effects, *witness, &execute).await?;
    }

    let own_nonces = witness_commit(&share, effects).await?;
    let mut commitments = BTreeMap::from([(me, own_nonces.commitment().clone())]);
    let id = round.consensus_id;
    collect(
        effects,
        &witnesses,
        max_polls,
        |source, message| match message {
            ChannelConsensusMessage::NonceCommit {
                consensus_id,
                commitment,
            } if consensus_id == id && !commitments.contains_key(&source) => {
                commitments.insert(source, commitment);
                commitments.len() > witnesses.len()
            }
            _ => false,
        },
    )
    .await?;
    let commitment_list: Vec<NonceCommitment> = commitments.values().cloned().collect();
    let sign_request = ChannelConsensusMessage::SignRequest {
        consensus_id: id,
        commitments: commitment_list.clone(),
    };
    for witness in &witnesses {
        send(effects, *witness, &sign_request).await?;
    }
    // The coordinator's nonces never leave this call.
    let own_nonces = own_nonces
        .retire(&ProcessFrostNonceRetirement::default())
        .await?;
    let mut partials = BTreeMap::from([(
        me,
        witness_sign(&round, &share, own_nonces, &commitment_list, &group)?,
    )]);
    collect(
        effects,
        &witnesses,
        max_polls,
        |source, message| match message {
            ChannelConsensusMessage::SignShare {
                consensus_id,
                partial,
            } if consensus_id == id && !partials.contains_key(&source) => {
                partials.insert(source, partial);
                partials.len() > witnesses.len()
            }
            _ => false,
        },
    )
    .await?;
    let timestamp = aura_core::time::ProvenancedTime {
        stamp: aura_core::time::TimeStamp::PhysicalClock(effects.physical_time().await?),
        proofs: vec![],
        origin: None,
    };
    let participants: Vec<AuthorityId> = partials.keys().copied().collect();
    let partials: Vec<PartialSignature> = partials.into_values().collect();
    assemble_commit_fact(
        &round,
        &commitment_list,
        &partials,
        &group,
        participants,
        timestamp,
    )
}

/// Take messages from `witnesses` until `accept` reports completion.
async fn collect(
    effects: &AuraEffectSystem,
    witnesses: &[AuthorityId],
    max_polls: u32,
    mut accept: impl FnMut(AuthorityId, ChannelConsensusMessage) -> bool,
) -> Result<(), AuraError> {
    if witnesses.is_empty() {
        return Ok(());
    }
    for _ in 0..max_polls {
        while let Ok(envelope) = effects.take_inbound_envelope(|envelope| {
            witnesses.contains(&envelope.source)
                && matches!(
                    decode(envelope),
                    Some(
                        ChannelConsensusMessage::NonceCommit { .. }
                            | ChannelConsensusMessage::SignShare { .. }
                    )
                )
        }) {
            if let Some(message) = decode(&envelope) {
                if accept(envelope.source, message) {
                    return Ok(());
                }
            }
        }
        effects
            .sleep_ms(RESPONSE_POLL_MS)
            .await
            .map_err(|error| AuraError::internal(error.to_string()))?;
    }
    Err(AuraError::internal(
        "channel consensus witnesses did not answer before the deadline",
    ))
}

/// A witness's open round: the round and the single-use nonces for its share.
struct OpenRound {
    coordinator: AuthorityId,
    scope: ChannelKeyScope,
    key_epoch: u64,
    round: ConsensusRound,
    token: Option<FrostNonces>,
}

/// Witness-side state, owned by the runtime's witness loop.
#[derive(Clone, Default)]
pub(crate) struct ChannelConsensusWitness {
    open: Arc<Mutex<HashMap<aura_consensus::ConsensusId, OpenRound>>>,
    /// Retirement log for witness nonces, which never leave process memory.
    nonce_retirement: Arc<ProcessFrostNonceRetirement>,
}

impl ChannelConsensusWitness {
    /// Answer the consensus messages coordinators sent this authority.
    /// `has_standing` decides whether a coordinator may change the scoped
    /// channel's epoch.
    pub(crate) async fn process<F, Fut>(
        &self,
        effects: &AuraEffectSystem,
        has_standing: F,
    ) -> Result<usize, AuraError>
    where
        F: Fn(ChannelKeyScope, AuthorityId) -> Fut,
        Fut: std::future::Future<Output = Result<bool, AuraError>>,
    {
        let mut processed = 0;
        while let Ok(envelope) = effects.take_inbound_envelope(|envelope| {
            matches!(
                decode(envelope),
                Some(
                    ChannelConsensusMessage::Execute { .. }
                        | ChannelConsensusMessage::SignRequest { .. }
                )
            )
        }) {
            processed += 1;
            let Some(message) = decode(&envelope) else {
                continue;
            };
            let outcome = match message {
                ChannelConsensusMessage::Execute {
                    scope,
                    key_epoch,
                    round,
                } => {
                    self.execute(
                        effects,
                        envelope.source,
                        scope,
                        key_epoch,
                        round,
                        &has_standing,
                    )
                    .await
                }
                ChannelConsensusMessage::SignRequest {
                    consensus_id,
                    commitments,
                } => {
                    self.sign(effects, envelope.source, consensus_id, commitments)
                        .await
                }
                _ => Ok(()),
            };
            if let Err(error) = outcome {
                tracing::warn!(source = %envelope.source, %error, "channel consensus message refused");
            }
        }
        Ok(processed)
    }

    async fn execute<F, Fut>(
        &self,
        effects: &AuraEffectSystem,
        coordinator: AuthorityId,
        scope: ChannelKeyScope,
        key_epoch: u64,
        round: ConsensusRound,
        has_standing: &F,
    ) -> Result<(), AuraError>
    where
        F: Fn(ChannelKeyScope, AuthorityId) -> Fut,
        Fut: std::future::Future<Output = Result<bool, AuraError>>,
    {
        let (share, group, roster) = epoch_key(effects, scope, key_epoch).await?;
        if !roster.contains(&coordinator)
            || round.threshold != group.threshold
            || decode_bump(&round, scope, key_epoch).is_none()
            || !has_standing(scope, coordinator).await?
        {
            return Err(AuraError::permission_denied(
                "channel consensus round is not for this epoch's key or its coordinator lacks standing",
            ));
        }
        let nonces = witness_commit(&share, effects).await?;
        let commitment = nonces.commitment().clone();
        let consensus_id = round.consensus_id;
        self.open.lock().await.insert(
            consensus_id,
            OpenRound {
                coordinator,
                scope,
                key_epoch,
                round,
                token: Some(nonces),
            },
        );
        send(
            effects,
            coordinator,
            &ChannelConsensusMessage::NonceCommit {
                consensus_id,
                commitment,
            },
        )
        .await
    }

    async fn sign(
        &self,
        effects: &AuraEffectSystem,
        coordinator: AuthorityId,
        consensus_id: aura_consensus::ConsensusId,
        commitments: Vec<NonceCommitment>,
    ) -> Result<(), AuraError> {
        let Some(mut open) = self.open.lock().await.remove(&consensus_id) else {
            return Err(AuraError::invalid("no open channel consensus round"));
        };
        if open.coordinator != coordinator {
            return Err(AuraError::permission_denied(
                "sign request is not from the round's coordinator",
            ));
        }
        let (share, group, _) = epoch_key(effects, open.scope, open.key_epoch).await?;
        let token = open
            .token
            .take()
            .ok_or_else(|| AuraError::invalid("channel consensus nonce already used"))?;
        if !commitments
            .iter()
            .any(|commitment| commitment.signer == share.identifier)
        {
            return Err(AuraError::invalid("sign request omits this witness"));
        }
        let token = token.retire(self.nonce_retirement.as_ref()).await?;
        let partial = witness_sign(&open.round, &share, token, &commitments, &group)?;
        send(
            effects,
            coordinator,
            &ChannelConsensusMessage::SignShare {
                consensus_id,
                partial,
            },
        )
        .await
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use crate::runtime::channel_key_ceremony::{
        coordinate_channel_key_ceremony, own_device_key, process_channel_key_invites,
        record_verified_device_key, ChannelKeyInvite, CEREMONY_MAX_POLLS,
    };
    use aura_core::types::identifiers::{ChannelId, ContextId, DeviceId};

    fn runtime(shared: &crate::SharedTransport, seed: u8) -> AuraEffectSystem {
        let config = AgentConfig {
            device_id: DeviceId::new_from_entropy([seed.wrapping_add(100); 32]),
            ..AgentConfig::default()
        };
        AuraEffectSystem::simulation_for_named_test_with_shared_transport_for_authority(
            &config,
            &format!("channel-consensus-{seed}"),
            AuthorityId::new_from_entropy([seed; 32]),
            shared.clone(),
        )
        .expect("effects")
    }

    fn id(effects: &AuraEffectSystem) -> AuthorityId {
        aura_guards::GuardContextProvider::authority_id(effects)
    }

    /// Task 164 step 2: after the epoch's key ceremony, the members agree on
    /// the membership bump by consensus with their own shares; the commit
    /// verifies against the epoch's group key and names the proposed bump.
    /// A witness refuses a round for an epoch it holds no key for.
    #[tokio::test(start_paused = true)]
    async fn members_agree_on_the_bump_with_their_own_shares() {
        let shared = crate::SharedTransport::new();
        let members = [
            runtime(&shared, 101),
            runtime(&shared, 102),
            runtime(&shared, 103),
        ];
        for owner in &members {
            let entry = own_device_key(owner).await.unwrap();
            for other in &members {
                if id(other) != entry.authority {
                    record_verified_device_key(other, &entry).await.unwrap();
                }
            }
        }
        let scope = ChannelKeyScope {
            context: ContextId::new_from_entropy([104; 32]),
            channel: ChannelId::from_bytes([105; 32]),
        };
        let roster: Vec<AuthorityId> = members.iter().map(id).collect();
        let invite = ChannelKeyInvite::new(scope, 1, id(&members[0]), roster.clone()).unwrap();
        let coordinator = id(&members[0]);
        let standing =
            move |invite: ChannelKeyInvite| async move { Ok(invite.coordinator == coordinator) };
        let participate = |effects: &'static AuraEffectSystem| async move {
            loop {
                if !process_channel_key_invites(effects, standing, CEREMONY_MAX_POLLS)
                    .await
                    .unwrap()
                    .is_empty()
                {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        };
        let members: &'static [AuraEffectSystem; 3] = Box::leak(Box::new(members));
        let (key, (), ()) = futures::join!(
            coordinate_channel_key_ceremony(&members[0], &invite, CEREMONY_MAX_POLLS),
            participate(&members[1]),
            participate(&members[2]),
        );
        key.expect("ceremony");

        let bump = membership_bump(scope, 0, &invite.participants, Hash32([7; 32])).unwrap();
        let prestate =
            Prestate::new(vec![(coordinator, Hash32([6; 32]))], Hash32([8; 32])).unwrap();
        let witness = ChannelConsensusWitness::default();
        let witness_standing = move |_scope: ChannelKeyScope, from: AuthorityId| async move {
            Ok(from == coordinator)
        };
        let witness_loop = |effects: &'static AuraEffectSystem,
                            witness: ChannelConsensusWitness| async move {
            for _ in 0..200 {
                witness.process(effects, witness_standing).await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        };
        let (commit, (), ()) = futures::join!(
            coordinate_channel_consensus(&members[0], scope, &prestate, &bump, 400),
            witness_loop(&members[1], witness.clone()),
            witness_loop(&members[2], ChannelConsensusWitness::default()),
        );
        let commit = commit.expect("consensus commits");
        commit
            .verify()
            .expect("commit verifies against the epoch group key");
        let agreed: ProposedChannelEpochBump =
            serde_json::from_slice(&commit.operation_bytes).unwrap();
        assert_eq!(agreed, bump);

        // No key for epoch 2: the coordinator cannot even start, and a
        // witness refuses an epoch-2 round it holds no share for.
        let next = membership_bump(scope, 1, &invite.participants, Hash32([9; 32])).unwrap();
        assert!(
            coordinate_channel_consensus(&members[0], scope, &prestate, &next, 4)
                .await
                .is_err()
        );
    }
}
