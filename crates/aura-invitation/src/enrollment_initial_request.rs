//! Public exact initial enrollment request transcript. This value is data, not
//! admission, signing custody, local approval, or a verified control capability.
use crate::enrollment_manifest::EnrollmentTrustManifest;
use crate::protocol::DeviceEnrollmentRequest;
use aura_signature::SecurityTranscript;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
enum InitialDecision {
    Request(DeviceEnrollmentRequest),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Exact deterministic initial Request control transcript derived from a manifest.
/// This public value grants no runtime control admission or signing permission.
pub struct EnrollmentInitialRequestTranscript {
    version: u16,
    manifest_digest: [u8; 32],
    decision: InitialDecision,
    committed_ops: Option<Vec<aura_core::AttestedOp>>,
}
impl EnrollmentInitialRequestTranscript {
    /// Derive only the initial Request domain, with no committed operations.
    /// All request fields and the manifest digest come from the supplied exact manifest.
    pub fn from_manifest(manifest: &EnrollmentTrustManifest) -> aura_signature::Result<Self> {
        Ok(Self {
            version: 2,
            manifest_digest: aura_core::hash::hash(&manifest.transcript_bytes()?),
            decision: InitialDecision::Request(DeviceEnrollmentRequest {
                invitation_id: manifest.invitation.clone(),
                subject_authority: manifest.subject,
                ceremony_id: manifest.ceremony.clone(),
                pending_epoch: manifest.pending_epoch,
                device_id: manifest.invitee_device,
            }),
            committed_ops: None,
        })
    }
}
impl SecurityTranscript for EnrollmentInitialRequestTranscript {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.device-enrollment-control.v2";
    fn transcript_payload(&self) -> Self {
        self.clone()
    }
}
