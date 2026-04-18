//! Pure OTA release identity, provenance, and certification types.

use aura_core::{
    to_vec, AuraError, AuthorityId, DeviceId, Ed25519Signature, Ed25519VerifyingKey, Hash32,
    SemanticVersion, SerializationError, TimeStamp,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReleaseManifestSignaturePreimage {
    domain: &'static str,
    series_id: AuraReleaseSeriesId,
    release_id: AuraReleaseId,
    version: SemanticVersion,
    author: AuthorityId,
    provenance: AuraReleaseProvenance,
    artifacts: Vec<AuraArtifactDescriptor>,
    compatibility: AuraCompatibilityManifest,
    migrations: Vec<AuraDataMigration>,
    activation_profile: AuraActivationProfile,
    metadata: BTreeMap<String, String>,
    release_notes_hash: Option<Hash32>,
    suggested_activation_time_unix_ms: Option<u64>,
    signing_key: Ed25519VerifyingKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct BuildCertificateSignaturePreimage {
    domain: &'static str,
    series_id: AuraReleaseSeriesId,
    release_id: AuraReleaseId,
    builder: AuthorityId,
    provenance: AuraReleaseProvenance,
    nix_drv_hash: Hash32,
    built_at: TimeStamp,
    tee_attestation: Option<AuraTeeAttestation>,
    signing_key: Ed25519VerifyingKey,
}

fn canonical_signature_payload<T: Serialize>(preimage: &T) -> Result<Vec<u8>, SerializationError> {
    to_vec(preimage)
}

fn canonical_signature_payload_hash<T: Serialize>(
    preimage: &T,
) -> Result<Hash32, SerializationError> {
    Hash32::from_value(preimage)
}

fn verify_canonical_signature<T: Serialize>(
    release_id_matches_provenance: bool,
    mismatch_message: &'static str,
    preimage: &T,
    signing_key: &Ed25519VerifyingKey,
    signature: &Ed25519Signature,
) -> Result<(), AuraError> {
    if !release_id_matches_provenance {
        return Err(AuraError::invalid(mismatch_message));
    }

    let payload = canonical_signature_payload(preimage)?;
    signing_key.verify(&payload, signature)
}

fn manifest_signature_preimage(manifest: &AuraReleaseManifest) -> ReleaseManifestSignaturePreimage {
    ReleaseManifestSignaturePreimage {
        domain: "aura.release.manifest.signature.v1",
        series_id: manifest.series_id,
        release_id: manifest.release_id,
        version: manifest.version,
        author: manifest.author,
        provenance: manifest.provenance.clone(),
        artifacts: manifest.artifacts.clone(),
        compatibility: manifest.compatibility.clone(),
        migrations: manifest.migrations.clone(),
        activation_profile: manifest.activation_profile.clone(),
        metadata: manifest.metadata.clone(),
        release_notes_hash: manifest.release_notes_hash,
        suggested_activation_time_unix_ms: manifest.suggested_activation_time_unix_ms,
        signing_key: manifest.signing_key,
    }
}

fn build_certificate_signature_preimage(
    certificate: &AuraDeterministicBuildCertificate,
) -> BuildCertificateSignaturePreimage {
    BuildCertificateSignaturePreimage {
        domain: "aura.release.certificate.signature.v1",
        series_id: certificate.series_id,
        release_id: certificate.release_id,
        builder: certificate.builder,
        provenance: certificate.provenance.clone(),
        nix_drv_hash: certificate.nix_drv_hash,
        built_at: certificate.built_at.clone(),
        tee_attestation: certificate.tee_attestation.clone(),
        signing_key: certificate.signing_key,
    }
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

/// Canonical platform identity for one staged artifact target.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraTargetPlatform {
    /// Rust-style target triple for the artifact.
    pub target_triple: String,
}

impl AuraTargetPlatform {
    /// Build a canonical platform identity.
    pub fn new(target_triple: impl Into<String>) -> Self {
        Self {
            target_triple: target_triple.into(),
        }
    }
}

/// Packaging format for one distributed release artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuraArtifactPackaging {
    /// A single raw binary payload.
    RawBinary,
    /// A compressed tarball with deterministic layout.
    TarZst,
    /// A compressed zip archive with deterministic layout.
    Zip,
    /// A WASM/module bundle with companion assets.
    WasmBundle,
    /// A source archive for rebuild or audit.
    SourceArchive,
}

/// Launcher entrypoint metadata for a staged artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraLauncherEntrypoint {
    /// Relative executable or startup path inside the staged artifact root.
    pub executable_relpath: String,
    /// Default arguments the launcher should pass when starting the release.
    pub default_args: Vec<String>,
}

impl AuraLauncherEntrypoint {
    /// Build a launcher entrypoint descriptor.
    pub fn new(executable_relpath: impl Into<String>, default_args: Vec<String>) -> Self {
        Self {
            executable_relpath: executable_relpath.into(),
            default_args,
        }
    }
}

/// Rollback restoration contract for one artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuraRollbackRequirement {
    /// Keep the prior staged release until post-cutover health is confirmed.
    KeepPriorReleaseStaged,
    /// Rehydrate the prior release from staged artifact storage before rollback.
    RehydratePriorRelease,
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
    pub platform: Option<AuraTargetPlatform>,
    /// Packaging format used for this artifact.
    pub packaging: AuraArtifactPackaging,
    /// Relative staged path within the release root.
    pub stage_subpath: String,
    /// Optional launcher entrypoint for executable artifacts.
    pub launcher_entrypoint: Option<AuraLauncherEntrypoint>,
    /// Rollback contract for this artifact.
    pub rollback_requirement: AuraRollbackRequirement,
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
        platform: Option<AuraTargetPlatform>,
        packaging: AuraArtifactPackaging,
        stage_subpath: impl Into<String>,
        launcher_entrypoint: Option<AuraLauncherEntrypoint>,
        rollback_requirement: AuraRollbackRequirement,
        artifact_hash: Hash32,
        size_bytes: u64,
    ) -> Self {
        Self {
            kind,
            name: name.into(),
            platform,
            packaging,
            stage_subpath: stage_subpath.into(),
            launcher_entrypoint,
            rollback_requirement,
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

/// Compatibility rules declared by a release manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraCompatibilityManifest {
    /// Mixed-version compatibility class for the release.
    pub class: AuraCompatibilityClass,
    /// Minimum legacy release that may upgrade directly, if any.
    pub minimum_legacy_release: Option<AuraReleaseId>,
    /// Optional protocol-compatibility notes keyed by namespace.
    pub protocol_requirements: BTreeMap<String, String>,
    /// Optional journal or storage migration notes keyed by domain.
    pub storage_requirements: BTreeMap<String, String>,
}

impl AuraCompatibilityManifest {
    /// Build a compatibility manifest for one release.
    pub fn new(
        class: AuraCompatibilityClass,
        minimum_legacy_release: Option<AuraReleaseId>,
        protocol_requirements: BTreeMap<String, String>,
        storage_requirements: BTreeMap<String, String>,
    ) -> Self {
        Self {
            class,
            minimum_legacy_release,
            protocol_requirements,
            storage_requirements,
        }
    }
}

/// One declared data migration required by a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraDataMigration {
    /// Stable migration identifier.
    pub migration_id: String,
    /// Human-readable migration description.
    pub description: String,
    /// Whether the migration can be rolled back safely.
    pub reversible: bool,
}

impl AuraDataMigration {
    /// Build a migration descriptor.
    pub fn new(
        migration_id: impl Into<String>,
        description: impl Into<String>,
        reversible: bool,
    ) -> Self {
        Self {
            migration_id: migration_id.into(),
            description: description.into(),
            reversible,
        }
    }
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

/// Activation requirements declared by a release manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraActivationProfile {
    /// Whether scoped threshold approval is required before cutover.
    pub require_threshold_approval: bool,
    /// Whether the target scope must satisfy an epoch fence before cutover.
    pub require_epoch_fence: bool,
    /// Named health gates the launcher/runtime must confirm after activation.
    pub health_gate_names: Vec<String>,
}

impl AuraActivationProfile {
    /// Build an activation profile for one release.
    pub fn new(
        require_threshold_approval: bool,
        require_epoch_fence: bool,
        health_gate_names: Vec<String>,
    ) -> Self {
        Self {
            require_threshold_approval,
            require_epoch_fence,
            health_gate_names,
        }
    }
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
    /// Compatibility contract for the release.
    pub compatibility: AuraCompatibilityManifest,
    /// Data migrations required by the release.
    pub migrations: Vec<AuraDataMigration>,
    /// Activation requirements and post-cutover health gates.
    pub activation_profile: AuraActivationProfile,
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
        compatibility: AuraCompatibilityManifest,
        migrations: Vec<AuraDataMigration>,
        activation_profile: AuraActivationProfile,
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
            compatibility,
            migrations,
            activation_profile,
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

    /// Canonical serialized payload that the manifest signature covers.
    pub fn signature_payload(&self) -> Result<Vec<u8>, SerializationError> {
        let preimage = manifest_signature_preimage(self);
        canonical_signature_payload(&preimage)
    }

    /// Canonical hash of the payload covered by the manifest signature.
    pub fn signature_payload_hash(&self) -> Result<Hash32, SerializationError> {
        let preimage = manifest_signature_preimage(self);
        canonical_signature_payload_hash(&preimage)
    }

    /// Verify the embedded signature against the canonical manifest payload.
    pub fn verify_signature(&self) -> Result<(), AuraError> {
        let preimage = manifest_signature_preimage(self);
        verify_canonical_signature(
            self.release_id_matches_provenance()?,
            "release manifest does not match canonical provenance-derived release id",
            &preimage,
            &self.signing_key,
            &self.signature,
        )
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

    /// Canonical serialized payload that the builder signature covers.
    pub fn signature_payload(&self) -> Result<Vec<u8>, SerializationError> {
        let preimage = build_certificate_signature_preimage(self);
        canonical_signature_payload(&preimage)
    }

    /// Canonical hash of the payload covered by the builder signature.
    pub fn signature_payload_hash(&self) -> Result<Hash32, SerializationError> {
        let preimage = build_certificate_signature_preimage(self);
        canonical_signature_payload_hash(&preimage)
    }

    /// Verify the embedded signature against the canonical certificate payload.
    pub fn verify_signature(&self) -> Result<(), AuraError> {
        let preimage = build_certificate_signature_preimage(self);
        verify_canonical_signature(
            self.release_id_matches_provenance()?,
            "build certificate does not match canonical provenance-derived release id",
            &preimage,
            &self.signing_key,
            &self.signature,
        )
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

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn series_id(seed: u8) -> AuraReleaseSeriesId {
        AuraReleaseSeriesId::new(hash(seed))
    }

    fn physical_timestamp(seed: u8) -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1_700_000_000_000 + seed as u64,
            uncertainty: Some(25),
        })
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
        let signature = signing_key.sign(b"aura-release-placeholder").unwrap();
        (signing_key, verifying_key, signature)
    }

    fn manifest_fixture(seed: u8) -> AuraReleaseManifest {
        let (signing_key, verifying_key, placeholder_signature) = signing_material(seed);
        let mut manifest = AuraReleaseManifest::new(
            series_id(seed.wrapping_add(1)),
            SemanticVersion::new(1, 2, 3),
            authority(seed.wrapping_add(2)),
            provenance(seed.wrapping_add(3)),
            vec![AuraArtifactDescriptor::new(
                AuraArtifactKind::Binary,
                "aura-agent-x86_64-linux",
                Some(AuraTargetPlatform::new("x86_64-linux")),
                AuraArtifactPackaging::TarZst,
                "bin/aura-agent",
                Some(AuraLauncherEntrypoint::new(
                    "bin/aura-agent",
                    vec!["--serve".to_string()],
                )),
                AuraRollbackRequirement::KeepPriorReleaseStaged,
                hash(seed.wrapping_add(4)),
                4096,
            )],
            AuraCompatibilityManifest::new(
                AuraCompatibilityClass::BackwardCompatible,
                None,
                BTreeMap::new(),
                BTreeMap::new(),
            ),
            vec![AuraDataMigration::new(
                "journal-v3",
                "Upgrade journal metadata encoding",
                true,
            )],
            AuraActivationProfile::new(false, false, vec!["post-stage-smoke".to_string()]),
            BTreeMap::from([("channel".to_string(), "stable".to_string())]),
            Some(hash(seed.wrapping_add(5))),
            Some(1_800_000_000_000),
            verifying_key,
            placeholder_signature,
        )
        .unwrap();
        let payload = manifest.signature_payload().unwrap();
        manifest.signature = signing_key.sign(&payload).unwrap();
        manifest
    }

    fn certificate_fixture(seed: u8) -> AuraDeterministicBuildCertificate {
        let (signing_key, verifying_key, placeholder_signature) = signing_material(seed);
        let mut cert = AuraDeterministicBuildCertificate::new(
            series_id(seed.wrapping_add(1)),
            authority(seed.wrapping_add(2)),
            provenance(seed.wrapping_add(3)),
            hash(seed.wrapping_add(4)),
            physical_timestamp(seed),
            Some(AuraTeeAttestation {
                attestor_device: DeviceId::new_from_entropy([seed.wrapping_add(5); 32]),
                measurement_hash: hash(seed.wrapping_add(6)),
                evidence_hash: hash(seed.wrapping_add(7)),
            }),
            verifying_key,
            placeholder_signature,
        )
        .unwrap();
        let payload = cert.signature_payload().unwrap();
        cert.signature = signing_key.sign(&payload).unwrap();
        cert
    }

    #[test]
    fn release_id_derivation_is_stable() {
        let series_id = series_id(10);
        let provenance = provenance(20);

        let first = AuraReleaseId::derive(series_id, &provenance).unwrap();
        let second = AuraReleaseId::derive(series_id, &provenance).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn release_id_changes_when_provenance_changes() {
        let series_id = series_id(10);

        let first = AuraReleaseId::derive(series_id, &provenance(20)).unwrap();
        let second = AuraReleaseId::derive(series_id, &provenance(21)).unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn manifest_derives_release_id_from_provenance() {
        let manifest = manifest_fixture(7);

        assert!(manifest.release_id_matches_provenance().unwrap());
    }

    #[test]
    fn certificate_derives_release_id_from_provenance() {
        let cert = certificate_fixture(11);

        assert!(cert.release_id_matches_provenance().unwrap());
    }

    #[test]
    fn manifest_signature_payload_hash_is_stable() {
        let manifest = manifest_fixture(12);

        assert_eq!(
            manifest.signature_payload_hash().unwrap(),
            manifest.signature_payload_hash().unwrap()
        );
    }

    #[test]
    fn manifest_signature_verification_detects_tampering() {
        let mut manifest = manifest_fixture(13);
        assert!(manifest.verify_signature().is_ok());

        manifest
            .metadata
            .insert("channel".to_string(), "beta".to_string());
        assert!(manifest.verify_signature().is_err());
    }

    #[test]
    fn certificate_signature_verification_detects_tampering() {
        let mut cert = certificate_fixture(14);
        assert!(cert.verify_signature().is_ok());

        cert.nix_drv_hash = hash(15);
        assert!(cert.verify_signature().is_err());
    }
}
