//! Internal executor step model for frontend-conformance harness flows.
//!
//! Scenario files now load through the semantic contract in `aura-app::scenario_contract`.
//! These types remain as an execution IR for the harness executor and targeted tests.

use std::fmt;

use aura_app::scenario_contract::SettingsSection;
use aura_app::ui::contract::{
    ConfirmationState, ControlId, ListId, ModalId, OperationId, OperationState, RuntimeEventKind,
    ScreenId, UiReadiness,
};
use aura_app::ui_contract::QuiescenceState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityAction {
    #[default]
    LaunchInstances,
    SetVar,
    CaptureSelection,
    ExtractVar,
    SendKeys,
    SendChatCommand,
    SendClipboard,
    AssertParity,
    WaitFor,
    MessageContains,
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

impl fmt::Display for CompatibilityAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::LaunchInstances => "launch_instances",
            Self::SetVar => "set_var",
            Self::CaptureSelection => "capture_selection",
            Self::ExtractVar => "extract_var",
            Self::SendKeys => "send_keys",
            Self::SendChatCommand => "send_chat_command",
            Self::SendClipboard => "send_clipboard",
            Self::AssertParity => "assert_parity",
            Self::WaitFor => "wait_for",
            Self::MessageContains => "message_contains",
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
pub struct CompatibilityStep {
    pub id: String,
    pub action: CompatibilityAction,
    pub instance: Option<String>,
    pub timeout_ms: Option<u64>,
    pub request_id: Option<u64>,
    pub keys: Option<String>,
    pub screen_source: Option<ScreenSource>,
    pub command: Option<String>,
    pub pattern: Option<String>,
    pub selector: Option<String>,
    pub screen_id: Option<ScreenId>,
    pub control_id: Option<ControlId>,
    pub modal_id: Option<ModalId>,
    pub readiness: Option<UiReadiness>,
    pub quiescence: Option<QuiescenceState>,
    pub runtime_event_kind: Option<RuntimeEventKind>,
    pub operation_id: Option<OperationId>,
    pub operation_state: Option<OperationState>,
    pub list_id: Option<ListId>,
    pub item_id: Option<String>,
    pub count: Option<usize>,
    pub confirmation: Option<ConfirmationState>,
    pub source_instance: Option<String>,
    pub peer_instance: Option<String>,
    pub var: Option<String>,
    pub value: Option<String>,
    pub regex: Option<String>,
    pub group: Option<u32>,
    pub from: Option<String>,
    pub contains: Option<String>,
    pub level: Option<String>,
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

pub(crate) fn settings_section_item_id(section: SettingsSection) -> &'static str {
    match section {
        SettingsSection::Devices => "devices",
    }
}
