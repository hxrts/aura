//! Membership-change channel rekeying (work/8.md Task 164 step 5).
//!
//! A membership change (an accepted invitation, a kick, a departure) applies
//! at once as membership facts (A1). The channel's key then follows by
//! consensus (A3): when a channel's observed members differ from the roster
//! of its current key epoch, the lowest remaining current key holder
//! coordinates a key ceremony among the members for the next epoch and
//! consensus on the bump, and publishes the [`ChannelEpochCommitFact`].
//! Every member holds the new epoch's key; a later joiner holds no earlier
//! key, and a departed member no later one. One deterministic coordinator per change keeps
//! members from racing competing successor epochs.
//!
//! The current epoch's roster is the bootstrap dealer and recipients at
//! epoch 0, and the key ceremony's roster after.

use super::channel_consensus::{coordinate_channel_consensus, membership_bump};
use super::channel_key_ceremony::{coordinate_channel_key_ceremony, ChannelKeyInvite};
use super::context_dkg::{load_roster, ChannelKeyScope};
use super::AuraEffectSystem;
use aura_amp::ChannelEpochCommitFact;
use aura_core::effects::RandomCoreEffects;
use aura_core::types::identifiers::AuthorityId;
use aura_core::{AuraError, Hash32, Prestate};
use aura_journal::DomainFact;
use aura_protocol::amp::AmpJournalEffects;
use std::collections::BTreeSet;

/// Receive polls a coordinator waits on members during one rekey attempt:
/// the members' own ceremony window, so a member that admits the invite late
/// (once it observes the coordinator's standing, Task 196) still joins the
/// live attempt. An attempt that cannot finish is retried on a later round.
pub(crate) const REKEY_MAX_POLLS: u32 = super::channel_key_ceremony::CEREMONY_MAX_POLLS;

/// The roster of `scope`'s current key epoch, when this member knows it.
async fn current_roster(
    effects: &AuraEffectSystem,
    scope: ChannelKeyScope,
    epoch: u64,
    bootstrap: Option<&aura_journal::fact::ChannelBootstrap>,
) -> Option<BTreeSet<AuthorityId>> {
    if epoch == 0 {
        return bootstrap.map(|bootstrap| {
            bootstrap
                .recipients
                .iter()
                .copied()
                .chain(std::iter::once(bootstrap.dealer))
                .collect()
        });
    }
    load_roster(effects, scope, epoch)
        .await
        .ok()
        .map(|(roster, _)| roster.participants.into_iter().collect())
}

/// What a rekey check decided for one channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RekeyOutcome {
    /// Members match the current epoch's roster (or too few to key).
    Current,
    /// Another member coordinates this change.
    NotCoordinator,
    /// This member moved the channel to `epoch`.
    Rekeyed { epoch: u64 },
}

/// Rekey `scope` if its members changed and this authority coordinates.
pub(crate) async fn rekey_channel_if_changed(
    effects: &AuraEffectSystem,
    scope: ChannelKeyScope,
    max_polls: u32,
) -> Result<RekeyOutcome, AuraError> {
    let state =
        aura_protocol::amp::get_channel_state(effects, scope.context, scope.channel).await?;
    let epoch = state.chan_epoch;
    let Some(roster) = current_roster(effects, scope, epoch, state.bootstrap.as_ref()).await else {
        return Ok(RekeyOutcome::Current);
    };
    let members: BTreeSet<AuthorityId> =
        aura_amp::channel_membership_observations(effects, scope.context, scope.channel)
            .await?
            .participants()
            .collect();
    if members.len() < 2 || members == roster {
        return Ok(RekeyOutcome::Current);
    }
    let me = aura_guards::GuardContextProvider::authority_id(effects);
    tracing::debug!(
        context = %scope.context,
        channel = %scope.channel,
        epoch,
        ?members,
        ?roster,
        "channel members differ from the current key roster"
    );
    // The coordinator is the lowest member that holds the current epoch's
    // key (a roster member with standing), never a member being added: a
    // joiner holds no key to coordinate with (LAN run 176).
    if coordinator(&roster, &members) != Some(me) {
        return Ok(RekeyOutcome::NotCoordinator);
    }
    rekey_channel(effects, scope, epoch, &roster, members, max_polls).await?;
    Ok(RekeyOutcome::Rekeyed { epoch: epoch + 1 })
}

/// Move `scope` from `epoch` to `epoch + 1` keyed to `members`: the key
/// ceremony, then consensus on the bump among the new roster, then the
/// committed epoch published as a fact members sync.
pub(crate) async fn rekey_channel(
    effects: &AuraEffectSystem,
    scope: ChannelKeyScope,
    epoch: u64,
    previous_roster: &BTreeSet<AuthorityId>,
    members: BTreeSet<AuthorityId>,
    max_polls: u32,
) -> Result<ChannelEpochCommitFact, AuraError> {
    let me = aura_guards::GuardContextProvider::authority_id(effects);
    let invite = ChannelKeyInvite::new(scope, epoch + 1, me, members)?
        .with_ceremony(Hash32(effects.random_bytes_32().await));
    coordinate_channel_key_ceremony(effects, &invite, max_polls).await?;
    let bump = membership_bump(
        scope,
        epoch,
        &invite.participants,
        Hash32(effects.random_bytes_32().await),
    )?;
    let previous = aura_core::util::serialization::to_vec(previous_roster)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    let prestate = Prestate::new(
        invite
            .participants
            .iter()
            .map(|member| (*member, Hash32::from_bytes(&member.to_bytes())))
            .collect(),
        Hash32::from_bytes(&previous),
    )
    .map_err(|error| AuraError::invalid(error.to_string()))?;
    let commit = coordinate_channel_consensus(effects, scope, &prestate, &bump, max_polls).await?;
    let (_, public) = load_roster(effects, scope, invite.epoch).await?;
    let transcript = public
        .serialize()
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    let fact = ChannelEpochCommitFact::new(&bump, commit, Some(Hash32::from_bytes(&transcript)))?;
    effects
        .insert_relational_fact(fact.committed_bump_fact())
        .await?;
    effects
        .commit_relational_facts(vec![fact.to_generic()])
        .await
        .map_err(|error| AuraError::internal(error.to_string()))?;
    Ok(fact)
}

/// The rekey coordinator: the lowest current member of the current key
/// roster.
fn coordinator(
    roster: &BTreeSet<AuthorityId>,
    members: &BTreeSet<AuthorityId>,
) -> Option<AuthorityId> {
    roster.intersection(members).next().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    /// LAN run 176: a joiner with the lowest id never coordinates; the
    /// lowest member that holds the current key does.
    #[test]
    fn coordinator_is_the_lowest_current_key_holder_never_a_joiner() {
        let (joiner, low, high) = (authority(1), authority(2), authority(3));
        assert!(joiner < low && low < high);
        let roster = BTreeSet::from([low, high]);
        let members = BTreeSet::from([joiner, low, high]);
        assert_eq!(coordinator(&roster, &members), Some(low));
        // After a departure the remaining roster member coordinates.
        assert_eq!(
            coordinator(&roster, &BTreeSet::from([joiner, high])),
            Some(high)
        );
    }
}
