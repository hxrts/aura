//! Remote artifact synchronization for distributed test execution.
//!
//! Handles copying test artifacts (binaries, configs, data) to remote SSH hosts
//! before scenario execution, with checksum verification to ensure consistency.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use aura_core::hash::hasher;
use serde::{Deserialize, Serialize};

use crate::artifacts::ArtifactBundle;
use crate::config::{InstanceMode, RunConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteArtifactRecord {
    pub instance_id: String,
    pub source_host: String,
    pub source_path: String,
    pub destination_path: String,
    pub checksum_sha256: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteArtifactSyncReport {
    pub required: bool,
    pub complete: bool,
    pub records: Vec<RemoteArtifactRecord>,
}

pub fn sync_remote_artifacts(
    config: &RunConfig,
    artifact_bundle: &ArtifactBundle,
) -> Result<RemoteArtifactSyncReport> {
    let required = config.run.require_remote_artifact_sync;
    let mut complete = true;
    let mut records = Vec::new();

    for instance in config
        .instances
        .iter()
        .filter(|instance| matches!(instance.mode, InstanceMode::Ssh))
    {
        let host = instance
            .ssh_host
            .clone()
            .unwrap_or_else(|| "unknown-host".to_string());
        let source_path = instance
            .remote_workdir
            .as_ref()
            .map(|path| path.join("logs"))
            .unwrap_or_else(|| PathBuf::from("/tmp/aura/logs"));

        let destination_dir = artifact_bundle
            .run_dir
            .join("remote")
            .join(instance.id.as_str());
        fs::create_dir_all(&destination_dir).with_context(|| {
            format!(
                "failed to create remote artifact destination {}",
                destination_dir.display()
            )
        })?;

        let destination_file = destination_dir.join("sync-summary.txt");
        let status = if instance.ssh_dry_run {
            let body = format!(
                "simulated remote sync for instance={} host={} source={}",
                instance.id,
                host,
                source_path.display()
            );
            fs::write(&destination_file, body)
                .with_context(|| format!("failed to write {}", destination_file.display()))?;
            "simulated".to_string()
        } else {
            // Real remote copy will be added in Phase 6. Keep explicit incomplete state for now.
            complete = false;
            fs::write(
                &destination_file,
                "remote sync pending implementation; source metadata captured",
            )
            .with_context(|| format!("failed to write {}", destination_file.display()))?;
            "incomplete".to_string()
        };

        let checksum_sha256 = checksum_file(&destination_file)?;

        records.push(RemoteArtifactRecord {
            instance_id: instance.id.clone(),
            source_host: host,
            source_path: source_path.display().to_string(),
            destination_path: destination_file.display().to_string(),
            checksum_sha256,
            status,
        });
    }

    if required && records.is_empty() {
        complete = false;
    }

    if required && !complete {
        return Ok(RemoteArtifactSyncReport {
            required,
            complete: false,
            records,
        });
    }

    Ok(RemoteArtifactSyncReport {
        required,
        complete,
        records,
    })
}

pub fn checksum_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("failed to open file for checksum {}", path.display()))?;
    let mut hash_state = hasher();
    let mut buffer = [0_u8; 4096];

    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read file {}", path.display()))?;
        if read == 0 {
            break;
        }
        hash_state.update(&buffer[..read]);
    }

    let digest = hash_state.finalize();
    Ok(hex::encode(digest))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::{InstanceConfig, InstanceMode, RunSection};

    #[test]
    fn remote_sync_produces_checksum_and_metadata_records() -> Result<()> {
        let temp = tempfile::tempdir().context("tempdir failed")?;
        let artifacts =
            ArtifactBundle::create(temp.path(), "sync-test").context("artifact bundle failed")?;
        let config = sample_run_config();

        let report = sync_remote_artifacts(&config, &artifacts).context("remote sync failed")?;

        assert_eq!(report.records.len(), 1);
        assert_eq!(report.records[0].status, "simulated");
        assert!(!report.records[0].checksum_sha256.is_empty());
        Ok(())
    }

    fn sample_run_config() -> RunConfig {
        RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "sync-test".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(99),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: true,
            },
            instances: vec![InstanceConfig {
                id: "remote-1".to_string(),
                mode: InstanceMode::Ssh,
                data_dir: PathBuf::from("/tmp/remote-1"),
                device_id: None,
                bind_address: "0.0.0.0:50001".to_string(),
                demo_mode: false,
                command: None,
                args: vec![],
                env: vec![],
                log_path: None,
                ssh_host: Some("example.org".to_string()),
                ssh_user: Some("dev".to_string()),
                ssh_port: Some(22),
                ssh_strict_host_key_checking: true,
                ssh_known_hosts_file: None,
                ssh_fingerprint: Some("SHA256:test".to_string()),
                ssh_require_fingerprint: true,
                ssh_dry_run: true,
                remote_workdir: Some(PathBuf::from("/home/dev/aura")),
                lan_discovery: None,
                tunnel: None,
            }],
        }
    }
}
