//! Shared semantic action and contract declarations.

use super::{
    ActorId, IntentKind, SharedActionHandle, SharedActionRequest, SubmissionContract,
    SubmissionValueContract,
};
use crate::ui_contract::{
    ControlId, FieldId, FlowAvailability, FrontendId, ListId, ModalId, OperationId, OperationState,
    ProjectionRevision, QuiescenceState, RuntimeEventKind, ScreenId, UiReadiness,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionPrecondition {
    Readiness(UiReadiness),
    Quiescence(QuiescenceState),
    Screen(ScreenId),
    RuntimeEvent(RuntimeEventKind),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusSemantics {
    Screen(ScreenId),
    Modal(ModalId),
    Control(ControlId),
    Field(FieldId),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionSemantics {
    List(ListId),
    PreservesCurrent,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedActionContract {
    pub intent: IntentKind,
    pub submission: SubmissionContract,
    pub preconditions: Vec<ActionPrecondition>,
    pub barriers: SharedActionBarrierMetadata,
    pub post_operation_convergence: Option<PostOperationConvergenceContract>,
    pub focus_semantics: FocusSemantics,
    pub selection_semantics: SelectionSemantics,
    pub transitions: Vec<AuthoritativeTransitionKind>,
    pub terminal_success: Vec<TerminalSuccessKind>,
    pub terminal_failure_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedActionBarrierMetadata {
    pub before_issue: Vec<BarrierDeclaration>,
    pub before_next_intent: Vec<BarrierDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarrierDeclaration {
    Readiness(UiReadiness),
    Quiescence(QuiescenceState),
    Screen(ScreenId),
    Modal(ModalId),
    RuntimeEvent(RuntimeEventKind),
    OperationState {
        operation_id: OperationId,
        state: OperationState,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticBarrierRef {
    Readiness {
        readiness: UiReadiness,
    },
    Quiescence {
        state: QuiescenceState,
    },
    Screen {
        screen: ScreenId,
    },
    Modal {
        modal: ModalId,
    },
    RuntimeEvent {
        event: RuntimeEventKind,
    },
    OperationState {
        operation_id: OperationId,
        state: OperationState,
    },
}

impl SemanticBarrierRef {
    #[must_use]
    pub fn matches_declaration(&self, barrier: &BarrierDeclaration) -> bool {
        match (self, barrier) {
            (Self::Readiness { readiness: actual }, BarrierDeclaration::Readiness(expected)) => {
                actual == expected
            }
            (Self::Quiescence { state: actual }, BarrierDeclaration::Quiescence(expected)) => {
                actual == expected
            }
            (Self::Screen { screen: actual }, BarrierDeclaration::Screen(expected)) => {
                actual == expected
            }
            (Self::Modal { modal: actual }, BarrierDeclaration::Modal(expected)) => {
                actual == expected
            }
            (Self::RuntimeEvent { event: actual }, BarrierDeclaration::RuntimeEvent(expected)) => {
                actual == expected
            }
            (
                Self::OperationState {
                    operation_id: actual_operation_id,
                    state: actual_state,
                },
                BarrierDeclaration::OperationState {
                    operation_id: expected_operation_id,
                    state: expected_state,
                },
            ) => actual_operation_id == expected_operation_id && actual_state == expected_state,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticCommandSupport {
    pub intent: IntentKind,
    pub web: FlowAvailability,
    pub tui: FlowAvailability,
}

macro_rules! supported_on_all_frontends {
    ($($intent:ident),+ $(,)?) => {
        pub const SEMANTIC_COMMAND_SUPPORT: &[SemanticCommandSupport] = &[
            $(SemanticCommandSupport {
                intent: IntentKind::$intent,
                web: FlowAvailability::Supported,
                tui: FlowAvailability::Supported,
            },)+
        ];
    };
}

supported_on_all_frontends!(
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
    SendChatMessage,
    SendFriendRequest,
    AcceptFriendRequest,
    DeclineFriendRequest,
);

#[must_use]
pub fn semantic_command_support(intent: IntentKind) -> &'static SemanticCommandSupport {
    SEMANTIC_COMMAND_SUPPORT
        .iter()
        .find(|support| support.intent == intent)
        .unwrap_or_else(|| panic!("missing semantic command support for {intent:?}"))
}

#[must_use]
pub fn frontend_supports_semantic_command(frontend: FrontendId, intent: IntentKind) -> bool {
    let support = semantic_command_support(intent);
    match frontend {
        FrontendId::Web => support.web == FlowAvailability::Supported,
        FrontendId::Tui => support.tui == FlowAvailability::Supported,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostOperationConvergenceContract {
    pub required_before_next_intent: Vec<BarrierDeclaration>,
    pub violation_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoritativeTransitionKind {
    RuntimeEvent(RuntimeEventKind),
    Operation(OperationId),
    Screen(ScreenId),
    Modal(ModalId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoritativeTransitionFact {
    pub handle: SharedActionHandle,
    pub transition: AuthoritativeTransitionKind,
    pub observed_revision: Option<ProjectionRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalSuccessKind {
    RuntimeEvent(RuntimeEventKind),
    OperationState {
        operation_id: OperationId,
        state: OperationState,
    },
    Screen(ScreenId),
    Readiness(UiReadiness),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSuccessFact {
    pub handle: SharedActionHandle,
    pub success: TerminalSuccessKind,
    pub observed_revision: Option<ProjectionRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalFailureFact {
    pub handle: SharedActionHandle,
    pub code: String,
    pub detail: Option<String>,
    pub observed_revision: Option<ProjectionRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalTraceEvent {
    ActionRequested {
        request: SharedActionRequest,
        observed_revision: Option<ProjectionRevision>,
    },
    ActionIssued {
        handle: SharedActionHandle,
    },
    TransitionObserved {
        fact: AuthoritativeTransitionFact,
    },
    ActionSucceeded {
        fact: TerminalSuccessFact,
    },
    ActionFailed {
        fact: TerminalFailureFact,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]

pub enum IntentAction {
    OpenScreen {
        screen: ScreenId,
        #[serde(default)]
        channel_id: Option<String>,
        #[serde(default)]
        context_id: Option<String>,
    },
    CreateAccount {
        account_name: String,
    },
    CreateHome {
        home_name: String,
    },
    CreateChannel {
        channel_name: String,
    },
    StartDeviceEnrollment {
        device_name: String,
        code_name: String,
        invitee_authority_id: String,
    },
    ImportDeviceEnrollmentCode {
        code: String,
    },
    OpenSettingsSection(SettingsSection),
    RemoveSelectedDevice {
        #[serde(default)]
        device_id: Option<String>,
    },
    SwitchAuthority {
        authority_id: String,
    },
    CreateContactInvitation {
        receiver_authority_id: String,
        code_name: Option<String>,
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
        #[serde(default)]
        channel_id: Option<String>,
        #[serde(default)]
        context_id: Option<String>,
        #[serde(default)]
        channel_name: Option<String>,
    },
    SendChatMessage {
        message: String,
        #[serde(default)]
        channel_id: Option<String>,
        #[serde(default)]
        context_id: Option<String>,
    },
    SendFriendRequest {
        authority_id: String,
    },
    AcceptFriendRequest {
        authority_id: String,
    },
    DeclineFriendRequest {
        authority_id: String,
    },
}

impl IntentAction {
    #[must_use]
    pub fn kind(&self) -> IntentKind {
        match self {
            Self::OpenScreen { .. } => IntentKind::OpenScreen,
            Self::CreateAccount { .. } => IntentKind::CreateAccount,
            Self::CreateHome { .. } => IntentKind::CreateHome,
            Self::CreateChannel { .. } => IntentKind::CreateChannel,
            Self::StartDeviceEnrollment { .. } => IntentKind::StartDeviceEnrollment,
            Self::ImportDeviceEnrollmentCode { .. } => IntentKind::ImportDeviceEnrollmentCode,
            Self::OpenSettingsSection(_) => IntentKind::OpenSettingsSection,
            Self::RemoveSelectedDevice { .. } => IntentKind::RemoveSelectedDevice,
            Self::SwitchAuthority { .. } => IntentKind::SwitchAuthority,
            Self::CreateContactInvitation { .. } => IntentKind::CreateContactInvitation,
            Self::AcceptContactInvitation { .. } => IntentKind::AcceptContactInvitation,
            Self::AcceptPendingChannelInvitation => IntentKind::AcceptPendingChannelInvitation,
            Self::JoinChannel { .. } => IntentKind::JoinChannel,
            Self::InviteActorToChannel { .. } => IntentKind::InviteActorToChannel,
            Self::SendChatMessage { .. } => IntentKind::SendChatMessage,
            Self::SendFriendRequest { .. } => IntentKind::SendFriendRequest,
            Self::AcceptFriendRequest { .. } => IntentKind::AcceptFriendRequest,
            Self::DeclineFriendRequest { .. } => IntentKind::DeclineFriendRequest,
        }
    }

    #[must_use]
    pub fn contract(&self) -> SharedActionContract {
        match self {
            Self::OpenScreen { screen, .. } => SharedActionContract {
                intent: IntentKind::OpenScreen,
                submission: SubmissionContract::Immediate {
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                    ActionPrecondition::Quiescence(QuiescenceState::Settled),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::Quiescence(QuiescenceState::Settled),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::Screen(*screen),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Screen(*screen),
                selection_semantics: SelectionSemantics::PreservesCurrent,
                transitions: vec![AuthoritativeTransitionKind::Screen(*screen)],
                terminal_success: vec![
                    TerminalSuccessKind::Screen(*screen),
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "screen_navigation_failed".to_string(),
                    "screen_navigation_timeout".to_string(),
                ],
            },
            Self::CreateAccount { .. } => SharedActionContract {
                intent: IntentKind::CreateAccount,
                submission: SubmissionContract::Immediate {
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![ActionPrecondition::Screen(ScreenId::Onboarding)],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![BarrierDeclaration::Screen(ScreenId::Onboarding)],
                    before_next_intent: vec![
                        BarrierDeclaration::Screen(ScreenId::Neighborhood),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Field(FieldId::AccountName),
                selection_semantics: SelectionSemantics::None,
                // Account creation reloads the bootstrap shell into the runtime-backed
                // generation, so the authoritative postcondition is the new shell state
                // rather than the pre-reload local operation handle.
                transitions: vec![AuthoritativeTransitionKind::Screen(ScreenId::Neighborhood)],
                terminal_success: vec![
                    TerminalSuccessKind::Screen(ScreenId::Neighborhood),
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "account_create_issue_failed".to_string(),
                    "account_create_convergence_timeout".to_string(),
                ],
            },
            Self::CreateHome { .. } => SharedActionContract {
                intent: IntentKind::CreateHome,
                submission: SubmissionContract::OperationHandle {
                    operation_id: OperationId::create_home(),
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Neighborhood),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                    ActionPrecondition::Quiescence(QuiescenceState::Settled),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Neighborhood),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::Quiescence(QuiescenceState::Settled),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::OperationState {
                            operation_id: OperationId::create_home(),
                            state: OperationState::Succeeded,
                        },
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Control(ControlId::NeighborhoodNewHomeButton),
                selection_semantics: SelectionSemantics::List(ListId::Homes),
                transitions: vec![AuthoritativeTransitionKind::Operation(
                    OperationId::create_home(),
                )],
                terminal_success: vec![
                    TerminalSuccessKind::OperationState {
                        operation_id: OperationId::create_home(),
                        state: OperationState::Succeeded,
                    },
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "create_home_issue_failed".to_string(),
                    "create_home_convergence_timeout".to_string(),
                ],
            },
            Self::CreateChannel { .. } => SharedActionContract {
                intent: IntentKind::CreateChannel,
                submission: SubmissionContract::Immediate {
                    value: SubmissionValueContract::AuthoritativeChannelBinding,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Chat),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                    ActionPrecondition::Quiescence(QuiescenceState::Settled),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Chat),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::Quiescence(QuiescenceState::Settled),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::RuntimeEvent(RuntimeEventKind::ChannelMembershipReady),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                },
                post_operation_convergence: Some(PostOperationConvergenceContract {
                    required_before_next_intent: vec![BarrierDeclaration::RuntimeEvent(
                        RuntimeEventKind::ChannelMembershipReady,
                    )],
                    violation_code: "channel_membership_convergence_required".to_string(),
                }),
                focus_semantics: FocusSemantics::Screen(ScreenId::Chat),
                selection_semantics: SelectionSemantics::List(ListId::Channels),
                transitions: vec![
                    AuthoritativeTransitionKind::Screen(ScreenId::Chat),
                    AuthoritativeTransitionKind::RuntimeEvent(
                        RuntimeEventKind::ChannelMembershipReady,
                    ),
                ],
                terminal_success: vec![
                    TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::ChannelMembershipReady),
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "create_channel_issue_failed".to_string(),
                    "create_channel_timeout".to_string(),
                ],
            },
            Self::StartDeviceEnrollment { .. } => SharedActionContract {
                intent: IntentKind::StartDeviceEnrollment,
                submission: SubmissionContract::OperationHandle {
                    operation_id: OperationId::device_enrollment(),
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Settings),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                    ActionPrecondition::Quiescence(QuiescenceState::Settled),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Settings),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::Quiescence(QuiescenceState::Settled),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::OperationState {
                            operation_id: OperationId::device_enrollment(),
                            state: OperationState::Succeeded,
                        },
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::RuntimeEvent(
                            RuntimeEventKind::DeviceEnrollmentCodeReady,
                        ),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Screen(ScreenId::Settings),
                selection_semantics: SelectionSemantics::List(ListId::Devices),
                transitions: vec![AuthoritativeTransitionKind::Operation(
                    OperationId::device_enrollment(),
                )],
                terminal_success: vec![
                    TerminalSuccessKind::OperationState {
                        operation_id: OperationId::device_enrollment(),
                        state: OperationState::Succeeded,
                    },
                    TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::DeviceEnrollmentCodeReady),
                ],
                terminal_failure_codes: vec![
                    "device_enrollment_issue_failed".to_string(),
                    "device_enrollment_timeout".to_string(),
                ],
            },
            Self::ImportDeviceEnrollmentCode { .. } => SharedActionContract {
                intent: IntentKind::ImportDeviceEnrollmentCode,
                submission: SubmissionContract::Immediate {
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![ActionPrecondition::Screen(ScreenId::Onboarding)],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![BarrierDeclaration::Screen(ScreenId::Onboarding)],
                    before_next_intent: vec![
                        BarrierDeclaration::Screen(ScreenId::Neighborhood),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Field(FieldId::DeviceImportCode),
                selection_semantics: SelectionSemantics::None,
                transitions: vec![AuthoritativeTransitionKind::Operation(
                    OperationId::device_enrollment(),
                )],
                terminal_success: vec![
                    TerminalSuccessKind::OperationState {
                        operation_id: OperationId::device_enrollment(),
                        state: OperationState::Succeeded,
                    },
                    TerminalSuccessKind::Screen(ScreenId::Neighborhood),
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "device_import_issue_failed".to_string(),
                    "device_import_convergence_timeout".to_string(),
                ],
            },
            Self::OpenSettingsSection(_) => SharedActionContract {
                intent: IntentKind::OpenSettingsSection,
                submission: SubmissionContract::Immediate {
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Settings),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                    ActionPrecondition::Quiescence(QuiescenceState::Settled),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Settings),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::Quiescence(QuiescenceState::Settled),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::Screen(ScreenId::Settings),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Screen(ScreenId::Settings),
                selection_semantics: SelectionSemantics::List(ListId::SettingsSections),
                transitions: vec![AuthoritativeTransitionKind::Screen(ScreenId::Settings)],
                terminal_success: vec![
                    TerminalSuccessKind::Screen(ScreenId::Settings),
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "settings_section_navigation_failed".to_string(),
                    "settings_section_navigation_timeout".to_string(),
                ],
            },
            Self::RemoveSelectedDevice { .. } => SharedActionContract {
                intent: IntentKind::RemoveSelectedDevice,
                submission: SubmissionContract::Immediate {
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Settings),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                    ActionPrecondition::Quiescence(QuiescenceState::Settled),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Settings),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::Quiescence(QuiescenceState::Settled),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::Screen(ScreenId::Settings),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::Quiescence(QuiescenceState::Settled),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Screen(ScreenId::Settings),
                selection_semantics: SelectionSemantics::List(ListId::Devices),
                transitions: vec![AuthoritativeTransitionKind::Screen(ScreenId::Settings)],
                terminal_success: vec![
                    TerminalSuccessKind::Screen(ScreenId::Settings),
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "remove_device_issue_failed".to_string(),
                    "remove_device_timeout".to_string(),
                ],
            },
            Self::SwitchAuthority { .. } => SharedActionContract {
                intent: IntentKind::SwitchAuthority,
                submission: SubmissionContract::Immediate {
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Settings),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                    ActionPrecondition::Quiescence(QuiescenceState::Settled),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Settings),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::Quiescence(QuiescenceState::Settled),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::Screen(ScreenId::Settings),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Screen(ScreenId::Settings),
                selection_semantics: SelectionSemantics::List(ListId::Authorities),
                transitions: vec![AuthoritativeTransitionKind::Screen(ScreenId::Settings)],
                terminal_success: vec![
                    TerminalSuccessKind::Screen(ScreenId::Settings),
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "switch_authority_issue_failed".to_string(),
                    "switch_authority_timeout".to_string(),
                ],
            },
            Self::CreateContactInvitation { .. } => SharedActionContract {
                intent: IntentKind::CreateContactInvitation,
                submission: SubmissionContract::OperationHandle {
                    operation_id: OperationId::invitation_create(),
                    value: SubmissionValueContract::ContactInvitationCode,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Contacts),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                    ActionPrecondition::Quiescence(QuiescenceState::Settled),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Contacts),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::Quiescence(QuiescenceState::Settled),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::OperationState {
                            operation_id: OperationId::invitation_create(),
                            state: OperationState::Succeeded,
                        },
                        BarrierDeclaration::RuntimeEvent(RuntimeEventKind::InvitationCodeReady),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Screen(ScreenId::Contacts),
                selection_semantics: SelectionSemantics::List(ListId::Contacts),
                transitions: vec![AuthoritativeTransitionKind::Operation(
                    OperationId::invitation_create(),
                )],
                terminal_success: vec![
                    TerminalSuccessKind::OperationState {
                        operation_id: OperationId::invitation_create(),
                        state: OperationState::Succeeded,
                    },
                    TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::InvitationCodeReady),
                ],
                terminal_failure_codes: vec![
                    "contact_invitation_issue_failed".to_string(),
                    "contact_invitation_timeout".to_string(),
                ],
            },
            Self::AcceptContactInvitation { .. } => SharedActionContract {
                intent: IntentKind::AcceptContactInvitation,
                submission: SubmissionContract::OperationHandle {
                    operation_id: OperationId::invitation_accept_contact(),
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Contacts),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                    ActionPrecondition::Quiescence(QuiescenceState::Settled),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Contacts),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::Quiescence(QuiescenceState::Settled),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::OperationState {
                            operation_id: OperationId::invitation_accept_contact(),
                            state: OperationState::Succeeded,
                        },
                        BarrierDeclaration::RuntimeEvent(RuntimeEventKind::InvitationAccepted),
                        BarrierDeclaration::RuntimeEvent(RuntimeEventKind::ContactLinkReady),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Field(FieldId::InvitationCode),
                selection_semantics: SelectionSemantics::List(ListId::Contacts),
                transitions: vec![AuthoritativeTransitionKind::Operation(
                    OperationId::invitation_accept_contact(),
                )],
                terminal_success: vec![
                    TerminalSuccessKind::OperationState {
                        operation_id: OperationId::invitation_accept_contact(),
                        state: OperationState::Succeeded,
                    },
                    TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::InvitationAccepted),
                    TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::ContactLinkReady),
                ],
                terminal_failure_codes: vec![
                    "contact_invitation_accept_issue_failed".to_string(),
                    "contact_invitation_accept_timeout".to_string(),
                ],
            },
            Self::AcceptPendingChannelInvitation => SharedActionContract {
                intent: IntentKind::AcceptPendingChannelInvitation,
                submission: SubmissionContract::OperationHandle {
                    operation_id: OperationId::invitation_accept_channel(),
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                    ActionPrecondition::Quiescence(QuiescenceState::Settled),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::Quiescence(QuiescenceState::Settled),
                    ],
                    before_next_intent: vec![BarrierDeclaration::Readiness(UiReadiness::Ready)],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Screen(ScreenId::Chat),
                selection_semantics: SelectionSemantics::PreservesCurrent,
                transitions: vec![AuthoritativeTransitionKind::Screen(ScreenId::Chat)],
                terminal_success: vec![
                    TerminalSuccessKind::Screen(ScreenId::Chat),
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "pending_channel_invitation_issue_failed".to_string(),
                    "pending_channel_invitation_timeout".to_string(),
                ],
            },
            Self::JoinChannel { .. } => SharedActionContract {
                intent: IntentKind::JoinChannel,
                submission: SubmissionContract::OperationHandle {
                    operation_id: OperationId::join_channel(),
                    value: SubmissionValueContract::AuthoritativeChannelBinding,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Chat),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Chat),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::RuntimeEvent(RuntimeEventKind::ChannelMembershipReady),
                    ],
                },
                post_operation_convergence: Some(PostOperationConvergenceContract {
                    required_before_next_intent: vec![BarrierDeclaration::RuntimeEvent(
                        RuntimeEventKind::ChannelMembershipReady,
                    )],
                    violation_code: "channel_membership_convergence_required".to_string(),
                }),
                focus_semantics: FocusSemantics::Screen(ScreenId::Chat),
                selection_semantics: SelectionSemantics::List(ListId::Channels),
                transitions: vec![AuthoritativeTransitionKind::RuntimeEvent(
                    RuntimeEventKind::ChannelJoined,
                )],
                terminal_success: vec![
                    TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::ChannelJoined),
                    TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::ChannelMembershipReady),
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "join_channel_issue_failed".to_string(),
                    "join_channel_timeout".to_string(),
                ],
            },
            Self::InviteActorToChannel { .. } => SharedActionContract {
                intent: IntentKind::InviteActorToChannel,
                submission: SubmissionContract::OperationHandle {
                    operation_id: OperationId::invitation_create(),
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Contacts),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Contacts),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::OperationState {
                            operation_id: OperationId::invitation_create(),
                            state: OperationState::Succeeded,
                        },
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Screen(ScreenId::Contacts),
                selection_semantics: SelectionSemantics::List(ListId::Contacts),
                transitions: vec![AuthoritativeTransitionKind::Operation(
                    OperationId::invitation_create(),
                )],
                terminal_success: vec![
                    TerminalSuccessKind::OperationState {
                        operation_id: OperationId::invitation_create(),
                        state: OperationState::Succeeded,
                    },
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "invite_actor_to_channel_issue_failed".to_string(),
                    "invite_actor_to_channel_timeout".to_string(),
                ],
            },
            Self::SendChatMessage { .. } => SharedActionContract {
                intent: IntentKind::SendChatMessage,
                submission: SubmissionContract::OperationHandle {
                    operation_id: OperationId::send_message(),
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Chat),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Chat),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                        BarrierDeclaration::RuntimeEvent(RuntimeEventKind::MessageDeliveryReady),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::RuntimeEvent(RuntimeEventKind::MessageCommitted),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Screen(ScreenId::Chat),
                selection_semantics: SelectionSemantics::List(ListId::Channels),
                transitions: vec![AuthoritativeTransitionKind::RuntimeEvent(
                    RuntimeEventKind::MessageCommitted,
                )],
                terminal_success: vec![
                    TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::MessageCommitted),
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "send_chat_message_issue_failed".to_string(),
                    "send_chat_message_timeout".to_string(),
                ],
            },
            Self::SendFriendRequest { .. } => SharedActionContract {
                intent: IntentKind::SendFriendRequest,
                submission: SubmissionContract::OperationHandle {
                    operation_id: OperationId::send_friend_request(),
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Contacts),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Contacts),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::OperationState {
                            operation_id: OperationId::send_friend_request(),
                            state: OperationState::Succeeded,
                        },
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Screen(ScreenId::Contacts),
                selection_semantics: SelectionSemantics::List(ListId::Contacts),
                transitions: vec![AuthoritativeTransitionKind::Operation(
                    OperationId::send_friend_request(),
                )],
                terminal_success: vec![
                    TerminalSuccessKind::OperationState {
                        operation_id: OperationId::send_friend_request(),
                        state: OperationState::Succeeded,
                    },
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "send_friend_request_issue_failed".to_string(),
                    "send_friend_request_timeout".to_string(),
                ],
            },
            Self::AcceptFriendRequest { .. } => SharedActionContract {
                intent: IntentKind::AcceptFriendRequest,
                submission: SubmissionContract::OperationHandle {
                    operation_id: OperationId::accept_friend_request(),
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Contacts),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Contacts),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::OperationState {
                            operation_id: OperationId::accept_friend_request(),
                            state: OperationState::Succeeded,
                        },
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Screen(ScreenId::Contacts),
                selection_semantics: SelectionSemantics::List(ListId::Contacts),
                transitions: vec![AuthoritativeTransitionKind::Operation(
                    OperationId::accept_friend_request(),
                )],
                terminal_success: vec![
                    TerminalSuccessKind::OperationState {
                        operation_id: OperationId::accept_friend_request(),
                        state: OperationState::Succeeded,
                    },
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "accept_friend_request_issue_failed".to_string(),
                    "accept_friend_request_timeout".to_string(),
                ],
            },
            Self::DeclineFriendRequest { .. } => SharedActionContract {
                intent: IntentKind::DeclineFriendRequest,
                submission: SubmissionContract::OperationHandle {
                    operation_id: OperationId::decline_friend_request(),
                    value: SubmissionValueContract::None,
                },
                preconditions: vec![
                    ActionPrecondition::Screen(ScreenId::Contacts),
                    ActionPrecondition::Readiness(UiReadiness::Ready),
                ],
                barriers: SharedActionBarrierMetadata {
                    before_issue: vec![
                        BarrierDeclaration::Screen(ScreenId::Contacts),
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                    before_next_intent: vec![
                        BarrierDeclaration::OperationState {
                            operation_id: OperationId::decline_friend_request(),
                            state: OperationState::Succeeded,
                        },
                        BarrierDeclaration::Readiness(UiReadiness::Ready),
                    ],
                },
                post_operation_convergence: None,
                focus_semantics: FocusSemantics::Screen(ScreenId::Contacts),
                selection_semantics: SelectionSemantics::List(ListId::Contacts),
                transitions: vec![AuthoritativeTransitionKind::Operation(
                    OperationId::decline_friend_request(),
                )],
                terminal_success: vec![
                    TerminalSuccessKind::OperationState {
                        operation_id: OperationId::decline_friend_request(),
                        state: OperationState::Succeeded,
                    },
                    TerminalSuccessKind::Readiness(UiReadiness::Ready),
                ],
                terminal_failure_codes: vec![
                    "decline_friend_request_issue_failed".to_string(),
                    "decline_friend_request_timeout".to_string(),
                ],
            },
        }
    }
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
    PrepareDeviceEnrollmentInviteeAuthority {
        name: String,
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
