//! Residue checks for harness runs.
//!
//! Detects leftover ports and stale lock/profile artifacts before a new run
//! starts, so repeated local and CI runs fail fast instead of compounding drift.

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::config::RunConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResidueIssue {
    pub kind: String,
    pub instance_id: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResidueReport {
    pub clean: bool,
    pub issues: Vec<ResidueIssue>,
}

pub fn check_run_residue(config: &RunConfig) -> ResidueReport {
    let mut issues = Vec::new();
    for instance in &config.instances {
        if let Err(error) = verify_bind_address_available(&instance.bind_address) {
            issues.push(ResidueIssue {
                kind: "bound_port".to_string(),
                instance_id: instance.id.clone(),
                detail: error.to_string(),
            });
        }

        if !matches!(instance.mode, crate::config::InstanceMode::Browser) {
            for lock_path in discover_lock_artifacts(&instance.data_dir) {
                issues.push(ResidueIssue {
                    kind: "stale_lock".to_string(),
                    instance_id: instance.id.clone(),
                    detail: lock_path.display().to_string(),
                });
            }
        }

        for browser_path in discover_browser_lock_artifacts(&instance.env) {
            issues.push(ResidueIssue {
                kind: "browser_profile_lock".to_string(),
                instance_id: instance.id.clone(),
                detail: browser_path.display().to_string(),
            });
        }
    }

    ResidueReport {
        clean: issues.is_empty(),
        issues,
    }
}

fn verify_bind_address_available(bind_address: &str) -> Result<()> {
    if bind_address.trim().ends_with(":0") {
        return Ok(());
    }
    let listener = TcpListener::bind(bind_address).map_err(|error| {
        anyhow!("bind address {bind_address} is still in use: {error}")
    })?;
    drop(listener);
    Ok(())
}

fn discover_lock_artifacts(root: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if !root.exists() {
        return results;
    }
    collect_named_artifacts(root, &["LOCK", "SingletonLock", ".lock"], &mut results);
    results
}

fn discover_browser_lock_artifacts(env: &[String]) -> Vec<PathBuf> {
    let browser_root = env.iter().find_map(|entry| {
        let (key, value) = entry.split_once('=')?;
        (key == "AURA_HARNESS_BROWSER_ARTIFACT_DIR").then(|| PathBuf::from(value))
    });
    let Some(browser_root) = browser_root else {
        return Vec::new();
    };
    if !browser_root.exists() {
        return Vec::new();
    }
    let mut results = Vec::new();
    collect_named_artifacts(
        &browser_root,
        &["SingletonLock", "SingletonCookie", ".org.chromium.Chromium"],
        &mut results,
    );
    results
}

fn collect_named_artifacts(root: &Path, names: &[&str], results: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if names.iter().any(|name| file_name == *name || file_name.ends_with(name)) {
            results.push(path.clone());
        }
        if path.is_dir() {
            collect_named_artifacts(&path, names, results);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check_run_residue;
    use crate::config::{InstanceConfig, InstanceMode, RunConfig, RunSection};
    use std::net::TcpListener;
    use std::path::PathBuf;

    #[test]
    fn residue_report_detects_busy_port() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("{error}"));
        let bind_address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("{error}"))
            .to_string();
        let report = check_run_residue(&sample_run_config(bind_address, None));
        assert!(!report.clean);
        assert!(report.issues.iter().any(|issue| issue.kind == "bound_port"));
        drop(listener);
    }

    #[test]
    fn residue_report_detects_stale_lock_files() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let lock_path = temp.path().join("LOCK");
        std::fs::write(&lock_path, "").unwrap_or_else(|error| panic!("{error}"));
        let report = check_run_residue(&sample_run_config(
            "127.0.0.1:0".to_string(),
            Some(temp.path().to_path_buf()),
        ));
        assert!(!report.clean);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.kind == "stale_lock" && issue.detail.ends_with("LOCK")));
    }

    fn sample_run_config(bind_address: String, data_dir: Option<PathBuf>) -> RunConfig {
        RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "residue".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(9),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
                runtime_substrate: Default::default(),
            },
            instances: vec![InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir: data_dir.unwrap_or_else(|| PathBuf::from(".tmp/residue/alice")),
                device_id: None,
                bind_address,
                demo_mode: false,
                command: Some("bash".to_string()),
                args: vec!["-lc".to_string(), "cat".to_string()],
                env: vec![],
                log_path: None,
                ssh_host: None,
                ssh_user: None,
                ssh_port: None,
                ssh_strict_host_key_checking: true,
                ssh_known_hosts_file: None,
                ssh_fingerprint: None,
                ssh_require_fingerprint: false,
                ssh_dry_run: true,
                remote_workdir: None,
                lan_discovery: None,
                tunnel: None,
            }],
        }
    }
}
