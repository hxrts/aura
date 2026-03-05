//! Resource monitoring and limit enforcement during test execution.
//!
//! Samples CPU, memory, and file descriptor usage at configurable intervals,
//! reporting violations against configured limits for CI resource control.

use serde::{Deserialize, Serialize};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

use crate::config::RunConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    pub max_cpu_percent: Option<u8>,
    pub max_memory_bytes: Option<u64>,
    pub max_open_files: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceSample {
    pub stage: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub open_files: Option<u64>,
    pub violations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceGuardReport {
    pub limits: ResourceLimits,
    pub samples: Vec<ResourceSample>,
}

pub struct ResourceGuard {
    limits: ResourceLimits,
    system: System,
    samples: Vec<ResourceSample>,
}

impl ResourceGuard {
    pub fn from_run_config(config: &RunConfig) -> Self {
        Self {
            limits: ResourceLimits {
                max_cpu_percent: config.run.max_cpu_percent,
                max_memory_bytes: config.run.max_memory_bytes,
                max_open_files: config.run.max_open_files,
            },
            system: System::new_with_specifics(
                RefreshKind::nothing()
                    .with_cpu(CpuRefreshKind::everything())
                    .with_memory(MemoryRefreshKind::everything()),
            ),
            samples: Vec::new(),
        }
    }

    pub fn sample(&mut self, stage: &str) {
        self.system.refresh_cpu_all();
        self.system.refresh_memory();

        let cpu_percent = self.system.global_cpu_usage();
        let memory_bytes = self.system.used_memory();
        let open_files = current_open_files();

        let mut violations = Vec::new();
        if let Some(limit) = self.limits.max_cpu_percent {
            if cpu_percent > f32::from(limit) {
                violations.push(format!("cpu usage {cpu_percent} exceeded limit {limit}"));
            }
        }
        if let Some(limit) = self.limits.max_memory_bytes {
            if memory_bytes > limit {
                violations.push(format!(
                    "memory usage {memory_bytes} exceeded limit {limit}"
                ));
            }
        }
        if let Some(limit) = self.limits.max_open_files {
            if let Some(open_files) = open_files {
                if open_files > limit {
                    violations.push(format!("open files {open_files} exceeded limit {limit}"));
                }
            }
        }

        self.samples.push(ResourceSample {
            stage: stage.to_string(),
            cpu_percent,
            memory_bytes,
            open_files,
            violations,
        });
    }

    pub fn report(&self) -> ResourceGuardReport {
        ResourceGuardReport {
            limits: self.limits.clone(),
            samples: self.samples.clone(),
        }
    }
}

fn current_open_files() -> Option<u64> {
    count_entries("/proc/self/fd").or_else(|| count_entries("/dev/fd"))
}

fn count_entries(path: &str) -> Option<u64> {
    let entries = std::fs::read_dir(path).ok()?;
    let count = entries.filter_map(Result::ok).count();
    u64::try_from(count).ok()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::{InstanceConfig, InstanceMode, RunSection};

    #[test]
    fn resource_guard_captures_samples_and_violation_messages() {
        let mut config = sample_run_config();
        config.run.max_cpu_percent = Some(0);
        let mut guard = ResourceGuard::from_run_config(&config);
        guard.sample("start");
        let report = guard.report();
        assert_eq!(report.samples.len(), 1);
        assert!(!report.samples[0].violations.is_empty());
    }

    fn sample_run_config() -> RunConfig {
        RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "resource-guard".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(55),
                max_cpu_percent: Some(95),
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
            },
            instances: vec![InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir: PathBuf::from("/tmp/resource-guard"),
                device_id: None,
                bind_address: "127.0.0.1:49002".to_string(),
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
