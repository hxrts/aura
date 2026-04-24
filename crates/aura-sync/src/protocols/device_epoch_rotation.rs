//! Device-scoped epoch rotation choreography surface.
//!
//! This protocol covers the device-specific share distribution / acceptance /
//! commit handshake used by enrollment and removal ceremonies once the local
//! initiator has already prepared the pending epoch.

use aura_core::crypto::{single_signer::SingleSignerPublicKeyPackage, tree_signing};
use aura_core::effects::CryptoEffects;
use aura_core::threshold::{SigningContext, ThresholdSignature};
use aura_core::types::identifiers::CeremonyId;
use aura_core::{
    AttestedOp, AuraError, AuthorityId, DeviceId, Hash32, TrustedKeyDomain, TrustedPublicKey,
};
use aura_macros::tell;
use aura_signature::{
    verify_ed25519_threshold_signing_context_transcript,
    verify_threshold_signing_context_transcript, SecurityTranscript,
};
use curve25519_dalek::{montgomery::MontgomeryPoint, scalar::Scalar};
use serde::{Deserialize, Serialize};

const DEVICE_EPOCH_KEY_PACKAGE_ENCRYPTION_PROTOCOL_VERSION: u8 = 1;
const DEVICE_EPOCH_KEY_PACKAGE_KDF_DOMAIN: &[u8] = b"aura.sync.device-epoch.key-package.v1";

/// The initiating ceremony type for one device-scoped epoch rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceEpochRotationKind {
    Rotation,
    Enrollment,
    Removal,
}

/// Proposal sent from the initiating device to one participant device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEpochProposal {
    pub ceremony_id: CeremonyId,
    pub kind: DeviceEpochRotationKind,
    pub subject_authority: AuthorityId,
    pub pending_epoch: u64,
    pub initiator_device_id: DeviceId,
    pub participant_device_id: DeviceId,
    pub key_package_hash: Hash32,
    pub threshold_config_hash: Hash32,
    pub public_key_package_hash: Hash32,
    pub proposed_at_ms: u64,
    pub authority_signature: ThresholdSignature,
    pub encrypted_key_package: EncryptedDeviceEpochKeyPackage,
    // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md choreography wire payload; producer must wrap/export with key-wrapping context before serialization.
    pub threshold_config: Vec<u8>,
    /// Untrusted key material: pending-epoch ceremony payload; authentication must resolve expected keys from trusted authority/device state.
    pub public_key_package: Vec<u8>,
}

/// Encrypted pending-epoch key package for the intended participant device.
///
/// Untrusted key material: remote device-epoch package payload; device identity,
/// recipient key, and ephemeral key bytes must be authenticated against trusted
/// ceremony state before use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedDeviceEpochKeyPackage {
    pub protocol_version: u8,
    pub recipient_device_id: DeviceId,
    /// Untrusted key material: remote recipient device key bytes; authenticate
    /// against trusted device state before using this package.
    pub recipient_public_key: Vec<u8>,
    /// Untrusted key material: remote ephemeral sender key bytes; bind to the
    /// authenticated package transcript before deriving shared secrets.
    pub ephemeral_public_key: Vec<u8>,
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
    pub binding_hash: Hash32,
}

/// Acceptance issued by one participant device after locally staging the share.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEpochAcceptance {
    pub ceremony_id: CeremonyId,
    pub acceptor_device_id: DeviceId,
    pub proposal_hash: Hash32,
    pub accepted_at_ms: u64,
    pub signature: Vec<u8>,
}

/// Commit sent by the initiator once the ceremony threshold is satisfied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEpochCommit {
    pub ceremony_id: CeremonyId,
    pub new_epoch: u64,
    pub proposal_hash: Hash32,
    pub committed_at_ms: u64,
    pub attested_leaf_op_hash: Option<Hash32>,
    pub authority_signature: ThresholdSignature,
    pub attested_leaf_op: Option<AttestedOp>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceEpochProposalTranscriptPayload {
    pub ceremony_id: CeremonyId,
    pub kind: DeviceEpochRotationKind,
    pub subject_authority: AuthorityId,
    pub pending_epoch: u64,
    pub initiator_device_id: DeviceId,
    pub participant_device_id: DeviceId,
    pub key_package_hash: Hash32,
    pub threshold_config_hash: Hash32,
    pub public_key_package_hash: Hash32,
    pub encrypted_key_package_binding_hash: Hash32,
    pub proposed_at_ms: u64,
}

pub struct DeviceEpochProposalTranscript<'a> {
    proposal: &'a DeviceEpochProposal,
}

impl<'a> DeviceEpochProposalTranscript<'a> {
    #[must_use]
    pub fn new(proposal: &'a DeviceEpochProposal) -> Self {
        Self { proposal }
    }
}

impl SecurityTranscript for DeviceEpochProposalTranscript<'_> {
    type Payload = DeviceEpochProposalTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.sync.device-epoch.proposal";

    fn transcript_payload(&self) -> Self::Payload {
        DeviceEpochProposalTranscriptPayload {
            ceremony_id: self.proposal.ceremony_id.clone(),
            kind: self.proposal.kind,
            subject_authority: self.proposal.subject_authority,
            pending_epoch: self.proposal.pending_epoch,
            initiator_device_id: self.proposal.initiator_device_id,
            participant_device_id: self.proposal.participant_device_id,
            key_package_hash: self.proposal.key_package_hash,
            threshold_config_hash: self.proposal.threshold_config_hash,
            public_key_package_hash: self.proposal.public_key_package_hash,
            encrypted_key_package_binding_hash: self.proposal.encrypted_key_package.binding_hash,
            proposed_at_ms: self.proposal.proposed_at_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceEpochAcceptanceTranscriptPayload {
    pub ceremony_id: CeremonyId,
    pub acceptor_device_id: DeviceId,
    pub proposal_hash: Hash32,
    pub accepted_at_ms: u64,
}

pub struct DeviceEpochAcceptanceTranscript<'a> {
    acceptance: &'a DeviceEpochAcceptance,
}

impl<'a> DeviceEpochAcceptanceTranscript<'a> {
    #[must_use]
    pub fn new(acceptance: &'a DeviceEpochAcceptance) -> Self {
        Self { acceptance }
    }
}

impl SecurityTranscript for DeviceEpochAcceptanceTranscript<'_> {
    type Payload = DeviceEpochAcceptanceTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.sync.device-epoch.acceptance";

    fn transcript_payload(&self) -> Self::Payload {
        DeviceEpochAcceptanceTranscriptPayload {
            ceremony_id: self.acceptance.ceremony_id.clone(),
            acceptor_device_id: self.acceptance.acceptor_device_id,
            proposal_hash: self.acceptance.proposal_hash,
            accepted_at_ms: self.acceptance.accepted_at_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceEpochCommitTranscriptPayload {
    pub ceremony_id: CeremonyId,
    pub new_epoch: u64,
    pub proposal_hash: Hash32,
    pub committed_at_ms: u64,
    pub attested_leaf_op_hash: Option<Hash32>,
}

pub struct DeviceEpochCommitTranscript<'a> {
    commit: &'a DeviceEpochCommit,
}

impl<'a> DeviceEpochCommitTranscript<'a> {
    #[must_use]
    pub fn new(commit: &'a DeviceEpochCommit) -> Self {
        Self { commit }
    }
}

impl SecurityTranscript for DeviceEpochCommitTranscript<'_> {
    type Payload = DeviceEpochCommitTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.sync.device-epoch.commit";

    fn transcript_payload(&self) -> Self::Payload {
        DeviceEpochCommitTranscriptPayload {
            ceremony_id: self.commit.ceremony_id.clone(),
            new_epoch: self.commit.new_epoch,
            proposal_hash: self.commit.proposal_hash,
            committed_at_ms: self.commit.committed_at_ms,
            attested_leaf_op_hash: self.commit.attested_leaf_op_hash,
        }
    }
}

pub fn verify_device_epoch_proposal_hashes(proposal: &DeviceEpochProposal) -> bool {
    let encrypted = &proposal.encrypted_key_package;
    proposal.threshold_config_hash == Hash32::from_bytes(&proposal.threshold_config)
        && proposal.public_key_package_hash == Hash32::from_bytes(&proposal.public_key_package)
        && encrypted.protocol_version == DEVICE_EPOCH_KEY_PACKAGE_ENCRYPTION_PROTOCOL_VERSION
        && encrypted.recipient_device_id == proposal.participant_device_id
        && encrypted.recipient_public_key.len() == 32
        && encrypted.ephemeral_public_key.len() == 32
        && device_epoch_key_package_binding_hash(
            proposal,
            &encrypted.recipient_public_key,
            &encrypted.ephemeral_public_key,
            &encrypted.ciphertext,
            encrypted.nonce,
        )
        .is_ok_and(|binding_hash| binding_hash == encrypted.binding_hash)
}

fn to_x25519_scalar(private_key: &[u8; 32]) -> Scalar {
    Scalar::from_bytes_mod_order(*private_key)
}

fn x25519_shared_secret(private_key: &[u8; 32], public_key: &[u8; 32]) -> [u8; 32] {
    let scalar = to_x25519_scalar(private_key);
    let point = MontgomeryPoint(*public_key);
    (scalar * point).to_bytes()
}

#[derive(Serialize)]
struct DeviceEpochKeyAgreementTranscript<'a> {
    protocol_version: u8,
    ceremony_id: &'a CeremonyId,
    subject_authority: AuthorityId,
    pending_epoch: u64,
    initiator_device_id: DeviceId,
    participant_device_id: DeviceId,
    recipient_public_key: &'a [u8],
    ephemeral_public_key: &'a [u8],
    key_package_hash: Hash32,
}

#[derive(Serialize)]
struct DeviceEpochKeyPackageBindingTranscript<'a> {
    protocol_version: u8,
    ceremony_id: &'a CeremonyId,
    subject_authority: AuthorityId,
    pending_epoch: u64,
    initiator_device_id: DeviceId,
    participant_device_id: DeviceId,
    recipient_public_key: &'a [u8],
    ephemeral_public_key: &'a [u8],
    key_package_hash: Hash32,
    ciphertext_hash: Hash32,
    nonce: [u8; 12],
}

fn device_epoch_key_agreement_transcript(
    proposal: &DeviceEpochProposal,
    recipient_public_key: &[u8],
    ephemeral_public_key: &[u8],
) -> Result<Vec<u8>, AuraError> {
    aura_core::util::serialization::to_vec(&DeviceEpochKeyAgreementTranscript {
        protocol_version: DEVICE_EPOCH_KEY_PACKAGE_ENCRYPTION_PROTOCOL_VERSION,
        ceremony_id: &proposal.ceremony_id,
        subject_authority: proposal.subject_authority,
        pending_epoch: proposal.pending_epoch,
        initiator_device_id: proposal.initiator_device_id,
        participant_device_id: proposal.participant_device_id,
        recipient_public_key,
        ephemeral_public_key,
        key_package_hash: proposal.key_package_hash,
    })
    .map_err(|error| {
        AuraError::crypto(format!(
            "device epoch key-agreement transcript encode failed: {error}"
        ))
    })
}

fn device_epoch_key_package_binding_hash(
    proposal: &DeviceEpochProposal,
    recipient_public_key: &[u8],
    ephemeral_public_key: &[u8],
    ciphertext: &[u8],
    nonce: [u8; 12],
) -> Result<Hash32, AuraError> {
    Hash32::from_value(&DeviceEpochKeyPackageBindingTranscript {
        protocol_version: DEVICE_EPOCH_KEY_PACKAGE_ENCRYPTION_PROTOCOL_VERSION,
        ceremony_id: &proposal.ceremony_id,
        subject_authority: proposal.subject_authority,
        pending_epoch: proposal.pending_epoch,
        initiator_device_id: proposal.initiator_device_id,
        participant_device_id: proposal.participant_device_id,
        recipient_public_key,
        ephemeral_public_key,
        key_package_hash: proposal.key_package_hash,
        ciphertext_hash: Hash32::from_bytes(ciphertext),
        nonce,
    })
    .map_err(|error| {
        AuraError::crypto(format!(
            "device epoch key-package binding hash failed: {error}"
        ))
    })
}

pub async fn encrypt_device_epoch_key_package<E>(
    effects: &E,
    proposal: &DeviceEpochProposal,
    recipient_public_key: &[u8],
    key_package: &[u8],
) -> Result<EncryptedDeviceEpochKeyPackage, AuraError>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    let recipient_x25519_public = effects
        .convert_ed25519_to_x25519_public(recipient_public_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "device epoch recipient key conversion failed: {error}"
            ))
        })?;
    let (ephemeral_private_key, ephemeral_ed25519_public_key) =
        effects.ed25519_generate_keypair().await.map_err(|error| {
            AuraError::crypto(format!(
                "device epoch ephemeral key generation failed: {error}"
            ))
        })?;
    let ephemeral_x25519_private = effects
        .convert_ed25519_to_x25519_private(&ephemeral_private_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "device epoch ephemeral private-key conversion failed: {error}"
            ))
        })?;
    let ephemeral_x25519_public = effects
        .convert_ed25519_to_x25519_public(&ephemeral_ed25519_public_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "device epoch ephemeral public-key conversion failed: {error}"
            ))
        })?;
    let shared_secret = x25519_shared_secret(&ephemeral_x25519_private, &recipient_x25519_public);
    let kdf_info = device_epoch_key_agreement_transcript(
        proposal,
        recipient_public_key,
        &ephemeral_x25519_public,
    )?;
    let encryption_key = effects
        .kdf_derive(
            &shared_secret,
            DEVICE_EPOCH_KEY_PACKAGE_KDF_DOMAIN,
            &kdf_info,
            32,
        )
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "device epoch encryption key derivation failed: {error}"
            ))
        })?;
    let nonce_bytes = effects.random_bytes(12).await;
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&nonce_bytes);
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&encryption_key);
    let ciphertext = effects
        .chacha20_encrypt(key_package, &key_array, &nonce)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "device epoch key-package encryption failed: {error}"
            ))
        })?;
    let binding_hash = device_epoch_key_package_binding_hash(
        proposal,
        recipient_public_key,
        &ephemeral_x25519_public,
        &ciphertext,
        nonce,
    )?;
    Ok(EncryptedDeviceEpochKeyPackage {
        protocol_version: DEVICE_EPOCH_KEY_PACKAGE_ENCRYPTION_PROTOCOL_VERSION,
        recipient_device_id: proposal.participant_device_id,
        recipient_public_key: recipient_public_key.to_vec(),
        ephemeral_public_key: ephemeral_x25519_public.to_vec(),
        nonce,
        ciphertext,
        binding_hash,
    })
}

pub async fn decrypt_device_epoch_key_package<E>(
    effects: &E,
    proposal: &DeviceEpochProposal,
    recipient_public_key: &[u8],
    recipient_private_key: &[u8; 32],
) -> Result<Vec<u8>, AuraError>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    let encrypted = &proposal.encrypted_key_package;
    if encrypted.protocol_version != DEVICE_EPOCH_KEY_PACKAGE_ENCRYPTION_PROTOCOL_VERSION {
        return Err(AuraError::invalid(format!(
            "unsupported device epoch key-package encryption version {}",
            encrypted.protocol_version
        )));
    }
    if encrypted.recipient_device_id != proposal.participant_device_id {
        return Err(AuraError::invalid(
            "device epoch encrypted key package recipient device mismatch".to_string(),
        ));
    }
    if encrypted.recipient_public_key != recipient_public_key {
        return Err(AuraError::invalid(
            "device epoch encrypted key package recipient key mismatch".to_string(),
        ));
    }
    let ephemeral_public_key: [u8; 32] = encrypted
        .ephemeral_public_key
        .as_slice()
        .try_into()
        .map_err(|_| AuraError::invalid("device epoch ephemeral key must be 32 bytes"))?;
    let expected_binding_hash = device_epoch_key_package_binding_hash(
        proposal,
        recipient_public_key,
        &encrypted.ephemeral_public_key,
        &encrypted.ciphertext,
        encrypted.nonce,
    )?;
    if encrypted.binding_hash != expected_binding_hash {
        return Err(AuraError::invalid(
            "device epoch encrypted key package binding mismatch".to_string(),
        ));
    }
    let shared_secret = x25519_shared_secret(recipient_private_key, &ephemeral_public_key);
    let kdf_info = device_epoch_key_agreement_transcript(
        proposal,
        recipient_public_key,
        &encrypted.ephemeral_public_key,
    )?;
    let decryption_key = effects
        .kdf_derive(
            &shared_secret,
            DEVICE_EPOCH_KEY_PACKAGE_KDF_DOMAIN,
            &kdf_info,
            32,
        )
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "device epoch decryption key derivation failed: {error}"
            ))
        })?;
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&decryption_key);
    let key_package = effects
        .chacha20_decrypt(&encrypted.ciphertext, &key_array, &encrypted.nonce)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "device epoch key-package decryption failed: {error}"
            ))
        })?;
    if Hash32::from_bytes(&key_package) != proposal.key_package_hash {
        return Err(AuraError::invalid(
            "device epoch decrypted key package hash mismatch".to_string(),
        ));
    }
    Ok(key_package)
}

pub fn device_epoch_proposal_hash(proposal: &DeviceEpochProposal) -> Result<Hash32, AuraError> {
    Hash32::from_value(&DeviceEpochProposalTranscript::new(proposal).transcript_payload()).map_err(
        |error| AuraError::serialization(format!("device epoch proposal hash failed: {error}")),
    )
}

pub fn device_epoch_commit_attested_op_hash(
    commit: &DeviceEpochCommit,
) -> Result<Option<Hash32>, AuraError> {
    commit
        .attested_leaf_op
        .as_ref()
        .map(Hash32::from_value)
        .transpose()
        .map_err(|error| {
            AuraError::serialization(format!("device epoch attested-op hash failed: {error}"))
        })
}

pub async fn verify_device_epoch_authority_signature<E, T>(
    effects: &E,
    authority: AuthorityId,
    signing_domain: &str,
    transcript: &T,
    signature: &ThresholdSignature,
    trusted_public_key_package: &TrustedPublicKey,
    expected_epoch: u64,
) -> Result<bool, AuraError>
where
    E: CryptoEffects + Send + Sync + ?Sized,
    T: SecurityTranscript + ?Sized,
{
    if trusted_public_key_package.domain() != TrustedKeyDomain::AuthorityThreshold
        || trusted_public_key_package.epoch() != Some(expected_epoch)
        || signature.public_key_package != trusted_public_key_package.bytes()
        || signature.epoch != expected_epoch
    {
        return Ok(false);
    }

    let payload = transcript.transcript_bytes().map_err(|error| {
        AuraError::serialization(format!(
            "device epoch authority transcript encoding failed: {error}"
        ))
    })?;
    let signing_context = SigningContext::message(authority, signing_domain.to_string(), payload);

    if signature.is_single_signer() {
        let public_key_package = SingleSignerPublicKeyPackage::from_bytes(
            trusted_public_key_package.bytes(),
        )
        .map_err(|error| {
            AuraError::crypto(format!(
                "device epoch single-signer public-key decode failed: {error}"
            ))
        })?;
        verify_ed25519_threshold_signing_context_transcript(
            effects,
            &signing_context,
            expected_epoch,
            &signature.signature,
            public_key_package.verifying_key(),
        )
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "device epoch Ed25519 authority signature verification failed: {error}"
            ))
        })
    } else {
        let public_key_package =
            tree_signing::public_key_package_from_bytes(trusted_public_key_package.bytes())
                .map_err(|error| {
                    AuraError::crypto(format!(
                        "device epoch threshold public-key decode failed: {error}"
                    ))
                })?;
        let trusted_key_group_public_key = public_key_package.group_public_key.clone();
        verify_threshold_signing_context_transcript(
            effects,
            &signing_context,
            expected_epoch,
            &signature.signature,
            &trusted_key_group_public_key,
        )
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "device epoch FROST authority signature verification failed: {error}"
            ))
        })
    }
}

tell!(include_str!("src/protocols/device_epoch_rotation.tell"));

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::CryptoCoreEffects;
    use aura_effects::RealCryptoHandler;
    use aura_protocol::admission::{
        CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE, CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
        CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION, CAPABILITY_RECONFIGURATION_SAFETY,
        THEOREM_PACK_AURA_TRANSITION_SAFETY,
    };
    use aura_signature::threshold_signing_context_transcript_bytes;

    fn test_encrypted_key_package(recipient_device_id: DeviceId) -> EncryptedDeviceEpochKeyPackage {
        EncryptedDeviceEpochKeyPackage {
            protocol_version: DEVICE_EPOCH_KEY_PACKAGE_ENCRYPTION_PROTOCOL_VERSION,
            recipient_device_id,
            recipient_public_key: vec![10; 32],
            ephemeral_public_key: vec![11; 32],
            nonce: [12; 12],
            ciphertext: vec![13],
            binding_hash: Hash32::from_bytes(&[14]),
        }
    }

    #[test]
    fn proof_status_exposes_required_transition_pack() {
        assert_eq!(
            telltale_session_types_device_epoch_rotation::proof_status::REQUIRED_THEOREM_PACKS,
            &[THEOREM_PACK_AURA_TRANSITION_SAFETY]
        );
    }

    #[test]
    fn manifest_emits_transition_safety_pack_metadata() {
        let manifest =
            telltale_session_types_device_epoch_rotation::vm_artifacts::composition_manifest();
        assert_eq!(
            manifest.required_theorem_packs,
            vec![THEOREM_PACK_AURA_TRANSITION_SAFETY.to_string()]
        );
        assert_eq!(
            manifest.required_theorem_pack_capabilities,
            vec![
                CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE.to_string(),
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE.to_string(),
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION.to_string(),
                CAPABILITY_RECONFIGURATION_SAFETY.to_string(),
            ]
        );
    }

    #[test]
    fn device_epoch_rotation_transcripts_bind_epoch_and_participant() {
        let proposal = DeviceEpochProposal {
            ceremony_id: CeremonyId::new("device-epoch-1"),
            kind: DeviceEpochRotationKind::Rotation,
            subject_authority: AuthorityId::new_from_entropy([2u8; 32]),
            pending_epoch: 10,
            initiator_device_id: DeviceId::new_from_entropy([3u8; 32]),
            participant_device_id: DeviceId::new_from_entropy([4u8; 32]),
            key_package_hash: Hash32::from_bytes(&[5]),
            threshold_config_hash: Hash32::from_bytes(&[6]),
            public_key_package_hash: Hash32::from_bytes(&[7]),
            proposed_at_ms: 100,
            authority_signature: ThresholdSignature::single_signer(
                vec![8],
                SingleSignerPublicKeyPackage::new(vec![9; 32])
                    .to_bytes()
                    .unwrap(),
                1,
            ),
            encrypted_key_package: test_encrypted_key_package(DeviceId::new_from_entropy(
                [4u8; 32],
            )),
            threshold_config: vec![6],
            public_key_package: vec![7],
        };
        let mut different_epoch = proposal.clone();
        different_epoch.pending_epoch = 11;
        let mut different_participant = proposal.clone();
        different_participant.participant_device_id = DeviceId::new_from_entropy([8u8; 32]);

        let base =
            aura_signature::encode_transcript("aura.device-epoch.proposal", 1, &proposal).unwrap();
        let epoch =
            aura_signature::encode_transcript("aura.device-epoch.proposal", 1, &different_epoch)
                .unwrap();
        let participant = aura_signature::encode_transcript(
            "aura.device-epoch.proposal",
            1,
            &different_participant,
        )
        .unwrap();

        assert_ne!(base, epoch);
        assert_ne!(base, participant);
    }

    #[tokio::test]
    async fn device_epoch_authority_signature_verification_accepts_signed_signing_context() {
        let crypto = RealCryptoHandler::for_simulation_seed([31; 32]);
        let authority = AuthorityId::new_from_entropy([32; 32]);
        let (private_key, public_key) = crypto
            .ed25519_generate_keypair()
            .await
            .expect("ed25519 keypair");
        let trusted_public_key_package_bytes = SingleSignerPublicKeyPackage::new(public_key)
            .to_bytes()
            .expect("serialize public key package");
        let trusted_public_key_package = TrustedPublicKey::active(
            TrustedKeyDomain::AuthorityThreshold,
            Some(0),
            trusted_public_key_package_bytes.clone(),
            Hash32::from_bytes(&trusted_public_key_package_bytes),
        );
        let mut proposal = DeviceEpochProposal {
            ceremony_id: CeremonyId::new("device-epoch-signed"),
            kind: DeviceEpochRotationKind::Rotation,
            subject_authority: authority,
            pending_epoch: 9,
            initiator_device_id: DeviceId::new_from_entropy([33u8; 32]),
            participant_device_id: DeviceId::new_from_entropy([34u8; 32]),
            key_package_hash: Hash32::from_bytes(&[35]),
            threshold_config_hash: Hash32::from_bytes(&[36]),
            public_key_package_hash: Hash32::from_bytes(&[37]),
            proposed_at_ms: 200,
            authority_signature: ThresholdSignature::single_signer(
                vec![],
                trusted_public_key_package_bytes.clone(),
                0,
            ),
            encrypted_key_package: test_encrypted_key_package(DeviceId::new_from_entropy(
                [34u8; 32],
            )),
            threshold_config: vec![36],
            public_key_package: vec![37],
        };
        let transcript = DeviceEpochProposalTranscript::new(&proposal);
        let signing_context = aura_core::threshold::SigningContext::message(
            authority,
            "aura.sync.device_epoch_rotation.proposal".to_string(),
            transcript
                .transcript_bytes()
                .expect("proposal transcript bytes"),
        );
        let signature = crypto
            .ed25519_sign(
                &threshold_signing_context_transcript_bytes(&signing_context, 0)
                    .expect("signing-context transcript"),
                &private_key,
            )
            .await
            .expect("sign proposal");
        proposal.authority_signature = ThresholdSignature::single_signer(
            signature,
            trusted_public_key_package_bytes.clone(),
            0,
        );

        let verified = verify_device_epoch_authority_signature(
            &crypto,
            authority,
            "aura.sync.device_epoch_rotation.proposal",
            &DeviceEpochProposalTranscript::new(&proposal),
            &proposal.authority_signature,
            &trusted_public_key_package,
            0,
        )
        .await
        .expect("verify proposal authority signature");
        assert!(verified);
    }

    #[tokio::test]
    async fn device_epoch_authority_signature_verification_rejects_wrong_domain() {
        let crypto = RealCryptoHandler::for_simulation_seed([41; 32]);
        let authority = AuthorityId::new_from_entropy([42; 32]);
        let (private_key, public_key) = crypto
            .ed25519_generate_keypair()
            .await
            .expect("ed25519 keypair");
        let trusted_public_key_package_bytes = SingleSignerPublicKeyPackage::new(public_key)
            .to_bytes()
            .expect("serialize public key package");
        let trusted_public_key_package = TrustedPublicKey::active(
            TrustedKeyDomain::AuthorityThreshold,
            Some(0),
            trusted_public_key_package_bytes.clone(),
            Hash32::from_bytes(&trusted_public_key_package_bytes),
        );
        let mut commit = DeviceEpochCommit {
            ceremony_id: CeremonyId::new("device-epoch-commit"),
            new_epoch: 11,
            proposal_hash: Hash32::from_bytes(&[43]),
            committed_at_ms: 300,
            attested_leaf_op_hash: None,
            authority_signature: ThresholdSignature::single_signer(
                vec![],
                trusted_public_key_package_bytes.clone(),
                0,
            ),
            attested_leaf_op: None,
        };
        let transcript = DeviceEpochCommitTranscript::new(&commit);
        let signing_context = aura_core::threshold::SigningContext::message(
            authority,
            "aura.sync.device_epoch_rotation.commit".to_string(),
            transcript
                .transcript_bytes()
                .expect("commit transcript bytes"),
        );
        let signature = crypto
            .ed25519_sign(
                &threshold_signing_context_transcript_bytes(&signing_context, 0)
                    .expect("signing-context transcript"),
                &private_key,
            )
            .await
            .expect("sign commit");
        commit.authority_signature = ThresholdSignature::single_signer(
            signature,
            trusted_public_key_package_bytes.clone(),
            0,
        );

        let verified = verify_device_epoch_authority_signature(
            &crypto,
            authority,
            "aura.sync.device_epoch_rotation.proposal",
            &DeviceEpochCommitTranscript::new(&commit),
            &commit.authority_signature,
            &trusted_public_key_package,
            0,
        )
        .await
        .expect("verify commit authority signature");
        assert!(!verified);
    }
}
