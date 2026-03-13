use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Condvar;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use aura_app::ui::contract::{
    ControlId, FieldId, ListId, ModalId, OperationId, OperationState, ScreenId, UiSnapshot,
};
use aura_app::ui_contract::RuntimeFact;
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::backend::{
    observe_operation, submit_accept_contact_invitation_via_shared_ui,
    submit_invite_actor_to_channel_via_shared_ui, wait_for_modal_visible,
    wait_for_operation_submission, wait_for_screen_visible, ContactInvitationCode, InstanceBackend,
    RawUiBackend, SharedSemanticBackend, SubmittedAction, UiSnapshotEvent,
};
use crate::config::InstanceConfig;
use crate::recovery_registry::{run_registered_recovery, RecoveryPath};
use crate::screen_normalization::{authoritative_screen, has_nav_header};
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
    backend.activate_control(ControlId::NavNeighborhood)?;
    wait_for_screen_visible(backend, ScreenId::Neighborhood, Duration::from_secs(5))?;
    backend.send_keys("\x1b")?;
    thread::sleep(Duration::from_millis(120));
    let home_deadline = Instant::now() + Duration::from_secs(15);
    let mut selected_home = false;
    loop {
        let snapshot = backend.ui_snapshot()?;
        let has_home = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Homes)
            .is_some_and(|list| list.items.iter().any(|item| item.id == channel_id));
        if has_home {
            match backend.activate_list_item(ListId::Homes, channel_id) {
                Ok(()) => {
                    selected_home = true;
                    backend.send_keys("\r")?;
                    thread::sleep(Duration::from_millis(250));
                }
                Err(error) => {
                    tracing::debug!(
                        "select_home_and_channel: home {channel_id} selection did not converge: {error}"
                    );
                }
            }
            break;
        }
        if Instant::now() >= home_deadline {
            break;
        }
        thread::sleep(Duration::from_millis(80));
    }
    backend.activate_control(ControlId::NavChat)?;
    wait_for_screen_visible(backend, ScreenId::Chat, Duration::from_secs(5))?;
    backend.activate_list_item(ListId::Channels, channel_id)?;
    backend.send_keys("\r")?;
    thread::sleep(Duration::from_millis(150));
    let committed = backend
        .ui_snapshot()?
        .selections
        .iter()
        .find(|selection| selection.list == ListId::Channels)
        .is_some_and(|selection| selection.item_id == channel_id);
    if !committed {
        backend.activate_list_item(ListId::Channels, channel_id)?;
        backend.send_keys("\r")?;
        thread::sleep(Duration::from_millis(150));
    }
    if !selected_home {
        tracing::debug!(
            "select_home_and_channel: home {channel_id} not visible, fell back to chat selection"
        );
    }
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
                guard.latest = Some(snapshot);
                feed.ready.notify_all();
            }
            let _ = fs::remove_file(socket_path);
        }))
    }

    fn submit_chat_command_via_ui(&mut self, command: &str) -> Result<()> {
        self.activate_control(ControlId::NavChat)?;
        wait_for_screen_visible(self, ScreenId::Chat, Duration::from_secs(5))?;
        self.fill_field(FieldId::ChatInput, &format!("/{command}"))?;
        self.send_key(crate::tool_api::ToolKey::Enter, 1)
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
            thread::sleep(Duration::from_millis(inter_key_delay_ms));
        }
        thread::sleep(Duration::from_millis(50));
        Ok(())
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
            thread::sleep(Duration::from_millis(SETTLE_DELAY_MS));
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
                thread::sleep(Duration::from_millis(HEADER_RECOVERY_DELAY_MS));
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
                let version = snapshot.revision.semantic_seq;
                if after_version.map_or(true, |required| version > required) {
                    return Some(Ok(UiSnapshotEvent { snapshot, version }));
                }
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
                thread::sleep(Duration::from_millis(40));
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
        let reader_alive = self
            .session
            .as_ref()
            .and_then(|session| session.reader_thread.as_ref())
            .map_or(true, |thread| !thread.is_finished());
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
                thread::sleep(Duration::from_millis(100));
            }
        }

        let deadline = Instant::now() + timeout;
        loop {
            if self.ui_snapshot().is_ok() {
                return Ok(());
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
                anyhow::bail!(
                    "local PTY instance {} did not reach readiness within {:?}",
                    self.config.id,
                    timeout
                );
            }
            thread::sleep(Duration::from_millis(100));
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
        match control_id {
            ControlId::SettingsAddDeviceButton | ControlId::SettingsRemoveDeviceButton => {
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
                thread::sleep(Duration::from_millis(50));
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
                    thread::sleep(Duration::from_millis(200));
                }
                self.type_text(value, 12)
            }
            FieldId::InvitationCode | FieldId::DeviceImportCode => self.type_text(value, 3),
            _ => self.type_text(value, 8),
        }
    }

    fn activate_list_item(&mut self, list_id: ListId, item_id: &str) -> Result<()> {
        let mut snapshot = self.ui_snapshot()?;
        if matches!(list_id, ListId::SettingsSections)
            && matches!(
                snapshot.focused_control,
                Some(ControlId::Screen(ScreenId::Settings))
            )
        {
            self.send_keys("\u{1b}[B")?;
            thread::sleep(Duration::from_millis(80));
            snapshot = self.ui_snapshot()?;
        }
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
        if matches!(list_id, ListId::Navigation) {
            let list_len = list.items.len();
            if list_len == 0 {
                return Ok(());
            }
            self.send_keys("\x1b")?;
            let forward_steps = (target_index + list_len - current_index) % list_len;
            for _ in 0..forward_steps {
                self.send_keys("\t")?;
            }
            return Ok(());
        }
        if matches!(list_id, ListId::SettingsSections) {
            let max_attempts = list.items.len().saturating_mul(2).max(1);
            for attempt in 0..max_attempts {
                let current_snapshot = self.ui_snapshot()?;
                let current_list = current_snapshot
                    .lists
                    .iter()
                    .find(|candidate| candidate.id == list_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "list {list_id:?} is not visible in the current TUI snapshot"
                        )
                    })?;
                let current_index = current_list
                    .items
                    .iter()
                    .position(|item| item.selected)
                    .unwrap_or(0);
                if current_list
                    .items
                    .get(current_index)
                    .map(|item| item.id.as_str() == item_id)
                    .unwrap_or(false)
                {
                    return Ok(());
                }
                let sequence = if current_index > target_index {
                    "\u{1b}[A"
                } else {
                    "\u{1b}[B"
                };
                eprintln!(
                    "[local_pty activate_list_item stepwise] instance={} list={:?} item_id={} attempt={} current_index={} target_index={} sequence={}",
                    self.config.id,
                    list_id,
                    item_id,
                    attempt,
                    current_index,
                    target_index,
                    sequence
                );
                self.send_keys(sequence)?;
                thread::sleep(Duration::from_millis(80));
            }
            let final_snapshot = self.ui_snapshot()?;
            let final_selected = final_snapshot
                .lists
                .iter()
                .find(|candidate| candidate.id == list_id)
                .and_then(|candidate| candidate.items.iter().find(|item| item.selected))
                .map(|item| item.id.clone())
                .unwrap_or_else(|| "<none>".to_string());
            anyhow::bail!(
                "failed to select item {item_id} in list {list_id:?}; final selection was {final_selected}"
            );
        }
        let delta = target_index as isize - current_index as isize;
        let sequence = if matches!(list_id, ListId::SettingsSections | ListId::Homes) {
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
            thread::sleep(Duration::from_millis(60));
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
            thread::sleep(Duration::from_millis(80));
        }
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

    fn submit_create_account(&mut self, account_name: &str) -> Result<SubmittedAction<()>> {
        self.fill_field(FieldId::AccountName, account_name)?;
        self.activate_control(ControlId::OnboardingCreateAccountButton)?;
        let issue_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = self.ui_snapshot()?;
            if snapshot.operations.iter().any(|operation| {
                operation.id == OperationId::account_create()
                    && operation.state == aura_app::ui::contract::OperationState::Failed
            }) {
                anyhow::bail!("submit_create_account: account creation failed");
            }
            if snapshot.operations.iter().any(|operation| {
                operation.id == OperationId::account_create()
                    && matches!(
                        operation.state,
                        aura_app::ui::contract::OperationState::Submitting
                            | aura_app::ui::contract::OperationState::Succeeded
                    )
            }) {
                return Ok(SubmittedAction::without_handle(()));
            }
            if snapshot.screen != ScreenId::Onboarding || snapshot_has_real_home(&snapshot) {
                return Ok(SubmittedAction::without_handle(()));
            }
            if Instant::now() >= issue_deadline {
                anyhow::bail!("submit_create_account: account creation did not issue");
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn submit_create_home(&mut self, home_name: &str) -> Result<SubmittedAction<()>> {
        self.activate_control(ControlId::NavNeighborhood)
            .context("submit_create_home: nav_neighborhood")?;
        wait_for_screen_visible(self, ScreenId::Neighborhood, Duration::from_secs(5))
            .context("submit_create_home: wait_neighborhood")?;
        let modal_open_deadline = Instant::now() + Duration::from_secs(5);
        let mut modal_open = false;
        while Instant::now() < modal_open_deadline {
            self.activate_control(ControlId::NeighborhoodNewHomeButton)
                .context("submit_create_home: open_create_home")?;
            if wait_for_modal_visible(self, ModalId::CreateHome, Duration::from_secs(2)).is_ok() {
                modal_open = true;
                break;
            }
            thread::sleep(Duration::from_millis(150));
        }
        anyhow::ensure!(
            modal_open,
            "submit_create_home: create_home_modal did not open"
        );
        thread::sleep(Duration::from_millis(200));
        self.fill_field(FieldId::HomeName, home_name)
            .context("submit_create_home: fill_home_name")?;
        thread::sleep(Duration::from_millis(150));
        for _ in 0..3 {
            self.send_keys("\r")
                .context("submit_create_home: submit_create_home")?;
            let modal_close_deadline = Instant::now() + Duration::from_millis(800);
            loop {
                let snapshot = self.ui_snapshot()?;
                if snapshot.open_modal != Some(ModalId::CreateHome) {
                    break;
                }
                if Instant::now() >= modal_close_deadline {
                    break;
                }
                thread::sleep(Duration::from_millis(80));
            }
            if self.ui_snapshot()?.open_modal != Some(ModalId::CreateHome) {
                break;
            }
        }
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
                self.activate_list_item(ListId::Homes, &home_id)?;
                return Ok(SubmittedAction::without_handle(()));
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(80));
        }
        anyhow::bail!("submit_create_home did not produce a non-placeholder home")
    }

    fn submit_create_contact_invitation(
        &mut self,
        receiver_authority_id: &str,
    ) -> Result<SubmittedAction<ContactInvitationCode>> {
        let previous_operation =
            observe_operation(&self.ui_snapshot()?, &OperationId::invitation_create());
        self.activate_control(ControlId::ContactsCreateInvitationButton)?;
        wait_for_modal_visible(self, ModalId::CreateInvitation, Duration::from_secs(5))?;
        self.activate_list_item(ListId::InvitationTypes, "contact")?;
        self.fill_field(FieldId::InvitationReceiver, receiver_authority_id)?;
        self.activate_control(ControlId::ModalConfirmButton)?;
        let handle = wait_for_operation_submission(
            self,
            OperationId::invitation_create(),
            previous_operation,
            Duration::from_secs(5),
        )?;
        wait_for_modal_visible(self, ModalId::InvitationCode, Duration::from_secs(5))?;
        self.activate_control(ControlId::ModalCancelButton)?;
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
            thread::sleep(Duration::from_millis(50));
        }
        Ok(SubmittedAction::with_ui_operation(
            ContactInvitationCode {
                code: String::new(),
            },
            handle,
        ))
    }

    fn submit_accept_contact_invitation(&mut self, code: &str) -> Result<SubmittedAction<()>> {
        submit_accept_contact_invitation_via_shared_ui(self, code)
    }

    fn submit_invite_actor_to_channel(
        &mut self,
        authority_id: &str,
    ) -> Result<SubmittedAction<()>> {
        submit_invite_actor_to_channel_via_shared_ui(self, authority_id)
    }

    fn submit_accept_pending_channel_invitation(&mut self) -> Result<SubmittedAction<()>> {
        let snapshot = self.ui_snapshot()?;
        let previous_channel_count = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Channels)
            .map(|list| list.items.len())
            .unwrap_or(0);
        let joined_channel_id =
            snapshot
                .runtime_events
                .iter()
                .find_map(|event| match &event.fact {
                    RuntimeFact::ChannelMembershipReady {
                        channel,
                        member_count: Some(member_count),
                        ..
                    } if *member_count > 1 => channel.id.clone(),
                    _ => None,
                });
        let joined_channel_name =
            snapshot
                .runtime_events
                .iter()
                .find_map(|event| match &event.fact {
                    RuntimeFact::ChannelMembershipReady {
                        channel,
                        member_count: Some(member_count),
                        ..
                    } if *member_count > 1 => channel.name.clone(),
                    _ => None,
                });
        let already_joined = previous_channel_count > 1 || joined_channel_id.is_some();
        if already_joined {
            if let Some(channel_id) = joined_channel_id {
                let selected_channel_id = snapshot
                    .selections
                    .iter()
                    .find(|selection| selection.list == ListId::Channels)
                    .map(|selection| selection.item_id.clone());
                if selected_channel_id.as_deref() != Some(channel_id.as_str()) {
                    select_home_and_channel(self, &channel_id)?;
                }
            }
            if let Some(channel_name) =
                joined_channel_name.filter(|name| !name.eq_ignore_ascii_case("note to self"))
            {
                let _ = self.submit_chat_command_via_ui(&format!("join {channel_name}"));
            }
            return Ok(SubmittedAction::without_handle(()));
        }
        self.submit_chat_command_via_ui("homeaccept")?;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let snapshot = self.ui_snapshot()?;
            let channel_count = snapshot
                .lists
                .iter()
                .find(|list| list.id == ListId::Channels)
                .map(|list| list.items.len())
                .unwrap_or(0);
            let joined_channel_id =
                snapshot
                    .runtime_events
                    .iter()
                    .find_map(|event| match &event.fact {
                        RuntimeFact::ChannelMembershipReady {
                            channel,
                            member_count: Some(member_count),
                            ..
                        } if *member_count > 1 => channel.id.clone(),
                        _ => None,
                    });
            let joined_channel_name = snapshot.runtime_events.iter().find_map(|event| match &event
                .fact
            {
                RuntimeFact::ChannelMembershipReady {
                    channel,
                    member_count: Some(member_count),
                    ..
                } if *member_count > 1 => channel.name.clone(),
                _ => None,
            });
            let joined = channel_count > previous_channel_count || joined_channel_id.is_some();
            if joined {
                if let Some(channel_id) = joined_channel_id {
                    let selected_channel_id = snapshot
                        .selections
                        .iter()
                        .find(|selection| selection.list == ListId::Channels)
                        .map(|selection| selection.item_id.clone());
                    if selected_channel_id.as_deref() != Some(channel_id.as_str()) {
                        select_home_and_channel(self, &channel_id)?;
                    }
                }
                if let Some(channel_name) =
                    joined_channel_name.filter(|name| !name.eq_ignore_ascii_case("note to self"))
                {
                    let _ = self.submit_chat_command_via_ui(&format!("join {channel_name}"));
                }
                return Ok(SubmittedAction::without_handle(()));
            }
            if snapshot.operation_state(&OperationId::invitation_accept())
                == Some(OperationState::Failed)
            {
                anyhow::bail!("submit_accept_pending_channel_invitation: invitation_accept failed");
            }
            if Instant::now() >= deadline {
                anyhow::bail!(
                    "submit_accept_pending_channel_invitation: timed out waiting for channel join"
                );
            }
            thread::sleep(Duration::from_millis(80));
        }
    }

    fn submit_join_channel(&mut self, channel_name: &str) -> Result<SubmittedAction<()>> {
        self.activate_control(ControlId::NavChat)
            .context("submit_join_channel: nav_chat")?;
        wait_for_screen_visible(self, ScreenId::Chat, Duration::from_secs(5))
            .context("submit_join_channel: wait_chat")?;
        self.submit_chat_command_via_ui(&format!("join {channel_name}"))
            .context("submit_join_channel: join_command")?;
        let joined_deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let snapshot = self.ui_snapshot()?;
            let joined_channel_id =
                snapshot
                    .runtime_events
                    .iter()
                    .find_map(|event| match &event.fact {
                        RuntimeFact::ChannelMembershipReady { channel, .. }
                            if channel
                                .name
                                .as_deref()
                                .map(|name: &str| name.eq_ignore_ascii_case(channel_name))
                                .unwrap_or(false) =>
                        {
                            channel.id.clone()
                        }
                        _ => None,
                    });
            let joined = joined_channel_id.is_some();
            if joined || Instant::now() >= joined_deadline {
                if joined {
                    if let Some(channel_id) = joined_channel_id {
                        let selected_channel_id = snapshot
                            .selections
                            .iter()
                            .find(|selection| selection.list == ListId::Channels)
                            .map(|selection| selection.item_id.clone());
                        if selected_channel_id.as_deref() != Some(channel_id.as_str()) {
                            select_home_and_channel(self, &channel_id)?;
                        }
                    }
                    return Ok(SubmittedAction::without_handle(()));
                }
                break;
            }
            thread::sleep(Duration::from_millis(80));
        }
        run_registered_recovery(RecoveryPath::LocalPtyJoinChannelSlashFallback, || {
            self.activate_control(ControlId::ChatNewGroupButton)
                .context("submit_join_channel: open_create_channel")?;
            wait_for_modal_visible(self, ModalId::CreateChannel, Duration::from_secs(2))
                .context("submit_join_channel: wait_create_channel")?;
            self.fill_field(FieldId::CreateChannelName, channel_name)
                .context("submit_join_channel: fill_channel_name")?;
            self.send_keys("\r")
                .context("submit_join_channel: advance_details")?;
            self.send_keys("\r")
                .context("submit_join_channel: advance_members")?;
            self.send_keys("\r")
                .context("submit_join_channel: submit_threshold")
        })?;
        Ok(SubmittedAction::without_handle(()))
    }

    fn submit_send_chat_message(&mut self, message: &str) -> Result<SubmittedAction<()>> {
        fn message_visible(snapshot: &UiSnapshot, expected: &str) -> bool {
            snapshot
                .messages
                .iter()
                .any(|message| message.content.contains(expected))
        }

        fn send_once(backend: &mut LocalPtyBackend, message: &str) -> Result<()> {
            backend.fill_field(FieldId::ChatInput, message)?;
            let input_deadline = Instant::now() + Duration::from_millis(1200);
            loop {
                if backend.snapshot()?.contains(message) {
                    break;
                }
                if Instant::now() >= input_deadline {
                    backend.send_keys("\x1b")?;
                    thread::sleep(Duration::from_millis(80));
                    backend.send_keys("i")?;
                    thread::sleep(Duration::from_millis(250));
                    backend.type_text(message, 20)?;
                    let retry_deadline = Instant::now() + Duration::from_millis(1200);
                    loop {
                        if backend.snapshot()?.contains(message) {
                            break;
                        }
                        if Instant::now() >= retry_deadline {
                            anyhow::bail!(
                                "submit_send_chat_message: message never appeared in chat input"
                            );
                        }
                        thread::sleep(Duration::from_millis(80));
                    }
                    break;
                }
                thread::sleep(Duration::from_millis(80));
            }
            backend.send_key(crate::tool_api::ToolKey::Enter, 1)?;
            Ok(())
        }

        self.activate_control(ControlId::NavChat)?;
        wait_for_screen_visible(self, ScreenId::Chat, Duration::from_secs(5))?;
        let previous_operation =
            observe_operation(&self.ui_snapshot()?, &OperationId::send_message());
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
        }
        if let Some(channels) = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Channels)
        {
            let selected = channels.items.iter().any(|item| item.selected);
            if !selected && channels.items.len() == 1 {
                self.send_keys("h")?;
                thread::sleep(Duration::from_millis(80));
                for sequence in ["k", "j"] {
                    self.send_keys(sequence)?;
                    thread::sleep(Duration::from_millis(80));
                    let updated = self.ui_snapshot()?;
                    let visible_selected = updated
                        .lists
                        .iter()
                        .find(|list| list.id == ListId::Channels)
                        .is_some_and(|list| list.items.iter().any(|item| item.selected));
                    if visible_selected {
                        break;
                    }
                }
            }
        }

        send_once(self, message)?;
        let handle = wait_for_operation_submission(
            self,
            OperationId::send_message(),
            previous_operation,
            Duration::from_secs(5),
        )?;
        let first_deadline = Instant::now() + Duration::from_millis(1500);
        loop {
            let snapshot = self.ui_snapshot()?;
            match snapshot.operation_state_for_instance(&handle.id, &handle.instance_id) {
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
            thread::sleep(Duration::from_millis(80));
        }

        send_once(self, message)?;
        let retry_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let snapshot = self.ui_snapshot()?;
            match snapshot.operation_state_for_instance(&handle.id, &handle.instance_id) {
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
            if Instant::now() >= retry_deadline {
                anyhow::bail!(
                    "submit_send_chat_message: send_message never reached a live submitted state"
                );
            }
            thread::sleep(Duration::from_millis(80));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::thread;
    use std::time::Duration;

    use super::*;
    use crate::config::InstanceMode;

    fn test_config() -> InstanceConfig {
        InstanceConfig {
            id: "local-test".to_string(),
            mode: InstanceMode::Local,
            data_dir: std::env::temp_dir().join("aura-harness-local-test"),
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
        const POLL_INTERVAL_MS: u128 = 30;
        let max_attempts_u128 = timeout
            .as_millis()
            .max(POLL_INTERVAL_MS)
            .div_ceil(POLL_INTERVAL_MS);
        let max_attempts = usize::try_from(max_attempts_u128).unwrap_or(usize::MAX);

        for attempt in 0..max_attempts {
            let screen = match backend.snapshot() {
                Ok(screen) => screen,
                Err(error) => panic!("snapshot failed: {error}"),
            };
            if screen.contains(needle) {
                return screen;
            }
            if attempt + 1 == max_attempts {
                panic!("timed out waiting for screen to contain {needle:?}; got: {screen:?}");
            }
            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS as u64));
        }

        panic!("wait_for_screen_contains reached unreachable state");
    }

    #[test]
    fn local_backend_injects_default_clipboard_isolation_env() {
        let mut config = test_config();
        config.id = "local-test-clipboard-default".to_string();
        config.data_dir = std::env::temp_dir().join("aura-harness-local-clipboard-default");
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
    fn local_shared_intent_methods_drive_visible_tui_controls() {
        let source = include_str!("local_pty.rs");
        assert!(source.contains("fn submit_create_account"));
        assert!(source.contains("self.fill_field(FieldId::AccountName, account_name)?;"));
        assert!(
            source.contains("self.activate_control(ControlId::OnboardingCreateAccountButton)?;")
        );
        assert!(source.contains("fn submit_create_contact_invitation"));
        assert!(
            source.contains("self.activate_control(ControlId::ContactsCreateInvitationButton)?;")
        );
        assert!(source.contains(
            "wait_for_modal_visible(self, ModalId::CreateInvitation, Duration::from_secs(5))?;"
        ));
    }

    #[test]
    fn local_backend_default_command_targets_aura_tui() {
        let mut config = test_config();
        config.command = None;
        config.args.clear();
        config.data_dir = std::env::temp_dir().join("aura-harness-local-default-cmd");
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
        config.data_dir = std::env::temp_dir().join("aura-harness-local-clipboard-override");
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
        config.data_dir = std::env::temp_dir().join("aura-harness-local-missing-ui-snapshot");

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
        thread::sleep(Duration::from_millis(80));
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
        thread::sleep(Duration::from_millis(50));
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
        let temp_root = std::env::temp_dir().join("aura-harness-tail-log-dat");
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
