use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Condvar;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use aura_app::scenario_contract::SettingsSection;
use aura_app::scenario_contract::{
    IntentAction, SemanticCommandRequest, SemanticCommandResponse, SemanticCommandValue,
    SemanticSubmissionHandle, SubmissionState,
};
use aura_app::ui::contract::{
    semantic_settings_section_item_id, ControlId, FieldId, HarnessUiCommand,
    HarnessUiCommandReceipt, ListId, ModalId, ScreenId, UiReadiness, UiSnapshot,
};
use nix::errno::Errno;
use nix::sys::signal;
use nix::unistd::Pid;
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::backend::{
    ChannelBinding, ContactInvitationCode, DiagnosticBackend, InstanceBackend, ObservationBackend,
    RawUiBackend, SharedSemanticBackend, SubmittedAction, UiOperationHandle, UiSnapshotEvent,
};
use crate::config::InstanceConfig;
use crate::screen_normalization::{authoritative_screen, has_nav_header};
use crate::timeouts::blocking_sleep;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BackendState {
    Stopped,
    Running,
}

fn into_ui_operation_handle(
    handle: aura_app::ui_contract::HarnessUiOperationHandle,
) -> UiOperationHandle {
    UiOperationHandle::new(handle.operation_id().clone(), handle.instance_id().clone())
}

fn require_ui_operation_handle(
    receipt: HarnessUiCommandReceipt,
    operation_name: &str,
) -> Result<UiOperationHandle> {
    match receipt {
        HarnessUiCommandReceipt::AcceptedWithOperation { operation, .. } => {
            Ok(into_ui_operation_handle(operation))
        }
        HarnessUiCommandReceipt::Accepted { .. } => {
            anyhow::bail!("{operation_name} accepted without a canonical ui operation handle")
        }
        HarnessUiCommandReceipt::Rejected { reason } => {
            anyhow::bail!("{operation_name} rejected: {reason}")
        }
    }
}

fn require_channel_binding_submission(
    receipt: HarnessUiCommandReceipt,
    operation_name: &str,
) -> Result<SubmittedAction<ChannelBinding>> {
    match receipt {
        HarnessUiCommandReceipt::AcceptedWithOperation {
            operation,
            value:
                Some(SemanticCommandValue::AuthoritativeChannelBinding {
                    channel_id,
                    context_id,
                }),
        } => Ok(SubmittedAction::with_ui_operation(
            ChannelBinding {
                channel_id,
                context_id,
            },
            into_ui_operation_handle(operation),
        )),
        HarnessUiCommandReceipt::AcceptedWithOperation {
            operation,
            value: None,
        } => {
            let handle = into_ui_operation_handle(operation);
            anyhow::bail!(
                "{operation_name} did not return an authoritative channel binding payload for {}:{}",
                handle.id().0,
                handle.instance_id().0
            );
        }
        HarnessUiCommandReceipt::AcceptedWithOperation {
            operation: _,
            value: Some(SemanticCommandValue::ChannelSelection { channel_id }),
        } => anyhow::bail!(
            "{operation_name} returned only a weak selected-channel payload without authoritative context: {channel_id}"
        ),
        HarnessUiCommandReceipt::AcceptedWithOperation {
            operation: _,
            value: Some(SemanticCommandValue::None),
        } => anyhow::bail!(
            "{operation_name} returned an explicit empty semantic payload"
        ),
        HarnessUiCommandReceipt::AcceptedWithOperation {
            operation: _,
            value: Some(SemanticCommandValue::ContactInvitationCode { .. }),
        } => anyhow::bail!(
            "{operation_name} returned an unexpected contact invitation payload"
        ),
        HarnessUiCommandReceipt::Accepted {
            value: Some(SemanticCommandValue::AuthoritativeChannelBinding { .. }),
        } => anyhow::bail!(
            "{operation_name} accepted without exact operation handle"
        ),
        HarnessUiCommandReceipt::Accepted {
            value: Some(SemanticCommandValue::ChannelSelection { .. }),
        } => anyhow::bail!(
            "{operation_name} returned a weak selected-channel payload without a canonical operation handle"
        ),
        HarnessUiCommandReceipt::Accepted { value: None } => anyhow::bail!(
            "{operation_name} accepted without exact operation handle or authoritative channel binding payload"
        ),
        HarnessUiCommandReceipt::Accepted {
            value: Some(SemanticCommandValue::None),
        } => anyhow::bail!(
            "{operation_name} returned an explicit empty semantic payload without a canonical operation handle"
        ),
        HarnessUiCommandReceipt::Accepted {
            value: Some(SemanticCommandValue::ContactInvitationCode { .. }),
        } => anyhow::bail!(
            "{operation_name} returned an unexpected contact invitation payload without a canonical operation handle"
        ),
        HarnessUiCommandReceipt::Rejected { reason } => {
            anyhow::bail!("{operation_name} rejected: {reason}")
        }
    }
}

fn require_contact_invitation_submission(
    receipt: HarnessUiCommandReceipt,
    operation_name: &str,
) -> Result<(UiOperationHandle, Option<ContactInvitationCode>)> {
    match receipt {
        HarnessUiCommandReceipt::AcceptedWithOperation {
            operation,
            value: Some(SemanticCommandValue::ContactInvitationCode { code }),
        } => Ok((
            into_ui_operation_handle(operation),
            Some(ContactInvitationCode { code }),
        )),
        HarnessUiCommandReceipt::AcceptedWithOperation {
            operation,
            value: None,
        }
        | HarnessUiCommandReceipt::AcceptedWithOperation {
            operation,
            value: Some(SemanticCommandValue::None),
        } => Ok((into_ui_operation_handle(operation), None)),
        HarnessUiCommandReceipt::AcceptedWithOperation {
            operation: _,
            value: Some(SemanticCommandValue::ChannelSelection { .. }),
        } => anyhow::bail!(
            "{operation_name} returned an unexpected channel selection payload"
        ),
        HarnessUiCommandReceipt::AcceptedWithOperation {
            operation: _,
            value: Some(SemanticCommandValue::AuthoritativeChannelBinding { .. }),
        } => anyhow::bail!(
            "{operation_name} returned an unexpected channel binding payload"
        ),
        HarnessUiCommandReceipt::Accepted {
            value: Some(SemanticCommandValue::ContactInvitationCode { .. }),
        }
        | HarnessUiCommandReceipt::Accepted { value: None }
        | HarnessUiCommandReceipt::Accepted {
            value: Some(SemanticCommandValue::None),
        } => anyhow::bail!(
            "{operation_name} accepted without exact operation handle"
        ),
        HarnessUiCommandReceipt::Accepted {
            value: Some(SemanticCommandValue::ChannelSelection { .. }),
        } => anyhow::bail!(
            "{operation_name} returned an unexpected channel selection payload without a canonical operation handle"
        ),
        HarnessUiCommandReceipt::Accepted {
            value: Some(SemanticCommandValue::AuthoritativeChannelBinding { .. }),
        } => anyhow::bail!(
            "{operation_name} returned an unexpected channel binding payload without a canonical operation handle"
        ),
        HarnessUiCommandReceipt::Rejected { reason } => {
            anyhow::bail!("{operation_name} rejected: {reason}")
        }
    }
}

struct RunningSession {
    child: Mutex<Box<dyn Child + Send>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<vt100::Parser>>,
    parse_generation: Arc<AtomicU64>,
    reader_status: Arc<std::sync::Mutex<Option<String>>>,
    reader_thread: Option<thread::JoinHandle<()>>,
    ui_snapshot_feed: Arc<UiSnapshotFeed>,
    ui_snapshot_listener_status: Arc<std::sync::Mutex<Option<String>>>,
    ui_snapshot_thread: Option<thread::JoinHandle<()>>,
    ui_snapshot_stop: Arc<AtomicU64>,
    transient_root: PathBuf,
    ui_snapshot_socket_path: PathBuf,
    ui_snapshot_file_path: PathBuf,
    command_socket_path: PathBuf,
    clipboard_file_path: PathBuf,
    child_pid_path: PathBuf,
}

#[derive(Default)]
struct UiSnapshotFeedState {
    latest: Option<UiSnapshot>,
    version: u64,
}

#[allow(clippy::disallowed_types)]
#[derive(Default)]
struct UiSnapshotFeed {
    state: std::sync::Mutex<UiSnapshotFeedState>,
    ready: Condvar,
}

pub struct LocalPtyBackend {
    config: InstanceConfig,
    state: BackendState,
    session: Option<RunningSession>,
    session_generation: u64,
    pty_rows: u16,
    pty_cols: u16,
    last_authoritative_screen: Arc<Mutex<Option<String>>>,
}

impl LocalPtyBackend {
    fn reader_thread_alive(session: &RunningSession) -> bool {
        session
            .reader_thread
            .as_ref()
            .map_or(true, |thread| !thread.is_finished())
    }

    fn ensure_session_alive(&self, session: &RunningSession, context: &str) -> Result<()> {
        if Self::reader_thread_alive(session) && Self::child_process_alive(session) {
            return Ok(());
        }
        let screen = Self::read_screen(&session.parser);
        let log_tail = self.tail_log(40).unwrap_or_default().join("\n");
        let reader_alive = Self::reader_thread_alive(session);
        let child_alive = Self::child_process_alive(session);
        let child_status = Self::child_process_status(session);
        let ui_snapshot_thread_alive = session
            .ui_snapshot_thread
            .as_ref()
            .map_or(true, |thread| !thread.is_finished());
        let reader_status = session
            .reader_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let ui_snapshot_listener_status = session
            .ui_snapshot_listener_status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        anyhow::bail!(
            "local PTY instance {} exited before {context}; last_screen={:?} reader_alive={} reader_status={reader_status:?} child_alive={} child_status={child_status:?} ui_snapshot_thread_alive={} ui_snapshot_listener_status={ui_snapshot_listener_status:?} command_socket_exists={} ui_snapshot_socket_exists={} ui_snapshot_file_exists={} child_pid_file_exists={} log_tail={}",
            self.config.id,
            self.select_authoritative_screen(screen),
            reader_alive,
            child_alive,
            ui_snapshot_thread_alive,
            session.command_socket_path.exists(),
            session.ui_snapshot_socket_path.exists(),
            session.ui_snapshot_file_path.exists(),
            session.child_pid_path.exists(),
            log_tail,
        );
    }

    fn child_process_alive(session: &RunningSession) -> bool {
        let Ok(mut child) = session.child.try_lock() else {
            return true;
        };
        match child.try_wait() {
            Ok(Some(_)) => return false,
            Ok(None) => {}
            Err(_) => {}
        }
        let Some(pid) = child.process_id() else {
            return true;
        };
        match signal::kill(Pid::from_raw(pid as i32), None) {
            Ok(()) => true,
            Err(Errno::EPERM) => true,
            Err(_) => false,
        }
    }

    fn child_process_status(session: &RunningSession) -> Option<String> {
        let Ok(mut child) = session.child.try_lock() else {
            return Some("child mutex busy".to_string());
        };
        match child.try_wait() {
            Ok(Some(status)) => Some(status.to_string()),
            Ok(None) => {
                let Some(pid) = child.process_id() else {
                    return Some("running (no child pid available)".to_string());
                };
                match signal::kill(Pid::from_raw(pid as i32), None) {
                    Ok(()) | Err(Errno::EPERM) => Some(format!("running (pid={pid})")),
                    Err(error) => Some(format!("unreachable pid={pid}: {error}")),
                }
            }
            Err(error) => Some(format!("try_wait failed: {error}")),
        }
    }

    fn is_cargo_program(program: &str) -> bool {
        std::path::Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name == "cargo")
            .unwrap_or(false)
    }

    fn transient_root(&self) -> PathBuf {
        Self::env_value("AURA_HARNESS_INSTANCE_TRANSIENT_ROOT", &self.config.env)
            .map(PathBuf::from)
            .map(Self::absolutize_path)
            .unwrap_or_else(|| {
                Self::absolutize_path(self.config.data_dir.join(".harness-transient"))
            })
    }

    fn ui_state_socket_path(&self, session_generation: u64) -> PathBuf {
        self.transient_root()
            .join(format!("ui-state-gen{session_generation}.sock"))
    }

    fn ui_state_file_path(&self, session_generation: u64) -> PathBuf {
        self.transient_root()
            .join(format!("ui-state-gen{session_generation}.json"))
    }

    fn command_socket_path(&self, session_generation: u64) -> PathBuf {
        self.transient_root()
            .join(format!("command-gen{session_generation}.sock"))
    }

    fn clipboard_file_path(&self) -> PathBuf {
        self.transient_root().join("clipboard.txt")
    }

    fn child_pid_path(&self, session_generation: u64) -> PathBuf {
        self.transient_root()
            .join(format!("child-gen{session_generation}.pid"))
    }

    pub fn new(config: InstanceConfig, pty_rows: Option<u16>, pty_cols: Option<u16>) -> Self {
        Self {
            config,
            state: BackendState::Stopped,
            session: None,
            session_generation: 0,
            pty_rows: pty_rows.unwrap_or(40),
            pty_cols: pty_cols.unwrap_or(120),
            last_authoritative_screen: Arc::new(Mutex::new(None)),
        }
    }

    fn spawn_ui_snapshot_listener(
        socket_path: &PathBuf,
        feed: &Arc<UiSnapshotFeed>,
        stop_flag: &Arc<AtomicU64>,
        listener_status: &Arc<std::sync::Mutex<Option<String>>>,
    ) -> Result<thread::JoinHandle<()>> {
        let _ = fs::remove_file(socket_path);
        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create TUI UI snapshot socket directory {}",
                    parent.display()
                )
            })?;
        }
        let listener = UnixListener::bind(socket_path).with_context(|| {
            format!(
                "failed to bind TUI UI snapshot socket {}",
                socket_path.display()
            )
        })?;
        let socket_path = socket_path.clone();
        let feed = Arc::clone(feed);
        let stop_flag = Arc::clone(stop_flag);
        let listener_status = Arc::clone(listener_status);
        Ok(thread::spawn(move || {
            let set_status = |status: String| {
                *listener_status
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(status);
            };
            for stream in listener.incoming() {
                if stop_flag.load(Ordering::Acquire) > 0 {
                    set_status("stopped by harness request".to_string());
                    break;
                }
                let Ok(mut stream) = stream else {
                    set_status("listener accept failed".to_string());
                    continue;
                };
                let mut payload = String::new();
                if let Err(error) = stream.read_to_string(&mut payload) {
                    set_status(format!("listener failed reading snapshot payload: {error}"));
                    continue;
                }
                if payload.trim() == "__AURA_UI_STATE_SHUTDOWN__" {
                    set_status("received harness shutdown sentinel".to_string());
                    break;
                }
                let Ok(snapshot) = serde_json::from_str::<UiSnapshot>(&payload) else {
                    set_status("listener failed decoding snapshot payload".to_string());
                    continue;
                };
                let mut guard = feed
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.version = guard.version.saturating_add(1);
                guard.latest = Some(snapshot);
                feed.ready.notify_all();
            }
            if listener_status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_none()
            {
                *listener_status
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                    Some("listener exited without explicit status".to_string());
            }
            let _ = fs::remove_file(socket_path);
        }))
    }

    fn default_command(&self) -> (String, Vec<String>) {
        let mut aura_args = vec![
            "tui".to_string(),
            "--data-dir".to_string(),
            Self::absolutize_path(self.config.data_dir.clone())
                .to_string_lossy()
                .to_string(),
            "--bind-address".to_string(),
            self.config.bind_address.clone(),
        ];
        if let Some(device_id) = self.config.device_id.as_deref() {
            aura_args.push("--device-id".to_string());
            aura_args.push(device_id.to_string());
        }
        if self.config.demo_mode {
            aura_args.push("--demo".to_string());
            return self.default_command_with_args(aura_args);
        }
        self.default_command_with_args(aura_args)
    }

    fn default_command_with_args(&self, aura_args: Vec<String>) -> (String, Vec<String>) {
        if let Some(explicit) = std::env::var("AURA_HARNESS_AURA_BIN")
            .ok()
            .filter(|value| !value.trim().is_empty())
        {
            let explicit_path = Self::absolutize_path(PathBuf::from(explicit));
            if explicit_path.exists() {
                return (explicit_path.to_string_lossy().to_string(), aura_args);
            }
        }

        if let Ok(cargo) = which::which("cargo") {
            let mut args = vec![
                "run".to_string(),
                "-q".to_string(),
                "-p".to_string(),
                "aura-terminal".to_string(),
                "--bin".to_string(),
                "aura".to_string(),
                "--".to_string(),
            ];
            args.extend(aura_args);
            return (cargo.to_string_lossy().to_string(), args);
        }

        if let Some(candidate) = std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join("target/debug/aura"))
            .filter(|candidate| candidate.exists())
        {
            return (candidate.to_string_lossy().to_string(), aura_args);
        }

        ("aura".to_string(), aura_args)
    }

    fn command_spec(&self) -> (String, Vec<String>) {
        match &self.config.command {
            Some(command) => (command.clone(), self.config.args.clone()),
            None => self.default_command(),
        }
    }

    fn parser_size(&self) -> (u16, u16) {
        (self.pty_rows, self.pty_cols)
    }

    fn read_screen(parser: &Arc<Mutex<vt100::Parser>>) -> String {
        let parser = parser.blocking_lock();
        parser.screen().contents()
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

    fn absolutize_path(path: PathBuf) -> PathBuf {
        if path.is_absolute() {
            return path;
        }
        if let Ok(cwd) = std::env::current_dir() {
            return cwd.join(path);
        }
        path
    }

    fn select_authoritative_screen(&self, current_screen: String) -> String {
        let current_authoritative = authoritative_screen(&current_screen);
        let mut cached = self.last_authoritative_screen.blocking_lock();

        if has_nav_header(&current_authoritative) {
            *cached = Some(current_authoritative.clone());
            return current_authoritative;
        }

        cached.clone().unwrap_or(current_authoritative)
    }

    fn requires_tui_readiness(&self) -> bool {
        self.config.command.is_none()
    }

    fn type_text(&mut self, value: &str, inter_key_delay_ms: u64) -> Result<()> {
        for ch in value.chars() {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            self.send_keys(s)?;
            blocking_sleep(Duration::from_millis(inter_key_delay_ms));
        }
        blocking_sleep(Duration::from_millis(50));
        Ok(())
    }

    fn send_harness_command_receipt(
        &self,
        command: &HarnessUiCommand,
    ) -> Result<HarnessUiCommandReceipt> {
        let socket_path = self
            .session
            .as_ref()
            .map(|session| session.command_socket_path.clone())
            .unwrap_or_else(|| {
                Self::absolutize_path(self.command_socket_path(self.session_generation))
            });
        let payload = serde_json::to_vec(command).context("failed to encode harness UI command")?;
        let deadline = Instant::now() + Duration::from_secs(10);

        loop {
            match UnixStream::connect(&socket_path) {
                Ok(mut stream) => {
                    let command_result: Result<HarnessUiCommandReceipt> = (|| {
                        stream
                            .write_all(&payload)
                            .context("failed to write harness UI command")?;
                        stream
                            .flush()
                            .context("failed to flush harness UI command")?;
                        stream
                            .shutdown(Shutdown::Write)
                            .context("failed to half-close harness UI command socket")?;
                        let mut receipt = Vec::new();
                        stream
                            .read_to_end(&mut receipt)
                            .context("failed to read harness UI command receipt")?;
                        if receipt.is_empty() {
                            return Err(std::io::Error::new(
                                ErrorKind::UnexpectedEof,
                                "empty harness UI command receipt",
                            )
                            .into());
                        }
                        let receipt = serde_json::from_slice::<HarnessUiCommandReceipt>(&receipt)
                            .map_err(|error| {
                            if error.classify() == serde_json::error::Category::Eof {
                                std::io::Error::new(
                                    ErrorKind::UnexpectedEof,
                                    format!("truncated harness UI command receipt: {error}"),
                                )
                                .into()
                            } else {
                                anyhow::Error::new(error)
                                    .context("failed to decode harness UI command receipt")
                            }
                        })?;
                        Ok(receipt)
                    })();

                    match command_result {
                        Ok(receipt) => return Ok(receipt),
                        Err(error)
                            if error
                                .downcast_ref::<std::io::Error>()
                                .is_some_and(|io_error| {
                                    matches!(
                                        io_error.kind(),
                                        ErrorKind::BrokenPipe
                                            | ErrorKind::ConnectionAborted
                                            | ErrorKind::ConnectionReset
                                            | ErrorKind::UnexpectedEof
                                    )
                                }) =>
                        {
                            if Instant::now() >= deadline {
                                return Err(anyhow::anyhow!(
                                    "timed out waiting for TUI harness command socket {} to become ready (last error: {})",
                                    socket_path.display(),
                                    error
                                ));
                            }
                            blocking_sleep(Duration::from_millis(50));
                            continue;
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::NotFound
                            | ErrorKind::ConnectionRefused
                            | ErrorKind::ConnectionReset
                    ) =>
                {
                    if Instant::now() >= deadline {
                        if let Some(session) = self.session.as_ref() {
                            self.ensure_session_alive(
                                session,
                                "the harness command plane became ready",
                            )?;
                        }
                        return Err(anyhow::anyhow!(
                            "timed out waiting for TUI harness command socket {} to become ready (last error: {})",
                            socket_path.display(),
                            error
                        ));
                    }
                    blocking_sleep(Duration::from_millis(50));
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to connect TUI harness command socket {}",
                            socket_path.display()
                        )
                    });
                }
            }
        }
    }

    fn send_harness_command(
        &mut self,
        command: &HarnessUiCommand,
    ) -> Result<HarnessUiCommandReceipt> {
        match self.send_harness_command_receipt(command)? {
            HarnessUiCommandReceipt::Rejected { reason } => {
                anyhow::bail!("TUI harness command rejected: {reason}")
            }
            receipt => Ok(receipt),
        }
    }
}

impl InstanceBackend for LocalPtyBackend {
    fn id(&self) -> &str {
        &self.config.id
    }

    fn backend_kind(&self) -> &'static str {
        "local_pty"
    }

    fn start(&mut self) -> Result<()> {
        if self.state == BackendState::Running {
            return Ok(());
        }
        *self.last_authoritative_screen.blocking_lock() = None;
        self.session_generation = self.session_generation.saturating_add(1);
        let session_generation = self.session_generation;

        let (rows, cols) = self.parser_size();
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .with_context(|| format!("failed to allocate PTY for {}", self.config.id))?;

        let (program, args) = self.command_spec();
        let use_workspace_cwd = Self::is_cargo_program(&program);
        let mut command = CommandBuilder::new(program);
        for arg in args {
            command.arg(arg);
        }
        if use_workspace_cwd {
            if let Ok(cwd) = std::env::current_dir() {
                command.cwd(cwd);
            }
        } else {
            command.cwd(&self.config.data_dir);
        }
        command.env("TERM", "xterm-256color");
        command.env("LANG", "C.UTF-8");

        // Harness runs default to clipboard isolation so local user clipboard is not mutated
        // by agent-driven TUI interactions. Per-instance env can explicitly override this.
        if Self::env_value("AURA_CLIPBOARD_MODE", &self.config.env).is_none() {
            command.env("AURA_CLIPBOARD_MODE", "file_only");
        }
        let transient_root = self.transient_root();
        fs::create_dir_all(&transient_root).with_context(|| {
            format!(
                "failed to create instance transient root {}",
                transient_root.display()
            )
        })?;
        if Self::env_value("AURA_CLIPBOARD_FILE", &self.config.env).is_none() {
            let clipboard_file = Self::absolutize_path(self.clipboard_file_path());
            command.env(
                "AURA_CLIPBOARD_FILE",
                clipboard_file.to_string_lossy().to_string(),
            );
        }
        if Self::env_value("AURA_TUI_UI_STATE_SOCKET", &self.config.env).is_none() {
            let ui_state_socket =
                Self::absolutize_path(self.ui_state_socket_path(session_generation));
            command.env(
                "AURA_TUI_UI_STATE_SOCKET",
                ui_state_socket.to_string_lossy().to_string(),
            );
        }
        if Self::env_value("AURA_TUI_UI_STATE_FILE", &self.config.env).is_none() {
            let ui_state_file = self.ui_state_file_path(session_generation);
            command.env(
                "AURA_TUI_UI_STATE_FILE",
                ui_state_file.to_string_lossy().to_string(),
            );
        }
        if Self::env_value("AURA_TUI_COMMAND_SOCKET", &self.config.env).is_none() {
            let command_socket =
                Self::absolutize_path(self.command_socket_path(session_generation));
            command.env(
                "AURA_TUI_COMMAND_SOCKET",
                command_socket.to_string_lossy().to_string(),
            );
        }

        for item in &self.config.env {
            if let Some((key, value)) = item.split_once('=') {
                if key.trim() == "AURA_CLIPBOARD_FILE" {
                    let resolved = Self::absolutize_path(PathBuf::from(value.trim()));
                    command.env(
                        "AURA_CLIPBOARD_FILE",
                        resolved.to_string_lossy().to_string(),
                    );
                    continue;
                }
                command.env(key.trim(), value.trim());
            }
        }

        if let Some(lan) = &self.config.lan_discovery {
            command.env(
                "AURA_HARNESS_LAN_DISCOVERY_ENABLED",
                if lan.enabled { "true" } else { "false" },
            );
            command.env("AURA_HARNESS_LAN_DISCOVERY_BIND_ADDR", &lan.bind_addr);
            command.env(
                "AURA_HARNESS_LAN_DISCOVERY_BROADCAST_ADDR",
                &lan.broadcast_addr,
            );
            command.env("AURA_HARNESS_LAN_DISCOVERY_PORT", lan.port.to_string());
        }

        fs::create_dir_all(&self.config.data_dir).with_context(|| {
            format!(
                "failed to create instance data_dir {}",
                self.config.data_dir.display()
            )
        })?;
        let ui_snapshot_feed = Arc::new(UiSnapshotFeed::default());
        let ui_snapshot_stop = Arc::new(AtomicU64::new(0));
        let ui_snapshot_socket_path =
            Self::absolutize_path(self.ui_state_socket_path(session_generation));
        let ui_snapshot_file_path =
            Self::absolutize_path(self.ui_state_file_path(session_generation));
        let command_socket_path =
            Self::absolutize_path(self.command_socket_path(session_generation));
        let clipboard_file_path = Self::absolutize_path(self.clipboard_file_path());
        let child_pid_path = Self::absolutize_path(self.child_pid_path(session_generation));
        let ui_snapshot_listener_status = Arc::new(std::sync::Mutex::new(None));
        let ui_snapshot_thread = Self::spawn_ui_snapshot_listener(
            &ui_snapshot_socket_path,
            &ui_snapshot_feed,
            &ui_snapshot_stop,
            &ui_snapshot_listener_status,
        )?;
        let child = match pair.slave.spawn_command(command) {
            Ok(child) => child,
            Err(error) => {
                ui_snapshot_stop.store(1, Ordering::Release);
                if let Ok(mut stream) = UnixStream::connect(&ui_snapshot_socket_path) {
                    let _ = stream.write_all(b"__AURA_UI_STATE_SHUTDOWN__");
                }
                let _ = ui_snapshot_thread.join();
                return Err(error)
                    .with_context(|| format!("failed to spawn process for {}", self.config.id));
            }
        };
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to acquire PTY writer")?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let parse_generation = Arc::new(AtomicU64::new(0));
        let parser_for_thread = Arc::clone(&parser);
        let generation_for_thread = Arc::clone(&parse_generation);
        let log_path_for_thread = self.config.log_path.clone();
        let reader_status = Arc::new(std::sync::Mutex::new(None));
        let reader_status_for_thread = Arc::clone(&reader_status);
        let reader_thread = thread::spawn(move || {
            let set_status = |status: String| {
                *reader_status_for_thread
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(status);
            };
            let mut log_file = log_path_for_thread.and_then(|path| {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .ok()
            });
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        set_status("PTY reader reached EOF".to_string());
                        break;
                    }
                    Ok(read) => {
                        if let Some(file) = log_file.as_mut() {
                            let _ = file.write_all(&buffer[..read]);
                            let _ = file.flush();
                        }
                        parser_for_thread.blocking_lock().process(&buffer[..read]);
                        generation_for_thread.fetch_add(1, Ordering::Release);
                    }
                    Err(error) => {
                        set_status(format!("PTY reader error: {error}"));
                        break;
                    }
                }
            }
        });

        if let Some(pid) = child.process_id() {
            fs::write(&child_pid_path, pid.to_string()).with_context(|| {
                format!(
                    "failed to persist local PTY child pid at {}",
                    child_pid_path.display()
                )
            })?;
        }

        self.session = Some(RunningSession {
            child: Mutex::new(child),
            writer: Arc::new(Mutex::new(writer)),
            parser,
            parse_generation,
            reader_status,
            reader_thread: Some(reader_thread),
            ui_snapshot_feed,
            ui_snapshot_listener_status,
            ui_snapshot_thread: Some(ui_snapshot_thread),
            ui_snapshot_stop,
            transient_root,
            ui_snapshot_socket_path,
            ui_snapshot_file_path,
            command_socket_path,
            clipboard_file_path,
            child_pid_path,
        });
        self.state = BackendState::Running;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if self.state == BackendState::Stopped {
            return Ok(());
        }

        if let Some(mut session) = self.session.take() {
            session.ui_snapshot_stop.store(1, Ordering::Release);
            if let Ok(mut stream) = UnixStream::connect(&session.ui_snapshot_socket_path) {
                let _ = stream.write_all(b"__AURA_UI_STATE_SHUTDOWN__");
            }
            {
                let mut child = session
                    .child
                    .try_lock()
                    .unwrap_or_else(|_| panic!("pty child mutex already locked during stop"));
                let _ = child.kill();
                let _ = child.wait();
            }
            drop(session.writer);
            if let Some(handle) = session.reader_thread.take() {
                let _ = handle.join();
            }
            if let Some(handle) = session.ui_snapshot_thread.take() {
                let _ = handle.join();
            }
            let _ = fs::remove_file(&session.child_pid_path);
            let _ = fs::remove_file(&session.command_socket_path);
            let _ = fs::remove_file(&session.ui_snapshot_socket_path);
            let _ = fs::remove_file(&session.clipboard_file_path);
            let _ = fs::remove_dir_all(&session.transient_root);
        }

        self.state = BackendState::Stopped;
        Ok(())
    }

    fn inject_message(&mut self, message: &str) -> Result<()> {
        let sanitized = message
            .chars()
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if sanitized.is_empty() {
            return Ok(());
        }

        // Mirror browser-submitted message text by switching to Chat, entering insert
        // mode, and submitting the message payload.
        self.send_keys(&format!("\u{1b}2i{sanitized}\r"))
    }

    fn authority_id(&mut self) -> Result<Option<String>> {
        let snapshot = self.ui_snapshot()?;
        Ok(snapshot
            .selected_item_id(ListId::Authorities)
            .map(str::to_string))
    }

    fn health_check(&self) -> Result<bool> {
        let running = self.state == BackendState::Running && self.session.is_some();
        if !running {
            return Ok(false);
        }
        let Some(session) = self.session.as_ref() else {
            return Ok(false);
        };
        Ok(Self::reader_thread_alive(session) && Self::child_process_alive(session))
    }

    fn wait_until_ready(&self, timeout: Duration) -> Result<()> {
        if !self.requires_tui_readiness() {
            let deadline = Instant::now() + timeout;
            loop {
                if self.health_check()? {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    anyhow::bail!(
                        "local PTY instance {} did not reach health readiness within {:?}",
                        self.config.id,
                        timeout
                    );
                }
                blocking_sleep(Duration::from_millis(100));
            }
        }

        let deadline = Instant::now() + timeout;
        let mut last_ping_error: Option<String> = None;
        loop {
            if let Ok(snapshot) = self.ui_snapshot() {
                if snapshot.readiness == UiReadiness::Ready {
                    match self.send_harness_command_receipt(&HarnessUiCommand::Ping) {
                        Ok(HarnessUiCommandReceipt::Accepted { .. })
                        | Ok(HarnessUiCommandReceipt::AcceptedWithOperation { .. }) => {
                            return Ok(());
                        }
                        Ok(HarnessUiCommandReceipt::Rejected { reason }) => {
                            last_ping_error =
                                Some(format!("command plane rejected readiness ping: {reason}"));
                        }
                        Err(error) => {
                            last_ping_error = Some(error.to_string());
                        }
                    }
                }
            }
            if self.session.as_ref().is_some_and(|session| {
                session
                    .reader_thread
                    .as_ref()
                    .is_some_and(|thread| thread.is_finished())
                    || !Self::child_process_alive(session)
            }) {
                let screen = self.diagnostic_screen_snapshot().unwrap_or_default();
                let log_tail = self.tail_log(40).unwrap_or_default().join("\n");
                anyhow::bail!(
                    "local PTY instance {} exited before publishing an authoritative UI snapshot; screen={:?} log_tail={}",
                    self.config.id,
                    screen,
                    log_tail
                );
            }
            if Instant::now() >= deadline {
                let readiness = self.ui_snapshot().ok().map(|snapshot| snapshot.readiness);
                anyhow::bail!(
                    "local PTY instance {} did not reach semantic readiness within {:?}; readiness={readiness:?}; command_plane={last_ping_error:?}",
                    self.config.id,
                    timeout,
                );
            }
            blocking_sleep(Duration::from_millis(100));
        }
    }

    fn is_healthy(&self) -> bool {
        self.state == BackendState::Running
    }
}

impl DiagnosticBackend for LocalPtyBackend {
    fn diagnostic_screen_snapshot(&self) -> Result<String> {
        let session = self
            .session
            .as_ref()
            .with_context(|| format!("instance {} is not running", self.config.id))?;
        const SETTLE_DELAY_MS: u64 = 25;
        const MAX_SETTLE_ATTEMPTS: u8 = 8;
        const HEADER_RECOVERY_DELAY_MS: u64 = 20;
        const HEADER_RECOVERY_ATTEMPTS: u8 = 30;

        let mut last_generation = session.parse_generation.load(Ordering::Acquire);
        let mut last_screen = Self::read_screen(&session.parser);

        for _ in 0..MAX_SETTLE_ATTEMPTS {
            blocking_sleep(Duration::from_millis(SETTLE_DELAY_MS));
            let current_generation = session.parse_generation.load(Ordering::Acquire);
            let current_screen = Self::read_screen(&session.parser);
            if current_generation == last_generation && current_screen == last_screen {
                return Ok(self.select_authoritative_screen(current_screen));
            }
            last_generation = current_generation;
            last_screen = current_screen;
        }

        let mut recovered_screen = last_screen;
        if !has_nav_header(&recovered_screen) {
            for _ in 0..HEADER_RECOVERY_ATTEMPTS {
                blocking_sleep(Duration::from_millis(HEADER_RECOVERY_DELAY_MS));
                recovered_screen = Self::read_screen(&session.parser);
                if has_nav_header(&recovered_screen) {
                    break;
                }
            }
        }

        Ok(self.select_authoritative_screen(recovered_screen))
    }

    fn diagnostic_dom_snapshot(&self) -> Result<String> {
        self.diagnostic_screen_snapshot()
    }

    fn wait_for_diagnostic_dom_patterns(
        &self,
        _patterns: &[String],
        _timeout_ms: u64,
    ) -> Option<Result<String>> {
        None
    }

    fn wait_for_diagnostic_target(
        &self,
        _selector: &str,
        _timeout_ms: u64,
    ) -> Option<Result<String>> {
        None
    }

    fn tail_log(&self, lines: usize) -> Result<Vec<String>> {
        let Some(path) = &self.config.log_path else {
            return Ok(Vec::new());
        };

        let mut candidates = Vec::with_capacity(2);
        candidates.push(path.clone());
        candidates.push(PathBuf::from(format!("{}.dat", path.display())));

        let mut body: Option<String> = None;
        for candidate in candidates {
            let bytes = match fs::read(&candidate) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            body = Some(String::from_utf8_lossy(&bytes).into_owned());
            break;
        }
        let Some(body) = body else {
            return Ok(Vec::new());
        };

        let mut result: Vec<String> = body.lines().map(ToOwned::to_owned).collect();
        if result.len() > lines {
            result = result.split_off(result.len() - lines);
        }
        Ok(result)
    }

    fn read_clipboard(&self) -> Result<String> {
        let path = Self::env_value("AURA_CLIPBOARD_FILE", &self.config.env)
            .map(PathBuf::from)
            .map(Self::absolutize_path)
            .unwrap_or_else(|| Self::absolutize_path(self.clipboard_file_path()));
        let mut text = fs::read_to_string(&path).map_err(|error| {
            anyhow::anyhow!(
                "failed reading clipboard file {} for instance {}: {error}",
                path.display(),
                self.config.id
            )
        })?;
        while matches!(text.chars().last(), Some('\n' | '\r')) {
            text.pop();
        }
        if text.is_empty() {
            anyhow::bail!("clipboard for instance {} is empty", self.config.id);
        }
        Ok(text)
    }
}

impl ObservationBackend for LocalPtyBackend {
    fn ui_snapshot(&self) -> Result<UiSnapshot> {
        let session = self
            .session
            .as_ref()
            .with_context(|| format!("instance {} is not running", self.config.id))?;
        self.ensure_session_alive(session, "reading authoritative UI snapshot")?;
        let guard = session
            .ui_snapshot_feed
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(snapshot) = guard.latest.clone() {
            return Ok(snapshot);
        }
        drop(guard);
        let guard = session
            .ui_snapshot_feed
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.latest.clone().with_context(|| {
            format!(
                "TUI UI snapshot unavailable for instance {} via {}",
                self.config.id,
                session.ui_snapshot_socket_path.display()
            )
        })
    }

    fn wait_for_ui_snapshot_event(
        &self,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> Option<Result<UiSnapshotEvent>> {
        let session = self.session.as_ref()?;
        let deadline = Instant::now() + timeout;
        let mut guard = session
            .ui_snapshot_feed
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if let Some(snapshot) = guard.latest.clone() {
                let version = guard.version;
                if after_version.map_or(true, |required| version > required) {
                    return Some(Ok(UiSnapshotEvent { snapshot, version }));
                }
            }
            drop(guard);
            if let Err(error) =
                self.ensure_session_alive(session, "publishing a newer UI snapshot event")
            {
                return Some(Err(error));
            }
            guard = session
                .ui_snapshot_feed
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let now = Instant::now();
            if now >= deadline {
                return Some(Err(anyhow::anyhow!(
                    "timed out waiting for TUI UI snapshot event on instance {} after_version={:?}",
                    self.config.id,
                    after_version
                )));
            }
            let poll_timeout = deadline
                .saturating_duration_since(now)
                .min(Duration::from_millis(250));
            let timeout_result = session
                .ui_snapshot_feed
                .ready
                .wait_timeout(guard, poll_timeout)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard = timeout_result.0;
        }
    }
}

impl Drop for LocalPtyBackend {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl RawUiBackend for LocalPtyBackend {
    fn send_keys(&mut self, keys: &str) -> Result<()> {
        let session = self
            .session
            .as_ref()
            .with_context(|| format!("instance {} is not running", self.config.id))?;
        if !keys.as_bytes().contains(&0x1b) {
            let mut writer = session.writer.blocking_lock();
            writer
                .write_all(keys.as_bytes())
                .with_context(|| format!("failed writing keys for instance {}", self.config.id))?;
            writer.flush().context("failed flushing PTY writer")?;
            return Ok(());
        }

        let bytes = keys.as_bytes();
        let mut index = 0usize;
        while index < bytes.len() {
            {
                let mut writer = session.writer.blocking_lock();
                writer
                    .write_all(&bytes[index..index + 1])
                    .with_context(|| {
                        format!("failed writing keys for instance {}", self.config.id)
                    })?;
                writer.flush().context("failed flushing PTY writer")?;
            }

            if bytes[index] == 0x1b
                && bytes
                    .get(index + 1)
                    .map_or(true, |next| *next != b'[' && *next != b'O')
            {
                // Prevent accidental Alt-key combos when callers intend standalone Esc.
                blocking_sleep(Duration::from_millis(40));
            }
            index += 1;
        }
        Ok(())
    }

    fn click_button(&mut self, label: &str) -> Result<()> {
        let _ = label;
        anyhow::bail!("local_pty does not support label-driven button clicks")
    }

    fn activate_control(&mut self, control_id: ControlId) -> Result<()> {
        let target_screen = match control_id {
            ControlId::NavNeighborhood => Some(ScreenId::Neighborhood),
            ControlId::NavChat => Some(ScreenId::Chat),
            ControlId::NavContacts => Some(ScreenId::Contacts),
            ControlId::NavNotifications => Some(ScreenId::Notifications),
            ControlId::NavSettings => Some(ScreenId::Settings),
            _ => None,
        };
        match control_id {
            ControlId::SettingsAddDeviceButton
            | ControlId::SettingsImportDeviceCodeButton
            | ControlId::SettingsRemoveDeviceButton => {
                let snapshot = self.ui_snapshot()?;
                if snapshot.screen == aura_app::ui::contract::ScreenId::Settings {
                    let devices_item_id =
                        semantic_settings_section_item_id(SettingsSection::Devices);
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
            _ => {}
        }
        if control_id == ControlId::ModalConfirmButton {
            let snapshot = self.ui_snapshot()?;
            if matches!(
                snapshot.open_modal,
                Some(ModalId::CreateInvitation | ModalId::AddDevice)
            ) {
                return self.send_keys("\r");
            }
        }
        if let Some(target_screen) = target_screen {
            self.send_harness_command(&HarnessUiCommand::NavigateScreen {
                screen: target_screen,
            })?;
            return Ok(());
        }
        if matches!(
            control_id,
            ControlId::SettingsAddDeviceButton
                | ControlId::SettingsImportDeviceCodeButton
                | ControlId::SettingsRemoveDeviceButton
                | ControlId::ContactsInviteToChannelButton
        ) {
            self.send_harness_command(&HarnessUiCommand::ActivateControl { control_id })?;
            return Ok(());
        }
        let sequence = control_id.activation_key().ok_or_else(|| {
            anyhow::anyhow!("control {control_id:?} does not have a PTY activation mapping")
        })?;
        self.send_keys(sequence)
    }

    fn click_target(&mut self, selector: &str) -> Result<()> {
        let _ = selector;
        anyhow::bail!("local_pty does not support selector-driven clicks")
    }

    fn fill_input(&mut self, selector: &str, value: &str) -> Result<()> {
        let _ = selector;
        self.type_text(value, 8)
    }

    fn fill_field(&mut self, field_id: FieldId, value: &str) -> Result<()> {
        if matches!(field_id, FieldId::DeviceImportCode) {
            let snapshot = self.ui_snapshot()?;
            if snapshot.screen == ScreenId::Onboarding {
                self.send_keys("\t")?;
                blocking_sleep(Duration::from_millis(50));
            }
        }
        match field_id {
            FieldId::ChatInput => {
                let snapshot = self.ui_snapshot()?;
                if snapshot.screen == ScreenId::Chat
                    && !matches!(
                        snapshot.focused_control,
                        Some(ControlId::Field(FieldId::ChatInput))
                    )
                {
                    self.send_keys("\x1b")?;
                    self.send_keys("i")?;
                    blocking_sleep(Duration::from_millis(200));
                }
                self.type_text(value, 12)
            }
            FieldId::InvitationCode | FieldId::DeviceImportCode => self.type_text(value, 3),
            _ => self.type_text(value, 8),
        }
    }

    fn activate_list_item(&mut self, list_id: ListId, item_id: &str) -> Result<()> {
        if matches!(
            list_id,
            ListId::Navigation | ListId::SettingsSections | ListId::Channels | ListId::Contacts
        ) {
            self.send_harness_command(&HarnessUiCommand::ActivateListItem {
                list_id,
                item_id: item_id.to_string(),
            })?;
            return Ok(());
        }
        let snapshot = self.ui_snapshot()?;
        let list = snapshot
            .lists
            .iter()
            .find(|candidate| candidate.id == list_id)
            .ok_or_else(|| {
                anyhow::anyhow!("list {list_id:?} is not visible in the current TUI snapshot")
            })?;
        let target_index = list
            .items
            .iter()
            .position(|item| item.id == item_id)
            .ok_or_else(|| anyhow::anyhow!("item {item_id} not found in list {list_id:?}"))?;
        let current_index = snapshot
            .selected_item_id(list_id)
            .and_then(|selected_id| list.items.iter().position(|item| item.id == selected_id))
            .unwrap_or(0);
        eprintln!(
            "[local_pty activate_list_item] instance={} list={:?} item_id={} current_index={} target_index={} focused_control={:?}",
            self.config.id,
            list_id,
            item_id,
            current_index,
            target_index,
            snapshot.focused_control
        );
        if matches!(list_id, ListId::InvitationTypes) {
            if current_index == target_index {
                return Ok(());
            }
            match snapshot.focused_control {
                Some(ControlId::Field(FieldId::InvitationType)) => {}
                Some(ControlId::Field(FieldId::InvitationMessage)) => self.send_keys("\u{1b}[A")?,
                Some(ControlId::Field(FieldId::InvitationTtl)) => self.send_keys("\u{1b}[B")?,
                Some(other) => anyhow::bail!(
                    "invitation type selector is visible but focus is on incompatible control {other:?}"
                ),
                None => anyhow::bail!(
                    "invitation type selector is visible but the TUI snapshot has no focused control"
                ),
            }

            let len = list.items.len();
            if len == 0 {
                return Ok(());
            }
            let forward_steps = (target_index + len - current_index) % len;
            let backward_steps = (current_index + len - target_index) % len;
            if forward_steps <= backward_steps {
                for _ in 0..forward_steps {
                    self.send_keys("\u{1b}[C")?;
                }
            } else {
                for _ in 0..backward_steps {
                    self.send_keys("\u{1b}[D")?;
                }
            }
            return Ok(());
        }
        let delta = target_index as isize - current_index as isize;
        let sequence = if matches!(list_id, ListId::Homes) {
            if delta < 0 {
                "\u{1b}[A"
            } else {
                "\u{1b}[B"
            }
        } else if delta < 0 {
            "k"
        } else {
            "j"
        };
        for _ in 0..delta.unsigned_abs() {
            eprintln!(
                "[local_pty activate_list_item send] instance={} list={:?} item_id={} sequence={}",
                self.config.id, list_id, item_id, sequence
            );
            self.send_keys(sequence)?;
            blocking_sleep(Duration::from_millis(60));
        }
        let selection_deadline = Instant::now() + Duration::from_millis(1500);
        loop {
            let current_snapshot = self.ui_snapshot()?;
            let selected_item = current_snapshot.selected_item_id(list_id);
            if selected_item == Some(item_id) {
                return Ok(());
            }
            if Instant::now() >= selection_deadline {
                anyhow::bail!(
                    "failed to converge selection for item {item_id} in list {list_id:?}; final selection was {}",
                    selected_item.unwrap_or("<none>")
                );
            }
            blocking_sleep(Duration::from_millis(80));
        }
    }
}

impl LocalPtyBackend {
    fn dismiss_contact_invitation_code_modal(&mut self, timeout: Duration) -> Result<()> {
        const POLL_INTERVAL: Duration = Duration::from_millis(50);
        let modal_visible = crate::timeouts::blocking_wait_until(timeout, POLL_INTERVAL, || {
            let snapshot = self.ui_snapshot().ok()?;
            (snapshot.open_modal == Some(ModalId::InvitationCode)).then_some(())
        })
        .is_some();
        if !modal_visible {
            eprintln!(
                "[local_pty contact_invite] instance={} modal_not_visible_within_timeout",
                self.config.id
            );
            return Ok(());
        }

        eprintln!(
            "[local_pty contact_invite] instance={} dismiss_invitation_modal",
            self.config.id
        );
        self.send_harness_command(&HarnessUiCommand::DismissTransient)?;
        crate::timeouts::blocking_wait_until(timeout, POLL_INTERVAL, || {
            let snapshot = self.ui_snapshot().ok()?;
            (snapshot.open_modal != Some(ModalId::InvitationCode)).then_some(())
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "submit_create_contact_invitation did not dismiss the invitation code modal"
            )
        })?;
        eprintln!(
            "[local_pty contact_invite] instance={} invitation_modal_closed",
            self.config.id
        );
        Ok(())
    }

    fn submit_start_device_enrollment(
        &mut self,
        device_name: &str,
        invitee_authority_id: &str,
    ) -> Result<SubmittedAction<()>> {
        let handle = require_ui_operation_handle(
            self.send_harness_command(&HarnessUiCommand::StartDeviceEnrollment {
                device_name: device_name.to_string(),
                invitee_authority_id: invitee_authority_id.to_string(),
            })?,
            "start_device_enrollment",
        )?;
        Ok(SubmittedAction::with_ui_operation((), handle))
    }

    fn submit_import_device_enrollment_code(&mut self, code: &str) -> Result<SubmittedAction<()>> {
        let handle = require_ui_operation_handle(
            self.send_harness_command(&HarnessUiCommand::ImportDeviceEnrollmentCode {
                code: code.to_string(),
            })?,
            "import_device_enrollment_code",
        )?;
        Ok(SubmittedAction::with_ui_operation((), handle))
    }
}

impl SharedSemanticBackend for LocalPtyBackend {
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
        match request.intent {
            IntentAction::OpenScreen(screen) => {
                self.send_harness_command(&HarnessUiCommand::NavigateScreen { screen })?;
                Ok(SemanticCommandResponse::accepted_without_value())
            }
            IntentAction::OpenSettingsSection(section) => {
                self.send_harness_command(&HarnessUiCommand::OpenSettingsSection { section })?;
                Ok(SemanticCommandResponse::accepted_without_value())
            }
            IntentAction::CreateAccount { account_name } => {
                let handle = require_ui_operation_handle(
                    self.send_harness_command_receipt(&HarnessUiCommand::CreateAccount {
                        account_name,
                    })?,
                    "create_account",
                )?;
                Ok(SemanticCommandResponse {
                    submission: SubmissionState::Accepted,
                    handle: SemanticSubmissionHandle {
                        ui_operation: Some(handle),
                    },
                    value: SemanticCommandValue::None,
                })
            }
            IntentAction::CreateHome { home_name } => {
                let handle = require_ui_operation_handle(
                    self.send_harness_command_receipt(&HarnessUiCommand::CreateHome { home_name })?,
                    "create_home",
                )?;
                Ok(SemanticCommandResponse {
                    submission: SubmissionState::Accepted,
                    handle: SemanticSubmissionHandle {
                        ui_operation: Some(handle),
                    },
                    value: SemanticCommandValue::None,
                })
            }
            IntentAction::StartDeviceEnrollment {
                device_name,
                invitee_authority_id,
                ..
            } => {
                let submitted =
                    self.submit_start_device_enrollment(&device_name, &invitee_authority_id)?;
                Ok(SemanticCommandResponse {
                    submission: submitted.submission,
                    handle: submitted.handle,
                    value: SemanticCommandValue::None,
                })
            }
            IntentAction::ImportDeviceEnrollmentCode { code } => {
                let submitted = self.submit_import_device_enrollment_code(&code)?;
                Ok(SemanticCommandResponse {
                    submission: submitted.submission,
                    handle: submitted.handle,
                    value: SemanticCommandValue::None,
                })
            }
            IntentAction::RemoveSelectedDevice { device_id } => {
                self.send_harness_command(&HarnessUiCommand::RemoveSelectedDevice { device_id })?;
                Ok(SemanticCommandResponse::accepted_without_value())
            }
            IntentAction::SwitchAuthority { authority_id } => {
                self.send_harness_command(&HarnessUiCommand::SwitchAuthority { authority_id })?;
                Ok(SemanticCommandResponse::accepted_without_value())
            }
            IntentAction::CreateContactInvitation {
                receiver_authority_id,
                ..
            } => {
                let submitted = self.submit_create_contact_invitation(&receiver_authority_id)?;
                Ok(SemanticCommandResponse {
                    submission: submitted.submission,
                    handle: submitted.handle,
                    value: SemanticCommandValue::ContactInvitationCode {
                        code: submitted.value.code,
                    },
                })
            }
            IntentAction::AcceptContactInvitation { code } => {
                let submitted = self.submit_accept_contact_invitation(&code)?;
                Ok(SemanticCommandResponse {
                    submission: submitted.submission,
                    handle: submitted.handle,
                    value: SemanticCommandValue::None,
                })
            }
            IntentAction::InviteActorToChannel {
                authority_id,
                channel_id,
                context_id: _,
                channel_name: _,
            } => {
                let submitted = self.submit_invite_actor_to_channel(
                    &authority_id,
                    channel_id.as_deref(),
                    None,
                    None,
                )?;
                Ok(SemanticCommandResponse {
                    submission: submitted.submission,
                    handle: submitted.handle,
                    value: SemanticCommandValue::None,
                })
            }
            IntentAction::AcceptPendingChannelInvitation => {
                let submitted = self.submit_accept_pending_channel_invitation()?;
                Ok(SemanticCommandResponse {
                    submission: submitted.submission,
                    handle: submitted.handle,
                    value: SemanticCommandValue::None,
                })
            }
            IntentAction::CreateChannel { channel_name } => {
                let submitted = self.submit_create_channel(&channel_name)?;
                Ok(SemanticCommandResponse {
                    submission: submitted.submission,
                    handle: submitted.handle,
                    value: SemanticCommandValue::AuthoritativeChannelBinding {
                        channel_id: submitted.value.channel_id,
                        context_id: submitted.value.context_id,
                    },
                })
            }
            IntentAction::JoinChannel { channel_name } => {
                let submitted = self.submit_join_channel(&channel_name)?;
                Ok(SemanticCommandResponse {
                    submission: submitted.submission,
                    handle: submitted.handle,
                    value: SemanticCommandValue::AuthoritativeChannelBinding {
                        channel_id: submitted.value.channel_id,
                        context_id: submitted.value.context_id,
                    },
                })
            }
            IntentAction::SendChatMessage { message } => {
                let handle = require_ui_operation_handle(
                    self.send_harness_command_receipt(&HarnessUiCommand::SendChatMessage {
                        content: message,
                    })?,
                    "send_chat_message",
                )?;
                Ok(SemanticCommandResponse {
                    submission: SubmissionState::Accepted,
                    handle: SemanticSubmissionHandle {
                        ui_operation: Some(handle),
                    },
                    value: SemanticCommandValue::None,
                })
            }
        }
    }

    fn submit_create_account(&mut self, account_name: &str) -> Result<SubmittedAction<()>> {
        let handle = require_ui_operation_handle(
            self.send_harness_command_receipt(&HarnessUiCommand::CreateAccount {
                account_name: account_name.to_string(),
            })?,
            "create_account",
        )?;
        Ok(SubmittedAction::with_ui_operation((), handle))
    }

    fn submit_create_home(&mut self, home_name: &str) -> Result<SubmittedAction<()>> {
        let handle = require_ui_operation_handle(
            self.send_harness_command_receipt(&HarnessUiCommand::CreateHome {
                home_name: home_name.to_string(),
            })?,
            "create_home",
        )?;
        Ok(SubmittedAction::with_ui_operation((), handle))
    }

    fn submit_create_channel(
        &mut self,
        channel_name: &str,
    ) -> Result<SubmittedAction<ChannelBinding>> {
        let operation_name =
            format!("submit_create_channel: create_channel_command:{channel_name}");
        let receipt = self
            .send_harness_command_receipt(&HarnessUiCommand::CreateChannel {
                channel_name: channel_name.to_string(),
            })
            .with_context(|| operation_name.clone())?;
        require_channel_binding_submission(receipt, "submit_create_channel")
    }

    fn submit_create_contact_invitation(
        &mut self,
        receiver_authority_id: &str,
    ) -> Result<SubmittedAction<ContactInvitationCode>> {
        let (handle, code) = require_contact_invitation_submission(
            self.send_harness_command_receipt(&HarnessUiCommand::CreateContactInvitation {
                receiver_authority_id: receiver_authority_id.to_string(),
            })?,
            "create_contact_invitation",
        )?;
        let code = code.ok_or_else(|| {
            anyhow::anyhow!(
                "create_contact_invitation accepted without an authoritative contact invitation code payload"
            )
        })?;
        self.dismiss_contact_invitation_code_modal(Duration::from_secs(5))?;
        Ok(SubmittedAction::with_ui_operation(code, handle))
    }

    fn submit_accept_contact_invitation(&mut self, code: &str) -> Result<SubmittedAction<()>> {
        let handle = require_ui_operation_handle(
            self.send_harness_command(&HarnessUiCommand::ImportInvitation {
                code: code.to_string(),
            })?,
            "accept_contact_invitation",
        )?;
        Ok(SubmittedAction::with_ui_operation((), handle))
    }

    fn submit_invite_actor_to_channel(
        &mut self,
        authority_id: &str,
        channel_id: Option<&str>,
        _context_id: Option<&str>,
        _channel_name: Option<&str>,
    ) -> Result<SubmittedAction<()>> {
        let channel_id = channel_id.map(ToOwned::to_owned).ok_or_else(|| {
            anyhow::anyhow!(
                "submit_invite_actor_to_channel requires an authoritative channel binding"
            )
        })?;
        let handle = require_ui_operation_handle(
            self.send_harness_command(&HarnessUiCommand::InviteActorToChannel {
                authority_id: authority_id.to_string(),
                channel_id,
            })?,
            "invite_actor_to_channel",
        )?;
        Ok(SubmittedAction::with_ui_operation((), handle))
    }

    fn submit_accept_pending_channel_invitation(&mut self) -> Result<SubmittedAction<()>> {
        let handle = require_ui_operation_handle(
            self.send_harness_command_receipt(&HarnessUiCommand::AcceptPendingChannelInvitation)?,
            "accept_pending_channel_invitation",
        )?;
        Ok(SubmittedAction::with_ui_operation((), handle))
    }

    fn submit_join_channel(
        &mut self,
        channel_name: &str,
    ) -> Result<SubmittedAction<ChannelBinding>> {
        let receipt = self.send_harness_command_receipt(&HarnessUiCommand::JoinChannel {
            channel_name: channel_name.to_string(),
        })?;
        require_channel_binding_submission(receipt, "join_channel")
    }

    fn submit_send_chat_message(&mut self, message: &str) -> Result<SubmittedAction<()>> {
        let handle = require_ui_operation_handle(
            self.send_harness_command_receipt(&HarnessUiCommand::SendChatMessage {
                content: message.to_string(),
            })?,
            "send_chat_message",
        )?;
        Ok(SubmittedAction::with_ui_operation((), handle))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use super::*;
    use crate::config::InstanceMode;

    #[allow(clippy::disallowed_methods)]
    fn unique_test_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "aura-harness-local-{label}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("create temp test dir: {error}"));
        root
    }

    fn test_config() -> InstanceConfig {
        InstanceConfig {
            id: "local-test".to_string(),
            mode: InstanceMode::Local,
            data_dir: unique_test_dir("default"),
            device_id: None,
            bind_address: "127.0.0.1:41001".to_string(),
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
        }
    }

    fn wait_for_screen_contains(
        backend: &mut LocalPtyBackend,
        needle: &str,
        timeout: Duration,
    ) -> String {
        const POLL_INTERVAL: Duration = Duration::from_millis(30);

        let mut last_screen = String::new();
        crate::timeouts::blocking_wait_until(timeout, POLL_INTERVAL, || {
            let screen = match backend.diagnostic_screen_snapshot() {
                Ok(screen) => screen,
                Err(error) => panic!("snapshot failed: {error}"),
            };
            if screen.contains(needle) {
                return Some(screen);
            }
            last_screen = screen;
            None
        })
        .unwrap_or_else(|| {
            panic!("timed out waiting for screen to contain {needle:?}; got: {last_screen:?}")
        })
    }

    #[test]
    fn local_backend_injects_default_clipboard_isolation_env() {
        let mut config = test_config();
        config.id = "local-test-clipboard-default".to_string();
        config.data_dir = unique_test_dir("clipboard-default");
        config.command = Some("bash".to_string());
        config.args = vec![
            "-lc".to_string(),
            "printf \"mode=%s file=%s\\n\" \"$AURA_CLIPBOARD_MODE\" \"$AURA_CLIPBOARD_FILE\"; cat"
                .to_string(),
        ];
        config.env = vec![];

        let mut backend = LocalPtyBackend::new(config, Some(20), Some(120));
        if let Err(error) = backend.start() {
            panic!("backend must start: {error}");
        }
        let screen =
            wait_for_screen_contains(&mut backend, "mode=file_only", Duration::from_secs(3));
        assert!(
            screen.contains("mode=file_only"),
            "expected default clipboard mode to be file_only, got: {screen:?}"
        );
        assert!(
            screen.contains("file="),
            "expected clipboard file to be present, got: {screen:?}"
        );
        if let Err(error) = backend.stop() {
            panic!("backend must stop: {error}");
        }
    }

    #[test]
    fn local_shared_intent_methods_use_semantic_harness_commands_for_shared_flows() {
        let source = include_str!("local_pty.rs");
        let invitation_start = source
            .find("fn submit_create_contact_invitation")
            .unwrap_or_else(|| panic!("missing submit_create_contact_invitation"));
        let invitation_end = source[invitation_start..]
            .find("fn submit_accept_contact_invitation")
            .map(|offset| invitation_start + offset)
            .unwrap_or_else(|| panic!("missing submit_accept_contact_invitation"));
        let invitation_branch = &source[invitation_start..invitation_end];
        assert!(source.contains("fn submit_create_account"));
        assert!(
            source.contains("self.send_harness_command_receipt(&HarnessUiCommand::CreateAccount {")
        );
        assert!(source.contains("fn submit_create_home"));
        assert!(
            source.contains("self.send_harness_command_receipt(&HarnessUiCommand::CreateHome {")
        );
        assert!(source.contains("fn submit_create_contact_invitation"));
        assert!(source.contains(
            "self.send_harness_command_receipt(&HarnessUiCommand::CreateContactInvitation {"
        ));
        assert!(source.contains(
            "fn dismiss_contact_invitation_code_modal(&mut self, timeout: Duration) -> Result<()> {"
        ));
        assert!(invitation_branch
            .contains("self.dismiss_contact_invitation_code_modal(Duration::from_secs(5))?;"));
        assert!(!invitation_branch.contains("wait_for_contact_invitation_code"));
        assert!(source.contains("self.send_harness_command(&HarnessUiCommand::DismissTransient)?;"));
        assert!(
            source.contains("self.send_harness_command(&HarnessUiCommand::StartDeviceEnrollment {")
        );
        assert!(source
            .contains("self.send_harness_command(&HarnessUiCommand::ImportDeviceEnrollmentCode {"));
        assert!(source.contains(
            "self.send_harness_command(&HarnessUiCommand::RemoveSelectedDevice { device_id })?;"
        ));
        assert!(source.contains(
            "self.send_harness_command(&HarnessUiCommand::SwitchAuthority { authority_id })?;"
        ));
        assert!(
            source.contains("self.send_harness_command(&HarnessUiCommand::InviteActorToChannel {")
        );
        assert!(source.contains("self.send_harness_command(&HarnessUiCommand::SelectChannel {"));
        assert!(!invitation_branch
            .contains("self.activate_control(ControlId::ContactsCreateInvitationButton)?;"));
        assert!(
            !invitation_branch.contains("self.activate_control(ControlId::ModalCancelButton)?;")
        );
        assert!(!invitation_branch.contains(
            "wait_for_modal_visible(self, ModalId::CreateInvitation, Duration::from_secs(5))?;"
        ));
    }

    #[test]
    fn local_frontend_conformance_preserves_navigation_and_settings_semantics() {
        let source = include_str!("local_pty.rs");
        assert!(source.contains("IntentAction::OpenScreen(screen) => {"));
        assert!(source
            .contains("self.send_harness_command(&HarnessUiCommand::NavigateScreen { screen })?;"));
        assert!(source.contains("IntentAction::OpenSettingsSection(section) => {"));
        assert!(source.contains(
            "self.send_harness_command(&HarnessUiCommand::OpenSettingsSection { section })?;"
        ));
    }

    #[test]
    fn local_backend_parity_critical_submissions_require_handles_and_single_issue_path() {
        let source = include_str!("local_pty.rs");
        let create_account_start = source
            .find("fn submit_create_account")
            .unwrap_or_else(|| panic!("missing submit_create_account"));
        let accept_start = source
            .find("fn submit_accept_pending_channel_invitation")
            .unwrap_or_else(|| panic!("missing submit_accept_pending_channel_invitation"));
        let join_start = source
            .find("fn submit_join_channel")
            .unwrap_or_else(|| panic!("missing submit_join_channel"));
        let send_start = source
            .find("fn submit_send_chat_message")
            .unwrap_or_else(|| panic!("missing submit_send_chat_message"));
        let next_fn_after_send = source[send_start..]
            .find("}\n}\n\n#[cfg(test)]")
            .map(|offset| send_start + offset)
            .unwrap_or(source.len());
        let semantic_submission_block = &source[create_account_start..next_fn_after_send];
        let accept_branch = &source[accept_start..join_start];
        let join_branch = &source[join_start..send_start];
        let send_branch = &source[send_start..next_fn_after_send];

        assert!(
            !accept_branch.contains("SubmittedAction::without_handle"),
            "accept_pending_channel_invitation must not short-circuit around canonical owner handles"
        );
        assert!(
            !join_branch.contains("SubmittedAction::without_handle"),
            "join_channel must not short-circuit around canonical owner handles"
        );
        assert!(
            !semantic_submission_block.contains("wait_for_operation_submission("),
            "shared semantic local PTY submissions must not repair missing handles by polling for likely operation instances"
        );
        assert!(
            !semantic_submission_block.contains("SelectHome"),
            "shared semantic local PTY submissions must not infer create success from visible homes"
        );
        assert!(
            !semantic_submission_block.contains("OperationState::Submitting"),
            "shared semantic local PTY submissions must not treat Submitting as semantic success"
        );
        assert!(
            !semantic_submission_block.contains("await_channel_binding_for_operation"),
            "shared semantic local PTY submissions must not repair missing channel bindings after issue"
        );
        assert!(
            source.contains("fn require_ui_operation_handle("),
            "shared semantic local PTY submissions must use one strict receipt-to-handle conversion path"
        );
        assert_eq!(
            send_branch
                .matches("HarnessUiCommand::SendChatMessage")
                .count(),
            1,
            "send_chat_message must not retry by issuing the semantic command twice"
        );
    }

    #[test]
    fn local_backend_selection_reads_use_canonical_snapshot_selections_only() {
        let source = include_str!("local_pty.rs");
        let authority_start = source
            .find("fn authority_id(&mut self) -> Result<Option<String>> {")
            .unwrap_or_else(|| panic!("missing authority_id"));
        let health_check_start = source[authority_start..]
            .find("fn health_check(&self) -> Result<bool> {")
            .map(|offset| authority_start + offset)
            .unwrap_or(source.len());
        let authority_block = &source[authority_start..health_check_start];

        assert!(
            authority_block.contains(".selected_item_id(ListId::Authorities)"),
            "authority_id must read the canonical exported authority selection"
        );
        assert!(
            !authority_block.contains(".lists"),
            "authority_id must not scan list rows as a fallback authority source"
        );
        assert!(
            !authority_block.contains("item.selected"),
            "authority_id must not infer selection from row highlight state"
        );
    }

    #[test]
    fn local_backend_send_message_issue_path_does_not_prevalidate_selected_channel_snapshot() {
        let source = include_str!("local_pty.rs");
        let send_start = source
            .find("fn submit_send_chat_message(&mut self, message: &str) -> Result<SubmittedAction<()>> {")
            .unwrap_or_else(|| panic!("missing submit_send_chat_message"));
        let next_fn_after_send = source[send_start..]
            .find("}\n}\n\n#[cfg(test)]")
            .map(|offset| send_start + offset)
            .unwrap_or(source.len());
        let send_block = &source[send_start..next_fn_after_send];

        assert!(
            !send_block.contains("selected_channel_ref"),
            "send_chat_message must not gate submission on harness-side snapshot repair logic"
        );
        assert!(
            !send_block.contains("authoritative selected channel reference"),
            "send_chat_message must defer selection validity to the command plane"
        );
    }

    #[test]
    fn local_backend_default_command_targets_aura_tui() {
        let mut config = test_config();
        config.command = None;
        config.args.clear();
        config.data_dir = unique_test_dir("default-cmd");
        config.device_id = Some("local-test-device".to_string());
        config.bind_address = "127.0.0.1:49999".to_string();

        let backend = LocalPtyBackend::new(config, Some(20), Some(120));
        let (program, args) = backend.command_spec();

        assert!(
            program.ends_with("cargo") || program.ends_with("aura"),
            "default program must target cargo or aura binary, got: {program}"
        );
        if program.ends_with("cargo") {
            assert!(
                args.windows(2)
                    .any(|window| window == ["-p", "aura-terminal"]),
                "cargo launch must target aura-terminal package: {args:?}"
            );
            assert!(
                args.windows(2).any(|window| window == ["--bin", "aura"]),
                "cargo launch must target aura binary: {args:?}"
            );
            assert!(
                args.contains(&"--".to_string()),
                "cargo launch must separate cargo args from aura args: {args:?}"
            );
        }
        assert!(
            args.iter().any(|arg| arg == "tui"),
            "default args must include tui subcommand: {args:?}"
        );
        assert!(
            args.windows(2)
                .any(|window| window == ["--bind-address", "127.0.0.1:49999"]),
            "default args must include bind address: {args:?}"
        );
        assert!(
            args.windows(2)
                .any(|window| window == ["--device-id", "local-test-device"]),
            "default args must include device id: {args:?}"
        );
        assert!(
            args.windows(2).any(|window| window[0] == "--data-dir"),
            "default args must include data dir: {args:?}"
        );
    }

    #[test]
    fn local_backend_respects_clipboard_env_override() {
        let mut config = test_config();
        config.id = "local-test-clipboard-override".to_string();
        config.data_dir = unique_test_dir("clipboard-override");
        config.command = Some("bash".to_string());
        config.args = vec![
            "-lc".to_string(),
            "printf \"mode=%s file=%s\\n\" \"$AURA_CLIPBOARD_MODE\" \"$AURA_CLIPBOARD_FILE\"; cat"
                .to_string(),
        ];
        config.env = vec![
            "AURA_CLIPBOARD_MODE=system".to_string(),
            "AURA_CLIPBOARD_FILE=/tmp/custom-harness-clipboard.txt".to_string(),
        ];

        let mut backend = LocalPtyBackend::new(config, Some(20), Some(120));
        if let Err(error) = backend.start() {
            panic!("backend must start: {error}");
        }
        let screen = wait_for_screen_contains(&mut backend, "mode=system", Duration::from_secs(3));
        assert!(
            screen.contains("mode=system"),
            "expected custom clipboard mode override, got: {screen:?}"
        );
        assert!(
            screen.contains("file=/tmp/custom-harness-clipboard.txt"),
            "expected custom clipboard file override, got: {screen:?}"
        );
        if let Err(error) = backend.stop() {
            panic!("backend must stop: {error}");
        }
    }

    #[test]
    fn local_backend_uses_socket_driven_ui_snapshot_channel() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let backend_path = repo_root.join("crates/aura-harness/src/backend/local_pty.rs");
        let source = std::fs::read_to_string(&backend_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", backend_path.display()));
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(&source);
        let ui_snapshot_body = production_source
            .split("fn ui_snapshot(&self) -> Result<UiSnapshot> {")
            .nth(1)
            .and_then(|body| body.split("\n    fn ").next())
            .unwrap_or(production_source);

        assert!(
            production_source.contains("AURA_TUI_UI_STATE_SOCKET"),
            "local PTY backend must provision an event-driven TUI snapshot socket"
        );
        assert!(
            !ui_snapshot_body.contains("SNAPSHOT_WAIT_ATTEMPTS")
                && !ui_snapshot_body.contains("fs::read_to_string(&path)"),
            "local PTY UI snapshot path may not poll the filesystem"
        );
    }

    #[test]
    fn missing_tui_ui_snapshot_fails_loudly() {
        let mut config = test_config();
        config.id = "local-test-missing-ui-snapshot".to_string();
        config.data_dir = unique_test_dir("missing-ui-snapshot");

        let mut backend = LocalPtyBackend::new(config, Some(20), Some(120));
        backend
            .start()
            .unwrap_or_else(|error| panic!("backend must start: {error}"));

        let error = backend
            .ui_snapshot()
            .err()
            .unwrap_or_else(|| panic!("missing UI snapshot publication must fail"));
        let message = format!("{error:#}");
        assert!(
            message.contains("TUI UI snapshot unavailable"),
            "missing TUI snapshot publication must fail diagnostically, got: {message}"
        );

        backend
            .stop()
            .unwrap_or_else(|error| panic!("backend must stop: {error}"));
    }

    #[test]
    fn local_backend_start_send_snapshot_stop() {
        let mut backend = LocalPtyBackend::new(test_config(), Some(40), Some(120));
        if let Err(error) = backend.start() {
            panic!("backend must start: {error}");
        }
        if let Err(error) = backend.send_keys("hello-harness\n") {
            panic!("keys send failed: {error}");
        }
        blocking_sleep(Duration::from_millis(80));
        let screen = match backend.diagnostic_screen_snapshot() {
            Ok(screen) => screen,
            Err(error) => panic!("snapshot failed: {error}"),
        };
        assert!(screen.contains("hello-harness"));
        if let Err(error) = backend.stop() {
            panic!("backend must stop: {error}");
        }
    }

    #[test]
    fn local_snapshot_is_bounded_by_pty_rows() {
        let mut backend = LocalPtyBackend::new(test_config(), Some(40), Some(120));
        if let Err(error) = backend.start() {
            panic!("backend must start: {error}");
        }
        blocking_sleep(Duration::from_millis(50));
        let screen = match backend.diagnostic_screen_snapshot() {
            Ok(screen) => screen,
            Err(error) => panic!("snapshot must succeed: {error}"),
        };
        let line_count = screen.lines().count();
        assert!(
            line_count <= 40,
            "snapshot should not exceed configured PTY rows (got {line_count})"
        );
        if let Err(error) = backend.stop() {
            panic!("backend must stop: {error}");
        }
    }

    #[test]
    fn local_snapshot_observes_recent_output_without_extra_sleep() {
        let mut backend = LocalPtyBackend::new(test_config(), Some(40), Some(120));
        if let Err(error) = backend.start() {
            panic!("backend must start: {error}");
        }
        if let Err(error) = backend.send_keys("freshness-check\n") {
            panic!("send_keys must succeed: {error}");
        }
        let screen = match backend.diagnostic_screen_snapshot() {
            Ok(screen) => screen,
            Err(error) => panic!("snapshot must succeed: {error}"),
        };
        assert!(screen.contains("freshness-check"));
        if let Err(error) = backend.stop() {
            panic!("backend must stop: {error}");
        }
    }

    #[test]
    fn local_tail_log_reads_dat_fallback_path() {
        let temp_root = unique_test_dir("tail-log-dat");
        let _ = fs::remove_dir_all(&temp_root);
        if let Err(error) = fs::create_dir_all(&temp_root) {
            panic!("create temp dir: {error}");
        }

        let mut config = test_config();
        config.data_dir = temp_root.clone();
        config.log_path = Some(temp_root.join("instance.log"));

        if let Err(error) = fs::write(temp_root.join("instance.log.dat"), "line-1\nline-2\n") {
            panic!("write log: {error}");
        }

        let backend = LocalPtyBackend::new(config, Some(40), Some(120));
        let lines = match backend.tail_log(1) {
            Ok(lines) => lines,
            Err(error) => panic!("tail_log should succeed: {error}"),
        };
        assert_eq!(lines, vec!["line-2".to_string()]);
    }

    #[test]
    fn authoritative_snapshot_falls_back_to_cached_tui_frame() {
        let backend = LocalPtyBackend::new(test_config(), Some(40), Some(120));
        let tui_frame = "Neighborhood Chat Contacts Notifications Settings\nframe".to_string();
        let noisy = "2026-01-01T00:00:00Z INFO log line".to_string();

        let first = backend.select_authoritative_screen(tui_frame.clone());
        assert_eq!(first, tui_frame);

        let second = backend.select_authoritative_screen(noisy);
        assert_eq!(second, first);
    }

    #[test]
    fn authoritative_snapshot_strips_stale_prefix_rows() {
        let backend = LocalPtyBackend::new(test_config(), Some(40), Some(120));
        let mixed = "\
stale footer row\n\
Neighborhood Chat Contacts Notifications Settings\n\
latest frame row";

        let selected = backend.select_authoritative_screen(mixed.to_string());
        assert_eq!(
            selected,
            "Neighborhood Chat Contacts Notifications Settings\nlatest frame row"
        );
    }
}
