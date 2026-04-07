//! Shared semantic scenario definitions, expectations, and file parsing helpers.

use super::values::is_row_index_item_id;
use super::{
    ActorId, EnvironmentAction, ExtractSource, InputKey, IntentAction, SettingsSection, UiAction,
    VariableAction,
};
use crate::ui_contract::{
    ConfirmationState, ControlId, FieldId, ListId, ModalId, OperationId, OperationState,
    RuntimeEventKind, ScreenId, ToastKind, UiReadiness,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioDefinition {
    pub id: String,
    pub goal: String,
    pub steps: Vec<ScenarioStep>,
}

impl ScenarioDefinition {
    pub fn validate_shared_intent_contract(&self) -> Result<(), String> {
        for step in &self.steps {
            if let Some(actor) = &step.actor {
                if actor.is_frontend_binding_label() {
                    return Err(format!(
                        "shared scenario {} step {} uses frontend-bound actor id '{}'; shared scenarios must use frontend-neutral actor ids",
                        self.id, step.id, actor.0
                    ));
                }
            }
            if let ScenarioAction::Ui(action) = &step.action {
                return Err(format!(
                    "shared scenario {} step {} uses raw ui action {:?}; shared scenarios must use intent actions instead",
                    self.id, step.id, action
                ));
            }
            for actor in step.action.referenced_actor_ids() {
                if actor.is_frontend_binding_label() {
                    return Err(format!(
                        "shared scenario {} step {} references frontend-bound actor id '{}'; renderer binding belongs in config or matrix layers",
                        self.id, step.id, actor.0
                    ));
                }
            }
            if let Some(item_id) = step.action.referenced_item_ids() {
                if is_row_index_item_id(item_id) {
                    return Err(format!(
                        "shared scenario {} step {} references row-index item id '{}'; parity-critical list targeting must be id-based",
                        self.id, step.id, item_id
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioStep {
    pub id: String,
    pub actor: Option<ActorId>,
    pub timeout_ms: Option<u64>,
    pub action: ScenarioAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioAction {
    Environment(EnvironmentAction),
    Intent(IntentAction),
    Ui(UiAction),
    Expect(Expectation),
    Variables(VariableAction),
}

impl ScenarioAction {
    #[must_use]
    pub fn referenced_actor_ids(&self) -> Vec<&ActorId> {
        match self {
            Self::Environment(EnvironmentAction::RestartActor { actor })
            | Self::Environment(EnvironmentAction::KillActor { actor })
            | Self::Environment(EnvironmentAction::FaultDelay { actor, .. })
            | Self::Environment(EnvironmentAction::FaultLoss { actor, .. })
            | Self::Environment(EnvironmentAction::FaultTunnelDrop { actor }) => vec![actor],
            Self::Ui(UiAction::PasteClipboard {
                source_actor: Some(actor),
            }) => vec![actor],
            Self::Expect(Expectation::ParityWithActor { actor }) => vec![actor],
            _ => Vec::new(),
        }
    }

    #[must_use]
    pub fn referenced_item_ids(&self) -> Option<&str> {
        match self {
            Self::Ui(UiAction::ActivateListItem { item_id, .. })
            | Self::Expect(Expectation::ListContains { item_id, .. })
            | Self::Expect(Expectation::ListItemConfirmation { item_id, .. })
            | Self::Expect(Expectation::SelectionIs { item_id, .. }) => Some(item_id.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expectation {
    ScreenIs(ScreenId),
    ControlVisible(ControlId),
    ModalOpen(ModalId),
    MessageContains {
        message_contains: String,
    },
    DiagnosticScreenContains {
        text_contains: String,
    },
    ToastContains {
        kind: Option<ToastKind>,
        message_contains: String,
    },
    ListContains {
        list: ListId,
        item_id: String,
    },
    ListCountIs {
        list: ListId,
        count: usize,
    },
    ListItemConfirmation {
        list: ListId,
        item_id: String,
        confirmation: ConfirmationState,
    },
    SelectionIs {
        list: ListId,
        item_id: String,
    },
    ReadinessIs(UiReadiness),
    RuntimeEventOccurred {
        kind: RuntimeEventKind,
        detail_contains: Option<String>,
        capture_name: Option<String>,
    },
    OperationStateIs {
        operation_id: OperationId,
        state: OperationState,
    },
    ParityWithActor {
        actor: ActorId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticScenarioFile {
    pub id: String,
    pub goal: String,
    pub steps: Vec<SemanticScenarioFileStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticScenarioFileStep {
    pub id: String,
    pub actor: Option<ActorId>,
    pub timeout_ms: Option<u64>,
    pub action: SemanticActionKind,
    pub screen_id: Option<ScreenId>,
    pub control_id: Option<ControlId>,
    pub field_id: Option<FieldId>,
    pub modal_id: Option<ModalId>,
    pub list_id: Option<ListId>,
    pub item_id: Option<String>,
    pub count: Option<usize>,
    pub value: Option<String>,
    pub key: Option<InputKey>,
    pub repeat: Option<u16>,
    pub source_actor: Option<ActorId>,
    pub kind: Option<ToastKind>,
    pub readiness: Option<UiReadiness>,
    pub runtime_event_kind: Option<RuntimeEventKind>,
    pub operation_id: Option<OperationId>,
    pub operation_state: Option<OperationState>,
    pub peer_actor: Option<ActorId>,
    pub invitee_authority_id: Option<String>,
    pub confirmation: Option<ConfirmationState>,
    pub section: Option<SettingsSection>,
    pub name: Option<String>,
    pub regex: Option<String>,
    pub group: Option<u32>,
    pub from: Option<ExtractSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticActionKind {
    LaunchActors,
    RestartActor,
    KillActor,
    FaultDelay,
    FaultLoss,
    FaultTunnelDrop,
    OpenScreen,
    CreateAccount,
    CreateHome,
    CreateChannel,
    StartDeviceEnrollment,
    ImportDeviceEnrollmentCode,
    OpenSettingsSection,
    RemoveSelectedDevice,
    SwitchAuthority,
    CreateContactInvitation,
    AcceptContactInvitation,
    AcceptPendingChannelInvitation,
    JoinChannel,
    InviteActorToChannel,
    SendChatCommand,
    SendChatMessage,
    SendFriendRequest,
    AcceptFriendRequest,
    DeclineFriendRequest,
    PasteClipboard,
    ReadClipboard,
    Navigate,
    Activate,
    ActivateListItem,
    Fill,
    InputText,
    DismissTransient,
    PressKey,
    ScreenIs,
    ControlVisible,
    ModalOpen,
    MessageContains,
    DiagnosticScreenContains,
    ToastContains,
    ListContains,
    ListCountIs,
    ListItemConfirmation,
    SelectionIs,
    ReadinessIs,
    RuntimeEventOccurred,
    OperationStateIs,
    ParityWithActor,
    PrepareDeviceEnrollmentInviteeAuthority,
    CaptureCurrentAuthorityId,
    CaptureSelection,
    SetVar,
    ExtractVar,
}

impl TryFrom<SemanticScenarioFile> for ScenarioDefinition {
    type Error = String;

    fn try_from(value: SemanticScenarioFile) -> Result<Self, Self::Error> {
        let steps = value
            .steps
            .into_iter()
            .map(SemanticScenarioFileStep::try_into)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            id: value.id,
            goal: value.goal,
            steps,
        })
    }
}

impl TryFrom<SemanticScenarioFileStep> for ScenarioStep {
    type Error = String;

    fn try_from(value: SemanticScenarioFileStep) -> Result<Self, Self::Error> {
        let step_actor = value.actor.clone();
        let action = match value.action {
            SemanticActionKind::LaunchActors => {
                ScenarioAction::Environment(EnvironmentAction::LaunchActors)
            }
            SemanticActionKind::RestartActor => {
                ScenarioAction::Environment(EnvironmentAction::RestartActor {
                    actor: required(value.actor, "actor", value.action)?,
                })
            }
            SemanticActionKind::KillActor => {
                ScenarioAction::Environment(EnvironmentAction::KillActor {
                    actor: required(value.actor, "actor", value.action)?,
                })
            }
            SemanticActionKind::FaultDelay => {
                ScenarioAction::Environment(EnvironmentAction::FaultDelay {
                    actor: required(value.actor, "actor", value.action)?,
                    delay_ms: value.timeout_ms.unwrap_or_default(),
                })
            }
            SemanticActionKind::FaultLoss => {
                ScenarioAction::Environment(EnvironmentAction::FaultLoss {
                    actor: required(value.actor, "actor", value.action)?,
                    loss_percent: value.value.as_deref().unwrap_or("100").parse().map_err(
                        |_| {
                            format!(
                                "action {:?} requires numeric loss percent in value",
                                value.action
                            )
                        },
                    )?,
                })
            }
            SemanticActionKind::FaultTunnelDrop => {
                ScenarioAction::Environment(EnvironmentAction::FaultTunnelDrop {
                    actor: required(value.actor, "actor", value.action)?,
                })
            }
            SemanticActionKind::OpenScreen => ScenarioAction::Intent(IntentAction::OpenScreen {
                screen: required(value.screen_id, "screen_id", value.action)?,
                channel_id: None,
                context_id: None,
            }),
            SemanticActionKind::CreateAccount => {
                ScenarioAction::Intent(IntentAction::CreateAccount {
                    account_name: required(value.value, "value", value.action)?,
                })
            }
            SemanticActionKind::CreateHome => ScenarioAction::Intent(IntentAction::CreateHome {
                home_name: required(value.value, "value", value.action)?,
            }),
            SemanticActionKind::CreateChannel => {
                ScenarioAction::Intent(IntentAction::CreateChannel {
                    channel_name: required(value.value, "value", value.action)?,
                })
            }
            SemanticActionKind::StartDeviceEnrollment => {
                ScenarioAction::Intent(IntentAction::StartDeviceEnrollment {
                    device_name: required(value.value, "value", value.action)?,
                    code_name: required(value.name, "name", value.action)?,
                    invitee_authority_id: required(
                        value.invitee_authority_id,
                        "invitee_authority_id",
                        value.action,
                    )?,
                })
            }
            SemanticActionKind::ImportDeviceEnrollmentCode => {
                ScenarioAction::Intent(IntentAction::ImportDeviceEnrollmentCode {
                    code: required(value.value, "value", value.action)?,
                })
            }
            SemanticActionKind::OpenSettingsSection => {
                ScenarioAction::Intent(IntentAction::OpenSettingsSection(required(
                    value.section,
                    "section",
                    value.action,
                )?))
            }
            SemanticActionKind::RemoveSelectedDevice => {
                ScenarioAction::Intent(IntentAction::RemoveSelectedDevice { device_id: None })
            }
            SemanticActionKind::SwitchAuthority => {
                ScenarioAction::Intent(IntentAction::SwitchAuthority {
                    authority_id: required(value.value, "value", value.action)?,
                })
            }
            SemanticActionKind::CreateContactInvitation => {
                ScenarioAction::Intent(IntentAction::CreateContactInvitation {
                    receiver_authority_id: required(value.value, "value", value.action)?,
                    code_name: value.name,
                })
            }
            SemanticActionKind::AcceptContactInvitation => {
                ScenarioAction::Intent(IntentAction::AcceptContactInvitation {
                    code: required(value.value, "value", value.action)?,
                })
            }
            SemanticActionKind::AcceptPendingChannelInvitation => {
                ScenarioAction::Intent(IntentAction::AcceptPendingChannelInvitation)
            }
            SemanticActionKind::JoinChannel => ScenarioAction::Intent(IntentAction::JoinChannel {
                channel_name: required(value.value, "value", value.action)?,
            }),
            SemanticActionKind::InviteActorToChannel => {
                ScenarioAction::Intent(IntentAction::InviteActorToChannel {
                    authority_id: required(value.value, "value", value.action)?,
                    channel_id: None,
                    context_id: None,
                    channel_name: None,
                })
            }
            SemanticActionKind::Navigate => ScenarioAction::Ui(UiAction::Navigate(required(
                value.screen_id,
                "screen_id",
                value.action,
            )?)),
            SemanticActionKind::Activate => ScenarioAction::Ui(UiAction::Activate(required(
                value.control_id,
                "control_id",
                value.action,
            )?)),
            SemanticActionKind::ActivateListItem => {
                ScenarioAction::Ui(UiAction::ActivateListItem {
                    list: required(value.list_id, "list_id", value.action)?,
                    item_id: required(value.item_id, "item_id", value.action)?,
                })
            }
            SemanticActionKind::Fill => ScenarioAction::Ui(UiAction::Fill(
                required(value.field_id, "field_id", value.action)?,
                required(value.value, "value", value.action)?,
            )),
            SemanticActionKind::InputText => ScenarioAction::Ui(UiAction::InputText(required(
                value.value,
                "value",
                value.action,
            )?)),
            SemanticActionKind::DismissTransient => ScenarioAction::Ui(UiAction::DismissTransient),
            SemanticActionKind::PressKey => ScenarioAction::Ui(UiAction::PressKey(
                required(value.key, "key", value.action)?,
                value.repeat.unwrap_or(1).max(1),
            )),
            SemanticActionKind::SendChatCommand => ScenarioAction::Ui(UiAction::SendChatCommand(
                required(value.value, "value", value.action)?,
            )),
            SemanticActionKind::SendChatMessage => {
                ScenarioAction::Intent(IntentAction::SendChatMessage {
                    message: required(value.value, "value", value.action)?,
                    channel_id: None,
                    context_id: None,
                })
            }
            SemanticActionKind::SendFriendRequest => {
                ScenarioAction::Intent(IntentAction::SendFriendRequest {
                    authority_id: required(value.value, "value", value.action)?,
                })
            }
            SemanticActionKind::AcceptFriendRequest => {
                ScenarioAction::Intent(IntentAction::AcceptFriendRequest {
                    authority_id: required(value.value, "value", value.action)?,
                })
            }
            SemanticActionKind::DeclineFriendRequest => {
                ScenarioAction::Intent(IntentAction::DeclineFriendRequest {
                    authority_id: required(value.value, "value", value.action)?,
                })
            }
            SemanticActionKind::PasteClipboard => ScenarioAction::Ui(UiAction::PasteClipboard {
                source_actor: value.source_actor,
            }),
            SemanticActionKind::ReadClipboard => ScenarioAction::Ui(UiAction::ReadClipboard {
                name: required(value.name, "name", value.action)?,
            }),
            SemanticActionKind::ScreenIs => ScenarioAction::Expect(Expectation::ScreenIs(
                required(value.screen_id, "screen_id", value.action)?,
            )),
            SemanticActionKind::ControlVisible => {
                ScenarioAction::Expect(Expectation::ControlVisible(required(
                    value.control_id,
                    "control_id",
                    value.action,
                )?))
            }
            SemanticActionKind::ModalOpen => ScenarioAction::Expect(Expectation::ModalOpen(
                required(value.modal_id, "modal_id", value.action)?,
            )),
            SemanticActionKind::MessageContains => {
                ScenarioAction::Expect(Expectation::MessageContains {
                    message_contains: required(value.value, "value", value.action)?,
                })
            }
            SemanticActionKind::DiagnosticScreenContains => {
                ScenarioAction::Expect(Expectation::DiagnosticScreenContains {
                    text_contains: required(value.value, "value", value.action)?,
                })
            }
            SemanticActionKind::ToastContains => {
                ScenarioAction::Expect(Expectation::ToastContains {
                    kind: value.kind,
                    message_contains: required(value.value, "value", value.action)?,
                })
            }
            SemanticActionKind::ListContains => ScenarioAction::Expect(Expectation::ListContains {
                list: required(value.list_id, "list_id", value.action)?,
                item_id: required(value.item_id, "item_id", value.action)?,
            }),
            SemanticActionKind::ListCountIs => ScenarioAction::Expect(Expectation::ListCountIs {
                list: required(value.list_id, "list_id", value.action)?,
                count: required(value.count, "count", value.action)?,
            }),
            SemanticActionKind::ListItemConfirmation => {
                ScenarioAction::Expect(Expectation::ListItemConfirmation {
                    list: required(value.list_id, "list_id", value.action)?,
                    item_id: required(value.item_id, "item_id", value.action)?,
                    confirmation: required(value.confirmation, "confirmation", value.action)?,
                })
            }
            SemanticActionKind::SelectionIs => ScenarioAction::Expect(Expectation::SelectionIs {
                list: required(value.list_id, "list_id", value.action)?,
                item_id: required(value.item_id, "item_id", value.action)?,
            }),
            SemanticActionKind::ReadinessIs => ScenarioAction::Expect(Expectation::ReadinessIs(
                required(value.readiness, "readiness", value.action)?,
            )),
            SemanticActionKind::RuntimeEventOccurred => {
                ScenarioAction::Expect(Expectation::RuntimeEventOccurred {
                    kind: required(value.runtime_event_kind, "runtime_event_kind", value.action)?,
                    detail_contains: value.value,
                    capture_name: value.name,
                })
            }
            SemanticActionKind::OperationStateIs => {
                ScenarioAction::Expect(Expectation::OperationStateIs {
                    operation_id: required(value.operation_id, "operation_id", value.action)?,
                    state: required(value.operation_state, "operation_state", value.action)?,
                })
            }
            SemanticActionKind::ParityWithActor => {
                ScenarioAction::Expect(Expectation::ParityWithActor {
                    actor: required(value.peer_actor, "peer_actor", value.action)?,
                })
            }
            SemanticActionKind::PrepareDeviceEnrollmentInviteeAuthority => {
                ScenarioAction::Variables(VariableAction::PrepareDeviceEnrollmentInviteeAuthority {
                    name: required(value.name, "name", value.action)?,
                })
            }
            SemanticActionKind::CaptureCurrentAuthorityId => {
                ScenarioAction::Variables(VariableAction::CaptureCurrentAuthorityId {
                    name: required(value.name, "name", value.action)?,
                })
            }
            SemanticActionKind::CaptureSelection => {
                ScenarioAction::Variables(VariableAction::CaptureSelection {
                    name: required(value.name, "name", value.action)?,
                    list: required(value.list_id, "list_id", value.action)?,
                })
            }
            SemanticActionKind::SetVar => ScenarioAction::Variables(VariableAction::Set {
                name: required(value.name, "name", value.action)?,
                value: required(value.value, "value", value.action)?,
            }),
            SemanticActionKind::ExtractVar => ScenarioAction::Variables(VariableAction::Extract {
                name: required(value.name, "name", value.action)?,
                regex: required(value.regex, "regex", value.action)?,
                group: value.group.unwrap_or(0),
                from: required(value.from, "from", value.action)?,
            }),
        };

        Ok(ScenarioStep {
            id: value.id,
            actor: step_actor,
            timeout_ms: value.timeout_ms,
            action,
        })
    }
}

fn required<T>(value: Option<T>, field: &str, action: SemanticActionKind) -> Result<T, String> {
    value.ok_or_else(|| format!("semantic action {action:?} requires {field}"))
}
