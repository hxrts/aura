//! Public identification of exactly three enrollment initiation transcripts.
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
/// The three explicit transcript domains authorized by an initiation intent.
pub enum EnrollmentInitiationSigningDomain {
    /// Canonical enrollment trust manifest.
    Manifest,
    /// Exact public v3 invitation transport transcript.
    PublicTransport,
    /// Exact deterministic initial enrollment Request control transcript.
    InitialRequest,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Versioned public declaration of exactly three enrollment initiation transcripts.
/// Validation establishes public shape and binding, never native user approval.
pub struct EnrollmentSigningIntent {
    version: u16,
    domains: [EnrollmentInitiationSigningDomain; 3],
    manifest: EnrollmentTrustManifest,
    transport: PublicEnrollmentTransportSigningIntent,
    initial_request: EnrollmentInitialRequestTranscript,
}

impl EnrollmentSigningIntent {
    const DOMAINS: [EnrollmentInitiationSigningDomain; 3] = [
        EnrollmentInitiationSigningDomain::Manifest,
        EnrollmentInitiationSigningDomain::PublicTransport,
        EnrollmentInitiationSigningDomain::InitialRequest,
    ];
    /// Construct and validate the three-domain declaration from the exact manifest and transport.
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
        let intent = Self {
            version: 2,
            domains: Self::DOMAINS,
            manifest,
            transport,
            initial_request,
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
    /// Observe the fixed ordered set of explicitly declared initiation domains.
    pub fn domains(&self) -> &[EnrollmentInitiationSigningDomain; 3] {
        &self.domains
    }
    /// Reject unsupported versions, domains, incomplete final inventory, or transcript substitution.
    /// Successful validation does not authorize native signing or control admission.
    pub fn validate(&self) -> Result<(), AuraError> {
        if self.version != 2
            || self.domains != Self::DOMAINS
            || self.manifest.version != 2
            || self.manifest.final_inventory.is_none()
        {
            return Err(AuraError::permission_denied(
                "signing intent does not explicitly approve current initiation domains",
            ));
        }
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
