//! Pure OTA release identity, provenance, and certification types.

use aura_core::{
    AuthorityId, DeviceId, Ed25519Signature, Ed25519VerifyingKey, Hash32, SemanticVersion,
    SerializationError, TimeStamp,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable identifier for a long-lived Aura release line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraReleaseSeriesId(pub Hash32);

impl AuraReleaseSeriesId {
    /// Construct from a precomputed canonical hash.
    pub fn new(hash: Hash32) -> Self {
        Self(hash)
    }

    /// Access the underlying hash.
    pub fn as_hash(&self) -> &Hash32 {
        &self.0
    }
}

/// Stable identifier for one exact release in a series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraReleaseId(pub Hash32);

impl AuraReleaseId {
    /// Construct from a precomputed canonical hash.
    pub fn new(hash: Hash32) -> Self {
        Self(hash)
    }

    /// Access the underlying hash.
    pub fn as_hash(&self) -> &Hash32 {
        &self.0
    }

    /// Derive a self-certifying release id from the series and provenance.
    pub fn derive(
        series_id: AuraReleaseSeriesId,
        provenance: &AuraReleaseProvenance,
    ) -> Result<Self, SerializationError> {
        Ok(Self(Hash32::from_value(&ReleaseIdentityPreimage {
            domain: "aura.release.id.v1",
            series_id,
            provenance: provenance.clone(),
        })?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReleaseIdentityPreimage {
    domain: &'static str,
    series_id: AuraReleaseSeriesId,
    provenance: AuraReleaseProvenance,
}

/// Canonical build inputs and outputs for one release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraReleaseProvenance {
    /// Canonical source repository URL for the release inputs.
    pub source_repo_url: String,
    /// Content hash of the canonical source bundle for this release.
    pub source_bundle_hash: Hash32,
    /// Hash of the build recipe material beyond the source bundle itself.
    pub build_recipe_hash: Hash32,
    /// Expected hash of the built output artifact set.
    pub output_hash: Hash32,
    /// Hash of the canonical `flake.nix`.
    pub nix_flake_hash: Hash32,
    /// Hash of the canonical `flake.lock`.
    pub nix_flake_lock_hash: Hash32,
}

impl AuraReleaseProvenance {
    /// Convenience constructor for canonical release provenance.
    pub fn new(
        source_repo_url: impl Into<String>,
        source_bundle_hash: Hash32,
        build_recipe_hash: Hash32,
        output_hash: Hash32,
        nix_flake_hash: Hash32,
        nix_flake_lock_hash: Hash32,
    ) -> Self {
        Self {
            source_repo_url: source_repo_url.into(),
            source_bundle_hash,
            build_recipe_hash,
            output_hash,
            nix_flake_hash,
            nix_flake_lock_hash,
        }
    }

    /// Canonical provenance hash for indexing and signatures.
    pub fn canonical_hash(&self) -> Result<Hash32, SerializationError> {
        Hash32::from_value(self)
    }
}

/// Release artifact classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuraArtifactKind {
    /// Canonical release source bundle.
    SourceBundle,
    /// Signed manifest describing the release.
    ReleaseManifest,
    /// Signed deterministic build certificate.
    BuildCertificate,
    /// Executable or library payload for a target platform.
    Binary,
    /// Extra release material, such as notes or migration payloads.
    Auxiliary,
}

/// Content-addressed descriptor for a release artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraArtifactDescriptor {
    /// Artifact role within the release.
    pub kind: AuraArtifactKind,
    /// Stable human-readable artifact name.
    pub name: String,
    /// Optional target platform discriminator.
    pub platform: Option<String>,
    /// Content hash of the artifact payload.
    pub artifact_hash: Hash32,
    /// Declared artifact size in bytes.
    pub size_bytes: u64,
}

impl AuraArtifactDescriptor {
    /// Build a canonical artifact descriptor.
    pub fn new(
        kind: AuraArtifactKind,
        name: impl Into<String>,
        platform: Option<String>,
        artifact_hash: Hash32,
        size_bytes: u64,
    ) -> Self {
        Self {
            kind,
            name: name.into(),
            platform,
            artifact_hash,
            size_bytes,
        }
    }
}

/// Optional attestation evidence bound to a deterministic build certificate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraTeeAttestation {
    /// Device that produced the attestation evidence.
    pub attestor_device: DeviceId,
    /// Measurement hash for the attested execution environment.
    pub measurement_hash: Hash32,
    /// Content hash of the attestation evidence blob.
    pub evidence_hash: Hash32,
}

/// Mixed-version compatibility class for one release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuraCompatibilityClass {
    /// Backward-compatible upgrade path with no special coexistence handling.
    BackwardCompatible,
    /// Legacy and target releases may coexist during rollout.
    MixedCoexistenceAllowed,
    /// Hard cutover is scoped and requires an explicit fence/approval.
    ScopedHardFork,
    /// New and old releases cannot coexist without partition handling.
    IncompatibleWithoutPartition,
}

/// Result of one named activation health gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraHealthGate {
    /// Human-readable gate identifier.
    pub gate_name: String,
    /// Whether the gate passed.
    pub passed: bool,
}

/// Signed manifest describing one reproducible Aura release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraReleaseManifest {
    /// Release line this manifest belongs to.
    pub series_id: AuraReleaseSeriesId,
    /// Exact release identifier derived from provenance.
    pub release_id: AuraReleaseId,
    /// User-facing semantic version of the release.
    pub version: SemanticVersion,
    /// Authority asserting the manifest.
    pub author: AuthorityId,
    /// Canonical source and build provenance.
    pub provenance: AuraReleaseProvenance,
    /// Content-addressed artifacts shipped with the release.
    pub artifacts: Vec<AuraArtifactDescriptor>,
    /// Stable extra manifest metadata for policy and display.
    pub metadata: BTreeMap<String, String>,
    /// Optional content hash of release notes material.
    pub release_notes_hash: Option<Hash32>,
    /// Optional advisory activation time in Unix milliseconds.
    ///
    /// This is a rollout hint only. It is not a network-wide fence and must never
    /// be sufficient by itself to authorize activation.
    pub suggested_activation_time_unix_ms: Option<u64>,
    /// Public key corresponding to the manifest signature.
    pub signing_key: Ed25519VerifyingKey,
    /// Signature over the manifest payload.
    pub signature: Ed25519Signature,
}

impl AuraReleaseManifest {
    /// Construct a manifest with a release id derived from provenance.
    pub fn new(
        series_id: AuraReleaseSeriesId,
        version: SemanticVersion,
        author: AuthorityId,
        provenance: AuraReleaseProvenance,
        artifacts: Vec<AuraArtifactDescriptor>,
        metadata: BTreeMap<String, String>,
        release_notes_hash: Option<Hash32>,
        suggested_activation_time_unix_ms: Option<u64>,
        signing_key: Ed25519VerifyingKey,
        signature: Ed25519Signature,
    ) -> Result<Self, SerializationError> {
        let release_id = AuraReleaseId::derive(series_id, &provenance)?;
        Ok(Self {
            series_id,
            release_id,
            version,
            author,
            provenance,
            artifacts,
            metadata,
            release_notes_hash,
            suggested_activation_time_unix_ms,
            signing_key,
            signature,
        })
    }

    /// Recompute the expected release id from the manifest provenance.
    pub fn expected_release_id(&self) -> Result<AuraReleaseId, SerializationError> {
        AuraReleaseId::derive(self.series_id, &self.provenance)
    }

    /// Check whether the embedded release id matches the canonical derivation.
    pub fn release_id_matches_provenance(&self) -> Result<bool, SerializationError> {
        Ok(self.release_id == self.expected_release_id()?)
    }
}

/// Signed evidence that a builder reproduced the declared release output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraDeterministicBuildCertificate {
    /// Release line this certificate is attesting to.
    pub series_id: AuraReleaseSeriesId,
    /// Exact release identifier derived from provenance.
    pub release_id: AuraReleaseId,
    /// Builder authority that performed the deterministic build.
    pub builder: AuthorityId,
    /// Canonical source and build provenance.
    pub provenance: AuraReleaseProvenance,
    /// Hash of the realized Nix derivation used for the build.
    pub nix_drv_hash: Hash32,
    /// Time claim for when the build completed.
    pub built_at: TimeStamp,
    /// Optional TEE evidence bound to the build.
    pub tee_attestation: Option<AuraTeeAttestation>,
    /// Public key corresponding to the builder signature.
    pub signing_key: Ed25519VerifyingKey,
    /// Signature over the certificate payload.
    pub signature: Ed25519Signature,
}

impl AuraDeterministicBuildCertificate {
    /// Construct a build certificate bound to the canonical release id.
    pub fn new(
        series_id: AuraReleaseSeriesId,
        builder: AuthorityId,
        provenance: AuraReleaseProvenance,
        nix_drv_hash: Hash32,
        built_at: TimeStamp,
        tee_attestation: Option<AuraTeeAttestation>,
        signing_key: Ed25519VerifyingKey,
        signature: Ed25519Signature,
    ) -> Result<Self, SerializationError> {
        let release_id = AuraReleaseId::derive(series_id, &provenance)?;
        Ok(Self {
            series_id,
            release_id,
            builder,
            provenance,
            nix_drv_hash,
            built_at,
            tee_attestation,
            signing_key,
            signature,
        })
    }

    /// Recompute the expected release id from the certificate provenance.
    pub fn expected_release_id(&self) -> Result<AuraReleaseId, SerializationError> {
        AuraReleaseId::derive(self.series_id, &self.provenance)
    }

    /// Check whether the embedded release id matches the canonical derivation.
    pub fn release_id_matches_provenance(&self) -> Result<bool, SerializationError> {
        Ok(self.release_id == self.expected_release_id()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::PhysicalTime;
    use aura_core::Ed25519SigningKey;
    use std::collections::BTreeMap;

    fn hash(byte: u8) -> Hash32 {
        Hash32([byte; 32])
    }

    fn provenance(seed: u8) -> AuraReleaseProvenance {
        AuraReleaseProvenance::new(
            format!("https://example.invalid/aura-release-{seed}.git"),
            hash(seed),
            hash(seed.wrapping_add(1)),
            hash(seed.wrapping_add(2)),
            hash(seed.wrapping_add(3)),
            hash(seed.wrapping_add(4)),
        )
    }

    fn signing_material(seed: u8) -> (Ed25519SigningKey, Ed25519VerifyingKey, Ed25519Signature) {
        let signing_key = Ed25519SigningKey::from_bytes([seed; 32]);
        let verifying_key = signing_key.verifying_key().unwrap();
        let signature = signing_key.sign(b"aura-release-test").unwrap();
        (signing_key, verifying_key, signature)
    }

    #[test]
    fn release_id_derivation_is_stable() {
        let series_id = AuraReleaseSeriesId::new(hash(10));
        let provenance = provenance(20);

        let first = AuraReleaseId::derive(series_id, &provenance).unwrap();
        let second = AuraReleaseId::derive(series_id, &provenance).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn release_id_changes_when_provenance_changes() {
        let series_id = AuraReleaseSeriesId::new(hash(10));

        let first = AuraReleaseId::derive(series_id, &provenance(20)).unwrap();
        let second = AuraReleaseId::derive(series_id, &provenance(21)).unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn manifest_derives_release_id_from_provenance() {
        let (_, verifying_key, signature) = signing_material(7);
        let manifest = AuraReleaseManifest::new(
            AuraReleaseSeriesId::new(hash(1)),
            SemanticVersion::new(1, 2, 3),
            AuthorityId::new_from_entropy([9; 32]),
            provenance(30),
            vec![AuraArtifactDescriptor::new(
                AuraArtifactKind::Binary,
                "aura-agent-x86_64-linux",
                Some("x86_64-linux".to_string()),
                hash(99),
                4096,
            )],
            BTreeMap::from([("channel".to_string(), "stable".to_string())]),
            Some(hash(55)),
            Some(1_800_000_000_000),
            verifying_key,
            signature,
        )
        .unwrap();

        assert!(manifest.release_id_matches_provenance().unwrap());
    }

    #[test]
    fn certificate_derives_release_id_from_provenance() {
        let (_, verifying_key, signature) = signing_material(11);
        let cert = AuraDeterministicBuildCertificate::new(
            AuraReleaseSeriesId::new(hash(2)),
            AuthorityId::new_from_entropy([6; 32]),
            provenance(40),
            hash(77),
            TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1_700_000_000_000,
                uncertainty: Some(25),
            }),
            Some(AuraTeeAttestation {
                attestor_device: DeviceId::new_from_entropy([5; 32]),
                measurement_hash: hash(88),
                evidence_hash: hash(89),
            }),
            verifying_key,
            signature,
        )
        .unwrap();

        assert!(cert.release_id_matches_provenance().unwrap());
    }
}
