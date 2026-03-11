pub mod local_pty;
pub mod playwright_browser;
pub mod ssh_tunnel;

use anyhow::{bail, Result};
use aura_app::ui::contract::{
    ControlId, FieldId, ListId, ModalId, OperationId, OperationInstanceId, OperationState,
    ScreenId, UiSnapshot,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::config::{InstanceConfig, InstanceMode};
use crate::tool_api::ToolKey;

#[derive(Debug, Clone)]
pub struct UiSnapshotEvent {
    pub snapshot: UiSnapshot,
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactInvitationCode {
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiOperationHandle {
    pub id: OperationId,
    pub instance_id: OperationInstanceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SemanticSubmissionHandle {
    pub ui_operation: Option<UiOperationHandle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionState {
    Accepted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedAction<T> {
    pub value: T,
    pub submission: SubmissionState,
    pub handle: SemanticSubmissionHandle,
}

impl<T> SubmittedAction<T> {
    #[must_use]
    pub fn without_handle(value: T) -> Self {
        Self {
            value,
            submission: SubmissionState::Accepted,
            handle: SemanticSubmissionHandle::default(),
        }
    }

    #[must_use]
    pub fn with_ui_operation(value: T, handle: UiOperationHandle) -> Self {
        Self {
            value,
            submission: SubmissionState::Accepted,
            handle: SemanticSubmissionHandle {
                ui_operation: Some(handle),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedOperation {
    pub instance_id: OperationInstanceId,
    pub state: OperationState,
}

pub trait ObservationBackend {
    fn snapshot(&self) -> Result<String>;
    fn snapshot_dom(&self) -> Result<String>;
    fn ui_snapshot(&self) -> Result<UiSnapshot>;
    fn wait_for_ui_snapshot_event(
        &self,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> Option<Result<UiSnapshotEvent>>;
    fn wait_for_dom_patterns(
        &self,
        patterns: &[String],
        timeout_ms: u64,
    ) -> Option<Result<String>>;
    fn wait_for_target(&self, selector: &str, timeout_ms: u64) -> Option<Result<String>>;
    fn tail_log(&self, lines: usize) -> Result<Vec<String>>;
    fn read_clipboard(&self) -> Result<String>;
}

pub trait InstanceBackend {
    fn id(&self) -> &str;
    fn backend_kind(&self) -> &'static str;
    fn supports_ui_snapshot(&self) -> bool {
        false
    }
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn snapshot(&self) -> Result<String>;
    fn snapshot_dom(&self) -> Result<String> {
        self.snapshot()
    }
    fn ui_snapshot(&self) -> Result<UiSnapshot> {
        bail!(
            "structured UI snapshots are not supported by backend {}",
            self.backend_kind()
        )
    }
    fn wait_for_ui_snapshot_event(
        &self,
        _timeout: Duration,
        _after_version: Option<u64>,
    ) -> Option<Result<UiSnapshotEvent>> {
        None
    }
    fn wait_for_dom_patterns(
        &self,
        _patterns: &[String],
        _timeout_ms: u64,
    ) -> Option<Result<String>> {
        None
    }
    fn wait_for_target(&self, _selector: &str, _timeout_ms: u64) -> Option<Result<String>> {
        None
    }
    fn send_keys(&mut self, keys: &str) -> Result<()>;
    fn send_key(&mut self, key: ToolKey, repeat: u16) -> Result<()> {
        let sequence = tool_key_sequence(key);
        let repeat = repeat.max(1);
        for _ in 0..repeat {
            self.send_keys(sequence)?;
        }
        Ok(())
    }
    fn click_button(&mut self, _label: &str) -> Result<()> {
        bail!(
            "button clicks are not supported by backend {}",
            self.backend_kind()
        )
    }
    fn activate_control(&mut self, control_id: ControlId) -> Result<()> {
        let _ = control_id;
        bail!(
            "semantic control activation is not supported by backend {}",
            self.backend_kind()
        )
    }
    fn click_target(&mut self, selector: &str) -> Result<()> {
        let _ = selector;
        bail!(
            "selector clicks are not supported by backend {}",
            self.backend_kind()
        )
    }
    fn fill_input(&mut self, selector: &str, value: &str) -> Result<()> {
        let _ = (selector, value);
        bail!(
            "input filling is not supported by backend {}",
            self.backend_kind()
        )
    }
    fn fill_field(&mut self, field_id: FieldId, value: &str) -> Result<()> {
        let _ = (field_id, value);
        bail!(
            "semantic field filling is not supported by backend {}",
            self.backend_kind()
        )
    }
    fn activate_list_item(&mut self, list_id: ListId, item_id: &str) -> Result<()> {
        let _ = (list_id, item_id);
        bail!(
            "semantic list activation is not supported by backend {}",
            self.backend_kind()
        )
    }
    fn create_contact_invitation(&mut self, receiver_authority_id: &str) -> Result<String> {
        let _ = receiver_authority_id;
        bail!(
            "semantic contact invitation creation is not supported by backend {}",
            self.backend_kind()
        )
    }
    fn create_account_via_ui(&mut self, account_name: &str) -> Result<SubmittedAction<()>> {
        let _ = account_name;
        bail!(
            "semantic create_account is not supported by backend {}",
            self.backend_kind()
        )
    }
    fn create_home_via_ui(&mut self, home_name: &str) -> Result<SubmittedAction<()>> {
        let _ = home_name;
        bail!(
            "semantic create_home is not supported by backend {}",
            self.backend_kind()
        )
    }
    fn create_contact_invitation_via_ui(
        &mut self,
        receiver_authority_id: &str,
    ) -> Result<SubmittedAction<ContactInvitationCode>> {
        let code = self.create_contact_invitation(receiver_authority_id)?;
        Ok(SubmittedAction::without_handle(ContactInvitationCode {
            code,
        }))
    }
    fn accept_contact_invitation_via_ui(&mut self, code: &str) -> Result<SubmittedAction<()>> {
        let _ = code;
        bail!(
            "semantic accept_contact_invitation is not supported by backend {}",
            self.backend_kind()
        )
    }
    fn invite_actor_to_channel_via_ui(
        &mut self,
        authority_id: &str,
    ) -> Result<SubmittedAction<()>> {
        let _ = authority_id;
        bail!(
            "semantic invite_actor_to_channel is not supported by backend {}",
            self.backend_kind()
        )
    }
    fn accept_pending_channel_invitation_via_ui(&mut self) -> Result<SubmittedAction<()>> {
        bail!(
            "semantic accept_pending_channel_invitation is not supported by backend {}",
            self.backend_kind()
        )
    }
    fn join_channel_via_ui(&mut self, channel_name: &str) -> Result<SubmittedAction<()>> {
        let _ = channel_name;
        bail!(
            "semantic join_channel is not supported by backend {}",
            self.backend_kind()
        )
    }
    fn send_chat_message_via_ui(&mut self, message: &str) -> Result<SubmittedAction<()>> {
        let _ = message;
        bail!(
            "semantic send_chat_message is not supported by backend {}",
            self.backend_kind()
        )
    }
    fn tail_log(&self, lines: usize) -> Result<Vec<String>>;
    fn inject_message(&mut self, _message: &str) -> Result<()> {
        Ok(())
    }
    fn read_clipboard(&self) -> Result<String> {
        bail!(
            "clipboard reads are not supported by backend {}",
            self.backend_kind()
        )
    }
    fn authority_id(&mut self) -> Result<Option<String>> {
        Ok(None)
    }
    fn health_check(&self) -> Result<bool> {
        Ok(self.is_healthy())
    }
    fn wait_until_ready(&self, _timeout: Duration) -> Result<()> {
        Ok(())
    }
    fn restart(&mut self) -> Result<()> {
        self.stop()?;
        self.start()
    }
    fn is_healthy(&self) -> bool;
}

impl<T: InstanceBackend + ?Sized> ObservationBackend for T {
    fn snapshot(&self) -> Result<String> {
        InstanceBackend::snapshot(self)
    }

    fn snapshot_dom(&self) -> Result<String> {
        InstanceBackend::snapshot_dom(self)
    }

    fn ui_snapshot(&self) -> Result<UiSnapshot> {
        InstanceBackend::ui_snapshot(self)
    }

    fn wait_for_ui_snapshot_event(
        &self,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> Option<Result<UiSnapshotEvent>> {
        InstanceBackend::wait_for_ui_snapshot_event(self, timeout, after_version)
    }

    fn wait_for_dom_patterns(
        &self,
        patterns: &[String],
        timeout_ms: u64,
    ) -> Option<Result<String>> {
        InstanceBackend::wait_for_dom_patterns(self, patterns, timeout_ms)
    }

    fn wait_for_target(&self, selector: &str, timeout_ms: u64) -> Option<Result<String>> {
        InstanceBackend::wait_for_target(self, selector, timeout_ms)
    }

    fn tail_log(&self, lines: usize) -> Result<Vec<String>> {
        InstanceBackend::tail_log(self, lines)
    }

    fn read_clipboard(&self) -> Result<String> {
        InstanceBackend::read_clipboard(self)
    }
}

pub trait SharedSemanticBackend {
    fn shared_projection(&self) -> Result<UiSnapshot>;
    fn wait_for_shared_projection_event(
        &self,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> Option<Result<UiSnapshotEvent>>;
    fn submit_create_account(&mut self, account_name: &str) -> Result<SubmittedAction<()>>;
    fn submit_create_home(&mut self, home_name: &str) -> Result<SubmittedAction<()>>;
    fn submit_create_contact_invitation(
        &mut self,
        receiver_authority_id: &str,
    ) -> Result<SubmittedAction<ContactInvitationCode>>;
    fn submit_accept_contact_invitation(&mut self, code: &str) -> Result<SubmittedAction<()>>;
    fn submit_invite_actor_to_channel(&mut self, authority_id: &str)
        -> Result<SubmittedAction<()>>;
    fn submit_accept_pending_channel_invitation(&mut self) -> Result<SubmittedAction<()>>;
    fn submit_join_channel(&mut self, channel_name: &str) -> Result<SubmittedAction<()>>;
    fn submit_send_chat_message(&mut self, message: &str) -> Result<SubmittedAction<()>>;
}

pub(crate) fn wait_for_modal_visible(
    backend: &dyn InstanceBackend,
    modal_id: ModalId,
    timeout: Duration,
) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if backend.ui_snapshot()?.open_modal == Some(modal_id) {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            bail!("timed out waiting for modal {modal_id:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(crate) fn wait_for_screen_visible(
    backend: &dyn InstanceBackend,
    screen_id: ScreenId,
    timeout: Duration,
) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if backend.ui_snapshot()?.screen == screen_id {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            bail!("timed out waiting for screen {screen_id:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[must_use]
pub(crate) fn observe_operation(
    snapshot: &UiSnapshot,
    operation_id: &OperationId,
) -> Option<ObservedOperation> {
    snapshot
        .operations
        .iter()
        .find(|operation| &operation.id == operation_id)
        .map(|operation| ObservedOperation {
            instance_id: operation.instance_id.clone(),
            state: operation.state,
        })
}

pub(crate) fn wait_for_operation_submission(
    backend: &dyn InstanceBackend,
    operation_id: OperationId,
    previous: Option<ObservedOperation>,
    timeout: Duration,
) -> Result<UiOperationHandle> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let snapshot = backend.ui_snapshot()?;
        if let Some(current) = observe_operation(&snapshot, &operation_id) {
            let changed = previous.as_ref().map_or(true, |previous| {
                current.instance_id != previous.instance_id || current.state != previous.state
            });
            if changed {
                return Ok(UiOperationHandle {
                    id: operation_id,
                    instance_id: current.instance_id,
                });
            }
        }
        if std::time::Instant::now() >= deadline {
            bail!(
                "timed out waiting for operation submission {:?}",
                operation_id
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn tool_key_sequence(key: ToolKey) -> &'static str {
    match key {
        ToolKey::Enter => "\r",
        ToolKey::Esc => "\x1b",
        ToolKey::Tab => "\t",
        ToolKey::BackTab => "\x1b[Z",
        ToolKey::Up => "\x1b[A",
        ToolKey::Down => "\x1b[B",
        ToolKey::Right => "\x1b[C",
        ToolKey::Left => "\x1b[D",
        ToolKey::Home => "\x1b[H",
        ToolKey::End => "\x1b[F",
        ToolKey::PageUp => "\x1b[5~",
        ToolKey::PageDown => "\x1b[6~",
        ToolKey::Backspace => "\x7f",
        ToolKey::Delete => "\x1b[3~",
    }
}

pub enum BackendHandle {
    Local(local_pty::LocalPtyBackend),
    Browser(Box<playwright_browser::PlaywrightBrowserBackend>),
    Ssh(ssh_tunnel::SshTunnelBackend),
}

impl BackendHandle {
    pub fn from_config(
        config: InstanceConfig,
        pty_rows: Option<u16>,
        pty_cols: Option<u16>,
    ) -> Result<Self> {
        match config.mode {
            InstanceMode::Local => Ok(Self::Local(local_pty::LocalPtyBackend::new(
                config, pty_rows, pty_cols,
            ))),
            InstanceMode::Browser => Ok(Self::Browser(Box::new(
                playwright_browser::PlaywrightBrowserBackend::new(config)?,
            ))),
            InstanceMode::Ssh => Ok(Self::Ssh(ssh_tunnel::SshTunnelBackend::new(config))),
        }
    }

    pub fn as_trait_mut(&mut self) -> &mut dyn InstanceBackend {
        match self {
            Self::Local(backend) => backend,
            Self::Browser(backend) => backend.as_mut(),
            Self::Ssh(backend) => backend,
        }
    }

    pub fn as_trait(&self) -> &dyn InstanceBackend {
        match self {
            Self::Local(backend) => backend,
            Self::Browser(backend) => backend.as_ref(),
            Self::Ssh(backend) => backend,
        }
    }

    pub fn as_shared_semantic_mut(&mut self) -> Result<&mut dyn SharedSemanticBackend> {
        match self {
            Self::Local(backend) => Ok(backend),
            Self::Browser(backend) => Ok(backend.as_mut()),
            Self::Ssh(backend) => bail!(
                "backend {} does not implement the shared semantic adapter contract",
                backend.backend_kind()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BackendHandle, InstanceBackend, ObservationBackend, UiSnapshotEvent};
    use crate::config::{InstanceConfig, InstanceMode};
    use anyhow::Result;
    use aura_app::ui::contract::{ScreenId, UiReadiness, UiSnapshot};
    use aura_app::ui_contract::{ProjectionRevision, QuiescenceSnapshot};
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn backend_handle_constructs_browser_variant() -> Result<()> {
        let config = InstanceConfig {
            id: "alice".to_string(),
            mode: InstanceMode::Browser,
            data_dir: PathBuf::from(".tmp/harness/browser-alice"),
            device_id: None,
            bind_address: "127.0.0.1:47001".to_string(),
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
        };

        let backend = BackendHandle::from_config(config, Some(40), Some(120))?;
        match backend {
            BackendHandle::Browser(_) => {}
            _ => panic!("expected browser backend"),
        }
        Ok(())
    }

    struct ReadOnlyBackend {
        mutation_calls: Cell<u32>,
    }

    impl ReadOnlyBackend {
        fn new() -> Self {
            Self {
                mutation_calls: Cell::new(0),
            }
        }

        fn snapshot_value() -> UiSnapshot {
            UiSnapshot {
                screen: ScreenId::Chat,
                focused_control: None,
                open_modal: None,
                readiness: UiReadiness::Ready,
                revision: ProjectionRevision {
                    semantic_seq: 7,
                    render_seq: Some(7),
                },
                quiescence: QuiescenceSnapshot::settled(),
                selections: Vec::new(),
                lists: Vec::new(),
                messages: Vec::new(),
                operations: Vec::new(),
                toasts: Vec::new(),
                runtime_events: Vec::new(),
            }
        }
    }

    impl InstanceBackend for ReadOnlyBackend {
        fn id(&self) -> &str {
            "read-only"
        }

        fn backend_kind(&self) -> &'static str {
            "test"
        }

        fn supports_ui_snapshot(&self) -> bool {
            true
        }

        fn start(&mut self) -> Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> Result<()> {
            Ok(())
        }

        fn snapshot(&self) -> Result<String> {
            Ok("snapshot".to_string())
        }

        fn snapshot_dom(&self) -> Result<String> {
            Ok("dom".to_string())
        }

        fn ui_snapshot(&self) -> Result<UiSnapshot> {
            Ok(Self::snapshot_value())
        }

        fn wait_for_ui_snapshot_event(
            &self,
            _timeout: Duration,
            after_version: Option<u64>,
        ) -> Option<Result<UiSnapshotEvent>> {
            Some(Ok(UiSnapshotEvent {
                snapshot: Self::snapshot_value(),
                version: after_version.unwrap_or(0) + 1,
            }))
        }

        fn wait_for_dom_patterns(
            &self,
            _patterns: &[String],
            _timeout_ms: u64,
        ) -> Option<Result<String>> {
            Some(Ok("dom-match".to_string()))
        }

        fn wait_for_target(&self, _selector: &str, _timeout_ms: u64) -> Option<Result<String>> {
            Some(Ok("target-match".to_string()))
        }

        fn send_keys(&mut self, _keys: &str) -> Result<()> {
            self.mutation_calls.set(self.mutation_calls.get() + 1);
            Ok(())
        }

        fn tail_log(&self, _lines: usize) -> Result<Vec<String>> {
            Ok(vec!["log".to_string()])
        }

        fn is_healthy(&self) -> bool {
            true
        }

        fn read_clipboard(&self) -> Result<String> {
            Ok("clipboard".to_string())
        }
    }

    #[test]
    fn observation_endpoints_are_side_effect_free() -> Result<()> {
        let backend = ReadOnlyBackend::new();
        let observer: &dyn ObservationBackend = &backend;

        assert_eq!(observer.snapshot()?, "snapshot");
        assert_eq!(observer.snapshot()?, "snapshot");
        assert_eq!(observer.snapshot_dom()?, "dom");
        assert_eq!(observer.ui_snapshot()?.revision.semantic_seq, 7);
        assert_eq!(observer.ui_snapshot()?.revision.semantic_seq, 7);
        assert_eq!(
            observer
                .wait_for_ui_snapshot_event(Duration::from_millis(1), Some(7))
                .expect("event should be present")?
                .version,
            8
        );
        assert_eq!(
            observer
                .wait_for_dom_patterns(&["chat".to_string()], 1)
                .expect("dom result should be present")?,
            "dom-match"
        );
        assert_eq!(
            observer
                .wait_for_target("#aura-screen-chat", 1)
                .expect("target result should be present")?,
            "target-match"
        );
        assert_eq!(observer.tail_log(1)?, vec!["log".to_string()]);
        assert_eq!(observer.read_clipboard()?, "clipboard");
        assert_eq!(backend.mutation_calls.get(), 0);
        Ok(())
    }
}
