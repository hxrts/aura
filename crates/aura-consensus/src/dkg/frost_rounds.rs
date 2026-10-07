//! Real distributed key generation rounds (work/8.md Task 59).
//!
//! Wraps `frost_ed25519::keys::dkg` (Pedersen VSS with proofs of knowledge)
//! for the participants of a [`DkgConfig`]: participant `i` in
//! `config.participants` has FROST identifier `i + 1`. Round 1 packages are
//! broadcast; each round 2 package is for exactly one recipient and must be
//! sealed to it in transit. Round 3 verifies everything received and yields
//! the participant's key package and the shared public key package.
//!
//! The module is pure: randomness comes from the caller (seed it from
//! `RandomEffects`), and transport is the ceremony's concern.

use super::types::DkgConfig;
use aura_core::{AuraError, AuthorityId, Result};
use frost_ed25519::keys::dkg::{part1, part2, part3, round1, round2};
use frost_ed25519::keys::{KeyPackage, PublicKeyPackage};
use frost_ed25519::Identifier;
use rand::{CryptoRng, RngCore};
use std::collections::BTreeMap;

/// The FROST identifier of `authority` in `config` (its 1-based position).
pub fn participant_identifier(config: &DkgConfig, authority: AuthorityId) -> Result<Identifier> {
    let position = config
        .participants
        .iter()
        .position(|participant| *participant == authority)
        .ok_or_else(|| AuraError::invalid("authority is not a DKG participant"))?;
    let index =
        u16::try_from(position + 1).map_err(|_| AuraError::invalid("too many DKG participants"))?;
    Identifier::try_from(index).map_err(|error| AuraError::invalid(error.to_string()))
}

fn validate(config: &DkgConfig) -> Result<()> {
    let total = u16::try_from(config.participants.len())
        .map_err(|_| AuraError::invalid("too many DKG participants"))?;
    if total != config.max_signers {
        return Err(AuraError::invalid(
            "DKG participant count must equal max_signers",
        ));
    }
    if config.threshold < 1 || config.threshold > config.max_signers {
        return Err(AuraError::invalid(
            "DKG threshold must be in 1..=max_signers",
        ));
    }
    Ok(())
}

/// Round 1: generate this participant's secret polynomial and the package to
/// broadcast to every other participant.
pub fn round_one<R: RngCore + CryptoRng>(
    config: &DkgConfig,
    me: AuthorityId,
    rng: &mut R,
) -> Result<(round1::SecretPackage, round1::Package)> {
    validate(config)?;
    part1(
        participant_identifier(config, me)?,
        config.max_signers,
        config.threshold,
        rng,
    )
    .map_err(|error| AuraError::crypto(format!("DKG round 1: {error}")))
}

/// Round 2: verify every other participant's round 1 package (proofs of
/// knowledge) and produce one private package per recipient.
pub fn round_two(
    secret: round1::SecretPackage,
    received: &BTreeMap<Identifier, round1::Package>,
) -> Result<(round2::SecretPackage, BTreeMap<Identifier, round2::Package>)> {
    part2(secret, received).map_err(|error| AuraError::crypto(format!("DKG round 2: {error}")))
}

/// Round 3: verify the private shares received against the round 1
/// commitments and derive this participant's key package and the group's
/// public key package.
pub fn round_three(
    secret: &round2::SecretPackage,
    round_one_received: &BTreeMap<Identifier, round1::Package>,
    round_two_received: &BTreeMap<Identifier, round2::Package>,
) -> Result<(KeyPackage, PublicKeyPackage)> {
    part3(secret, round_one_received, round_two_received)
        .map_err(|error| AuraError::crypto(format!("DKG round 3: {error}")))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use aura_core::crypto::threshold_prf::{
        channel_base_key_input, combine, evaluate_partial, verify_partial,
    };
    use aura_core::Hash32;
    use rand::SeedableRng;

    fn config(threshold: u16, total: u8) -> DkgConfig {
        DkgConfig {
            epoch: 1,
            threshold,
            max_signers: u16::from(total),
            membership_hash: Hash32::default(),
            cutoff: 0,
            prestate_hash: Hash32::default(),
            operation_hash: Hash32::default(),
            participants: (1..=total)
                .map(|seed| AuthorityId::new_from_entropy([seed; 32]))
                .collect(),
        }
    }

    /// Run all three rounds for every participant, as the ceremony would.
    fn run(config: &DkgConfig, seed: u64) -> Vec<(KeyPackage, PublicKeyPackage)> {
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(seed);
        let ids: Vec<Identifier> = config
            .participants
            .iter()
            .map(|authority| participant_identifier(config, *authority).unwrap())
            .collect();
        let mut secrets1 = BTreeMap::new();
        let mut broadcast = BTreeMap::new();
        for (authority, id) in config.participants.iter().zip(&ids) {
            let (secret, package) = round_one(config, *authority, &mut rng).unwrap();
            secrets1.insert(*id, secret);
            broadcast.insert(*id, package);
        }
        let mut secrets2 = BTreeMap::new();
        let mut inboxes: BTreeMap<Identifier, BTreeMap<Identifier, round2::Package>> =
            BTreeMap::new();
        for id in &ids {
            let mut others = broadcast.clone();
            others.remove(id);
            let (secret, outgoing) = round_two(secrets1.remove(id).unwrap(), &others).unwrap();
            secrets2.insert(*id, secret);
            for (recipient, package) in outgoing {
                inboxes.entry(recipient).or_default().insert(*id, package);
            }
        }
        ids.iter()
            .map(|id| {
                let mut others = broadcast.clone();
                others.remove(id);
                round_three(&secrets2[id], &others, &inboxes[id]).unwrap()
            })
            .collect()
    }

    #[test]
    fn every_participant_agrees_on_the_group_key() {
        let config = config(2, 3);
        let outcomes = run(&config, 1);
        let group = outcomes[0].1.verifying_key();
        assert!(outcomes
            .iter()
            .all(|(_, public)| public.verifying_key() == group));
    }

    #[test]
    fn dkg_shares_drive_the_threshold_prf() {
        // Task 55: any threshold of context members derives the same channel
        // key from real DKG shares.
        let config = config(2, 3);
        let outcomes = run(&config, 2);
        let input = channel_base_key_input(
            &aura_core::types::identifiers::ContextId::new_from_entropy([9; 32]),
            &aura_core::types::identifiers::ChannelId::from_bytes([8; 32]),
            1,
        );
        let partial = |index: usize, nonce: u8| {
            let (key, public) = &outcomes[index];
            let participant = u16::try_from(index + 1).unwrap();
            let share = key.signing_share().serialize();
            let partial = evaluate_partial(participant, &share, &input, &[nonce; 64]).unwrap();
            let verifying = public.verifying_shares()[key.identifier()].serialize();
            verify_partial(&verifying, &input, &partial).expect("verifies");
            partial
        };
        let a = combine(2, &input, &[partial(0, 1), partial(1, 2)]).unwrap();
        let b = combine(2, &input, &[partial(1, 3), partial(2, 4)]).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_tampered_private_share_is_detected() {
        let config = config(2, 3);
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(3);
        let ids: Vec<Identifier> = config
            .participants
            .iter()
            .map(|authority| participant_identifier(&config, *authority).unwrap())
            .collect();
        let mut secrets1 = BTreeMap::new();
        let mut broadcast = BTreeMap::new();
        for (authority, id) in config.participants.iter().zip(&ids) {
            let (secret, package) = round_one(&config, *authority, &mut rng).unwrap();
            secrets1.insert(*id, secret);
            broadcast.insert(*id, package);
        }
        let mut secrets2 = BTreeMap::new();
        let mut inboxes: BTreeMap<Identifier, BTreeMap<Identifier, round2::Package>> =
            BTreeMap::new();
        for id in &ids {
            let mut others = broadcast.clone();
            others.remove(id);
            let (secret, outgoing) = round_two(secrets1.remove(id).unwrap(), &others).unwrap();
            secrets2.insert(*id, secret);
            for (recipient, package) in outgoing {
                inboxes.entry(recipient).or_default().insert(*id, package);
            }
        }
        // Participant 1 receives participant 3's share meant for participant 2.
        let (victim, sender, intended) = (ids[0], ids[2], ids[1]);
        let swapped = inboxes[&intended][&sender].clone();
        inboxes.get_mut(&victim).unwrap().insert(sender, swapped);
        let mut others = broadcast.clone();
        others.remove(&victim);
        assert!(round_three(&secrets2[&victim], &others, &inboxes[&victim]).is_err());
    }

    #[test]
    fn configs_must_be_consistent() {
        let mut bad = config(2, 3);
        bad.max_signers = 4;
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(4);
        assert!(round_one(&bad, bad.participants[0], &mut rng).is_err());
        let outsider = AuthorityId::new_from_entropy([99; 32]);
        assert!(participant_identifier(&config(2, 3), outsider).is_err());
    }
}
