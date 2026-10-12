use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{anyhow, bail, Result};
use nix::unistd::getpid;

use crate::config::{InstanceMode, RunConfig};
use crate::determinism::{build_seed_bundle, DEFAULT_HARNESS_SEED};
use crate::workspace_root;

const DETERMINISTIC_PORT_MIN: u16 = 41_000;
const DETERMINISTIC_PORT_SPAN: u16 = 20_000;
const ISOLATED_PORT_MIN: u16 = 20_000;
const ISOLATED_PORT_SPAN: u16 = 40_000;
static RUN_TOKEN_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn materialize_run_config(config: RunConfig, config_path: &Path) -> Result<RunConfig> {
    materialize_run_config_with_artifact_base(config, config_path, None)
}

pub fn materialize_run_config_with_artifact_base(
    mut config: RunConfig,
    _config_path: &Path,
    artifact_base_dir: Option<PathBuf>,
) -> Result<RunConfig> {
    let root = run_root(&config, artifact_base_dir);
    config.run.artifact_dir = Some(root.clone());
    let run_name = config.run.name.clone();

    let seed_bundle = build_seed_bundle(&config);
    let run_token = run_token();
    let port_namespace = port_namespace_for(config.run.fixed_ports, run_token.as_deref());
    let mut used_ports = HashSet::new();

    for instance in &mut config.instances {
        let instance_root = root.join("instances").join(&instance.id);
        if !instance.data_dir.is_absolute() {
            instance.data_dir = instance_root.join("state");
        }
        let transient_root = transient_root(
            &run_name,
            &instance.id,
            run_token.as_deref(),
            &instance.data_dir,
        )?;
        if instance.log_path.is_none() {
            instance.log_path = Some(instance_root.join("instance.log"));
        }

        if matches!(instance.mode, InstanceMode::Local | InstanceMode::Browser) {
            ensure_env_value(&mut instance.env, "AURA_HARNESS_MODE", "1");
            ensure_env_value(&mut instance.env, "AURA_HARNESS_PROFILE", "deterministic");
            ensure_env_value(
                &mut instance.env,
                "AURA_HARNESS_SCENARIO_SEED",
                &seed_bundle.scenario_seed.to_string(),
            );
            ensure_env_value(&mut instance.env, "AURA_HARNESS_INSTANCE_ID", &instance.id);
            if let Some(run_token) = run_token.as_deref() {
                ensure_env_value(&mut instance.env, "AURA_HARNESS_RUN_TOKEN", run_token);
            }
            ensure_env_path(&mut instance.env, "AURA_HARNESS_RUN_ROOT", root.clone());
            ensure_env_path(
                &mut instance.env,
                "AURA_HARNESS_INSTANCE_ROOT",
                instance_root.clone(),
            );
            ensure_env_path(
                &mut instance.env,
                "AURA_HARNESS_INSTANCE_TRANSIENT_ROOT",
                transient_root,
            );
        }

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
            port_namespace,
            &mut used_ports,
        )?;

        if let Some(lan_discovery) = instance.lan_discovery.as_mut() {
            lan_discovery.port = namespace_port(lan_discovery.port, port_namespace);
        }
    }

    Ok(config)
}

fn run_root(config: &RunConfig, artifact_base_dir: Option<PathBuf>) -> PathBuf {
    if let Some(explicit) = std::env::var_os("AURA_HARNESS_RUN_ROOT") {
        return absolutize_path(PathBuf::from(explicit));
    }

    let explicit = artifact_base_dir
        .map(absolutize_path)
        .map(|base| {
            base.join("harness")
                .join(sanitize_segment(&config.run.name))
        })
        .unwrap_or_else(|| {
            config
                .run
                .artifact_dir
                .clone()
                .unwrap_or_else(|| default_run_root(config))
        });
    let explicit = absolutize_path(explicit);

    isolate_run_root(explicit, run_token().as_deref())
}

fn default_run_root(config: &RunConfig) -> PathBuf {
    let run_seed = config.run.seed.unwrap_or(DEFAULT_HARNESS_SEED);
    workspace_root()
        .join(".tmp")
        .join("harness")
        .join("runs")
        .join(format!(
            "{}-{}",
            sanitize_segment(&config.run.name),
            run_seed
        ))
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

pub(crate) fn run_token() -> Option<String> {
    if let Some(token) = std::env::var("AURA_HARNESS_RUN_TOKEN")
        .ok()
        .map(|value| sanitize_segment(&value))
        .filter(|value| !value.is_empty())
    {
        return Some(token);
    }

    let pid = getpid().as_raw();
    let counter = RUN_TOKEN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let suffix = tempfile::Builder::new()
        .prefix("aura-harness-run-")
        .tempdir()
        .ok()
        .and_then(|dir| {
            dir.path()
                .file_name()
                .and_then(|name| name.to_str())
                .map(sanitize_segment)
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("{counter:x}"));
    let mut token = format!("pid{pid}-run{counter}-{suffix}");
    if token.len() < 16 {
        token.push_str(&format!("-{counter:08x}"));
    }
    Some(token)
}

fn isolate_run_root(base: PathBuf, token: Option<&str>) -> PathBuf {
    match token {
        Some(token) if !token.is_empty() => base.join("runs").join(token),
        _ => base,
    }
}

// Reserve the longest existing generation socket filename and its terminator.
const MAX_TRANSIENT_SOCKET_PATH: usize = 100;
pub(crate) fn validate_transient_root(root: PathBuf) -> Result<PathBuf> {
    let longest = root.join("tui").join("uffffffffffffffff.sock");
    if longest.as_os_str().len() > MAX_TRANSIENT_SOCKET_PATH {
        return Err(anyhow::Error::new(TransientSocketPathTooLong { root }));
    }
    Ok(root)
}
#[derive(Debug)]
struct TransientSocketPathTooLong {
    root: PathBuf,
}
impl std::fmt::Display for TransientSocketPathTooLong {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "selected harness transient root cannot fit owned Unix sockets: {}",
            self.root.display()
        )
    }
}
impl std::error::Error for TransientSocketPathTooLong {}

pub(crate) fn select_owned_transient_root(
    identity: u64,
    explicit: Option<PathBuf>,
    runtime_candidates: impl IntoIterator<Item = PathBuf>,
) -> Result<PathBuf> {
    if let Some(root) = explicit {
        return validate_transient_root(absolutize_path(root));
    }
    let namespace = format!("aura-harness-{}", nix::unistd::getuid().as_raw());
    for candidate in runtime_candidates
        .into_iter()
        .chain([PathBuf::from("/tmp")])
    {
        if !candidate.is_absolute() {
            continue;
        }
        let Ok(candidate) = std::fs::canonicalize(candidate) else {
            continue;
        };
        let root = candidate.join(&namespace).join(format!("{identity:016x}"));
        if let Ok(root) = validate_transient_root(root) {
            return Ok(root);
        }
    }
    bail!("no selected local runtime directory can fit owned harness sockets")
}

fn validate_owned_directory(path: &Path, allow_shared_base: bool) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path)?;
    let shared_base = allow_shared_base && metadata.uid() == 0 && metadata.mode() & 0o1000 != 0;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || (metadata.uid() != nix::unistd::getuid().as_raw() && !shared_base)
    {
        bail!(
            "harness directory is not an original owned native directory: {}",
            path.display()
        );
    }
    Ok(())
}

pub(crate) fn prepare_private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    validate_owned_directory(path, false)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn prepare_selected_parent(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => validate_owned_directory(path, true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            prepare_selected_parent(
                path.parent()
                    .ok_or_else(|| anyhow!("selected parent has no owner"))?,
            )?;
            prepare_private_directory(path)
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn prepare_owned_transient_root(root: &Path) -> Result<()> {
    let parent = root
        .parent()
        .ok_or_else(|| anyhow!("transient root has no original parent"))?;
    let namespace = format!("aura-harness-{}", nix::unistd::getuid().as_raw());
    if parent
        .file_name()
        .is_some_and(|name| name == namespace.as_str())
    {
        let base = parent
            .parent()
            .ok_or_else(|| anyhow!("runtime namespace has no parent"))?;
        validate_owned_directory(base, true)?;
        prepare_private_directory(parent)?;
    } else {
        prepare_selected_parent(parent)?;
    }
    prepare_private_directory(root)
}

/// Validate the original private namespace before any destructive traversal.
pub(crate) fn validate_owned_transient_cleanup(root: &Path) -> Result<()> {
    if !root.is_absolute() {
        bail!("selected harness transient root must be absolute");
    }
    let parent = root
        .parent()
        .ok_or_else(|| anyhow!("transient root has no parent"))?;
    let namespace = format!("aura-harness-{}", nix::unistd::getuid().as_raw());
    if parent
        .file_name()
        .is_some_and(|name| name == namespace.as_str())
    {
        validate_owned_directory(
            parent
                .parent()
                .ok_or_else(|| anyhow!("namespace has no parent"))?,
            true,
        )?;
        validate_owned_directory(parent, false)?;
    } else {
        validate_owned_directory(parent, true)?;
    }
    validate_owned_directory(root, false)?;
    let tui = root.join("tui");
    match std::fs::symlink_metadata(&tui) {
        Ok(_) => validate_owned_directory(&tui, false)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub(crate) fn prepare_owned_transient_reset(root: &Path) -> Result<()> {
    match std::fs::symlink_metadata(root) {
        Ok(_) => validate_owned_transient_cleanup(root),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            prepare_owned_transient_root(root)
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn owned_transient_root_for_scope(identity: u64) -> Result<PathBuf> {
    let explicit = std::env::var_os("AURA_HARNESS_TRANSIENT_ROOT")
        .map(PathBuf::from)
        .map(|root| root.join(format!("{identity:016x}")));
    let candidates = ["XDG_RUNTIME_DIR", "TMPDIR"]
        .into_iter()
        .filter_map(|key| std::env::var_os(key).map(PathBuf::from));
    select_owned_transient_root(identity, explicit, candidates)
}

pub(crate) fn transient_root(
    run_name: &str,
    instance_id: &str,
    token: Option<&str>,
    data_dir: &Path,
) -> Result<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    run_name.hash(&mut hasher);
    instance_id.hash(&mut hasher);
    token.unwrap_or_default().hash(&mut hasher);
    data_dir.hash(&mut hasher);
    owned_transient_root_for_scope(hasher.finish())
}

fn absolutize_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    workspace_root().join(path)
}

/// Port offset for this run: none for fixed-port (multi-host) configs,
/// otherwise derived from the run token so parallel runs do not collide.
fn port_namespace_for(fixed_ports: bool, run_token: Option<&str>) -> u16 {
    if fixed_ports {
        return 0;
    }
    run_token.map(namespaced_port_offset).unwrap_or_default()
}

fn namespaced_port_offset(token: &str) -> u16 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    token.hash(&mut hasher);
    (hasher.finish() % u64::from(ISOLATED_PORT_SPAN)) as u16
}

fn namespace_port(port: u16, offset: u16) -> u16 {
    if offset == 0 {
        return port;
    }

    let normalized = if port >= ISOLATED_PORT_MIN {
        u32::from(port - ISOLATED_PORT_MIN)
    } else {
        u32::from(port % ISOLATED_PORT_SPAN)
    };
    let offset = u32::from(offset);
    let span = u32::from(ISOLATED_PORT_SPAN);
    let min = u32::from(ISOLATED_PORT_MIN);
    (min + ((normalized + offset) % span)) as u16
}

fn ensure_env_path(env: &mut Vec<String>, key: &str, value: PathBuf) {
    if env.iter().any(|entry| {
        entry
            .split_once('=')
            .is_some_and(|(candidate, _)| candidate == key)
    }) {
        return;
    }
    env.push(format!("{key}={}", value.display()));
}

fn ensure_env_value(env: &mut Vec<String>, key: &str, value: &str) {
    if env.iter().any(|entry| {
        entry
            .split_once('=')
            .is_some_and(|(candidate, _)| candidate == key)
    }) {
        return;
    }
    env.push(format!("{key}={value}"));
}

fn materialize_bind_address(
    bind_address: &str,
    instance_seed: u64,
    port_namespace: u16,
    used_ports: &mut HashSet<u16>,
) -> Result<String> {
    let (host, port) = split_host_port(bind_address)?;
    if port != 0 {
        let mut candidate = namespace_port(port, port_namespace);
        for _ in 0..ISOLATED_PORT_SPAN {
            if used_ports.insert(candidate) {
                return Ok(format!("{host}:{candidate}"));
            }
            candidate =
                ISOLATED_PORT_MIN + ((candidate + 1 - ISOLATED_PORT_MIN) % ISOLATED_PORT_SPAN);
        }
        bail!("unable to allocate isolated port for {bind_address}");
    }

    let base = DETERMINISTIC_PORT_MIN as u64 + (instance_seed % u64::from(DETERMINISTIC_PORT_SPAN));
    for offset in 0..DETERMINISTIC_PORT_SPAN {
        let candidate = u32::from(DETERMINISTIC_PORT_MIN)
            + (((base as u32 - u32::from(DETERMINISTIC_PORT_MIN)) + u32::from(offset))
                % u32::from(DETERMINISTIC_PORT_SPAN));
        let candidate = candidate as u16;
        if used_ports.insert(candidate) {
            return Ok(format!("{host}:{candidate}"));
        }
    }

    bail!("unable to allocate deterministic port for {bind_address}")
}

fn split_host_port(bind_address: &str) -> Result<(&str, u16)> {
    let (host, port) = bind_address
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("bind_address must be in host:port form, got {bind_address}"))?;
    let port = port
        .parse::<u16>()
        .map_err(|error| anyhow!("invalid bind_address port in {bind_address}: {error}"))?;
    Ok((host, port))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{materialize_run_config, materialize_run_config_with_artifact_base};
    use crate::config::{InstanceConfig, InstanceMode, RunConfig, RunSection, RuntimeSubstrate};

    #[test]
    fn materialize_assigns_deterministic_run_root_and_browser_artifacts() {
        let config = sample_run_config();
        let materialized = materialize_run_config(config, PathBuf::from("run.toml").as_path())
            .unwrap_or_else(|error| panic!("materialization should succeed: {error}"));

        let run_root = materialized
            .run
            .artifact_dir
            .unwrap_or_else(|| panic!("artifact_dir should be assigned"));
        assert!(run_root.is_absolute());
        assert!(run_root.to_string_lossy().contains(".tmp/harness/runs"));
        assert!(run_root.to_string_lossy().contains("/runs/"));

        let browser = materialized
            .instances
            .iter()
            .find(|instance| matches!(instance.mode, InstanceMode::Browser))
            .unwrap_or_else(|| panic!("browser instance should exist"));
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
            .unwrap_or_else(|error| panic!("materialization should succeed: {error}"));
        let second = materialize_run_config(config, PathBuf::from("run.toml").as_path())
            .unwrap_or_else(|error| panic!("materialization should succeed: {error}"));

        let first_port = first.instances[0].bind_address.clone();
        let second_port = second.instances[0].bind_address.clone();
        assert_eq!(first_port, second_port);
        assert_ne!(first_port, "127.0.0.1:0");
    }

    #[test]
    fn isolate_run_root_appends_token_segment() {
        let root = super::isolate_run_root(PathBuf::from("/tmp/aura"), Some("scenario13-tui"));
        assert_eq!(root, PathBuf::from("/tmp/aura/runs/scenario13-tui"));
    }

    #[test]
    fn namespace_port_offsets_explicit_ports() {
        let offset = super::namespaced_port_offset("scenario13-tui");
        let port = super::namespace_port(41_001, offset);
        assert_ne!(port, 41_001);
        assert!(
            (super::ISOLATED_PORT_MIN..super::ISOLATED_PORT_MIN + super::ISOLATED_PORT_SPAN)
                .contains(&port)
        );
    }

    #[test]
    fn fixed_ports_keep_configured_ports_literal() {
        assert_eq!(
            super::port_namespace_for(true, Some("lan-e2e-shared-run-9")),
            0
        );
        assert_eq!(super::namespace_port(19_433, 0), 19_433);
        assert_ne!(
            super::port_namespace_for(false, Some("lan-e2e-shared-run-9")),
            0
        );
    }

    #[test]
    fn materialize_respects_cli_artifact_base_override() {
        let config = sample_run_config();
        let override_root = PathBuf::from("/tmp/aura-artifacts");
        let materialized = materialize_run_config_with_artifact_base(
            config,
            PathBuf::from("run.toml").as_path(),
            Some(override_root.clone()),
        )
        .unwrap_or_else(|error| panic!("materialization should succeed: {error}"));

        let run_root = materialized
            .run
            .artifact_dir
            .unwrap_or_else(|| panic!("artifact_dir should be assigned"));
        assert!(
            run_root.starts_with(override_root.join("harness").join("provisioning-test")),
            "CLI artifact override should become the authoritative harness run root"
        );
    }

    fn sample_run_config() -> RunConfig {
        RunConfig {
            schema_version: 1,
            run: RunSection {
                network_topology: None,
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
                runtime_substrate: RuntimeSubstrate::default(),
                fixed_ports: false,
            },
            instances: vec![
                InstanceConfig {
                    lab_authority: None,
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
                    lab_authority: None,
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

#[cfg(test)]
mod short_transient_owner_tests {
    use super::*;
    #[test]
    fn selected_short_root_is_honored_and_overlong_explicit_root_is_typed_failure() {
        let owner = tempfile::Builder::new()
            .prefix("ah316-")
            .tempdir_in("/tmp")
            .expect("short original test owner");
        let explicit = owner.path().join("exact");
        assert_eq!(
            select_owned_transient_root(1, Some(explicit.clone()), [PathBuf::from("/ignored")])
                .expect("valid selected root"),
            explicit
        );
        let deep = owner.path().join("x".repeat(120));
        let error = select_owned_transient_root(1, Some(deep), [owner.path().to_path_buf()])
            .expect_err("explicit invalid root cannot redirect");
        assert!(error.downcast_ref::<TransientSocketPathTooLong>().is_some());
        assert!(!owner.path().join("exact").exists());
    }
}

#[cfg(test)]
mod private_namespace_tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    #[test]
    fn existing_symlink_namespace_and_tui_cannot_redirect_chmod_or_creation() {
        let original = tempfile::Builder::new()
            .prefix("ah316-")
            .tempdir_in("/tmp")
            .expect("original test owner");
        let target = original.path().join("untouched");
        std::fs::create_dir(&target).expect("actual target");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
            .expect("target mode");
        let namespace = original
            .path()
            .join(format!("aura-harness-{}", nix::unistd::getuid().as_raw()));
        symlink(&target, &namespace).expect("actual untrusted namespace alias");
        assert!(prepare_owned_transient_root(&namespace.join("scope")).is_err());
        assert!(prepare_owned_transient_reset(&namespace.join("absent-reset-child")).is_err());
        assert!(!target.join("absent-reset-child").exists());
        assert!(!target.join("scope").exists());
        assert_eq!(
            std::fs::metadata(&target)
                .expect("target unchanged")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        let root = original.path().join("owned");
        prepare_owned_transient_root(&root).expect("actual owned root");
        symlink(&target, root.join("tui")).expect("actual untrusted socket directory alias");
        assert!(prepare_private_directory(&root.join("tui")).is_err());
        assert_eq!(
            std::fs::metadata(&target)
                .expect("target unchanged")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
    #[test]
    fn foreign_owned_namespace_is_refused_without_permission_change() {
        use std::os::unix::fs::MetadataExt;
        let foreign = Path::new("/private/tmp");
        let foreign = if foreign.exists() {
            foreign
        } else {
            Path::new("/tmp")
        };
        let before = std::fs::symlink_metadata(foreign).expect("actual shared OS directory");
        assert_ne!(
            before.uid(),
            nix::unistd::getuid().as_raw(),
            "fixture requires actual foreign-owned OS directory"
        );
        assert!(prepare_private_directory(foreign).is_err());
        assert_eq!(
            std::fs::symlink_metadata(foreign)
                .expect("unchanged foreign directory")
                .mode(),
            before.mode()
        );
    }
}
