//! Individual key possession proofs. These are never group quorum signatures.
//! Runtime authorization must bind the verifier to the current exact device
//! participant inventory before accepting this pure cryptographic evidence.
use crate::crypto::single_signer::{SigningMode, SingleSignerKeyPackage};
use crate::secrets::SecretExportContext;
use crate::AuraError;
use rand::{CryptoRng, RngCore};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
#[error("individual participant-key proof signing is unsupported by this handler")]
pub struct ParticipantKeyProofUnsupported;

pub fn sign_participant_key_proof(
    message: &[u8],
    key_package: &[u8],
    mode: SigningMode,
    rng: &mut (impl RngCore + CryptoRng),
) -> Result<Vec<u8>, AuraError> {
    if message.len() > crate::effects::crypto::MAX_SIGNING_MESSAGE_BYTES
        || key_package.len() > crate::effects::crypto::MAX_KEY_PACKAGE_BYTES
    {
        return Err(AuraError::invalid(
            "participant proof exceeds crypto bounds",
        ));
    }
    match mode {
        SigningMode::SingleSigner => {
            let package = SingleSignerKeyPackage::import_from_secure_storage(
                key_package,
                SecretExportContext::secure_storage("participant_key_proof"),
            )?;
            let seed =
                zeroize::Zeroizing::new(<[u8; 32]>::try_from(package.signing_key()).map_err(
                    |_| AuraError::invalid("participant signing seed has invalid length"),
                )?);
            let key = crate::crypto::Ed25519SigningKey::from_bytes(*seed);
            if key.verifying_key()?.to_bytes().as_slice() != package.verifying_key() {
                return Err(AuraError::crypto(
                    "participant seed does not match exact verifying key",
                ));
            }
            Ok(key.sign(message)?.to_bytes().to_vec())
        }
        SigningMode::Threshold => {
            let package =
                frost_ed25519::keys::KeyPackage::deserialize(key_package).map_err(|source| {
                    AuraError::crypto_with_source(
                        "decode actual participant key package",
                        Arc::new(source),
                    )
                })?;
            // Canonical FROST field encoding. Never interpret this scalar as an
            // Ed25519 seed, clamp it, or construct custom nonce/challenge bytes.
            let key = frost_ed25519::SigningKey::deserialize(package.signing_share().serialize())
                .map_err(|source| {
                AuraError::crypto_with_source("decode current participant scalar", Arc::new(source))
            })?;
            if frost_ed25519::VerifyingKey::from(key).serialize()
                != package.verifying_share().serialize()
            {
                return Err(AuraError::crypto(
                    "participant share does not match actual verifying share",
                ));
            }
            Ok(key.sign(rng, message).serialize().to_vec())
        }
    }
}

pub fn verify_participant_key_proof(
    message: &[u8],
    signature: &[u8],
    exact_participant_public_key: &[u8],
    mode: SigningMode,
) -> Result<(), AuraError> {
    if message.len() > crate::effects::crypto::MAX_SIGNING_MESSAGE_BYTES
        || signature.len() != 64
        || exact_participant_public_key.len() != 32
    {
        return Err(AuraError::invalid(
            "participant proof has invalid crypto bounds",
        ));
    }
    match mode {
        SigningMode::SingleSigner => {
            let key_bytes: [u8; 32] = exact_participant_public_key
                .try_into()
                .map_err(|_| AuraError::invalid("participant verifier length"))?;
            let sig_bytes: [u8; 64] = signature
                .try_into()
                .map_err(|_| AuraError::invalid("participant proof length"))?;
            let key = ed25519_dalek::VerifyingKey::from_bytes(&key_bytes).map_err(|source| {
                AuraError::crypto_with_source("decode exact participant verifier", Arc::new(source))
            })?;
            key.verify_strict(message, &ed25519_dalek::Signature::from_bytes(&sig_bytes))
                .map_err(|source| {
                    AuraError::crypto_with_source("participant proof rejected", Arc::new(source))
                })
        }
        SigningMode::Threshold => {
            let key_bytes: [u8; 32] = exact_participant_public_key
                .try_into()
                .map_err(|_| AuraError::invalid("participant verifier length"))?;
            let sig_bytes: [u8; 64] = signature
                .try_into()
                .map_err(|_| AuraError::invalid("participant proof length"))?;
            let key = frost_ed25519::VerifyingKey::deserialize(key_bytes).map_err(|source| {
                AuraError::crypto_with_source(
                    "decode exact current participant verifier",
                    Arc::new(source),
                )
            })?;
            let proof = frost_ed25519::Signature::deserialize(sig_bytes).map_err(|source| {
                AuraError::crypto_with_source("decode participant proof", Arc::new(source))
            })?;
            key.verify(message, &proof).map_err(|source| {
                AuraError::crypto_with_source(
                    "participant key possession proof rejected",
                    Arc::new(source),
                )
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn genuine_threshold_share_proof_binds_individual_not_group(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut rng = rand::rngs::StdRng::from_seed([71; 32]);
        let (shares, public) = frost_ed25519::keys::generate_with_dealer(
            3,
            2,
            frost_ed25519::keys::IdentifierList::Default,
            &mut rng,
        )?;
        let (_, share) = shares.iter().next().ok_or("dealer produced no shares")?;
        let key = frost_ed25519::keys::KeyPackage::try_from(share.clone())?;
        let bytes = key.serialize()?;
        let actual_verifier = key.verifying_share().serialize();
        let message = b"aura.sync.device-epoch.participant-acceptance.v2 exact binding";
        let proof = sign_participant_key_proof(message, &bytes, SigningMode::Threshold, &mut rng)?;
        verify_participant_key_proof(message, &proof, &actual_verifier, SigningMode::Threshold)?;
        assert!(verify_participant_key_proof(
            b"other ceremony",
            &proof,
            &actual_verifier,
            SigningMode::Threshold
        )
        .is_err());
        assert!(verify_participant_key_proof(
            message,
            &proof,
            &public.verifying_key().serialize(),
            SigningMode::Threshold
        )
        .is_err());
        let other = public
            .verifying_shares()
            .iter()
            .find(|(id, _)| **id != *key.identifier())
            .ok_or("dealer produced no second verifier")?
            .1
            .serialize();
        assert!(
            verify_participant_key_proof(message, &proof, &other, SigningMode::Threshold).is_err()
        );
        let mut altered = proof;
        altered[12] ^= 1;
        assert!(verify_participant_key_proof(
            message,
            &altered,
            &actual_verifier,
            SigningMode::Threshold
        )
        .is_err());
        Ok(())
    }
}
