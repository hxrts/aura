//! Public identification of initiation transcripts and conditional terminal policy.
//! Deserialization/validation establishes shape, never local signing authority.
use crate::{
    enrollment_initial_request::EnrollmentInitialRequestTranscript,
    enrollment_manifest::EnrollmentTrustManifest,
    shareable::PublicEnrollmentTransportSigningIntent,
};
use aura_core::AuraError;
use aura_signature::SecurityTranscript;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
/// The explicit domains reviewed by an enrollment signing intent.
pub enum EnrollmentSigningDomain {
    /// Canonical enrollment trust manifest.
    Manifest,
    /// Exact public v4 invitation transport transcript.
    PublicTransport,
    /// Exact deterministic initial enrollment Request control transcript.
    InitialRequest,
    /// Conditional negative terminal policy; never an initiation signature.
    NegativeTerminalPolicy,
}

/// Finite negative outcomes covered by an explicitly reviewed terminal policy.
/// This public discriminator never establishes that an outcome occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnrollmentNegativeTerminalKind {
    Rejected,
    Cancelled,
    TimedOut,
    ChoreographyFailed,
    RuntimeFailed,
    Superseded,
}

/// Public conditional policy, not a decision, signature, or runtime capability.
/// Only the original runtime owner may bind an actual first negative decision
/// to this policy and consume its retained participant signing authority.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentNegativeTerminalPolicy {
    version: u16,
    manifest_digest: [u8; 32],
    outcomes: [EnrollmentNegativeTerminalKind; 6],
}
impl EnrollmentNegativeTerminalPolicy {
    const OUTCOMES: [EnrollmentNegativeTerminalKind; 6] = [
        EnrollmentNegativeTerminalKind::Rejected,
        EnrollmentNegativeTerminalKind::Cancelled,
        EnrollmentNegativeTerminalKind::TimedOut,
        EnrollmentNegativeTerminalKind::ChoreographyFailed,
        EnrollmentNegativeTerminalKind::RuntimeFailed,
        EnrollmentNegativeTerminalKind::Superseded,
    ];
    /// Identify the finite conditional policy for this exact manifest.
    pub fn from_manifest(manifest: &EnrollmentTrustManifest) -> Result<Self, AuraError> {
        let bytes = manifest.transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source(
                "bind negative terminal policy manifest",
                Arc::new(source),
            )
        })?;
        Ok(Self {
            version: 1,
            manifest_digest: aura_core::hash::hash(&bytes),
            outcomes: Self::OUTCOMES,
        })
    }
    /// Validate exact public scope and supported outcomes; this grants no authority.
    pub fn require_manifest(&self, manifest: &EnrollmentTrustManifest) -> Result<(), AuraError> {
        if self != &Self::from_manifest(manifest)? {
            return Err(AuraError::permission_denied(
                "negative terminal policy differs from the exact approved manifest",
            ));
        }
        Ok(())
    }
    /// Read the explicitly reviewed finite outcome set.
    pub fn outcomes(&self) -> &[EnrollmentNegativeTerminalKind; 6] {
        &self.outcomes
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Versioned public declaration of initiation transcripts and conditional terminal policy.
/// Validation establishes public shape and binding, never native user approval.
pub struct EnrollmentSigningIntent {
    version: u16,
    domains: [EnrollmentSigningDomain; 4],
    manifest: EnrollmentTrustManifest,
    transport: PublicEnrollmentTransportSigningIntent,
    initial_request: EnrollmentInitialRequestTranscript,
    negative_terminal_policy: EnrollmentNegativeTerminalPolicy,
}

impl EnrollmentSigningIntent {
    const DOMAINS: [EnrollmentSigningDomain; 4] = [
        EnrollmentSigningDomain::Manifest,
        EnrollmentSigningDomain::PublicTransport,
        EnrollmentSigningDomain::InitialRequest,
        EnrollmentSigningDomain::NegativeTerminalPolicy,
    ];
    /// Construct and validate the complete domain declaration from the exact manifest and transport.
    pub fn new(
        manifest: EnrollmentTrustManifest,
        transport: PublicEnrollmentTransportSigningIntent,
    ) -> Result<Self, AuraError> {
        let initial_request = EnrollmentInitialRequestTranscript::from_manifest(&manifest)
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "derive exact original initial request",
                    Arc::new(source),
                )
            })?;
        let negative_terminal_policy = EnrollmentNegativeTerminalPolicy::from_manifest(&manifest)?;
        let intent = Self {
            version: 3,
            domains: Self::DOMAINS,
            manifest,
            transport,
            initial_request,
            negative_terminal_policy,
        };
        intent.validate()?;
        Ok(intent)
    }
    /// Borrow the exact declared enrollment trust manifest.
    pub fn manifest(&self) -> &EnrollmentTrustManifest {
        &self.manifest
    }
    /// Borrow the exact declared public transport signing transcript.
    pub fn transport(&self) -> &PublicEnrollmentTransportSigningIntent {
        &self.transport
    }
    /// Borrow the exact initial Request transcript derived from the declared manifest.
    pub fn initial_request(&self) -> &EnrollmentInitialRequestTranscript {
        &self.initial_request
    }
    /// Borrow the exact conditional policy reviewed with the initiation transcripts.
    pub fn negative_terminal_policy(&self) -> &EnrollmentNegativeTerminalPolicy {
        &self.negative_terminal_policy
    }
    /// Observe the fixed ordered set of explicitly declared initiation domains.
    pub fn domains(&self) -> &[EnrollmentSigningDomain; 4] {
        &self.domains
    }
    /// Reject unsupported versions, domains, incomplete final inventory, or transcript substitution.
    /// Successful validation does not authorize native signing or control admission.
    pub fn validate(&self) -> Result<(), AuraError> {
        if self.version != 3
            || self.domains != Self::DOMAINS
            || self.manifest.version != 3
            || self.manifest.final_inventory.is_empty()
        {
            return Err(AuraError::permission_denied(
                "signing intent does not explicitly approve current enrollment signing domains",
            ));
        }
        self.negative_terminal_policy
            .require_manifest(&self.manifest)?;
        self.manifest.validate_shape().map_err(|source| {
            AuraError::crypto_with_source(
                "validate declared enrollment signing manifest",
                Arc::new(source),
            )
        })?;
        crate::shareable::require_transport_manifest(&self.transport, &self.manifest).map_err(
            |source| {
                AuraError::crypto_with_source(
                    "validate declared public transport intent",
                    Arc::new(source),
                )
            },
        )?;
        let expected = EnrollmentInitialRequestTranscript::from_manifest(&self.manifest).map_err(
            |source| {
                AuraError::crypto_with_source("derive declared initial request", Arc::new(source))
            },
        )?;
        let expected = expected.transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("encode expected initial request", Arc::new(source))
        })?;
        let supplied = self.initial_request.transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("encode declared initial request", Arc::new(source))
        })?;
        if supplied != expected {
            return Err(AuraError::permission_denied(
                "declared initial request differs from exact approved manifest",
            ));
        }
        Ok(())
    }
}

/// Exact negative control transcript. Public data cannot prove its decision or
/// authorize signing; the runtime requires its original first-decision receipt.
#[derive(Clone, Serialize)]
pub struct EnrollmentNegativeTerminalTranscript {
    version: u16,
    manifest_digest: [u8; 32],
    decision: NegativeTerminalDecision,
    committed_ops: Option<Vec<aura_core::AttestedOp>>,
}
#[derive(Clone, Serialize)]
enum NegativeTerminalDecision {
    Failed {
        invitation: aura_core::InvitationId,
        ceremony: aura_core::CeremonyId,
        reason: EnrollmentNegativeTerminalKind,
    },
}
impl EnrollmentNegativeTerminalTranscript {
    /// Derive the exact public control bytes for a conditional negative kind.
    /// Actual decision provenance remains a separate runtime requirement.
    pub fn from_manifest(
        manifest: &EnrollmentTrustManifest,
        kind: EnrollmentNegativeTerminalKind,
    ) -> Result<Self, AuraError> {
        let policy = EnrollmentNegativeTerminalPolicy::from_manifest(manifest)?;
        Ok(Self {
            version: 2,
            manifest_digest: policy.manifest_digest,
            decision: NegativeTerminalDecision::Failed {
                invitation: manifest.invitation.clone(),
                ceremony: manifest.ceremony.clone(),
                reason: kind,
            },
            committed_ops: None,
        })
    }
}
impl SecurityTranscript for EnrollmentNegativeTerminalTranscript {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.device-enrollment-control.v2";
    fn transcript_payload(&self) -> Self {
        self.clone()
    }
}

/// Exact committed confirmation data. Construction does not establish commit
/// provenance, local approval, or signing authority; those require native owners.
#[derive(Clone, Serialize)]
pub struct EnrollmentPositiveTerminalTranscript {
    version: u16,
    manifest_digest: [u8; 32],
    decision: PositiveTerminalDecision,
    committed_ops: Option<Vec<aura_core::AttestedOp>>,
}

#[derive(Clone, Serialize)]
enum PositiveTerminalDecision {
    Committed(crate::protocol::DeviceEnrollmentConfirm),
}

impl EnrollmentPositiveTerminalTranscript {
    /// Derive only the successful confirmation for the manifest's leaf and
    /// epoch fence. Public shape checks do not authenticate either operation.
    pub fn from_manifest(
        manifest: &EnrollmentTrustManifest,
        committed_ops: &[aura_core::AttestedOp],
    ) -> Result<Self, AuraError> {
        let [leaf, fence] = committed_ops else {
            return Err(AuraError::invalid(
                "committed confirmation requires leaf and fence",
            ));
        };
        if !matches!(&leaf.op.op, aura_core::TreeOpKind::AddLeaf { leaf, .. }
            if leaf.device_id == manifest.invitee_device)
            || !matches!(&fence.op.op, aura_core::TreeOpKind::RotateEpoch { .. })
            || leaf.op.parent_epoch.value() != manifest.final_epoch
            || fence.op.parent_epoch.value() != manifest.final_epoch
        {
            return Err(AuraError::invalid(
                "committed confirmation differs from manifest transition",
            ));
        }
        let manifest_digest =
            aura_core::hash::hash(&manifest.required_transcript_bytes().map_err(|source| {
                AuraError::crypto_with_source("bind positive terminal manifest", Arc::new(source))
            })?);
        Ok(Self {
            version: 2,
            manifest_digest,
            decision: PositiveTerminalDecision::Committed(
                crate::protocol::DeviceEnrollmentConfirm {
                    invitation_id: manifest.invitation.clone(),
                    ceremony_id: manifest.ceremony.clone(),
                    established: true,
                    new_epoch: Some(manifest.pending_epoch),
                },
            ),
            committed_ops: Some(committed_ops.to_vec()),
        })
    }
}

impl SecurityTranscript for EnrollmentPositiveTerminalTranscript {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.device-enrollment-control.v2";
    fn transcript_payload(&self) -> Self {
        self.clone()
    }
}
