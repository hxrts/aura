//! Deterministic seed generation for reproducible test runs.
//!
//! Derives per-instance and per-component seeds from a root seed, enabling
//! exact replay of randomized test scenarios across different environments.

use std::collections::BTreeMap;

use aura_core::hash::hasher;
use serde::{Deserialize, Serialize};

use crate::config::RunConfig;

pub const DEFAULT_HARNESS_SEED: u64 = 0xA11A_2026;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedBundle {
    pub run_seed: u64,
    pub scenario_seed: u64,
    pub fault_seed: u64,
    pub instance_seeds: BTreeMap<String, u64>,
}

pub fn build_seed_bundle(config: &RunConfig) -> SeedBundle {
    let run_seed = config.run.seed.unwrap_or(DEFAULT_HARNESS_SEED);
    let scenario_seed = derive_seed(run_seed, "scenario", "global");
    let fault_seed = derive_seed(run_seed, "fault", "global");

    let mut instance_seeds = BTreeMap::new();
    for instance in &config.instances {
        instance_seeds.insert(
            instance.id.clone(),
            derive_seed(run_seed, "instance", &instance.id),
        );
    }

    SeedBundle {
        run_seed,
        scenario_seed,
        fault_seed,
        instance_seeds,
    }
}

fn derive_seed(root: u64, scope: &str, key: &str) -> u64 {
    let mut hash_state = hasher();
    hash_state.update(&root.to_le_bytes());
    hash_state.update(scope.as_bytes());
    hash_state.update(key.as_bytes());
    let digest = hash_state.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_le_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::{InstanceConfig, InstanceMode, RunSection};

    #[test]
    fn seed_bundle_is_deterministic_for_same_config() {
        let config = sample_config();
        let first = build_seed_bundle(&config);
        let second = build_seed_bundle(&config);
        assert_eq!(first.run_seed, second.run_seed);
        assert_eq!(first.scenario_seed, second.scenario_seed);
        assert_eq!(first.instance_seeds, second.instance_seeds);
    }

    fn sample_config() -> RunConfig {
        RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "seed-test".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(12345),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
            },
            instances: vec![InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir: PathBuf::from("/tmp/seed-test"),
                device_id: None,
                bind_address: "127.0.0.1:49001".to_string(),
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
