//! Aura-native OTA release distribution plumbing.

use crate::core::{sync_validation_error, SyncResult};
use aura_core::effects::StorageEffects;
use aura_core::util::serialization::{from_slice, to_vec};
use aura_core::{AuthorityId, Hash32, TimeStamp};
use aura_maintenance::{
    AuraArtifactDescriptor, AuraDeterministicBuildCertificate, AuraReleaseId, AuraReleaseManifest,
    MaintenanceFact, ReleaseDistributionFact,
};

/// Storage keys and facts produced while publishing one release bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseBundlePublication {
    /// Storage key used for the manifest.
    pub manifest_key: String,
    /// Storage keys used for release artifacts.
    pub artifact_keys: Vec<String>,
    /// Storage keys used for build certificates.
    pub certificate_keys: Vec<String>,
    /// Anti-entropy-visible facts describing the published bundle.
    pub facts: Vec<MaintenanceFact>,
}

/// Stateless OTA distribution helper.
#[derive(Debug, Clone, Copy, Default)]
pub struct OtaDistributionService;

impl OtaDistributionService {
    /// Create a new OTA distribution helper.
    pub fn new() -> Self {
        Self
    }

    /// Publish a release manifest, its artifacts, and its build certificates.
    pub async fn publish_release_bundle<E: StorageEffects>(
        &self,
        storage: &E,
        authority_id: AuthorityId,
        manifest: &AuraReleaseManifest,
        artifacts: &[(AuraArtifactDescriptor, Vec<u8>)],
        certificates: &[AuraDeterministicBuildCertificate],
        published_at: TimeStamp,
    ) -> SyncResult<ReleaseBundlePublication> {
        if !manifest.release_id_matches_provenance()? {
            return Err(sync_validation_error(
                "release manifest does not match canonical provenance-derived release id",
            ));
        }

        let manifest_key = Self::manifest_storage_key(manifest.release_id);
        let manifest_bytes = to_vec(manifest)?;
        let manifest_hash = Hash32::from_value(manifest)?;
        storage
            .store(&manifest_key, manifest_bytes)
            .await
            .map_err(|e| aura_core::AuraError::storage(format!("store OTA manifest: {e}")))?;

        let mut artifact_keys = Vec::with_capacity(artifacts.len());
        let mut facts = Vec::with_capacity(1 + artifacts.len() + certificates.len());
        facts.push(MaintenanceFact::ReleaseDistribution(
            ReleaseDistributionFact::ReleaseDeclared {
                authority_id,
                series_id: manifest.series_id,
                release_id: manifest.release_id,
                manifest_hash,
                version: manifest.version,
                declared_at: published_at.clone(),
            },
        ));

        for (descriptor, bytes) in artifacts {
            Self::validate_artifact(descriptor, bytes)?;
            let key = Self::artifact_storage_key(manifest.release_id, descriptor.artifact_hash);
            storage
                .store(&key, bytes.clone())
                .await
                .map_err(|e| aura_core::AuraError::storage(format!("store OTA artifact: {e}")))?;
            artifact_keys.push(key);
            facts.push(MaintenanceFact::ReleaseDistribution(
                ReleaseDistributionFact::ArtifactAvailable {
                    authority_id,
                    release_id: manifest.release_id,
                    artifact_hash: descriptor.artifact_hash,
                    published_at: published_at.clone(),
                },
            ));
        }

        let mut certificate_keys = Vec::with_capacity(certificates.len());
        for certificate in certificates {
            Self::validate_certificate(manifest, certificate)?;
            let certificate_hash = Hash32::from_value(certificate)?;
            let key = Self::certificate_storage_key(manifest.release_id, certificate_hash);
            let bytes = to_vec(certificate)?;
            storage.store(&key, bytes).await.map_err(|e| {
                aura_core::AuraError::storage(format!("store OTA certificate: {e}"))
            })?;
            certificate_keys.push(key);
            facts.push(MaintenanceFact::ReleaseDistribution(
                ReleaseDistributionFact::BuildCertified {
                    authority_id,
                    series_id: certificate.series_id,
                    release_id: certificate.release_id,
                    certificate_hash,
                    output_hash: certificate.provenance.output_hash,
                    certified_at: published_at.clone(),
                },
            ));
        }

        Ok(ReleaseBundlePublication {
            manifest_key,
            artifact_keys,
            certificate_keys,
            facts,
        })
    }

    /// Load a published manifest from storage.
    pub async fn load_manifest<E: StorageEffects>(
        &self,
        storage: &E,
        release_id: AuraReleaseId,
    ) -> SyncResult<Option<AuraReleaseManifest>> {
        let key = Self::manifest_storage_key(release_id);
        let bytes = storage
            .retrieve(&key)
            .await
            .map_err(|e| aura_core::AuraError::storage(format!("load OTA manifest: {e}")))?;
        bytes
            .map(|payload| from_slice(&payload).map_err(Into::into))
            .transpose()
    }

    /// Load a published artifact blob from storage.
    pub async fn load_artifact<E: StorageEffects>(
        &self,
        storage: &E,
        release_id: AuraReleaseId,
        artifact_hash: Hash32,
    ) -> SyncResult<Option<Vec<u8>>> {
        let key = Self::artifact_storage_key(release_id, artifact_hash);
        storage
            .retrieve(&key)
            .await
            .map_err(|e| aura_core::AuraError::storage(format!("load OTA artifact: {e}")))
    }

    /// Load a published build certificate from storage.
    pub async fn load_certificate<E: StorageEffects>(
        &self,
        storage: &E,
        release_id: AuraReleaseId,
        certificate_hash: Hash32,
    ) -> SyncResult<Option<AuraDeterministicBuildCertificate>> {
        let key = Self::certificate_storage_key(release_id, certificate_hash);
        let bytes = storage
            .retrieve(&key)
            .await
            .map_err(|e| aura_core::AuraError::storage(format!("load OTA certificate: {e}")))?;
        bytes
            .map(|payload| from_slice(&payload).map_err(Into::into))
            .transpose()
    }

    fn validate_artifact(descriptor: &AuraArtifactDescriptor, bytes: &[u8]) -> SyncResult<()> {
        let actual_hash = Hash32::from_bytes(bytes);
        if actual_hash != descriptor.artifact_hash {
            return Err(sync_validation_error(format!(
                "artifact {} hash mismatch: expected {}, got {}",
                descriptor.name, descriptor.artifact_hash, actual_hash
            )));
        }
        if descriptor.size_bytes != bytes.len() as u64 {
            return Err(sync_validation_error(format!(
                "artifact {} size mismatch: expected {}, got {}",
                descriptor.name,
                descriptor.size_bytes,
                bytes.len()
            )));
        }
        Ok(())
    }

    fn validate_certificate(
        manifest: &AuraReleaseManifest,
        certificate: &AuraDeterministicBuildCertificate,
    ) -> SyncResult<()> {
        if certificate.series_id != manifest.series_id {
            return Err(sync_validation_error(
                "build certificate series does not match release manifest",
            ));
        }
        if certificate.release_id != manifest.release_id {
            return Err(sync_validation_error(
                "build certificate release id does not match release manifest",
            ));
        }
        if !certificate.release_id_matches_provenance()? {
            return Err(sync_validation_error(
                "build certificate does not match canonical provenance-derived release id",
            ));
        }
        Ok(())
    }

    fn manifest_storage_key(release_id: AuraReleaseId) -> String {
        format!("ota/releases/{}/manifest.cbor", release_id.as_hash())
    }

    fn artifact_storage_key(release_id: AuraReleaseId, artifact_hash: Hash32) -> String {
        format!(
            "ota/releases/{}/artifacts/{}.bin",
            release_id.as_hash(),
            artifact_hash
        )
    }

    fn certificate_storage_key(release_id: AuraReleaseId, certificate_hash: Hash32) -> String {
        format!(
            "ota/releases/{}/certificates/{}.cbor",
            release_id.as_hash(),
            certificate_hash
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::crypto::Ed25519SigningKey;
    use aura_core::time::PhysicalTime;
    use aura_core::SemanticVersion;
    use aura_maintenance::{
        AuraArtifactKind, AuraReleaseProvenance, AuraReleaseSeriesId, AuraTeeAttestation,
    };
    use aura_testkit::MemoryStorageHandler;
    use std::collections::BTreeMap;

    fn hash(byte: u8) -> Hash32 {
        Hash32([byte; 32])
    }

    fn ts(ms: u64) -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: ms,
            uncertainty: Some(5),
        })
    }

    fn provenance(seed: u8) -> AuraReleaseProvenance {
        AuraReleaseProvenance::new(
            format!("https://example.invalid/aura-sync-{seed}.git"),
            hash(seed),
            hash(seed.wrapping_add(1)),
            hash(seed.wrapping_add(2)),
            hash(seed.wrapping_add(3)),
            hash(seed.wrapping_add(4)),
        )
    }

    fn signing_material(seed: u8) -> (aura_core::Ed25519VerifyingKey, aura_core::Ed25519Signature) {
        let signing_key = Ed25519SigningKey::from_bytes([seed; 32]);
        (
            signing_key.verifying_key().unwrap(),
            signing_key.sign(b"ota-distribution-test").unwrap(),
        )
    }

    #[tokio::test]
    async fn publish_release_bundle_persists_all_distribution_objects() {
        let storage = MemoryStorageHandler::new();
        let service = OtaDistributionService::new();
        let authority_id = AuthorityId::new_from_entropy([1; 32]);
        let series_id = AuraReleaseSeriesId::new(hash(9));
        let provenance = provenance(20);
        let (manifest_key, manifest_sig) = signing_material(2);
        let artifact_bytes = b"artifact-payload".to_vec();
        let artifact = AuraArtifactDescriptor::new(
            AuraArtifactKind::Binary,
            "aura-agent-x86_64-linux",
            Some("x86_64-linux".to_string()),
            Hash32::from_bytes(&artifact_bytes),
            artifact_bytes.len() as u64,
        );
        let manifest = AuraReleaseManifest::new(
            series_id,
            SemanticVersion::new(2, 1, 0),
            authority_id,
            provenance.clone(),
            vec![artifact.clone()],
            BTreeMap::from([("channel".to_string(), "candidate".to_string())]),
            None,
            Some(1_900_000_000_000),
            manifest_key,
            manifest_sig,
        )
        .unwrap();
        let (cert_key, cert_sig) = signing_material(3);
        let certificate = AuraDeterministicBuildCertificate::new(
            series_id,
            authority_id,
            provenance,
            hash(77),
            ts(5),
            Some(AuraTeeAttestation {
                attestor_device: aura_core::DeviceId::new_from_entropy([8; 32]),
                measurement_hash: hash(78),
                evidence_hash: hash(79),
            }),
            cert_key,
            cert_sig,
        )
        .unwrap();

        let publication = service
            .publish_release_bundle(
                &storage,
                authority_id,
                &manifest,
                &[(artifact.clone(), artifact_bytes.clone())],
                std::slice::from_ref(&certificate),
                ts(10),
            )
            .await
            .unwrap();

        assert_eq!(publication.facts.len(), 3);
        assert_eq!(publication.artifact_keys.len(), 1);
        assert_eq!(publication.certificate_keys.len(), 1);

        let loaded_manifest = service
            .load_manifest(&storage, manifest.release_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded_manifest, manifest);

        let loaded_artifact = service
            .load_artifact(&storage, manifest.release_id, artifact.artifact_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded_artifact, artifact_bytes);

        let certificate_hash = Hash32::from_value(&certificate).unwrap();
        let loaded_certificate = service
            .load_certificate(&storage, manifest.release_id, certificate_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded_certificate, certificate);
    }
}
