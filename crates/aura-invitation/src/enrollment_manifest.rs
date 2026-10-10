//! Exact signed enrollment handoff inventory. This domain establishes signature
//! integrity; app/runtime owners establish independent pin provenance and replay
//! the baseline under these exact parents before admitting a response lease.
use crate::enrollment_setup::DeviceEnrollmentSetupBinding;
use aura_core::crypto::single_signer::SigningMode;
use aura_core::effects::CryptoEffects;
use aura_core::threshold::{AgreementMode, ParticipantIdentity};
use aura_core::tree::NodeIndex;
use aura_core::{AuthorityId, CeremonyId, DeviceId, InvitationId};
use aura_signature::SecurityTranscript;
use serde::{Deserialize, Serialize};

/// Canonical maximum original allocation window for device enrollment.
/// Setup-code validity may be longer; it cannot widen this operation policy.
pub const ENROLLMENT_ALLOCATION_TIMEOUT_MS: u64 = 600_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Remote untrusted key material until independent manifest pinning and complete
/// baseline verification establish the exact parent ownership contract.
pub struct EnrollmentParentVerifier {
    pub epoch: u64,
    pub commitment: [u8; 32],
    pub signing_node: NodeIndex,
    pub mode: SigningMode,
    pub threshold: u16,
    /// Ordered signer-index inventory: element zero is signer index one.
    pub participants: Vec<ParticipantIdentity>,
    pub public_key_package: Vec<u8>,
    pub agreement: AgreementMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Remote untrusted key material; the embedded verifier is only a match target.
/// Signature integrity requires an independently supplied verifier. Admission
/// requires the separate app-owned user-transfer pin and runtime replay owner.
pub struct EnrollmentTrustManifest {
    pub version: u16,
    pub subject: AuthorityId,
    pub initiator_device: DeviceId,
    pub invitee_authority: AuthorityId,
    pub invitee_device: DeviceId,
    pub setup: DeviceEnrollmentSetupBinding,
    pub invitation: InvitationId,
    pub ceremony: CeremonyId,
    pub expires_at_ms: u64,
    pub baseline_digest: [u8; 32],
    pub baseline_count: u32,
    pub starting_epoch: u64,
    pub starting_commitment: [u8; 32],
    pub parents: Vec<EnrollmentParentVerifier>,
    /// Active exact-node verification inventory, distinct from historical op parents.
    /// Required exact independently authenticated active inventory.
    pub final_inventory: Vec<EnrollmentParentVerifier>,
    pub final_epoch: u64,
    pub final_commitment: [u8; 32],
    pub pending_epoch: u64,
    pub pending_share_digest: [u8; 32],
    pub pending_public_key_package_digest: [u8; 32],
    /// Public content hash of the exact pending configuration bytes, not key material.
    pub pending_threshold_config_digest: aura_core::Hash32,
    /// Included for exact matching; never used as its own trust anchor.
    pub initiator_confirmation_verifier: Vec<u8>,
}

impl SecurityTranscript for EnrollmentTrustManifest {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.enrollment-trust-manifest.v3";
    fn transcript_bytes(&self) -> aura_signature::Result<Vec<u8>> {
        aura_signature::encode_transcript(Self::DOMAIN_SEPARATOR, self.version, self)
    }

    fn transcript_payload(&self) -> Self {
        self.clone()
    }
    fn required_transcript_bytes(
        &self,
    ) -> Result<Vec<u8>, aura_signature::RequiredTranscriptEncodingError> {
        aura_signature::encode_transcript_required(Self::DOMAIN_SEPARATOR, self.version, self)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EnrollmentManifestError {
    #[error("enrollment manifest canonical codec failed")]
    Codec(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("enrollment manifest baseline reduction failed")]
    Baseline(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("enrollment manifest native proof failed")]
    NativeProof(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("enrollment verifier transfer encoding failed")]
    PinEncoding(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("enrollment manifest runtime stage failed")]
    Runtime(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("enrollment initiator verifier must be transferred separately")]
    MissingPin,
    #[error("manifest lacks independently authenticated final active verifier inventory")]
    MissingFinalInventory,
    #[error("enrollment manifest validity exceeds the selected setup grant")]
    SetupValidity,
    #[error("enrollment manifest verification is unavailable")]
    Unavailable,
    #[error("enrollment manifest has expired")]
    Expired,
    #[error("enrollment manifest clock failed")]
    Time(#[from] aura_core::effects::time::TimeError),
    #[error("enrollment manifest workflow boundary failed")]
    Boundary(#[source] aura_core::AuraError),
    #[error("invalid or oversized enrollment manifest")]
    Shape,
    #[error("manifest does not match independently supplied initiator verifier")]
    Pin,
    #[error("manifest signature is invalid")]
    Signature,
    #[error("manifest transcript failed")]
    Transcript(#[from] aura_signature::AuthenticationError),
    #[error("manifest required signing or verification transcript failed")]
    RequiredTranscript(#[from] aura_signature::TranscriptCryptoError),
    #[error("manifest cryptography failed")]
    Crypto(#[source] aura_core::AuraError),
}

/// Signature evidence only; no Deserialize or public constructor.
#[derive(Debug, Clone)]
pub struct VerifiedEnrollmentManifestSignature {
    manifest: EnrollmentTrustManifest,
    digest: [u8; 32],
}
impl VerifiedEnrollmentManifestSignature {
    pub fn manifest(&self) -> &EnrollmentTrustManifest {
        &self.manifest
    }
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

/// Pure validated metadata; this shape alone never authorizes key selection.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentPendingPolicy {
    threshold_k: u16,
    total_n: u16,
    participants: Vec<ParticipantIdentity>,
    mode: SigningMode,
    agreement_mode: AgreementMode,
}
impl EnrollmentPendingPolicy {
    /// Exact signer cardinality authenticated by the signed pending policy.
    pub fn total_participants(&self) -> u16 {
        self.total_n
    }

    pub fn signing_mode(&self) -> SigningMode {
        self.mode
    }
    pub fn participants(&self) -> &[ParticipantIdentity] {
        &self.participants
    }
    pub fn agreement(&self) -> AgreementMode {
        self.agreement_mode
    }
    pub fn threshold(&self) -> u16 {
        self.threshold_k
    }
}

impl EnrollmentTrustManifest {
    /// Manifest validity may attenuate the selected setup grant, never extend it.
    /// This arithmetic check does not establish independent setup provenance.
    pub fn validate_setup_validity(
        &self,
        setup: &crate::enrollment_setup::DeviceEnrollmentSetupStatement,
    ) -> Result<(), EnrollmentManifestError> {
        if self.expires_at_ms <= setup.issued_at_ms || self.expires_at_ms > setup.expires_at_ms {
            return Err(EnrollmentManifestError::SetupValidity);
        }
        Ok(())
    }

    /// Validate the exact canonical provisional policy bytes bound at issuance.
    /// The field order is the persisted threshold metadata wire contract.
    pub fn validate_pending_policy(bytes: &[u8]) -> Result<(), EnrollmentManifestError> {
        Self::decode_pending_policy(bytes).map(|_| ())
    }
    pub fn decode_pending_policy(
        bytes: &[u8],
    ) -> Result<EnrollmentPendingPolicy, EnrollmentManifestError> {
        if bytes.len() > 65_536 {
            return Err(EnrollmentManifestError::Shape);
        }
        let policy: EnrollmentPendingPolicy = serde_json::from_slice(bytes)
            .map_err(|e| EnrollmentManifestError::Runtime(Box::new(e)))?;
        if policy.agreement_mode != AgreementMode::Provisional
            || policy.threshold_k == 0
            || policy.threshold_k > policy.total_n
            || usize::from(policy.total_n) != policy.participants.len()
            || policy.participants.len() > Self::MAX_PARTICIPANTS
            || (policy.mode == SigningMode::SingleSigner
                && (policy.threshold_k != 1 || policy.total_n != 1))
            || (policy.mode == SigningMode::Threshold && policy.threshold_k < 2)
        {
            return Err(EnrollmentManifestError::Shape);
        }
        let mut identities = std::collections::BTreeSet::new();
        for participant in &policy.participants {
            let encoded = aura_core::util::serialization::to_vec(participant)
                .map_err(|e| EnrollmentManifestError::Runtime(Box::new(e)))?;
            if !identities.insert(encoded) {
                return Err(EnrollmentManifestError::Shape);
            }
        }
        if serde_json::to_vec(&policy).map_err(|e| EnrollmentManifestError::Runtime(Box::new(e)))?
            != bytes
        {
            return Err(EnrollmentManifestError::Shape);
        }
        Ok(policy)
    }
    /// This getter requires the current signed inventory; historical parent tuples are
    /// never promoted to an active epoch or substituted for this inventory.
    pub fn final_inventory(&self) -> Result<&[EnrollmentParentVerifier], EnrollmentManifestError> {
        if self.final_inventory.is_empty() {
            return Err(EnrollmentManifestError::MissingFinalInventory);
        }
        Ok(&self.final_inventory)
    }
    /// Current signed enrollment manifest schema.
    pub const CURRENT_VERSION: u16 = 3;
    pub const MAX_BYTES: usize = 1_048_576;
    pub const MAX_PARENTS: usize = 4096;
    /// Maximum canonical operations in an enrollment baseline.
    pub const MAX_BASELINE_OPS: usize = 4096;
    pub const MAX_PARTICIPANTS: usize = 1024;

    pub fn validate_shape(&self) -> Result<(), EnrollmentManifestError> {
        if self.version != Self::CURRENT_VERSION
            || self.parents.is_empty()
            || self.parents.len() > Self::MAX_PARENTS
            || self.starting_epoch > self.final_epoch
            || self.pending_epoch <= self.final_epoch
            || self.initiator_confirmation_verifier.len() != 32
            || self.invitation.to_string().is_empty()
            || self.invitation.to_string().len() > 128
            || self.ceremony.to_string().is_empty()
            || self.ceremony.to_string().len() > 128
        {
            return Err(EnrollmentManifestError::Shape);
        }
        {
            let inventory = &self.final_inventory;
            if inventory.is_empty() || inventory.len() > Self::MAX_PARENTS {
                return Err(EnrollmentManifestError::Shape);
            }
            let mut nodes = std::collections::BTreeSet::new();
            for active in inventory {
                if active.epoch != self.final_epoch
                    || active.commitment != self.final_commitment
                    || !nodes.insert(active.signing_node)
                    || active.threshold == 0
                    || usize::from(active.threshold) > active.participants.len()
                    || active.participants.len() > Self::MAX_PARTICIPANTS
                    || active.public_key_package.is_empty()
                    || active.public_key_package.len() > 65_536
                    || active
                        .participants
                        .iter()
                        .enumerate()
                        .any(|(i, p)| active.participants[..i].contains(p))
                    || match active.mode {
                        SigningMode::SingleSigner => {
                            active.threshold != 1 || active.participants.len() != 1
                        }
                        SigningMode::Threshold => active.threshold < 2,
                    }
                {
                    return Err(EnrollmentManifestError::Shape);
                }
            }
        }
        let mut keys = std::collections::BTreeSet::new();
        for p in &self.parents {
            let node = aura_core::util::serialization::to_vec(&p.signing_node)
                .map_err(|source| EnrollmentManifestError::Codec(Box::new(source)))?;
            if !keys.insert((p.epoch, p.commitment, node))
                || p.epoch < self.starting_epoch
                || p.epoch > self.final_epoch
                || p.threshold == 0
                || usize::from(p.threshold) > p.participants.len()
                || p.participants.len() > Self::MAX_PARTICIPANTS
                || p.public_key_package.is_empty()
                || p.public_key_package.len() > 65_536
                || p.participants
                    .iter()
                    .enumerate()
                    .any(|(i, x)| p.participants[..i].contains(x))
                || match p.mode {
                    SigningMode::SingleSigner => p.threshold != 1 || p.participants.len() != 1,
                    SigningMode::Threshold => p.threshold < 2,
                }
            {
                return Err(EnrollmentManifestError::Shape);
            }
        }
        {
            let inventory = &self.final_inventory;
            for active in inventory {
                match active.mode {
                    SigningMode::SingleSigner => {
                        aura_core::crypto::single_signer::SingleSignerPublicKeyPackage::from_bytes(
                            &active.public_key_package,
                        )
                        .map_err(|source| EnrollmentManifestError::Runtime(Box::new(source)))?;
                    }
                    SigningMode::Threshold => {
                        let package =
                            aura_core::crypto::tree_signing::public_key_package_from_bytes(
                                &active.public_key_package,
                            )
                            .map_err(|source| EnrollmentManifestError::Runtime(Box::new(source)))?;
                        let total = u16::try_from(active.participants.len())
                            .map_err(|source| EnrollmentManifestError::Runtime(Box::new(source)))?;
                        // Native FROST public packages do not encode quorum.
                        // The signed tuple retains the actual protected policy.
                        if package.max_signers != total
                            || package.signer_public_keys.len() != usize::from(total)
                            || !(1..=total)
                                .all(|index| package.signer_public_keys.contains_key(&index))
                        {
                            return Err(EnrollmentManifestError::Shape);
                        }
                    }
                }
            }
        }
        if self.transcript_bytes()?.len() > Self::MAX_BYTES {
            return Err(EnrollmentManifestError::Shape);
        }
        Ok(())
    }

    pub async fn verify_signature<E: CryptoEffects + Send + Sync + ?Sized>(
        self,
        crypto: &E,
        independently_supplied_verifier: &[u8],
        signature: &[u8],
    ) -> Result<VerifiedEnrollmentManifestSignature, EnrollmentManifestError> {
        self.validate_shape()?;
        if independently_supplied_verifier.len() != 32
            || independently_supplied_verifier != self.initiator_confirmation_verifier
        {
            return Err(EnrollmentManifestError::Pin);
        }
        if signature.len() != 64
            || !aura_signature::verify_ed25519_transcript(
                crypto,
                &self,
                signature,
                independently_supplied_verifier,
            )
            .await?
        {
            return Err(EnrollmentManifestError::Signature);
        }
        let digest = aura_core::hash::hash(&self.transcript_bytes()?);
        Ok(VerifiedEnrollmentManifestSignature {
            manifest: self,
            digest,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::CryptoCoreEffects;
    use aura_effects::crypto::RealCryptoHandler;

    #[test]
    fn missing_final_inventory_and_obsolete_manifest_versions_are_rejected() {
        let current = manifest(vec![114; 32]);
        let mut missing = serde_json::to_value(&current).unwrap();
        missing.as_object_mut().unwrap().remove("final_inventory");
        assert!(serde_json::from_value::<EnrollmentTrustManifest>(missing).is_err());
        for version in [1, 2] {
            let code = format!("aura-enrollment-manifest:v{version}:AA");
            assert!(SignedEnrollmentTrustManifest::decode(&code).is_err());
            let mut old = current.clone();
            old.version = version;
            assert!(old.validate_shape().is_err());
        }
    }

    #[tokio::test]
    async fn active_inventory_has_distinct_signed_epoch_and_domain() {
        let crypto = RealCryptoHandler::for_simulation_seed([115; 32]);
        let (private, public) = crypto.ed25519_generate_keypair().await.unwrap();
        let mut current = manifest(public.clone());
        current.version = 3;
        let package =
            aura_core::crypto::single_signer::SingleSignerPublicKeyPackage::try_new(public.clone())
                .unwrap();
        let mut active = current.parents[0].clone();
        active.epoch = current.final_epoch;
        active.commitment = current.final_commitment;
        active.public_key_package = package.to_bytes().unwrap();
        current.final_inventory = vec![active];
        let signature = aura_signature::sign_ed25519_transcript(&crypto, &current, &private)
            .await
            .unwrap();
        current
            .clone()
            .verify_signature(&crypto, &public, &signature)
            .await
            .unwrap();
        let mut changed = current.clone();
        changed.final_inventory[0].public_key_package[0] ^= 1;
        assert!(changed
            .verify_signature(&crypto, &public, &signature)
            .await
            .is_err());
        let mut promoted = current.clone();
        promoted.final_inventory = promoted.parents.clone();
        assert!(promoted.validate_shape().is_err());
        let old_domain = aura_signature::encode_transcript(
            "aura.invitation.enrollment-trust-manifest.v1",
            1,
            &current,
        )
        .unwrap();
        assert_ne!(old_domain, current.transcript_bytes().unwrap());
        let old_signature = crypto.ed25519_sign(&old_domain, &private).await.unwrap();
        assert!(current
            .clone()
            .verify_signature(&crypto, &public, &old_signature)
            .await
            .is_err());
        let signed = SignedEnrollmentTrustManifest {
            manifest: current,
            signature,
        };
        let code = signed.encode().unwrap();
        assert!(code.starts_with("aura-enrollment-manifest:v3:"));
        let confused = code.replacen(":v3:", ":v2:", 1);
        assert!(SignedEnrollmentTrustManifest::decode(&confused).is_err());
    }

    // These fixtures test signature integrity only.
    // They do not stand in for
    // an app-owned transfer pin, authenticated baseline or response lease.
    #[test]
    fn selected_setup_validity_can_only_be_attenuated() {
        let mut value = manifest(vec![113; 32]);
        let setup = crate::enrollment_setup::DeviceEnrollmentSetupStatement {
            version: 1,
            authority: value.invitee_authority,
            device: value.invitee_device,
            nonce: value.setup.nonce,
            issued_at_ms: 100,
            expires_at_ms: 200,
            signing_epoch: 0,
            signing_mode: SigningMode::SingleSigner,
            threshold: 1,
            participants: 1,
            public_key_package: Vec::new(),
        };
        for allowed in [101, 150, 200] {
            value.expires_at_ms = allowed;
            value.validate_setup_validity(&setup).unwrap();
        }
        for denied in [0, 100, 201, u64::MAX] {
            value.expires_at_ms = denied;
            assert!(matches!(
                value.validate_setup_validity(&setup),
                Err(EnrollmentManifestError::SetupValidity)
            ));
        }
    }

    #[test]
    fn required_manifest_codec_failures_preserve_native_causes() {
        use std::error::Error;
        let base64 =
            SignedEnrollmentTrustManifest::decode("aura-enrollment-manifest:v3:!").unwrap_err();
        assert!(matches!(base64, EnrollmentManifestError::Codec(_)));
        assert!(base64.source().unwrap().is::<base64::DecodeError>());
        let canonical =
            SignedEnrollmentTrustManifest::decode("aura-enrollment-manifest:v3:AA").unwrap_err();
        assert!(matches!(canonical, EnrollmentManifestError::Codec(_)));
        assert!(canonical
            .source()
            .unwrap()
            .is::<aura_core::util::serialization::SerializationError>());
        let pin = decode_initiator_verifier_transfer("aura-initiator-verifier:v1:!").unwrap_err();
        assert!(matches!(pin, EnrollmentManifestError::PinEncoding(_)));
        assert!(pin.source().unwrap().is::<base64::DecodeError>());
    }

    #[test]
    fn required_manifest_encoding_preserves_current_domain_bytes() {
        let version = 3;
        {
            let mut original = manifest(vec![113; 32]);
            original.version = version;
            let diagnostic = original
                .transcript_bytes()
                .unwrap_or_else(|error| panic!("encode original manifest schema: {error}"));
            let required = original
                .required_transcript_bytes()
                .unwrap_or_else(|error| panic!("encode required manifest schema: {error}"));
            assert_eq!(
                diagnostic, required,
                "manifest schema {version} must retain exact domain/version bytes"
            );
        }
    }

    fn manifest(verifier: Vec<u8>) -> EnrollmentTrustManifest {
        let device = DeviceId::new_from_entropy([91; 32]);
        EnrollmentTrustManifest {
            version: EnrollmentTrustManifest::CURRENT_VERSION,
            subject: AuthorityId::new_from_entropy([92; 32]),
            initiator_device: device,
            invitee_authority: AuthorityId::new_from_entropy([93; 32]),
            invitee_device: DeviceId::new_from_entropy([94; 32]),
            setup: DeviceEnrollmentSetupBinding {
                nonce: [95; 32],
                digest: [96; 32],
            },
            invitation: InvitationId::new("exact-manifest-invitation"),
            ceremony: CeremonyId::new("exact-manifest-ceremony"),
            expires_at_ms: 200,
            baseline_digest: [97; 32],
            baseline_count: 1,
            starting_epoch: 0,
            starting_commitment: [98; 32],
            final_inventory: vec![EnrollmentParentVerifier {
                epoch: 1,
                commitment: [99; 32],
                signing_node: NodeIndex(0),
                mode: SigningMode::SingleSigner,
                threshold: 1,
                participants: vec![ParticipantIdentity::device(device)],
                public_key_package:
                    aura_core::crypto::single_signer::SingleSignerPublicKeyPackage::try_new(
                        verifier.clone(),
                    )
                    .unwrap()
                    .to_bytes()
                    .unwrap(),
                agreement: AgreementMode::ConsensusFinalized,
            }],
            final_epoch: 1,
            final_commitment: [99; 32],
            pending_epoch: 2,
            pending_share_digest: [106; 32],
            pending_public_key_package_digest: [100; 32],
            pending_threshold_config_digest: aura_core::Hash32::new([101; 32]),
            initiator_confirmation_verifier: verifier,
            parents: vec![EnrollmentParentVerifier {
                epoch: 0,
                commitment: [98; 32],
                signing_node: NodeIndex(0),
                mode: SigningMode::SingleSigner,
                threshold: 1,
                participants: vec![ParticipantIdentity::device(device)],
                public_key_package: vec![102; 32],
                agreement: AgreementMode::ConsensusFinalized,
            }],
        }
    }

    #[tokio::test]
    async fn exact_manifest_signature_rejects_external_pin_and_inventory_changes() {
        let crypto = RealCryptoHandler::for_simulation_seed([103; 32]);
        let (private, public) = crypto.ed25519_generate_keypair().await.unwrap();
        let original = manifest(public.clone());
        let signature = aura_signature::sign_ed25519_transcript(&crypto, &original, &private)
            .await
            .unwrap();
        original
            .clone()
            .verify_signature(&crypto, &public, &signature)
            .await
            .unwrap();
        assert!(original
            .clone()
            .verify_signature(&crypto, &[104; 32], &signature)
            .await
            .is_err());
        for which in 0..26 {
            let mut changed = original.clone();
            match which {
                0 => changed.setup.nonce[0] ^= 1,
                1 => changed.setup.digest[0] ^= 1,
                2 => changed.parents[0].commitment[0] ^= 1,
                3 => changed.parents[0].signing_node = NodeIndex(1),
                4 => changed.parents[0].public_key_package[0] ^= 1,
                5 => {
                    changed.parents[0].participants[0] =
                        ParticipantIdentity::device(DeviceId::new_from_entropy([105; 32]));
                }
                6 => changed.parents[0].agreement = AgreementMode::Provisional,
                7 => changed.baseline_digest[0] ^= 1,
                8 => changed.final_commitment[0] ^= 1,
                9 => changed.pending_public_key_package_digest[0] ^= 1,
                10 => changed.invitation = InvitationId::new("other-invitation"),
                11 => changed.ceremony = CeremonyId::new("other-ceremony"),
                12 => changed.subject = AuthorityId::new_from_entropy([107; 32]),
                13 => changed.initiator_device = DeviceId::new_from_entropy([108; 32]),
                14 => changed.invitee_authority = AuthorityId::new_from_entropy([109; 32]),
                15 => changed.invitee_device = DeviceId::new_from_entropy([110; 32]),
                16 => changed.expires_at_ms += 1,
                17 => changed.pending_share_digest[0] ^= 1,
                18 => changed.pending_threshold_config_digest.0[0] ^= 1,
                19 => changed.initiator_confirmation_verifier[0] ^= 1,
                20 => changed.starting_commitment[0] ^= 1,
                21 => changed.starting_epoch += 1,
                22 => changed.final_epoch += 1,
                23 => changed.pending_epoch += 1,
                24 => changed.parents[0].epoch += 1,
                _ => changed.baseline_count += 1,
            }
            assert!(
                changed
                    .verify_signature(&crypto, &public, &signature)
                    .await
                    .is_err(),
                "manifest substitution {which} accepted the original signature"
            );
        }
        let mut duplicate = original.clone();
        duplicate.parents.push(duplicate.parents[0].clone());
        assert!(duplicate.validate_shape().is_err());
        let mut oversized = original;
        oversized.invitation = InvitationId::new("x".repeat(129));
        assert!(oversized.validate_shape().is_err());
    }
}

/// Fully staged signature/transition validation; committing this baseline still
/// requires an app-owned independent transfer pin and runtime admission owner.
pub struct VerifiedEnrollmentBaseline {
    manifest: EnrollmentTrustManifest,
    manifest_digest: [u8; 32],
    ops: Vec<aura_core::AttestedOp>,
}
impl VerifiedEnrollmentBaseline {
    pub fn manifest(&self) -> &EnrollmentTrustManifest {
        &self.manifest
    }
    pub fn manifest_digest(&self) -> [u8; 32] {
        self.manifest_digest
    }
    pub fn ops(&self) -> &[aura_core::AttestedOp] {
        &self.ops
    }
}
impl VerifiedEnrollmentManifestSignature {
    /// Verify the exact transferred baseline from its empty genesis state.
    /// Snapshot starts require an independent authenticated snapshot contract;
    /// this method deliberately rejects them instead of inventing state.
    pub fn verify_baseline(
        self,
        baseline: &[Vec<u8>],
    ) -> Result<VerifiedEnrollmentBaseline, EnrollmentManifestError> {
        use aura_core::crypto::single_signer::SingleSignerPublicKeyPackage;
        use aura_core::tree::{extract_target_node, verify_attested_op, BranchSigningKey};
        if baseline.len() != self.manifest.baseline_count as usize
            || baseline.len() > EnrollmentTrustManifest::MAX_BASELINE_OPS
        {
            return Err(EnrollmentManifestError::Shape);
        }
        let encoded = aura_core::util::serialization::to_vec(&baseline)
            .map_err(|source| EnrollmentManifestError::Codec(Box::new(source)))?;
        if encoded.len() > EnrollmentTrustManifest::MAX_BYTES
            || aura_core::hash::hash(&encoded) != self.manifest.baseline_digest
        {
            return Err(EnrollmentManifestError::Shape);
        }
        let mut staged = Vec::with_capacity(baseline.len());
        let mut used = std::collections::BTreeSet::new();
        let mut state = aura_journal::commitment_tree::reduce(&staged)
            .map_err(|source| EnrollmentManifestError::Baseline(Box::new(source)))?;
        if state.epoch.value() != self.manifest.starting_epoch
            || state.root_commitment != self.manifest.starting_commitment
        {
            return Err(EnrollmentManifestError::Shape);
        }
        for bytes in baseline {
            if bytes.len() > 131_072 {
                return Err(EnrollmentManifestError::Shape);
            }
            let op: aura_core::AttestedOp = aura_core::util::serialization::from_slice(bytes)
                .map_err(|source| EnrollmentManifestError::Codec(Box::new(source)))?;
            if op.op.parent_epoch != state.epoch || op.op.parent_commitment != state.root_commitment
            {
                return Err(EnrollmentManifestError::Shape);
            }
            let target = extract_target_node(&op.op.op)
                .or_else(|| match &op.op.op {
                    aura_core::TreeOpKind::RemoveLeaf { leaf, .. } => {
                        state.get_remove_leaf_affected_parent(leaf)
                    }
                    _ => None,
                })
                .ok_or(EnrollmentManifestError::Shape)?;
            let (index, parent) = self
                .manifest
                .parents
                .iter()
                .enumerate()
                .find(|(_, p)| {
                    p.epoch == op.op.parent_epoch.value()
                        && p.commitment == op.op.parent_commitment
                        && p.signing_node == target
                })
                .ok_or(EnrollmentManifestError::Shape)?;
            used.insert(index);
            let key: [u8; 32] = match parent.mode {
                SigningMode::SingleSigner => {
                    SingleSignerPublicKeyPackage::from_bytes(&parent.public_key_package)
                        .map_err(|source| EnrollmentManifestError::NativeProof(Box::new(source)))?
                        .verifying_key()
                        .try_into()
                        .map_err(|source| EnrollmentManifestError::NativeProof(Box::new(source)))?
                }
                SigningMode::Threshold => {
                    aura_core::crypto::tree_signing::public_key_package_from_bytes(
                        &parent.public_key_package,
                    )
                    .map_err(|source| EnrollmentManifestError::NativeProof(Box::new(source)))?
                    .group_public_key
                    .as_slice()
                    .try_into()
                    .map_err(|source| EnrollmentManifestError::NativeProof(Box::new(source)))?
                }
            };
            let branch = BranchSigningKey::new(key, op.op.parent_epoch);
            if state
                .get_signing_key(&target)
                .is_some_and(|stored| stored != &branch)
                || usize::from(op.signer_count) > parent.participants.len()
            {
                return Err(EnrollmentManifestError::Shape);
            }
            verify_attested_op(&op, &branch, parent.threshold, state.epoch)
                .map_err(|source| EnrollmentManifestError::NativeProof(Box::new(source)))?;
            staged.push(op);
            state = aura_journal::commitment_tree::reduce(&staged)
                .map_err(|source| EnrollmentManifestError::Baseline(Box::new(source)))?;
        }
        if used.len() != self.manifest.parents.len()
            || state.epoch.value() != self.manifest.final_epoch
            || state.root_commitment != self.manifest.final_commitment
        {
            return Err(EnrollmentManifestError::Shape);
        }
        {
            let inventory = &self.manifest.final_inventory;
            let devices: std::collections::BTreeSet<_> = state
                .leaves
                .values()
                .filter(|leaf| leaf.role == aura_core::tree::LeafRole::Device)
                .map(|leaf| leaf.device_id)
                .collect();
            for active in inventory {
                // Exact-node subtree roster persistence is not yet implemented;
                // do not copy a root verifier into a nonroot branch on replay.
                if active.signing_node != NodeIndex(0)
                    || state.branches.keys().any(|node| *node != NodeIndex(0))
                    || active.participants.len() != devices.len()
                    || active
                        .participants
                        .iter()
                        .any(|participant| match participant {
                            ParticipantIdentity::Device(device) => !devices.contains(device),
                            _ => true,
                        })
                {
                    return Err(EnrollmentManifestError::Shape);
                }
            }
        }
        Ok(VerifiedEnrollmentBaseline {
            manifest: self.manifest,
            manifest_digest: self.digest,
            ops: staged,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedEnrollmentTrustManifest {
    pub manifest: EnrollmentTrustManifest,
    pub signature: Vec<u8>,
}
impl SignedEnrollmentTrustManifest {
    pub fn encode(&self) -> Result<String, EnrollmentManifestError> {
        use base64::Engine;
        self.manifest.validate_shape()?;
        if self.signature.len() != 64 {
            return Err(EnrollmentManifestError::Signature);
        }
        let bytes = aura_core::util::serialization::to_vec(self)
            .map_err(|source| EnrollmentManifestError::Codec(Box::new(source)))?;
        if bytes.len() > EnrollmentTrustManifest::MAX_BYTES {
            return Err(EnrollmentManifestError::Shape);
        }
        Ok(format!(
            "aura-enrollment-manifest:v{}:{}",
            self.manifest.version,
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
        ))
    }
    pub fn decode(code: &str) -> Result<Self, EnrollmentManifestError> {
        use base64::Engine;
        let outer_version = 3;
        let encoded = code
            .strip_prefix("aura-enrollment-manifest:v3:")
            .ok_or(EnrollmentManifestError::Shape)?;
        if encoded.len()
            > aura_core::envelope::max_base64_encoded_len(EnrollmentTrustManifest::MAX_BYTES)
        {
            return Err(EnrollmentManifestError::Shape);
        }
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|source| EnrollmentManifestError::Codec(Box::new(source)))?;
        if bytes.len() > EnrollmentTrustManifest::MAX_BYTES {
            return Err(EnrollmentManifestError::Shape);
        }
        let value: Self = aura_core::util::serialization::from_slice(&bytes)
            .map_err(|source| EnrollmentManifestError::Codec(Box::new(source)))?;
        if value.manifest.version != outer_version {
            return Err(EnrollmentManifestError::Shape);
        }
        value.manifest.validate_shape()?;
        if value.signature.len() != 64 {
            return Err(EnrollmentManifestError::Signature);
        }
        Ok(value)
    }
}

/// Separate user-transferred confirmation verifier code. Decoding is not trust;
/// only the app's explicit transfer owner may promote it to a selected pin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitiatorVerifierTransferStatement {
    pub version: u16,
    pub subject: AuthorityId,
    pub initiator_device: DeviceId,
    pub verifying_key: [u8; 32],
}
pub fn decode_initiator_verifier_transfer(
    code: &str,
) -> Result<InitiatorVerifierTransferStatement, EnrollmentManifestError> {
    use base64::Engine;
    let encoded = code
        .strip_prefix("aura-initiator-verifier:v1:")
        .ok_or(EnrollmentManifestError::Pin)?;
    if encoded.len() > aura_core::envelope::max_base64_encoded_len(512) {
        return Err(EnrollmentManifestError::Pin);
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|source| EnrollmentManifestError::PinEncoding(Box::new(source)))?;
    if bytes.len() > 512 {
        return Err(EnrollmentManifestError::Pin);
    }
    let selected: InitiatorVerifierTransferStatement =
        aura_core::util::serialization::from_slice(&bytes)
            .map_err(|source| EnrollmentManifestError::PinEncoding(Box::new(source)))?;
    if selected.version != 1 {
        return Err(EnrollmentManifestError::Pin);
    }
    Ok(selected)
}
pub fn encode_initiator_verifier_transfer(
    subject: AuthorityId,
    initiator_device: DeviceId,
    verifier: &[u8],
) -> Result<String, EnrollmentManifestError> {
    use base64::Engine;
    let verifying_key = verifier
        .try_into()
        .map_err(|source| EnrollmentManifestError::PinEncoding(Box::new(source)))?;
    let selected = InitiatorVerifierTransferStatement {
        version: 1,
        subject,
        initiator_device,
        verifying_key,
    };
    let bytes = aura_core::util::serialization::to_vec(&selected)
        .map_err(|source| EnrollmentManifestError::PinEncoding(Box::new(source)))?;
    Ok(format!(
        "aura-initiator-verifier:v1:{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    ))
}

#[cfg(test)]
mod transfer_wire_tests {
    use super::*;

    #[test]
    fn separate_transfer_preserves_selected_identity_and_key() {
        let subject = AuthorityId::new_from_entropy([141; 32]);
        let device = DeviceId::new_from_entropy([142; 32]);
        let key = [143; 32];
        let code = encode_initiator_verifier_transfer(subject, device, &key).unwrap();
        let selected = decode_initiator_verifier_transfer(&code).unwrap();
        assert_eq!(selected.subject, subject);
        assert_eq!(selected.initiator_device, device);
        assert_eq!(selected.verifying_key, key);
    }

    #[test]
    fn separate_transfer_rejects_key_only_and_oversized_legacy_input() {
        use base64::Engine;
        let old = format!(
            "aura-initiator-verifier:v1:{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([144; 32])
        );
        assert!(decode_initiator_verifier_transfer(&old).is_err());
        let oversized = format!("aura-initiator-verifier:v1:{}", "A".repeat(1024));
        assert!(decode_initiator_verifier_transfer(&oversized).is_err());
        assert!(encode_initiator_verifier_transfer(
            AuthorityId::new_from_entropy([141; 32]),
            DeviceId::new_from_entropy([142; 32]),
            &[1; 31]
        )
        .is_err());
    }
}

#[cfg(test)]
mod public_manifest_digest_wire_tests {
    #[test]
    fn typed_public_hash_preserves_original_array_wire_representation() {
        let bytes = [101u8; 32];
        let digest = aura_core::Hash32::new(bytes);
        let typed = aura_core::util::serialization::to_vec(&digest).expect("encode typed digest");
        let legacy =
            aura_core::util::serialization::to_vec(&bytes).expect("encode raw digest array");
        assert_eq!(
            typed, legacy,
            "public hash migration must preserve canonical wire bytes"
        );
        let decoded: aura_core::Hash32 = aura_core::util::serialization::from_slice(&legacy)
            .expect("decode existing digest bytes");
        assert_eq!(decoded.as_bytes(), &bytes);
    }
}
