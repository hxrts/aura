//! Shared semantic scenario contract for harness, simulator, and verification flows.
//!
//! This contract describes scenario actions and expectations without embedding
//! renderer-specific details such as PTY key sequences or DOM selectors.

#![allow(missing_docs)] // Shared semantic contract - expanded incrementally during migration.

use crate::ui_contract::{
    ControlId, FieldId, ListId, ModalId, OperationId, OperationState, ScreenId, ToastKind,
    UiReadiness,
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
    Ui(UiAction),
    Expect(Expectation),
    Variables(VariableAction),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiAction {
    Navigate(ScreenId),
    Activate(ControlId),
    Fill(FieldId, String),
    InputText(String),
    PressKey(InputKey, u16),
    PasteClipboard { source_actor: Option<ActorId> },
    ReadClipboard,
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
    Extract {
        name: String,
        regex: String,
        group: usize,
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
    ToastContains {
        kind: Option<ToastKind>,
        message_contains: String,
    },
    ListContains {
        list: ListId,
        item_id: String,
    },
    SelectionIs {
        list: ListId,
        item_id: String,
    },
    ReadinessIs(UiReadiness),
    OperationStateIs {
        operation_id: OperationId,
        state: OperationState,
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
    pub value: Option<String>,
    pub key: Option<InputKey>,
    pub repeat: Option<u16>,
    pub source_actor: Option<ActorId>,
    pub kind: Option<ToastKind>,
    pub readiness: Option<UiReadiness>,
    pub operation_id: Option<OperationId>,
    pub operation_state: Option<OperationState>,
    pub name: Option<String>,
    pub regex: Option<String>,
    pub group: Option<usize>,
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
    Navigate,
    Activate,
    Fill,
    InputText,
    PressKey,
    PasteClipboard,
    ReadClipboard,
    ScreenIs,
    ControlVisible,
    ModalOpen,
    ToastContains,
    ListContains,
    SelectionIs,
    ReadinessIs,
    OperationStateIs,
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
            SemanticActionKind::Fill => ScenarioAction::Ui(UiAction::Fill(
                required(value.field_id, "field_id", value.action)?,
                required(value.value, "value", value.action)?,
            )),
            SemanticActionKind::InputText => ScenarioAction::Ui(UiAction::InputText(required(
                value.value,
                "value",
                value.action,
            )?)),
            SemanticActionKind::PressKey => ScenarioAction::Ui(UiAction::PressKey(
                required(value.key, "key", value.action)?,
                value.repeat.unwrap_or(1).max(1),
            )),
            SemanticActionKind::PasteClipboard => ScenarioAction::Ui(UiAction::PasteClipboard {
                source_actor: value.source_actor,
            }),
            SemanticActionKind::ReadClipboard => ScenarioAction::Ui(UiAction::ReadClipboard),
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
            SemanticActionKind::SelectionIs => ScenarioAction::Expect(Expectation::SelectionIs {
                list: required(value.list_id, "list_id", value.action)?,
                item_id: required(value.item_id, "item_id", value.action)?,
            }),
            SemanticActionKind::ReadinessIs => ScenarioAction::Expect(Expectation::ReadinessIs(
                required(value.readiness, "readiness", value.action)?,
            )),
            SemanticActionKind::OperationStateIs => {
                ScenarioAction::Expect(Expectation::OperationStateIs {
                    operation_id: required(value.operation_id, "operation_id", value.action)?,
                    state: required(value.operation_state, "operation_state", value.action)?,
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
    value.ok_or_else(|| format!("semantic action {:?} requires {field}", action))
}

#[cfg(test)]
mod tests {
    use super::{
        Expectation, FieldId, ScenarioAction, ScenarioDefinition, ScenarioStep, ScreenId,
        SemanticActionKind, SemanticScenarioFile, SemanticScenarioFileStep, UiAction,
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
                    readiness: None,
                    operation_id: None,
                    operation_state: None,
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
                    readiness: None,
                    operation_id: None,
                    operation_state: None,
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
            value: None,
            key: None,
            repeat: None,
            source_actor: None,
            kind: None,
            readiness: None,
            operation_id: None,
            operation_state: None,
            name: None,
            regex: None,
            group: None,
            from: None,
        };

        let error = ScenarioStep::try_from(step)
            .expect_err("screen expectation without screen_id must fail");
        assert!(error.to_string().contains("screen_id"));
    }

    #[test]
    fn semantic_expectation_variant_is_constructible() {
        let expectation = Expectation::ScreenIs(ScreenId::Settings);
        assert!(matches!(
            expectation,
            Expectation::ScreenIs(ScreenId::Settings)
        ));
    }
}
