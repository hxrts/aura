//! Tool API for harness-client RPC communication.
//!
//! Defines request/response types and dispatch logic for the harness tool API,
//! enabling test clients to send input, capture screens, and query instance state.

use aura_app::scenario_contract::{IntentAction, SharedActionContract, SubmissionContract};
use aura_app::ui::contract::{ControlId, FieldId, ListId, UiSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api_version::{negotiate, TOOL_API_DEFAULT_VERSION, TOOL_API_VERSIONS};
use crate::backend::{SemanticCommandRequest, SemanticCommandResponse, UiSnapshotEvent};
use crate::config::{RunConfig, RuntimeSubstrate, ScreenSource};
use crate::coordinator::{DiagnosticObservationWait, HarnessCoordinator};
use crate::introspection::{
    extract_channels, extract_contacts, extract_current_selection, extract_toast, ChannelSnapshot,
    ContactSnapshot, SelectionSnapshot, ToastSnapshot,
};
use crate::screen_normalization::{authoritative_screen, normalize_screen};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupSummary {
    pub tool_api_version: String,
    pub schema_version: u32,
    pub run_name: String,
    pub runtime_substrate: RuntimeSubstrate,
    pub artifact_dir: Option<String>,
    pub instance_count: u64,
    pub instances: Vec<StartupInstanceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupInstanceSummary {
    pub id: String,
    pub mode: String,
    pub bind_address: String,
    pub data_dir: String,
    pub browser_artifact_dir: Option<String>,
}

impl StartupSummary {
    pub fn from_run_config(config: &RunConfig) -> Self {
        let instances = config
            .instances
            .iter()
            .map(|instance| StartupInstanceSummary {
                id: instance.id.clone(),
                mode: format!("{:?}", instance.mode).to_lowercase(),
                bind_address: instance.bind_address.clone(),
                data_dir: instance.data_dir.display().to_string(),
                browser_artifact_dir: instance.env.iter().find_map(|entry| {
                    let (key, value) = entry.split_once('=')?;
                    (key == "AURA_HARNESS_BROWSER_ARTIFACT_DIR").then(|| value.to_string())
                }),
            })
            .collect();

        Self {
            tool_api_version: TOOL_API_DEFAULT_VERSION.to_string(),
            schema_version: config.schema_version,
            run_name: config.run.name.clone(),
            runtime_substrate: config.run.runtime_substrate,
            artifact_dir: config
                .run
                .artifact_dir
                .as_ref()
                .map(|path| path.display().to_string()),
            instance_count: config.instances.len() as u64,
            instances,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticScreenCapture {
    pub diagnostic_authoritative_screen: String,
    pub diagnostic_raw_screen: String,
    pub diagnostic_normalized_screen: String,
    pub screen_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_consistency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_view: Option<String>,
}

impl DiagnosticScreenCapture {
    fn settled(screen: String, screen_source: ScreenSource) -> Self {
        let diagnostic_authoritative_screen = authoritative_screen(&screen);
        let diagnostic_normalized_screen = normalize_screen(&screen);
        Self {
            diagnostic_authoritative_screen,
            diagnostic_raw_screen: screen,
            diagnostic_normalized_screen,
            screen_source: format!("{screen_source:?}").to_ascii_lowercase(),
            capture_consistency: Some("settled".to_string()),
            matched: None,
            matched_view: None,
        }
    }

    fn matched(screen: String, screen_source: ScreenSource) -> Self {
        let diagnostic_authoritative_screen = authoritative_screen(&screen);
        let diagnostic_normalized_screen = normalize_screen(&screen);
        Self {
            diagnostic_authoritative_screen,
            diagnostic_raw_screen: screen,
            diagnostic_normalized_screen,
            screen_source: format!("{screen_source:?}").to_ascii_lowercase(),
            capture_consistency: None,
            matched: Some(true),
            matched_view: Some("normalized".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolNegotiationPayload {
    pub negotiated_version: String,
    pub supported_versions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Sent,
    Activated,
    Created,
    Clicked,
    Filled,
    Restarted,
    Killed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolStatusPayload {
    pub status: ToolStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactInvitationCreatedPayload {
    pub status: ToolStatus,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TailLogPayload {
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipboardPayload {
    pub text: String,
}

/// Exact setup code exported by the selected invitee runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceEnrollmentSetupPayload {
    pub setup_code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityIdSource {
    Backend,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityIdPayload {
    pub authority_id: String,
    pub source: AuthorityIdSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticChannelListPayload {
    pub diagnostic_channels: Vec<ChannelSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticSelectionPayload {
    pub diagnostic_selection: Option<SelectionSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticContactListPayload {
    pub diagnostic_contacts: Vec<ContactSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic_toast: Option<ToastSnapshot>,
}

/// Original receipt with the canonical shared contract used for this submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticCommandPayload {
    #[serde(flatten)]
    pub response: SemanticCommandResponse,
    pub contract: SharedActionContract,
}

impl SemanticCommandPayload {
    fn new(
        contract: SharedActionContract,
        response: SemanticCommandResponse,
    ) -> anyhow::Result<Self> {
        if let SubmissionContract::OperationHandle { operation_id, .. } = &contract.submission {
            let handle = response.handle.ui_operation.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "semantic receipt is missing required operation handle {operation_id:?}"
                )
            })?;
            anyhow::ensure!(
                handle.id() == operation_id,
                "semantic receipt operation handle does not match canonical contract"
            );
        }
        Ok(Self { response, contract })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceMetadataPayload {
    pub instance_id: String,
    pub data_dir: std::path::PathBuf,
}

/// A bounded authoritative subscription wait; `None` is not semantic success.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSnapshotEventPayload {
    pub event: Option<UiSnapshotEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolPayload {
    Negotiation(ToolNegotiationPayload),
    DiagnosticScreenCapture(DiagnosticScreenCapture),
    UiSnapshot(Box<UiSnapshot>),
    InstanceMetadata(InstanceMetadataPayload),
    SemanticCommand(Box<SemanticCommandPayload>),
    UiSnapshotEvent(Box<UiSnapshotEventPayload>),
    Status(ToolStatusPayload),
    ContactInvitationCreated(ContactInvitationCreatedPayload),
    TailLog(TailLogPayload),
    Clipboard(ClipboardPayload),
    DeviceEnrollmentSetup(DeviceEnrollmentSetupPayload),
    AuthorityId(AuthorityIdPayload),
    DiagnosticChannels(DiagnosticChannelListPayload),
    DiagnosticSelection(DiagnosticSelectionPayload),
    DiagnosticContacts(DiagnosticContactListPayload),
}

impl ToolPayload {
    pub fn to_json_value(&self) -> serde_json::Result<Value> {
        serde_json::to_value(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ToolRequest {
    Negotiate {
        client_versions: Vec<String>,
    },
    Screen {
        instance_id: String,
        #[serde(default)]
        screen_source: ScreenSource,
    },
    UiState {
        instance_id: String,
    },
    InstanceMetadata {
        instance_id: String,
    },
    SubmitSemanticCommand {
        instance_id: String,
        intent: IntentAction,
    },
    WaitForUiSnapshotEvent {
        instance_id: String,
        timeout_ms: u64,
        after_version: Option<u64>,
    },
    SendKeys {
        instance_id: String,
        keys: String,
    },
    SendKey {
        instance_id: String,
        key: ToolKey,
        #[serde(default)]
        repeat: u16,
    },
    ActivateControl {
        instance_id: String,
        control_id: ControlId,
    },
    ActivateListItem {
        instance_id: String,
        list_id: ListId,
        item_id: String,
    },
    CreateContactInvitation {
        instance_id: String,
        receiver_authority_id: String,
    },
    ClickButton {
        instance_id: String,
        label: String,
        selector: Option<String>,
    },
    FillInput {
        instance_id: String,
        selector: String,
        value: String,
    },
    FillField {
        instance_id: String,
        field_id: FieldId,
        value: String,
    },
    WaitFor {
        instance_id: String,
        pattern: String,
        timeout_ms: u64,
        #[serde(default)]
        screen_source: ScreenSource,
        selector: Option<String>,
    },
    TailLog {
        instance_id: String,
        lines: u64,
    },
    ReadClipboard {
        instance_id: String,
    },
    PrepareDeviceEnrollmentSetup {
        instance_id: String,
    },
    GetAuthorityId {
        instance_id: String,
    },
    DiagnosticListChannels {
        instance_id: String,
    },
    DiagnosticCurrentSelection {
        instance_id: String,
    },
    DiagnosticListContacts {
        instance_id: String,
    },
    Restart {
        instance_id: String,
    },
    Kill {
        instance_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKey {
    Enter,
    Esc,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolResponse {
    Ok { payload: ToolPayload },
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolActionRecord {
    pub request: ToolRequest,
    pub response: ToolResponse,
}

pub struct ToolApi {
    coordinator: HarnessCoordinator,
    action_log: Vec<ToolActionRecord>,
    negotiated_version: String,
}

impl ToolApi {
    pub fn new(coordinator: HarnessCoordinator) -> Self {
        Self {
            coordinator,
            action_log: Vec::new(),
            negotiated_version: TOOL_API_DEFAULT_VERSION.to_string(),
        }
    }

    pub fn start_all(&mut self) -> anyhow::Result<()> {
        self.coordinator.start_all()
    }

    pub fn stop_all(&mut self) -> anyhow::Result<()> {
        self.coordinator.stop_all()
    }

    pub fn runtime_substrate(&self) -> RuntimeSubstrate {
        self.coordinator.runtime_substrate()
    }

    pub fn backend_kind(&self, instance_id: &str) -> anyhow::Result<&'static str> {
        self.coordinator.backend_kind(instance_id)
    }

    pub fn supports_ui_snapshot(&self, instance_id: &str) -> anyhow::Result<bool> {
        self.coordinator.supports_ui_snapshot(instance_id)
    }

    pub fn ui_snapshot(&self, instance_id: &str) -> anyhow::Result<UiSnapshot> {
        self.coordinator.ui_snapshot(instance_id)
    }

    pub fn submit_semantic_command(
        &mut self,
        instance_id: &str,
        request: SemanticCommandRequest,
    ) -> anyhow::Result<SemanticCommandResponse> {
        self.coordinator
            .submit_semantic_command_via_ui(instance_id, request)
    }

    pub fn prepare_device_enrollment_setup(&mut self, instance_id: &str) -> anyhow::Result<String> {
        self.coordinator
            .prepare_device_enrollment_setup(instance_id)
    }

    pub fn current_authority_id(&mut self, instance_id: &str) -> anyhow::Result<String> {
        self.coordinator
            .get_authority_id(instance_id)?
            .ok_or_else(|| anyhow::anyhow!("current authority id is unavailable for {instance_id}"))
    }

    pub fn wait_for_ui_snapshot_event(
        &mut self,
        instance_id: &str,
        timeout: Duration,
        after_version: Option<u64>,
    ) -> anyhow::Result<Option<UiSnapshotEvent>> {
        self.coordinator
            .wait_for_ui_snapshot_event(instance_id, timeout, after_version)
    }

    pub fn apply_fault_delay(&mut self, actor: &str, delay_ms: u64) -> anyhow::Result<()> {
        self.coordinator.apply_fault_delay(actor, delay_ms)
    }

    pub fn apply_fault_loss(&mut self, actor: &str, loss_percent: u8) -> anyhow::Result<()> {
        self.coordinator.apply_fault_loss(actor, loss_percent)
    }

    pub fn apply_fault_tunnel_drop(&mut self, actor: &str) -> anyhow::Result<()> {
        self.coordinator.apply_fault_tunnel_drop(actor)
    }

    pub fn handle_request(&mut self, request: ToolRequest) -> ToolResponse {
        let request_for_log = request.clone();
        let outcome = match request {
            ToolRequest::Negotiate { client_versions } => {
                negotiate(&client_versions).map(|result| {
                    self.negotiated_version = result.negotiated_version.clone();
                    ToolPayload::Negotiation(ToolNegotiationPayload {
                        negotiated_version: result.negotiated_version,
                        supported_versions: result.supported_versions,
                    })
                })
            }
            ToolRequest::Screen {
                instance_id,
                screen_source,
            } => self
                .coordinator
                .diagnostic_screen_with_source(&instance_id, screen_source)
                .map(|screen| {
                    ToolPayload::DiagnosticScreenCapture(DiagnosticScreenCapture::settled(
                        screen,
                        screen_source,
                    ))
                }),
            ToolRequest::UiState { instance_id } => self
                .coordinator
                .ui_snapshot(&instance_id)
                .map(|snapshot| ToolPayload::UiSnapshot(Box::new(snapshot))),
            ToolRequest::InstanceMetadata { instance_id } => self
                .coordinator
                .instance_data_dir(&instance_id)
                .map(|data_dir| {
                    ToolPayload::InstanceMetadata(InstanceMetadataPayload {
                        instance_id: instance_id.clone(),
                        data_dir: data_dir.to_path_buf(),
                    })
                }),
            ToolRequest::SubmitSemanticCommand {
                instance_id,
                intent,
            } => {
                let request = SemanticCommandRequest::new(intent);
                let contract = request.contract.clone();
                self.submit_semantic_command(&instance_id, request)
                    .and_then(|response| SemanticCommandPayload::new(contract, response))
                    .map(|payload| ToolPayload::SemanticCommand(Box::new(payload)))
            }
            ToolRequest::WaitForUiSnapshotEvent {
                instance_id,
                timeout_ms,
                after_version,
            } => self
                .wait_for_ui_snapshot_event(
                    &instance_id,
                    Duration::from_millis(timeout_ms),
                    after_version,
                )
                .map(|event| {
                    ToolPayload::UiSnapshotEvent(Box::new(UiSnapshotEventPayload { event }))
                }),
            ToolRequest::SendKeys { instance_id, keys } => {
                self.coordinator.send_keys(&instance_id, &keys).map(|_| {
                    ToolPayload::Status(ToolStatusPayload {
                        status: ToolStatus::Sent,
                    })
                })
            }
            ToolRequest::SendKey {
                instance_id,
                key,
                repeat,
            } => self
                .coordinator
                .send_key(&instance_id, key, repeat)
                .map(|_| {
                    ToolPayload::Status(ToolStatusPayload {
                        status: ToolStatus::Sent,
                    })
                }),
            ToolRequest::ActivateControl {
                instance_id,
                control_id,
            } => self
                .coordinator
                .activate_control(&instance_id, control_id)
                .map(|_| {
                    ToolPayload::Status(ToolStatusPayload {
                        status: ToolStatus::Activated,
                    })
                }),
            ToolRequest::ActivateListItem {
                instance_id,
                list_id,
                item_id,
            } => self
                .coordinator
                .activate_list_item(&instance_id, list_id, &item_id)
                .map(|_| {
                    ToolPayload::Status(ToolStatusPayload {
                        status: ToolStatus::Activated,
                    })
                }),
            ToolRequest::CreateContactInvitation {
                instance_id,
                receiver_authority_id,
            } => self
                .coordinator
                .create_contact_invitation(&instance_id, &receiver_authority_id)
                .map(|code| {
                    ToolPayload::ContactInvitationCreated(ContactInvitationCreatedPayload {
                        status: ToolStatus::Created,
                        code,
                    })
                }),
            ToolRequest::ClickButton {
                instance_id,
                label,
                selector,
            } => {
                let result = if let Some(selector) = selector.as_deref() {
                    self.coordinator.click_target(&instance_id, selector)
                } else {
                    self.coordinator.click_button(&instance_id, &label)
                };
                result.map(|_| {
                    ToolPayload::Status(ToolStatusPayload {
                        status: ToolStatus::Clicked,
                    })
                })
            }
            ToolRequest::FillInput {
                instance_id,
                selector,
                value,
            } => self
                .coordinator
                .fill_input(&instance_id, &selector, &value)
                .map(|_| {
                    ToolPayload::Status(ToolStatusPayload {
                        status: ToolStatus::Filled,
                    })
                }),
            ToolRequest::FillField {
                instance_id,
                field_id,
                value,
            } => self
                .coordinator
                .fill_field(&instance_id, field_id, &value)
                .map(|_| {
                    ToolPayload::Status(ToolStatusPayload {
                        status: ToolStatus::Filled,
                    })
                }),
            ToolRequest::WaitFor {
                instance_id,
                pattern,
                timeout_ms,
                screen_source,
                selector,
            } => self
                .coordinator
                .wait_for_diagnostic_observation(
                    &instance_id,
                    if let Some(selector) = selector.as_deref() {
                        DiagnosticObservationWait::Target { selector }
                    } else {
                        DiagnosticObservationWait::Pattern {
                            pattern: &pattern,
                            source: screen_source,
                        }
                    },
                    timeout_ms,
                )
                .map(|screen| {
                    ToolPayload::DiagnosticScreenCapture(DiagnosticScreenCapture::matched(
                        screen,
                        screen_source,
                    ))
                }),
            ToolRequest::TailLog { instance_id, lines } => self
                .coordinator
                .tail_log(
                    &instance_id,
                    match usize::try_from(lines) {
                        Ok(lines) => lines,
                        Err(_) => {
                            return ToolResponse::Error {
                                message: format!("tail_log lines out of range: {lines}"),
                            };
                        }
                    },
                )
                .map(|lines| ToolPayload::TailLog(TailLogPayload { lines })),
            ToolRequest::ReadClipboard { instance_id } => self
                .coordinator
                .read_clipboard(&instance_id)
                .map(|text| ToolPayload::Clipboard(ClipboardPayload { text })),
            ToolRequest::PrepareDeviceEnrollmentSetup { instance_id } => self
                .coordinator
                .prepare_device_enrollment_setup(&instance_id)
                .map(|setup_code| {
                    ToolPayload::DeviceEnrollmentSetup(DeviceEnrollmentSetupPayload { setup_code })
                }),
            ToolRequest::GetAuthorityId { instance_id } => self
                .coordinator
                .get_authority_id(&instance_id)
                .and_then(|authority_id| match authority_id {
                    Some(authority_id) => Ok(ToolPayload::AuthorityId(AuthorityIdPayload {
                        authority_id,
                        source: AuthorityIdSource::Backend,
                    })),
                    None => {
                        anyhow::bail!("authoritative authority id is unavailable for {instance_id}")
                    }
                }),
            ToolRequest::DiagnosticListChannels { instance_id } => self
                .coordinator
                .diagnostic_screen(&instance_id)
                .map(|screen| {
                    let channels = extract_channels(&screen);
                    ToolPayload::DiagnosticChannels(DiagnosticChannelListPayload {
                        diagnostic_channels: channels,
                    })
                }),
            ToolRequest::DiagnosticCurrentSelection { instance_id } => self
                .coordinator
                .diagnostic_screen(&instance_id)
                .map(|screen| {
                    let selection = extract_current_selection(&screen);
                    ToolPayload::DiagnosticSelection(DiagnosticSelectionPayload {
                        diagnostic_selection: selection,
                    })
                }),
            ToolRequest::DiagnosticListContacts { instance_id } => self
                .coordinator
                .diagnostic_screen(&instance_id)
                .map(|screen| {
                    let contacts = extract_contacts(&screen);
                    let toast = extract_toast(&screen);
                    ToolPayload::DiagnosticContacts(DiagnosticContactListPayload {
                        diagnostic_contacts: contacts,
                        diagnostic_toast: toast,
                    })
                }),
            ToolRequest::Restart { instance_id } => {
                self.coordinator.restart(&instance_id).map(|_| {
                    ToolPayload::Status(ToolStatusPayload {
                        status: ToolStatus::Restarted,
                    })
                })
            }
            ToolRequest::Kill { instance_id } => self.coordinator.kill(&instance_id).map(|_| {
                ToolPayload::Status(ToolStatusPayload {
                    status: ToolStatus::Killed,
                })
            }),
        };

        let response = match outcome {
            Ok(payload) => ToolResponse::Ok { payload },
            Err(error) => ToolResponse::Error {
                message: error.to_string(),
            },
        };

        self.action_log.push(ToolActionRecord {
            request: request_for_log,
            response: response.clone(),
        });

        response
    }

    pub fn event_snapshot(&self) -> Vec<crate::events::HarnessEvent> {
        self.coordinator.event_snapshot()
    }

    pub fn action_log(&self) -> Vec<ToolActionRecord> {
        self.action_log.clone()
    }

    pub fn negotiated_version(&self) -> &str {
        &self.negotiated_version
    }

    pub fn supported_versions() -> &'static [&'static str] {
        &TOOL_API_VERSIONS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_semantic_driver_regression_fixtures() {
        use aura_app::scenario_contract::{SemanticSubmissionHandle, UiOperationHandle};
        use aura_app::ui::contract::{OperationId, OperationInstanceId};

        let receipt = |intent: IntentAction, operation_id: OperationId| {
            let mut response = SemanticCommandResponse::accepted_without_value();
            response.handle = SemanticSubmissionHandle {
                ui_operation: Some(UiOperationHandle::new(
                    operation_id,
                    OperationInstanceId("original-7".into()),
                )),
            };
            serde_json::to_string(&ToolResponse::Ok {
                payload: ToolPayload::SemanticCommand(Box::new(
                    SemanticCommandPayload::new(intent.contract(), response)
                        .expect("canonical fixture receipt"),
                )),
            })
            .expect("serialize real wire receipt")
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output = std::process::Command::new("bash")
            .arg(root.join("scripts/harness/lan/test-semantic.sh"))
            .current_dir(&root)
            .env("AURA_E2E_ROOT", &root)
            .env(
                "AURA_LAN_FIXTURE_RECEIPT",
                receipt(
                    IntentAction::AcceptContactInvitation {
                        code: "original-code".into(),
                    },
                    OperationId::invitation_accept_contact(),
                ),
            )
            .env(
                "AURA_LAN_FIXTURE_IMMEDIATE_RECEIPT",
                receipt(
                    IntentAction::CreateAccount {
                        account_name: "Alice".into(),
                    },
                    OperationId::account_create(),
                ),
            )
            .env_remove("BASH_ENV")
            .output()
            .expect("run deterministic LAN driver fixtures");
        assert!(
            output.status.success(),
            "LAN driver fixtures failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn semantic_wire_round_trip_preserves_intent_and_operation_instance() {
        use aura_app::scenario_contract::{SemanticSubmissionHandle, UiOperationHandle};
        use aura_app::ui::contract::{OperationId, OperationInstanceId};
        let request = ToolRequest::SubmitSemanticCommand {
            instance_id: "alice".into(),
            intent: IntentAction::AcceptContactInvitation {
                code: "original-code".into(),
            },
        };
        let encoded = serde_json::to_string(&request).expect("encode request");
        let decoded: ToolRequest = serde_json::from_str(&encoded).expect("decode request");
        assert_eq!(decoded, request);
        let mut receipt = SemanticCommandResponse::accepted_without_value();
        receipt.handle = SemanticSubmissionHandle {
            ui_operation: Some(UiOperationHandle::new(
                OperationId::invitation_accept_contact(),
                OperationInstanceId("original-instance-7".into()),
            )),
        };
        let response = ToolResponse::Ok {
            payload: ToolPayload::SemanticCommand(Box::new(
                SemanticCommandPayload::new(
                    IntentAction::AcceptContactInvitation {
                        code: "code".into(),
                    }
                    .contract(),
                    receipt,
                )
                .expect("matching receipt"),
            )),
        };
        let encoded = serde_json::to_string(&response).expect("encode receipt");
        let decoded: ToolResponse = serde_json::from_str(&encoded).expect("decode receipt");
        assert_eq!(decoded, response);
    }

    #[test]
    fn canonical_submission_contract_rejects_missing_or_wrong_operation_handle() {
        use aura_app::scenario_contract::{SemanticSubmissionHandle, UiOperationHandle};
        use aura_app::ui::contract::{OperationId, OperationInstanceId};
        let contract = IntentAction::AcceptContactInvitation {
            code: "code".into(),
        }
        .contract();
        assert!(SemanticCommandPayload::new(
            contract.clone(),
            SemanticCommandResponse::accepted_without_value()
        )
        .is_err());
        let mut wrong = SemanticCommandResponse::accepted_without_value();
        wrong.handle = SemanticSubmissionHandle {
            ui_operation: Some(UiOperationHandle::new(
                OperationId::create_home(),
                OperationInstanceId("wrong".into()),
            )),
        };
        assert!(SemanticCommandPayload::new(contract, wrong).is_err());
        let immediate = IntentAction::CreateAccount {
            account_name: "Alice".into(),
        }
        .contract();
        assert!(SemanticCommandPayload::new(
            immediate,
            SemanticCommandResponse::accepted_without_value()
        )
        .is_ok());
    }

    #[test]
    fn snapshot_event_wire_preserves_backend_version_and_projection_revision() {
        let mut snapshot = UiSnapshot::loading(aura_app::ui::contract::ScreenId::Neighborhood);
        snapshot.revision.semantic_seq = 37;
        snapshot.revision.render_seq = Some(42);
        for event in [
            Some(UiSnapshotEvent {
                snapshot,
                version: 91,
            }),
            None,
        ] {
            let response = ToolResponse::Ok {
                payload: ToolPayload::UiSnapshotEvent(Box::new(UiSnapshotEventPayload { event })),
            };
            let encoded = serde_json::to_string(&response).expect("encode event");
            let decoded: ToolResponse = serde_json::from_str(&encoded).expect("decode event");
            assert_eq!(decoded, response);
        }
        let request = ToolRequest::WaitForUiSnapshotEvent {
            instance_id: "alice".into(),
            timeout_ms: 250,
            after_version: Some(91),
        };
        let encoded = serde_json::to_string(&request).expect("encode wait");
        assert_eq!(
            serde_json::from_str::<ToolRequest>(&encoded).expect("decode wait"),
            request
        );
    }

    #[test]
    fn instance_metadata_returns_exact_owned_profile_without_starting_runtime() {
        let temp = tempfile::tempdir().expect("profile root");
        let profile = temp.path().join("actor-profile");
        let config: RunConfig = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "run": { "name": "metadata-tool-test", "artifact_dir": temp.path().join("artifacts") },
            "instances": [{ "id": "alice", "mode": "local", "data_dir": profile, "bind_address": "127.0.0.1:41001" }],
        })).expect("config");
        let mut api =
            ToolApi::new(HarnessCoordinator::from_run_config(&config).expect("coordinator"));
        let response = api.handle_request(ToolRequest::InstanceMetadata {
            instance_id: "alice".into(),
        });
        let expected = ToolResponse::Ok {
            payload: ToolPayload::InstanceMetadata(InstanceMetadataPayload {
                instance_id: "alice".into(),
                data_dir: profile.clone(),
            }),
        };
        assert_eq!(response, expected);
        let encoded = serde_json::to_string(&response).expect("encode metadata");
        assert_eq!(
            serde_json::from_str::<ToolResponse>(&encoded).expect("decode metadata"),
            expected
        );
        assert!(
            !profile.exists(),
            "metadata must not create or open a profile"
        );
    }

    #[test]
    fn semantic_wire_dispatch_fails_closed_and_records_exact_request() {
        let temp = tempfile::tempdir().expect("dispatch fixture root");
        let mut config: RunConfig = toml::from_str(
            "schema_version = 1\ninstances = []\n[run]\nname = 'tool-wire-dispatch'\n",
        )
        .expect("config");
        config.run.artifact_dir = Some(temp.path().join("artifacts"));
        let mut api =
            ToolApi::new(HarnessCoordinator::from_run_config(&config).expect("coordinator"));
        for request in [
            ToolRequest::InstanceMetadata {
                instance_id: "missing".into(),
            },
            ToolRequest::SubmitSemanticCommand {
                instance_id: "missing".into(),
                intent: IntentAction::CreateAccount {
                    account_name: "Alice".into(),
                },
            },
            ToolRequest::WaitForUiSnapshotEvent {
                instance_id: "missing".into(),
                timeout_ms: 0,
                after_version: Some(91),
            },
        ] {
            let encoded = serde_json::to_string(&request).expect("encode request");
            let decoded = serde_json::from_str(&encoded).expect("decode request");
            let response = api.handle_request(decoded);
            assert_eq!(
                response,
                ToolResponse::Error {
                    message: "unknown instance_id: missing".into()
                }
            );
            assert_eq!(
                api.action_log().last(),
                Some(&ToolActionRecord { request, response })
            );
        }
    }

    #[test]
    fn diagnostic_screen_capture_serializes_explicit_diagnostic_field_names() {
        let payload = serde_json::to_value(DiagnosticScreenCapture::settled(
            "  Chat  ".to_string(),
            ScreenSource::Default,
        ))
        .unwrap_or_else(|error| panic!("failed to encode diagnostic capture: {error}"));
        assert!(payload.get("diagnostic_authoritative_screen").is_some());
        assert!(payload.get("diagnostic_raw_screen").is_some());
        assert!(payload.get("diagnostic_normalized_screen").is_some());
        assert!(payload.get("screen").is_none());
        assert!(payload.get("raw_screen").is_none());
        assert!(payload.get("authoritative_screen").is_none());
        assert!(payload.get("normalized_screen").is_none());
    }

    #[test]
    fn tool_response_round_trips_typed_payloads() {
        let response = ToolResponse::Ok {
            payload: ToolPayload::Clipboard(ClipboardPayload {
                text: "clipboard-value".to_string(),
            }),
        };

        let encoded = serde_json::to_string(&response)
            .unwrap_or_else(|error| panic!("failed to encode tool response: {error}"));
        let decoded: ToolResponse = serde_json::from_str(&encoded)
            .unwrap_or_else(|error| panic!("failed to decode tool response: {error}"));

        assert_eq!(decoded, response);
    }

    #[test]
    fn ui_snapshot_tool_response_preserves_subscription_failure() {
        let mut snapshot = UiSnapshot::loading(aura_app::ui_contract::ScreenId::Neighborhood);
        snapshot
            .subscription_health
            .push(aura_app::ui_contract::SubscriptionHealthSnapshot {
                signal: "contacts".to_string(),
                state: aura_app::ui_contract::SubscriptionHealthState::Degraded {
                    reason: aura_app::ui_contract::SubscriptionFailureCode::StreamClosed,
                },
            });
        let response = ToolResponse::Ok {
            payload: ToolPayload::UiSnapshot(Box::new(snapshot)),
        };
        let encoded = serde_json::to_string(&response).expect("encode tool response");
        let decoded: ToolResponse = serde_json::from_str(&encoded).expect("decode tool response");
        assert_eq!(decoded, response);
        assert!(encoded.contains("subscription_health"));
    }

    #[test]
    fn tool_api_uses_explicit_diagnostic_observation_methods() {
        let source = include_str!("tool_api.rs");
        assert!(source.contains(".diagnostic_screen_with_source("));
        assert!(source.contains(".wait_for_diagnostic_observation("));
        assert!(source.contains("DiagnosticObservationWait::Target"));
        assert!(source.contains("DiagnosticObservationWait::Pattern"));
        assert!(source.contains(".ui_snapshot(&instance_id)"));
        assert!(source.contains(".wait_for_ui_snapshot_event(instance_id, timeout, after_version)"));
    }
}
