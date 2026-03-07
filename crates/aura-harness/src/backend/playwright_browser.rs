use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use aura_app::ui::contract::{list_item_selector, ControlId, FieldId, ListId, UiSnapshot};
use nix::poll::{poll, PollFd, PollFlags};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::backend::InstanceBackend;
use crate::config::InstanceConfig;
use crate::tool_api::ToolKey;

const DEFAULT_PAGE_GOTO_TIMEOUT_MS: u64 = 90_000;
const DEFAULT_HARNESS_READY_TIMEOUT_MS: u64 = 90_000;
const DEFAULT_RPC_TIMEOUT_MS: u64 = 15_000;
const WAIT_RPC_TIMEOUT_MARGIN_MS: u64 = 5_000;
const DEFAULT_START_MAX_ATTEMPTS: u32 = 3;
const DEFAULT_START_RETRY_BACKOFF_MS: u64 = 1_200;
const MAX_START_ATTEMPTS: u32 = 10;
const MAX_TIMEOUT_MS: u64 = 600_000;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BackendState {
    Stopped,
    Running,
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
            bail!("Playwright driver {method} failed: {error}");
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
                "data_dir": absolutize_path(self.config.data_dir.clone()),
                "artifact_dir": absolutize_path(self.artifact_dir.clone()),
                "headless": self.headless,
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

    fn snapshot(&self) -> Result<String> {
        let payload = self.with_session(|session| {
            session.rpc_call(
                "snapshot",
                json!({
                    "instance_id": self.config.id,
                    "screenshot": self.capture_screenshots,
                }),
            )
        })?;
        let screen = payload
            .get("authoritative_screen")
            .and_then(Value::as_str)
            .or_else(|| payload.get("screen").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();
        Ok(screen)
    }

    fn snapshot_dom(&self) -> Result<String> {
        let payload = self.with_session(|session| {
            session.rpc_call("dom_snapshot", json!({ "instance_id": self.config.id }))
        })?;
        let screen = payload
            .get("authoritative_screen")
            .and_then(Value::as_str)
            .or_else(|| payload.get("screen").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();
        Ok(screen)
    }

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

    fn wait_for_dom_patterns(
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
            let screen = payload
                .get("authoritative_screen")
                .and_then(Value::as_str)
                .or_else(|| payload.get("screen").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string();
            Ok(screen)
        }))
    }

    fn wait_for_target(&self, selector: &str, timeout_ms: u64) -> Option<Result<String>> {
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
            let screen = payload
                .get("authoritative_screen")
                .and_then(Value::as_str)
                .or_else(|| payload.get("screen").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string();
            Ok(screen)
        }))
    }

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
        let selector = control_id
            .web_selector()
            .ok_or_else(|| anyhow!("control {control_id:?} does not have a web selector"))?;
        self.click_target(&selector)
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
        let selector = field_id
            .web_selector()
            .ok_or_else(|| anyhow!("field {field_id:?} does not have a web selector"))?;
        self.fill_input(&selector, value)
    }

    fn activate_list_item(&mut self, list_id: ListId, item_id: &str) -> Result<()> {
        if matches!(list_id, ListId::Navigation) {
            let control_id = match item_id {
                "neighborhood" => ControlId::NavNeighborhood,
                "chat" => ControlId::NavChat,
                "contacts" => ControlId::NavContacts,
                "notifications" => ControlId::NavNotifications,
                "settings" => ControlId::NavSettings,
                _ => anyhow::bail!("item {item_id} not found in list {list_id:?}"),
            };
            return self.activate_control(control_id);
        }

        let selector = list_item_selector(list_id, item_id);
        self.click_target(&selector)
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
        let mut merged = payload
            .get("lines")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect::<Vec<_>>();

        let stderr_tail = self
            .stderr_log
            .blocking_lock()
            .iter()
            .rev()
            .take(lines)
            .cloned()
            .collect::<Vec<_>>();
        for line in stderr_tail.into_iter().rev() {
            merged.push(line);
        }

        if merged.len() > lines {
            merged = merged.split_off(merged.len() - lines);
        }
        Ok(merged)
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

    fn read_clipboard(&mut self) -> Result<String> {
        self.with_session(|session| {
            let payload = session.rpc_call(
                "read_clipboard",
                json!({
                    "instance_id": self.config.id,
                }),
            )?;
            let text = payload
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim_end_matches(['\n', '\r'])
                .to_string();
            if text.trim().is_empty() {
                bail!("clipboard for browser instance {} is empty", self.config.id);
            }
            Ok(text)
        })
    }

    fn authority_id(&mut self) -> Result<Option<String>> {
        self.with_session(|session| {
            let payload = session.rpc_call(
                "get_authority_id",
                json!({
                    "instance_id": self.config.id,
                }),
            )?;
            let authority_id = payload
                .get("authority_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            Ok(authority_id)
        })
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

    fn is_healthy(&self) -> bool {
        self.state == BackendState::Running
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
        browser_app_url, parse_bool_setting, parse_u64_setting, DEFAULT_PAGE_GOTO_TIMEOUT_MS,
    };

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
}
