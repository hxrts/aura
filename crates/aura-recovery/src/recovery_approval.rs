//! Canonical recovery approval signing transcripts.

use aura_core::effects::CryptoEffects;
use aura_core::key_resolution::TrustedKeyResolver;
use aura_core::types::identifiers::RecoveryId;
use aura_core::{hash::hash, AuraError, AuthorityId, Hash32, Result};
use aura_signature::{verify_ed25519_transcript, SecurityTranscript};
use serde::{Deserialize, Serialize};

/// Domain label for guardian recovery approval signatures.
pub const RECOVERY_APPROVAL_DOMAIN: &str = "aura.recovery.guardian-approval";

/// Canonical guardian approval transcript payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryApprovalTranscriptPayload {
    /// Recovery ceremony ID being approved.
    pub recovery_id: RecoveryId,
    /// Account authority being recovered.
    pub account_authority: AuthorityId,
    /// Stable hash of the requested recovery operation.
    pub operation_hash: Hash32,
    /// Prestate hash bound by the recovery request.
    pub prestate_hash: Hash32,
    /// Guardian approval value.
    pub approved: bool,
    /// Approval timestamp in milliseconds.
    pub approved_at_ms: u64,
    /// Guardian authority signing the approval.
    pub guardian_id: AuthorityId,
}

/// Typed recovery approval transcript.
#[derive(Debug, Clone)]
pub struct RecoveryApprovalTranscript {
    payload: RecoveryApprovalTranscriptPayload,
}

impl RecoveryApprovalTranscript {
    /// Build a canonical recovery approval transcript.
    pub fn new(payload: RecoveryApprovalTranscriptPayload) -> Self {
        Self { payload }
    }

    /// Access the payload covered by the signature.
    pub fn payload(&self) -> &RecoveryApprovalTranscriptPayload {
        &self.payload
    }
}

impl SecurityTranscript for RecoveryApprovalTranscript {
    type Payload = RecoveryApprovalTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = RECOVERY_APPROVAL_DOMAIN;

    fn transcript_payload(&self) -> Self::Payload {
        self.payload.clone()
    }
}

/// Hash a recovery operation into the approval transcript binding.
pub fn recovery_operation_hash<T: Serialize>(operation: &T) -> Result<Hash32> {
    let bytes = serde_json::to_vec(operation)
        .map_err(|error| AuraError::serialization(format!("encode recovery operation: {error}")))?;
    Ok(Hash32(hash(&bytes)))
}

/// Verify a guardian recovery approval against the trusted guardian key registry.
pub async fn verify_recovery_approval_signature<E, R>(
    crypto: &E,
    payload: RecoveryApprovalTranscriptPayload,
    signature: &[u8],
    key_resolver: &R,
) -> Result<bool>
where
    E: CryptoEffects + Send + Sync + ?Sized,
    R: TrustedKeyResolver + ?Sized,
{
    if signature.len() != 64 || signature.iter().all(|byte| *byte == 0) {
        return Ok(false);
    }

    let trusted_key = key_resolver
        .resolve_guardian_key(payload.guardian_id)
        .map_err(|error| {
            AuraError::crypto(format!(
                "trusted recovery approval key resolution failed for {}: {error}",
                payload.guardian_id
            ))
        })?;
    let transcript = RecoveryApprovalTranscript::new(payload);
    verify_ed25519_transcript(crypto, &transcript, signature, trusted_key.bytes())
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "recovery approval signature verification failed: {error}"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::CryptoCoreEffects;
    use aura_core::key_resolution::{KeyResolutionError, TrustedKeyDomain, TrustedPublicKey};
    use aura_effects::crypto::RealCryptoHandler;
    use aura_signature::sign_ed25519_transcript;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct TestGuardianKeyResolver {
        keys: BTreeMap<AuthorityId, Vec<u8>>,
    }

    impl TestGuardianKeyResolver {
        fn with_guardian_key(mut self, guardian: AuthorityId, key: Vec<u8>) -> Self {
            self.keys.insert(guardian, key);
            self
        }
    }

    impl TrustedKeyResolver for TestGuardianKeyResolver {
        fn resolve_authority_threshold_key(
            &self,
            _authority: AuthorityId,
            _epoch: u64,
        ) -> std::result::Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::AuthorityThreshold,
            })
        }

        fn resolve_device_key(
            &self,
            _device: aura_core::DeviceId,
        ) -> std::result::Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Device,
            })
        }

        fn resolve_guardian_key(
            &self,
            guardian: AuthorityId,
        ) -> std::result::Result<TrustedPublicKey, KeyResolutionError> {
            let key = self
                .keys
                .get(&guardian)
                .ok_or(KeyResolutionError::Unknown {
                    domain: TrustedKeyDomain::Guardian,
                })?;
            Ok(TrustedPublicKey::active(
                TrustedKeyDomain::Guardian,
                None,
                key.clone(),
                Hash32(hash(key)),
            ))
        }

        fn resolve_release_key(
            &self,
            _authority: AuthorityId,
        ) -> std::result::Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Release,
            })
        }
    }

    fn test_payload(guardian_id: AuthorityId) -> RecoveryApprovalTranscriptPayload {
        RecoveryApprovalTranscriptPayload {
            recovery_id: RecoveryId::new("recovery-test"),
            account_authority: AuthorityId::new_from_entropy([1; 32]),
            operation_hash: Hash32([2; 32]),
            prestate_hash: Hash32([3; 32]),
            approved: true,
            approved_at_ms: 123,
            guardian_id,
        }
    }

    #[tokio::test]
    async fn recovery_approval_signature_verifies_with_trusted_guardian_key() {
        let crypto = RealCryptoHandler::for_simulation_seed([0xA9; 32]);
        let (private_key, public_key) = crypto.ed25519_generate_keypair().await.unwrap();
        let guardian_id = AuthorityId::new_from_entropy([9; 32]);
        let payload = test_payload(guardian_id);
        let transcript = RecoveryApprovalTranscript::new(payload.clone());
        let signature = sign_ed25519_transcript(&crypto, &transcript, &private_key)
            .await
            .unwrap();
        let key_resolver =
            TestGuardianKeyResolver::default().with_guardian_key(guardian_id, public_key);

        assert!(
            verify_recovery_approval_signature(&crypto, payload, &signature, &key_resolver)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn recovery_approval_signature_rejects_wrong_prestate_and_zero_signature() {
        let crypto = RealCryptoHandler::for_simulation_seed([0xB9; 32]);
        let (private_key, public_key) = crypto.ed25519_generate_keypair().await.unwrap();
        let guardian_id = AuthorityId::new_from_entropy([10; 32]);
        let payload = test_payload(guardian_id);
        let transcript = RecoveryApprovalTranscript::new(payload.clone());
        let signature = sign_ed25519_transcript(&crypto, &transcript, &private_key)
            .await
            .unwrap();
        let key_resolver =
            TestGuardianKeyResolver::default().with_guardian_key(guardian_id, public_key);

        let mut wrong_prestate = payload.clone();
        wrong_prestate.prestate_hash = Hash32([4; 32]);
        assert!(!verify_recovery_approval_signature(
            &crypto,
            wrong_prestate,
            &signature,
            &key_resolver
        )
        .await
        .unwrap());
        assert!(
            !verify_recovery_approval_signature(&crypto, payload, &[0; 64], &key_resolver)
                .await
                .unwrap()
        );
    }
}
