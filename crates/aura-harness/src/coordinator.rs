//! Central coordinator for multi-instance test harness execution.
//!
//! Manages the lifecycle of multiple backend instances (local, browser, SSH),
//! dispatches commands, captures screen states, and enforces timeouts.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::net::TcpListener;
use std::net::TcpStream;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use aura_app::ui::contract::{ControlId, FieldId, ListId, UiSnapshot};
use aura_app::ui::workflows::ids;
use aura_core::{hash::hash, AuthorityId};
use nix::errno::Errno;
use nix::sys::signal;
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::workspace_root;

use crate::backend::{
    BackendHandle, DiagnosticObservationProbe,
    SemanticCommandRequest as BackendSemanticCommandRequest,
    SemanticCommandResponse as BackendSemanticCommandResponse,
};
use crate::config::{InstanceMode, RunConfig, RuntimeSubstrate, ScreenSource};
use crate::event_details;
use crate::events::EventStream;
use crate::runtime_substrate::RuntimeSubstrateController;
use crate::screen_normalization::normalize_screen;
use crate::timeouts::blocking_sleep;
use crate::tool_api::ToolKey;

const BACKEND_HEALTH_TIMEOUT: Duration = Duration::from_secs(30);
const BACKEND_READY_TIMEOUT: Duration = Duration::from_secs(120);
const BACKEND_TEARDOWN_TIMEOUT: Duration = Duration::from_secs(15);
const BACKEND_POLL_INTERVAL: Duration = Duration::from_millis(100);
const WEB_SERVER_READY_TIMEOUT: Duration = Duration::from_secs(600);
const WEB_SERVER_POLL_INTERVAL: Duration = Duration::from_millis(250);

struct OwnedWebServer {
    child: Child,
    url: String,
}

pub enum DiagnosticObservationWait<'a> {
    Pattern {
        pattern: &'a str,
        source: ScreenSource,
    },
    Target {
        selector: &'a str,
    },
}

pub struct HarnessCoordinator {
    backends: HashMap<String, BackendHandle>,
    instance_order: Vec<String>,
    instance_modes: HashMap<String, InstanceMode>,
    instance_bind_addresses: HashMap<String, String>,
    instance_data_dirs: HashMap<String, PathBuf>,
    instance_transient_dirs: HashMap<String, PathBuf>,
    runtime_substrate: RuntimeSubstrate,
    runtime_substrate_controller: RuntimeSubstrateController,
    owned_web_server: Option<OwnedWebServer>,
    owned_web_server_log_path: Option<PathBuf>,
    events: EventStream,
    run_root: PathBuf,
    run_token: Option<String>,
    instance_claim_paths: HashMap<String, PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
struct InstanceOwnershipManifest {
    instance_id: String,
    mode: String,
    bind_address: String,
    data_dir: String,
    transient_dir: String,
    claim_path: String,
}

#[derive(Debug, Clone, Serialize)]
struct RunOwnershipManifest {
    run_token: Option<String>,
    coordinator_pid: u32,
    run_root: String,
    instances: Vec<InstanceOwnershipManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstanceClaim {
    instance_id: String,
    run_token: Option<String>,
    coordinator_pid: u32,
    run_root: String,
}

#[allow(clippy::disallowed_methods)] // Harness timeout enforcement requires wall-clock bounds.
impl HarnessCoordinator {
    pub fn from_run_config(config: &RunConfig) -> Result<Self> {
        let browser_app_url = provision_browser_app_url(config)?;
        let mut backends = HashMap::new();
        let mut instance_order = Vec::new();
        let mut instance_modes = HashMap::new();
        let mut instance_bind_addresses = HashMap::new();
        let mut instance_data_dirs = HashMap::new();
        let mut instance_transient_dirs = HashMap::new();
        let pty_rows = config.run.pty_rows;
        let pty_cols = config.run.pty_cols;
        for instance in &config.instances {
            let id = instance.id.clone();
            let mut instance = instance.clone();
            if matches!(instance.mode, InstanceMode::Browser)
                && !instance_has_browser_app_url(&instance)
            {
                instance.env.push(format!(
                    "AURA_HARNESS_BROWSER_APP_URL={}",
                    browser_app_url.url
                ));
            }
            let instance_mode = instance.mode.clone();
            let instance_bind_address = instance.bind_address.clone();
            let instance_data_dir = absolutize_path(instance.data_dir.clone());
            let instance_transient_dir = instance
                .env
                .iter()
                .find_map(|entry| {
                    let (key, value) = entry.split_once('=')?;
                    (key == "AURA_HARNESS_INSTANCE_TRANSIENT_ROOT")
                        .then(|| absolutize_path(PathBuf::from(value)))
                })
                .unwrap_or_else(|| instance_data_dir.join(".harness-transient"));
            let backend = BackendHandle::from_config(instance, pty_rows, pty_cols)?;
            instance_order.push(id.clone());
            instance_modes.insert(id.clone(), instance_mode);
            instance_bind_addresses.insert(id.clone(), instance_bind_address);
            instance_data_dirs.insert(id.clone(), instance_data_dir);
            instance_transient_dirs.insert(id.clone(), instance_transient_dir);
            backends.insert(id, backend);
        }
        let artifact_dir = config.run.artifact_dir.clone().map(absolutize_path);
        let run_root = artifact_dir
            .clone()
            .unwrap_or_else(|| absolutize_path(PathBuf::from(".tmp/harness")));
        let runtime_substrate_controller = RuntimeSubstrateController::new(
            config.run.runtime_substrate,
            config.run.seed.unwrap_or_default(),
            instance_order.clone(),
            artifact_dir,
        )?;

        let run_token = std::env::var("AURA_HARNESS_RUN_TOKEN").ok();
        let instance_claim_paths = instance_order
            .iter()
            .map(|instance_id| {
                (
                    instance_id.clone(),
                    workspace_root()
                        .join(".tmp")
                        .join("harness")
                        .join("instance-claims")
                        .join(format!("{instance_id}.json")),
                )
            })
            .collect::<HashMap<_, _>>();

        let coordinator = Self {
            backends,
            instance_order,
            instance_modes,
            instance_bind_addresses,
            instance_data_dirs,
            instance_transient_dirs,
            runtime_substrate: config.run.runtime_substrate,
            runtime_substrate_controller,
            owned_web_server: browser_app_url.server,
            owned_web_server_log_path: browser_app_url.log_path,
            events: EventStream::new(),
            run_root,
            run_token,
            instance_claim_paths,
        };
        coordinator.write_run_manifest()?;
        Ok(coordinator)
    }

    pub fn start_all(&mut self) -> Result<()> {
        self.acquire_instance_claims()?;
        self.clear_stale_local_state()?;
        if let Some(server) = &mut self.owned_web_server {
            wait_for_owned_web_server(
                self.owned_web_server_log_path.as_deref(),
                server,
                owned_web_server_ready_timeout(),
            )?;
            self.events.push(
                "lifecycle",
                "web_server_ready",
                None,
                event_details!({ "url" => server.url.clone() }),
            );
        }
        self.runtime_substrate_controller.start()?;
        let browser_ids = self
            .instance_order
            .iter()
            .filter(|instance_id| {
                self.instance_modes
                    .get(*instance_id)
                    .is_some_and(|mode| matches!(mode, InstanceMode::Browser))
            })
            .cloned()
            .collect::<Vec<_>>();
        let browser_start_kinds = self.start_browser_backends_parallel(&browser_ids)?;
        for id in self.instance_order.clone() {
            if browser_start_kinds.contains_key(&id) {
                continue;
            }
            let backend_kind = {
                let backend = self
                    .backends
                    .get_mut(&id)
                    .ok_or_else(|| anyhow!("unknown instance_id: {id}"))?;
                let backend_kind = backend.backend_kind();
                eprintln!(
                    "[harness] startup phase=backend_start begin instance={id} backend={backend_kind}"
                );
                backend.start()?;
                eprintln!(
                    "[harness] startup phase=backend_start done instance={id} backend={backend_kind}"
                );
                backend_kind
            };
            self.events.push(
                "lifecycle",
                "start",
                Some(id.clone()),
                event_details!({ "backend" => backend_kind }),
            );
            eprintln!(
                "[harness] startup phase=health_check begin instance={id} backend={backend_kind}"
            );
            self.wait_for_backend_health(&id, BACKEND_HEALTH_TIMEOUT)?;
            eprintln!(
                "[harness] startup phase=health_check done instance={id} backend={backend_kind}"
            );
            self.events.push(
                "lifecycle",
                "health_ok",
                Some(id.clone()),
                event_details!({ "timeout_ms" => BACKEND_HEALTH_TIMEOUT.as_millis() }),
            );
            if backend_kind == "playwright_browser" {
                self.events.push(
                    "lifecycle",
                    "ready_ok",
                    Some(id.clone()),
                    event_details!({
                        "timeout_ms" => BACKEND_READY_TIMEOUT.as_millis(),
                        "source" => "playwright_startup_semantic_ready"
                    }),
                );
                continue;
            }
            eprintln!(
                "[harness] startup phase=ready_check begin instance={id} backend={backend_kind}"
            );
            self.backends
                .get(&id)
                .ok_or_else(|| anyhow!("unknown instance_id: {id}"))?
                .wait_until_ready(BACKEND_READY_TIMEOUT)?;
            eprintln!(
                "[harness] startup phase=ready_check done instance={id} backend={backend_kind}"
            );
            self.events.push(
                "lifecycle",
                "ready_ok",
                Some(id.clone()),
                event_details!({ "timeout_ms" => BACKEND_READY_TIMEOUT.as_millis() }),
            );
        }
        for id in browser_ids {
            let backend_kind = browser_start_kinds
                .get(&id)
                .copied()
                .ok_or_else(|| anyhow!("missing browser startup result for instance {id}"))?;
            self.events.push(
                "lifecycle",
                "start",
                Some(id.clone()),
                event_details!({ "backend" => backend_kind, "startup_mode" => "parallel" }),
            );
            self.events.push(
                "lifecycle",
                "health_ok",
                Some(id.clone()),
                event_details!({
                    "timeout_ms" => BACKEND_HEALTH_TIMEOUT.as_millis(),
                    "startup_mode" => "parallel"
                }),
            );
            self.events.push(
                "lifecycle",
                "ready_ok",
                Some(id.clone()),
                event_details!({
                    "timeout_ms" => BACKEND_READY_TIMEOUT.as_millis(),
                    "source" => "playwright_startup_semantic_ready",
                    "startup_mode" => "parallel"
                }),
            );
        }
        Ok(())
    }

    fn wait_for_backend_health(&self, instance_id: &str, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let backend = self
                .backends
                .get(instance_id)
                .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
            if backend.health_check()? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("instance {instance_id} failed startup health gate within {timeout:?}");
            }
            blocking_sleep(BACKEND_POLL_INTERVAL);
        }
    }

    fn start_browser_backends_parallel(
        &mut self,
        browser_ids: &[String],
    ) -> Result<HashMap<String, &'static str>> {
        let mut handles = Vec::new();
        for id in browser_ids {
            let backend = self
                .backends
                .remove(id)
                .ok_or_else(|| anyhow!("unknown instance_id: {id}"))?;
            let instance_id = id.clone();
            handles.push((
                instance_id.clone(),
                thread::spawn(move || {
                    let mut backend = backend;
                    let backend_kind = backend.backend_kind();
                    let result = (|| -> Result<()> {
                        eprintln!(
                            "[harness] startup phase=backend_start begin instance={instance_id} backend={backend_kind}"
                        );
                        backend.start()?;
                        eprintln!(
                            "[harness] startup phase=backend_start done instance={instance_id} backend={backend_kind}"
                        );
                        eprintln!(
                            "[harness] startup phase=health_check begin instance={instance_id} backend={backend_kind}"
                        );
                        let deadline = Instant::now() + BACKEND_HEALTH_TIMEOUT;
                        loop {
                            if backend.health_check()? {
                                break;
                            }
                            if Instant::now() >= deadline {
                                bail!(
                                    "instance {instance_id} failed startup health gate within {BACKEND_HEALTH_TIMEOUT:?}"
                                );
                            }
                            blocking_sleep(BACKEND_POLL_INTERVAL);
                        }
                        eprintln!(
                            "[harness] startup phase=health_check done instance={instance_id} backend={backend_kind}"
                        );
                        Ok(())
                    })();
                    match result {
                        Ok(()) => Ok((backend, backend_kind)),
                        Err(error) => Err((backend, error)),
                    }
                }),
            ));
        }

        let mut kinds = HashMap::new();
        let mut first_error = None;
        for (id, handle) in handles {
            match handle.join() {
                Ok(Ok((backend, backend_kind))) => {
                    self.backends.insert(id.clone(), backend);
                    kinds.insert(id, backend_kind);
                }
                Ok(Err((backend, error))) => {
                    self.backends.insert(id.clone(), backend);
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
                Err(_) => {
                    if first_error.is_none() {
                        first_error =
                            Some(anyhow!("browser startup thread panicked for instance {id}"));
                    }
                }
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }

        Ok(kinds)
    }

    fn clear_stale_local_state(&mut self) -> Result<()> {
        for instance_id in &self.instance_order {
            let mode = self
                .instance_modes
                .get(instance_id)
                .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
            if !matches!(mode, InstanceMode::Local | InstanceMode::Browser) {
                continue;
            }
            let data_dir = self
                .instance_data_dirs
                .get(instance_id)
                .ok_or_else(|| anyhow!("missing data_dir for instance_id: {instance_id}"))?;
            clear_directory_contents(data_dir)?;
            if let Some(transient_dir) = self.instance_transient_dirs.get(instance_id) {
                clear_directory_contents(transient_dir)?;
            }
            self.events.push(
                "lifecycle",
                "clear_stale_state",
                Some(instance_id.clone()),
                event_details!({
                    "data_dir" => data_dir.display().to_string(),
                    "transient_dir" => self.instance_transient_dirs
                        .get(instance_id)
                        .map(|path| path.display().to_string())
                        .unwrap_or_default()
                }),
            );
        }
        Ok(())
    }

    pub fn stop_all(&mut self) -> Result<()> {
        let stop_result = (|| -> Result<()> {
            for id in self.instance_order.iter().rev() {
                let backend_kind = {
                    let backend = self
                        .backends
                        .get_mut(id)
                        .ok_or_else(|| anyhow!("unknown instance_id: {id}"))?;
                    let backend_kind = backend.backend_kind();
                    backend.stop()?;
                    backend_kind
                };
                self.events.push(
                    "lifecycle",
                    "stop",
                    Some(id.clone()),
                    event_details!({ "backend" => backend_kind }),
                );
                self.wait_for_backend_stopped(id, BACKEND_TEARDOWN_TIMEOUT)?;
                self.verify_bind_address_released(id)?;
                self.events.push(
                    "lifecycle",
                    "cleanup_ok",
                    Some(id.clone()),
                    event_details!({ "timeout_ms" => BACKEND_TEARDOWN_TIMEOUT.as_millis() }),
                );
            }
            self.verify_post_run_cleanup()?;
            self.runtime_substrate_controller.finish()?;
            if let Some(server) = &mut self.owned_web_server {
                let _ = server.child.kill();
                let _ = server.child.wait();
            }
            Ok(())
        })();
        let release_result = self.release_instance_claims();
        stop_result?;
        release_result?;
        Ok(())
    }

    pub fn runtime_substrate(&self) -> RuntimeSubstrate {
        self.runtime_substrate
    }

    pub fn apply_fault_delay(&mut self, actor: &str, delay_ms: u64) -> Result<()> {
        self.runtime_substrate_controller
            .fault_delay(actor, delay_ms)
    }

    pub fn apply_fault_loss(&mut self, actor: &str, loss_percent: u8) -> Result<()> {
        self.runtime_substrate_controller
            .fault_loss(actor, loss_percent)
    }

    pub fn apply_fault_tunnel_drop(&mut self, actor: &str) -> Result<()> {
        self.runtime_substrate_controller.fault_tunnel_drop(actor)
    }

    fn write_run_manifest(&self) -> Result<()> {
        fs::create_dir_all(&self.run_root).with_context(|| {
            format!(
                "failed to create harness run root {}",
                self.run_root.display()
            )
        })?;
        let manifest = RunOwnershipManifest {
            run_token: self.run_token.clone(),
            coordinator_pid: process::id(),
            run_root: self.run_root.display().to_string(),
            instances: self
                .instance_order
                .iter()
                .map(|instance_id| InstanceOwnershipManifest {
                    instance_id: instance_id.clone(),
                    mode: format!(
                        "{:?}",
                        self.instance_modes
                            .get(instance_id)
                            .unwrap_or_else(|| panic!("missing mode for {instance_id}"))
                    )
                    .to_ascii_lowercase(),
                    bind_address: self
                        .instance_bind_addresses
                        .get(instance_id)
                        .cloned()
                        .unwrap_or_else(|| panic!("missing bind address for {instance_id}")),
                    data_dir: self
                        .instance_data_dirs
                        .get(instance_id)
                        .unwrap_or_else(|| panic!("missing data dir for {instance_id}"))
                        .display()
                        .to_string(),
                    transient_dir: self
                        .instance_transient_dirs
                        .get(instance_id)
                        .unwrap_or_else(|| panic!("missing transient dir for {instance_id}"))
                        .display()
                        .to_string(),
                    claim_path: self
                        .instance_claim_paths
                        .get(instance_id)
                        .unwrap_or_else(|| panic!("missing claim path for {instance_id}"))
                        .display()
                        .to_string(),
                })
                .collect(),
        };
        let manifest_path = self.run_root.join("ownership_manifest.json");
        let body = serde_json::to_vec_pretty(&manifest)?;
        fs::write(&manifest_path, body).with_context(|| {
            format!(
                "failed to write harness ownership manifest {}",
                manifest_path.display()
            )
        })
    }

    fn acquire_instance_claims(&self) -> Result<()> {
        for instance_id in &self.instance_order {
            let mode = self
                .instance_modes
                .get(instance_id)
                .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
            if !matches!(mode, InstanceMode::Local | InstanceMode::Browser) {
                continue;
            }
            let claim_path = self
                .instance_claim_paths
                .get(instance_id)
                .ok_or_else(|| anyhow!("missing claim path for instance_id: {instance_id}"))?;
            if let Some(parent) = claim_path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "failed to create harness claim directory {}",
                        parent.display()
                    )
                })?;
            }
            self.acquire_instance_claim(instance_id, claim_path)?;
        }
        Ok(())
    }

    fn acquire_instance_claim(&self, instance_id: &str, claim_path: &Path) -> Result<()> {
        let claim = InstanceClaim {
            instance_id: instance_id.to_string(),
            run_token: self.run_token.clone(),
            coordinator_pid: process::id(),
            run_root: self.run_root.display().to_string(),
        };
        let body = serde_json::to_vec_pretty(&claim)?;
        loop {
            match File::options()
                .create_new(true)
                .write(true)
                .open(claim_path)
            {
                Ok(mut file) => {
                    use std::io::Write as _;
                    file.write_all(&body)?;
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let existing = fs::read(claim_path)
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<InstanceClaim>(&bytes).ok());
                    if existing
                        .as_ref()
                        .is_some_and(|existing| process_alive(existing.coordinator_pid))
                    {
                        let existing = existing.unwrap_or_else(|| InstanceClaim {
                            instance_id: instance_id.to_string(),
                            run_token: None,
                            coordinator_pid: 0,
                            run_root: String::new(),
                        });
                        bail!(
                            "instance {instance_id} is already claimed by live harness run token={:?} pid={} root={}; refer to docs/122_ownership_model.md best practices and clean the existing owner before starting a new run",
                            existing.run_token,
                            existing.coordinator_pid,
                            existing.run_root,
                        );
                    }
                    fs::remove_file(claim_path).with_context(|| {
                        format!(
                            "failed to remove stale harness claim {}",
                            claim_path.display()
                        )
                    })?;
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to create harness claim {} for instance {}",
                            claim_path.display(),
                            instance_id
                        )
                    });
                }
            }
        }
    }

    fn release_instance_claims(&self) -> Result<()> {
        for claim_path in self.instance_claim_paths.values() {
            match fs::remove_file(claim_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to remove harness claim {}", claim_path.display())
                    });
                }
            }
        }
        Ok(())
    }

    fn verify_post_run_cleanup(&self) -> Result<()> {
        for instance_id in &self.instance_order {
            let mode = self
                .instance_modes
                .get(instance_id)
                .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
            if !matches!(mode, InstanceMode::Local | InstanceMode::Browser) {
                continue;
            }
            let transient_dir = self
                .instance_transient_dirs
                .get(instance_id)
                .ok_or_else(|| anyhow!("missing transient_dir for instance_id: {instance_id}"))?;
            if transient_dir.exists() {
                let mut lingering = fs::read_dir(transient_dir)
                    .with_context(|| {
                        format!(
                            "failed to inspect transient dir {}",
                            transient_dir.display()
                        )
                    })?
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .collect::<Vec<_>>();
                lingering.sort();
                if !lingering.is_empty() {
                    bail!(
                        "instance {instance_id} left transient residue in {}: {}; refer to docs/122_ownership_model.md best practices and ensure owner death performs full cleanup",
                        transient_dir.display(),
                        lingering
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                }
            }
        }
        Ok(())
    }

    fn wait_for_backend_stopped(&self, instance_id: &str, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let backend = self
                .backends
                .get(instance_id)
                .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
            if !backend.health_check()? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("instance {instance_id} failed teardown health gate within {timeout:?}");
            }
            blocking_sleep(BACKEND_POLL_INTERVAL);
        }
    }

    fn verify_bind_address_released(&self, instance_id: &str) -> Result<()> {
        let mode = self
            .instance_modes
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        if matches!(mode, InstanceMode::Ssh) {
            return Ok(());
        }

        let bind_address = self.lookup_bind_address(instance_id)?;
        if bind_address.ends_with(":0") {
            return Ok(());
        }
        let listener = TcpListener::bind(&bind_address).map_err(|error| {
            anyhow!("instance {instance_id} did not release bind address {bind_address}: {error}")
        })?;
        drop(listener);
        Ok(())
    }

    fn lookup_bind_address(&self, instance_id: &str) -> Result<String> {
        self.instance_bind_addresses
            .get(instance_id)
            .cloned()
            .ok_or_else(|| anyhow!("missing bind_address for instance {instance_id}"))
    }

    pub fn diagnostic_screen(&self, instance_id: &str) -> Result<String> {
        self.diagnostic_screen_with_source(instance_id, ScreenSource::Default)
    }

    pub fn diagnostic_screen_with_source(
        &self,
        instance_id: &str,
        source: ScreenSource,
    ) -> Result<String> {
        let backend = self
            .backends
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        match source {
            ScreenSource::Default => backend.diagnostic_screen_snapshot(),
            ScreenSource::Dom => backend.diagnostic_dom_snapshot(),
        }
    }

    pub fn ui_snapshot(&self, instance_id: &str) -> Result<UiSnapshot> {
        let backend = self
            .backends
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        let snapshot = backend.ui_snapshot()?;
        self.events.push(
            "observation",
            "ui_snapshot",
            Some(instance_id.to_string()),
            event_details!({
                "screen" => format!("{:?}", snapshot.screen).to_ascii_lowercase(),
                "open_modal" => snapshot
                    .open_modal
                    .map(|modal| format!("{modal:?}").to_ascii_lowercase())
                    .unwrap_or_else(|| "none".to_string())
            }),
        );
        Ok(snapshot)
    }

    pub fn wait_for_ui_snapshot_event(
        &self,
        instance_id: &str,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> Result<Option<crate::backend::UiSnapshotEvent>> {
        let backend = self
            .backends
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        let Some(event) = backend.wait_for_ui_snapshot_event(timeout, after_version)? else {
            return Ok(None);
        };
        self.events.push(
            "observation",
            "ui_snapshot_event",
            Some(instance_id.to_string()),
            event_details!({
                "version" => event.version,
                "screen" => format!("{:?}", event.snapshot.screen).to_ascii_lowercase(),
                "open_modal" => event
                    .snapshot
                    .open_modal
                    .map(|modal| format!("{modal:?}").to_ascii_lowercase())
                    .unwrap_or_else(|| "none".to_string())
            }),
        );
        Ok(Some(event))
    }

    pub fn backend_kind(&self, instance_id: &str) -> Result<&'static str> {
        let backend = self
            .backends
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        Ok(backend.backend_kind())
    }

    pub fn supports_ui_snapshot(&self, instance_id: &str) -> Result<bool> {
        let backend = self
            .backends
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        Ok(backend.supports_ui_snapshot())
    }

    pub fn send_keys(&mut self, instance_id: &str, keys: &str) -> Result<()> {
        let normalized = normalize_key_stream(keys);
        {
            let backend = self
                .backends
                .get_mut(instance_id)
                .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
            backend.send_keys(normalized.as_ref())?;
        }

        self.events.push(
            "action",
            "send_keys",
            Some(instance_id.to_string()),
            event_details!({ "bytes" => normalized.len() }),
        );
        Ok(())
    }

    pub fn send_key(&mut self, instance_id: &str, key: ToolKey, repeat: u16) -> Result<()> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "action",
            "send_key",
            Some(instance_id.to_string()),
            event_details!({
                "key" => format!("{key:?}").to_ascii_lowercase(),
                "repeat" => repeat.max(1)
            }),
        );
        backend.send_key(key, repeat)
    }

    pub fn click_button(&mut self, instance_id: &str, label: &str) -> Result<()> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "action",
            "click_button",
            Some(instance_id.to_string()),
            event_details!({ "label" => label }),
        );
        backend.click_button(label)
    }

    pub fn activate_control(&mut self, instance_id: &str, control_id: ControlId) -> Result<()> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "action",
            "activate_control",
            Some(instance_id.to_string()),
            event_details!({ "control_id" => format!("{control_id:?}") }),
        );
        backend.activate_control(control_id)
    }

    pub fn click_target(&mut self, instance_id: &str, selector: &str) -> Result<()> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "action",
            "click_target",
            Some(instance_id.to_string()),
            event_details!({ "selector" => selector }),
        );
        backend.click_target(selector)
    }

    pub fn fill_input(&mut self, instance_id: &str, selector: &str, value: &str) -> Result<()> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "action",
            "fill_input",
            Some(instance_id.to_string()),
            event_details!({
                "selector" => selector,
                "bytes" => value.len()
            }),
        );
        backend.fill_input(selector, value)
    }

    pub fn fill_field(&mut self, instance_id: &str, field_id: FieldId, value: &str) -> Result<()> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "action",
            "fill_field",
            Some(instance_id.to_string()),
            event_details!({
                "field_id" => format!("{field_id:?}"),
                "bytes" => value.len()
            }),
        );
        backend.fill_field(field_id, value)
    }

    pub fn activate_list_item(
        &mut self,
        instance_id: &str,
        list_id: ListId,
        item_id: &str,
    ) -> Result<()> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "action",
            "activate_list_item",
            Some(instance_id.to_string()),
            event_details!({
                "list_id" => format!("{list_id:?}"),
                "item_id" => item_id
            }),
        );
        backend.activate_list_item(list_id, item_id)
    }

    pub fn create_contact_invitation(
        &mut self,
        instance_id: &str,
        receiver_authority_id: &str,
    ) -> Result<String> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "action",
            "create_contact_invitation",
            Some(instance_id.to_string()),
            event_details!({
                "receiver_authority_id" => receiver_authority_id
            }),
        );
        backend
            .submit_create_contact_invitation(receiver_authority_id)
            .map(|submitted| submitted.value.code)
    }

    pub fn submit_semantic_command_via_ui(
        &mut self,
        instance_id: &str,
        request: BackendSemanticCommandRequest,
    ) -> Result<BackendSemanticCommandResponse> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "action",
            "submit_semantic_command_via_ui",
            Some(instance_id.to_string()),
            event_details!({
                "intent" => format!("{:?}", request.kind()).to_ascii_lowercase()
            }),
        );
        backend.submit_semantic_command(request)
    }

    pub fn wait_for_diagnostic_observation(
        &mut self,
        instance_id: &str,
        wait: DiagnosticObservationWait<'_>,
        timeout_ms: u64,
    ) -> Result<String> {
        match wait {
            DiagnosticObservationWait::Target { selector } => {
                let backend = self
                    .backends
                    .get(instance_id)
                    .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
                if let Some(result) = backend.wait_for_diagnostic_observation_probe(
                    DiagnosticObservationProbe::Target(selector),
                    timeout_ms,
                ) {
                    let screen = result?;
                    self.events.push(
                        "observation",
                        "wait_for_selector",
                        Some(instance_id.to_string()),
                        event_details!({
                            "selector" => selector,
                            "timeout_ms" => timeout_ms
                        }),
                    );
                    return Ok(screen);
                }

                bail!(
                    "wait_for_selector is not supported by backend {}",
                    backend.backend_kind()
                )
            }
            DiagnosticObservationWait::Pattern { pattern, source } => {
                if matches!(source, ScreenSource::Dom) {
                    let patterns = wait_pattern_candidates(pattern);
                    if let Some(result) = self
                        .backends
                        .get(instance_id)
                        .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?
                        .wait_for_diagnostic_observation_probe(
                            DiagnosticObservationProbe::DomPatterns(&patterns),
                            timeout_ms,
                        )
                    {
                        let screen = result?;
                        self.events.push(
                            "observation",
                            "wait_for",
                            Some(instance_id.to_string()),
                            event_details!({
                                "pattern" => pattern,
                                "normalized_pattern" => normalize_screen(pattern),
                                "attempts" => 1_u64,
                                "matched_view" => "normalized",
                                "source" => format!("{source:?}").to_ascii_lowercase()
                            }),
                        );
                        return Ok(screen);
                    }
                }

                let poll_ms: u64 = 40;
                let mut attempts = 0_u64;
                let deadline = Instant::now() + Duration::from_millis(timeout_ms);

                loop {
                    if Instant::now() >= deadline {
                        break;
                    }
                    let screen = self.diagnostic_screen_with_source(instance_id, source)?;
                    let normalized = normalize_screen(&screen);
                    if wait_pattern_matches(&normalized, pattern) {
                        self.events.push(
                            "observation",
                            "wait_for",
                            Some(instance_id.to_string()),
                            event_details!({
                                "pattern" => pattern,
                                "normalized_pattern" => normalize_screen(pattern),
                                "attempts" => attempts + 1,
                                "matched_view" => "normalized",
                                "source" => format!("{source:?}").to_ascii_lowercase()
                            }),
                        );
                        return Ok(screen);
                    }
                    attempts = attempts.saturating_add(1);
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    let remaining = deadline.saturating_duration_since(now);
                    let delay = remaining.min(Duration::from_millis(poll_ms));
                    if !delay.is_zero() {
                        blocking_sleep(delay);
                    }
                }

                self.events.push(
                    "error",
                    "wait_for_timeout",
                    Some(instance_id.to_string()),
                    event_details!({
                        "pattern" => pattern,
                        "normalized_pattern" => normalize_screen(pattern),
                        "timeout_ms" => timeout_ms,
                        "source" => format!("{source:?}").to_ascii_lowercase()
                    }),
                );
                bail!(
                    "wait_for timed out for instance {instance_id} pattern {pattern:?} timeout_ms={timeout_ms}"
                )
            }
        }
    }

    pub fn tail_log(&mut self, instance_id: &str, lines: usize) -> Result<Vec<String>> {
        let backend = self
            .backends
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        let result = backend.tail_log(lines)?;
        self.events.push(
            "observation",
            "tail_log",
            Some(instance_id.to_string()),
            event_details!({ "requested_lines" => lines, "returned_lines" => result.len() }),
        );
        Ok(result)
    }

    pub fn read_clipboard(&self, instance_id: &str) -> Result<String> {
        let backend = self
            .backends
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        let text = backend.read_clipboard()?;

        self.events.push(
            "observation",
            "read_clipboard",
            Some(instance_id.to_string()),
            event_details!({ "bytes" => text.len() }),
        );
        Ok(text)
    }

    pub fn get_authority_id(&mut self, instance_id: &str) -> Result<Option<String>> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        let authority_id = backend.authority_id()?;
        self.events.push(
            "observation",
            "get_authority_id",
            Some(instance_id.to_string()),
            event_details!({
                "source" => if authority_id.is_some() { "backend" } else { "unavailable" }
            }),
        );
        Ok(authority_id)
    }

    pub fn prepare_device_enrollment_invitee_authority(
        &mut self,
        instance_id: &str,
    ) -> Result<String> {
        let mode = self
            .instance_modes
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        let data_dir = self
            .instance_data_dirs
            .get(instance_id)
            .ok_or_else(|| anyhow!("missing data_dir for instance_id: {instance_id}"))?;
        let authority_path = data_dir.join(".harness-device-enrollment-invitee-authority");
        if let Ok(raw) = fs::read_to_string(&authority_path) {
            let authority_id = raw.trim();
            if !authority_id.is_empty() && authority_id.parse::<AuthorityId>().is_ok() {
                self.events.push(
                    "observation",
                    "prepare_device_enrollment_invitee_authority",
                    Some(instance_id.to_string()),
                    event_details!({
                        "source" => authority_path.display().to_string(),
                        "mode" => "reused",
                        "authority_id" => authority_id
                    }),
                );
                return Ok(authority_id.to_string());
            }
        }

        let seed = format!(
            "harness-device-enrollment-invitee:{}:{}",
            instance_id,
            data_dir.display()
        );
        let authority_id = AuthorityId::new_from_entropy(hash(seed.as_bytes())).to_string();
        let provisional_device_id = ids::device_id(&format!("{seed}:device")).to_string();
        fs::create_dir_all(data_dir).with_context(|| {
            format!(
                "failed to create data dir for prepared invitee authority {}",
                data_dir.display()
            )
        })?;
        fs::write(&authority_path, format!("{authority_id}\n")).with_context(|| {
            format!(
                "failed to persist prepared invitee authority {}",
                authority_path.display()
            )
        })?;
        self.events.push(
            "observation",
            "prepare_device_enrollment_invitee_authority",
            Some(instance_id.to_string()),
            event_details!({
                "source" => authority_path.display().to_string(),
                "mode" => "created",
                "authority_id" => authority_id.clone()
            }),
        );

        match mode {
            InstanceMode::Local => {
                self.restart(instance_id)?;
                self.wait_for_backend_health(instance_id, BACKEND_HEALTH_TIMEOUT)?;
                self.backends
                    .get(instance_id)
                    .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?
                    .wait_until_ready(BACKEND_READY_TIMEOUT)?;
                self.events.push(
                    "lifecycle",
                    "prepare_device_enrollment_invitee_authority_ready",
                    Some(instance_id.to_string()),
                    event_details!({
                        "authority_id" => authority_id.clone(),
                        "timeout_ms" => BACKEND_READY_TIMEOUT.as_millis()
                    }),
                );
            }
            InstanceMode::Browser => {
                self.backends
                    .get_mut(instance_id)
                    .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?
                    .stage_runtime_identity(&authority_id, &provisional_device_id)?;
                self.wait_for_backend_health(instance_id, BACKEND_HEALTH_TIMEOUT)?;
                self.backends
                    .get(instance_id)
                    .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?
                    .wait_until_ready(BACKEND_READY_TIMEOUT)?;
                self.events.push(
                    "lifecycle",
                    "prepare_device_enrollment_invitee_authority_ready",
                    Some(instance_id.to_string()),
                    event_details!({
                        "authority_id" => authority_id.clone(),
                        "device_id" => provisional_device_id,
                        "timeout_ms" => BACKEND_READY_TIMEOUT.as_millis()
                    }),
                );
            }
            InstanceMode::Ssh => {}
        }
        Ok(authority_id)
    }

    pub fn restart(&mut self, instance_id: &str) -> Result<()> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "lifecycle",
            "restart",
            Some(instance_id.to_string()),
            event_details!(),
        );
        backend.restart()
    }

    pub fn kill(&mut self, instance_id: &str) -> Result<()> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "lifecycle",
            "kill",
            Some(instance_id.to_string()),
            event_details!(),
        );
        backend.stop()
    }

    pub fn event_snapshot(&self) -> Vec<crate::events::HarnessEvent> {
        self.events.snapshot()
    }
}

struct BrowserAppUrlProvision {
    url: String,
    server: Option<OwnedWebServer>,
    log_path: Option<PathBuf>,
}

fn owned_web_server_log_tail(log_path: Option<&Path>) -> Option<String> {
    let log_path = log_path?;
    let contents = fs::read_to_string(log_path).ok()?;
    let mut lines = contents.lines().rev().take(80).collect::<Vec<_>>();
    lines.reverse();
    Some(lines.join("\n"))
}

fn provision_browser_app_url(config: &RunConfig) -> Result<BrowserAppUrlProvision> {
    let has_browser = config
        .instances
        .iter()
        .any(|instance| matches!(instance.mode, InstanceMode::Browser));
    if !has_browser {
        return Ok(BrowserAppUrlProvision {
            url: "http://127.0.0.1:4173".to_string(),
            server: None,
            log_path: None,
        });
    }

    if let Some(existing) = config.instances.iter().find_map(browser_app_url_from_env) {
        return Ok(BrowserAppUrlProvision {
            url: existing,
            server: None,
            log_path: None,
        });
    }

    if let Some(existing) = browser_app_url_from_process_env() {
        return Ok(BrowserAppUrlProvision {
            url: existing,
            server: None,
            log_path: None,
        });
    }

    let port = choose_available_loopback_port(4173, 32)?;
    let script = harness_repo_root().join("scripts/web/serve-static.sh");
    let artifact_root = config
        .run
        .artifact_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("artifacts/harness").join(&config.run.name));
    let artifact_root = absolutize_path(artifact_root);
    fs::create_dir_all(&artifact_root)?;
    let log_path = artifact_root.join("owned-web-server.log");
    let log_file = File::create(&log_path)?;
    let log_file_err = log_file.try_clone()?;
    let child = Command::new(&script)
        .arg(port.to_string())
        .env("AURA_HARNESS_WEB_BUILD_PROFILE", "release")
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err))
        .spawn()
        .map_err(|error| {
            anyhow!(
                "failed to spawn owned web server {}: {error}",
                script.display()
            )
        })?;
    Ok(BrowserAppUrlProvision {
        url: format!("http://127.0.0.1:{port}"),
        server: Some(OwnedWebServer {
            child,
            url: format!("http://127.0.0.1:{port}"),
        }),
        log_path: Some(log_path),
    })
}

fn instance_has_browser_app_url(instance: &crate::config::InstanceConfig) -> bool {
    browser_app_url_from_env(instance).is_some()
}

fn browser_app_url_from_env(instance: &crate::config::InstanceConfig) -> Option<String> {
    instance.env.iter().find_map(|item| {
        let (key, value) = item.split_once('=')?;
        let key = key.trim();
        ((key == "AURA_HARNESS_BROWSER_APP_URL") || (key == "AURA_WEB_APP_URL"))
            .then(|| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn browser_app_url_from_process_env() -> Option<String> {
    ["AURA_HARNESS_BROWSER_APP_URL", "AURA_WEB_APP_URL"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn owned_web_server_ready_timeout() -> Duration {
    std::env::var("AURA_HARNESS_WEB_SERVER_READY_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(WEB_SERVER_READY_TIMEOUT)
}

fn wait_for_owned_web_server(
    log_path: Option<&Path>,
    server: &mut OwnedWebServer,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = server.child.try_wait()? {
            let log_tail = owned_web_server_log_tail(log_path)
                .filter(|tail| !tail.trim().is_empty())
                .map(|tail| format!("\n--- owned-web-server.log (tail) ---\n{tail}"))
                .unwrap_or_default();
            bail!(
                "owned web server exited before becoming ready (status={status}) url={}{}",
                server.url,
                log_tail
            );
        }
        if http_server_ready(&server.url) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let log_tail = owned_web_server_log_tail(log_path)
                .filter(|tail| !tail.trim().is_empty())
                .map(|tail| format!("\n--- owned-web-server.log (tail) ---\n{tail}"))
                .unwrap_or_default();
            bail!(
                "owned web server did not become ready within {:?}: {}{}",
                timeout,
                server.url,
                log_tail
            );
        }
        blocking_sleep(WEB_SERVER_POLL_INTERVAL);
    }
}

fn http_server_ready(url: &str) -> bool {
    let Some(host_port) = url.strip_prefix("http://") else {
        return false;
    };
    let mut parts = host_port.splitn(2, ':');
    let Some(host) = parts.next() else {
        return false;
    };
    let Some(port) = parts.next().and_then(|value| value.parse::<u16>().ok()) else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect((host, port)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    if std::io::Write::write_all(
        &mut stream,
        format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .is_err()
    {
        return false;
    }
    let mut response = [0u8; 16];
    std::io::Read::read(&mut stream, &mut response)
        .ok()
        .is_some_and(|read| read > 0)
}

fn choose_available_loopback_port(start: u16, attempts: u16) -> Result<u16> {
    for offset in 0..attempts {
        let port = start.saturating_add(offset);
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            continue;
        }
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    bail!(
        "failed to allocate loopback port in range 127.0.0.1:{}-{}",
        start,
        start.saturating_add(attempts.saturating_sub(1))
    )
}

fn harness_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn normalize_key_stream(keys: &str) -> Cow<'_, str> {
    if keys.contains('\n') {
        Cow::Owned(keys.replace('\n', "\r"))
    } else {
        Cow::Borrowed(keys)
    }
}

fn wait_pattern_matches(normalized_screen: &str, pattern: &str) -> bool {
    wait_pattern_candidates(pattern)
        .iter()
        .any(|candidate| normalized_screen.contains(candidate))
}

fn wait_pattern_candidates(pattern: &str) -> Vec<String> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Vec::new();
    }

    let mut candidates = vec![pattern.to_string()];
    let normalized_pattern = normalize_screen(pattern);
    if normalized_pattern != pattern {
        candidates.push(normalized_pattern);
    }
    if pattern.eq_ignore_ascii_case("Map") {
        candidates.push("Neighborhood".to_string());
    }
    if pattern.eq_ignore_ascii_case("Can enter:") {
        candidates.push("Access:".to_string());
    }
    if pattern.eq_ignore_ascii_case("Map → Limited")
        || pattern.eq_ignore_ascii_case("Map -> Limited")
    {
        candidates.push("Access: Limited".to_string());
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn absolutize_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    if let Ok(cwd) = std::env::current_dir() {
        return cwd.join(path);
    }
    path
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    match signal::kill(Pid::from_raw(pid as i32), None) {
        Ok(()) => true,
        Err(Errno::EPERM) => true,
        Err(_) => false,
    }
}

fn clear_directory_contents(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)
        .map_err(|error| anyhow!("failed to create data_dir {}: {error}", dir.display()))?;

    for entry in fs::read_dir(dir)
        .map_err(|error| anyhow!("failed to read data_dir {}: {error}", dir.display()))?
    {
        let entry = entry.map_err(|error| {
            anyhow!(
                "failed to read entry in data_dir {}: {error}",
                dir.display()
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            anyhow!(
                "failed to inspect entry {} in data_dir {}: {error}",
                path.display(),
                dir.display()
            )
        })?;

        if file_type.is_dir() {
            fs::remove_dir_all(&path).map_err(|error| {
                anyhow!(
                    "failed to remove stale directory {} in data_dir {}: {error}",
                    path.display(),
                    dir.display()
                )
            })?;
        } else {
            // Remove regular files and symlinks uniformly.
            fs::remove_file(&path).map_err(|error| {
                anyhow!(
                    "failed to remove stale file {} in data_dir {}: {error}",
                    path.display(),
                    dir.display()
                )
            })?;
        }
    }

    Ok(())
}
impl Drop for HarnessCoordinator {
    fn drop(&mut self) {
        let _ = self.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        clear_directory_contents, normalize_key_stream, wait_pattern_matches, HarnessCoordinator,
    };
    use crate::config::{InstanceConfig, InstanceMode, RunConfig, RunSection};
    use std::net::TcpListener;
    use std::path::PathBuf;

    #[allow(clippy::disallowed_methods)]
    fn unique_test_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "aura-harness-coordinator-{label}-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|error| panic!("create coordinator temp dir failed: {error}"));
        root
    }

    #[test]
    fn normalize_key_stream_rewrites_newline_to_carriage_return() {
        assert_eq!(
            normalize_key_stream("hello\nworld").as_ref(),
            "hello\rworld"
        );
    }

    #[test]
    fn normalize_key_stream_keeps_plain_text() {
        assert_eq!(normalize_key_stream("abc123").as_ref(), "abc123");
    }

    #[test]
    fn wait_pattern_matches_normalized_dynamic_tokens() {
        let screen = "message id #<n> at <time>";
        assert!(wait_pattern_matches(screen, "#123"));
        assert!(wait_pattern_matches(screen, "12:34:56"));
    }

    #[test]
    fn wait_pattern_matches_exact_literals() {
        let screen = "Neighborhood Chat Contacts Notifications Settings";
        assert!(wait_pattern_matches(screen, "Chat Contacts"));
        assert!(!wait_pattern_matches(screen, "Missing Token"));
    }

    #[test]
    fn wait_pattern_matches_map_alias_for_neighborhood() {
        let screen = "Neighborhood Chat Contacts Notifications Settings";
        assert!(wait_pattern_matches(screen, "Map"));
    }

    #[test]
    fn wait_pattern_matches_can_enter_alias_for_access() {
        let screen = "Authority: authority-local (local) Access: Limited";
        assert!(wait_pattern_matches(screen, "Can enter:"));
    }

    #[test]
    fn wait_pattern_matches_map_limited_alias_for_access_limited() {
        let screen = "Welcome to Aura Access: Limited";
        assert!(wait_pattern_matches(screen, "Map → Limited"));
        assert!(wait_pattern_matches(screen, "Map -> Limited"));
    }

    #[test]
    fn clear_directory_contents_removes_files_and_subdirectories() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let root = temp.path().join("instance");
        std::fs::create_dir_all(root.join("nested"))
            .unwrap_or_else(|error| panic!("failed to create nested dir: {error}"));
        std::fs::write(root.join("stale.txt"), "stale")
            .unwrap_or_else(|error| panic!("failed to create stale file: {error}"));
        std::fs::write(root.join("nested").join("child.txt"), "child")
            .unwrap_or_else(|error| panic!("failed to create nested stale file: {error}"));

        clear_directory_contents(&root).unwrap_or_else(|error| panic!("{error}"));

        let entries = std::fs::read_dir(&root).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(entries.count(), 0, "stale entries were not cleared");
    }

    #[test]
    fn coordinator_preserves_instance_startup_order_from_config() {
        let run = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "startup-order-test".to_string(),
                pty_rows: Some(10),
                pty_cols: Some(40),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(7),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
            },
            instances: vec![
                InstanceConfig {
                    id: "alpha".to_string(),
                    mode: InstanceMode::Ssh,
                    data_dir: unique_test_dir("alpha"),
                    device_id: None,
                    bind_address: "127.0.0.1:45001".to_string(),
                    demo_mode: false,
                    command: None,
                    args: vec![],
                    env: vec![],
                    log_path: None,
                    ssh_host: Some("example.org".to_string()),
                    ssh_user: Some("dev".to_string()),
                    ssh_port: Some(22),
                    ssh_strict_host_key_checking: true,
                    ssh_known_hosts_file: Some(unique_test_dir("known-hosts").join("known_hosts")),
                    ssh_fingerprint: Some("SHA256:test".to_string()),
                    ssh_require_fingerprint: true,
                    ssh_dry_run: true,
                    remote_workdir: None,
                    lan_discovery: None,
                    tunnel: None,
                },
                InstanceConfig {
                    id: "beta".to_string(),
                    mode: InstanceMode::Ssh,
                    data_dir: unique_test_dir("beta"),
                    device_id: None,
                    bind_address: "127.0.0.1:45002".to_string(),
                    demo_mode: false,
                    command: None,
                    args: vec![],
                    env: vec![],
                    log_path: None,
                    ssh_host: Some("example.org".to_string()),
                    ssh_user: Some("dev".to_string()),
                    ssh_port: Some(22),
                    ssh_strict_host_key_checking: true,
                    ssh_known_hosts_file: Some(unique_test_dir("known-hosts").join("known_hosts")),
                    ssh_fingerprint: Some("SHA256:test".to_string()),
                    ssh_require_fingerprint: true,
                    ssh_dry_run: true,
                    remote_workdir: None,
                    lan_discovery: None,
                    tunnel: None,
                },
            ],
        };

        let coordinator =
            HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            coordinator.instance_order,
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn verify_bind_address_released_detects_busy_port() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("{error}"));
        let bind_address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("{error}"))
            .to_string();
        let run = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "busy-port-test".to_string(),
                pty_rows: Some(10),
                pty_cols: Some(40),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: Some(11),
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
                runtime_substrate: crate::config::RuntimeSubstrate::default(),
            },
            instances: vec![InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir: unique_test_dir("busy-port"),
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
        };

        let coordinator =
            HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}"));
        let error = coordinator
            .verify_bind_address_released("alice")
            .err()
            .unwrap_or_else(|| panic!("busy port should fail teardown verification"));
        assert!(error.to_string().contains("did not release bind address"));
        drop(listener);
    }
}
