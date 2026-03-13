//! Legacy scripted scenario language retained only for quarantined non-shared scenarios.
//!
//! Shared scenarios execute from the canonical semantic contract and do not lower through
//! this compatibility step language.

use std::fmt;

use anyhow::{anyhow, bail, Result};
use aura_app::scenario_contract::{
    ActorId, EnvironmentAction, Expectation, ExtractSource, InputKey, IntentAction,
    ScenarioAction as SemanticAction, ScenarioStep as SemanticStep, SettingsSection, UiAction,
    VariableAction,
};
use aura_app::ui::contract::{
    ConfirmationState, ControlId, FieldId, ListId, ModalId, OperationId, OperationState,
    RuntimeEventKind, ScreenId, ToastKind, UiReadiness,
};
use aura_app::ui_contract::QuiescenceState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioAction {
    LaunchInstances,
    #[default]
    Noop,
    SetVar,
    CaptureCurrentAuthorityId,
    CaptureSelection,
    ExtractVar,
    CreateAccount,
    CreateHome,
    StartDeviceEnrollment,
    ImportDeviceEnrollmentCode,
    RemoveSelectedDevice,
    CreateContactInvitation,
    AcceptContactInvitation,
    JoinChannel,
    InviteActorToChannel,
    AcceptPendingChannelInvitation,
    SendKeys,
    SendChatCommand,
    SendChatMessage,
    SendClipboard,
    ReadClipboard,
    DismissTransient,
    SendKey,
    ClickButton,
    FillInput,
    AssertParity,
    WaitFor,
    MessageContains,
    ExpectToast,
    ExpectCommandResult,
    ExpectMembership,
    ExpectDenied,
    GetAuthorityId,
    ListChannels,
    CurrentSelection,
    ListContacts,
    SelectChannel,
    Restart,
    Kill,
    FaultDelay,
    FaultLoss,
    FaultTunnelDrop,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScreenSource {
    #[default]
    Default,
    Dom,
}

impl fmt::Display for ScenarioAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::LaunchInstances => "launch_instances",
            Self::Noop => "noop",
            Self::SetVar => "set_var",
            Self::CaptureCurrentAuthorityId => "capture_current_authority_id",
            Self::CaptureSelection => "capture_selection",
            Self::ExtractVar => "extract_var",
            Self::CreateAccount => "create_account",
            Self::CreateHome => "create_home",
            Self::StartDeviceEnrollment => "start_device_enrollment",
            Self::ImportDeviceEnrollmentCode => "import_device_enrollment_code",
            Self::RemoveSelectedDevice => "remove_selected_device",
            Self::CreateContactInvitation => "create_contact_invitation",
            Self::AcceptContactInvitation => "accept_contact_invitation",
            Self::JoinChannel => "join_channel",
            Self::InviteActorToChannel => "invite_actor_to_channel",
            Self::AcceptPendingChannelInvitation => "accept_pending_channel_invitation",
            Self::SendKeys => "send_keys",
            Self::SendChatCommand => "send_chat_command",
            Self::SendChatMessage => "send_chat_message",
            Self::SendClipboard => "send_clipboard",
            Self::ReadClipboard => "read_clipboard",
            Self::DismissTransient => "dismiss_transient",
            Self::SendKey => "send_key",
            Self::ClickButton => "click_button",
            Self::FillInput => "fill_input",
            Self::AssertParity => "assert_parity",
            Self::WaitFor => "wait_for",
            Self::MessageContains => "message_contains",
            Self::ExpectToast => "expect_toast",
            Self::ExpectCommandResult => "expect_command_result",
            Self::ExpectMembership => "expect_membership",
            Self::ExpectDenied => "expect_denied",
            Self::GetAuthorityId => "get_authority_id",
            Self::ListChannels => "list_channels",
            Self::CurrentSelection => "current_selection",
            Self::ListContacts => "list_contacts",
            Self::SelectChannel => "select_channel",
            Self::Restart => "restart",
            Self::Kill => "kill",
            Self::FaultDelay => "fault_delay",
            Self::FaultLoss => "fault_loss",
            Self::FaultTunnelDrop => "fault_tunnel_drop",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioStep {
    pub id: String,
    pub action: ScenarioAction,
    pub instance: Option<String>,
    // Backward-compatible overloaded field used by scripted actions.
    // Prefer action-specific fields (`keys`, `command`, `pattern`, `key`,
    // `source_instance`) in new scenarios to keep intent explicit.
    pub expect: Option<String>,
    pub timeout_ms: Option<u64>,
    /// Optional pipeline request id used by strict scenario execution ordering.
    pub request_id: Option<u64>,
    /// Explicit key stream for `send_keys`.
    pub keys: Option<String>,
    /// Screen observation source for browser-oriented waits/assertions.
    pub screen_source: Option<ScreenSource>,
    /// Explicit slash-command body for `send_chat_command`.
    pub command: Option<String>,
    /// Explicit screen pattern for `wait_for`.
    pub pattern: Option<String>,
    /// Explicit named key for `send_key`.
    pub key: Option<String>,
    /// Visible button label for `click_button` when no selector is supplied.
    pub label: Option<String>,
    /// CSS selector for `fill_input` and optional stable target selector for `click_button`.
    pub selector: Option<String>,
    /// Semantic screen reference for typed scenario actions/expectations.
    pub screen_id: Option<ScreenId>,
    /// Semantic control reference for typed scenario actions/expectations.
    pub control_id: Option<ControlId>,
    /// Semantic field reference for typed scenario actions.
    pub field_id: Option<FieldId>,
    /// Semantic modal reference for typed scenario expectations.
    pub modal_id: Option<ModalId>,
    /// Semantic readiness reference for typed scenario expectations.
    pub readiness: Option<UiReadiness>,
    /// Semantic quiescence reference for typed scenario expectations.
    pub quiescence: Option<QuiescenceState>,
    /// Semantic runtime-event reference for typed scenario expectations.
    pub runtime_event_kind: Option<RuntimeEventKind>,
    /// Semantic operation identifier for typed lifecycle expectations.
    pub operation_id: Option<OperationId>,
    /// Semantic operation state for typed lifecycle expectations.
    pub operation_state: Option<OperationState>,
    /// Semantic list reference for typed scenario expectations.
    pub list_id: Option<ListId>,
    /// Stable list item identifier for typed list expectations.
    pub item_id: Option<String>,
    /// Expected list size for typed list expectations.
    pub count: Option<usize>,
    /// Expected confirmation state for typed list expectations.
    pub confirmation: Option<ConfirmationState>,
    /// Repeat count for `send_key` actions.
    pub repeat: Option<u16>,
    /// Explicit source instance for `send_clipboard`.
    pub source_instance: Option<String>,
    /// Peer instance identifier for semantic parity assertions.
    pub peer_instance: Option<String>,
    /// Variable identifier for set/extract/introspection actions.
    pub var: Option<String>,
    /// Static or templated value for `set_var`.
    pub value: Option<String>,
    /// Regular expression for `extract_var`.
    pub regex: Option<String>,
    /// Capture group index for `extract_var`.
    pub group: Option<u32>,
    /// Source field for `extract_var`: `screen`, `raw_screen`, `authoritative_screen`, or `normalized_screen`.
    pub from: Option<String>,
    /// Substring expectation for typed assertion actions.
    pub contains: Option<String>,
    /// Toast/assertion level (`success`, `info`, `error`) for typed assertions.
    pub level: Option<String>,
    /// Command outcome status (`ok`, `denied`, `invalid`, `failed`) for command assertions.
    pub status: Option<String>,
    /// Consistency label (`accepted`, `replicated`, `enforced`, `partial-timeout`) for command-result assertions.
    pub consistency: Option<String>,
    /// Stable command reason code (for normalized command outcomes).
    pub reason_code: Option<String>,
    /// Channel display name for membership assertions.
    pub channel: Option<String>,
    /// Expected selected state for membership assertions.
    pub selected: Option<bool>,
    /// Expected present state for membership assertions (defaults to true when omitted).
    pub present: Option<bool>,
    /// Denial reason discriminator for `expect_denied` (`permission`, `banned`, `muted`).
    pub reason: Option<String>,
    /// Additional allowed denial substrings for `expect_denied`.
    pub contains_any: Option<Vec<String>>,
}

impl ScenarioStep {
    pub fn to_semantic_step(&self) -> Result<Option<SemanticStep>> {
        let actor = self.instance.clone().map(ActorId);
        let timeout_ms = self.timeout_ms;
        let id = self.id.clone();

        let action = match self.action {
            ScenarioAction::LaunchInstances => {
                Some(SemanticAction::Environment(EnvironmentAction::LaunchActors))
            }
            ScenarioAction::Noop => None,
            ScenarioAction::SetVar => Some(SemanticAction::Variables(VariableAction::Set {
                name: required_field(self.var.clone(), "var", self.action)?,
                value: required_field(
                    self.value.clone().or_else(|| self.expect.clone()),
                    "value",
                    self.action,
                )?,
            })),
            ScenarioAction::CaptureCurrentAuthorityId => Some(SemanticAction::Variables(
                VariableAction::CaptureCurrentAuthorityId {
                    name: required_field(self.var.clone(), "var", self.action)?,
                },
            )),
            ScenarioAction::CaptureSelection => Some(SemanticAction::Variables(
                VariableAction::CaptureSelection {
                    name: required_field(self.var.clone(), "var", self.action)?,
                    list: self
                        .list_id
                        .ok_or_else(|| anyhow!("action {} requires list_id", self.action))?,
                },
            )),
            ScenarioAction::ExtractVar => {
                Some(SemanticAction::Variables(VariableAction::Extract {
                    name: required_field(self.var.clone(), "var", self.action)?,
                    regex: required_field(self.regex.clone(), "regex", self.action)?,
                    group: self.group.unwrap_or(0),
                    from: parse_extract_source(self.from.as_deref().unwrap_or("screen"))?,
                }))
            }
            ScenarioAction::CreateAccount => {
                Some(SemanticAction::Intent(IntentAction::CreateAccount {
                    account_name: required_field(
                        self.value.clone().or_else(|| self.expect.clone()),
                        "value",
                        self.action,
                    )?,
                }))
            }
            ScenarioAction::CreateHome => Some(SemanticAction::Intent(IntentAction::CreateHome {
                home_name: required_field(
                    self.value.clone().or_else(|| self.expect.clone()),
                    "value",
                    self.action,
                )?,
            })),
            ScenarioAction::StartDeviceEnrollment => Some(SemanticAction::Intent(
                IntentAction::StartDeviceEnrollment {
                    device_name: required_field(
                        self.value.clone().or_else(|| self.expect.clone()),
                        "value",
                        self.action,
                    )?,
                    code_name: required_field(self.var.clone(), "var", self.action)?,
                },
            )),
            ScenarioAction::ImportDeviceEnrollmentCode => Some(SemanticAction::Intent(
                IntentAction::ImportDeviceEnrollmentCode {
                    code: required_field(
                        self.value.clone().or_else(|| self.expect.clone()),
                        "value",
                        self.action,
                    )?,
                },
            )),
            ScenarioAction::RemoveSelectedDevice => {
                Some(SemanticAction::Intent(IntentAction::RemoveSelectedDevice))
            }
            ScenarioAction::CreateContactInvitation => Some(SemanticAction::Intent(
                IntentAction::CreateContactInvitation {
                    receiver_authority_id: required_field(
                        self.value.clone().or_else(|| self.expect.clone()),
                        "value",
                        self.action,
                    )?,
                    code_name: self.var.clone(),
                },
            )),
            ScenarioAction::AcceptContactInvitation => Some(SemanticAction::Intent(
                IntentAction::AcceptContactInvitation {
                    code: required_field(
                        self.value.clone().or_else(|| self.expect.clone()),
                        "value",
                        self.action,
                    )?,
                },
            )),
            ScenarioAction::InviteActorToChannel => {
                Some(SemanticAction::Intent(IntentAction::InviteActorToChannel {
                    authority_id: required_field(
                        self.value.clone().or_else(|| self.expect.clone()),
                        "value",
                        self.action,
                    )?,
                }))
            }
            ScenarioAction::AcceptPendingChannelInvitation => Some(SemanticAction::Intent(
                IntentAction::AcceptPendingChannelInvitation,
            )),
            ScenarioAction::JoinChannel => {
                Some(SemanticAction::Intent(IntentAction::JoinChannel {
                    channel_name: required_field(
                        self.value.clone().or_else(|| self.expect.clone()),
                        "value",
                        self.action,
                    )?,
                }))
            }
            ScenarioAction::SendKeys => {
                Some(SemanticAction::Ui(UiAction::InputText(required_field(
                    self.keys.clone().or_else(|| self.expect.clone()),
                    "keys",
                    self.action,
                )?)))
            }
            ScenarioAction::SendClipboard => Some(SemanticAction::Ui(UiAction::PasteClipboard {
                source_actor: self.source_instance.clone().map(ActorId),
            })),
            ScenarioAction::ReadClipboard => Some(SemanticAction::Ui(UiAction::ReadClipboard {
                name: required_field(self.var.clone(), "var", self.action)?,
            })),
            ScenarioAction::DismissTransient => {
                Some(SemanticAction::Ui(UiAction::DismissTransient))
            }
            ScenarioAction::SendKey => Some(SemanticAction::Ui(UiAction::PressKey(
                parse_input_key(self.key.as_deref().unwrap_or_default())?,
                self.repeat.unwrap_or(1).max(1),
            ))),
            ScenarioAction::SendChatCommand => Some(SemanticAction::Ui(UiAction::SendChatCommand(
                required_field(
                    self.command.clone().or_else(|| self.expect.clone()),
                    "command",
                    self.action,
                )?,
            ))),
            ScenarioAction::SendChatMessage => {
                Some(SemanticAction::Intent(IntentAction::SendChatMessage {
                    message: required_field(
                        self.value.clone().or_else(|| self.expect.clone()),
                        "value",
                        self.action,
                    )?,
                }))
            }
            ScenarioAction::ClickButton => {
                if let Some(screen_id) = self.control_id.and_then(screen_id_for_nav_control_id) {
                    Some(SemanticAction::Intent(IntentAction::OpenScreen(screen_id)))
                } else if let Some(control_id) = self.control_id {
                    Some(SemanticAction::Ui(UiAction::Activate(control_id)))
                } else if let (Some(list_id), Some(item_id)) = (self.list_id, self.item_id.clone())
                {
                    if list_id == ListId::SettingsSections {
                        settings_section_from_item_id(&item_id).map(|section| {
                            SemanticAction::Intent(IntentAction::OpenSettingsSection(section))
                        })
                    } else {
                        Some(SemanticAction::Ui(UiAction::ActivateListItem {
                            list: list_id,
                            item_id,
                        }))
                    }
                } else {
                    None
                }
            }
            ScenarioAction::FillInput => match self.field_id {
                Some(field_id) => Some(SemanticAction::Ui(UiAction::Fill(
                    field_id,
                    required_field(
                        self.value.clone().or_else(|| self.expect.clone()),
                        "value",
                        self.action,
                    )?,
                ))),
                None => None,
            },
            ScenarioAction::AssertParity => {
                Some(SemanticAction::Expect(Expectation::ParityWithActor {
                    actor: ActorId(required_field(
                        self.peer_instance.clone(),
                        "peer_instance",
                        self.action,
                    )?),
                }))
            }
            ScenarioAction::WaitFor => expectation_from_step(self)?,
            ScenarioAction::MessageContains => {
                Some(SemanticAction::Expect(Expectation::MessageContains {
                    message_contains: required_field(
                        self.value.clone().or_else(|| self.expect.clone()),
                        "value",
                        self.action,
                    )?,
                }))
            }
            ScenarioAction::ExpectToast => {
                Some(SemanticAction::Expect(Expectation::ToastContains {
                    kind: self.level.as_deref().map(parse_toast_kind).transpose()?,
                    message_contains: required_field(
                        self.contains.clone().or_else(|| self.expect.clone()),
                        "contains",
                        self.action,
                    )?,
                }))
            }
            ScenarioAction::Restart => Some(SemanticAction::Environment(
                EnvironmentAction::RestartActor {
                    actor: actor
                        .clone()
                        .ok_or_else(|| anyhow!("step {} requires instance", self.id))?,
                },
            )),
            ScenarioAction::Kill => {
                Some(SemanticAction::Environment(EnvironmentAction::KillActor {
                    actor: actor
                        .clone()
                        .ok_or_else(|| anyhow!("step {} requires instance", self.id))?,
                }))
            }
            ScenarioAction::FaultDelay => {
                Some(SemanticAction::Environment(EnvironmentAction::FaultDelay {
                    actor: actor
                        .clone()
                        .ok_or_else(|| anyhow!("step {} requires instance", self.id))?,
                    delay_ms: self.timeout_ms.unwrap_or_default(),
                }))
            }
            ScenarioAction::FaultLoss => {
                Some(SemanticAction::Environment(EnvironmentAction::FaultLoss {
                    actor: actor
                        .clone()
                        .ok_or_else(|| anyhow!("step {} requires instance", self.id))?,
                    loss_percent: 100,
                }))
            }
            ScenarioAction::FaultTunnelDrop => Some(SemanticAction::Environment(
                EnvironmentAction::FaultTunnelDrop {
                    actor: actor
                        .clone()
                        .ok_or_else(|| anyhow!("step {} requires instance", self.id))?,
                },
            )),
            ScenarioAction::ExpectCommandResult
            | ScenarioAction::ExpectMembership
            | ScenarioAction::ExpectDenied
            | ScenarioAction::GetAuthorityId
            | ScenarioAction::ListChannels
            | ScenarioAction::CurrentSelection
            | ScenarioAction::ListContacts
            | ScenarioAction::SelectChannel => None,
        };

        Ok(action.map(|action| SemanticStep {
            id,
            actor,
            timeout_ms,
            action,
        }))
    }
}

fn required_field(value: Option<String>, field: &str, action: ScenarioAction) -> Result<String> {
    value.ok_or_else(|| anyhow!("action {action} requires {field}"))
}

pub(crate) fn nav_control_id_for_screen(screen_id: ScreenId) -> ControlId {
    match screen_id {
        ScreenId::Onboarding => ControlId::OnboardingRoot,
        ScreenId::Neighborhood => ControlId::NavNeighborhood,
        ScreenId::Chat => ControlId::NavChat,
        ScreenId::Contacts => ControlId::NavContacts,
        ScreenId::Notifications => ControlId::NavNotifications,
        ScreenId::Settings => ControlId::NavSettings,
    }
}

fn screen_id_for_nav_control_id(control_id: ControlId) -> Option<ScreenId> {
    match control_id {
        ControlId::OnboardingRoot => Some(ScreenId::Onboarding),
        ControlId::NavNeighborhood => Some(ScreenId::Neighborhood),
        ControlId::NavChat => Some(ScreenId::Chat),
        ControlId::NavContacts => Some(ScreenId::Contacts),
        ControlId::NavNotifications => Some(ScreenId::Notifications),
        ControlId::NavSettings => Some(ScreenId::Settings),
        _ => None,
    }
}

pub(crate) fn settings_section_item_id(section: SettingsSection) -> &'static str {
    match section {
        SettingsSection::Devices => "devices",
    }
}

fn settings_section_from_item_id(item_id: &str) -> Option<SettingsSection> {
    match item_id.replace('_', "-").as_str() {
        "devices" => Some(SettingsSection::Devices),
        _ => None,
    }
}

fn parse_extract_source(value: &str) -> Result<ExtractSource> {
    match value.trim().to_ascii_lowercase().as_str() {
        "screen" => Ok(ExtractSource::Screen),
        "raw_screen" => Ok(ExtractSource::RawScreen),
        "authoritative_screen" => Ok(ExtractSource::AuthoritativeScreen),
        "normalized_screen" => Ok(ExtractSource::NormalizedScreen),
        other => bail!("unsupported extract source: {other}"),
    }
}

fn parse_input_key(value: &str) -> Result<InputKey> {
    match value.trim().to_ascii_lowercase().as_str() {
        "enter" => Ok(InputKey::Enter),
        "esc" => Ok(InputKey::Esc),
        "tab" => Ok(InputKey::Tab),
        "backtab" | "back_tab" | "shift_tab" => Ok(InputKey::BackTab),
        "up" => Ok(InputKey::Up),
        "down" => Ok(InputKey::Down),
        "left" => Ok(InputKey::Left),
        "right" => Ok(InputKey::Right),
        "home" => Ok(InputKey::Home),
        "end" => Ok(InputKey::End),
        "pageup" | "page_up" => Ok(InputKey::PageUp),
        "pagedown" | "page_down" => Ok(InputKey::PageDown),
        "backspace" => Ok(InputKey::Backspace),
        "delete" => Ok(InputKey::Delete),
        other => bail!("unsupported semantic key: {other}"),
    }
}

fn parse_toast_kind(value: &str) -> Result<ToastKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "success" => Ok(ToastKind::Success),
        "info" => Ok(ToastKind::Info),
        "error" => Ok(ToastKind::Error),
        other => bail!("unsupported toast kind: {other}"),
    }
}

fn expectation_from_step(step: &ScenarioStep) -> Result<Option<SemanticAction>> {
    if matches!(step.action, ScenarioAction::MessageContains) {
        return Ok(Some(SemanticAction::Expect(Expectation::MessageContains {
            message_contains: required_field(
                step.value.clone().or_else(|| step.expect.clone()),
                "value",
                step.action,
            )?,
        })));
    }
    if step.contains.is_some() || step.level.is_some() {
        return Ok(Some(SemanticAction::Expect(Expectation::ToastContains {
            kind: step.level.as_deref().map(parse_toast_kind).transpose()?,
            message_contains: required_field(
                step.contains.clone().or_else(|| step.expect.clone()),
                "contains",
                step.action,
            )?,
        })));
    }
    if let Some(screen_id) = step.screen_id {
        return Ok(Some(SemanticAction::Expect(Expectation::ScreenIs(
            screen_id,
        ))));
    }
    if let Some(control_id) = step.control_id {
        return Ok(Some(SemanticAction::Expect(Expectation::ControlVisible(
            control_id,
        ))));
    }
    if let Some(modal_id) = step.modal_id {
        return Ok(Some(SemanticAction::Expect(Expectation::ModalOpen(
            modal_id,
        ))));
    }
    if let Some(readiness) = step.readiness {
        return Ok(Some(SemanticAction::Expect(Expectation::ReadinessIs(
            readiness,
        ))));
    }
    if let Some(kind) = step.runtime_event_kind {
        return Ok(Some(SemanticAction::Expect(
            Expectation::RuntimeEventOccurred {
                kind,
                detail_contains: step.contains.clone().or_else(|| step.expect.clone()),
                capture_name: step.var.clone(),
            },
        )));
    }
    if let (Some(operation_id), Some(state)) = (step.operation_id.clone(), step.operation_state) {
        return Ok(Some(SemanticAction::Expect(
            Expectation::OperationStateIs {
                operation_id,
                state,
            },
        )));
    }
    if let (Some(list_id), Some(item_id)) = (step.list_id, step.item_id.clone()) {
        if let Some(confirmation) = step.confirmation {
            return Ok(Some(SemanticAction::Expect(
                Expectation::ListItemConfirmation {
                    list: list_id,
                    item_id,
                    confirmation,
                },
            )));
        }
        return Ok(Some(SemanticAction::Expect(Expectation::ListContains {
            list: list_id,
            item_id,
        })));
    }
    if let (Some(list_id), Some(count)) = (step.list_id, step.count) {
        return Ok(Some(SemanticAction::Expect(Expectation::ListCountIs {
            list: list_id,
            count,
        })));
    }
    Ok(None)
}
