//! Shared semantic scenario contract for harness, simulator, and verification flows.
//!
//! This contract describes scenario actions and expectations without embedding
//! renderer-specific details such as PTY key sequences or DOM selectors.

#![allow(missing_docs)] // Shared semantic contract - expanded incrementally during migration.

use crate::ui_contract::{
    ConfirmationState, ControlId, FieldId, ListId, ModalId, OperationId, OperationState,
    RuntimeEventKind, ScreenId, ToastKind, UiReadiness,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActorId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioDefinition {
    pub id: String,
    pub goal: String,
    pub steps: Vec<ScenarioStep>,
}

impl ScenarioDefinition {
    pub fn validate_shared_intent_contract(&self) -> Result<(), String> {
        for step in &self.steps {
            if let ScenarioAction::Ui(action) = &step.action {
                return Err(format!(
                    "shared scenario {} step {} uses raw ui action {:?}; shared scenarios must use intent actions instead",
                    self.id, step.id, action
                ));
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentAction {
    OpenScreen(ScreenId),
    CreateAccount {
        account_name: String,
    },
    CreateHome {
        home_name: String,
    },
    StartDeviceEnrollment {
        device_name: String,
        code_name: String,
    },
    ImportDeviceEnrollmentCode {
        code: String,
    },
    OpenSettingsSection(SettingsSection),
    RemoveSelectedDevice,
    CreateContactInvitation {
        receiver_authority_id: String,
        code_name: String,
    },
    AcceptContactInvitation {
        code: String,
    },
    AcceptPendingChannelInvitation,
    JoinChannel {
        channel_name: String,
    },
    InviteActorToChannel {
        authority_id: String,
    },
    SendChatMessage {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSection {
    Devices,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiAction {
    Navigate(ScreenId),
    Activate(ControlId),
    ActivateListItem { list: ListId, item_id: String },
    Fill(FieldId, String),
    InputText(String),
    DismissTransient,
    PressKey(InputKey, u16),
    SendChatCommand(String),
    PasteClipboard { source_actor: Option<ActorId> },
    ReadClipboard { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKey {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentAction {
    LaunchActors,
    RestartActor { actor: ActorId },
    KillActor { actor: ActorId },
    FaultDelay { actor: ActorId, delay_ms: u64 },
    FaultLoss { actor: ActorId, loss_percent: u8 },
    FaultTunnelDrop { actor: ActorId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableAction {
    Set {
        name: String,
        value: String,
    },
    CaptureCurrentAuthorityId {
        name: String,
    },
    CaptureSelection {
        name: String,
        list: ListId,
    },
    Extract {
        name: String,
        regex: String,
        group: u32,
        from: ExtractSource,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractSource {
    Screen,
    RawScreen,
    AuthoritativeScreen,
    NormalizedScreen,
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
    StartDeviceEnrollment,
    ImportDeviceEnrollmentCode,
    OpenSettingsSection,
    RemoveSelectedDevice,
    CreateContactInvitation,
    AcceptContactInvitation,
    AcceptPendingChannelInvitation,
    JoinChannel,
    InviteActorToChannel,
    SendChatCommand,
    SendChatMessage,
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
    ToastContains,
    ListContains,
    ListCountIs,
    ListItemConfirmation,
    SelectionIs,
    ReadinessIs,
    RuntimeEventOccurred,
    OperationStateIs,
    ParityWithActor,
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
            SemanticActionKind::OpenScreen => ScenarioAction::Intent(IntentAction::OpenScreen(
                required(value.screen_id, "screen_id", value.action)?,
            )),
            SemanticActionKind::CreateAccount => {
                ScenarioAction::Intent(IntentAction::CreateAccount {
                    account_name: required(value.value, "value", value.action)?,
                })
            }
            SemanticActionKind::StartDeviceEnrollment => {
                ScenarioAction::Intent(IntentAction::StartDeviceEnrollment {
                    device_name: required(value.value, "value", value.action)?,
                    code_name: required(value.name, "name", value.action)?,
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
                ScenarioAction::Intent(IntentAction::RemoveSelectedDevice)
            }
            SemanticActionKind::CreateContactInvitation => {
                ScenarioAction::Intent(IntentAction::CreateContactInvitation {
                    receiver_authority_id: required(value.value, "value", value.action)?,
                    code_name: required(value.name, "name", value.action)?,
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

#[cfg(test)]
mod tests {
    use super::{
        Expectation, FieldId, IntentAction, ScenarioAction, ScenarioDefinition, ScenarioStep,
        ScreenId, SemanticActionKind, SemanticScenarioFile, SemanticScenarioFileStep, UiAction,
    };

    #[test]
    fn semantic_file_converts_to_definition() {
        let file = SemanticScenarioFile {
            id: "semantic-smoke".to_string(),
            goal: "check semantic schema".to_string(),
            steps: vec![
                SemanticScenarioFileStep {
                    id: "nav".to_string(),
                    actor: None,
                    timeout_ms: Some(1000),
                    action: SemanticActionKind::Navigate,
                    screen_id: Some(ScreenId::Chat),
                    control_id: None,
                    field_id: None,
                    modal_id: None,
                    list_id: None,
                    item_id: None,
                    value: None,
                    key: None,
                    repeat: None,
                    source_actor: None,
                    kind: None,
                    count: None,
                    readiness: None,
                    runtime_event_kind: None,
                    operation_id: None,
                    operation_state: None,
                    confirmation: None,
                    peer_actor: None,
                    section: None,
                    name: None,
                    regex: None,
                    group: None,
                    from: None,
                },
                SemanticScenarioFileStep {
                    id: "fill".to_string(),
                    actor: None,
                    timeout_ms: None,
                    action: SemanticActionKind::Fill,
                    screen_id: None,
                    control_id: None,
                    field_id: Some(FieldId::Nickname),
                    modal_id: None,
                    list_id: None,
                    item_id: None,
                    value: Some("ops".to_string()),
                    key: None,
                    repeat: None,
                    source_actor: None,
                    kind: None,
                    count: None,
                    readiness: None,
                    runtime_event_kind: None,
                    operation_id: None,
                    operation_state: None,
                    confirmation: None,
                    peer_actor: None,
                    section: None,
                    name: None,
                    regex: None,
                    group: None,
                    from: None,
                },
            ],
        };

        let definition = ScenarioDefinition::try_from(file)
            .unwrap_or_else(|error| panic!("semantic conversion failed: {error}"));
        assert_eq!(definition.id, "semantic-smoke");
        assert!(matches!(
            definition.steps[0],
            ScenarioStep {
                action: ScenarioAction::Ui(UiAction::Navigate(ScreenId::Chat)),
                ..
            }
        ));
        assert!(matches!(
            definition.steps[1],
            ScenarioStep {
                action: ScenarioAction::Ui(UiAction::Fill(FieldId::Nickname, ref value)),
                ..
            } if value == "ops"
        ));
    }

    #[test]
    fn semantic_file_rejects_missing_required_fields() {
        let step = SemanticScenarioFileStep {
            id: "bad".to_string(),
            actor: None,
            timeout_ms: None,
            action: SemanticActionKind::ScreenIs,
            screen_id: None,
            control_id: None,
            field_id: None,
            modal_id: None,
            list_id: None,
            item_id: None,
            count: None,
            value: None,
            key: None,
            repeat: None,
            source_actor: None,
            kind: None,
            readiness: None,
            runtime_event_kind: None,
            operation_id: None,
            operation_state: None,
            confirmation: None,
            peer_actor: None,
            section: None,
            name: None,
            regex: None,
            group: None,
            from: None,
        };

        let error = ScenarioStep::try_from(step)
            .expect_err("screen expectation without screen_id must fail");
        assert!(error.contains("screen_id"));
    }

    #[test]
    fn semantic_expectation_variant_is_constructible() {
        let expectation = Expectation::ScreenIs(ScreenId::Settings);
        assert!(matches!(
            expectation,
            Expectation::ScreenIs(ScreenId::Settings)
        ));
    }

    #[test]
    fn semantic_parity_expectation_requires_peer_actor() {
        let step = SemanticScenarioFileStep {
            id: "parity".to_string(),
            actor: Some(super::ActorId("web".to_string())),
            timeout_ms: Some(1000),
            action: SemanticActionKind::ParityWithActor,
            screen_id: None,
            control_id: None,
            field_id: None,
            modal_id: None,
            list_id: None,
            item_id: None,
            count: None,
            value: None,
            key: None,
            repeat: None,
            source_actor: None,
            kind: None,
            readiness: None,
            runtime_event_kind: None,
            operation_id: None,
            operation_state: None,
            peer_actor: Some(super::ActorId("tui".to_string())),
            confirmation: None,
            section: None,
            name: None,
            regex: None,
            group: None,
            from: None,
        };

        let converted = ScenarioStep::try_from(step).expect("parity conversion should succeed");
        assert!(matches!(
            converted.action,
            ScenarioAction::Expect(Expectation::ParityWithActor { actor })
                if actor.0 == "tui"
        ));
    }

    #[test]
    fn semantic_intent_file_converts_to_definition() {
        let file = SemanticScenarioFile {
            id: "semantic-intent".to_string(),
            goal: "check intent schema".to_string(),
            steps: vec![SemanticScenarioFileStep {
                id: "create-account".to_string(),
                actor: Some(super::ActorId("alice".to_string())),
                timeout_ms: Some(1000),
                action: SemanticActionKind::CreateAccount,
                screen_id: None,
                control_id: None,
                field_id: None,
                modal_id: None,
                list_id: None,
                item_id: None,
                count: None,
                value: Some("Alice".to_string()),
                key: None,
                repeat: None,
                source_actor: None,
                kind: None,
                readiness: None,
                runtime_event_kind: None,
                operation_id: None,
                operation_state: None,
                peer_actor: None,
                confirmation: None,
                section: None,
                name: None,
                regex: None,
                group: None,
                from: None,
            }],
        };

        let definition = ScenarioDefinition::try_from(file)
            .unwrap_or_else(|error| panic!("semantic conversion failed: {error}"));
        assert!(matches!(
            definition.steps[0].action,
            ScenarioAction::Intent(IntentAction::CreateAccount { ref account_name })
                if account_name == "Alice"
        ));
    }

    #[test]
    fn shared_intent_contract_accepts_intents() {
        let definition = ScenarioDefinition {
            id: "shared-intent".to_string(),
            goal: "intent validation".to_string(),
            steps: vec![ScenarioStep {
                id: "open".to_string(),
                actor: Some(super::ActorId("alice".to_string())),
                timeout_ms: Some(1000),
                action: ScenarioAction::Intent(IntentAction::OpenScreen(ScreenId::Chat)),
            }],
        };

        definition
            .validate_shared_intent_contract()
            .unwrap_or_else(|error| panic!("intent validation failed: {error}"));
    }

    #[test]
    fn shared_intent_contract_rejects_ui_actions() {
        let definition = ScenarioDefinition {
            id: "shared-ui-invalid".to_string(),
            goal: "intent validation".to_string(),
            steps: vec![ScenarioStep {
                id: "bad".to_string(),
                actor: Some(super::ActorId("alice".to_string())),
                timeout_ms: Some(1000),
                action: ScenarioAction::Ui(UiAction::Navigate(ScreenId::Chat)),
            }],
        };

        let error = definition
            .validate_shared_intent_contract()
            .expect_err("shared validator must reject raw ui actions");
        assert!(error.contains("raw ui action"));
    }
}
