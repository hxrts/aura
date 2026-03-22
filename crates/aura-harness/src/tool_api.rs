//! Tool API for harness-client RPC communication.
//!
//! Defines request/response types and dispatch logic for the harness tool API,
//! enabling test clients to send input, capture screens, and query instance state.

use aura_app::ui::contract::{ControlId, FieldId, ListId, UiSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api_version::{negotiate, TOOL_API_DEFAULT_VERSION, TOOL_API_VERSIONS};
use crate::backend::{SemanticCommandRequest, SemanticCommandResponse};
use crate::config::{RunConfig, RuntimeSubstrate, ScreenSource};
use crate::coordinator::HarnessCoordinator;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityIdSource {
    Backend,
    PreparedInviteeAuthority,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolPayload {
    Negotiation(ToolNegotiationPayload),
    DiagnosticScreenCapture(DiagnosticScreenCapture),
    UiSnapshot(UiSnapshot),
    Status(ToolStatusPayload),
    ContactInvitationCreated(ContactInvitationCreatedPayload),
    TailLog(TailLogPayload),
    Clipboard(ClipboardPayload),
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
    PrepareDeviceEnrollmentInviteeAuthority {
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

    pub fn prepare_device_enrollment_invitee_authority(
        &mut self,
        instance_id: &str,
    ) -> anyhow::Result<String> {
        self.coordinator
            .prepare_device_enrollment_invitee_authority(instance_id)
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
    ) -> anyhow::Result<Option<(UiSnapshot, u64)>> {
        self.coordinator
            .wait_for_ui_snapshot_event(instance_id, timeout, after_version)
            .map(|event| event.map(|event| (event.snapshot, event.version)))
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
                .map(ToolPayload::UiSnapshot),
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
            } => {
                let result = if let Some(selector) = selector.as_deref() {
                    self.coordinator
                        .wait_for_diagnostic_target(&instance_id, selector, timeout_ms)
                } else {
                    self.coordinator.wait_for_diagnostic_screen_with_source(
                        &instance_id,
                        &pattern,
                        timeout_ms,
                        screen_source,
                    )
                };
                result.map(|screen| {
                    ToolPayload::DiagnosticScreenCapture(DiagnosticScreenCapture::matched(
                        screen,
                        screen_source,
                    ))
                })
            }
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
            ToolRequest::PrepareDeviceEnrollmentInviteeAuthority { instance_id } => self
                .coordinator
                .prepare_device_enrollment_invitee_authority(&instance_id)
                .map(|authority_id| {
                    ToolPayload::AuthorityId(AuthorityIdPayload {
                        authority_id,
                        source: AuthorityIdSource::PreparedInviteeAuthority,
                    })
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
    fn tool_api_uses_explicit_diagnostic_observation_methods() {
        let source = include_str!("tool_api.rs");
        assert!(source.contains(".diagnostic_screen_with_source("));
        assert!(source.contains(".wait_for_diagnostic_screen_with_source("));
        assert!(source.contains(".wait_for_diagnostic_target("));
        assert!(source.contains(".ui_snapshot(&instance_id)"));
        assert!(source.contains(".wait_for_ui_snapshot_event(instance_id, timeout, after_version)"));
    }
}
