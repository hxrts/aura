pub mod local_pty;
pub mod playwright_browser;
pub mod ssh_tunnel;

use crate::config::{InstanceConfig, InstanceMode};
use crate::tool_api::ToolKey;
use anyhow::{anyhow, bail, Result};
use aura_app::scenario_contract::IntentAction;
pub use aura_app::scenario_contract::{
    SemanticCommandRequest, SemanticCommandResponse, SemanticCommandValue, SubmissionState,
    SubmittedAction, UiOperationHandle,
};
use aura_app::ui::contract::{
    ControlId, FieldId, ListId, OperationId, OperationInstanceId, OperationState, UiSnapshot,
};
use aura_app::ui_contract::ProjectionRevision;
use std::time::Duration;

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
pub struct ChannelBinding {
    pub channel_id: String,
    pub context_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedOperation {
    pub instance_id: OperationInstanceId,
    pub state: OperationState,
}

pub enum DiagnosticObservationProbe<'a> {
    DomPatterns(&'a [String]),
    Target(&'a str),
}

pub trait ObservationBackend {
    fn ui_snapshot(&self) -> Result<UiSnapshot>;
    fn wait_for_ui_snapshot_event(
        &self,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> Option<Result<UiSnapshotEvent>>;
}

pub trait DiagnosticBackend {
    fn diagnostic_screen_snapshot(&self) -> Result<String>;
    fn diagnostic_dom_snapshot(&self) -> Result<String>;
    fn wait_for_diagnostic_dom_patterns(
        &self,
        patterns: &[String],
        timeout_ms: u64,
    ) -> Option<Result<String>>;
    fn wait_for_diagnostic_target(&self, selector: &str, timeout_ms: u64)
        -> Option<Result<String>>;
    fn tail_log(&self, lines: usize) -> Result<Vec<String>>;
    fn read_clipboard(&self) -> Result<String>;
}

pub trait RawUiBackend {
    fn send_keys(&mut self, keys: &str) -> Result<()>;
    fn send_key(&mut self, key: ToolKey, repeat: u16) -> Result<()> {
        let sequence = tool_key_sequence(key);
        let repeat = repeat.max(1);
        for _ in 0..repeat {
            self.send_keys(sequence)?;
        }
        Ok(())
    }
    fn click_button(&mut self, label: &str) -> Result<()>;
    fn activate_control(&mut self, control_id: ControlId) -> Result<()>;
    fn click_target(&mut self, selector: &str) -> Result<()>;
    fn fill_input(&mut self, selector: &str, value: &str) -> Result<()>;
    fn fill_field(&mut self, field_id: FieldId, value: &str) -> Result<()>;
    fn activate_list_item(&mut self, list_id: ListId, item_id: &str) -> Result<()>;
}

pub trait InstanceBackend {
    fn id(&self) -> &str;
    fn backend_kind(&self) -> &'static str;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn inject_message(&mut self, _message: &str) -> Result<()> {
        Ok(())
    }
    fn authority_id(&mut self) -> Result<Option<String>>;
    fn health_check(&self) -> Result<bool> {
        Ok(self.is_healthy())
    }
    fn wait_until_ready(&self, timeout: Duration) -> Result<()>;
    fn is_healthy(&self) -> bool;
}

pub trait SharedSemanticBackend {
    fn shared_projection(&self) -> Result<UiSnapshot>;
    fn wait_for_shared_projection_event(
        &self,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> Option<Result<UiSnapshotEvent>>;
    fn submit_semantic_command(
        &mut self,
        request: SemanticCommandRequest,
    ) -> Result<SemanticCommandResponse>;

    fn wait_for_newer_shared_projection(
        &self,
        timeout: Duration,
        after_version: Option<u64>,
        baseline: ProjectionRevision,
    ) -> Result<Option<UiSnapshotEvent>> {
        let Some(event) = self.wait_for_shared_projection_event(timeout, after_version) else {
            return Ok(None);
        };
        let event = event?;
        if event.snapshot.revision.is_newer_than(baseline) {
            return Ok(Some(event));
        }
        bail!(
            "shared projection freshness violation: revision {:?} is not newer than baseline {:?}",
            event.snapshot.revision,
            baseline
        );
    }

    fn submit_create_account(&mut self, account_name: &str) -> Result<SubmittedAction<()>> {
        expect_semantic_command_unit(
            self.submit_semantic_command(SemanticCommandRequest::new(
                IntentAction::CreateAccount {
                    account_name: account_name.to_string(),
                },
            ))?,
            "submit_create_account",
        )
    }

    fn submit_create_home(&mut self, home_name: &str) -> Result<SubmittedAction<()>> {
        expect_semantic_command_unit(
            self.submit_semantic_command(SemanticCommandRequest::new(IntentAction::CreateHome {
                home_name: home_name.to_string(),
            }))?,
            "submit_create_home",
        )
    }

    fn submit_create_channel(
        &mut self,
        channel_name: &str,
    ) -> Result<SubmittedAction<ChannelBinding>> {
        expect_semantic_command_channel_binding(
            self.submit_semantic_command(SemanticCommandRequest::new(
                IntentAction::CreateChannel {
                    channel_name: channel_name.to_string(),
                },
            ))?,
            "submit_create_channel",
        )
    }

    fn submit_create_contact_invitation(
        &mut self,
        receiver_authority_id: &str,
    ) -> Result<SubmittedAction<ContactInvitationCode>> {
        let response = self.submit_semantic_command(SemanticCommandRequest::new(
            IntentAction::CreateContactInvitation {
                receiver_authority_id: receiver_authority_id.to_string(),
                code_name: None,
            },
        ))?;
        match response.value {
            SemanticCommandValue::ContactInvitationCode { code } => Ok(SubmittedAction {
                value: ContactInvitationCode { code },
                submission: response.submission,
                handle: response.handle,
            }),
            SemanticCommandValue::None => Err(anyhow!(
                "submit_create_contact_invitation did not produce a contact invitation code"
            )),
            SemanticCommandValue::ChannelSelection { .. } => Err(anyhow!(
                "submit_create_contact_invitation produced an unexpected channel selection payload"
            )),
            SemanticCommandValue::AuthoritativeChannelBinding { .. } => Err(anyhow!(
                "submit_create_contact_invitation produced an unexpected channel binding payload"
            )),
        }
    }

    fn submit_accept_contact_invitation(&mut self, code: &str) -> Result<SubmittedAction<()>> {
        expect_semantic_command_unit(
            self.submit_semantic_command(SemanticCommandRequest::new(
                IntentAction::AcceptContactInvitation {
                    code: code.to_string(),
                },
            ))?,
            "submit_accept_contact_invitation",
        )
    }

    fn submit_invite_actor_to_channel(
        &mut self,
        authority_id: &str,
        channel_id: Option<&str>,
        context_id: Option<&str>,
        channel_name: Option<&str>,
    ) -> Result<SubmittedAction<()>> {
        expect_semantic_command_unit_with_required_handle(
            self.submit_semantic_command(SemanticCommandRequest::new(
                IntentAction::InviteActorToChannel {
                    authority_id: authority_id.to_string(),
                    channel_id: channel_id.map(ToOwned::to_owned),
                    context_id: context_id.map(ToOwned::to_owned),
                    channel_name: channel_name.map(ToOwned::to_owned),
                },
            ))?,
            "submit_invite_actor_to_channel",
        )
    }

    fn submit_accept_pending_channel_invitation(&mut self) -> Result<SubmittedAction<()>> {
        expect_semantic_command_unit_with_required_handle(
            self.submit_semantic_command(SemanticCommandRequest::new(
                IntentAction::AcceptPendingChannelInvitation,
            ))?,
            "submit_accept_pending_channel_invitation",
        )
    }

    fn submit_join_channel(
        &mut self,
        channel_name: &str,
    ) -> Result<SubmittedAction<ChannelBinding>> {
        expect_semantic_command_channel_binding_with_required_handle(
            self.submit_semantic_command(SemanticCommandRequest::new(IntentAction::JoinChannel {
                channel_name: channel_name.to_string(),
            }))?,
            "submit_join_channel",
        )
    }

    fn submit_send_chat_message(&mut self, message: &str) -> Result<SubmittedAction<()>> {
        expect_semantic_command_unit_with_required_handle(
            self.submit_semantic_command(SemanticCommandRequest::new(
                IntentAction::SendChatMessage {
                    message: message.to_string(),
                },
            ))?,
            "submit_send_chat_message",
        )
    }
}

fn expect_semantic_command_unit(
    response: SemanticCommandResponse,
    operation: &str,
) -> Result<SubmittedAction<()>> {
    match response.value {
        SemanticCommandValue::None => Ok(SubmittedAction {
            value: (),
            submission: response.submission,
            handle: response.handle,
        }),
        SemanticCommandValue::ContactInvitationCode { .. } => Err(anyhow!(
            "{operation} produced an unexpected contact invitation code payload"
        )),
        SemanticCommandValue::ChannelSelection { .. } => Err(anyhow!(
            "{operation} produced an unexpected channel selection payload"
        )),
        SemanticCommandValue::AuthoritativeChannelBinding { .. } => Err(anyhow!(
            "{operation} produced an unexpected channel binding payload"
        )),
    }
}

fn expect_semantic_command_unit_with_required_handle(
    response: SemanticCommandResponse,
    operation: &str,
) -> Result<SubmittedAction<()>> {
    let submitted = expect_semantic_command_unit(response, operation)?;
    if submitted.handle.ui_operation.is_none() {
        return Err(anyhow!(
            "{operation} must expose a canonical ui operation handle with exact instance tracking"
        ));
    }
    Ok(submitted)
}

fn expect_semantic_command_channel_binding(
    response: SemanticCommandResponse,
    operation: &str,
) -> Result<SubmittedAction<ChannelBinding>> {
    match response.value {
        SemanticCommandValue::AuthoritativeChannelBinding {
            channel_id,
            context_id,
        } => Ok(SubmittedAction {
            value: ChannelBinding {
                channel_id,
                context_id,
            },
            submission: response.submission,
            handle: response.handle,
        }),
        SemanticCommandValue::None => Err(anyhow!(
            "{operation} did not produce a channel binding payload"
        )),
        SemanticCommandValue::ChannelSelection { channel_id } => Err(anyhow!(
            "{operation} produced only a weak selected-channel payload without authoritative context: {channel_id}"
        )),
        SemanticCommandValue::ContactInvitationCode { .. } => Err(anyhow!(
            "{operation} produced an unexpected contact invitation code payload"
        )),
    }
}

fn expect_semantic_command_channel_binding_with_required_handle(
    response: SemanticCommandResponse,
    operation: &str,
) -> Result<SubmittedAction<ChannelBinding>> {
    let submitted = expect_semantic_command_channel_binding(response, operation)?;
    if submitted.handle.ui_operation.is_none() {
        return Err(anyhow!(
            "{operation} must expose a canonical ui operation handle with exact instance tracking"
        ));
    }
    Ok(submitted)
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
    Local(Box<local_pty::LocalPtyBackend>),
    Browser(Box<playwright_browser::PlaywrightBrowserBackend>),
    Ssh(Box<ssh_tunnel::SshTunnelBackend>),
}

impl BackendHandle {
    pub fn from_config(
        config: InstanceConfig,
        pty_rows: Option<u16>,
        pty_cols: Option<u16>,
    ) -> Result<Self> {
        match config.mode {
            InstanceMode::Local => Ok(Self::Local(Box::new(local_pty::LocalPtyBackend::new(
                config, pty_rows, pty_cols,
            )))),
            InstanceMode::Browser => Ok(Self::Browser(Box::new(
                playwright_browser::PlaywrightBrowserBackend::new(config)?,
            ))),
            InstanceMode::Ssh => Ok(Self::Ssh(Box::new(ssh_tunnel::SshTunnelBackend::new(
                config,
            )))),
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Local(backend) => backend.id(),
            Self::Browser(backend) => backend.id(),
            Self::Ssh(backend) => backend.id(),
        }
    }

    pub fn backend_kind(&self) -> &'static str {
        match self {
            Self::Local(backend) => backend.backend_kind(),
            Self::Browser(backend) => backend.backend_kind(),
            Self::Ssh(backend) => backend.backend_kind(),
        }
    }

    pub fn start(&mut self) -> Result<()> {
        match self {
            Self::Local(backend) => backend.start(),
            Self::Browser(backend) => backend.start(),
            Self::Ssh(backend) => backend.start(),
        }
    }

    pub fn stop(&mut self) -> Result<()> {
        match self {
            Self::Local(backend) => backend.stop(),
            Self::Browser(backend) => backend.stop(),
            Self::Ssh(backend) => backend.stop(),
        }
    }

    pub fn restart(&mut self) -> Result<()> {
        self.stop()?;
        self.start()
    }

    pub fn authority_id(&mut self) -> Result<Option<String>> {
        match self {
            Self::Local(backend) => backend.authority_id(),
            Self::Browser(backend) => backend.authority_id(),
            Self::Ssh(backend) => backend.authority_id(),
        }
    }

    pub fn health_check(&self) -> Result<bool> {
        match self {
            Self::Local(backend) => backend.health_check(),
            Self::Browser(backend) => backend.health_check(),
            Self::Ssh(backend) => backend.health_check(),
        }
    }

    pub fn wait_until_ready(&self, timeout: Duration) -> Result<()> {
        match self {
            Self::Local(backend) => backend.wait_until_ready(timeout),
            Self::Browser(backend) => backend.wait_until_ready(timeout),
            Self::Ssh(backend) => backend.wait_until_ready(timeout),
        }
    }

    pub fn stage_runtime_identity(&mut self, authority_id: &str, device_id: &str) -> Result<()> {
        match self {
            Self::Browser(backend) => backend.stage_runtime_identity(authority_id, device_id),
            Self::Local(backend) => bail!(
                "backend {} does not support runtime identity staging",
                backend.backend_kind()
            ),
            Self::Ssh(backend) => bail!(
                "backend {} does not support runtime identity staging",
                backend.backend_kind()
            ),
        }
    }

    pub fn diagnostic_screen_snapshot(&self) -> Result<String> {
        match self {
            Self::Local(backend) => backend.diagnostic_screen_snapshot(),
            Self::Browser(backend) => backend.diagnostic_screen_snapshot(),
            Self::Ssh(backend) => backend.diagnostic_screen_snapshot(),
        }
    }

    pub fn diagnostic_dom_snapshot(&self) -> Result<String> {
        match self {
            Self::Local(backend) => backend.diagnostic_dom_snapshot(),
            Self::Browser(backend) => backend.diagnostic_dom_snapshot(),
            Self::Ssh(backend) => backend.diagnostic_dom_snapshot(),
        }
    }

    pub fn wait_for_diagnostic_observation_probe(
        &self,
        probe: DiagnosticObservationProbe<'_>,
        timeout_ms: u64,
    ) -> Option<Result<String>> {
        match self {
            Self::Local(backend) => match probe {
                DiagnosticObservationProbe::DomPatterns(patterns) => {
                    backend.wait_for_diagnostic_dom_patterns(patterns, timeout_ms)
                }
                DiagnosticObservationProbe::Target(selector) => {
                    backend.wait_for_diagnostic_target(selector, timeout_ms)
                }
            },
            Self::Browser(backend) => match probe {
                DiagnosticObservationProbe::DomPatterns(patterns) => {
                    backend.wait_for_diagnostic_dom_patterns(patterns, timeout_ms)
                }
                DiagnosticObservationProbe::Target(selector) => {
                    backend.wait_for_diagnostic_target(selector, timeout_ms)
                }
            },
            Self::Ssh(backend) => match probe {
                DiagnosticObservationProbe::DomPatterns(patterns) => {
                    backend.wait_for_diagnostic_dom_patterns(patterns, timeout_ms)
                }
                DiagnosticObservationProbe::Target(selector) => {
                    backend.wait_for_diagnostic_target(selector, timeout_ms)
                }
            },
        }
    }

    pub fn tail_log(&self, lines: usize) -> Result<Vec<String>> {
        match self {
            Self::Local(backend) => backend.tail_log(lines),
            Self::Browser(backend) => backend.tail_log(lines),
            Self::Ssh(backend) => backend.tail_log(lines),
        }
    }

    pub fn read_clipboard(&self) -> Result<String> {
        match self {
            Self::Local(backend) => backend.read_clipboard(),
            Self::Browser(backend) => backend.read_clipboard(),
            Self::Ssh(backend) => backend.read_clipboard(),
        }
    }

    pub fn supports_ui_snapshot(&self) -> bool {
        matches!(self, Self::Local(_) | Self::Browser(_))
    }

    pub fn ui_snapshot(&self) -> Result<UiSnapshot> {
        match self {
            Self::Local(backend) => backend.ui_snapshot(),
            Self::Browser(backend) => backend.ui_snapshot(),
            Self::Ssh(backend) => bail!(
                "backend {} does not support structured ui snapshots",
                backend.backend_kind()
            ),
        }
    }

    pub fn wait_for_ui_snapshot_event(
        &self,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> Result<Option<UiSnapshotEvent>> {
        match self {
            Self::Local(backend) => backend
                .wait_for_ui_snapshot_event(timeout, after_version)
                .transpose(),
            Self::Browser(backend) => backend
                .wait_for_ui_snapshot_event(timeout, after_version)
                .transpose(),
            Self::Ssh(backend) => bail!(
                "backend {} does not support structured ui snapshot events",
                backend.backend_kind()
            ),
        }
    }

    pub fn send_keys(&mut self, keys: &str) -> Result<()> {
        match self {
            Self::Local(backend) => backend.send_keys(keys),
            Self::Browser(backend) => backend.send_keys(keys),
            Self::Ssh(backend) => bail!(
                "backend {} does not support raw ui key input",
                backend.backend_kind()
            ),
        }
    }

    pub fn send_key(&mut self, key: ToolKey, repeat: u16) -> Result<()> {
        match self {
            Self::Local(backend) => backend.send_key(key, repeat),
            Self::Browser(backend) => backend.send_key(key, repeat),
            Self::Ssh(backend) => bail!(
                "backend {} does not support raw ui key input",
                backend.backend_kind()
            ),
        }
    }

    pub fn click_button(&mut self, label: &str) -> Result<()> {
        match self {
            Self::Local(backend) => backend.click_button(label),
            Self::Browser(backend) => backend.click_button(label),
            Self::Ssh(backend) => bail!(
                "backend {} does not support raw ui button clicks",
                backend.backend_kind()
            ),
        }
    }

    pub fn activate_control(&mut self, control_id: ControlId) -> Result<()> {
        match self {
            Self::Local(backend) => backend.activate_control(control_id),
            Self::Browser(backend) => backend.activate_control(control_id),
            Self::Ssh(backend) => bail!(
                "backend {} does not support raw ui control activation",
                backend.backend_kind()
            ),
        }
    }

    pub fn click_target(&mut self, selector: &str) -> Result<()> {
        match self {
            Self::Local(backend) => backend.click_target(selector),
            Self::Browser(backend) => backend.click_target(selector),
            Self::Ssh(backend) => bail!(
                "backend {} does not support raw ui target clicks",
                backend.backend_kind()
            ),
        }
    }

    pub fn fill_input(&mut self, selector: &str, value: &str) -> Result<()> {
        match self {
            Self::Local(backend) => backend.fill_input(selector, value),
            Self::Browser(backend) => backend.fill_input(selector, value),
            Self::Ssh(backend) => bail!(
                "backend {} does not support raw ui selector input",
                backend.backend_kind()
            ),
        }
    }

    pub fn fill_field(&mut self, field_id: FieldId, value: &str) -> Result<()> {
        match self {
            Self::Local(backend) => backend.fill_field(field_id, value),
            Self::Browser(backend) => backend.fill_field(field_id, value),
            Self::Ssh(backend) => bail!(
                "backend {} does not support raw ui field input",
                backend.backend_kind()
            ),
        }
    }

    pub fn activate_list_item(&mut self, list_id: ListId, item_id: &str) -> Result<()> {
        match self {
            Self::Local(backend) => backend.activate_list_item(list_id, item_id),
            Self::Browser(backend) => backend.activate_list_item(list_id, item_id),
            Self::Ssh(backend) => bail!(
                "backend {} does not support raw ui list activation",
                backend.backend_kind()
            ),
        }
    }

    pub fn submit_create_contact_invitation(
        &mut self,
        receiver_authority_id: &str,
    ) -> Result<SubmittedAction<ContactInvitationCode>> {
        match self {
            Self::Local(backend) => backend.submit_create_contact_invitation(receiver_authority_id),
            Self::Browser(backend) => {
                backend.submit_create_contact_invitation(receiver_authority_id)
            }
            Self::Ssh(backend) => bail!(
                "backend {} does not support shared semantic contact invitation submission",
                backend.backend_kind()
            ),
        }
    }

    pub fn submit_semantic_command(
        &mut self,
        request: SemanticCommandRequest,
    ) -> Result<SemanticCommandResponse> {
        match self {
            Self::Local(backend) => backend.submit_semantic_command(request),
            Self::Browser(backend) => backend.submit_semantic_command(request),
            Self::Ssh(backend) => bail!(
                "backend {} does not support shared semantic command submission",
                backend.backend_kind()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BackendHandle, DiagnosticBackend, InstanceBackend, ObservationBackend,
        SemanticCommandRequest, SemanticCommandResponse, SemanticCommandValue,
        SharedSemanticBackend, SubmissionState, UiOperationHandle, UiSnapshotEvent,
    };
    use crate::config::{InstanceConfig, InstanceMode};
    use anyhow::{anyhow, Result};
    use aura_app::scenario_contract::{IntentAction, SemanticSubmissionHandle};
    use aura_app::ui::contract::{
        OperationId, OperationInstanceId, ScreenId, UiReadiness, UiSnapshot,
    };
    use aura_app::ui_contract::{ProjectionRevision, QuiescenceSnapshot};
    use std::cell::Cell;
    use std::cell::RefCell;
    use std::fs;
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

        fn start(&mut self) -> Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> Result<()> {
            Ok(())
        }

        fn authority_id(&mut self) -> Result<Option<String>> {
            Ok(None)
        }

        fn wait_until_ready(&self, _timeout: Duration) -> Result<()> {
            Ok(())
        }

        fn is_healthy(&self) -> bool {
            true
        }
    }

    impl DiagnosticBackend for ReadOnlyBackend {
        fn diagnostic_screen_snapshot(&self) -> Result<String> {
            Ok("snapshot".to_string())
        }

        fn diagnostic_dom_snapshot(&self) -> Result<String> {
            Ok("dom".to_string())
        }

        fn wait_for_diagnostic_dom_patterns(
            &self,
            _patterns: &[String],
            _timeout_ms: u64,
        ) -> Option<Result<String>> {
            Some(Ok("dom-match".to_string()))
        }

        fn wait_for_diagnostic_target(
            &self,
            _selector: &str,
            _timeout_ms: u64,
        ) -> Option<Result<String>> {
            Some(Ok("target-match".to_string()))
        }

        fn tail_log(&self, _lines: usize) -> Result<Vec<String>> {
            Ok(vec!["log".to_string()])
        }

        fn read_clipboard(&self) -> Result<String> {
            Ok("clipboard".to_string())
        }
    }

    impl ObservationBackend for ReadOnlyBackend {
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
    }

    #[test]
    fn observation_endpoints_are_side_effect_free() -> Result<()> {
        let backend = ReadOnlyBackend::new();
        let observer: &dyn ObservationBackend = &backend;
        let diagnostic: &dyn DiagnosticBackend = &backend;

        assert_eq!(diagnostic.diagnostic_screen_snapshot()?, "snapshot");
        assert_eq!(diagnostic.diagnostic_screen_snapshot()?, "snapshot");
        assert_eq!(diagnostic.diagnostic_dom_snapshot()?, "dom");
        assert_eq!(observer.ui_snapshot()?.revision.semantic_seq, 7);
        assert_eq!(observer.ui_snapshot()?.revision.semantic_seq, 7);
        assert_eq!(
            observer
                .wait_for_ui_snapshot_event(Duration::from_millis(1), Some(7))
                .ok_or_else(|| anyhow::anyhow!("event should be present"))??
                .version,
            8
        );
        assert_eq!(
            diagnostic
                .wait_for_diagnostic_dom_patterns(&["chat".to_string()], 1)
                .ok_or_else(|| anyhow::anyhow!("dom result should be present"))??,
            "dom-match"
        );
        assert_eq!(
            diagnostic
                .wait_for_diagnostic_target("#aura-screen-chat", 1)
                .ok_or_else(|| anyhow::anyhow!("target result should be present"))??,
            "target-match"
        );
        assert_eq!(diagnostic.tail_log(1)?, vec!["log".to_string()]);
        assert_eq!(diagnostic.read_clipboard()?, "clipboard");
        assert_eq!(backend.mutation_calls.get(), 0);
        Ok(())
    }

    #[test]
    fn backend_handle_does_not_reintroduce_broad_capability_wrappers() {
        let source = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/backend/mod.rs"),
        )
        .unwrap_or_else(|error| panic!("failed to read backend source: {error}"));

        let forbidden = [
            ["pub fn ", "as_", "lifecycle_mut("].concat(),
            ["pub fn ", "as_", "lifecycle("].concat(),
            ["pub fn ", "as_", "diagnostic("].concat(),
            ["pub fn ", "as_", "observation("].concat(),
            ["pub fn ", "as_", "raw_ui_mut("].concat(),
            ["pub fn ", "as_", "shared_semantic_mut("].concat(),
            ["runtime identity ", "staging is not supported by backend"].concat(),
        ];

        for forbidden in forbidden {
            assert!(
                !source.contains(&forbidden),
                "backend wrapper cleanup regressed: found {forbidden:?}"
            );
        }
    }

    #[test]
    fn backend_handle_uses_single_diagnostic_observation_probe_surface() {
        let source = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/backend/mod.rs"),
        )
        .unwrap_or_else(|error| panic!("failed to read backend source: {error}"));
        let old_dom_wrapper = ["pub fn ", "wait_for_diagnostic_dom_patterns("].concat();
        let old_target_wrapper = ["pub fn ", "wait_for_diagnostic_target("].concat();
        let new_probe_wrapper = ["pub fn ", "wait_for_diagnostic_observation_probe("].concat();

        assert!(
            !source.contains(&old_dom_wrapper),
            "backend handle should not expose a dedicated dom-pattern wait wrapper"
        );
        assert!(
            !source.contains(&old_target_wrapper),
            "backend handle should not expose a dedicated selector wait wrapper"
        );
        assert!(source.contains(&new_probe_wrapper));
    }

    struct RecordingSemanticBackend {
        submit_requests: RefCell<Vec<SemanticCommandRequest>>,
        next_response: RefCell<Option<Result<SemanticCommandResponse>>>,
        projection_event: RefCell<Option<Result<UiSnapshotEvent>>>,
    }

    impl RecordingSemanticBackend {
        fn new() -> Self {
            Self {
                submit_requests: RefCell::new(Vec::new()),
                next_response: RefCell::new(None),
                projection_event: RefCell::new(None),
            }
        }

        fn with_response(self, response: Result<SemanticCommandResponse>) -> Self {
            self.next_response.replace(Some(response));
            self
        }

        fn with_projection_event(self, event: Result<UiSnapshotEvent>) -> Self {
            self.projection_event.replace(Some(event));
            self
        }
    }

    impl SharedSemanticBackend for RecordingSemanticBackend {
        fn shared_projection(&self) -> Result<UiSnapshot> {
            Ok(ReadOnlyBackend::snapshot_value())
        }

        fn wait_for_shared_projection_event(
            &self,
            _timeout: Duration,
            _after_version: Option<u64>,
        ) -> Option<Result<UiSnapshotEvent>> {
            self.projection_event.borrow_mut().take()
        }

        fn submit_semantic_command(
            &mut self,
            request: SemanticCommandRequest,
        ) -> Result<SemanticCommandResponse> {
            self.submit_requests.borrow_mut().push(request);
            self.next_response
                .borrow_mut()
                .take()
                .unwrap_or_else(|| Ok(SemanticCommandResponse::accepted_without_value()))
        }
    }

    fn operation_handle() -> UiOperationHandle {
        UiOperationHandle::new(
            OperationId::invitation_accept(),
            OperationInstanceId("test-op-1".to_string()),
        )
    }

    #[test]
    fn shared_semantic_submit_convenience_methods_forward_requests_and_preserve_handles() {
        let handle = operation_handle();
        let response = SemanticCommandResponse {
            submission: SubmissionState::Accepted,
            handle: SemanticSubmissionHandle {
                ui_operation: Some(handle.clone()),
            },
            value: SemanticCommandValue::None,
        };
        let mut backend = RecordingSemanticBackend::new().with_response(Ok(response));

        let submitted = backend
            .submit_create_account("alice")
            .unwrap_or_else(|error| panic!("semantic submit should succeed: {error:#}"));

        assert_eq!(submitted.submission, SubmissionState::Accepted);
        assert_eq!(submitted.handle.ui_operation, Some(handle));
        assert_eq!(
            backend.submit_requests.borrow().as_slice(),
            &[SemanticCommandRequest::new(IntentAction::CreateAccount {
                account_name: "alice".to_string(),
            })]
        );
    }

    #[test]
    fn shared_semantic_submit_failures_remain_diagnostic() {
        let mut backend = RecordingSemanticBackend::new().with_response(Err(anyhow!(
            "unsupported semantic browser command: open_screen"
        )));

        let error = backend
            .submit_create_home("alice-home")
            .err()
            .unwrap_or_else(|| panic!("unsupported semantic command should fail"));

        assert!(
            error
                .to_string()
                .contains("unsupported semantic browser command"),
            "expected unsupported-command context, got {error:#}"
        );
        assert_eq!(
            backend.submit_requests.borrow().as_slice(),
            &[SemanticCommandRequest::new(IntentAction::CreateHome {
                home_name: "alice-home".to_string(),
            })]
        );
    }

    #[test]
    fn shared_semantic_submit_rejects_unexpected_payload_shapes() {
        let mut backend = RecordingSemanticBackend::new().with_response(Ok(
            SemanticCommandResponse::accepted_contact_invitation_code("invite-code".to_string()),
        ));

        let error = backend
            .submit_create_account("alice")
            .err()
            .unwrap_or_else(|| panic!("unexpected payload shape should fail"));

        assert!(
            error
                .to_string()
                .contains("unexpected contact invitation code payload"),
            "expected payload-shape rejection, got {error:#}"
        );
    }

    #[test]
    fn parity_critical_shared_submit_helpers_require_ui_operation_handles() {
        let mut backend = RecordingSemanticBackend::new().with_response(Ok(
            SemanticCommandResponse::accepted_authoritative_channel_binding(
                "channel:test".to_string(),
                "ctx:test".to_string(),
            ),
        ));

        let error = backend
            .submit_join_channel("shared-parity-lab")
            .err()
            .unwrap_or_else(|| panic!("missing ui handle must fail"));

        assert!(
            error
                .to_string()
                .contains("canonical ui operation handle with exact instance tracking"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn shared_submit_rejects_weak_channel_selection_when_binding_is_required() {
        let mut backend = RecordingSemanticBackend::new().with_response(Ok(
            SemanticCommandResponse::accepted_channel_selection("channel:test".to_string()),
        ));

        let error = backend
            .submit_join_channel("shared-parity-lab")
            .err()
            .unwrap_or_else(|| panic!("weak selection payload must fail"));

        assert!(
            error
                .to_string()
                .contains("weak selected-channel payload without authoritative context"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn shared_projection_wait_requires_strictly_newer_revision() {
        let baseline = ProjectionRevision {
            semantic_seq: 7,
            render_seq: Some(7),
        };
        let stale_event = UiSnapshotEvent {
            snapshot: UiSnapshot {
                revision: baseline,
                ..ReadOnlyBackend::snapshot_value()
            },
            version: 8,
        };
        let backend = RecordingSemanticBackend::new().with_projection_event(Ok(stale_event));

        let error = backend
            .wait_for_newer_shared_projection(Duration::from_millis(1), Some(7), baseline)
            .err()
            .unwrap_or_else(|| panic!("stale projections must fail"));
        assert!(
            error
                .to_string()
                .contains("shared projection freshness violation"),
            "expected explicit freshness violation, got {error:#}"
        );
    }

    #[test]
    fn shared_projection_wait_accepts_strictly_newer_revision() {
        let baseline = ProjectionRevision {
            semantic_seq: 7,
            render_seq: Some(7),
        };
        let newer = ProjectionRevision {
            semantic_seq: 7,
            render_seq: Some(8),
        };
        let backend = RecordingSemanticBackend::new().with_projection_event(Ok(UiSnapshotEvent {
            snapshot: UiSnapshot {
                revision: newer,
                ..ReadOnlyBackend::snapshot_value()
            },
            version: 8,
        }));

        let event = backend
            .wait_for_newer_shared_projection(Duration::from_millis(1), Some(7), baseline)
            .unwrap_or_else(|error| panic!("newer projections should pass: {error:#}"))
            .unwrap_or_else(|| panic!("projection event should be present"));
        assert_eq!(event.snapshot.revision, newer);
        assert_eq!(event.version, 8);
    }
}
