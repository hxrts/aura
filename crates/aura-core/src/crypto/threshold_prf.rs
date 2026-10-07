//! Threshold PRF over FROST shares, for AMP channel base keys (docs/112 §7.2,
//! docs/100). A context's DKG leaves the group secret `s` unknown to every
//! participant; the channel base key for an input `x` is
//! `KDF(s · H(x))`, where `H` hashes to a prime-order curve point with an
//! unknown discrete log.
//!
//! Any participant holding share `s_i` publishes a partial evaluation
//! `σ_i = s_i · H(x)` with a Chaum-Pedersen (DLEQ) proof that it uses the same
//! scalar as its public verifying share `Y_i = s_i · B`. Any `t` verified
//! partials combine by Lagrange interpolation in the exponent to `s · H(x)`,
//! so every member derives the same key, no one learns `s`, and a holder of
//! only public data (verifying key and shares) cannot compute it.
//!
//! This module is pure: callers supply the proof nonce from their random
//! effect, and transport of partials is the caller's concern.

use frost_ed25519::{Ciphersuite, Ed25519Group, Ed25519ScalarField, Ed25519Sha512, Field, Group};
use serde::{Deserialize, Serialize};

type Scalar = <Ed25519ScalarField as Field>::Scalar;
type Point = <Ed25519Group as Group>::Element;

const HASH_TO_POINT_DOMAIN: &[u8] = b"aura.threshold-prf.v1.point";
const CHALLENGE_DOMAIN: &[u8] = b"aura.threshold-prf.v1.dleq";
const KEY_DOMAIN: &[u8] = b"aura.threshold-prf.v1.key";
const HASH_TO_POINT_ATTEMPT_LIMIT: u32 = 1024;

/// Errors from evaluating, verifying or combining partial evaluations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThresholdPrfError {
    /// A share, nonce, point or proof scalar did not decode canonically.
    #[error("malformed {0}")]
    Malformed(&'static str),
    /// A participant identifier was zero.
    #[error("participant identifier must be non-zero")]
    ZeroIdentifier,
    /// The DLEQ proof did not verify against the participant's share.
    #[error("partial evaluation from participant {0} does not match its verifying share")]
    InvalidProof(u16),
    /// Fewer distinct partials than the threshold were supplied.
    #[error("need {needed} distinct partial evaluations, have {have}")]
    InsufficientPartials {
        /// Partials required.
        needed: usize,
        /// Distinct partials supplied.
        have: usize,
    },
    /// Hashing to the curve failed to find a point (negligible probability).
    #[error("could not hash the input to a curve point")]
    HashToPoint,
}

/// A Chaum-Pedersen proof that `log_B(Y_i) == log_H(σ_i)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DleqProof {
    /// Challenge scalar.
    pub challenge: [u8; 32],
    /// Response scalar.
    pub response: [u8; 32],
}

/// One participant's partial evaluation `σ_i = s_i · H(x)` with its proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartialEvaluation {
    /// FROST participant identifier (the Shamir x-coordinate, non-zero).
    pub participant: u16,
    /// Compressed `σ_i`.
    pub point: [u8; 32],
    /// Proof that `σ_i` uses the participant's share.
    pub proof: DleqProof,
}

fn decode_scalar(bytes: &[u8; 32], what: &'static str) -> Result<Scalar, ThresholdPrfError> {
    <Ed25519ScalarField as Field>::deserialize(bytes)
        .map_err(|_| ThresholdPrfError::Malformed(what))
}

fn decode_point(bytes: &[u8; 32], what: &'static str) -> Result<Point, ThresholdPrfError> {
    <Ed25519Group as Group>::deserialize(bytes).map_err(|_| ThresholdPrfError::Malformed(what))
}

fn encode_point(point: &Point) -> [u8; 32] {
    <Ed25519Group as Group>::serialize(point)
}

fn identifier_scalar(participant: u16) -> Result<Scalar, ThresholdPrfError> {
    if participant == 0 {
        return Err(ThresholdPrfError::ZeroIdentifier);
    }
    Ok(Scalar::from(u64::from(participant)))
}

/// Hash `input` to a prime-order point with an unknown discrete log
/// (try-and-increment; the input is public, so variable time is fine).
pub fn hash_to_point(input: &[u8]) -> Result<[u8; 32], ThresholdPrfError> {
    hash_to_point_inner(input).map(|point| encode_point(&point))
}

fn hash_to_point_inner(input: &[u8]) -> Result<Point, ThresholdPrfError> {
    for counter in 0..HASH_TO_POINT_ATTEMPT_LIMIT {
        let mut material = Vec::with_capacity(HASH_TO_POINT_DOMAIN.len() + input.len() + 4);
        material.extend_from_slice(HASH_TO_POINT_DOMAIN);
        material.extend_from_slice(&counter.to_le_bytes());
        material.extend_from_slice(input);
        let digest = Ed25519Sha512::H4(&material);
        let mut candidate = [0u8; 32];
        candidate.copy_from_slice(&digest[..32]);
        // Canonical decoding rejects the identity and points outside the
        // prime-order subgroup.
        if let Ok(point) = <Ed25519Group as Group>::deserialize(&candidate) {
            return Ok(point);
        }
    }
    Err(ThresholdPrfError::HashToPoint)
}

fn challenge(points: [&Point; 6]) -> Scalar {
    let mut material = Vec::with_capacity(CHALLENGE_DOMAIN.len() + 6 * 32);
    material.extend_from_slice(CHALLENGE_DOMAIN);
    for point in points {
        material.extend_from_slice(&encode_point(point));
    }
    Ed25519Sha512::H2(&material)
}

/// Evaluate this participant's partial `s_i · H(input)` with a DLEQ proof.
/// `share` is the FROST signing share (canonical scalar); `nonce` must be
/// fresh, uniformly random bytes from the caller's random effect.
pub fn evaluate_partial(
    participant: u16,
    share: &[u8; 32],
    input: &[u8],
    nonce: &[u8; 64],
) -> Result<PartialEvaluation, ThresholdPrfError> {
    identifier_scalar(participant)?;
    let secret = decode_scalar(share, "share")?;
    let generator = <Ed25519Group as Group>::generator();
    let base = hash_to_point_inner(input)?;
    let verifying = generator * secret;
    let evaluation = base * secret;
    let k = Ed25519Sha512::H2(nonce);
    let (commit_g, commit_h) = (generator * k, base * k);
    let c = challenge([
        &generator,
        &verifying,
        &base,
        &evaluation,
        &commit_g,
        &commit_h,
    ]);
    let z = k + c * secret;
    Ok(PartialEvaluation {
        participant,
        point: encode_point(&evaluation),
        proof: DleqProof {
            challenge: <Ed25519ScalarField as Field>::serialize(&c),
            response: <Ed25519ScalarField as Field>::serialize(&z),
        },
    })
}

/// Verify a partial against the participant's public verifying share.
pub fn verify_partial(
    verifying_share: &[u8; 32],
    input: &[u8],
    partial: &PartialEvaluation,
) -> Result<(), ThresholdPrfError> {
    identifier_scalar(partial.participant)?;
    let generator = <Ed25519Group as Group>::generator();
    let verifying = decode_point(verifying_share, "verifying share")?;
    let base = hash_to_point_inner(input)?;
    let evaluation = decode_point(&partial.point, "partial evaluation")?;
    let c = decode_scalar(&partial.proof.challenge, "proof challenge")?;
    let z = decode_scalar(&partial.proof.response, "proof response")?;
    // z·B - c·Y = k·B and z·H - c·σ = k·H for an honest prover.
    let commit_g = generator * z - verifying * c;
    let commit_h = base * z - evaluation * c;
    if challenge([
        &generator,
        &verifying,
        &base,
        &evaluation,
        &commit_g,
        &commit_h,
    ]) == c
    {
        Ok(())
    } else {
        Err(ThresholdPrfError::InvalidProof(partial.participant))
    }
}

/// Combine at least `threshold` verified partials (distinct participants) into
/// the PRF output and derive the 32-byte key bound to `input`.
///
/// Callers must verify every partial with [`verify_partial`] first.
pub fn combine(
    threshold: usize,
    input: &[u8],
    partials: &[PartialEvaluation],
) -> Result<[u8; 32], ThresholdPrfError> {
    let mut chosen: Vec<&PartialEvaluation> = Vec::new();
    for partial in partials {
        if !chosen
            .iter()
            .any(|seen| seen.participant == partial.participant)
        {
            chosen.push(partial);
        }
        if chosen.len() == threshold {
            break;
        }
    }
    if threshold == 0 || chosen.len() < threshold {
        return Err(ThresholdPrfError::InsufficientPartials {
            needed: threshold,
            have: chosen.len(),
        });
    }
    let identifiers = chosen
        .iter()
        .map(|partial| identifier_scalar(partial.participant))
        .collect::<Result<Vec<_>, _>>()?;
    let mut output = <Ed25519Group as Group>::identity();
    for (index, partial) in chosen.iter().enumerate() {
        // Lagrange coefficient at zero: Π_{j≠i} x_j / (x_j - x_i).
        let x_i = identifiers[index];
        let mut numerator = <Ed25519ScalarField as Field>::one();
        let mut denominator = <Ed25519ScalarField as Field>::one();
        for (other, x_j) in identifiers.iter().enumerate() {
            if other != index {
                numerator *= *x_j;
                denominator *= *x_j - x_i;
            }
        }
        let lambda = numerator
            * <Ed25519ScalarField as Field>::invert(&denominator)
                .map_err(|_| ThresholdPrfError::ZeroIdentifier)?;
        output += decode_point(&partial.point, "partial evaluation")? * lambda;
    }
    let mut material = Vec::with_capacity(KEY_DOMAIN.len() + 32 + input.len());
    material.extend_from_slice(KEY_DOMAIN);
    material.extend_from_slice(&encode_point(&output));
    material.extend_from_slice(input);
    let digest = Ed25519Sha512::H4(&material);
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest[..32]);
    Ok(key)
}

/// The canonical PRF input for an AMP channel base key.
#[must_use]
pub fn channel_base_key_input(
    context: &crate::types::identifiers::ContextId,
    channel: &crate::types::identifiers::ChannelId,
    epoch: u64,
) -> Vec<u8> {
    let mut input = Vec::with_capacity(25 + 16 + 32 + 8);
    input.extend_from_slice(b"aura.amp.channel-base-key");
    input.extend_from_slice(context.as_bytes());
    input.extend_from_slice(channel.as_bytes());
    input.extend_from_slice(&epoch.to_le_bytes());
    input
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use frost_ed25519::keys::{generate_with_dealer, IdentifierList, KeyPackage};
    use rand::SeedableRng;

    struct Member {
        participant: u16,
        share: [u8; 32],
        verifying_share: [u8; 32],
    }

    fn ctx() -> crate::types::identifiers::ContextId {
        crate::types::identifiers::ContextId::new_from_entropy([1; 32])
    }

    fn chan(byte: u8) -> crate::types::identifiers::ChannelId {
        crate::types::identifiers::ChannelId::from_bytes([byte; 32])
    }

    fn dealt(threshold: u16, total: u16, seed: u64) -> Vec<Member> {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let (shares, _public) =
            generate_with_dealer(total, threshold, IdentifierList::Default, &mut rng)
                .expect("dealer split");
        let mut members: Vec<Member> = shares
            .into_iter()
            .enumerate()
            .map(|(index, (_id, secret_share))| {
                let package = KeyPackage::try_from(secret_share).expect("key package");
                let participant = u16::try_from(index + 1).expect("small");
                Member {
                    participant,
                    share: package.signing_share().serialize(),
                    verifying_share: package.verifying_share().serialize(),
                }
            })
            .collect();
        members.sort_by_key(|member| member.participant);
        members
    }

    fn partial(member: &Member, input: &[u8], nonce_seed: u8) -> PartialEvaluation {
        let partial = evaluate_partial(member.participant, &member.share, input, &[nonce_seed; 64])
            .expect("partial");
        verify_partial(&member.verifying_share, input, &partial).expect("proof verifies");
        partial
    }

    #[test]
    fn any_threshold_subset_derives_the_same_key() {
        let members = dealt(2, 3, 7);
        let input = channel_base_key_input(&ctx(), &chan(2), 1);
        let all: Vec<_> = members
            .iter()
            .enumerate()
            .map(|(i, m)| partial(m, &input, i as u8 + 1))
            .collect();
        let a = combine(2, &input, &[all[0], all[1]]).unwrap();
        let b = combine(2, &input, &[all[1], all[2]]).unwrap();
        let c = combine(2, &input, &[all[2], all[0]]).unwrap();
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn keys_differ_across_channels_and_epochs() {
        let members = dealt(2, 3, 8);
        let key = |channel: u8, epoch: u64| {
            let input = channel_base_key_input(&ctx(), &chan(channel), epoch);
            let partials: Vec<_> = members[..2].iter().map(|m| partial(m, &input, 3)).collect();
            combine(2, &input, &partials).unwrap()
        };
        assert_ne!(key(2, 1), key(2, 2), "epoch rotation changes the key");
        assert_ne!(key(2, 1), key(3, 1), "channels have distinct keys");
    }

    #[test]
    fn a_tampered_or_mismatched_partial_is_rejected() {
        let members = dealt(2, 3, 9);
        let input = channel_base_key_input(&ctx(), &chan(2), 1);
        let honest = partial(&members[0], &input, 4);
        // Verified against another participant's share.
        assert_eq!(
            verify_partial(&members[1].verifying_share, &input, &honest),
            Err(ThresholdPrfError::InvalidProof(honest.participant))
        );
        // A partial for a different input does not verify for this one.
        let other_input = channel_base_key_input(&ctx(), &chan(2), 2);
        assert!(verify_partial(&members[0].verifying_share, &other_input, &honest).is_err());
        // A substituted evaluation point fails.
        let forged = PartialEvaluation {
            point: partial(&members[1], &input, 5).point,
            ..honest
        };
        assert!(verify_partial(&members[0].verifying_share, &input, &forged).is_err());
    }

    #[test]
    fn fewer_than_threshold_partials_cannot_combine() {
        let members = dealt(3, 4, 10);
        let input = channel_base_key_input(&ctx(), &chan(2), 1);
        let one = partial(&members[0], &input, 6);
        assert_eq!(
            combine(3, &input, &[one, one, partial(&members[1], &input, 7)]),
            Err(ThresholdPrfError::InsufficientPartials { needed: 3, have: 2 }),
            "duplicates do not count toward the threshold"
        );
    }

    #[test]
    fn public_data_alone_does_not_give_the_key() {
        // A non-member knows the verifying shares and the input; combining
        // public points as if they were partials yields a different key.
        let members = dealt(2, 3, 11);
        let input = channel_base_key_input(&ctx(), &chan(2), 1);
        let honest: Vec<_> = members[..2].iter().map(|m| partial(m, &input, 8)).collect();
        let key = combine(2, &input, &honest).unwrap();
        let impostor: Vec<_> = members[..2]
            .iter()
            .zip(&honest)
            .map(|(m, h)| PartialEvaluation {
                point: m.verifying_share,
                ..*h
            })
            .collect();
        assert_ne!(combine(2, &input, &impostor).unwrap(), key);
        assert!(verify_partial(&members[0].verifying_share, &input, &impostor[0]).is_err());
    }

    #[test]
    fn hash_to_point_is_deterministic_and_input_bound() {
        assert_eq!(hash_to_point(b"x").unwrap(), hash_to_point(b"x").unwrap());
        assert_ne!(hash_to_point(b"x").unwrap(), hash_to_point(b"y").unwrap());
    }
}
