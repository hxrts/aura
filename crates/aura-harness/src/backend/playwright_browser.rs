use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use aura_app::ui::contract::{
    classify_screen_item_id, list_item_selector, nav_control_id_for_screen,
    semantic_settings_section_item_id, ControlId, FieldId, ListId, UiSnapshot,
};
use aura_app::ui::scenarios::{
    IntentAction, SemanticCommandRequest, SemanticCommandResponse, SettingsSection,
};
use aura_app::ui::types::BootstrapRuntimeIdentity;
use aura_core::{AuthorityId, DeviceId};
use nix::poll::{poll, PollFd, PollFlags};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::backend::{
    DiagnosticBackend, InstanceBackend, ObservationBackend, RawUiBackend, SharedSemanticBackend,
    UiSnapshotEvent,
};
use crate::config::InstanceConfig;
use crate::tool_api::ToolKey;

const DEFAULT_PAGE_GOTO_TIMEOUT_MS: u64 = 90_000;
const DEFAULT_HARNESS_READY_TIMEOUT_MS: u64 = 90_000;
const DEFAULT_RPC_TIMEOUT_MS: u64 = 15_000;
const WAIT_RPC_TIMEOUT_MARGIN_MS: u64 = 5_000;
const DIAGNOSTIC_DOM_RPC_TIMEOUT_MS: u64 = 2_000;
const SUBMIT_SEMANTIC_COMMAND_RPC_TIMEOUT_MS: u64 = 60_000 + WAIT_RPC_TIMEOUT_MARGIN_MS;
const STAGE_RUNTIME_IDENTITY_RPC_TIMEOUT_MS: u64 = 60_000 + WAIT_RPC_TIMEOUT_MARGIN_MS;
const DEFAULT_START_MAX_ATTEMPTS: u32 = 3;
const DEFAULT_START_RETRY_BACKOFF_MS: u64 = 1_200;
const MAX_START_ATTEMPTS: u32 = 10;
const MAX_TIMEOUT_MS: u64 = 600_000;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BackendState {
    Stopped,
    Running,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserDiagnosticScreenPayload {
    authoritative_screen: String,
    #[serde(default, rename = "screen")]
    _screen: Option<String>,
    #[serde(default, rename = "raw_screen")]
    _raw_screen: Option<String>,
    #[serde(default, rename = "normalized_screen")]
    _normalized_screen: Option<String>,
    #[serde(default, rename = "capture_consistency")]
    _capture_consistency: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserUiSnapshotEventPayload {
    version: u64,
    snapshot: UiSnapshot,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserTailLogPayload {
    lines: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserClipboardPayload {
    text: String,
}

fn decode_rpc_payload<T: DeserializeOwned>(
    payload: Value,
    context: impl FnOnce() -> String,
) -> Result<T> {
    serde_json::from_value(payload).with_context(context)
}

struct RunningSession {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    stderr_thread: Option<thread::JoinHandle<()>>,
    stderr_log: Arc<Mutex<Vec<String>>>,
    request_id: u64,
    rpc_timeout_ms: u64,
}

impl RunningSession {
    fn stderr_tail(&self, lines: usize) -> Vec<String> {
        self.stderr_log
            .blocking_lock()
            .iter()
            .rev()
            .take(lines)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    #[allow(clippy::disallowed_methods)]
    fn wait_for_response_ready(
        &mut self,
        method: &str,
        request_id: u64,
        deadline: Instant,
    ) -> Result<()> {
        let now = Instant::now();
        if now >= deadline {
            return self.rpc_timeout(method, request_id, "deadline exceeded before response");
        }

        let remaining = deadline.saturating_duration_since(now);
        let timeout_ms = remaining.as_millis().clamp(1, i32::MAX as u128) as i32;
        let fd = self.stdout.get_ref().as_raw_fd();
        let mut pollfd = [PollFd::new(fd, PollFlags::POLLIN)];
        let poll_result = poll(&mut pollfd, timeout_ms);
        let pollfd = pollfd[0];
        let poll_result = poll_result.map_err(|error| {
            anyhow!(
                "Playwright driver {method} failed while polling for response to request {request_id}: {error}"
            )
        })?;
        if poll_result == 0 {
            return self.rpc_timeout(method, request_id, "timed out waiting for driver stdout");
        }

        let revents = pollfd.revents().unwrap_or(PollFlags::empty());
        if revents.contains(PollFlags::POLLNVAL) {
            return Err(anyhow!(
                "Playwright driver {method} encountered invalid stdout fd for request {request_id}"
            ));
        }
        if revents.contains(PollFlags::POLLERR) {
            let status = self.child.try_wait().ok().flatten();
            return Err(anyhow!(
                "Playwright driver {method} encountered stdout error for request {request_id} (child_status={status:?})"
            ));
        }

        Ok(())
    }

    fn rpc_timeout(&mut self, method: &str, request_id: u64, context: &str) -> Result<()> {
        let child_status = self.child.try_wait().ok().flatten();
        let stderr_tail = self.stderr_tail(40);
        let stderr_block = if stderr_tail.is_empty() {
            "none".to_string()
        } else {
            stderr_tail.join("\n")
        };
        Err(anyhow!(
            "Playwright driver {method} timed out for request {request_id}: {context} (child_status={child_status:?}, stderr_tail=\n{stderr_block})"
        ))
    }

    #[allow(clippy::disallowed_methods)]
    fn rpc_call_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout_ms: u64,
    ) -> Result<Value> {
        self.request_id = self.request_id.saturating_add(1);
        let request_id = self.request_id;
        let payload = json!({
            "id": request_id,
            "method": method,
            "params": params,
        });
        writeln!(self.stdin, "{payload}")
            .with_context(|| format!("failed writing Playwright request {method}"))?;
        self.stdin
            .flush()
            .with_context(|| format!("failed flushing Playwright request {method}"))?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut line = Vec::new();
        loop {
            self.wait_for_response_ready(method, request_id, deadline)?;
            line.clear();
            let read = self
                .stdout
                .read_until(b'\n', &mut line)
                .with_context(|| format!("failed reading Playwright response for {method}"))?;
            if read == 0 {
                let child_status = self.child.try_wait().ok().flatten();
                let stderr_tail = self.stderr_tail(40);
                let stderr_block = if stderr_tail.is_empty() {
                    "none".to_string()
                } else {
                    stderr_tail.join("\n")
                };
                bail!(
                    "Playwright driver closed stdout while awaiting {method} (child_status={child_status:?}, stderr_tail=\n{stderr_block})"
                );
            }

            let line_text =
                std::str::from_utf8(&line).with_context(|| "Playwright response was not UTF-8")?;
            let response: Value = serde_json::from_str(line_text.trim_end())
                .with_context(|| "invalid driver JSON line")?;
            if response
                .get("id")
                .and_then(Value::as_u64)
                .is_some_and(|value| value != request_id)
            {
                continue;
            }

            if response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                return Ok(response.get("result").cloned().unwrap_or(Value::Null));
            }

            let error = response
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| response.to_string());
            let stderr_tail = self.stderr_tail(40);
            let stderr_block = if stderr_tail.is_empty() {
                "none".to_string()
            } else {
                stderr_tail.join("\n")
            };
            bail!("Playwright driver {method} failed: {error} (stderr_tail=\n{stderr_block})");
        }
    }

    fn rpc_call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.rpc_call_with_timeout(method, params, self.rpc_timeout_ms)
    }
}

pub struct PlaywrightBrowserBackend {
    config: InstanceConfig,
    state: BackendState,
    session: Option<Mutex<RunningSession>>,
    stderr_log: Arc<Mutex<Vec<String>>>,
    app_url: String,
    headless: bool,
    capture_screenshots: bool,
    artifact_dir: PathBuf,
    page_goto_timeout_ms: u64,
    harness_ready_timeout_ms: u64,
    rpc_timeout_ms: u64,
    start_max_attempts: u32,
    start_retry_backoff_ms: u64,
}

impl PlaywrightBrowserBackend {
    pub fn new(config: InstanceConfig) -> Result<Self> {
        let app_url = browser_app_url(&config.env);
        let headless = parse_bool_setting("AURA_HARNESS_BROWSER_HEADLESS", &config.env, true)?;
        let capture_screenshots = parse_bool_setting(
            "AURA_HARNESS_BROWSER_SNAPSHOT_SCREENSHOT",
            &config.env,
            false,
        )?;
        let artifact_dir = env_value("AURA_HARNESS_BROWSER_ARTIFACT_DIR", &config.env)
            .map(PathBuf::from)
            .unwrap_or_else(|| config.data_dir.join("playwright-artifacts"));
        let page_goto_timeout_ms = parse_u64_setting(
            "AURA_HARNESS_BROWSER_PAGE_GOTO_TIMEOUT_MS",
            &config.env,
            DEFAULT_PAGE_GOTO_TIMEOUT_MS,
            1,
            MAX_TIMEOUT_MS,
        )?;
        let harness_ready_timeout_ms = parse_u64_setting(
            "AURA_HARNESS_BROWSER_HARNESS_READY_TIMEOUT_MS",
            &config.env,
            DEFAULT_HARNESS_READY_TIMEOUT_MS,
            1,
            MAX_TIMEOUT_MS,
        )?;
        let rpc_timeout_ms = parse_u64_setting(
            "AURA_HARNESS_BROWSER_RPC_TIMEOUT_MS",
            &config.env,
            DEFAULT_RPC_TIMEOUT_MS,
            1,
            MAX_TIMEOUT_MS,
        )?;
        let start_max_attempts = parse_u32_setting(
            "AURA_HARNESS_BROWSER_START_MAX_ATTEMPTS",
            &config.env,
            DEFAULT_START_MAX_ATTEMPTS,
            1,
            MAX_START_ATTEMPTS,
        )?;
        let start_retry_backoff_ms = parse_u64_setting(
            "AURA_HARNESS_BROWSER_START_RETRY_BACKOFF_MS",
            &config.env,
            DEFAULT_START_RETRY_BACKOFF_MS,
            0,
            MAX_TIMEOUT_MS,
        )?;

        Ok(Self {
            config,
            state: BackendState::Stopped,
            session: None,
            stderr_log: Arc::new(Mutex::new(Vec::new())),
            app_url,
            headless,
            capture_screenshots,
            artifact_dir,
            page_goto_timeout_ms,
            harness_ready_timeout_ms,
            rpc_timeout_ms,
            start_max_attempts,
            start_retry_backoff_ms,
        })
    }

    fn with_session<T>(
        &self,
        operation: impl FnOnce(&mut RunningSession) -> Result<T>,
    ) -> Result<T> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow!("instance {} is not running", self.config.id))?;
        let mut session = session.blocking_lock();
        operation(&mut session)
    }

    fn command_spec(&self) -> Result<(String, Vec<String>, Option<PathBuf>)> {
        if let Some(command) = &self.config.command {
            let cwd =
                env_value("AURA_HARNESS_BROWSER_DRIVER_CWD", &self.config.env).map(PathBuf::from);
            return Ok((command.clone(), self.config.args.clone(), cwd));
        }

        let driver_script = default_driver_script_path()?;
        let driver_cwd = driver_script
            .parent()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("invalid driver script path {}", driver_script.display()))?;
        Ok((
            "node".to_string(),
            vec![driver_script.to_string_lossy().to_string()],
            Some(driver_cwd),
        ))
    }

    fn stop_inner(&mut self) -> Result<()> {
        let Some(session_mutex) = self.session.take() else {
            self.state = BackendState::Stopped;
            return Ok(());
        };

        let mut session = session_mutex.into_inner();
        let _ = session.rpc_call("stop", json!({ "instance_id": self.config.id }));
        let _ = session.child.kill();
        let _ = session.child.wait();
        if let Some(handle) = session.stderr_thread.take() {
            let _ = handle.join();
        }
        self.state = BackendState::Stopped;
        Ok(())
    }

    pub fn stage_runtime_identity(&mut self, authority_id: &str, device_id: &str) -> Result<()> {
        let authority_id = authority_id
            .parse::<AuthorityId>()
            .with_context(|| format!("invalid authority id for runtime staging: {authority_id}"))?;
        let device_id = device_id
            .parse::<DeviceId>()
            .with_context(|| format!("invalid device id for runtime staging: {device_id}"))?;
        let runtime_identity_json =
            serde_json::to_string(&BootstrapRuntimeIdentity::new(authority_id, device_id))
                .context("failed to encode staged runtime identity")?;
        self.with_session(|session| {
            session.rpc_call_with_timeout(
                "stage_runtime_identity",
                json!({
                    "instance_id": self.config.id,
                    "runtime_identity_json": runtime_identity_json,
                }),
                STAGE_RUNTIME_IDENTITY_RPC_TIMEOUT_MS,
            )?;
            Ok(())
        })
    }

    fn submit_semantic_command(
        &mut self,
        request: SemanticCommandRequest,
    ) -> Result<SemanticCommandResponse> {
        let payload = serde_json::to_value(&request)
            .context("failed to encode browser semantic command request")?;
        let response = self.with_session(|session| {
            session.rpc_call_with_timeout(
                "submit_semantic_command",
                json!({
                    "instance_id": self.config.id,
                    "request": payload,
                }),
                SUBMIT_SEMANTIC_COMMAND_RPC_TIMEOUT_MS,
            )
        })?;
        serde_json::from_value(response)
            .context("failed to decode browser semantic command response")
    }
}

impl InstanceBackend for PlaywrightBrowserBackend {
    fn id(&self) -> &str {
        &self.config.id
    }

    fn backend_kind(&self) -> &'static str {
        "playwright_browser"
    }

    fn start(&mut self) -> Result<()> {
        if self.state == BackendState::Running {
            return Ok(());
        }

        let (command, args, cwd) = self.command_spec()?;
        fs::create_dir_all(&self.config.data_dir).with_context(|| {
            format!(
                "failed to create browser data_dir {}",
                self.config.data_dir.display()
            )
        })?;
        fs::create_dir_all(&self.artifact_dir).with_context(|| {
            format!(
                "failed to create browser artifact_dir {}",
                self.artifact_dir.display()
            )
        })?;

        let mut child_command = Command::new(command);
        child_command.args(args);
        if let Some(cwd) = cwd {
            child_command.current_dir(cwd);
        }
        child_command.stdin(Stdio::piped());
        child_command.stdout(Stdio::piped());
        child_command.stderr(Stdio::piped());
        child_command.env("AURA_HARNESS_BROWSER", "1");
        for entry in &self.config.env {
            if let Some((key, value)) = entry.split_once('=') {
                child_command.env(key.trim(), value.trim());
            }
        }

        let mut child = child_command
            .spawn()
            .with_context(|| format!("failed to spawn Playwright driver for {}", self.config.id))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("missing Playwright driver stdin for {}", self.config.id))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("missing Playwright driver stdout for {}", self.config.id))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("missing Playwright driver stderr for {}", self.config.id))?;

        let stderr_log = Arc::new(Mutex::new(Vec::new()));
        let stderr_log_for_thread = Arc::clone(&stderr_log);
        let stderr_thread = thread::spawn(move || collect_stderr(stderr, &stderr_log_for_thread));
        self.stderr_log = stderr_log;

        let mut session = RunningSession {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            stderr_thread: Some(stderr_thread),
            stderr_log: Arc::clone(&self.stderr_log),
            request_id: 0,
            rpc_timeout_ms: self.rpc_timeout_ms,
        };
        let start_timeout_ms = self
            .page_goto_timeout_ms
            .saturating_add(self.harness_ready_timeout_ms)
            .saturating_add(30_000)
            .max(self.rpc_timeout_ms);
        let start_result = session.rpc_call_with_timeout(
            "start_page",
            json!({
                "instance_id": self.config.id,
                "app_url": self.app_url,
                "scenario_seed": env_value("AURA_HARNESS_SCENARIO_SEED", &self.config.env),
                "data_dir": absolutize_path(self.config.data_dir.clone()),
                "artifact_dir": absolutize_path(self.artifact_dir.clone()),
                "headless": self.headless,
                "reset_storage": true,
                "page_goto_timeout_ms": self.page_goto_timeout_ms,
                "harness_ready_timeout_ms": self.harness_ready_timeout_ms,
                "start_max_attempts": self.start_max_attempts,
                "start_retry_backoff_ms": self.start_retry_backoff_ms,
            }),
            start_timeout_ms,
        );
        if let Err(error) = start_result {
            let _ = session.rpc_call("stop", json!({ "instance_id": self.config.id }));
            let _ = session.child.kill();
            let _ = session.child.wait();
            if let Some(handle) = session.stderr_thread.take() {
                let _ = handle.join();
            }
            let stderr_tail = self
                .stderr_log
                .blocking_lock()
                .iter()
                .rev()
                .take(40)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>();
            if stderr_tail.is_empty() {
                bail!(
                    "Playwright startup failed for instance {}: {error}",
                    self.config.id
                );
            }
            let joined = stderr_tail.join("\n");
            bail!(
                "Playwright startup failed for instance {}: {error}\nDriver stderr tail:\n{joined}",
                self.config.id
            );
        }

        self.session = Some(Mutex::new(session));
        self.state = BackendState::Running;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.stop_inner()
    }

    fn inject_message(&mut self, message: &str) -> Result<()> {
        self.with_session(|session| {
            session.rpc_call(
                "inject_message",
                json!({
                    "instance_id": self.config.id,
                    "message": message,
                }),
            )?;
            Ok(())
        })
    }

    fn authority_id(&mut self) -> Result<Option<String>> {
        let snapshot = self.ui_snapshot()?;
        Ok(snapshot
            .selected_item_id(ListId::Authorities)
            .map(str::to_string))
    }

    fn health_check(&self) -> Result<bool> {
        if self.state != BackendState::Running {
            return Ok(false);
        }
        self.with_session(|session| {
            Ok(session
                .child
                .try_wait()
                .context("failed to probe Playwright child status")?
                .is_none())
        })
    }

    fn wait_until_ready(&self, timeout: Duration) -> Result<()> {
        match self.wait_for_ui_snapshot_event(timeout, None) {
            Some(Ok(_)) => Ok(()),
            Some(Err(error)) => {
                let stderr_tail = self
                    .stderr_log
                    .blocking_lock()
                    .iter()
                    .rev()
                    .take(40)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>();
                let stderr_block = if stderr_tail.is_empty() {
                    "none".to_string()
                } else {
                    stderr_tail.join("\n")
                };
                bail!(
                    "browser instance {} did not reach semantic readiness within {:?} (last_error={}, stderr_tail=\n{})",
                    self.config.id,
                    timeout,
                    error,
                    stderr_block,
                );
            }
            None => bail!(
                "browser instance {} does not expose a typed ui snapshot readiness event",
                self.config.id
            ),
        }
    }

    fn is_healthy(&self) -> bool {
        self.state == BackendState::Running
    }
}

impl DiagnosticBackend for PlaywrightBrowserBackend {
    fn diagnostic_screen_snapshot(&self) -> Result<String> {
        let payload = self.with_session(|session| {
            session.rpc_call(
                "snapshot",
                json!({
                    "instance_id": self.config.id,
                    "screenshot": self.capture_screenshots,
                }),
            )
        })?;
        let payload: BrowserDiagnosticScreenPayload = decode_rpc_payload(payload, || {
            format!(
                "failed to decode browser diagnostic snapshot payload for instance {}",
                self.config.id
            )
        })?;
        Ok(payload.authoritative_screen)
    }

    fn diagnostic_dom_snapshot(&self) -> Result<String> {
        let payload = self.with_session(|session| {
            session.rpc_call_with_timeout(
                "dom_snapshot",
                json!({ "instance_id": self.config.id }),
                DIAGNOSTIC_DOM_RPC_TIMEOUT_MS.saturating_add(WAIT_RPC_TIMEOUT_MARGIN_MS),
            )
        })?;
        let payload: BrowserDiagnosticScreenPayload = decode_rpc_payload(payload, || {
            format!(
                "failed to decode browser DOM diagnostic payload for instance {}",
                self.config.id
            )
        })?;
        Ok(payload.authoritative_screen)
    }

    fn wait_for_diagnostic_dom_patterns(
        &self,
        patterns: &[String],
        timeout_ms: u64,
    ) -> Option<Result<String>> {
        Some(self.with_session(|session| {
            let payload = session.rpc_call_with_timeout(
                "wait_for_dom_patterns",
                json!({
                    "instance_id": self.config.id,
                    "patterns": patterns,
                    "timeout_ms": timeout_ms,
                }),
                timeout_ms.saturating_add(WAIT_RPC_TIMEOUT_MARGIN_MS),
            )?;
            let payload: BrowserDiagnosticScreenPayload = decode_rpc_payload(payload, || {
                format!(
                    "failed to decode browser diagnostic wait-for-dom payload for instance {}",
                    self.config.id
                )
            })?;
            Ok(payload.authoritative_screen)
        }))
    }

    fn wait_for_diagnostic_target(
        &self,
        selector: &str,
        timeout_ms: u64,
    ) -> Option<Result<String>> {
        Some(self.with_session(|session| {
            let payload = session.rpc_call_with_timeout(
                "wait_for_selector",
                json!({
                    "instance_id": self.config.id,
                    "selector": selector,
                    "timeout_ms": timeout_ms,
                }),
                timeout_ms.saturating_add(WAIT_RPC_TIMEOUT_MARGIN_MS),
            )?;
            let payload: BrowserDiagnosticScreenPayload = decode_rpc_payload(payload, || {
                format!(
                    "failed to decode browser diagnostic wait-for-target payload for instance {}",
                    self.config.id
                )
            })?;
            Ok(payload.authoritative_screen)
        }))
    }

    fn tail_log(&self, lines: usize) -> Result<Vec<String>> {
        let payload = self.with_session(|session| {
            session.rpc_call(
                "tail_log",
                json!({
                    "instance_id": self.config.id,
                    "lines": lines,
                }),
            )
        })?;
        let payload: BrowserTailLogPayload = decode_rpc_payload(payload, || {
            format!(
                "failed to decode browser tail_log payload for instance {}",
                self.config.id
            )
        })?;
        let mut merged = payload.lines;

        let stderr_tail = self
            .stderr_log
            .blocking_lock()
            .iter()
            .rev()
            .take(lines)
            .cloned()
            .collect::<Vec<_>>();
        for line in stderr_tail.into_iter().rev() {
            let noise = line.contains("[driver] request start")
                || line.contains("[driver] request done")
                || line.contains("method=ui_state")
                || line.contains("method=snapshot")
                || line.contains("method=tail_log");
            if !noise {
                merged.push(line);
            }
        }

        if merged.len() > lines {
            merged = merged.split_off(merged.len() - lines);
        }
        Ok(merged)
    }

    fn read_clipboard(&self) -> Result<String> {
        self.with_session(|session| {
            let payload = session.rpc_call(
                "read_clipboard",
                json!({
                    "instance_id": self.config.id,
                }),
            )?;
            let payload: BrowserClipboardPayload = decode_rpc_payload(payload, || {
                format!(
                    "failed to decode browser clipboard payload for instance {}",
                    self.config.id
                )
            })?;
            let text = payload.text.trim_end_matches(['\n', '\r']).to_string();
            if text.trim().is_empty() {
                bail!("clipboard for browser instance {} is empty", self.config.id);
            }
            Ok(text)
        })
    }
}

fn ui_snapshot_event_timeout(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("wait_for_ui_state timed out")
        || message.contains("request:wait_for_ui_state timed out")
        || message.contains("Playwright driver wait_for_ui_state timed out")
}

impl ObservationBackend for PlaywrightBrowserBackend {
    fn ui_snapshot(&self) -> Result<UiSnapshot> {
        let payload = self.with_session(|session| {
            session.rpc_call("ui_state", json!({ "instance_id": self.config.id }))
        })?;
        serde_json::from_value(payload).with_context(|| {
            format!(
                "failed to decode browser UiSnapshot for instance {}",
                self.config.id
            )
        })
    }

    fn wait_for_ui_snapshot_event(
        &self,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> Option<Result<UiSnapshotEvent>> {
        Some((|| {
            let deadline = Instant::now() + timeout;
            let slice = Duration::from_millis(750);

            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let wait_timeout = std::cmp::min(remaining, slice);
                let payload = match self.with_session(|session| {
                    session.rpc_call_with_timeout(
                        "wait_for_ui_state",
                        json!({
                            "instance_id": self.config.id,
                            "timeout_ms": wait_timeout.as_millis(),
                            "after_version": after_version,
                        }),
                        wait_timeout
                            .as_millis()
                            .clamp(1, u128::from(u64::MAX))
                            .try_into()
                            .unwrap_or(u64::MAX)
                            .saturating_add(WAIT_RPC_TIMEOUT_MARGIN_MS),
                    )
                }) {
                    Ok(payload) => payload,
                    Err(error)
                        if ui_snapshot_event_timeout(&error) && Instant::now() < deadline =>
                    {
                        continue;
                    }
                    Err(error) if ui_snapshot_event_timeout(&error) => {
                        let snapshot = self.ui_snapshot()?;
                        return Ok(UiSnapshotEvent {
                            version: snapshot.revision.semantic_seq,
                            snapshot,
                        });
                    }
                    Err(error) => return Err(error),
                };
                let payload: BrowserUiSnapshotEventPayload = decode_rpc_payload(payload, || {
                    format!(
                        "failed to decode browser ui event payload for instance {}",
                        self.config.id
                    )
                })?;
                return Ok(UiSnapshotEvent {
                    snapshot: payload.snapshot,
                    version: payload.version,
                });
            }
        })())
    }
}

impl RawUiBackend for PlaywrightBrowserBackend {
    fn send_keys(&mut self, keys: &str) -> Result<()> {
        self.with_session(|session| {
            session.rpc_call(
                "send_keys",
                json!({
                    "instance_id": self.config.id,
                    "keys": keys,
                }),
            )?;
            Ok(())
        })
    }

    fn send_key(&mut self, key: ToolKey, repeat: u16) -> Result<()> {
        self.with_session(|session| {
            session.rpc_call(
                "send_key",
                json!({
                    "instance_id": self.config.id,
                    "key": tool_key_name(key),
                    "repeat": repeat.max(1),
                }),
            )?;
            Ok(())
        })
    }

    fn click_button(&mut self, label: &str) -> Result<()> {
        self.with_session(|session| {
            session.rpc_call(
                "click_button",
                json!({
                    "instance_id": self.config.id,
                    "label": label,
                }),
            )?;
            Ok(())
        })
    }

    fn activate_control(&mut self, control_id: ControlId) -> Result<()> {
        if matches!(
            control_id,
            ControlId::SettingsAddDeviceButton | ControlId::SettingsRemoveDeviceButton
        ) {
            let snapshot = self.ui_snapshot()?;
            if snapshot.screen == aura_app::ui::contract::ScreenId::Settings {
                let devices_item_id = semantic_settings_section_item_id(SettingsSection::Devices);
                let needs_devices_section = snapshot
                    .selections
                    .iter()
                    .find(|selection| selection.list == ListId::SettingsSections)
                    .map(|selection| selection.item_id.as_str() != devices_item_id)
                    .unwrap_or(true);
                if needs_devices_section {
                    self.activate_list_item(ListId::SettingsSections, devices_item_id)?;
                }
            }
        }
        let selector = control_selector(control_id)?;
        let is_navigation_control = matches!(
            control_id,
            ControlId::NavNeighborhood
                | ControlId::NavChat
                | ControlId::NavContacts
                | ControlId::NavNotifications
                | ControlId::NavSettings,
        );
        if is_navigation_control {
            let target_screen = match control_id {
                ControlId::NavNeighborhood => Some(aura_app::ui::contract::ScreenId::Neighborhood),
                ControlId::NavChat => Some(aura_app::ui::contract::ScreenId::Chat),
                ControlId::NavContacts => Some(aura_app::ui::contract::ScreenId::Contacts),
                ControlId::NavNotifications => {
                    Some(aura_app::ui::contract::ScreenId::Notifications)
                }
                ControlId::NavSettings => Some(aura_app::ui::contract::ScreenId::Settings),
                _ => None,
            };
            if let Some(target_screen) = target_screen {
                let navigate_result = self.submit_semantic_command(SemanticCommandRequest::new(
                    IntentAction::OpenScreen(target_screen),
                ));
                if navigate_result.is_ok() {
                    return Ok(());
                }
            }
        }
        let click_result = self.click_target(&selector);
        if click_result.is_ok() {
            return Ok(());
        }
        if is_navigation_control && control_id.activation_key().is_none() {
            return Err(click_result
                .err()
                .unwrap_or_else(|| anyhow::anyhow!("control click failed")));
        }
        let click_error = click_result
            .err()
            .unwrap_or_else(|| anyhow::anyhow!("control click failed"));
        if let Some(fallback_key) = control_id.activation_key() {
            self.with_session(|session| {
                session.rpc_call(
                    "send_key",
                    json!({
                        "instance_id": self.config.id,
                        "key": fallback_key,
                        "repeat": 1,
                    }),
                )?;
                Ok(())
            })
            .map_err(|send_error| {
                anyhow::anyhow!(
                    "preferred click failed for {control_id:?} via {selector}: {click_error}; \
                     fallback key '{fallback_key}' failed: {send_error}"
                )
            })?;
            return Ok(());
        }
        Err(anyhow::anyhow!(
            "control activation failed for {control_id:?} via {selector}: {click_error}"
        ))
    }

    fn click_target(&mut self, selector: &str) -> Result<()> {
        self.with_session(|session| {
            session.rpc_call(
                "click_button",
                json!({
                    "instance_id": self.config.id,
                    "selector": selector,
                }),
            )?;
            Ok(())
        })
    }

    fn fill_input(&mut self, selector: &str, value: &str) -> Result<()> {
        self.with_session(|session| {
            session.rpc_call(
                "fill_input",
                json!({
                    "instance_id": self.config.id,
                    "selector": selector,
                    "value": value,
                }),
            )?;
            Ok(())
        })
    }

    fn fill_field(&mut self, field_id: FieldId, value: &str) -> Result<()> {
        let selector = field_selector(field_id)?;
        self.fill_input(&selector, value)
    }

    fn activate_list_item(&mut self, list_id: ListId, item_id: &str) -> Result<()> {
        if matches!(list_id, ListId::Navigation) {
            let control_id = navigation_control_id(item_id)?;
            return self.activate_control(control_id);
        }

        let selector = list_item_selector(list_id, item_id);
        self.click_target(&selector)
    }
}

impl SharedSemanticBackend for PlaywrightBrowserBackend {
    fn shared_projection(&self) -> Result<UiSnapshot> {
        self.ui_snapshot()
    }

    fn wait_for_shared_projection_event(
        &self,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> Option<Result<UiSnapshotEvent>> {
        self.wait_for_ui_snapshot_event(timeout, after_version)
    }

    fn submit_semantic_command(
        &mut self,
        request: SemanticCommandRequest,
    ) -> Result<SemanticCommandResponse> {
        PlaywrightBrowserBackend::submit_semantic_command(self, request)
    }
}

impl Drop for PlaywrightBrowserBackend {
    fn drop(&mut self) {
        let _ = self.stop_inner();
    }
}

fn collect_stderr(stderr: ChildStderr, buffer: &Arc<Mutex<Vec<String>>>) {
    let mut reader = BufReader::new(stderr);
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = match reader.read_until(b'\n', &mut line) {
            Ok(read) => read,
            Err(_) => break,
        };
        if read == 0 {
            break;
        }
        let entry = String::from_utf8_lossy(&line).trim_end().to_string();
        if entry.is_empty() {
            continue;
        }
        let mut logs = buffer.blocking_lock();
        logs.push(entry);
        if logs.len() > 2048 {
            let drain_to = logs.len().saturating_sub(1024);
            logs.drain(0..drain_to);
        }
    }
}

fn browser_app_url(env_entries: &[String]) -> String {
    env_value("AURA_HARNESS_BROWSER_APP_URL", env_entries)
        .or_else(|| env_value("AURA_WEB_APP_URL", env_entries))
        .or_else(|| std::env::var("AURA_HARNESS_BROWSER_APP_URL").ok())
        .or_else(|| std::env::var("AURA_WEB_APP_URL").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:4173".to_string())
}

fn parse_bool_setting(key: &str, env_entries: &[String], default: bool) -> Result<bool> {
    let Some(raw) = env_or_process_value(key, env_entries) else {
        return Ok(default);
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("invalid boolean value for {key}: {raw}"),
    }
}

fn env_value(key: &str, env_entries: &[String]) -> Option<String> {
    env_entries.iter().find_map(|item| {
        let (entry_key, entry_value) = item.split_once('=')?;
        if entry_key.trim() == key {
            Some(entry_value.trim().to_string())
        } else {
            None
        }
    })
}

fn env_or_process_value(key: &str, env_entries: &[String]) -> Option<String> {
    env_value(key, env_entries).or_else(|| std::env::var(key).ok())
}

fn parse_u64_setting(
    key: &str,
    env_entries: &[String],
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64> {
    let Some(raw) = env_or_process_value(key, env_entries) else {
        return Ok(default);
    };
    let value = raw
        .trim()
        .parse::<u64>()
        .with_context(|| format!("invalid integer value for {key}: {raw}"))?;
    if !(min..=max).contains(&value) {
        bail!("{key} must be in range [{min}, {max}], got {value}");
    }
    Ok(value)
}

fn parse_u32_setting(
    key: &str,
    env_entries: &[String],
    default: u32,
    min: u32,
    max: u32,
) -> Result<u32> {
    let value = parse_u64_setting(key, env_entries, default as u64, min as u64, max as u64)?;
    u32::try_from(value).with_context(|| format!("value for {key} overflows u32: {value}"))
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

fn default_driver_script_path() -> Result<PathBuf> {
    let root = std::env::current_dir().context("failed to resolve current_dir for harness")?;
    let candidate = root
        .join("crates")
        .join("aura-harness")
        .join("playwright-driver")
        .join("playwright_driver.mjs");
    require_existing_path(&candidate, "playwright driver script")?;
    Ok(candidate)
}

fn control_selector(control_id: ControlId) -> Result<String> {
    control_id
        .web_selector()
        .ok_or_else(|| anyhow!("control {control_id:?} does not have a web selector"))
}

fn field_selector(field_id: FieldId) -> Result<String> {
    field_id
        .web_selector()
        .ok_or_else(|| anyhow!("field {field_id:?} does not have a web selector"))
}

fn navigation_control_id(item_id: &str) -> Result<ControlId> {
    classify_screen_item_id(item_id)
        .filter(|screen| *screen != aura_app::ui::contract::ScreenId::Onboarding)
        .map(nav_control_id_for_screen)
        .ok_or_else(|| anyhow!("item {item_id} not found in list {:?}", ListId::Navigation))
}

fn tool_key_name(key: ToolKey) -> &'static str {
    match key {
        ToolKey::Enter => "enter",
        ToolKey::Esc => "esc",
        ToolKey::Tab => "tab",
        ToolKey::BackTab => "backtab",
        ToolKey::Up => "up",
        ToolKey::Down => "down",
        ToolKey::Right => "right",
        ToolKey::Left => "left",
        ToolKey::Home => "home",
        ToolKey::End => "end",
        ToolKey::PageUp => "pageup",
        ToolKey::PageDown => "pagedown",
        ToolKey::Backspace => "backspace",
        ToolKey::Delete => "delete",
    }
}

fn require_existing_path(path: &Path, label: &str) -> Result<()> {
    if !path.exists() {
        bail!("{label} does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("{label} must be a file: {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        browser_app_url, control_selector, field_selector, navigation_control_id,
        parse_bool_setting, parse_u64_setting, tool_key_name, DEFAULT_PAGE_GOTO_TIMEOUT_MS,
    };
    use crate::tool_api::ToolKey;
    use aura_app::ui::contract::{ControlId, FieldId};

    #[test]
    fn browser_app_url_prefers_instance_env_override() {
        let env = vec![
            "AURA_HARNESS_BROWSER_APP_URL=http://127.0.0.1:5000".to_string(),
            "AURA_WEB_APP_URL=http://127.0.0.1:4173".to_string(),
        ];
        assert_eq!(browser_app_url(&env), "http://127.0.0.1:5000");
    }

    #[test]
    fn parse_bool_setting_supports_common_values() {
        const KEY: &str = "AURA_HARNESS_BROWSER_BOOL_TEST";
        let yes_env = vec![format!("{KEY}=YES")];
        let no_env = vec![format!("{KEY}=off")];
        let yes = parse_bool_setting(KEY, &yes_env, false);
        assert!(matches!(yes, Ok(true)));
        let no = parse_bool_setting(KEY, &no_env, true);
        assert!(matches!(no, Ok(false)));
        let default_value = parse_bool_setting(KEY, &[], false);
        assert!(matches!(default_value, Ok(false)));
    }

    #[test]
    fn parse_u64_setting_uses_default_when_missing() {
        const KEY: &str = "AURA_HARNESS_BROWSER_U64_TEST";
        let value = parse_u64_setting(KEY, &[], DEFAULT_PAGE_GOTO_TIMEOUT_MS, 1, 600_000);
        assert!(matches!(value, Ok(DEFAULT_PAGE_GOTO_TIMEOUT_MS)));
    }

    #[test]
    fn parse_u64_setting_rejects_out_of_range() {
        const KEY: &str = "AURA_HARNESS_BROWSER_U64_TEST";
        let env = vec![format!("{KEY}=9999999")];
        let error = parse_u64_setting(KEY, &env, DEFAULT_PAGE_GOTO_TIMEOUT_MS, 1, 600_000);
        match error {
            Ok(value) => panic!("expected out-of-range error, got value {value}"),
            Err(err) => {
                assert!(err
                    .to_string()
                    .contains("AURA_HARNESS_BROWSER_U64_TEST must be in range"));
            }
        }
    }

    #[test]
    fn browser_driver_maps_shared_controls_to_selectors() {
        assert_eq!(
            control_selector(ControlId::NavChat).unwrap_or_else(|error| panic!("{error}")),
            "#aura-nav-chat"
        );
        assert_eq!(
            control_selector(ControlId::ModalConfirmButton)
                .unwrap_or_else(|error| panic!("{error}")),
            "#aura-modal-confirm-button"
        );
    }

    #[test]
    fn browser_driver_maps_shared_fields_to_selectors() {
        assert_eq!(
            field_selector(FieldId::ChatInput).unwrap_or_else(|error| panic!("{error}")),
            "#aura-field-chat-input"
        );
    }

    #[test]
    fn browser_driver_maps_navigation_items_to_controls() {
        assert_eq!(
            navigation_control_id("chat").unwrap_or_else(|error| panic!("{error}")),
            ControlId::NavChat
        );
        assert_eq!(
            navigation_control_id("settings").unwrap_or_else(|error| panic!("{error}")),
            ControlId::NavSettings
        );
        assert!(navigation_control_id("unknown").is_err());
    }

    #[test]
    fn browser_driver_maps_dismiss_key_name() {
        assert_eq!(tool_key_name(ToolKey::Esc), "esc");
    }

    #[test]
    fn playwright_shared_intent_methods_use_semantic_bridge() {
        let source = include_str!("playwright_browser.rs");
        assert!(source.contains("fn submit_semantic_command("));
        for intent in [
            "IntentAction::CreateAccount",
            "IntentAction::CreateHome",
            "IntentAction::CreateChannel",
            "IntentAction::CreateContactInvitation",
            "IntentAction::AcceptContactInvitation",
            "IntentAction::InviteActorToChannel",
            "IntentAction::AcceptPendingChannelInvitation",
            "IntentAction::JoinChannel",
            "IntentAction::SendChatMessage",
        ] {
            assert!(
                source.contains(intent),
                "browser shared semantic backend should route {intent} through the semantic bridge"
            );
        }
    }

    #[test]
    fn browser_frontend_conformance_keeps_settings_control_selector_aligned() {
        assert_eq!(
            control_selector(ControlId::NavSettings).unwrap_or_else(|error| panic!("{error}")),
            "#aura-nav-settings"
        );
        assert_eq!(
            control_selector(
                navigation_control_id("settings").unwrap_or_else(|error| panic!("{error}"))
            )
            .unwrap_or_else(|error| panic!("{error}")),
            "#aura-nav-settings"
        );
    }

    #[test]
    fn playwright_shared_semantic_methods_do_not_regress_to_raw_ui_driving() {
        fn method_block<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
            let (_, tail) = source
                .split_once(start)
                .unwrap_or_else(|| panic!("missing method start {start}"));
            tail.split_once(end)
                .map(|(body, _)| body)
                .unwrap_or_else(|| panic!("missing method end {end}"))
        }

        let source = include_str!("playwright_browser.rs");
        let raw_ui_calls = [
            "activate_control(",
            "click_button(",
            "click_target(",
            "fill_input(",
            "fill_field(",
            "activate_list_item(",
            "send_key(",
            "send_keys(",
        ];
        let body = method_block(
            source,
            "impl SharedSemanticBackend for PlaywrightBrowserBackend {",
            "impl Drop for PlaywrightBrowserBackend {",
        );
        assert!(
            body.contains("fn submit_semantic_command("),
            "shared semantic backend should expose one generic semantic submit path"
        );
        for raw_call in raw_ui_calls {
            assert!(
                !body.contains(raw_call),
                "shared semantic backend should not call raw UI helper {raw_call}"
            );
        }
    }

    #[test]
    fn playwright_shared_semantic_bridge_replaces_shortcut_bypasses() {
        let source = include_str!("playwright_browser.rs");
        assert!(source.contains("fn submit_semantic_command("));
        assert!(
            source.contains(
                "session.rpc_call_with_timeout(\n                \"submit_semantic_command\","
            ),
            "browser backend should give semantic submissions an explicit long-lived RPC timeout because preserved-profile restart is part of the owned path"
        );
        assert!(
            source.contains(
                "session.rpc_call_with_timeout(\n                \"stage_runtime_identity\","
            ),
            "browser backend should give runtime-identity staging the explicit longer RPC timeout budget required by generation-changing restart flows"
        );
        assert!(!source.contains("session.rpc_call(\n                \"create_account\","));
        assert!(!source.contains("session.rpc_call(\n                \"create_home\","));
        assert!(
            !source.contains("session.rpc_call(\n                \"create_contact_invitation\",")
        );
    }

    #[test]
    fn playwright_create_account_bootstrap_is_owned_by_web_bridge() {
        let source = include_str!("playwright_browser.rs");
        assert!(
            !source.contains("session.rpc_call(\n                    \"restart_page_session\","),
            "browser backend should not restart browser sessions after semantic create-account submission"
        );
        assert!(
            !source.contains("session.rpc_call(\n                    \"reload_page\","),
            "browser backend should not regress create-account bootstrap to a driver-owned soft page reload"
        );
    }

    #[test]
    fn playwright_semantic_bridge_failure_and_projection_contracts_are_explicit() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .unwrap_or_else(|| panic!("workspace root"));
        let bridge_source =
            std::fs::read_to_string(workspace_root.join("crates/aura-web/src/harness_bridge.rs"))
                .unwrap_or_else(|error| panic!("failed to read browser harness bridge: {error}"));
        let install_source =
            std::fs::read_to_string(workspace_root.join("crates/aura-web/src/harness/install.rs"))
                .unwrap_or_else(|error| {
                    panic!("failed to read browser harness installer: {error}")
                });
        let contract_source = std::fs::read_to_string(
            workspace_root.join("crates/aura-web/src/harness/driver_contract.rs"),
        )
        .unwrap_or_else(|error| panic!("failed to read browser harness driver contract: {error}"));
        let queue_source = std::fs::read_to_string(
            workspace_root.join("crates/aura-web/src/harness/page_owned_queue.rs"),
        )
        .unwrap_or_else(|error| panic!("failed to read browser harness queue module: {error}"));
        let commands_source =
            std::fs::read_to_string(workspace_root.join("crates/aura-web/src/harness/commands.rs"))
                .unwrap_or_else(|error| panic!("failed to read browser harness commands: {error}"));
        let app_source =
            std::fs::read_to_string(workspace_root.join("crates/aura-web/src/shell/app.rs"))
                .unwrap_or_else(|error| panic!("failed to read web shell app: {error}"));
        let driver_source = std::fs::read_to_string(
            workspace_root.join("crates/aura-harness/playwright-driver/src/playwright_driver.ts"),
        )
        .unwrap_or_else(|error| panic!("failed to read Playwright driver: {error}"));
        let backend_source = include_str!("playwright_browser.rs");

        assert!(
            commands_source.contains("BrowserSemanticBridgeRequest")
                && commands_source.contains("invalid semantic command request")
                && install_source.contains("BrowserSemanticBridgeRequest::from_json(&request_json)?"),
            "browser bridge should reject malformed semantic requests with typed context through the shared typed bridge surface"
        );
        assert!(
            commands_source.contains("BootstrapHandoff::PendingAccountBootstrap {")
                || app_source.contains("BootstrapHandoff::PendingAccountBootstrap {"),
            "browser semantic bridge should stage create-account bootstrap through the owned bootstrap handoff"
        );
        assert!(
            install_source.contains("&JsValue::from_str(\"stage_runtime_identity\")"),
            "browser harness bridge should expose an explicit runtime identity staging entrypoint"
        );
        assert!(
            install_source.contains("page_owned_queue::install(window)")
                && !install_source.contains("Function::new_no_args(")
                && !install_source.contains("include_str!(\"page_owned_mutation_queues.js\")"),
            "browser harness install should stay a thin typed installer over the canonical Rust-owned page queue module"
        );
        assert!(
            !bridge_source.contains("&JsValue::from_str(\"submit_bootstrap_handoff\")"),
            "browser harness bridge must not expose a generic bootstrap trigger"
        );
        assert!(
            install_source.contains("classify_screen_item_id(&screen_name)")
                && install_source
                    .contains("classify_semantic_settings_section_item_id(&section_name)"),
            "browser bridge should classify screen and settings item ids through the shared ui contract helpers"
        );
        assert!(
            !commands_source.contains("unsupported semantic browser command"),
            "browser bridge should cover the typed semantic intent surface directly instead of keeping a generic unsupported-intent fallback"
        );
        assert!(
            driver_source.contains("markObservationMutation(session, \"submit_semantic_command\")"),
            "browser driver should advance the semantic observation baseline after semantic submission"
        );
        assert!(
            contract_source.contains("pub(crate) const RUNTIME_STAGE_ENQUEUE_KEY")
                && queue_source.contains("use crate::harness::driver_contract::{")
                && queue_source.contains("crate::harness_bridge::stage_runtime_identity("),
            "browser harness bridge should reuse the production-owned driver contract module and invoke the explicit runtime identity staging entrypoint inside the page"
        );
        assert!(
            driver_source.contains("from \"./driver_contract.js\";")
                && driver_source.contains("window[runtimeStageEnqueueKey](payload)")
                && driver_source.contains("buildRuntimeStageQueuePayloadJson("),
            "browser driver should submit runtime-identity staging through the dedicated driver contract module instead of re-spelling queue globals or payload builders inline"
        );
        assert!(
            !driver_source.contains("\"__AURA_DRIVER_RUNTIME_STAGE_ENQUEUE__\"")
                && !driver_source.contains("\"__AURA_DRIVER_RUNTIME_STAGE_RESULTS__\"")
                && !driver_source.contains("\"__AURA_DRIVER_RUNTIME_STAGE_DEBUG__\""),
            "browser driver should not re-spell shared runtime-stage driver globals inline once the dedicated contract module owns them"
        );
        assert!(
            !driver_source.contains("await stageRuntimeIdentity(serializedIdentity);"),
            "browser driver should not hold a direct page-evaluate await open across a generation-changing runtime staging handoff"
        );
        assert!(
            !driver_source.contains("window.localStorage?.setItem"),
            "browser driver must not own browser runtime-identity storage layout"
        );
        assert!(
            contract_source.contains("pub(crate) const SEMANTIC_ENQUEUE_KEY")
                && contract_source.contains("pub(crate) struct SemanticQueuePayload")
                && queue_source.contains("BrowserSemanticBridgeRequest::from_json")
                && queue_source.contains("semantic_submit_surface_state().status() != PublicationStatus::Ready"),
            "browser harness bridge should reuse the production-owned driver contract module and keep semantic replay ownership inside the generation-aware page queue"
        );
        assert!(
            driver_source.contains("window[semanticEnqueueKey](payload)")
                && driver_source.contains("buildSemanticQueuePayloadJson(")
                && !driver_source.contains("request_json: requestJson"),
            "browser driver should submit semantic commands through the dedicated driver contract module instead of open-coded queue globals or payload JSON"
        );
        assert!(
            !driver_source.contains("\"__AURA_DRIVER_SEMANTIC_ENQUEUE__\"")
                && !driver_source.contains("\"__AURA_DRIVER_SEMANTIC_RESULTS__\"")
                && !driver_source.contains("\"__AURA_DRIVER_SEMANTIC_DEBUG__\""),
            "browser driver should not re-spell shared semantic driver globals inline once the dedicated contract module owns them"
        );
        assert!(
            backend_source.contains("failed to decode browser semantic command response"),
            "browser backend should preserve semantic bridge decode failures diagnostically"
        );
        let submit_start = backend_source
            .find("fn submit_semantic_command(")
            .unwrap_or_else(|| panic!("missing submit_semantic_command"));
        let submit_end = backend_source[submit_start..]
            .find("impl InstanceBackend for PlaywrightBrowserBackend {")
            .map(|offset| submit_start + offset)
            .unwrap_or(backend_source.len());
        let submit_block = &backend_source[submit_start..submit_end];
        assert!(
            !submit_block
                .contains("session.rpc_call(\n                        \"navigate_screen\",")
                && !submit_block.contains(
                    "session.rpc_call(\n                        \"open_settings_section\","
                ),
            "browser shared semantic submission should use submit_semantic_command instead of bypassing the page-owned semantic queue for navigation intents"
        );
    }

    #[test]
    fn playwright_backend_ready_wait_uses_typed_ui_snapshot_event() {
        let source = include_str!("playwright_browser.rs");
        let (_, tail) = source
            .split_once("fn wait_until_ready(&self, timeout: Duration) -> Result<()> {")
            .unwrap_or_else(|| panic!("missing wait_until_ready"));
        let body = tail
            .split_once("\n    }\n\n    fn is_healthy")
            .map(|(body, _)| body)
            .unwrap_or_else(|| panic!("missing wait_until_ready terminator"));
        assert!(
            body.contains("self.wait_for_ui_snapshot_event(timeout, None)"),
            "browser backend readiness should use the typed ui snapshot event contract"
        );
        assert!(
            !body.contains("blocking_sleep(Duration::from_millis(100))"),
            "browser backend readiness should not poll with fixed sleeps"
        );
    }

    #[test]
    fn playwright_backend_authority_id_reads_from_authoritative_snapshot() {
        let source = include_str!("playwright_browser.rs");
        let (_, tail) = source
            .split_once("fn authority_id(&mut self) -> Result<Option<String>> {")
            .unwrap_or_else(|| panic!("missing authority_id"));
        let body = tail
            .split_once("\n    }\n\n    fn health_check")
            .map(|(body, _)| body)
            .unwrap_or_else(|| panic!("missing authority_id terminator"));
        assert!(
            body.contains("self.ui_snapshot()?"),
            "browser authority id should read from the authoritative shared projection"
        );
        assert!(
            body.contains(".selected_item_id(ListId::Authorities)"),
            "browser authority id should mirror the TUI selection-based authority lookup"
        );
        assert!(
            !body.contains("\"get_authority_id\""),
            "browser authority id should not depend on a separate driver RPC"
        );
    }

    #[test]
    fn playwright_driver_startup_and_navigation_avoid_timer_loops() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .unwrap_or_else(|| panic!("workspace root"));
        let driver_source = std::fs::read_to_string(
            workspace_root.join("crates/aura-harness/playwright-driver/src/playwright_driver.ts"),
        )
        .unwrap_or_else(|error| panic!("failed to read Playwright driver: {error}"));

        assert!(
            driver_source.contains("resetPersistentProfileDir(dataDir);"),
            "driver startup should reset the persistent profile before launch instead of doing in-page storage scrubs"
        );
        assert!(
            !driver_source.contains("storage_reset start"),
            "driver startup should not keep the in-page storage reset loop"
        );
        assert!(
            driver_source.contains("window[wakePendingNavKey]?.();"),
            "navigation should wake an explicit pending-nav runner through the shared driver contract"
        );
        assert!(
            !driver_source.contains("window.setTimeout(drain, 16)"),
            "driver should not keep a perpetual 16ms pending-nav timer loop"
        );
    }
}
