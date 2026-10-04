//! Layer 4 public-only FROST round orchestration.
//!
//! This API proves a signature, not permission to solicit a participant.
//! Runtime ingress separately consumes explicit local approval and preserves
//! the original execution budget. Callback transport has no signing authority.
use aura_core::effects::crypto::{FrostPublicCommitment, FrostSigningPackage};
use aura_core::effects::CryptoEffects;
use aura_core::{AuraError, DeviceId, TrustedKeyDomain, TrustedKeyStatus, TrustedPublicKey};
use aura_signature::SecurityTranscript;
use std::{future::Future, sync::Arc};

#[derive(Debug, thiserror::Error)]
/// Failures in public roster validation or independently verified aggregation.
pub enum PublicTranscriptSigningError {
    #[error("selected public signing roster is not canonical or does not meet quorum")]
    /// Selected devices and indices do not form a canonical quorum.
    Roster,
    #[error("returned public commitment does not identify its selected participant")]
    /// A returned commitment does not match its selected native index.
    CommitmentIndex,
    #[error("aggregate signature fails independently retained group verification")]
    /// The aggregate does not verify against the independently retained group key.
    AggregateVerification,
    /// The retained key is not an active authority threshold verifier.
    #[error("public signing policy requires an active authority threshold verifier")]
    VerifierPolicy,
}
fn rejected(source: PublicTranscriptSigningError) -> AuraError {
    AuraError::Crypto {
        message: "public transcript signing protocol rejected".into(),
        source: Some(Arc::new(source)),
    }
}

/// Independently retained public policy. No private share or nonce is accepted.
/// Construction is not authorization; the runtime must retain the strong
/// native policy/approval capability for the entire protocol execution.
/// Raw message bytes cannot construct a public signing policy:
/// ```compile_fail
/// use aura_protocol::public_transcript_signing::PublicTranscriptSigningPolicy;
/// let _ = PublicTranscriptSigningPolicy::checked(b"raw message", b"package", b"key", 2, &[]);
/// ```
pub struct PublicTranscriptSigningPolicy<'a, T: SecurityTranscript + ?Sized> {
    transcript: &'a T,
    public_package: &'a [u8],
    verifying_key: &'a TrustedPublicKey,
    threshold: u16,
    roster: &'a [(DeviceId, u16)],
}
impl<'a, T: SecurityTranscript + ?Sized> PublicTranscriptSigningPolicy<'a, T> {
    /// Validate canonical ordering, unique devices and indices, and the quorum bound.
    /// This pure policy constructor grants no participant approval or signing custody.
    pub fn checked(
        transcript: &'a T,
        public_package: &'a [u8],
        verifying_key: &'a TrustedPublicKey,
        threshold: u16,
        roster: &'a [(DeviceId, u16)],
    ) -> Result<Self, AuraError> {
        if verifying_key.domain() != TrustedKeyDomain::AuthorityThreshold
            || verifying_key.status() != &TrustedKeyStatus::Active
            || verifying_key.epoch().is_none()
            || verifying_key.bytes().len() != 32
            || verifying_key.key_hash()
                != aura_core::Hash32(aura_core::hash::hash(verifying_key.bytes()))
        {
            return Err(rejected(PublicTranscriptSigningError::VerifierPolicy));
        }
        if threshold < 2
            || roster.len() < usize::from(threshold)
            || roster.len() > 1024
            || roster.windows(2).any(|pair| pair[0].1 >= pair[1].1)
            || roster
                .iter()
                .enumerate()
                .any(|(position, (device, index))| {
                    *index == 0
                        || roster[..position]
                            .iter()
                            .any(|(earlier, _)| earlier == device)
                })
        {
            return Err(rejected(PublicTranscriptSigningError::Roster));
        }
        let native = frost_ed25519::keys::PublicKeyPackage::deserialize(public_package).map_err(
            |source| {
                AuraError::crypto_with_source(
                    "decode retained public signing package",
                    Arc::new(source),
                )
            },
        )?;
        if native.verifying_key().serialize().as_slice() != verifying_key.bytes() {
            return Err(rejected(PublicTranscriptSigningError::VerifierPolicy));
        }
        Ok(Self {
            transcript,
            public_package,
            verifying_key,
            threshold,
            roster,
        })
    }
}

/// Callers execute this future under their existing original local window.
/// Each participant callback identifies one already admitted local owner;
/// receipt of a request never mints such an owner. Native aggregation verifies
/// the actual shares against the independently retained public package.
pub async fn sign_public_transcript<E, T, C, CF, S, SF>(
    effects: &E,
    policy: PublicTranscriptSigningPolicy<'_, T>,
    mut commitment: C,
    mut share: S,
) -> Result<Vec<u8>, AuraError>
where
    E: CryptoEffects + ?Sized,
    T: SecurityTranscript + ?Sized,
    C: FnMut(DeviceId, u16) -> CF,
    CF: Future<Output = Result<FrostPublicCommitment, AuraError>>,
    S: FnMut(DeviceId, u16, FrostSigningPackage) -> SF,
    SF: Future<Output = Result<Vec<u8>, AuraError>>,
{
    let transcript_bytes = required_public_transcript_bytes(policy.transcript)?;
    let mut commitments = Vec::with_capacity(policy.roster.len());
    for &(device, index) in policy.roster {
        let returned = commitment(device, index).await?;
        if returned.participant_index != index {
            return Err(rejected(PublicTranscriptSigningError::CommitmentIndex));
        }
        commitments.push(returned);
    }
    let package = effects
        .frost_create_public_signing_package(
            &transcript_bytes,
            &commitments,
            policy.public_package,
            policy.threshold,
        )
        .await?;
    let mut shares = Vec::with_capacity(policy.roster.len());
    for &(device, index) in policy.roster {
        shares.push(share(device, index, package.clone()).await?);
    }
    let signature = effects
        .frost_aggregate_signatures(&package, &shares)
        .await?;
    if !verify_public_transcript(effects, policy.transcript, &signature, policy.verifying_key)
        .await?
    {
        return Err(rejected(
            PublicTranscriptSigningError::AggregateVerification,
        ));
    }
    Ok(signature)
}

// Keep native cryptographic failures instead of projecting them into diagnostic
// strings. The actual typed domain encodes exactly the bytes used by aggregation.
async fn verify_public_transcript<E, T>(
    effects: &E,
    transcript: &T,
    signature: &[u8],
    verifying_key: &TrustedPublicKey,
) -> Result<bool, AuraError>
where
    E: CryptoEffects + ?Sized,
    T: SecurityTranscript + ?Sized,
{
    let transcript_bytes = required_public_transcript_bytes(transcript)?;
    effects
        .frost_verify(&transcript_bytes, signature, verifying_key.bytes())
        .await
}

fn required_public_transcript_bytes<T: SecurityTranscript + ?Sized>(
    transcript: &T,
) -> Result<Vec<u8>, AuraError> {
    transcript.required_transcript_bytes().map_err(|source| {
        AuraError::crypto_with_source("encode exact public signing transcript", Arc::new(source))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct RosterFixtureTranscript;
    impl SecurityTranscript for RosterFixtureTranscript {
        type Payload = &'static str;
        const DOMAIN_SEPARATOR: &'static str = "aura.protocol.test.public-transcript-roster.v1";
        fn transcript_payload(&self) -> Self::Payload {
            "roster fixture"
        }
    }
    #[test]
    fn rejects_wrong_domain_revoked_and_corrupt_retained_verifier_policy() {
        let bytes = vec![0; 32];
        let hash = aura_core::Hash32(aura_core::hash::hash(&bytes));
        let device_key =
            TrustedPublicKey::active(TrustedKeyDomain::Device, Some(1), bytes.clone(), hash);
        let mut revoked = TrustedPublicKey::active(
            TrustedKeyDomain::AuthorityThreshold,
            Some(1),
            bytes.clone(),
            hash,
        );
        revoked.set_status(TrustedKeyStatus::Revoked {
            reason: "actual retained policy revoked".into(),
        });
        let corrupt = TrustedPublicKey::active(
            TrustedKeyDomain::AuthorityThreshold,
            Some(1),
            bytes.clone(),
            aura_core::Hash32([1; 32]),
        );
        let missing_epoch =
            TrustedPublicKey::active(TrustedKeyDomain::AuthorityThreshold, None, bytes, hash);
        let roster = [
            (DeviceId::from_uuid(uuid::Uuid::from_u128(1)), 1),
            (DeviceId::from_uuid(uuid::Uuid::from_u128(2)), 2),
        ];
        for key in [&device_key, &revoked, &corrupt, &missing_epoch] {
            let Err(error) = PublicTranscriptSigningPolicy::checked(
                &RosterFixtureTranscript,
                b"unused package",
                key,
                2,
                &roster,
            ) else {
                panic!("non-authority or inactive policy must reject before native signing");
            };
            let Some(source) = std::error::Error::source(&error) else {
                panic!("retain verifier policy source");
            };
            assert!(matches!(
                source.downcast_ref::<PublicTranscriptSigningError>(),
                Some(PublicTranscriptSigningError::VerifierPolicy)
            ));
        }
    }
    #[test]
    fn required_public_transcript_encoding_retains_original_codec_source() {
        struct FailedPayload;
        impl serde::Serialize for FailedPayload {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom(
                    "original public transcript codec fault",
                ))
            }
        }
        struct FailedTranscript;
        impl SecurityTranscript for FailedTranscript {
            type Payload = FailedPayload;
            const DOMAIN_SEPARATOR: &'static str = "aura.protocol.test.public-transcript-codec.v1";
            fn transcript_payload(&self) -> Self::Payload {
                FailedPayload
            }
        }
        let Err(error) = required_public_transcript_bytes(&FailedTranscript) else {
            panic!("actual failed payload must reject required public encoding");
        };
        let Some(required) = std::error::Error::source(&error) else {
            panic!("required public encoding must retain its original source");
        };
        assert!(required.is::<aura_signature::RequiredTranscriptEncodingError>());
        let Some(codec) = required.source() else {
            panic!("required transcript must retain the canonical codec failure");
        };
        assert!(codec.is::<aura_core::util::serialization::SerializationError>());
        assert!(codec.source().is_some());
    }
    #[test]
    fn rejects_duplicate_device_and_noncanonical_index_rosters() {
        let verifier = TrustedPublicKey::active(
            TrustedKeyDomain::AuthorityThreshold,
            Some(1),
            vec![0; 32],
            aura_core::Hash32(aura_core::hash::hash(&[0; 32])),
        );
        let device = DeviceId::from_uuid(uuid::Uuid::from_u128(1));
        assert!(PublicTranscriptSigningPolicy::checked(
            &RosterFixtureTranscript,
            b"p",
            &verifier,
            2,
            &[(device, 1), (device, 2)]
        )
        .is_err());
        let other = DeviceId::from_uuid(uuid::Uuid::from_u128(2));
        assert!(PublicTranscriptSigningPolicy::checked(
            &RosterFixtureTranscript,
            b"p",
            &verifier,
            2,
            &[(device, 2), (other, 1)]
        )
        .is_err());
        assert!(PublicTranscriptSigningPolicy::checked(
            &RosterFixtureTranscript,
            b"p",
            &verifier,
            2,
            &[(device, 0), (other, 1)]
        )
        .is_err());
    }
}
