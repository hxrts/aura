//! Signed ceremony admission bindings. Signature integrity alone cannot mint
//! historical signing authority or authenticate a parent verifier inventory.

use crate::enrollment_setup::DeviceEnrollmentSetupBinding;
use aura_core::crypto::single_signer::SigningMode;
use aura_core::effects::CryptoExtendedEffects;
use aura_core::hash::hash;
use aura_core::threshold::{SigningContext, ThresholdSignature};
use aura_core::{AuthorityId, CeremonyId, DeviceId, InvitationId};
use aura_signature::{threshold_signing_context_transcript_bytes, SecurityTranscript};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentSetupAdmissionStatement {
    pub version: u16,
    pub setup: DeviceEnrollmentSetupBinding,
    pub subject: AuthorityId,
    pub initiator_device: DeviceId,
    pub invitee_authority: AuthorityId,
    pub invitee_device: DeviceId,
    pub ceremony: CeremonyId,
    pub invitation: InvitationId,
    pub manifest_digest: [u8; 32],
    pub parent_verifier_inventory_digest: [u8; 32],
    pub prestate_epoch: u64,
    pub prestate_commitment: [u8; 32],
    pub pending_epoch: u64,
    pub response_expires_at_ms: u64,
}

impl SecurityTranscript for EnrollmentSetupAdmissionStatement {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.enrollment-setup-admission.v1";
    fn transcript_payload(&self) -> Self::Payload {
        self.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentSetupAdmission {
    pub statement: EnrollmentSetupAdmissionStatement,
    pub proof: ThresholdSignature,
}

/// Pure expected verifier shape. Its construction does not establish trust;
/// runtime owners obtain the values from the independently pinned manifest's
/// validated exact-parent inventory, never the receipt's embedded proof.
pub struct EnrollmentAdmissionParentVerifier<'a> {
    pub epoch: u64,
    pub mode: SigningMode,
    pub threshold: u16,
    pub participants: u16,
    pub public_key_package: &'a [u8],
    pub inventory_digest: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum EnrollmentAdmissionError {
    #[error("unsupported or invalid enrollment admission statement")]
    Statement,
    #[error("enrollment admission parent verifier binding does not match")]
    ParentBinding,
    #[error("enrollment admission response deadline has passed")]
    Expired,
    #[error("enrollment admission signature is invalid")]
    InvalidSignature,
    #[error("enrollment admission transcript failed")]
    Transcript(#[from] aura_signature::AuthenticationError),
    #[error("enrollment admission cryptography failed")]
    Crypto(#[source] aura_core::AuraError),
    #[error("enrollment admission codec failed")]
    Codec(#[source] aura_core::util::serialization::SerializationError),
}

/// Sealed evidence of signature verification against the supplied verifier.
/// This is deliberately not authenticated manifest/admission provenance and
/// cannot alone authorize a historical response signer.
#[derive(Debug, Clone)]
pub struct VerifiedEnrollmentAdmissionSignature {
    statement: EnrollmentSetupAdmissionStatement,
    digest: [u8; 32],
}

impl VerifiedEnrollmentAdmissionSignature {
    pub fn statement(&self) -> &EnrollmentSetupAdmissionStatement {
        &self.statement
    }
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

impl EnrollmentSetupAdmission {
    pub const VERSION: u16 = 1;
    pub const MAX_BYTES: usize = 131_072;
    pub const MAX_PARTICIPANTS: u16 = 1024;

    pub fn signing_context(
        statement: &EnrollmentSetupAdmissionStatement,
    ) -> Result<SigningContext, EnrollmentAdmissionError> {
        Ok(SigningContext::message(
            statement.subject,
            EnrollmentSetupAdmissionStatement::DOMAIN_SEPARATOR.to_string(),
            statement.transcript_bytes()?,
        ))
    }

    pub fn encode(&self) -> Result<Vec<u8>, EnrollmentAdmissionError> {
        let bytes = aura_core::util::serialization::to_vec(self)
            .map_err(EnrollmentAdmissionError::Codec)?;
        if bytes.len() > Self::MAX_BYTES {
            return Err(EnrollmentAdmissionError::Statement);
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, EnrollmentAdmissionError> {
        if bytes.len() > Self::MAX_BYTES {
            return Err(EnrollmentAdmissionError::Statement);
        }
        aura_core::util::serialization::from_slice(bytes).map_err(EnrollmentAdmissionError::Codec)
    }

    pub async fn verify_signature<E: CryptoExtendedEffects + ?Sized>(
        self,
        crypto: &E,
        expected: EnrollmentAdmissionParentVerifier<'_>,
        now_ms: u64,
    ) -> Result<VerifiedEnrollmentAdmissionSignature, EnrollmentAdmissionError> {
        // Apply wire/resource bounds even to directly constructed values.
        self.encode()?;
        if self.statement.invitation.to_string().is_empty()
            || self.statement.invitation.to_string().len()
                > crate::shareable::ShareableInvitation::MAX_INVITATION_ID_BYTES
            || self.statement.ceremony.to_string().is_empty()
            || self.statement.ceremony.to_string().len()
                > crate::shareable::ShareableInvitation::MAX_CEREMONY_ID_BYTES
        {
            return Err(EnrollmentAdmissionError::Statement);
        }
        if self.statement.version != Self::VERSION
            || self.statement.pending_epoch <= self.statement.prestate_epoch
        {
            return Err(EnrollmentAdmissionError::Statement);
        }
        if now_ms >= self.statement.response_expires_at_ms {
            return Err(EnrollmentAdmissionError::Expired);
        }
        if expected.threshold == 0 || expected.threshold > expected.participants
            || expected.participants > Self::MAX_PARTICIPANTS
            || expected.public_key_package.is_empty()
            || expected.public_key_package.len() > crate::enrollment_setup::DeviceEnrollmentSetupRequest::MAX_PUBLIC_KEY_PACKAGE_BYTES
            || self.proof.epoch != expected.epoch || self.proof.public_key_package != expected.public_key_package
            || self.statement.prestate_epoch != expected.epoch
            || self.statement.parent_verifier_inventory_digest != expected.inventory_digest
            || self.proof.signature.is_empty()
            || self.proof.signature.len() > crate::enrollment_setup::DeviceEnrollmentSetupRequest::MAX_SIGNATURE_BYTES
            || self.proof.signer_count < expected.threshold || self.proof.signer_count > expected.participants
            || self.proof.signers.len() != usize::from(self.proof.signer_count)
            || self.proof.signers.iter().any(|i| *i == 0 || *i > expected.participants)
            || self.proof.signers.windows(2).any(|w| w[0] >= w[1])
            || match expected.mode {
                SigningMode::SingleSigner => expected.threshold != 1 || expected.participants != 1 || self.proof.signer_count != 1 || self.proof.signers != [1],
                SigningMode::Threshold => expected.threshold < 2 || self.proof.signer_count < 2,
            }
        { return Err(EnrollmentAdmissionError::ParentBinding); }
        let context = Self::signing_context(&self.statement)?;
        let bytes = threshold_signing_context_transcript_bytes(&context, expected.epoch)?;
        if !crypto
            .verify_signature(
                &bytes,
                &self.proof.signature,
                expected.public_key_package,
                expected.mode,
            )
            .await
            .map_err(EnrollmentAdmissionError::Crypto)?
        {
            return Err(EnrollmentAdmissionError::InvalidSignature);
        }
        let digest = hash(&self.statement.transcript_bytes()?);
        Ok(VerifiedEnrollmentAdmissionSignature {
            statement: self.statement,
            digest,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_effects::crypto::RealCryptoHandler;

    #[tokio::test]
    async fn admission_signature_binds_setup_manifest_parent_and_ceremony() {
        let crypto = RealCryptoHandler::for_simulation_seed([119; 32]);
        let keys = crypto.generate_signing_keys(1, 1).await.unwrap();
        let statement = EnrollmentSetupAdmissionStatement {
            version: EnrollmentSetupAdmission::VERSION,
            setup: DeviceEnrollmentSetupBinding {
                nonce: [120; 32],
                digest: [121; 32],
            },
            subject: AuthorityId::new_from_entropy([122; 32]),
            initiator_device: DeviceId::new_from_entropy([123; 32]),
            invitee_authority: AuthorityId::new_from_entropy([124; 32]),
            invitee_device: DeviceId::new_from_entropy([125; 32]),
            ceremony: CeremonyId::new("signed admission"),
            invitation: InvitationId::new("admitted invitation"),
            manifest_digest: [126; 32],
            parent_verifier_inventory_digest: [127; 32],
            prestate_epoch: 3,
            prestate_commitment: [128; 32],
            pending_epoch: 4,
            response_expires_at_ms: 200,
        };
        let context = EnrollmentSetupAdmission::signing_context(&statement).unwrap();
        let bytes = threshold_signing_context_transcript_bytes(&context, 3).unwrap();
        let proof = ThresholdSignature::single_signer(
            crypto
                .sign_with_key(&bytes, &keys.key_packages[0], keys.mode)
                .await
                .unwrap(),
            keys.public_key_package.clone(),
            3,
        );
        let receipt = EnrollmentSetupAdmission { statement, proof };
        let expected = || EnrollmentAdmissionParentVerifier {
            epoch: 3,
            mode: SigningMode::SingleSigner,
            threshold: 1,
            participants: 1,
            public_key_package: &keys.public_key_package,
            inventory_digest: [127; 32],
        };
        let decoded = EnrollmentSetupAdmission::decode(&receipt.encode().unwrap()).unwrap();
        let verified = decoded
            .verify_signature(&crypto, expected(), 150)
            .await
            .unwrap();
        assert_eq!(verified.statement(), &receipt.statement);
        let mut candidates = Vec::new();
        let mut changed = receipt.clone();
        changed.statement.setup.nonce[0] ^= 1;
        candidates.push(changed);
        let mut changed = receipt.clone();
        changed.statement.setup.digest[0] ^= 1;
        candidates.push(changed);
        let mut changed = receipt.clone();
        changed.statement.manifest_digest[0] ^= 1;
        candidates.push(changed);
        let mut changed = receipt.clone();
        changed.statement.prestate_commitment[0] ^= 1;
        candidates.push(changed);
        let mut changed = receipt.clone();
        changed.statement.invitation = InvitationId::new("other invitation");
        candidates.push(changed);
        let mut changed = receipt.clone();
        changed.statement.ceremony = CeremonyId::new("other ceremony");
        candidates.push(changed);
        let mut changed = receipt.clone();
        changed.proof.public_key_package[0] ^= 1;
        candidates.push(changed);
        let mut changed = receipt.clone();
        changed.proof.signature[0] ^= 1;
        candidates.push(changed);
        for changed in candidates {
            assert!(changed
                .verify_signature(&crypto, expected(), 150)
                .await
                .is_err());
        }
        assert!(matches!(
            receipt.verify_signature(&crypto, expected(), 200).await,
            Err(EnrollmentAdmissionError::Expired)
        ));
        // These tests prove signature binding, not independent initiator pinning
        // or manifest parent-inventory provenance.
    }
}
