//! Tool API for harness-client RPC communication.
//!
//! Defines request/response types and dispatch logic for the harness tool API,
//! enabling test clients to send input, capture screens, and query instance state.

use aura_app::ui::contract::{ControlId, FieldId, ListId, UiSnapshot};
use serde::{Deserialize, Serialize};

use crate::api_version::{negotiate, TOOL_API_DEFAULT_VERSION, TOOL_API_VERSIONS};
use crate::config::{RunConfig, RuntimeSubstrate, ScreenSource};
use crate::coordinator::HarnessCoordinator;
use crate::introspection::{
    extract_authority_id, extract_channels, extract_contacts, extract_current_selection,
    extract_toast,
};
use crate::screen_normalization::{authoritative_screen, normalize_screen};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    GetAuthorityId {
        instance_id: String,
    },
    ListChannels {
        instance_id: String,
    },
    CurrentSelection {
        instance_id: String,
    },
    ListContacts {
        instance_id: String,
    },
    Restart {
        instance_id: String,
    },
    Kill {
        instance_id: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolResponse {
    Ok { payload: serde_json::Value },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
                    serde_json::json!({
                        "negotiated_version": result.negotiated_version,
                        "supported_versions": result.supported_versions
                    })
                })
            }
            ToolRequest::Screen {
                instance_id,
                screen_source,
            } => self
                .coordinator
                .screen_with_source(&instance_id, screen_source)
                .map(|screen| {
                    let authoritative = authoritative_screen(&screen);
                    let normalized = normalize_screen(&screen);
                    serde_json::json!({
                        "screen": &authoritative,
                        "raw_screen": screen,
                        "authoritative_screen": &authoritative,
                        "normalized_screen": normalized,
                        "capture_consistency": "settled",
                        "screen_source": format!("{screen_source:?}").to_ascii_lowercase()
                    })
                }),
            ToolRequest::UiState { instance_id } => self
                .coordinator
                .ui_snapshot(&instance_id)
                .and_then(|snapshot: UiSnapshot| serde_json::to_value(snapshot).map_err(Into::into)),
            ToolRequest::SendKeys { instance_id, keys } => self
                .coordinator
                .send_keys(&instance_id, &keys)
                .map(|_| serde_json::json!({ "status": "sent" })),
            ToolRequest::SendKey {
                instance_id,
                key,
                repeat,
            } => self
                .coordinator
                .send_key(&instance_id, key, repeat)
                .map(|_| serde_json::json!({ "status": "sent" })),
            ToolRequest::ActivateControl {
                instance_id,
                control_id,
            } => self
                .coordinator
                .activate_control(&instance_id, control_id)
                .map(|_| serde_json::json!({ "status": "activated" })),
            ToolRequest::ActivateListItem {
                instance_id,
                list_id,
                item_id,
            } => self
                .coordinator
                .activate_list_item(&instance_id, list_id, &item_id)
                .map(|_| serde_json::json!({ "status": "activated" })),
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
                result.map(|_| serde_json::json!({ "status": "clicked" }))
            }
            ToolRequest::FillInput {
                instance_id,
                selector,
                value,
            } => self
                .coordinator
                .fill_input(&instance_id, &selector, &value)
                .map(|_| serde_json::json!({ "status": "filled" })),
            ToolRequest::FillField {
                instance_id,
                field_id,
                value,
            } => self
                .coordinator
                .fill_field(&instance_id, field_id, &value)
                .map(|_| serde_json::json!({ "status": "filled" })),
            ToolRequest::WaitFor {
                instance_id,
                pattern,
                timeout_ms,
                screen_source,
                selector,
            } => {
                let result = if let Some(selector) = selector.as_deref() {
                    self.coordinator
                        .wait_for_selector(&instance_id, selector, timeout_ms)
                } else {
                    self.coordinator.wait_for_with_source(
                        &instance_id,
                        &pattern,
                        timeout_ms,
                        screen_source,
                    )
                };
                result.map(|screen| {
                    let authoritative = authoritative_screen(&screen);
                    let normalized = normalize_screen(&screen);
                    serde_json::json!({
                        "matched": true,
                        "screen": &authoritative,
                        "raw_screen": screen,
                        "authoritative_screen": &authoritative,
                        "normalized_screen": normalized,
                        "matched_view": "normalized",
                        "screen_source": format!("{screen_source:?}").to_ascii_lowercase()
                    })
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
                .map(|lines| serde_json::json!({ "lines": lines })),
            ToolRequest::ReadClipboard { instance_id } => self
                .coordinator
                .read_clipboard(&instance_id)
                .map(|text| serde_json::json!({ "text": text })),
            ToolRequest::GetAuthorityId { instance_id } => self
                .coordinator
                .get_authority_id(&instance_id)
                .and_then(|authority_id| {
                    if let Some(authority_id) = authority_id {
                        return Ok(serde_json::json!({
                            "authority_id": authority_id,
                            "source": "backend"
                        }));
                    }

                    self.coordinator.screen(&instance_id).and_then(|screen| {
                        if let Some(authority_id) = extract_authority_id(&screen) {
                            return Ok(serde_json::json!({
                                "authority_id": authority_id,
                                "source": "screen"
                            }));
                        }

                        self.coordinator
                            .resolve_authority_id_from_local_state(&instance_id)
                            .map(|authority_id| {
                                serde_json::json!({
                                    "authority_id": authority_id,
                                    "source": "local_state"
                                })
                            })
                    })
                }),
            ToolRequest::ListChannels { instance_id } => {
                self.coordinator.screen(&instance_id).map(|screen| {
                    let channels = extract_channels(&screen);
                    serde_json::json!({ "channels": channels })
                })
            }
            ToolRequest::CurrentSelection { instance_id } => {
                self.coordinator.screen(&instance_id).map(|screen| {
                    let selection = extract_current_selection(&screen);
                    serde_json::json!({ "selection": selection })
                })
            }
            ToolRequest::ListContacts { instance_id } => {
                self.coordinator.screen(&instance_id).map(|screen| {
                    let contacts = extract_contacts(&screen);
                    let toast = extract_toast(&screen);
                    serde_json::json!({ "contacts": contacts, "toast": toast })
                })
            }
            ToolRequest::Restart { instance_id } => self
                .coordinator
                .restart(&instance_id)
                .map(|_| serde_json::json!({ "status": "restarted" })),
            ToolRequest::Kill { instance_id } => self
                .coordinator
                .kill(&instance_id)
                .map(|_| serde_json::json!({ "status": "killed" })),
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
