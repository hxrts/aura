use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use aura_app::ui::contract::{
    ControlId, FieldId, ListId, ModalId, OperationId, ScreenId, UiReadiness, UiSnapshot,
};
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::backend::{
    observe_operation, wait_for_modal_visible, wait_for_operation_submission,
    wait_for_screen_visible, ContactInvitationCode, InstanceBackend, SharedSemanticBackend,
    SubmittedAction, UiSnapshotEvent,
};
use crate::config::InstanceConfig;
use crate::screen_normalization::{authoritative_screen, has_nav_header};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BackendState {
    Stopped,
    Running,
}

struct RunningSession {
    child: Box<dyn Child + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<vt100::Parser>>,
    parse_generation: Arc<AtomicU64>,
    reader_thread: Option<thread::JoinHandle<()>>,
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
    fn synthetic_onboarding_snapshot() -> UiSnapshot {
        UiSnapshot {
            screen: ScreenId::Onboarding,
            focused_control: Some(ControlId::OnboardingRoot),
            open_modal: None,
            readiness: UiReadiness::Loading,
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        }
    }

    fn is_cargo_program(program: &str) -> bool {
        std::path::Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name == "cargo")
            .unwrap_or(false)
    }

    fn ui_state_file(&self) -> PathBuf {
        self.config.data_dir.join(".ui-state.json")
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
        if Self::env_value("AURA_TUI_UI_STATE_FILE", &self.config.env).is_none() {
            let ui_state_file = Self::absolutize_path(self.ui_state_file());
            command.env(
                "AURA_TUI_UI_STATE_FILE",
                ui_state_file.to_string_lossy().to_string(),
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
        let _ = fs::remove_file(self.ui_state_file());

        let child = pair
            .slave
            .spawn_command(command)
            .with_context(|| format!("failed to spawn process for {}", self.config.id))?;
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
        });
        self.state = BackendState::Running;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if self.state == BackendState::Stopped {
            return Ok(());
        }

        if let Some(mut session) = self.session.take() {
            let _ = session.child.kill();
            let _ = session.child.wait();
            drop(session.writer);
            if let Some(handle) = session.reader_thread.take() {
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
        let path = self.ui_state_file();
        const SNAPSHOT_WAIT_ATTEMPTS: usize = 50;
        const SNAPSHOT_WAIT_DELAY_MS: u64 = 100;

        let mut last_error = None;
        for _ in 0..SNAPSHOT_WAIT_ATTEMPTS {
            match fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(snapshot) => return Ok(snapshot),
                    Err(error) => last_error = Some(format!("parse error: {error}")),
                },
                Err(error) => last_error = Some(format!("read error: {error}")),
            }
            thread::sleep(Duration::from_millis(SNAPSHOT_WAIT_DELAY_MS));
        }

        let detail = last_error.unwrap_or_else(|| "no snapshot produced".to_string());
        if detail.contains("No such file or directory") {
            return Ok(Self::synthetic_onboarding_snapshot());
        }
        anyhow::bail!(
            "failed to read TUI UI snapshot {} after {} attempts: {}",
            path.display(),
            SNAPSHOT_WAIT_ATTEMPTS,
            detail
        )
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
            if snapshot.open_modal == Some(ModalId::CreateInvitation) {
                return self.send_keys("\r");
            }
        }
        let sequence = control_id.activation_key().ok_or_else(|| {
            anyhow::anyhow!("control {control_id:?} does not have a PTY activation mapping")
        })?;
        self.send_keys(sequence)
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
                    thread::sleep(Duration::from_millis(120));
                }
                self.type_text(value, 4)
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
            // Normalize back to command/navigation mode before cycling tabs.
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
        let sequence = if matches!(list_id, ListId::SettingsSections) {
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
        Ok(())
    }

    fn create_account_via_ui(&mut self, account_name: &str) -> Result<SubmittedAction<()>> {
        self.fill_field(FieldId::AccountName, account_name)?;
        self.activate_control(ControlId::OnboardingCreateAccountButton)?;
        let onboarding_exit_deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let snapshot = self.ui_snapshot()?;
            if snapshot.screen != ScreenId::Onboarding {
                break;
            }
            if Instant::now() >= onboarding_exit_deadline {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        Ok(SubmittedAction::without_handle(()))
    }

    fn create_contact_invitation(&mut self, receiver_authority_id: &str) -> Result<String> {
        Ok(self
            .create_contact_invitation_via_ui(receiver_authority_id)?
            .value
            .code)
    }

    fn create_contact_invitation_via_ui(
        &mut self,
        receiver_authority_id: &str,
    ) -> Result<SubmittedAction<ContactInvitationCode>> {
        let previous_operation =
            observe_operation(&self.ui_snapshot()?, &OperationId::invitation_create());
        self.activate_control(ControlId::ContactsCreateInvitationButton)?;
        wait_for_modal_visible(self, ModalId::CreateInvitation, Duration::from_secs(5))?;
        self.fill_field(FieldId::InvitationReceiver, receiver_authority_id)?;
        self.activate_control(ControlId::ModalConfirmButton)?;
        let handle = wait_for_operation_submission(
            self,
            OperationId::invitation_create(),
            previous_operation,
            Duration::from_secs(5),
        )?;
        Ok(SubmittedAction::with_ui_operation(
            ContactInvitationCode {
                code: self.read_clipboard()?,
            },
            handle,
        ))
    }

    fn accept_contact_invitation_via_ui(&mut self, code: &str) -> Result<SubmittedAction<()>> {
        let previous_operation =
            observe_operation(&self.ui_snapshot()?, &OperationId::invitation_accept());
        self.activate_control(ControlId::ContactsAcceptInvitationButton)?;
        wait_for_modal_visible(self, ModalId::AcceptInvitation, Duration::from_secs(5))?;
        self.fill_field(FieldId::InvitationCode, code)?;
        self.activate_control(ControlId::ModalConfirmButton)?;
        let handle = wait_for_operation_submission(
            self,
            OperationId::invitation_accept(),
            previous_operation,
            Duration::from_secs(5),
        )?;
        Ok(SubmittedAction::with_ui_operation((), handle))
    }

    fn invite_actor_to_channel_via_ui(
        &mut self,
        authority_id: &str,
    ) -> Result<SubmittedAction<()>> {
        self.activate_control(ControlId::NavContacts)?;
        wait_for_screen_visible(self, ScreenId::Contacts, Duration::from_secs(5))?;
        self.activate_list_item(ListId::Contacts, authority_id)?;
        self.activate_control(ControlId::ContactsInviteToChannelButton)?;
        Ok(SubmittedAction::without_handle(()))
    }

    fn accept_pending_channel_invitation_via_ui(&mut self) -> Result<SubmittedAction<()>> {
        self.submit_chat_command_via_ui("homeaccept")?;
        Ok(SubmittedAction::without_handle(()))
    }

    fn join_channel_via_ui(&mut self, channel_name: &str) -> Result<SubmittedAction<()>> {
        self.activate_control(ControlId::NavChat)?;
        wait_for_screen_visible(self, ScreenId::Chat, Duration::from_secs(5))?;
        self.send_keys("n")?;
        wait_for_modal_visible(self, ModalId::CreateChannel, Duration::from_secs(5))?;
        self.fill_field(FieldId::CreateChannelName, channel_name)?;
        self.activate_control(ControlId::ModalConfirmButton)?;
        let members_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = self.ui_snapshot()?;
            let advanced = snapshot.open_modal == Some(ModalId::CreateChannel)
                && !matches!(
                    snapshot.focused_control,
                    Some(ControlId::Field(FieldId::CreateChannelName))
                        | Some(ControlId::Field(FieldId::CreateChannelTopic))
                );
            if advanced || Instant::now() >= members_deadline {
                break;
            }
            thread::sleep(Duration::from_millis(80));
        }
        self.activate_control(ControlId::ModalConfirmButton)?;
        let threshold_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = self.ui_snapshot()?;
            if snapshot.focused_control == Some(ControlId::Field(FieldId::ThresholdInput))
                || Instant::now() >= threshold_deadline
            {
                break;
            }
            thread::sleep(Duration::from_millis(80));
        }
        self.activate_control(ControlId::ModalConfirmButton)?;
        Ok(SubmittedAction::without_handle(()))
    }

    fn send_chat_message_via_ui(&mut self, message: &str) -> Result<SubmittedAction<()>> {
        self.activate_control(ControlId::NavChat)?;
        wait_for_screen_visible(self, ScreenId::Chat, Duration::from_secs(5))?;
        self.fill_field(FieldId::ChatInput, message)?;
        self.send_key(crate::tool_api::ToolKey::Enter, 1)?;
        Ok(SubmittedAction::without_handle(()))
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

    fn read_clipboard(&mut self) -> Result<String> {
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
        if self
            .ui_snapshot()
            .ok()
            .and_then(|snapshot| snapshot.open_modal)
            == Some(ModalId::InvitationCode)
        {
            self.send_keys("\r")?;
            thread::sleep(Duration::from_millis(40));
        }
        Ok(text)
    }

    fn health_check(&self) -> Result<bool> {
        Ok(self.state == BackendState::Running && self.session.is_some())
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
            if let Ok(screen) = self.snapshot() {
                if !screen.trim().is_empty() {
                    return Ok(());
                }
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

impl SharedSemanticBackend for LocalPtyBackend {
    fn shared_projection(&self) -> Result<UiSnapshot> {
        InstanceBackend::ui_snapshot(self)
    }

    fn wait_for_shared_projection_event(
        &self,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> Option<Result<UiSnapshotEvent>> {
        InstanceBackend::wait_for_ui_snapshot_event(self, timeout, after_version)
    }

    fn submit_create_account(&mut self, account_name: &str) -> Result<SubmittedAction<()>> {
        InstanceBackend::create_account_via_ui(self, account_name)
    }

    fn submit_create_contact_invitation(
        &mut self,
        receiver_authority_id: &str,
    ) -> Result<SubmittedAction<ContactInvitationCode>> {
        InstanceBackend::create_contact_invitation_via_ui(self, receiver_authority_id)
    }

    fn submit_accept_contact_invitation(&mut self, code: &str) -> Result<SubmittedAction<()>> {
        InstanceBackend::accept_contact_invitation_via_ui(self, code)
    }

    fn submit_invite_actor_to_channel(
        &mut self,
        authority_id: &str,
    ) -> Result<SubmittedAction<()>> {
        InstanceBackend::invite_actor_to_channel_via_ui(self, authority_id)
    }

    fn submit_accept_pending_channel_invitation(&mut self) -> Result<SubmittedAction<()>> {
        InstanceBackend::accept_pending_channel_invitation_via_ui(self)
    }

    fn submit_join_channel(&mut self, channel_name: &str) -> Result<SubmittedAction<()>> {
        InstanceBackend::join_channel_via_ui(self, channel_name)
    }

    fn submit_send_chat_message(&mut self, message: &str) -> Result<SubmittedAction<()>> {
        InstanceBackend::send_chat_message_via_ui(self, message)
    }
}

impl Drop for LocalPtyBackend {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
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
