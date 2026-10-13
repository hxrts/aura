//! Setup-bound registration for the temporary enrollment VM mailbox.
//! The setup signature proves possession only. An independently admitted
//! original manifest is required before the registration can select a route.

use crate::enrollment_manifest::{EnrollmentTrustManifest, VerifiedEnrollmentBaseline};
use crate::enrollment_setup::{DeviceEnrollmentSetupRequest, VerifiedEnrollmentSetupPossession};
use aura_core::crypto::single_signer::SigningMode;
use aura_core::effects::{CryptoExtendedEffects, PhysicalTimeEffects};
use aura_core::threshold::{SigningContext, ThresholdSignature};
use aura_core::{AuthorityId, CeremonyId, DeviceId, InvitationId};
use aura_signature::{threshold_signing_context_transcript_bytes, SecurityTranscript};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingEnrollmentMailboxStatement {
    version: u16,
    invitation: InvitationId,
    ceremony: CeremonyId,
    manifest_digest: [u8; 32],
    setup_digest: [u8; 32],
    subject: AuthorityId,
    provisional: AuthorityId,
    invitee_device: DeviceId,
    expires_at_ms: u64,
    retrieval_credential_commitment: [u8; 32],
}

impl SecurityTranscript for PendingEnrollmentMailboxStatement {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.pending-enrollment-vm-mailbox.v1";

    fn transcript_payload(&self) -> Self::Payload {
        self.clone()
    }
}

impl PendingEnrollmentMailboxStatement {
    pub const VERSION: u16 = 1;

    pub fn version(&self) -> u16 {
        self.version
    }

    /// Only an independently verified original transfer and local setup proof
    /// may select the route. The credential commitment is public, never the
    /// retrieval credential itself.
    pub fn for_admitted_original(
        baseline: &VerifiedEnrollmentBaseline,
        setup: &VerifiedEnrollmentSetupPossession,
        retrieval_credential_commitment: [u8; 32],
    ) -> Result<Self, PendingEnrollmentMailboxError> {
        let manifest = baseline.manifest();
        let setup_statement = setup.statement();
        if manifest.setup.digest != setup.digest()
            || manifest.setup.nonce != setup_statement.nonce
            || manifest.invitee_authority != setup_statement.authority
            || manifest.invitee_device != setup_statement.device
            || manifest.expires_at_ms > setup_statement.expires_at_ms
            || retrieval_credential_commitment == [0; 32]
        {
            return Err(PendingEnrollmentMailboxError::Binding);
        }
        Ok(Self {
            version: Self::VERSION,
            invitation: manifest.invitation.clone(),
            ceremony: manifest.ceremony.clone(),
            manifest_digest: baseline.manifest_digest(),
            setup_digest: setup.digest(),
            subject: manifest.subject,
            provisional: setup_statement.authority,
            invitee_device: setup_statement.device,
            expires_at_ms: manifest.expires_at_ms,
            retrieval_credential_commitment,
        })
    }

    pub fn signing_context(&self) -> Result<SigningContext, PendingEnrollmentMailboxError> {
        Ok(SigningContext::message(
            self.provisional,
            Self::DOMAIN_SEPARATOR.to_owned(),
            self.transcript_bytes()?,
        ))
    }

    pub fn invitation(&self) -> &InvitationId {
        &self.invitation
    }

    pub fn ceremony(&self) -> &CeremonyId {
        &self.ceremony
    }

    pub fn manifest_digest(&self) -> [u8; 32] {
        self.manifest_digest
    }

    pub fn setup_digest(&self) -> [u8; 32] {
        self.setup_digest
    }

    pub fn provisional(&self) -> AuthorityId {
        self.provisional
    }

    pub fn subject(&self) -> AuthorityId {
        self.subject
    }

    pub fn invitee_device(&self) -> DeviceId {
        self.invitee_device
    }

    pub fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }

    pub fn retrieval_credential_commitment(&self) -> [u8; 32] {
        self.retrieval_credential_commitment
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingEnrollmentMailboxRegistration {
    pub statement: PendingEnrollmentMailboxStatement,
    pub proof: ThresholdSignature,
}

/// Sealed verified registration. It grants only temporary mailbox routing;
/// it is not a device-membership or issuer-authority capability.
/// Decoded wire data cannot mint verified route custody:
/// ```compile_fail
/// use aura_invitation::enrollment_mailbox::{PendingEnrollmentMailboxRegistration, VerifiedPendingEnrollmentMailboxRegistration};
/// fn promote(wire: PendingEnrollmentMailboxRegistration) -> VerifiedPendingEnrollmentMailboxRegistration {
///     VerifiedPendingEnrollmentMailboxRegistration { statement: wire.statement }
/// }
/// ```
pub struct VerifiedPendingEnrollmentMailboxRegistration {
    statement: PendingEnrollmentMailboxStatement,
}

impl VerifiedPendingEnrollmentMailboxRegistration {
    pub fn statement(&self) -> &PendingEnrollmentMailboxStatement {
        &self.statement
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PendingEnrollmentMailboxError {
    #[error("pending enrollment mailbox registration differs from original admission")]
    Binding,
    #[error("pending enrollment mailbox registration is outside selected validity")]
    OutsideValidity,
    #[error("pending enrollment mailbox signature does not match setup signing policy")]
    SigningPolicy,
    #[error("pending enrollment mailbox signature is invalid")]
    InvalidSignature,
    #[error("pending enrollment mailbox transcript failed: {0}")]
    Transcript(#[from] aura_signature::AuthenticationError),
    #[error("pending enrollment mailbox required transcript failed: {0}")]
    RequiredTranscript(#[from] aura_signature::RequiredTranscriptEncodingError),
    #[error("pending enrollment mailbox cryptographic verification failed: {0}")]
    Crypto(#[from] aura_core::AuraError),
    #[error("pending enrollment mailbox setup possession failed: {0}")]
    SetupPossession(#[from] crate::enrollment_setup::EnrollmentSetupError),
    #[error("pending enrollment mailbox selected physical-time observation failed: {0}")]
    Time(#[from] aura_core::effects::time::TimeError),
}

impl PendingEnrollmentMailboxRegistration {
    pub async fn verify_for_admitted_original<E: CryptoExtendedEffects + ?Sized>(
        self,
        crypto: &E,
        baseline: &VerifiedEnrollmentBaseline,
        setup: &VerifiedEnrollmentSetupPossession,
        selected_now_ms: u64,
    ) -> Result<VerifiedPendingEnrollmentMailboxRegistration, PendingEnrollmentMailboxError> {
        let expected = PendingEnrollmentMailboxStatement::for_admitted_original(
            baseline,
            setup,
            self.statement.retrieval_credential_commitment,
        )?;
        if self.statement != expected {
            return Err(PendingEnrollmentMailboxError::Binding);
        }
        let setup_statement = setup.statement();
        if selected_now_ms < setup_statement.issued_at_ms
            || selected_now_ms >= self.statement.expires_at_ms
        {
            return Err(PendingEnrollmentMailboxError::OutsideValidity);
        }
        if self.proof.epoch != setup_statement.signing_epoch
            || self.proof.public_key_package != setup_statement.public_key_package
            || self.proof.signer_count < setup_statement.threshold
            || self.proof.signer_count > setup_statement.participants
            || self.proof.signers.len() != usize::from(self.proof.signer_count)
            || self.proof.signers.windows(2).any(|pair| pair[0] >= pair[1])
            || self
                .proof
                .signers
                .iter()
                .any(|&index| index == 0 || index > setup_statement.participants)
        {
            return Err(PendingEnrollmentMailboxError::SigningPolicy);
        }
        let bytes = threshold_signing_context_transcript_bytes(
            &self.statement.signing_context()?,
            setup_statement.signing_epoch,
        )?;
        if !crypto
            .verify_signature(
                &bytes,
                &self.proof.signature,
                &setup_statement.public_key_package,
                setup_statement.signing_mode,
            )
            .await?
        {
            return Err(PendingEnrollmentMailboxError::InvalidSignature);
        }
        if setup_statement.signing_mode == SigningMode::SingleSigner
            && (self.proof.signer_count != 1 || self.proof.signers != [1])
        {
            return Err(PendingEnrollmentMailboxError::SigningPolicy);
        }
        Ok(VerifiedPendingEnrollmentMailboxRegistration {
            statement: self.statement,
        })
    }
}

/// Public, direction-specific return route. It is only a signing transcript;
/// the original issuer owner must obtain its proof from the approved enrollment
/// threshold ceremony before any broker registration is published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssuerEnrollmentReturnStatement {
    version: u16,
    invitation: InvitationId,
    ceremony: CeremonyId,
    manifest_digest: [u8; 32],
    setup_digest: [u8; 32],
    session: uuid::Uuid,
    subject: AuthorityId,
    provisional: AuthorityId,
    issuer_device: DeviceId,
    invitee_device: DeviceId,
    issuer_policy_index: u16,
    issuer_public_key_package_digest: [u8; 32],
    expires_at_ms: u64,
    retrieval_credential_commitment: [u8; 32],
}

impl SecurityTranscript for IssuerEnrollmentReturnStatement {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.issuer-enrollment-return-mailbox.v2";

    fn transcript_payload(&self) -> Self::Payload {
        self.clone()
    }
}

impl IssuerEnrollmentReturnStatement {
    pub const VERSION: u16 = 2;

    pub fn for_verified_original(
        baseline: &VerifiedEnrollmentBaseline,
        retrieval_credential_commitment: [u8; 32],
    ) -> Result<Self, PendingEnrollmentMailboxError> {
        let statement =
            Self::for_manifest_mode(baseline.manifest(), retrieval_credential_commitment, None)?;
        if statement.manifest_digest != baseline.manifest_digest() {
            return Err(PendingEnrollmentMailboxError::Binding);
        }
        Ok(statement)
    }

    /// Public identification for the original user-approved signing intent.
    /// This constructor grants no local signing authority or broker route.
    pub fn for_approved_manifest(
        manifest: &EnrollmentTrustManifest,
        retrieval_credential_commitment: [u8; 32],
    ) -> Result<Self, PendingEnrollmentMailboxError> {
        Self::for_manifest_mode(
            manifest,
            retrieval_credential_commitment,
            Some(SigningMode::Threshold),
        )
    }

    /// The original K1 export owner alone may use this constructor with its
    /// retained physical identity key. It does not grant signing authority.
    pub fn for_owned_single_signer_manifest(
        manifest: &EnrollmentTrustManifest,
        retrieval_credential_commitment: [u8; 32],
    ) -> Result<Self, PendingEnrollmentMailboxError> {
        Self::for_manifest_mode(
            manifest,
            retrieval_credential_commitment,
            Some(SigningMode::SingleSigner),
        )
    }

    fn for_manifest_mode(
        manifest: &EnrollmentTrustManifest,
        retrieval_credential_commitment: [u8; 32],
        required_mode: Option<SigningMode>,
    ) -> Result<Self, PendingEnrollmentMailboxError> {
        if retrieval_credential_commitment == [0; 32] {
            return Err(PendingEnrollmentMailboxError::Binding);
        }
        manifest
            .validate_shape()
            .map_err(|_| PendingEnrollmentMailboxError::Binding)?;
        let inventory = manifest
            .final_inventory()
            .map_err(|_| PendingEnrollmentMailboxError::Binding)?;
        // Original quorum export requires the independently held native root
        // policy and binds its group verifier into the signed transfer. Other
        // active inventory nodes cannot select this return route.
        let (index, policy) = inventory
            .iter()
            .enumerate()
            .find(|(_, policy)| policy.signing_node == aura_core::tree::NodeIndex(0))
            .ok_or(PendingEnrollmentMailboxError::SigningPolicy)?;
        if policy.epoch != manifest.final_epoch
            || required_mode.is_some_and(|mode| policy.mode != mode)
            || match policy.mode {
                SigningMode::Threshold => policy.threshold < 2,
                SigningMode::SingleSigner => {
                    policy.threshold != 1
                        || policy.participants
                            != vec![aura_core::threshold::ParticipantIdentity::Device(
                                manifest.initiator_device,
                            )]
                }
            }
            || !policy
                .participants
                .contains(&aura_core::threshold::ParticipantIdentity::Device(
                    manifest.initiator_device,
                ))
        {
            return Err(PendingEnrollmentMailboxError::SigningPolicy);
        }
        let verifier_matches = match policy.mode {
            SigningMode::Threshold => {
                aura_core::crypto::tree_signing::public_key_package_from_bytes(
                    &policy.public_key_package,
                )
                .map(|native| native.group_public_key == manifest.initiator_confirmation_verifier)
                .unwrap_or(false)
            }
            SigningMode::SingleSigner => {
                aura_core::crypto::single_signer::SingleSignerPublicKeyPackage::from_bytes(
                    &policy.public_key_package,
                )
                .map(|native| native.verifying_key() == manifest.initiator_confirmation_verifier)
                .unwrap_or(false)
            }
        };
        if !verifier_matches {
            return Err(PendingEnrollmentMailboxError::SigningPolicy);
        }
        Ok(Self {
            version: Self::VERSION,
            invitation: manifest.invitation.clone(),
            ceremony: manifest.ceremony.clone(),
            manifest_digest: aura_core::hash::hash(&manifest.transcript_bytes()?),
            setup_digest: manifest.setup.digest,
            session: crate::enrollment_mailbox_session::original_enrollment_session_id(
                &manifest.invitation,
            ),
            subject: manifest.subject,
            provisional: manifest.invitee_authority,
            issuer_device: manifest.initiator_device,
            invitee_device: manifest.invitee_device,
            issuer_policy_index: u16::try_from(index)
                .map_err(|_| PendingEnrollmentMailboxError::SigningPolicy)?,
            issuer_public_key_package_digest: aura_core::hash::hash(&policy.public_key_package),
            expires_at_ms: manifest.expires_at_ms,
            retrieval_credential_commitment,
        })
    }

    pub fn invitation(&self) -> &InvitationId {
        &self.invitation
    }
    pub fn ceremony(&self) -> &CeremonyId {
        &self.ceremony
    }
    pub fn manifest_digest(&self) -> [u8; 32] {
        self.manifest_digest
    }
    pub fn setup_digest(&self) -> [u8; 32] {
        self.setup_digest
    }
    pub fn session(&self) -> uuid::Uuid {
        self.session
    }
    pub fn subject(&self) -> AuthorityId {
        self.subject
    }
    pub fn provisional(&self) -> AuthorityId {
        self.provisional
    }
    pub fn issuer_device(&self) -> DeviceId {
        self.issuer_device
    }
    pub fn invitee_device(&self) -> DeviceId {
        self.invitee_device
    }
    pub fn issuer_policy_index(&self) -> u16 {
        self.issuer_policy_index
    }
    pub fn issuer_public_key_package_digest(&self) -> [u8; 32] {
        self.issuer_public_key_package_digest
    }
    pub fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }
    pub fn retrieval_credential_commitment(&self) -> [u8; 32] {
        self.retrieval_credential_commitment
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssuerEnrollmentReturnRegistration {
    pub statement: IssuerEnrollmentReturnStatement,
    pub mode: SigningMode,
    pub proof: ThresholdSignature,
}

/// Only exact verification against an independently admitted original baseline
/// can mint this temporary routing capability. It never authorizes membership.
pub struct VerifiedIssuerEnrollmentReturnRegistration {
    statement: IssuerEnrollmentReturnStatement,
}

impl VerifiedIssuerEnrollmentReturnRegistration {
    pub fn statement(&self) -> &IssuerEnrollmentReturnStatement {
        &self.statement
    }
}

impl IssuerEnrollmentReturnRegistration {
    /// A broker can reject malformed or unauthenticated claimed issuer keys,
    /// but cannot establish issuer trust without Bob's protected baseline.
    /// This returns no routing capability; the admitted-original verifier is
    /// mandatory before a receiver uses the registration.
    pub async fn verify_untrusted_broker_registration<
        C: CryptoExtendedEffects + ?Sized,
        T: PhysicalTimeEffects + ?Sized,
    >(
        &self,
        crypto: &C,
        time: &T,
    ) -> Result<(), PendingEnrollmentMailboxError> {
        let now = time.physical_time().await?;
        let statement = &self.statement;
        if statement.version != IssuerEnrollmentReturnStatement::VERSION
            || statement.setup_digest == [0; 32]
            || statement.manifest_digest == [0; 32]
            || statement.retrieval_credential_commitment == [0; 32]
            || now.ts_ms >= statement.expires_at_ms
            || self.proof.public_key_package.is_empty()
            || aura_core::hash::hash(&self.proof.public_key_package)
                != statement.issuer_public_key_package_digest
            || self.proof.signer_count == 0
            || self.proof.signers.len() != usize::from(self.proof.signer_count)
            || self.proof.signers.windows(2).any(|pair| pair[0] >= pair[1])
            || self.proof.signers.iter().any(|&index| index == 0)
            || match self.mode {
                SigningMode::Threshold => self.proof.signer_count < 2,
                SigningMode::SingleSigner => !self.proof.is_single_signer(),
            }
        {
            return Err(PendingEnrollmentMailboxError::SigningPolicy);
        }
        let bytes = statement.required_transcript_bytes()?;
        if !crypto
            .verify_signature(
                &bytes,
                &self.proof.signature,
                &self.proof.public_key_package,
                self.mode,
            )
            .await?
        {
            return Err(PendingEnrollmentMailboxError::InvalidSignature);
        }
        Ok(())
    }

    pub async fn verify_for_admitted_original<
        E: CryptoExtendedEffects + PhysicalTimeEffects + ?Sized,
    >(
        self,
        effects: &E,
        baseline: &VerifiedEnrollmentBaseline,
        setup: &VerifiedEnrollmentSetupPossession,
    ) -> Result<VerifiedIssuerEnrollmentReturnRegistration, PendingEnrollmentMailboxError> {
        let expected = IssuerEnrollmentReturnStatement::for_verified_original(
            baseline,
            self.statement.retrieval_credential_commitment,
        )?;
        if self.statement != expected {
            return Err(PendingEnrollmentMailboxError::Binding);
        }
        let setup_statement = setup.statement();
        if setup.digest() != baseline.manifest().setup.digest
            || setup_statement.authority != self.statement.provisional
            || setup_statement.device != self.statement.invitee_device
            || setup_statement.nonce != baseline.manifest().setup.nonce
            || self.statement.expires_at_ms > setup_statement.expires_at_ms
        {
            return Err(PendingEnrollmentMailboxError::Binding);
        }
        let selected_now = effects.physical_time().await?;
        if selected_now.ts_ms < setup_statement.issued_at_ms
            || selected_now.ts_ms >= self.statement.expires_at_ms
        {
            return Err(PendingEnrollmentMailboxError::OutsideValidity);
        }
        let manifest = baseline.manifest();
        let inventory = manifest
            .final_inventory()
            .map_err(|_| PendingEnrollmentMailboxError::Binding)?;
        let policy = inventory
            .get(usize::from(self.statement.issuer_policy_index))
            .ok_or(PendingEnrollmentMailboxError::SigningPolicy)?;
        if aura_core::hash::hash(&policy.public_key_package)
            != self.statement.issuer_public_key_package_digest
            || self.proof.public_key_package != policy.public_key_package
            || self.proof.epoch != policy.epoch
            || self.mode != policy.mode
            || match self.mode {
                SigningMode::Threshold => {
                    policy.threshold < 2
                        || usize::from(self.proof.signer_count) != policy.participants.len()
                        || self.proof.signers != (1..=self.proof.signer_count).collect::<Vec<_>>()
                }
                SigningMode::SingleSigner => {
                    policy.threshold != 1
                        || !self.proof.is_single_signer()
                        || policy.participants
                            != vec![aura_core::threshold::ParticipantIdentity::Device(
                                manifest.initiator_device,
                            )]
                }
            }
        {
            return Err(PendingEnrollmentMailboxError::SigningPolicy);
        }
        // The original approved-domain actor signs the required public
        // transcript directly, unlike later generic SigningContext requests.
        let bytes = self.statement.required_transcript_bytes()?;
        if !effects
            .verify_signature(
                &bytes,
                &self.proof.signature,
                &policy.public_key_package,
                policy.mode,
            )
            .await?
        {
            return Err(PendingEnrollmentMailboxError::InvalidSignature);
        }
        Ok(VerifiedIssuerEnrollmentReturnRegistration {
            statement: self.statement,
        })
    }
}

/// Bob's setup-key authorization for one exact reverse VM frame. A broker may
/// use this to limit enqueue, but Alice still verifies the VM frame itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentReturnEnvelopeStatement {
    version: u16,
    invitation: InvitationId,
    manifest_digest: [u8; 32],
    return_registration_digest: [u8; 32],
    session: uuid::Uuid,
    subject: AuthorityId,
    provisional: AuthorityId,
    issuer_device: DeviceId,
    invitee_device: DeviceId,
    message_id: uuid::Uuid,
    envelope_digest: [u8; 32],
    expires_at_ms: u64,
}

impl SecurityTranscript for EnrollmentReturnEnvelopeStatement {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.enrollment-return-vm-envelope.v1";

    fn transcript_payload(&self) -> Self::Payload {
        self.clone()
    }
}

impl EnrollmentReturnEnvelopeStatement {
    pub const VERSION: u16 = 1;

    pub fn for_admitted_original(
        registration: &VerifiedIssuerEnrollmentReturnRegistration,
        baseline: &VerifiedEnrollmentBaseline,
        setup: &VerifiedEnrollmentSetupPossession,
        envelope: &aura_core::effects::transport::TransportEnvelope,
    ) -> Result<Self, PendingEnrollmentMailboxError> {
        let route = registration.statement();
        let manifest = baseline.manifest();
        let setup_statement = setup.statement();
        let message_id = envelope
            .metadata
            .get("aura-message-id")
            .and_then(|raw| uuid::Uuid::parse_str(raw).ok())
            .ok_or(PendingEnrollmentMailboxError::Binding)?;
        let session = envelope
            .metadata
            .get("session-id")
            .and_then(|raw| uuid::Uuid::parse_str(raw).ok())
            .ok_or(PendingEnrollmentMailboxError::Binding)?;
        if route.invitation() != &manifest.invitation
            || route.manifest_digest() != baseline.manifest_digest()
            || route.setup_digest() != setup.digest()
            || route.session() != session
            || route.subject() != manifest.subject
            || route.provisional() != setup_statement.authority
            || route.issuer_device() != manifest.initiator_device
            || route.invitee_device() != setup_statement.device
            || manifest.setup.digest != setup.digest()
            || envelope.source != manifest.invitee_authority
            || envelope.destination != manifest.subject
            || envelope.metadata.get("aura-source-device-id")
                != Some(&manifest.invitee_device.to_string())
            || envelope.metadata.get("aura-destination-device-id")
                != Some(&manifest.initiator_device.to_string())
            || envelope
                .metadata
                .get("content-type")
                .is_none_or(|kind| kind != "application/aura-choreography")
            || envelope.receipt.as_ref().is_none_or(|receipt| {
                receipt.src != envelope.source
                    || receipt.dst != envelope.destination
                    || receipt.context != envelope.context
            })
        {
            return Err(PendingEnrollmentMailboxError::Binding);
        }
        let route_bytes = route.required_transcript_bytes()?;
        let envelope_bytes = aura_core::util::serialization::to_vec(envelope)
            .map_err(|_| PendingEnrollmentMailboxError::Binding)?;
        Ok(Self {
            version: Self::VERSION,
            invitation: manifest.invitation.clone(),
            manifest_digest: baseline.manifest_digest(),
            return_registration_digest: aura_core::hash::hash(&route_bytes),
            session,
            subject: manifest.subject,
            provisional: setup_statement.authority,
            issuer_device: manifest.initiator_device,
            invitee_device: setup_statement.device,
            message_id,
            envelope_digest: aura_core::hash::hash(&envelope_bytes),
            expires_at_ms: route.expires_at_ms(),
        })
    }

    pub fn signing_context(&self) -> Result<SigningContext, PendingEnrollmentMailboxError> {
        Ok(SigningContext::message(
            self.provisional,
            Self::DOMAIN_SEPARATOR.to_owned(),
            self.transcript_bytes()?,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentReturnEnvelopeAuthorization {
    pub statement: EnrollmentReturnEnvelopeStatement,
    pub proof: ThresholdSignature,
}

impl EnrollmentReturnEnvelopeAuthorization {
    /// Broker admission checks only wire consistency and setup-key possession.
    /// The broker does not hold the original approved baseline, so this must
    /// never mint a trusted issuer route or replace receiver verification.
    pub async fn verify_untrusted_broker_admission<
        C: CryptoExtendedEffects + ?Sized,
        T: PhysicalTimeEffects + ?Sized,
    >(
        &self,
        crypto: &C,
        time: &T,
        registration: &IssuerEnrollmentReturnRegistration,
        setup_request: &DeviceEnrollmentSetupRequest,
        envelope: &aura_core::effects::transport::TransportEnvelope,
    ) -> Result<(), PendingEnrollmentMailboxError> {
        let now = time.physical_time().await?;
        let setup = setup_request
            .clone()
            .verify_possession(crypto, now.ts_ms)
            .await?;
        let route = &registration.statement;
        let policy = setup.statement();
        let statement = &self.statement;
        let route_digest = aura_core::hash::hash(&route.required_transcript_bytes()?);
        let envelope_bytes = aura_core::util::serialization::to_vec(envelope)
            .map_err(|_| PendingEnrollmentMailboxError::Binding)?;
        if route.version != IssuerEnrollmentReturnStatement::VERSION
            || statement.version != EnrollmentReturnEnvelopeStatement::VERSION
            || route.setup_digest != setup.digest()
            || statement.invitation != route.invitation
            || statement.manifest_digest != route.manifest_digest
            || statement.return_registration_digest != route_digest
            || statement.session != route.session
            || statement.subject != route.subject
            || statement.provisional != route.provisional
            || statement.issuer_device != route.issuer_device
            || statement.invitee_device != route.invitee_device
            || statement.expires_at_ms != route.expires_at_ms
            || policy.authority != route.provisional
            || policy.device != route.invitee_device
            || now.ts_ms < policy.issued_at_ms
            || now.ts_ms >= route.expires_at_ms
            || envelope.source != route.provisional
            || envelope.destination != route.subject
            || envelope.metadata.get("aura-source-device-id")
                != Some(&route.invitee_device.to_string())
            || envelope.metadata.get("aura-destination-device-id")
                != Some(&route.issuer_device.to_string())
            || envelope.metadata.get("session-id") != Some(&route.session.to_string())
            || envelope.metadata.get("aura-message-id") != Some(&statement.message_id.to_string())
            || envelope.metadata.get("content-type")
                != Some(&"application/aura-choreography".to_owned())
            || statement.envelope_digest != aura_core::hash::hash(&envelope_bytes)
            || envelope.receipt.as_ref().is_none_or(|receipt| {
                receipt.src != envelope.source
                    || receipt.dst != envelope.destination
                    || receipt.context != envelope.context
            })
        {
            return Err(PendingEnrollmentMailboxError::Binding);
        }
        if self.proof.epoch != policy.signing_epoch
            || self.proof.public_key_package != policy.public_key_package
            || self.proof.signer_count < policy.threshold
            || self.proof.signer_count > policy.participants
            || self.proof.signers.len() != usize::from(self.proof.signer_count)
            || self.proof.signers.windows(2).any(|pair| pair[0] >= pair[1])
            || self
                .proof
                .signers
                .iter()
                .any(|&index| index == 0 || index > policy.participants)
            || (policy.signing_mode == SigningMode::SingleSigner && !self.proof.is_single_signer())
        {
            return Err(PendingEnrollmentMailboxError::SigningPolicy);
        }
        let message = threshold_signing_context_transcript_bytes(
            &statement.signing_context()?,
            policy.signing_epoch,
        )?;
        if !crypto
            .verify_signature(
                &message,
                &self.proof.signature,
                &policy.public_key_package,
                policy.signing_mode,
            )
            .await?
        {
            return Err(PendingEnrollmentMailboxError::InvalidSignature);
        }
        Ok(())
    }

    pub async fn verify_for_admitted_original<
        E: CryptoExtendedEffects + PhysicalTimeEffects + ?Sized,
    >(
        self,
        effects: &E,
        registration: &VerifiedIssuerEnrollmentReturnRegistration,
        baseline: &VerifiedEnrollmentBaseline,
        setup: &VerifiedEnrollmentSetupPossession,
        envelope: &aura_core::effects::transport::TransportEnvelope,
    ) -> Result<(), PendingEnrollmentMailboxError> {
        let expected = EnrollmentReturnEnvelopeStatement::for_admitted_original(
            registration,
            baseline,
            setup,
            envelope,
        )?;
        if self.statement != expected {
            return Err(PendingEnrollmentMailboxError::Binding);
        }
        let now = effects.physical_time().await?;
        if now.ts_ms < setup.statement().issued_at_ms || now.ts_ms >= expected.expires_at_ms {
            return Err(PendingEnrollmentMailboxError::OutsideValidity);
        }
        let policy = setup.statement();
        if self.proof.epoch != policy.signing_epoch
            || self.proof.public_key_package != policy.public_key_package
            || self.proof.signer_count < policy.threshold
            || self.proof.signer_count > policy.participants
            || self.proof.signers.len() != usize::from(self.proof.signer_count)
            || self.proof.signers.windows(2).any(|pair| pair[0] >= pair[1])
            || self
                .proof
                .signers
                .iter()
                .any(|&index| index == 0 || index > policy.participants)
            || (policy.signing_mode == SigningMode::SingleSigner && !self.proof.is_single_signer())
        {
            return Err(PendingEnrollmentMailboxError::SigningPolicy);
        }
        let message = threshold_signing_context_transcript_bytes(
            &expected.signing_context()?,
            policy.signing_epoch,
        )?;
        if !effects
            .verify_signature(
                &message,
                &self.proof.signature,
                &policy.public_key_package,
                policy.signing_mode,
            )
            .await?
        {
            return Err(PendingEnrollmentMailboxError::InvalidSignature);
        }
        Ok(())
    }
}
