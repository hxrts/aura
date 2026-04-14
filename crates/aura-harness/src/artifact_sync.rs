//! Remote artifact synchronization for distributed test execution.
//!
//! Handles copying test artifacts (binaries, configs, data) to remote SSH hosts
//! before scenario execution, with checksum verification to ensure consistency.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    pub checksum_hex: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteArtifactSyncReport {
    pub required: bool,
    pub complete: bool,
    pub records: Vec<RemoteArtifactRecord>,
}

#[derive(Debug, Clone)]
struct LocalArtifactFile {
    relative_path: String,
    size_bytes: u64,
    checksum_hex: String,
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
        let (status, destination_path) = if instance.ssh_dry_run {
            let body = format!(
                "simulated remote sync for instance={} host={} source={}",
                instance.id,
                host,
                source_path.display()
            );
            fs::write(&destination_file, body)
                .with_context(|| format!("failed to write {}", destination_file.display()))?;
            (
                "simulated".to_string(),
                destination_file.display().to_string(),
            )
        } else {
            let destination_root = destination_dir.join("logs");
            fs::create_dir_all(&destination_root).with_context(|| {
                format!(
                    "failed to create remote artifact destination {}",
                    destination_root.display()
                )
            })?;

            match copy_remote_artifacts(instance, &source_path, &destination_root) {
                Ok(files) => {
                    let body = render_sync_summary(
                        &instance.id,
                        &host,
                        &source_path,
                        &destination_root,
                        &files,
                    );
                    fs::write(&destination_file, body).with_context(|| {
                        format!("failed to write {}", destination_file.display())
                    })?;
                    ("copied".to_string(), destination_root.display().to_string())
                }
                Err(error) => {
                    complete = false;
                    let body = format!(
                        "remote sync failed for instance={} host={} source={} error={error:#}",
                        instance.id,
                        host,
                        source_path.display()
                    );
                    fs::write(&destination_file, body).with_context(|| {
                        format!("failed to write {}", destination_file.display())
                    })?;
                    ("failed".to_string(), destination_file.display().to_string())
                }
            }
        };

        let checksum_hex = checksum_file(&destination_file)?;

        records.push(RemoteArtifactRecord {
            instance_id: instance.id.clone(),
            source_host: host,
            source_path: source_path.display().to_string(),
            destination_path,
            checksum_hex,
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

fn copy_remote_artifacts(
    instance: &crate::config::InstanceConfig,
    source_path: &Path,
    destination_root: &Path,
) -> Result<Vec<LocalArtifactFile>> {
    let remote_target = ssh_target(instance)?;
    let remote_source = format!("{remote_target}:{}", remote_directory_spec(source_path));
    let output = Command::new("scp")
        .args(build_scp_args(instance, &remote_source, destination_root))
        .output()
        .context("failed to spawn scp for remote artifact sync")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "scp exited with {}: {}{}",
            output.status,
            stderr.trim(),
            if stdout.trim().is_empty() {
                String::new()
            } else {
                format!(" | stdout: {}", stdout.trim())
            }
        );
    }

    collect_local_artifact_files(destination_root)
}

fn ssh_target(instance: &crate::config::InstanceConfig) -> Result<String> {
    let host = instance
        .ssh_host
        .as_deref()
        .filter(|host| !host.is_empty())
        .context("ssh instance missing ssh_host for remote artifact sync")?;

    Ok(match instance.ssh_user.as_deref() {
        Some(user) if !user.is_empty() => format!("{user}@{host}"),
        _ => host.to_string(),
    })
}

fn remote_directory_spec(source_path: &Path) -> String {
    format!("{}/.", source_path.display())
}

fn build_scp_args(
    instance: &crate::config::InstanceConfig,
    remote_source: &str,
    destination_root: &Path,
) -> Vec<String> {
    let mut args = vec!["-r".to_string()];

    if let Some(port) = instance.ssh_port {
        args.push("-P".to_string());
        args.push(port.to_string());
    }

    args.push("-o".to_string());
    args.push(format!(
        "StrictHostKeyChecking={}",
        if instance.ssh_strict_host_key_checking {
            "yes"
        } else {
            "no"
        }
    ));

    if let Some(known_hosts) = instance.ssh_known_hosts_file.as_ref() {
        args.push("-o".to_string());
        args.push(format!("UserKnownHostsFile={}", known_hosts.display()));
    }

    args.push(remote_source.to_string());
    args.push(destination_root.display().to_string());
    args
}

fn collect_local_artifact_files(root: &Path) -> Result<Vec<LocalArtifactFile>> {
    let mut files = Vec::new();
    collect_local_artifact_files_impl(root, root, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

fn collect_local_artifact_files_impl(
    root: &Path,
    current: &Path,
    files: &mut Vec<LocalArtifactFile>,
) -> Result<()> {
    for entry in fs::read_dir(current)
        .with_context(|| format!("failed to read directory {}", current.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", current.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", path.display()))?;

        if file_type.is_dir() {
            collect_local_artifact_files_impl(root, &path, files)?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        let relative_path = path
            .strip_prefix(root)
            .with_context(|| format!("failed to relativize {}", path.display()))?
            .display()
            .to_string();
        let size_bytes = entry
            .metadata()
            .with_context(|| format!("failed to stat {}", path.display()))?
            .len();
        let checksum_hex = checksum_file(&path)?;

        files.push(LocalArtifactFile {
            relative_path,
            size_bytes,
            checksum_hex,
        });
    }

    Ok(())
}

fn render_sync_summary(
    instance_id: &str,
    host: &str,
    source_path: &Path,
    destination_root: &Path,
    files: &[LocalArtifactFile],
) -> String {
    let total_bytes = files.iter().map(|file| file.size_bytes).sum::<u64>();
    let mut summary = vec![
        format!("instance={instance_id}"),
        format!("host={host}"),
        format!("source={}", source_path.display()),
        format!("destination={}", destination_root.display()),
        format!("files={}", files.len()),
        format!("bytes={total_bytes}"),
    ];

    for file in files {
        summary.push(format!(
            "file={} size={} checksum={}",
            file.relative_path, file.size_bytes, file.checksum_hex
        ));
    }

    summary.join("\n")
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
    use std::path::{Path, PathBuf};

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
        assert!(!report.records[0].checksum_hex.is_empty());
        Ok(())
    }

    #[test]
    fn build_scp_args_preserves_port_and_known_hosts() {
        let mut config = sample_run_config();
        let instance = config.instances.remove(0);
        let args = build_scp_args(
            &instance,
            "dev@example.org:/home/dev/aura/logs/.",
            Path::new("/tmp/out"),
        );

        assert!(args.iter().any(|arg| arg == "-r"));
        assert!(args.iter().any(|arg| arg == "-P"));
        assert!(args.iter().any(|arg| arg == "22"));
        assert!(args.iter().any(|arg| arg == "StrictHostKeyChecking=yes"));
    }

    #[test]
    fn collect_local_artifact_files_returns_relative_paths_and_checksums() -> Result<()> {
        let temp = tempfile::tempdir().context("tempdir failed")?;
        let nested = temp.path().join("logs/subdir");
        fs::create_dir_all(&nested).context("create nested dir failed")?;
        fs::write(temp.path().join("logs/root.txt"), "root").context("write root file failed")?;
        fs::write(nested.join("nested.txt"), "nested").context("write nested file failed")?;

        let files = collect_local_artifact_files(&temp.path().join("logs"))?;
        let paths: Vec<_> = files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect();

        assert_eq!(paths, vec!["root.txt", "subdir/nested.txt"]);
        assert!(files.iter().all(|file| !file.checksum_hex.is_empty()));
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
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
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
