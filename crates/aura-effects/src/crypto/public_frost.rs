//! Audited public-only FROST package construction and bound-message signing.
use aura_core::effects::crypto::{CryptoError, FrostPublicCommitment, FrostSigningPackage};
use aura_core::util::serialization::{from_slice, SerializationError};
use aura_core::AuraError;
use frost_ed25519 as frost;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use zeroize::Zeroizing;

const MAX_PUBLIC_BYTES: usize = 1_048_576;
const MAX_PARTICIPANTS: usize = 1024;
const MAX_LOCAL_BYTES: usize = 4096;

/// Concrete rejection causes at the audited public FROST boundary.
#[derive(Debug, thiserror::Error)]
pub enum PublicFrostSigningError {
    /// Input exceeds the bounded primitive's admission policy.
    #[error("FROST input bounds exceeded at {stage}")]
    Bounds {
        /// Required operation whose input violated the bound.
        stage: &'static str,
    },
    /// Selected participants cannot satisfy the supplied admitted quorum.
    #[error("invalid FROST quorum {threshold}, selected {selected}, available {available}")]
    Quorum {
        /// Independently admitted threshold.
        threshold: u16,
        /// Number of selected public commitments.
        selected: usize,
        /// Number of actual public verifying shares.
        available: usize,
    },
    /// A participant was included more than once.
    #[error("duplicate FROST participant {0}")]
    DuplicateParticipant(u16),
    /// A selected index has no share in the actual public package.
    #[error("unknown FROST participant {0}")]
    UnknownParticipant(u16),
    /// Public commitments differ from the actual local nonces.
    #[error("local FROST commitments mismatch")]
    CommitmentMismatch,
    /// Native or DTO message differs from the independently admitted message.
    #[error("FROST signing message mismatch")]
    MessageMismatch,
    /// Public package differs from the independently admitted canonical bytes.
    #[error("FROST public package mismatch")]
    PublicPackageMismatch,
    /// DTO participant order or contents differ from the native package.
    #[error("FROST participant inventory mismatch")]
    ParticipantInventoryMismatch,
    /// Aggregation must receive one share per selected participant.
    #[error("FROST signature share inventory mismatch: expected {expected}, received {received}")]
    ShareInventory {
        /// Selected participant count.
        expected: usize,
        /// Supplied signature share count.
        received: usize,
    },
    /// Own share, verifying key or threshold differs from admitted policy.
    #[error("local FROST key package mismatch")]
    KeyPackageMismatch,
    /// Audited library failure with its original cause.
    #[error("native FROST failure at {stage}: {source}")]
    Native {
        /// Required native operation.
        stage: &'static str,
        /// Original audited library error.
        #[source]
        source: frost::Error,
    },
    /// Local nonce container failure with its original codec cause.
    #[error("FROST nonce codec failure: {0}")]
    Codec(#[source] SerializationError),
}

fn rejected(source: PublicFrostSigningError) -> CryptoError {
    AuraError::Crypto {
        message: "audited public FROST operation rejected".into(),
        source: Some(Arc::new(source)),
    }
}
fn native(stage: &'static str, source: frost::Error) -> CryptoError {
    rejected(PublicFrostSigningError::Native { stage, source })
}
fn bounded(bytes: &[u8], limit: usize, stage: &'static str) -> Result<(), CryptoError> {
    if bytes.is_empty() || bytes.len() > limit {
        return Err(rejected(PublicFrostSigningError::Bounds { stage }));
    }
    Ok(())
}
fn local_nonces(bundle: &[u8]) -> Result<Zeroizing<frost::round1::SigningNonces>, CryptoError> {
    bounded(bundle, MAX_LOCAL_BYTES, "local nonce bundle")?;
    let (secret, public): (Vec<u8>, Vec<u8>) =
        from_slice(bundle).map_err(|source| rejected(PublicFrostSigningError::Codec(source)))?;
    let secret = Zeroizing::new(secret);
    let nonces = Zeroizing::new(
        frost::round1::SigningNonces::deserialize(&secret)
            .map_err(|source| native("local nonces", source))?,
    );
    let supplied = frost::round1::SigningCommitments::deserialize(&public)
        .map_err(|source| native("local public commitments", source))?;
    // Use the audited library's public conversion for both nonce commitments.
    // Check its cached commitment and the outer public component independently.
    let derived =
        frost::round1::SigningCommitments::new(nonces.hiding().into(), nonces.binding().into());
    if supplied != derived || frost::round1::SigningCommitments::from(&*nonces) != derived {
        return Err(rejected(PublicFrostSigningError::CommitmentMismatch));
    }
    Ok(nonces)
}

pub(super) fn public_commitment(
    participant_index: u16,
    bundle: &[u8],
) -> Result<FrostPublicCommitment, CryptoError> {
    frost::Identifier::try_from(participant_index)
        .map_err(|source| native("participant index", source))?;
    let nonces = local_nonces(bundle)?;
    let commitment_bytes = frost::round1::SigningCommitments::from(&*nonces)
        .serialize()
        .map_err(|source| native("public commitment encoding", source))?;
    Ok(FrostPublicCommitment {
        participant_index,
        commitment_bytes,
    })
}

fn public_package(bytes: &[u8]) -> Result<frost::keys::PublicKeyPackage, CryptoError> {
    bounded(bytes, MAX_PUBLIC_BYTES, "public key package")?;
    let package = frost::keys::PublicKeyPackage::deserialize(bytes)
        .map_err(|source| native("public key package", source))?;
    if package.verifying_shares().is_empty() || package.verifying_shares().len() > MAX_PARTICIPANTS
    {
        return Err(rejected(PublicFrostSigningError::Bounds {
            stage: "public participant inventory",
        }));
    }
    if package
        .serialize()
        .map_err(|source| native("public key encoding", source))?
        != bytes
    {
        return Err(rejected(PublicFrostSigningError::PublicPackageMismatch));
    }
    Ok(package)
}
fn quorum(threshold: u16, selected: usize, available: usize) -> Result<(), CryptoError> {
    if threshold == 0 || usize::from(threshold) > selected || selected > available {
        return Err(rejected(PublicFrostSigningError::Quorum {
            threshold,
            selected,
            available,
        }));
    }
    Ok(())
}

pub(super) fn create_package(
    message: &[u8],
    entries: &[FrostPublicCommitment],
    public_bytes: &[u8],
    threshold: u16,
) -> Result<FrostSigningPackage, CryptoError> {
    if message.len() > MAX_PUBLIC_BYTES || entries.len() > MAX_PARTICIPANTS {
        return Err(rejected(PublicFrostSigningError::Bounds {
            stage: "public signing intent",
        }));
    }
    let public = public_package(public_bytes)?;
    quorum(threshold, entries.len(), public.verifying_shares().len())?;
    let mut commitments = BTreeMap::new();
    let mut participants = BTreeSet::new();
    for entry in entries {
        let id = frost::Identifier::try_from(entry.participant_index)
            .map_err(|source| native("participant index", source))?;
        if !public.verifying_shares().contains_key(&id) {
            return Err(rejected(PublicFrostSigningError::UnknownParticipant(
                entry.participant_index,
            )));
        }
        if !participants.insert(entry.participant_index) {
            return Err(rejected(PublicFrostSigningError::DuplicateParticipant(
                entry.participant_index,
            )));
        }
        bounded(
            &entry.commitment_bytes,
            MAX_LOCAL_BYTES,
            "public commitment",
        )?;
        let commitment = frost::round1::SigningCommitments::deserialize(&entry.commitment_bytes)
            .map_err(|source| native("public commitment", source))?;
        if commitment
            .serialize()
            .map_err(|source| native("public commitment encoding", source))?
            != entry.commitment_bytes
        {
            return Err(rejected(PublicFrostSigningError::CommitmentMismatch));
        }
        commitments.insert(id, commitment);
    }
    let package = frost::SigningPackage::new(commitments, message)
        .serialize()
        .map_err(|source| native("signing package encoding", source))?;
    Ok(FrostSigningPackage {
        message: message.to_vec(),
        package,
        participants: participants.into_iter().collect(),
        public_key_package: public_bytes.to_vec(),
    })
}

pub(super) fn sign_for_message(
    outer: &FrostSigningPackage,
    share_bytes: &[u8],
    nonce_bundle: &[u8],
    expected_message: &[u8],
    expected_public: &[u8],
    expected_threshold: u16,
) -> Result<Vec<u8>, CryptoError> {
    if expected_message.len() > MAX_PUBLIC_BYTES || outer.participants.len() > MAX_PARTICIPANTS {
        return Err(rejected(PublicFrostSigningError::Bounds {
            stage: "admitted signing intent",
        }));
    }
    if outer.message != expected_message {
        return Err(rejected(PublicFrostSigningError::MessageMismatch));
    }
    if outer.public_key_package != expected_public {
        return Err(rejected(PublicFrostSigningError::PublicPackageMismatch));
    }
    let public = public_package(expected_public)?;
    quorum(
        expected_threshold,
        outer.participants.len(),
        public.verifying_shares().len(),
    )?;
    bounded(&outer.package, MAX_PUBLIC_BYTES * 2, "signing package")?;
    let package = frost::SigningPackage::deserialize(&outer.package)
        .map_err(|source| native("signing package", source))?;
    if package.message().as_slice() != expected_message {
        return Err(rejected(PublicFrostSigningError::MessageMismatch));
    }
    if package
        .serialize()
        .map_err(|source| native("signing package encoding", source))?
        != outer.package
    {
        return Err(rejected(
            PublicFrostSigningError::ParticipantInventoryMismatch,
        ));
    }
    let mut identifiers = BTreeSet::new();
    if outer.participants.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(rejected(
            PublicFrostSigningError::ParticipantInventoryMismatch,
        ));
    }
    for index in &outer.participants {
        let id = frost::Identifier::try_from(*index)
            .map_err(|source| native("participant index", source))?;
        if !public.verifying_shares().contains_key(&id) {
            return Err(rejected(PublicFrostSigningError::UnknownParticipant(
                *index,
            )));
        }
        identifiers.insert(id);
    }
    if identifiers != package.signing_commitments().keys().copied().collect() {
        return Err(rejected(
            PublicFrostSigningError::ParticipantInventoryMismatch,
        ));
    }
    bounded(share_bytes, MAX_LOCAL_BYTES, "local key package")?;
    let own = Zeroizing::new(
        frost::keys::KeyPackage::deserialize(share_bytes)
            .map_err(|source| native("local key package", source))?,
    );
    if *own.min_signers() != expected_threshold
        || own.verifying_key() != public.verifying_key()
        || public.verifying_shares().get(own.identifier()) != Some(own.verifying_share())
        || !identifiers.contains(own.identifier())
    {
        return Err(rejected(PublicFrostSigningError::KeyPackageMismatch));
    }
    let nonces = local_nonces(nonce_bundle)?;
    frost::round2::sign(&package, &nonces, &own)
        .map(|share| share.serialize().to_vec())
        .map_err(|source| native("bound message signing", source))
}

pub(super) fn aggregate(
    outer: &FrostSigningPackage,
    share_bytes: &[Vec<u8>],
) -> Result<Vec<u8>, CryptoError> {
    if outer.participants.is_empty() || outer.participants.len() > MAX_PARTICIPANTS {
        return Err(rejected(PublicFrostSigningError::Bounds {
            stage: "aggregation inventory",
        }));
    }
    if share_bytes.len() != outer.participants.len() {
        return Err(rejected(PublicFrostSigningError::ShareInventory {
            expected: outer.participants.len(),
            received: share_bytes.len(),
        }));
    }
    if outer
        .participants
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .len()
        != outer.participants.len()
    {
        return Err(rejected(
            PublicFrostSigningError::ParticipantInventoryMismatch,
        ));
    }
    bounded(&outer.package, MAX_PUBLIC_BYTES * 2, "aggregation package")?;
    let package = frost::SigningPackage::deserialize(&outer.package)
        .map_err(|source| native("aggregation package", source))?;
    if outer.message.len() > MAX_PUBLIC_BYTES || package.message().as_slice() != outer.message {
        return Err(rejected(PublicFrostSigningError::MessageMismatch));
    }
    let public = public_package(&outer.public_key_package)?;
    let mut shares = BTreeMap::new();
    for (index, bytes) in outer.participants.iter().zip(share_bytes) {
        let id = frost::Identifier::try_from(*index)
            .map_err(|source| native("aggregation participant", source))?;
        if !public.verifying_shares().contains_key(&id) {
            return Err(rejected(PublicFrostSigningError::UnknownParticipant(
                *index,
            )));
        }
        let bytes: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
            rejected(PublicFrostSigningError::Bounds {
                stage: "signature share",
            })
        })?;
        let share = frost::round2::SignatureShare::deserialize(bytes)
            .map_err(|source| native("signature share", source))?;
        shares.insert(id, share);
    }
    if shares.keys().copied().collect::<BTreeSet<_>>()
        != package.signing_commitments().keys().copied().collect()
    {
        return Err(rejected(
            PublicFrostSigningError::ParticipantInventoryMismatch,
        ));
    }
    frost::aggregate(&package, &shares, &public)
        .map(|signature| signature.serialize().to_vec())
        .map_err(|source| native("signature aggregation", source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::RealCryptoHandler;
    use aura_core::effects::CryptoExtendedEffects;

    fn cause(error: &CryptoError) -> &PublicFrostSigningError {
        match error {
            AuraError::Crypto {
                source: Some(source),
                ..
            } => source
                .downcast_ref::<PublicFrostSigningError>()
                .expect("retain concrete public signing rejection"),
            _ => panic!("missing concrete public signing rejection"),
        }
    }

    #[tokio::test]
    async fn public_commitments_sign_real_two_of_three_without_remote_nonces() {
        let dealer = RealCryptoHandler::for_simulation_seed([0x91; 32]);
        let keys = dealer.frost_generate_keys(2, 3).await.unwrap();
        let first = RealCryptoHandler::for_simulation_seed([0x92; 32]);
        let third = RealCryptoHandler::for_simulation_seed([0x93; 32]);
        let nonce1 = first
            .frost_generate_nonces(&keys.key_packages[0])
            .await
            .unwrap();
        let nonce3 = third
            .frost_generate_nonces(&keys.key_packages[2])
            .await
            .unwrap();
        let public1 = first.frost_public_commitment(1, &nonce1).await.unwrap();
        let public3 = third.frost_public_commitment(3, &nonce3).await.unwrap();
        let message = b"independently admitted enrollment intent";
        let package = dealer
            .frost_create_public_signing_package(
                message,
                &[public3, public1],
                &keys.public_key_package,
                2,
            )
            .await
            .unwrap();
        assert_eq!(package.participants, vec![1, 3]);
        let share1 = first
            .frost_sign_share_for_message(
                &package,
                &keys.key_packages[0],
                &nonce1,
                message,
                &keys.public_key_package,
                2,
            )
            .await
            .unwrap();
        let share3 = third
            .frost_sign_share_for_message(
                &package,
                &keys.key_packages[2],
                &nonce3,
                message,
                &keys.public_key_package,
                2,
            )
            .await
            .unwrap();
        let shares = vec![share1, share3];
        let mut surplus = shares.clone();
        surplus.push(shares[0].clone());
        assert!(matches!(
            cause(
                &dealer
                    .frost_aggregate_signatures(&package, &surplus)
                    .await
                    .unwrap_err()
            ),
            PublicFrostSigningError::ShareInventory { .. }
        ));
        assert!(matches!(
            cause(
                &dealer
                    .frost_aggregate_signatures(&package, &shares[..1])
                    .await
                    .unwrap_err()
            ),
            PublicFrostSigningError::ShareInventory { .. }
        ));
        let mut substituted = package.clone();
        substituted.message = b"substituted aggregation intent".to_vec();
        assert!(matches!(
            cause(
                &dealer
                    .frost_aggregate_signatures(&substituted, &shares)
                    .await
                    .unwrap_err()
            ),
            PublicFrostSigningError::MessageMismatch
        ));
        let signature = dealer
            .frost_aggregate_signatures(&package, &shares)
            .await
            .unwrap();
        aura_core::crypto::signature_input::validate_signature_encoding(
            &keys.public_key_package,
            &signature,
            aura_core::crypto::single_signer::SigningMode::Threshold,
        )
        .unwrap();
        assert!(dealer
            .verify_signature(
                message,
                &signature,
                &keys.public_key_package,
                aura_core::crypto::single_signer::SigningMode::Threshold
            )
            .await
            .unwrap());
        assert!(!dealer
            .verify_signature(
                b"wrong unified message",
                &signature,
                &keys.public_key_package,
                aura_core::crypto::single_signer::SigningMode::Threshold
            )
            .await
            .unwrap());
        assert!(
            aura_core::crypto::signature_input::validate_signature_encoding(
                &[0xff],
                &signature,
                aura_core::crypto::single_signer::SigningMode::Threshold
            )
            .is_err()
        );
        assert!(
            aura_core::crypto::signature_input::validate_signature_encoding(
                &keys.public_key_package,
                &signature[..63],
                aura_core::crypto::single_signer::SigningMode::Threshold
            )
            .is_err()
        );
        let public = frost::keys::PublicKeyPackage::deserialize(&keys.public_key_package).unwrap();
        let signature = frost::Signature::deserialize(signature.try_into().unwrap()).unwrap();
        public.verifying_key().verify(message, &signature).unwrap();
        assert!(public
            .verifying_key()
            .verify(b"substituted intent", &signature)
            .is_err());
    }

    #[tokio::test]
    async fn public_signing_rejects_substituted_intent_policy_and_inventory() {
        let crypto = RealCryptoHandler::for_simulation_seed([0x94; 32]);
        let keys = crypto.frost_generate_keys(2, 3).await.unwrap();
        let n1 = crypto
            .frost_generate_nonces(&keys.key_packages[0])
            .await
            .unwrap();
        let n2 = crypto
            .frost_generate_nonces(&keys.key_packages[1])
            .await
            .unwrap();
        let c1 = public_commitment(1, &n1).unwrap();
        let c2 = public_commitment(2, &n2).unwrap();
        let message = b"admitted";
        let package = create_package(
            message,
            &[c1.clone(), c2.clone()],
            &keys.public_key_package,
            2,
        )
        .unwrap();
        assert!(matches!(
            cause(
                &create_package(
                    message,
                    std::slice::from_ref(&c1),
                    &keys.public_key_package,
                    2
                )
                .unwrap_err()
            ),
            PublicFrostSigningError::Quorum { .. }
        ));
        assert!(matches!(
            cause(
                &create_package(
                    message,
                    &[c1.clone(), c1.clone()],
                    &keys.public_key_package,
                    2
                )
                .unwrap_err()
            ),
            PublicFrostSigningError::DuplicateParticipant(1)
        ));
        let mut foreign = c2;
        foreign.participant_index = 4;
        assert!(matches!(
            cause(
                &create_package(message, &[c1, foreign], &keys.public_key_package, 2).unwrap_err()
            ),
            PublicFrostSigningError::UnknownParticipant(4)
        ));
        let sign = |p: &FrostSigningPackage, k: &[u8], threshold| {
            sign_for_message(p, k, &n1, message, &keys.public_key_package, threshold)
        };
        let mut substituted = package.clone();
        substituted.message = b"substituted".to_vec();
        assert!(matches!(
            cause(&sign(&substituted, &keys.key_packages[0], 2).unwrap_err()),
            PublicFrostSigningError::MessageMismatch
        ));
        let native = frost::SigningPackage::deserialize(&package.package).unwrap();
        substituted = package.clone();
        substituted.package =
            frost::SigningPackage::new(native.signing_commitments().clone(), b"substituted")
                .serialize()
                .unwrap();
        assert!(matches!(
            cause(&sign(&substituted, &keys.key_packages[0], 2).unwrap_err()),
            PublicFrostSigningError::MessageMismatch
        ));
        substituted = package.clone();
        substituted.public_key_package = vec![0];
        assert!(matches!(
            cause(&sign(&substituted, &keys.key_packages[0], 2).unwrap_err()),
            PublicFrostSigningError::PublicPackageMismatch
        ));
        substituted = package.clone();
        substituted.participants = vec![1, 3];
        assert!(matches!(
            cause(&sign(&substituted, &keys.key_packages[0], 2).unwrap_err()),
            PublicFrostSigningError::ParticipantInventoryMismatch
        ));
        assert!(matches!(
            cause(&sign(&package, &keys.key_packages[2], 2).unwrap_err()),
            PublicFrostSigningError::KeyPackageMismatch
        ));
        assert!(matches!(
            cause(&sign(&package, &keys.key_packages[0], 1).unwrap_err()),
            PublicFrostSigningError::KeyPackageMismatch
        ));
        let wrong_nonce = crypto
            .frost_generate_nonces(&keys.key_packages[0])
            .await
            .unwrap();
        assert!(matches!(
            cause(
                &sign_for_message(
                    &package,
                    &keys.key_packages[0],
                    &wrong_nonce,
                    message,
                    &keys.public_key_package,
                    2
                )
                .unwrap_err()
            ),
            PublicFrostSigningError::Native { .. }
        ));
    }
}
