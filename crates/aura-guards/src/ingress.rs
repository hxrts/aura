//! Verified remote-ingress boundary.
//!
//! This module provides the typestate value that peer-originated data must
//! cross before it is eligible for state mutation. The boundary is deliberately
//! small: protocol-specific verifiers produce complete evidence, and downstream
//! mutation APIs can require `VerifiedIngress<T>` or `VerifiedIngressEvidence`
//! instead of accepting decoded messages directly.

use aura_core::{AuthorityId, ContextId, DeviceId, Hash32, SessionId};
use serde::{Deserialize, Serialize};

/// Verification checks required before remote data can be treated as ingress
/// evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IngressVerificationCheck {
    /// The transport envelope or protocol frame authenticated its sender.
    PeerIdentity,
    /// The envelope signature/MAC binds source, destination, payload, and
    /// freshness metadata.
    EnvelopeAuthenticity,
    /// The sender has the capability required for the operation.
    CapabilityAuthorization,
    /// The message namespace, scope, and context match the expected boundary.
    NamespaceScope,
    /// The decoded schema/wire version is accepted for this protocol.
    SchemaVersion,
    /// Replay and freshness checks passed.
    ReplayFreshness,
    /// The signer is a valid member for the claimed role/epoch.
    SignerMembership,
    /// Protocol-specific proof evidence, such as Merkle or threshold evidence,
    /// has been verified.
    ProofEvidence,
}

/// Full set of checks required for production remote-ingress admission.
pub const REQUIRED_INGRESS_VERIFICATION_CHECKS: [IngressVerificationCheck; 8] = [
    IngressVerificationCheck::PeerIdentity,
    IngressVerificationCheck::EnvelopeAuthenticity,
    IngressVerificationCheck::CapabilityAuthorization,
    IngressVerificationCheck::NamespaceScope,
    IngressVerificationCheck::SchemaVersion,
    IngressVerificationCheck::ReplayFreshness,
    IngressVerificationCheck::SignerMembership,
    IngressVerificationCheck::ProofEvidence,
];

/// Source principal authenticated at the ingress boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IngressSource {
    /// Authority-scoped peer source.
    Authority(AuthorityId),
    /// Device-scoped peer source.
    Device(DeviceId),
}

/// Metadata describing the verified remote-ingress boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedIngressMetadata {
    source: IngressSource,
    context_id: ContextId,
    session_id: Option<SessionId>,
    payload_hash: Hash32,
    schema_version: u16,
}

impl VerifiedIngressMetadata {
    /// Create metadata for a verified remote-ingress value.
    #[must_use]
    pub fn new(
        source: IngressSource,
        context_id: ContextId,
        session_id: Option<SessionId>,
        payload_hash: Hash32,
        schema_version: u16,
    ) -> Self {
        Self {
            source,
            context_id,
            session_id,
            payload_hash,
            schema_version,
        }
    }

    /// Source principal whose remote message was verified.
    #[must_use]
    pub fn source(&self) -> IngressSource {
        self.source
    }

    /// Authority source, when this ingress is authority-scoped.
    #[must_use]
    pub fn source_authority(&self) -> Option<AuthorityId> {
        match self.source {
            IngressSource::Authority(authority) => Some(authority),
            IngressSource::Device(_) => None,
        }
    }

    /// Device source, when this ingress is device-scoped.
    #[must_use]
    pub fn source_device(&self) -> Option<DeviceId> {
        match self.source {
            IngressSource::Authority(_) => None,
            IngressSource::Device(device) => Some(device),
        }
    }

    /// Context where the verified message is scoped.
    #[must_use]
    pub fn context_id(&self) -> ContextId {
        self.context_id
    }

    /// Optional durable session associated with the verified message.
    #[must_use]
    pub fn session_id(&self) -> Option<SessionId> {
        self.session_id
    }

    /// Hash of the verified payload bytes or canonical transcript.
    #[must_use]
    pub fn payload_hash(&self) -> Hash32 {
        self.payload_hash
    }

    /// Accepted schema version for the decoded payload.
    #[must_use]
    pub fn schema_version(&self) -> u16 {
        self.schema_version
    }
}

/// Complete evidence that the remote-ingress checks have passed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressVerificationEvidence {
    metadata: VerifiedIngressMetadata,
    checks: Vec<IngressVerificationCheck>,
}

impl IngressVerificationEvidence {
    /// Start an evidence builder for protocol-specific ingress verification.
    #[must_use]
    pub fn builder(metadata: VerifiedIngressMetadata) -> IngressVerificationEvidenceBuilder {
        IngressVerificationEvidenceBuilder::new(metadata)
    }

    /// Create evidence after all required ingress checks have passed.
    pub fn new(
        metadata: VerifiedIngressMetadata,
        checks: impl IntoIterator<Item = IngressVerificationCheck>,
    ) -> Result<Self, IngressVerificationError> {
        let checks = checks.into_iter().collect::<Vec<_>>();
        for required in REQUIRED_INGRESS_VERIFICATION_CHECKS {
            if !checks.contains(&required) {
                return Err(IngressVerificationError::MissingCheck(required));
            }
        }

        Ok(Self { metadata, checks })
    }

    /// Create evidence with every required ingress check present.
    #[cfg(test)]
    #[must_use]
    pub fn complete(metadata: VerifiedIngressMetadata) -> Self {
        Self {
            metadata,
            checks: REQUIRED_INGRESS_VERIFICATION_CHECKS.to_vec(),
        }
    }

    /// Metadata for the verified remote-ingress value.
    #[must_use]
    pub fn metadata(&self) -> &VerifiedIngressMetadata {
        &self.metadata
    }

    /// Checks satisfied by this evidence.
    #[must_use]
    pub fn checks(&self) -> &[IngressVerificationCheck] {
        &self.checks
    }
}

/// Builder that records each required remote-ingress check only after its
/// protocol-specific predicate has passed.
#[derive(Debug, Clone)]
pub struct IngressVerificationEvidenceBuilder {
    metadata: VerifiedIngressMetadata,
    checks: Vec<IngressVerificationCheck>,
}

impl IngressVerificationEvidenceBuilder {
    /// Start collecting evidence for a decoded remote message.
    #[must_use]
    pub fn new(metadata: VerifiedIngressMetadata) -> Self {
        Self {
            metadata,
            checks: Vec::new(),
        }
    }

    fn require(
        mut self,
        check: IngressVerificationCheck,
        passed: bool,
        detail: impl Into<String>,
    ) -> Result<Self, IngressVerificationError> {
        if !passed {
            return Err(IngressVerificationError::FailedCheck {
                check,
                detail: detail.into(),
            });
        }
        if !self.checks.contains(&check) {
            self.checks.push(check);
        }
        Ok(self)
    }

    /// Record authenticated sender identity.
    pub fn peer_identity(
        self,
        passed: bool,
        detail: impl Into<String>,
    ) -> Result<Self, IngressVerificationError> {
        self.require(IngressVerificationCheck::PeerIdentity, passed, detail)
    }

    /// Record envelope signature/MAC or guard-chain receipt authenticity.
    pub fn envelope_authenticity(
        self,
        passed: bool,
        detail: impl Into<String>,
    ) -> Result<Self, IngressVerificationError> {
        self.require(
            IngressVerificationCheck::EnvelopeAuthenticity,
            passed,
            detail,
        )
    }

    /// Record operation capability authorization.
    pub fn capability_authorization(
        self,
        passed: bool,
        detail: impl Into<String>,
    ) -> Result<Self, IngressVerificationError> {
        self.require(
            IngressVerificationCheck::CapabilityAuthorization,
            passed,
            detail,
        )
    }

    /// Record namespace, context, and scope validation.
    pub fn namespace_scope(
        self,
        passed: bool,
        detail: impl Into<String>,
    ) -> Result<Self, IngressVerificationError> {
        self.require(IngressVerificationCheck::NamespaceScope, passed, detail)
    }

    /// Record accepted schema version validation.
    pub fn schema_version(
        self,
        passed: bool,
        detail: impl Into<String>,
    ) -> Result<Self, IngressVerificationError> {
        self.require(IngressVerificationCheck::SchemaVersion, passed, detail)
    }

    /// Record replay/freshness validation.
    pub fn replay_freshness(
        self,
        passed: bool,
        detail: impl Into<String>,
    ) -> Result<Self, IngressVerificationError> {
        self.require(IngressVerificationCheck::ReplayFreshness, passed, detail)
    }

    /// Record signer membership validation.
    pub fn signer_membership(
        self,
        passed: bool,
        detail: impl Into<String>,
    ) -> Result<Self, IngressVerificationError> {
        self.require(IngressVerificationCheck::SignerMembership, passed, detail)
    }

    /// Record protocol-specific proof validation.
    pub fn proof_evidence(
        self,
        passed: bool,
        detail: impl Into<String>,
    ) -> Result<Self, IngressVerificationError> {
        self.require(IngressVerificationCheck::ProofEvidence, passed, detail)
    }

    /// Finish evidence construction after all required checks have passed.
    pub fn build(self) -> Result<IngressVerificationEvidence, IngressVerificationError> {
        IngressVerificationEvidence::new(self.metadata, self.checks)
    }
}

/// Peer-originated data that has been decoded but not yet verified.
///
/// This type is intentionally not accepted by mutation APIs. Protocol-specific
/// verifiers must promote it into `VerifiedIngress<T>` after all ingress checks
/// have produced evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodedIngress<T> {
    payload: T,
    metadata: VerifiedIngressMetadata,
}

impl<T> DecodedIngress<T> {
    /// Wrap decoded remote data before verification.
    #[must_use]
    pub fn new(payload: T, metadata: VerifiedIngressMetadata) -> Self {
        Self { payload, metadata }
    }

    /// Borrow the decoded payload.
    #[must_use]
    pub fn payload(&self) -> &T {
        &self.payload
    }

    /// Borrow the claimed ingress metadata.
    #[must_use]
    pub fn metadata(&self) -> &VerifiedIngressMetadata {
        &self.metadata
    }

    /// Consume decoded remote data after complete verification evidence exists.
    pub fn verify(
        self,
        evidence: IngressVerificationEvidence,
    ) -> Result<VerifiedIngress<T>, IngressVerificationError> {
        if &self.metadata != evidence.metadata() {
            return Err(IngressVerificationError::MetadataMismatch);
        }

        Ok(VerifiedIngress::new(self.payload, evidence))
    }

    /// Consume the wrapper and return the decoded payload and claimed metadata.
    #[must_use]
    pub fn into_parts(self) -> (T, VerifiedIngressMetadata) {
        (self.payload, self.metadata)
    }
}

/// Peer-originated data that has crossed the verified ingress boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedIngress<T> {
    payload: T,
    evidence: IngressVerificationEvidence,
}

impl<T> VerifiedIngress<T> {
    /// Promote decoded remote data into verified ingress after evidence is
    /// complete.
    #[must_use]
    fn new(payload: T, evidence: IngressVerificationEvidence) -> Self {
        Self { payload, evidence }
    }

    /// Borrow the verified payload.
    #[must_use]
    pub fn payload(&self) -> &T {
        &self.payload
    }

    /// Borrow the verification evidence.
    #[must_use]
    pub fn evidence(&self) -> &IngressVerificationEvidence {
        &self.evidence
    }

    /// Consume the wrapper and return the verified payload and evidence.
    #[must_use]
    pub fn into_parts(self) -> (T, IngressVerificationEvidence) {
        (self.payload, self.evidence)
    }
}

mod sealed {
    pub trait Sealed {}
}

/// Marker trait for values that carry verified remote-ingress evidence.
///
/// The trait is sealed so external crates cannot mark decoded/raw messages as
/// verified without going through this module's typestate wrapper.
pub trait VerifiedIngressEvidence: sealed::Sealed {
    /// Return the verified ingress evidence.
    fn ingress_evidence(&self) -> &IngressVerificationEvidence;
}

impl<T> sealed::Sealed for VerifiedIngress<T> {}

impl<T> VerifiedIngressEvidence for VerifiedIngress<T> {
    fn ingress_evidence(&self) -> &IngressVerificationEvidence {
        self.evidence()
    }
}

/// Errors raised while constructing ingress verification evidence.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IngressVerificationError {
    /// A required check was not present in the evidence set.
    #[error("verified ingress evidence is missing required check {0:?}")]
    MissingCheck(IngressVerificationCheck),
    /// A required check was attempted and failed.
    #[error("verified ingress check {check:?} failed: {detail}")]
    FailedCheck {
        /// Check that failed.
        check: IngressVerificationCheck,
        /// Protocol-specific failure detail.
        detail: String,
    },
    /// Decoded-message metadata did not match the supplied verification
    /// evidence.
    #[error("decoded ingress metadata does not match verification evidence")]
    MetadataMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> VerifiedIngressMetadata {
        let source = AuthorityId::new_from_entropy([1; 32]);
        let context = ContextId::new_from_entropy([2; 32]);
        let session = Some(SessionId::new_from_entropy([3; 32]));
        let hash = Hash32::from([4; 32]);
        VerifiedIngressMetadata::new(IngressSource::Authority(source), context, session, hash, 1)
    }

    #[test]
    fn complete_evidence_is_accepted() {
        let evidence =
            IngressVerificationEvidence::new(metadata(), REQUIRED_INGRESS_VERIFICATION_CHECKS)
                .unwrap();
        assert_eq!(
            evidence.checks().len(),
            REQUIRED_INGRESS_VERIFICATION_CHECKS.len()
        );
    }

    #[test]
    fn incomplete_evidence_is_rejected() {
        let checks = REQUIRED_INGRESS_VERIFICATION_CHECKS
            .into_iter()
            .filter(|check| *check != IngressVerificationCheck::ProofEvidence);
        let error = IngressVerificationEvidence::new(metadata(), checks).unwrap_err();
        assert_eq!(
            error,
            IngressVerificationError::MissingCheck(IngressVerificationCheck::ProofEvidence)
        );
    }

    #[test]
    fn builder_rejects_failed_check() {
        let error = IngressVerificationEvidence::builder(metadata())
            .peer_identity(true, "peer authenticated")
            .unwrap()
            .envelope_authenticity(false, "bad envelope MAC")
            .unwrap_err();

        assert_eq!(
            error,
            IngressVerificationError::FailedCheck {
                check: IngressVerificationCheck::EnvelopeAuthenticity,
                detail: "bad envelope MAC".to_string(),
            }
        );
    }

    #[test]
    fn builder_requires_every_check_before_build() {
        let error = IngressVerificationEvidence::builder(metadata())
            .peer_identity(true, "peer authenticated")
            .unwrap()
            .build()
            .unwrap_err();

        assert_eq!(
            error,
            IngressVerificationError::MissingCheck(IngressVerificationCheck::EnvelopeAuthenticity)
        );
    }

    #[test]
    fn verified_ingress_carries_payload_and_evidence() {
        let evidence = IngressVerificationEvidence::complete(metadata());
        let ingress = VerifiedIngress::new("decoded-message", evidence.clone());
        assert_eq!(ingress.payload(), &"decoded-message");
        assert_eq!(ingress.ingress_evidence(), &evidence);
    }

    #[test]
    fn decoded_ingress_promotes_only_with_matching_evidence() {
        let metadata = metadata();
        let decoded = DecodedIngress::new("decoded-message", metadata.clone());
        let evidence = IngressVerificationEvidence::complete(metadata);

        let verified = decoded.verify(evidence).unwrap();

        assert_eq!(verified.payload(), &"decoded-message");
    }

    #[test]
    fn decoded_ingress_rejects_mismatched_evidence() {
        let decoded = DecodedIngress::new("decoded-message", metadata());
        let mismatched = VerifiedIngressMetadata::new(
            IngressSource::Authority(AuthorityId::new_from_entropy([9; 32])),
            ContextId::new_from_entropy([2; 32]),
            Some(SessionId::new_from_entropy([3; 32])),
            Hash32::from([4; 32]),
            1,
        );
        let evidence = IngressVerificationEvidence::complete(mismatched);

        assert_eq!(
            decoded.verify(evidence).unwrap_err(),
            IngressVerificationError::MetadataMismatch
        );
    }
}
