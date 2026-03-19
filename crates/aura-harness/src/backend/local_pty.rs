use std::collections::BTreeSet;
use std::fs;
use std::hash::{Hash, Hasher};
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
use aura_app::scenario_contract::{
    IntentAction, SemanticCommandRequest, SemanticCommandResponse, SemanticCommandValue,
};
use aura_app::ui::contract::{
    ControlId, FieldId, HarnessUiCommand, HarnessUiCommandReceipt, ListId, ModalId, OperationId,
    OperationState, ScreenId, UiReadiness, UiSnapshot,
};
use aura_app::ui_contract::RuntimeFact;
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::backend::{
    latest_invitation_code, observe_operation, wait_for_operation_submission, ChannelBinding,
    ContactInvitationCode, InstanceBackend, RawUiBackend, SharedSemanticBackend, SubmittedAction,
    UiOperationHandle, UiSnapshotEvent,
};
use crate::config::InstanceConfig;
use crate::screen_normalization::{authoritative_screen, has_nav_header};
use crate::timeouts::blocking_sleep;
use crate::workspace_root;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BackendState {
    Stopped,
    Running,
}

const PLACEHOLDER_HOME_ID: &str =
    "channel:0000000000000000000000000000000000000000000000000000000000000000";

fn snapshot_has_real_home(snapshot: &UiSnapshot) -> bool {
    snapshot
        .lists
        .iter()
        .find(|list| list.id == ListId::Homes)
        .map(|list| list.items.iter().any(|item| item.id != PLACEHOLDER_HOME_ID))
        .unwrap_or(false)
}

fn select_home_and_channel(backend: &mut LocalPtyBackend, channel_id: &str) -> Result<()> {
    let snapshot = backend.ui_snapshot()?;
    let home_visible = snapshot
        .lists
        .iter()
        .find(|list| list.id == ListId::Homes)
        .is_some_and(|list| list.items.iter().any(|item| item.id == channel_id));
    if home_visible {
        backend.send_harness_command(&HarnessUiCommand::SelectHome {
            home_id: channel_id.to_string(),
        })?;
    }
    backend.send_harness_command(&HarnessUiCommand::SelectChannel {
        channel_id: channel_id.to_string(),
    })?;
    Ok(())
}

fn unique_shared_channel_candidate(snapshot: &UiSnapshot) -> Option<String> {
    let mut candidates = snapshot
        .runtime_events
        .iter()
        .filter_map(|event| match &event.fact {
            RuntimeFact::ChannelMembershipReady {
                channel,
                member_count: Some(member_count),
            } if *member_count > 1
                && channel
                    .name
                    .as_deref()
                    .map(|name| !name.eq_ignore_ascii_case("note to self"))
                    .unwrap_or(true) =>
            {
                channel.id.clone()
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    if let [channel_id] = candidates.as_slice() {
        return Some(channel_id.clone());
    }

    let note_to_self_id = snapshot
        .runtime_events
        .iter()
        .find_map(|event| match &event.fact {
            RuntimeFact::ChannelMembershipReady { channel, .. }
                if channel
                    .name
                    .as_deref()
                    .map(|name| name.eq_ignore_ascii_case("note to self"))
                    .unwrap_or(false) =>
            {
                channel.id.clone()
            }
            _ => None,
        });
    let mut listed_candidates = snapshot
        .lists
        .iter()
        .find(|list| list.id == ListId::Channels)
        .map(|list| {
            list.items
                .iter()
                .map(|item| item.id.clone())
                .filter(|item_id| note_to_self_id.as_deref() != Some(item_id.as_str()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    listed_candidates.sort();
    listed_candidates.dedup();
    match listed_candidates.as_slice() {
        [channel_id] => Some(channel_id.clone()),
        _ => None,
    }
}

fn authoritative_channel_binding(
    snapshot: &UiSnapshot,
    channel_name: &str,
) -> Option<ChannelBinding> {
    snapshot
        .runtime_events
        .iter()
        .rev()
        .find_map(|event| match &event.fact {
            RuntimeFact::ChannelMembershipReady { channel, .. }
                if channel
                    .name
                    .as_deref()
                    .map(|name| name.eq_ignore_ascii_case(channel_name))
                    .unwrap_or(false) =>
            {
                channel.id.as_ref().map(|channel_id| ChannelBinding {
                    channel_id: channel_id.clone(),
                    context_id: None,
                })
            }
            _ => None,
        })
}

fn materialized_channel_binding(
    snapshot: &UiSnapshot,
    previous_channel_ids: &BTreeSet<String>,
) -> Option<ChannelBinding> {
    let channels = snapshot
        .lists
        .iter()
        .find(|list| list.id == ListId::Channels)?;
    let mut new_channel_ids = channels
        .items
        .iter()
        .filter(|item| !previous_channel_ids.contains(&item.id))
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    new_channel_ids.sort();
    new_channel_ids.dedup();
    match new_channel_ids.as_slice() {
        [channel_id] => Some(ChannelBinding {
            channel_id: channel_id.clone(),
            context_id: None,
        }),
        _ => None,
    }
}

struct RunningSession {
    child: Box<dyn Child + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<vt100::Parser>>,
    parse_generation: Arc<AtomicU64>,
    reader_thread: Option<thread::JoinHandle<()>>,
    ui_snapshot_feed: Arc<UiSnapshotFeed>,
    ui_snapshot_thread: Option<thread::JoinHandle<()>>,
    ui_snapshot_stop: Arc<AtomicU64>,
    ui_snapshot_socket_path: PathBuf,
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
        if Self::reader_thread_alive(session) {
            return Ok(());
        }
        let screen = Self::read_screen(&session.parser);
        anyhow::bail!(
            "local PTY instance {} exited before {context}; last_screen={:?}",
            self.config.id,
            self.select_authoritative_screen(screen)
        );
    }

    fn is_cargo_program(program: &str) -> bool {
        std::path::Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name == "cargo")
            .unwrap_or(false)
    }

    fn ui_state_socket_path(&self) -> PathBuf {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.config.data_dir.hash(&mut hasher);
        self.config.id.hash(&mut hasher);
        workspace_root()
            .join(".tmp")
            .join("harness-ui")
            .join(format!("{:016x}.sock", hasher.finish()))
    }

    fn command_socket_path(&self) -> PathBuf {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.config.data_dir.hash(&mut hasher);
        self.config.id.hash(&mut hasher);
        "command".hash(&mut hasher);
        workspace_root()
            .join(".tmp")
            .join("harness-ui")
            .join(format!("{:016x}.cmd.sock", hasher.finish()))
    }

    pub fn new(config: InstanceConfig, pty_rows: Option<u16>, pty_cols: Option<u16>) -> Self {
        Self {
            config,
            state: BackendState::Stopped,
            session: None,
            pty_rows: pty_rows.unwrap_or(40),
            pty_cols: pty_cols.unwrap_or(120),
            last_authoritative_screen: Arc::new(Mutex::new(None)),
        }
    }

    fn spawn_ui_snapshot_listener(
        socket_path: &PathBuf,
        feed: &Arc<UiSnapshotFeed>,
        stop_flag: &Arc<AtomicU64>,
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
        Ok(thread::spawn(move || {
            for stream in listener.incoming() {
                if stop_flag.load(Ordering::Acquire) > 0 {
                    break;
                }
                let Ok(mut stream) = stream else {
                    continue;
                };
                let mut payload = String::new();
                if stream.read_to_string(&mut payload).is_err() {
                    continue;
                }
                if payload.trim() == "__AURA_UI_STATE_SHUTDOWN__" {
                    break;
                }
                let Ok(snapshot) = serde_json::from_str::<UiSnapshot>(&payload) else {
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

    fn send_harness_command(
        &mut self,
        command: &HarnessUiCommand,
    ) -> Result<Option<aura_app::ui_contract::HarnessUiOperationHandle>> {
        let socket_path = Self::absolutize_path(self.command_socket_path());
        let payload = serde_json::to_vec(command).context("failed to encode harness UI command")?;
        let deadline = Instant::now() + Duration::from_secs(10);

        loop {
            match UnixStream::connect(&socket_path) {
                Ok(mut stream) => {
                    let command_result: Result<
                        Option<aura_app::ui_contract::HarnessUiOperationHandle>,
                    > = (|| {
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
                        match receipt {
                            HarnessUiCommandReceipt::Accepted { operation } => Ok(operation),
                            HarnessUiCommandReceipt::Rejected { reason } => {
                                anyhow::bail!("TUI harness command rejected: {reason}")
                            }
                        }
                    })();

                    match command_result {
                        Ok(operation) => return Ok(operation),
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
                    if self
                        .session
                        .as_ref()
                        .and_then(|session| session.reader_thread.as_ref())
                        .is_some_and(|thread| thread.is_finished())
                    {
                        let screen = self.snapshot().unwrap_or_default();
                        let log_tail = self.tail_log(40).unwrap_or_default().join("\n");
                        return Err(anyhow::anyhow!(
                            "local PTY instance {} exited before the harness command plane became ready; socket={} screen={:?} log_tail={}",
                            self.config.id,
                            socket_path.display(),
                            screen,
                            log_tail
                        ));
                    }
                    if Instant::now() >= deadline {
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
}

impl InstanceBackend for LocalPtyBackend {
    fn id(&self) -> &str {
        &self.config.id
    }

    fn backend_kind(&self) -> &'static str {
        "local_pty"
    }

    fn supports_ui_snapshot(&self) -> bool {
        self.requires_tui_readiness()
    }

    fn start(&mut self) -> Result<()> {
        if self.state == BackendState::Running {
            return Ok(());
        }
        *self.last_authoritative_screen.blocking_lock() = None;

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
        if Self::env_value("AURA_CLIPBOARD_FILE", &self.config.env).is_none() {
            let clipboard_file =
                Self::absolutize_path(self.config.data_dir.join(".harness-clipboard.txt"));
            command.env(
                "AURA_CLIPBOARD_FILE",
                clipboard_file.to_string_lossy().to_string(),
            );
        }
        if Self::env_value("AURA_TUI_UI_STATE_SOCKET", &self.config.env).is_none() {
            let ui_state_socket = Self::absolutize_path(self.ui_state_socket_path());
            command.env(
                "AURA_TUI_UI_STATE_SOCKET",
                ui_state_socket.to_string_lossy().to_string(),
            );
        }
        if Self::env_value("AURA_TUI_COMMAND_SOCKET", &self.config.env).is_none() {
            let command_socket = Self::absolutize_path(self.command_socket_path());
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
        let ui_snapshot_socket_path = Self::absolutize_path(self.ui_state_socket_path());
        let ui_snapshot_thread = Self::spawn_ui_snapshot_listener(
            &ui_snapshot_socket_path,
            &ui_snapshot_feed,
            &ui_snapshot_stop,
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
        let reader_thread = thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        parser_for_thread.blocking_lock().process(&buffer[..read]);
                        generation_for_thread.fetch_add(1, Ordering::Release);
                    }
                    Err(_) => break,
                }
            }
        });

        self.session = Some(RunningSession {
            child,
            writer: Arc::new(Mutex::new(writer)),
            parser,
            parse_generation,
            reader_thread: Some(reader_thread),
            ui_snapshot_feed,
            ui_snapshot_thread: Some(ui_snapshot_thread),
            ui_snapshot_stop,
            ui_snapshot_socket_path,
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
            let _ = session.child.kill();
            let _ = session.child.wait();
            drop(session.writer);
            if let Some(handle) = session.reader_thread.take() {
                let _ = handle.join();
            }
            if let Some(handle) = session.ui_snapshot_thread.take() {
                let _ = handle.join();
            }
        }

        self.state = BackendState::Stopped;
        Ok(())
    }

    fn snapshot(&self) -> Result<String> {
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

        // Transitional frames can briefly miss the nav header while a full-screen redraw
        // is still in flight. Sample for a short bounded window before falling back.
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

    fn ui_snapshot(&self) -> Result<UiSnapshot> {
        let session = self
            .session
            .as_ref()
            .with_context(|| format!("instance {} is not running", self.config.id))?;
        let guard = session
            .ui_snapshot_feed
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(snapshot) = guard.latest.clone() {
            return Ok(snapshot);
        }
        drop(guard);
        self.ensure_session_alive(session, "reading authoritative UI snapshot")?;
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
            if !Self::reader_thread_alive(session) {
                return Some(Err(anyhow::anyhow!(
                    "local PTY instance {} exited before publishing a newer UI snapshot event; after_version={:?}",
                    self.config.id,
                    after_version
                )));
            }
            let now = Instant::now();
            if now >= deadline {
                return Some(Err(anyhow::anyhow!(
                    "timed out waiting for TUI UI snapshot event on instance {} after_version={:?}",
                    self.config.id,
                    after_version
                )));
            }
            let timeout_result = session
                .ui_snapshot_feed
                .ready
                .wait_timeout(guard, deadline.saturating_duration_since(now))
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard = timeout_result.0;
        }
    }

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
            .unwrap_or_else(|| {
                Self::absolutize_path(self.config.data_dir.join(".harness-clipboard.txt"))
            });
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

    fn health_check(&self) -> Result<bool> {
        let running = self.state == BackendState::Running && self.session.is_some();
        if !running {
            return Ok(false);
        }
        let reader_alive = self.session.as_ref().is_some_and(Self::reader_thread_alive);
        Ok(reader_alive)
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
        loop {
            if let Ok(snapshot) = self.ui_snapshot() {
                if snapshot.readiness == UiReadiness::Ready {
                    return Ok(());
                }
            }
            if self
                .session
                .as_ref()
                .and_then(|session| session.reader_thread.as_ref())
                .is_some_and(|thread| thread.is_finished())
            {
                let screen = self.snapshot().unwrap_or_default();
                anyhow::bail!(
                    "local PTY instance {} exited before publishing an authoritative UI snapshot; screen={:?}",
                    self.config.id,
                    screen
                );
            }
            if Instant::now() >= deadline {
                let readiness = self.ui_snapshot().ok().map(|snapshot| snapshot.readiness);
                anyhow::bail!(
                    "local PTY instance {} did not reach semantic readiness within {:?}; readiness={readiness:?}",
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

impl Drop for LocalPtyBackend {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl RawUiBackend for LocalPtyBackend {
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
                    let needs_devices_section = snapshot
                        .selections
                        .iter()
                        .find(|selection| selection.list == ListId::SettingsSections)
                        .map(|selection| selection.item_id.as_str() != "devices")
                        .unwrap_or(true);
                    if needs_devices_section {
                        self.activate_list_item(ListId::SettingsSections, "devices")?;
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
        let current_index = list
            .items
            .iter()
            .position(|item| item.selected)
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
            let selected_item = current_snapshot
                .lists
                .iter()
                .find(|candidate| candidate.id == list_id)
                .and_then(|candidate| candidate.items.iter().find(|item| item.selected))
                .map(|item| item.id.as_str());
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
    fn submit_start_device_enrollment(&mut self, device_name: &str) -> Result<SubmittedAction<()>> {
        let previous_operation =
            observe_operation(&self.ui_snapshot()?, &OperationId::device_enrollment());
        self.send_harness_command(&HarnessUiCommand::StartDeviceEnrollment {
            device_name: device_name.to_string(),
        })?;
        let handle = wait_for_operation_submission(
            self,
            OperationId::device_enrollment(),
            previous_operation,
            Duration::from_secs(5),
        )?;
        Ok(SubmittedAction::with_ui_operation((), handle))
    }

    fn submit_import_device_enrollment_code(&mut self, code: &str) -> Result<SubmittedAction<()>> {
        let previous_operation =
            observe_operation(&self.ui_snapshot()?, &OperationId::device_enrollment());
        self.send_harness_command(&HarnessUiCommand::ImportDeviceEnrollmentCode {
            code: code.to_string(),
        })?;
        let handle = wait_for_operation_submission(
            self,
            OperationId::device_enrollment(),
            previous_operation,
            Duration::from_secs(5),
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
                let submitted = self.submit_create_account(&account_name)?;
                Ok(SemanticCommandResponse {
                    submission: submitted.submission,
                    handle: submitted.handle,
                    value: SemanticCommandValue::None,
                })
            }
            IntentAction::CreateHome { home_name } => {
                let submitted = self.submit_create_home(&home_name)?;
                Ok(SemanticCommandResponse {
                    submission: submitted.submission,
                    handle: submitted.handle,
                    value: SemanticCommandValue::None,
                })
            }
            IntentAction::StartDeviceEnrollment { device_name, .. } => {
                let submitted = self.submit_start_device_enrollment(&device_name)?;
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
            IntentAction::RemoveSelectedDevice => {
                self.send_harness_command(&HarnessUiCommand::RemoveSelectedDevice)?;
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
            } => {
                let submitted =
                    self.submit_invite_actor_to_channel(&authority_id, channel_id.as_deref())?;
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
                    value: SemanticCommandValue::ChannelBinding {
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
                    value: SemanticCommandValue::None,
                })
            }
            IntentAction::SendChatMessage { message } => {
                let submitted = self.submit_send_chat_message(&message)?;
                Ok(SemanticCommandResponse {
                    submission: submitted.submission,
                    handle: submitted.handle,
                    value: SemanticCommandValue::None,
                })
            }
        }
    }

    fn submit_create_account(&mut self, account_name: &str) -> Result<SubmittedAction<()>> {
        let previous_operation =
            observe_operation(&self.ui_snapshot()?, &OperationId::account_create());
        let receipt_handle = self.send_harness_command(&HarnessUiCommand::CreateAccount {
            account_name: account_name.to_string(),
        })?;
        let handle = match receipt_handle {
            Some(handle) => UiOperationHandle::new(
                handle.operation_id().clone(),
                handle.instance_id().clone(),
            ),
            None => wait_for_operation_submission(
                self,
                OperationId::account_create(),
                previous_operation,
                Duration::from_secs(5),
            )?,
        };
        let issue_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = self.ui_snapshot()?;
            if snapshot.operation_state_for_instance(handle.id(), handle.instance_id())
                == Some(aura_app::ui::contract::OperationState::Failed)
            {
                anyhow::bail!("submit_create_account: account creation failed");
            }
            if snapshot
                .operation_state_for_instance(handle.id(), handle.instance_id())
                .is_some_and(|state| {
                    matches!(
                        state,
                        aura_app::ui::contract::OperationState::Submitting
                            | aura_app::ui::contract::OperationState::Succeeded
                    )
                })
            {
                return Ok(SubmittedAction::with_ui_operation((), handle));
            }
            if snapshot.screen != ScreenId::Onboarding || snapshot_has_real_home(&snapshot) {
                return Ok(SubmittedAction::with_ui_operation((), handle));
            }
            if Instant::now() >= issue_deadline {
                anyhow::bail!("submit_create_account: account creation did not issue");
            }
            blocking_sleep(Duration::from_millis(100));
        }
    }

    fn submit_create_home(&mut self, home_name: &str) -> Result<SubmittedAction<()>> {
        let previous_operation =
            observe_operation(&self.ui_snapshot()?, &OperationId::create_home());
        self.send_harness_command(&HarnessUiCommand::CreateHome {
            home_name: home_name.to_string(),
        })?;
        let handle = wait_for_operation_submission(
            self,
            OperationId::create_home(),
            previous_operation,
            Duration::from_secs(5),
        )?;
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let snapshot = self.ui_snapshot()?;
            let created_home = snapshot
                .lists
                .iter()
                .find(|list| list.id == ListId::Homes)
                .and_then(|list| {
                    list.items
                        .iter()
                        .find(|item| item.id != PLACEHOLDER_HOME_ID)
                        .map(|item| item.id.clone())
                });
            if let Some(home_id) = created_home {
                self.send_harness_command(&HarnessUiCommand::SelectHome { home_id })?;
                return Ok(SubmittedAction::with_ui_operation((), handle));
            }
            if Instant::now() >= deadline {
                break;
            }
            blocking_sleep(Duration::from_millis(80));
        }
        anyhow::bail!("submit_create_home did not produce a non-placeholder home")
    }

    fn submit_create_channel(
        &mut self,
        channel_name: &str,
    ) -> Result<SubmittedAction<ChannelBinding>> {
        let previous_snapshot = self.ui_snapshot()?;
        let previous_operation =
            observe_operation(&previous_snapshot, &OperationId::create_channel());
        let previous_channel_ids = previous_snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Channels)
            .map(|list| {
                list.items
                    .iter()
                    .map(|item| item.id.clone())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let receipt_handle = self
            .send_harness_command(&HarnessUiCommand::CreateChannel {
                channel_name: channel_name.to_string(),
            })
            .context("submit_create_channel: create_channel_command")?;
        let handle = match receipt_handle {
            Some(handle) => UiOperationHandle::new(
                handle.operation_id().clone(),
                handle.instance_id().clone(),
            ),
            None => wait_for_operation_submission(
                self,
                OperationId::create_channel(),
                previous_operation,
                Duration::from_secs(5),
            )?,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let snapshot = self.ui_snapshot()?;
            if let Some(binding) = authoritative_channel_binding(&snapshot, channel_name) {
                return Ok(SubmittedAction::with_ui_operation(binding, handle));
            }
            if let Some(binding) = materialized_channel_binding(&snapshot, &previous_channel_ids) {
                return Ok(SubmittedAction::with_ui_operation(binding, handle));
            }
            if snapshot.operation_state_for_instance(handle.id(), handle.instance_id())
                == Some(OperationState::Failed)
            {
                anyhow::bail!("submit_create_channel: create_channel failed");
            }
            if Instant::now() >= deadline {
                anyhow::bail!(
                    "submit_create_channel did not publish an authoritative channel binding for {channel_name}"
                );
            }
            blocking_sleep(Duration::from_millis(80));
        }
    }

    fn submit_create_contact_invitation(
        &mut self,
        receiver_authority_id: &str,
    ) -> Result<SubmittedAction<ContactInvitationCode>> {
        let previous_operation =
            observe_operation(&self.ui_snapshot()?, &OperationId::invitation_create());
        let receipt_handle =
            self.send_harness_command(&HarnessUiCommand::CreateContactInvitation {
                receiver_authority_id: receiver_authority_id.to_string(),
            })?;
        let handle = match receipt_handle {
            Some(handle) => UiOperationHandle::new(
                handle.operation_id().clone(),
                handle.instance_id().clone(),
            ),
            None => wait_for_operation_submission(
                self,
                OperationId::invitation_create(),
                previous_operation,
                Duration::from_secs(5),
            )?,
        };
        let code_deadline = Instant::now() + Duration::from_secs(5);
        let code = loop {
            let snapshot = self.ui_snapshot()?;
            if let Some(code) = latest_invitation_code(&snapshot) {
                break code;
            }
            if Instant::now() >= code_deadline {
                anyhow::bail!(
                    "submit_create_contact_invitation did not publish InvitationCodeReady"
                );
            }
            blocking_sleep(Duration::from_millis(50));
        };

        if self.ui_snapshot()?.open_modal == Some(ModalId::InvitationCode) {
            self.send_harness_command(&HarnessUiCommand::DismissTransient)?;
        }
        let close_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.ui_snapshot()?.open_modal != Some(ModalId::InvitationCode) {
                break;
            }
            if Instant::now() >= close_deadline {
                anyhow::bail!(
                    "submit_create_contact_invitation did not close InvitationCode modal"
                );
            }
            blocking_sleep(Duration::from_millis(50));
        }
        Ok(SubmittedAction::with_ui_operation(
            ContactInvitationCode { code },
            handle,
        ))
    }

    fn submit_accept_contact_invitation(&mut self, code: &str) -> Result<SubmittedAction<()>> {
        let previous_operation =
            observe_operation(&self.ui_snapshot()?, &OperationId::invitation_accept());
        self.send_harness_command(&HarnessUiCommand::ImportInvitation {
            code: code.to_string(),
        })?;
        let handle = wait_for_operation_submission(
            self,
            OperationId::invitation_accept(),
            previous_operation,
            Duration::from_secs(5),
        )?;
        Ok(SubmittedAction::with_ui_operation((), handle))
    }

    fn submit_invite_actor_to_channel(
        &mut self,
        authority_id: &str,
        channel_id: Option<&str>,
    ) -> Result<SubmittedAction<()>> {
        let snapshot = self.ui_snapshot()?;
        let previous_operation = observe_operation(&snapshot, &OperationId::invitation_create());
        let channel_id = channel_id
            .map(ToOwned::to_owned)
            .or_else(|| {
                snapshot
                    .selections
                    .iter()
                    .find(|selection| selection.list == ListId::Channels)
                    .map(|selection| selection.item_id.clone())
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "submit_invite_actor_to_channel requires an authoritative selected channel"
                )
            })?;
        let receipt_handle =
            self.send_harness_command(&HarnessUiCommand::InviteActorToChannel {
                authority_id: authority_id.to_string(),
                channel_id,
            })?;
        let handle = match receipt_handle {
            Some(handle) => UiOperationHandle::new(
                handle.operation_id().clone(),
                handle.instance_id().clone(),
            ),
            None => wait_for_operation_submission(
                self,
                OperationId::invitation_create(),
                previous_operation,
                Duration::from_secs(5),
            )?,
        };
        Ok(SubmittedAction::with_ui_operation((), handle))
    }

    fn submit_accept_pending_channel_invitation(&mut self) -> Result<SubmittedAction<()>> {
        let snapshot = self.ui_snapshot()?;
        let previous_operation = observe_operation(&snapshot, &OperationId::invitation_accept());
        let receipt_handle =
            self.send_harness_command(&HarnessUiCommand::AcceptPendingChannelInvitation)?;
        let handle = match receipt_handle {
            Some(handle) => UiOperationHandle::new(
                handle.operation_id().clone(),
                handle.instance_id().clone(),
            ),
            None => wait_for_operation_submission(
                self,
                OperationId::invitation_accept(),
                previous_operation,
                Duration::from_secs(5),
            )?,
        };
        Ok(SubmittedAction::with_ui_operation((), handle))
    }

    fn submit_join_channel(&mut self, channel_name: &str) -> Result<SubmittedAction<()>> {
        let prejoin_snapshot = self.ui_snapshot()?;
        let already_joined_channel_id =
            prejoin_snapshot
                .runtime_events
                .iter()
                .find_map(|event| match &event.fact {
                    RuntimeFact::ChannelMembershipReady {
                        channel,
                        member_count: Some(member_count),
                        ..
                    } if *member_count > 1
                        && channel
                            .name
                            .as_deref()
                            .map(|name: &str| name.eq_ignore_ascii_case(channel_name))
                            .unwrap_or(false) =>
                    {
                        channel.id.clone()
                    }
                    _ => None,
                });
        if let Some(channel_id) = already_joined_channel_id {
            let selected_channel_id = prejoin_snapshot
                .selections
                .iter()
                .find(|selection| selection.list == ListId::Channels)
                .map(|selection| selection.item_id.clone());
            if selected_channel_id.as_deref() != Some(channel_id.as_str()) {
                select_home_and_channel(self, &channel_id)?;
            }
        }

        let previous_operation = observe_operation(&prejoin_snapshot, &OperationId::join_channel());
        let receipt_handle = self.send_harness_command(&HarnessUiCommand::JoinChannel {
            channel_name: channel_name.to_string(),
        })?;
        let handle = match receipt_handle {
            Some(handle) => UiOperationHandle::new(
                handle.operation_id().clone(),
                handle.instance_id().clone(),
            ),
            None => wait_for_operation_submission(
                self,
                OperationId::join_channel(),
                previous_operation,
                Duration::from_secs(5),
            )?,
        };
        Ok(SubmittedAction::with_ui_operation((), handle))
    }

    fn submit_send_chat_message(&mut self, message: &str) -> Result<SubmittedAction<()>> {
        fn message_visible(snapshot: &UiSnapshot, expected: &str) -> bool {
            snapshot
                .messages
                .iter()
                .any(|message| message.content.contains(expected))
        }

        let snapshot = self.ui_snapshot()?;
        if let Some(channel_id) = unique_shared_channel_candidate(&snapshot) {
            let selected_channel_id = snapshot
                .selections
                .iter()
                .find(|selection| selection.list == ListId::Channels)
                .map(|selection| selection.item_id.clone());
            if selected_channel_id.as_deref() != Some(channel_id.as_str()) {
                let _ = select_home_and_channel(self, &channel_id);
            }
        } else if let Some(channels) = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Channels)
        {
            let selected = channels.items.iter().any(|item| item.selected);
            if !selected && channels.items.len() == 1 {
                self.send_harness_command(&HarnessUiCommand::SelectChannel {
                    channel_id: channels.items[0].id.clone(),
                })?;
            }
        }

        let previous_operation =
            observe_operation(&self.ui_snapshot()?, &OperationId::send_message());
        self.send_harness_command(&HarnessUiCommand::SendChatMessage {
            content: message.to_string(),
        })?;
        let handle = wait_for_operation_submission(
            self,
            OperationId::send_message(),
            previous_operation,
            Duration::from_secs(5),
        )?;
        let first_deadline = Instant::now() + Duration::from_millis(1500);
        loop {
            let snapshot = self.ui_snapshot()?;
            match snapshot.operation_state_for_instance(handle.id(), handle.instance_id()) {
                Some(OperationState::Failed) => {
                    anyhow::bail!("submit_send_chat_message: runtime send failed");
                }
                Some(OperationState::Succeeded) if message_visible(&snapshot, message) => {
                    return Ok(SubmittedAction::with_ui_operation((), handle));
                }
                Some(OperationState::Submitting) | Some(OperationState::Succeeded) => {
                    return Ok(SubmittedAction::with_ui_operation((), handle));
                }
                _ => {}
            }
            if Instant::now() >= first_deadline {
                break;
            }
            blocking_sleep(Duration::from_millis(80));
        }
        anyhow::bail!("submit_send_chat_message: send_message never reached a live submitted state")
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

    fn snapshot_with_channels(channel_ids: &[(&str, bool)]) -> UiSnapshot {
        UiSnapshot {
            screen: ScreenId::Chat,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: aura_app::ui_contract::ProjectionRevision {
                semantic_seq: 1,
                render_seq: Some(1),
            },
            quiescence: aura_app::ui_contract::QuiescenceSnapshot {
                state: aura_app::ui_contract::QuiescenceState::Settled,
                reason_codes: Vec::new(),
            },
            selections: channel_ids
                .iter()
                .find_map(|(id, selected)| {
                    selected.then(|| aura_app::ui_contract::SelectionSnapshot {
                        list: ListId::Channels,
                        item_id: (*id).to_string(),
                    })
                })
                .into_iter()
                .collect(),
            lists: vec![aura_app::ui_contract::ListSnapshot {
                id: ListId::Channels,
                items: channel_ids
                    .iter()
                    .map(|(id, selected)| aura_app::ui_contract::ListItemSnapshot {
                        id: (*id).to_string(),
                        selected: *selected,
                        confirmation: aura_app::ui_contract::ConfirmationState::PendingLocal,
                    })
                    .collect(),
            }],
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        }
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
            let screen = match backend.snapshot() {
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
    fn materialized_channel_binding_uses_unique_new_channel_id() {
        let snapshot = snapshot_with_channels(&[
            ("channel:note-to-self", false),
            ("channel:shared-parity-lab", true),
        ]);
        let previous = BTreeSet::from(["channel:note-to-self".to_string()]);
        let binding = materialized_channel_binding(&snapshot, &previous)
            .unwrap_or_else(|| panic!("expected new materialized channel binding"));
        assert_eq!(binding.channel_id, "channel:shared-parity-lab");
        assert_eq!(binding.context_id, None);
    }

    #[test]
    fn materialized_channel_binding_rejects_ambiguous_channel_delta() {
        let snapshot = snapshot_with_channels(&[
            ("channel:note-to-self", false),
            ("channel:shared-a", false),
            ("channel:shared-b", true),
        ]);
        let previous = BTreeSet::from(["channel:note-to-self".to_string()]);
        assert!(materialized_channel_binding(&snapshot, &previous).is_none());
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
        assert!(source.contains("self.send_harness_command(&HarnessUiCommand::CreateAccount {"));
        assert!(source.contains("fn submit_create_home"));
        assert!(source.contains("self.send_harness_command(&HarnessUiCommand::CreateHome {"));
        assert!(source.contains("fn submit_create_contact_invitation"));
        assert!(source
            .contains("self.send_harness_command(&HarnessUiCommand::CreateContactInvitation {"));
        assert!(
            source.contains("self.send_harness_command(&HarnessUiCommand::StartDeviceEnrollment {")
        );
        assert!(source
            .contains("self.send_harness_command(&HarnessUiCommand::ImportDeviceEnrollmentCode {"));
        assert!(
            source.contains("self.send_harness_command(&HarnessUiCommand::RemoveSelectedDevice)?;")
        );
        assert!(source.contains(
            "self.send_harness_command(&HarnessUiCommand::SwitchAuthority { authority_id })?;"
        ));
        assert!(
            source.contains("self.send_harness_command(&HarnessUiCommand::InviteActorToChannel {")
        );
        assert!(source.contains("self.send_harness_command(&HarnessUiCommand::SelectChannel {"));
        assert!(source.contains("self.send_harness_command(&HarnessUiCommand::SelectHome {"));
        assert!(invitation_branch
            .contains("self.send_harness_command(&HarnessUiCommand::DismissTransient)?;"));
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
        assert_eq!(
            send_branch.matches("HarnessUiCommand::SendChatMessage").count(),
            1,
            "send_chat_message must not retry by issuing the semantic command twice"
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
                && !ui_snapshot_body.contains("fs::read_to_string(&path)")
                && !production_source.contains("AURA_TUI_UI_STATE_FILE"),
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
        let screen = match backend.snapshot() {
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
        let screen = match backend.snapshot() {
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
        let screen = match backend.snapshot() {
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
