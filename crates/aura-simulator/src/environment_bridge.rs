//! Aura-owned bridge from simulator-local state into Telltale 11-style
//! environment concepts.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Schema identifier for environment snapshot artifacts.
pub const AURA_ENVIRONMENT_SNAPSHOT_ARTIFACT_SCHEMA_V1: &str = "aura.environment.snapshot.v1";
/// Schema identifier for environment trace artifacts.
pub const AURA_ENVIRONMENT_TRACE_ARTIFACT_SCHEMA_V1: &str = "aura.environment.trace.v1";

/// Environment-facing mobility profile derived from adaptive-privacy movement state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraMobilityProfile {
    pub profile_id: String,
    pub clusters: Vec<String>,
    pub home_locality_bias_millis: u64,
    pub neighborhood_locality_bias_millis: u64,
    pub recorded_at_tick: u64,
}

/// Environment-facing link-admission observation derived from sync opportunities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraLinkAdmissionObservation {
    pub profile_id: String,
    pub density: String,
    pub peers: Vec<String>,
    pub recorded_at_tick: u64,
}

/// Environment-facing node-capability observation derived from provider saturation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraNodeCapabilityObservation {
    pub provider: String,
    pub queue_depth: usize,
    pub utilization_per_mille: u64,
    pub recorded_at_tick: u64,
}

/// Snapshot of Aura environment concepts aligned to Telltale 11 vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuraEnvironmentSnapshot {
    pub mobility_profiles: Vec<AuraMobilityProfile>,
    pub link_admissions: Vec<AuraLinkAdmissionObservation>,
    pub node_capabilities: Vec<AuraNodeCapabilityObservation>,
}

/// Trace entry for the Aura environment bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuraEnvironmentTraceEntry {
    MobilityProfileConfigured(AuraMobilityProfile),
    LinkAdmissionObserved(AuraLinkAdmissionObservation),
    NodeCapabilityObserved(AuraNodeCapabilityObservation),
}

/// Deterministic trace over Aura environment bridge updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuraEnvironmentTrace {
    pub entries: Vec<AuraEnvironmentTraceEntry>,
}

/// Stable simulator run metadata shared by environment artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraEnvironmentArtifactMetadataV1 {
    pub scenario_name: String,
    pub seed: u64,
}

/// Stable on-disk snapshot artifact for one simulator scenario run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraEnvironmentSnapshotArtifactV1 {
    pub schema_version: String,
    pub metadata: AuraEnvironmentArtifactMetadataV1,
    pub snapshot: AuraEnvironmentSnapshot,
}

/// Stable on-disk trace artifact for one simulator scenario run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraEnvironmentTraceArtifactV1 {
    pub schema_version: String,
    pub metadata: AuraEnvironmentArtifactMetadataV1,
    pub trace: AuraEnvironmentTrace,
}

/// Snapshot plus trace captured from the environment bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuraEnvironmentArtifacts {
    pub snapshot: AuraEnvironmentSnapshot,
    pub trace: AuraEnvironmentTrace,
}

/// On-disk paths for one scenario run's environment artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuraEnvironmentArtifactPaths {
    pub snapshot_path: PathBuf,
    pub trace_path: PathBuf,
}

/// Errors emitted while materializing environment artifacts.
#[derive(Debug, thiserror::Error)]
pub enum AuraEnvironmentArtifactError {
    #[error("failed serializing environment artifact: {message}")]
    Serialize { message: String },
    #[error("failed writing environment artifact to {path}: {message}")]
    WriteArtifact { path: String, message: String },
}

/// Aura-owned environment bridge state.
#[derive(Debug, Clone, Default)]
pub struct AuraEnvironmentBridge {
    mobility_profiles: HashMap<String, AuraMobilityProfile>,
    link_admissions: HashMap<String, AuraLinkAdmissionObservation>,
    node_capabilities: HashMap<String, AuraNodeCapabilityObservation>,
    trace: AuraEnvironmentTrace,
}

impl AuraEnvironmentBridge {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn configure_mobility_profile(
        &mut self,
        profile_id: String,
        clusters: Vec<String>,
        home_locality_bias_millis: u64,
        neighborhood_locality_bias_millis: u64,
        recorded_at_tick: u64,
    ) -> AuraMobilityProfile {
        let profile = AuraMobilityProfile {
            profile_id,
            clusters,
            home_locality_bias_millis,
            neighborhood_locality_bias_millis,
            recorded_at_tick,
        };
        self.mobility_profiles
            .insert(profile.profile_id.clone(), profile.clone());
        self.trace
            .entries
            .push(AuraEnvironmentTraceEntry::MobilityProfileConfigured(
                profile.clone(),
            ));
        profile
    }

    pub fn observe_link_admission(
        &mut self,
        profile_id: String,
        density: String,
        peers: Vec<String>,
        recorded_at_tick: u64,
    ) -> AuraLinkAdmissionObservation {
        let observation = AuraLinkAdmissionObservation {
            profile_id,
            density,
            peers,
            recorded_at_tick,
        };
        self.link_admissions
            .insert(observation.profile_id.clone(), observation.clone());
        self.trace
            .entries
            .push(AuraEnvironmentTraceEntry::LinkAdmissionObserved(
                observation.clone(),
            ));
        observation
    }

    pub fn observe_node_capability(
        &mut self,
        provider: String,
        queue_depth: usize,
        utilization_per_mille: u64,
        recorded_at_tick: u64,
    ) -> AuraNodeCapabilityObservation {
        let observation = AuraNodeCapabilityObservation {
            provider,
            queue_depth,
            utilization_per_mille,
            recorded_at_tick,
        };
        self.node_capabilities
            .insert(observation.provider.clone(), observation.clone());
        self.trace
            .entries
            .push(AuraEnvironmentTraceEntry::NodeCapabilityObserved(
                observation.clone(),
            ));
        observation
    }

    #[must_use]
    pub fn snapshot(&self) -> AuraEnvironmentSnapshot {
        let mut mobility_profiles = self.mobility_profiles.values().cloned().collect::<Vec<_>>();
        mobility_profiles.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));

        let mut link_admissions = self.link_admissions.values().cloned().collect::<Vec<_>>();
        link_admissions.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));

        let mut node_capabilities = self.node_capabilities.values().cloned().collect::<Vec<_>>();
        node_capabilities.sort_by(|left, right| left.provider.cmp(&right.provider));

        AuraEnvironmentSnapshot {
            mobility_profiles,
            link_admissions,
            node_capabilities,
        }
    }

    #[must_use]
    pub fn trace(&self) -> &AuraEnvironmentTrace {
        &self.trace
    }

    #[must_use]
    pub fn capture_artifacts(&self) -> AuraEnvironmentArtifacts {
        AuraEnvironmentArtifacts {
            snapshot: self.snapshot(),
            trace: self.trace.clone(),
        }
    }
}

/// Materialize stable environment artifacts under the simulator artifacts root.
pub fn write_environment_artifacts(
    artifacts_root: &Path,
    scenario_name: &str,
    seed: u64,
    artifacts: &AuraEnvironmentArtifacts,
) -> Result<AuraEnvironmentArtifactPaths, AuraEnvironmentArtifactError> {
    let scenario_dir = artifacts_root
        .join("scenario-runs")
        .join(format!("{}-seed-{seed}", scenario_slug(scenario_name)));
    let metadata = AuraEnvironmentArtifactMetadataV1 {
        scenario_name: scenario_name.to_string(),
        seed,
    };
    let snapshot_path = scenario_dir.join("environment_snapshot.json");
    let trace_path = scenario_dir.join("environment_trace.json");

    write_json_artifact(
        &snapshot_path,
        &AuraEnvironmentSnapshotArtifactV1 {
            schema_version: AURA_ENVIRONMENT_SNAPSHOT_ARTIFACT_SCHEMA_V1.to_string(),
            metadata: metadata.clone(),
            snapshot: artifacts.snapshot.clone(),
        },
    )?;
    write_json_artifact(
        &trace_path,
        &AuraEnvironmentTraceArtifactV1 {
            schema_version: AURA_ENVIRONMENT_TRACE_ARTIFACT_SCHEMA_V1.to_string(),
            metadata,
            trace: artifacts.trace.clone(),
        },
    )?;

    Ok(AuraEnvironmentArtifactPaths {
        snapshot_path,
        trace_path,
    })
}

fn write_json_artifact<T>(path: &Path, artifact: &T) -> Result<(), AuraEnvironmentArtifactError>
where
    T: Serialize,
{
    let payload = serde_json::to_vec_pretty(artifact).map_err(|error| {
        AuraEnvironmentArtifactError::Serialize {
            message: error.to_string(),
        }
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            AuraEnvironmentArtifactError::WriteArtifact {
                path: path.display().to_string(),
                message: error.to_string(),
            }
        })?;
    }
    std::fs::write(path, payload).map_err(|error| AuraEnvironmentArtifactError::WriteArtifact {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}

fn scenario_slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut previous_was_separator = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            slug.push('_');
            previous_was_separator = true;
        }
    }
    slug.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn environment_bridge_captures_bridge_first_decisions() {
        let mut bridge = AuraEnvironmentBridge::new();

        let mobility = bridge.configure_mobility_profile(
            "profile-a".to_string(),
            vec!["cluster-1".to_string()],
            750,
            250,
            7,
        );
        let capability = bridge.observe_node_capability("provider-a".to_string(), 4, 900, 8);
        let admission = bridge.observe_link_admission(
            "profile-a".to_string(),
            "sparse".to_string(),
            vec!["alice".to_string(), "bob".to_string()],
            9,
        );

        assert_eq!(mobility.profile_id, "profile-a");
        assert_eq!(capability.provider, "provider-a");
        assert_eq!(admission.density, "sparse");

        let artifacts = bridge.capture_artifacts();
        assert_eq!(artifacts.snapshot.mobility_profiles.len(), 1);
        assert_eq!(artifacts.snapshot.node_capabilities.len(), 1);
        assert_eq!(artifacts.snapshot.link_admissions.len(), 1);
        assert_eq!(artifacts.trace.entries.len(), 3);
    }

    #[test]
    fn environment_bridge_writes_stable_artifacts_to_disk() {
        let dir = tempdir().expect("tempdir");
        let artifacts = AuraEnvironmentArtifacts {
            snapshot: AuraEnvironmentSnapshot {
                mobility_profiles: vec![AuraMobilityProfile {
                    profile_id: "profile-a".to_string(),
                    clusters: vec!["cluster-1".to_string()],
                    home_locality_bias_millis: 800,
                    neighborhood_locality_bias_millis: 200,
                    recorded_at_tick: 5,
                }],
                link_admissions: vec![],
                node_capabilities: vec![],
            },
            trace: AuraEnvironmentTrace {
                entries: vec![AuraEnvironmentTraceEntry::MobilityProfileConfigured(
                    AuraMobilityProfile {
                        profile_id: "profile-a".to_string(),
                        clusters: vec!["cluster-1".to_string()],
                        home_locality_bias_millis: 800,
                        neighborhood_locality_bias_millis: 200,
                        recorded_at_tick: 5,
                    },
                )],
            },
        };

        let paths = write_environment_artifacts(dir.path(), "Parity Example", 42, &artifacts)
            .expect("write environment artifacts");

        let snapshot: AuraEnvironmentSnapshotArtifactV1 =
            serde_json::from_slice(&std::fs::read(&paths.snapshot_path).expect("read snapshot"))
                .expect("decode snapshot");
        let trace: AuraEnvironmentTraceArtifactV1 =
            serde_json::from_slice(&std::fs::read(&paths.trace_path).expect("read trace"))
                .expect("decode trace");

        assert_eq!(
            snapshot.schema_version,
            AURA_ENVIRONMENT_SNAPSHOT_ARTIFACT_SCHEMA_V1
        );
        assert_eq!(
            trace.schema_version,
            AURA_ENVIRONMENT_TRACE_ARTIFACT_SCHEMA_V1
        );
        assert_eq!(snapshot.metadata.seed, 42);
        assert_eq!(trace.metadata.scenario_name, "Parity Example");
        assert_eq!(snapshot.snapshot.mobility_profiles.len(), 1);
        assert_eq!(trace.trace.entries.len(), 1);
    }
}
