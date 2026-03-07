use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};

use crate::config::{InstanceMode, RunConfig};
use crate::determinism::{build_seed_bundle, DEFAULT_HARNESS_SEED};
use crate::workspace_root;

const DETERMINISTIC_PORT_MIN: u16 = 41_000;
const DETERMINISTIC_PORT_SPAN: u16 = 20_000;

pub fn materialize_run_config(mut config: RunConfig, _config_path: &Path) -> Result<RunConfig> {
    let root = run_root(&config);
    config.run.artifact_dir = Some(root.clone());

    let seed_bundle = build_seed_bundle(&config);
    let mut used_ports = HashSet::new();

    for instance in &mut config.instances {
        let instance_root = root.join("instances").join(&instance.id);
        if !instance.data_dir.is_absolute() {
            instance.data_dir = instance_root.join("state");
        }

        ensure_env_value(&mut instance.env, "AURA_HARNESS_MODE", "1");
        ensure_env_value(
            &mut instance.env,
            "AURA_HARNESS_PROFILE",
            "deterministic",
        );

        if matches!(instance.mode, InstanceMode::Browser) {
            ensure_env_path(
                &mut instance.env,
                "AURA_HARNESS_BROWSER_ARTIFACT_DIR",
                instance_root.join("playwright-artifacts"),
            );
        }

        instance.bind_address = materialize_bind_address(
            &instance.bind_address,
            seed_bundle
                .instance_seeds
                .get(&instance.id)
                .copied()
                .unwrap_or(DEFAULT_HARNESS_SEED),
            &mut used_ports,
        )?;
    }

    Ok(config)
}

fn run_root(config: &RunConfig) -> PathBuf {
    let explicit = config
        .run
        .artifact_dir
        .clone()
        .unwrap_or_else(|| default_run_root(config));
    if explicit.is_absolute() {
        explicit
    } else {
        workspace_root().join(explicit)
    }
}

fn default_run_root(config: &RunConfig) -> PathBuf {
    let run_seed = config.run.seed.unwrap_or(DEFAULT_HARNESS_SEED);
    workspace_root()
        .join(".tmp")
        .join("harness")
        .join("runs")
        .join(format!("{}-{}", sanitize_segment(&config.run.name), run_seed))
}

fn sanitize_segment(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '-' | '_' | ' ') {
            output.push('-');
        }
    }
    let output = output.trim_matches('-');
    if output.is_empty() {
        "run".to_string()
    } else {
        output.to_string()
    }
}

fn ensure_env_path(env: &mut Vec<String>, key: &str, value: PathBuf) {
    if env
        .iter()
        .any(|entry| entry.split_once('=').is_some_and(|(candidate, _)| candidate == key))
    {
        return;
    }
    env.push(format!("{key}={}", value.display()));
}

fn ensure_env_value(env: &mut Vec<String>, key: &str, value: &str) {
    if env
        .iter()
        .any(|entry| entry.split_once('=').is_some_and(|(candidate, _)| candidate == key))
    {
        return;
    }
    env.push(format!("{key}={value}"));
}

fn materialize_bind_address(
    bind_address: &str,
    instance_seed: u64,
    used_ports: &mut HashSet<u16>,
) -> Result<String> {
    let (host, port) = split_host_port(bind_address)?;
    if port != 0 {
        used_ports.insert(port);
        return Ok(bind_address.to_string());
    }

    let base = DETERMINISTIC_PORT_MIN as u64 + (instance_seed % u64::from(DETERMINISTIC_PORT_SPAN));
    for offset in 0..DETERMINISTIC_PORT_SPAN {
        let candidate = DETERMINISTIC_PORT_MIN
            + (((base as u16 - DETERMINISTIC_PORT_MIN) + offset) % DETERMINISTIC_PORT_SPAN);
        if used_ports.insert(candidate) {
            return Ok(format!("{host}:{candidate}"));
        }
    }

    bail!("unable to allocate deterministic port for {bind_address}")
}

fn split_host_port(bind_address: &str) -> Result<(&str, u16)> {
    let (host, port) = bind_address.rsplit_once(':').ok_or_else(|| {
        anyhow!("bind_address must be in host:port form, got {bind_address}")
    })?;
    let port = port
        .parse::<u16>()
        .map_err(|error| anyhow!("invalid bind_address port in {bind_address}: {error}"))?;
    Ok((host, port))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::materialize_run_config;
    use crate::config::{InstanceConfig, InstanceMode, RunConfig, RunSection};

    #[test]
    fn materialize_assigns_deterministic_run_root_and_browser_artifacts() {
        let config = sample_run_config();
        let materialized = materialize_run_config(config, PathBuf::from("run.toml").as_path())
            .expect("materialization should succeed");

        let run_root = materialized
            .run
            .artifact_dir
            .clone()
            .expect("artifact_dir should be assigned");
        assert!(run_root.is_absolute());
        assert!(run_root.to_string_lossy().contains(".tmp/harness/runs"));

        let browser = materialized
            .instances
            .iter()
            .find(|instance| matches!(instance.mode, InstanceMode::Browser))
            .expect("browser instance should exist");
        assert!(browser.data_dir.is_absolute());
        assert!(browser
            .env
            .iter()
            .any(|entry| entry.contains("AURA_HARNESS_BROWSER_ARTIFACT_DIR=")));
        assert!(materialized.instances.iter().all(|instance| instance
            .env
            .iter()
            .any(|entry| entry == "AURA_HARNESS_MODE=1")));
        assert!(materialized.instances.iter().all(|instance| instance
            .env
            .iter()
            .any(|entry| entry == "AURA_HARNESS_PROFILE=deterministic")));
    }

    #[test]
    fn materialize_rewrites_zero_ports_deterministically() {
        let config = sample_run_config();
        let first = materialize_run_config(config.clone(), PathBuf::from("run.toml").as_path())
            .expect("materialization should succeed");
        let second =
            materialize_run_config(config, PathBuf::from("run.toml").as_path()).expect("ok");

        let first_port = first.instances[0].bind_address.clone();
        let second_port = second.instances[0].bind_address.clone();
        assert_eq!(first_port, second_port);
        assert_ne!(first_port, "127.0.0.1:0");
    }

    fn sample_run_config() -> RunConfig {
        RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "Provisioning Test".to_string(),
                pty_rows: Some(40),
                pty_cols: Some(120),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(99),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
            },
            instances: vec![
                InstanceConfig {
                    id: "alice".to_string(),
                    mode: InstanceMode::Local,
                    data_dir: PathBuf::from("artifacts/harness/state/alice"),
                    device_id: None,
                    bind_address: "127.0.0.1:0".to_string(),
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
                },
                InstanceConfig {
                    id: "browser".to_string(),
                    mode: InstanceMode::Browser,
                    data_dir: PathBuf::from(".tmp/browser/browser"),
                    device_id: None,
                    bind_address: "127.0.0.1:0".to_string(),
                    demo_mode: false,
                    command: None,
                    args: vec![],
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
                },
            ],
        }
    }
}
