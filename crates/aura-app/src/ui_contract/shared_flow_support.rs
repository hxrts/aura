use serde::{Deserialize, Serialize};

use super::{ListId, ModalId, ScreenId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParityException {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParityExceptionMetadata {
    pub exception: ParityException,
    pub reason_code: &'static str,
    pub scope: &'static str,
    pub affected_surface: &'static str,
    pub doc_reference: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowAvailability {
    Supported,
    Exception(ParityException),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedFlowId {
    NavigateNeighborhood,
    NavigateChat,
    NavigateContacts,
    NavigateNotifications,
    NavigateSettings,
    CreateInvitation,
    AcceptInvitation,
    CreateHome,
    JoinChannel,
    SendChatMessage,
    AddDevice,
    RemoveDevice,
    SwitchAuthority,
}

pub const ALL_SHARED_FLOW_IDS: &[SharedFlowId] = &[
    SharedFlowId::NavigateNeighborhood,
    SharedFlowId::NavigateChat,
    SharedFlowId::NavigateContacts,
    SharedFlowId::NavigateNotifications,
    SharedFlowId::NavigateSettings,
    SharedFlowId::CreateInvitation,
    SharedFlowId::AcceptInvitation,
    SharedFlowId::CreateHome,
    SharedFlowId::JoinChannel,
    SharedFlowId::SendChatMessage,
    SharedFlowId::AddDevice,
    SharedFlowId::RemoveDevice,
    SharedFlowId::SwitchAuthority,
];

pub const PARITY_EXCEPTION_METADATA: &[ParityExceptionMetadata] = &[];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedFlowSupport {
    pub flow: SharedFlowId,
    pub web: FlowAvailability,
    pub tui: FlowAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedFlowScenarioCoverage {
    pub flow: SharedFlowId,
    pub scenario_id: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedFlowSourceArea {
    pub flow: SharedFlowId,
    pub path: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedScreenSupport {
    pub screen: ScreenId,
    pub web: FlowAvailability,
    pub tui: FlowAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedModalSupport {
    pub modal: ModalId,
    pub web: FlowAvailability,
    pub tui: FlowAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedListSupport {
    pub list: ListId,
    pub web: FlowAvailability,
    pub tui: FlowAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedScreenModuleMap {
    pub screen: ScreenId,
    pub web_symbol: &'static str,
    pub web_path: &'static str,
    pub tui_symbol: &'static str,
    pub tui_path: &'static str,
}

macro_rules! shared_support {
    ($ty:ident { $field:ident: $value:expr }) => {
        $ty {
            $field: $value,
            web: FlowAvailability::Supported,
            tui: FlowAvailability::Supported,
        }
    };
}

pub const SHARED_SCREEN_SUPPORT: &[SharedScreenSupport] = &[
    shared_support!(SharedScreenSupport {
        screen: ScreenId::Onboarding
    }),
    shared_support!(SharedScreenSupport {
        screen: ScreenId::Neighborhood
    }),
    shared_support!(SharedScreenSupport {
        screen: ScreenId::Chat
    }),
    shared_support!(SharedScreenSupport {
        screen: ScreenId::Contacts
    }),
    shared_support!(SharedScreenSupport {
        screen: ScreenId::Notifications
    }),
    shared_support!(SharedScreenSupport {
        screen: ScreenId::Settings
    }),
];

pub const SHARED_MODAL_SUPPORT: &[SharedModalSupport] = &[
    shared_support!(SharedModalSupport {
        modal: ModalId::Help
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::CreateInvitation
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::AcceptContactInvitation
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::AcceptChannelInvitation
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::CreateHome
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::CreateChannel
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::ChannelInfo
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::EditNickname
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::GuardianSetup
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::RequestRecovery
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::AddDevice
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::ImportDeviceEnrollmentCode
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::SelectDeviceToRemove
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::ConfirmRemoveDevice
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::MfaSetup
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::AssignModerator
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::SwitchAuthority
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::AccessOverride
    }),
    shared_support!(SharedModalSupport {
        modal: ModalId::CapabilityConfig
    }),
];

pub const SHARED_LIST_SUPPORT: &[SharedListSupport] = &[
    shared_support!(SharedListSupport {
        list: ListId::Navigation
    }),
    shared_support!(SharedListSupport {
        list: ListId::Contacts
    }),
    shared_support!(SharedListSupport {
        list: ListId::Channels
    }),
    shared_support!(SharedListSupport {
        list: ListId::Notifications
    }),
    shared_support!(SharedListSupport {
        list: ListId::SettingsSections
    }),
    shared_support!(SharedListSupport {
        list: ListId::Homes
    }),
    shared_support!(SharedListSupport {
        list: ListId::NeighborhoodMembers
    }),
];

pub const SHARED_SCREEN_MODULE_MAP: &[SharedScreenModuleMap] = &[
    SharedScreenModuleMap {
        screen: ScreenId::Onboarding,
        web_symbol: "OnboardingScreen",
        web_path: "crates/aura-ui/src/app/screens/mod.rs",
        tui_symbol: "AccountSetupModal",
        tui_path: "crates/aura-terminal/src/tui/components/account_setup_modal_template.rs",
    },
    SharedScreenModuleMap {
        screen: ScreenId::Neighborhood,
        web_symbol: "NeighborhoodScreen",
        web_path: "crates/aura-ui/src/app/screens/neighborhood.rs",
        tui_symbol: "NeighborhoodScreen",
        tui_path: "crates/aura-terminal/src/tui/screens/neighborhood/screen.rs",
    },
    SharedScreenModuleMap {
        screen: ScreenId::Chat,
        web_symbol: "ChatScreen",
        web_path: "crates/aura-ui/src/app/screens/chat.rs",
        tui_symbol: "ChatScreen",
        tui_path: "crates/aura-terminal/src/tui/screens/chat/screen.rs",
    },
    SharedScreenModuleMap {
        screen: ScreenId::Contacts,
        web_symbol: "ContactsScreen",
        web_path: "crates/aura-ui/src/app/screens/contacts.rs",
        tui_symbol: "ContactsScreen",
        tui_path: "crates/aura-terminal/src/tui/screens/contacts/screen.rs",
    },
    SharedScreenModuleMap {
        screen: ScreenId::Notifications,
        web_symbol: "NotificationsScreen",
        web_path: "crates/aura-ui/src/app/screens/notifications.rs",
        tui_symbol: "NotificationsScreen",
        tui_path: "crates/aura-terminal/src/tui/screens/notifications/screen.rs",
    },
    SharedScreenModuleMap {
        screen: ScreenId::Settings,
        web_symbol: "SettingsScreen",
        web_path: "crates/aura-ui/src/app/screens/settings.rs",
        tui_symbol: "SettingsScreen",
        tui_path: "crates/aura-terminal/src/tui/screens/settings/screen.rs",
    },
];

pub const SHARED_FLOW_SUPPORT: &[SharedFlowSupport] = &[
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::NavigateNeighborhood
    }),
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::NavigateChat
    }),
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::NavigateContacts
    }),
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::NavigateNotifications
    }),
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::NavigateSettings
    }),
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::CreateInvitation
    }),
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::AcceptInvitation
    }),
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::CreateHome
    }),
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::JoinChannel
    }),
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::SendChatMessage
    }),
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::AddDevice
    }),
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::RemoveDevice
    }),
    shared_support!(SharedFlowSupport {
        flow: SharedFlowId::SwitchAuthority
    }),
];

pub const SHARED_FLOW_SCENARIO_COVERAGE: &[SharedFlowScenarioCoverage] = &[
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateNeighborhood,
        scenario_id: "scenario13-mixed-contact-channel-message-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateNeighborhood,
        scenario_id: "semantic-observation-browser-smoke",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateNeighborhood,
        scenario_id: "semantic-observation-tui-smoke",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateChat,
        scenario_id: "scenario13-mixed-contact-channel-message-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateChat,
        scenario_id: "semantic-observation-browser-smoke",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateChat,
        scenario_id: "semantic-observation-tui-smoke",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateContacts,
        scenario_id: "scenario13-mixed-contact-channel-message-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateContacts,
        scenario_id: "semantic-observation-browser-smoke",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateContacts,
        scenario_id: "semantic-observation-tui-smoke",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateNotifications,
        scenario_id: "shared-notifications-and-authority",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateSettings,
        scenario_id: "shared-notifications-and-authority",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::CreateInvitation,
        scenario_id: "scenario13-mixed-contact-channel-message-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::CreateInvitation,
        scenario_id: "semantic-observation-browser-smoke",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::CreateInvitation,
        scenario_id: "semantic-observation-tui-smoke",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::AcceptInvitation,
        scenario_id: "scenario13-mixed-contact-channel-message-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::AcceptInvitation,
        scenario_id: "semantic-observation-browser-smoke",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::AcceptInvitation,
        scenario_id: "semantic-observation-tui-smoke",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::CreateHome,
        scenario_id: "scenario13-mixed-contact-channel-message-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::JoinChannel,
        scenario_id: "scenario13-mixed-contact-channel-message-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::SendChatMessage,
        scenario_id: "scenario13-mixed-contact-channel-message-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::AddDevice,
        scenario_id: "scenario12-mixed-device-enrollment-removal-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::RemoveDevice,
        scenario_id: "scenario12-mixed-device-enrollment-removal-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::SwitchAuthority,
        scenario_id: "shared-notifications-and-authority",
    },
];

pub const SHARED_FLOW_SOURCE_AREAS: &[SharedFlowSourceArea] = &[
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateNeighborhood,
        path: "crates/aura-app/src/workflows/context.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateNeighborhood,
        path: "crates/aura-terminal/src/tui/screens/neighborhood/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateNeighborhood,
        path: "crates/aura-ui/src/app/screens/neighborhood.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateNeighborhood,
        path: "crates/aura-web/src/main.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateChat,
        path: "crates/aura-app/src/workflows/messaging.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateChat,
        path: "crates/aura-terminal/src/tui/screens/chat/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateChat,
        path: "crates/aura-ui/src/app/screens/chat.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateChat,
        path: "crates/aura-web/src/main.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateContacts,
        path: "crates/aura-app/src/workflows/invitation.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateContacts,
        path: "crates/aura-terminal/src/tui/screens/contacts/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateContacts,
        path: "crates/aura-ui/src/app/screens/contacts.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateContacts,
        path: "crates/aura-web/src/main.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateNotifications,
        path: "crates/aura-app/src/workflows/recovery.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateNotifications,
        path: "crates/aura-terminal/src/tui/screens/notifications/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateNotifications,
        path: "crates/aura-ui/src/app/screens/notifications.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateNotifications,
        path: "crates/aura-web/src/main.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateSettings,
        path: "crates/aura-app/src/workflows/settings.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateSettings,
        path: "crates/aura-terminal/src/tui/screens/settings/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateSettings,
        path: "crates/aura-ui/src/app/screens/settings.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::NavigateSettings,
        path: "crates/aura-web/src/main.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::CreateInvitation,
        path: "crates/aura-app/src/workflows/invitation.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::CreateInvitation,
        path: "crates/aura-terminal/src/tui/screens/contacts/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::CreateInvitation,
        path: "crates/aura-ui/src/app/screens/contacts.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::CreateInvitation,
        path: "crates/aura-web/src/main.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::AcceptInvitation,
        path: "crates/aura-app/src/workflows/invitation.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::AcceptInvitation,
        path: "crates/aura-terminal/src/tui/screens/contacts/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::AcceptInvitation,
        path: "crates/aura-terminal/src/tui/screens/notifications/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::AcceptInvitation,
        path: "crates/aura-ui/src/app/screens/contacts.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::AcceptInvitation,
        path: "crates/aura-web/src/main.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::CreateHome,
        path: "crates/aura-app/src/workflows/context.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::CreateHome,
        path: "crates/aura-terminal/src/tui/screens/neighborhood/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::CreateHome,
        path: "crates/aura-ui/src/app/screens/neighborhood.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::CreateHome,
        path: "crates/aura-web/src/main.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::JoinChannel,
        path: "crates/aura-app/src/workflows/messaging.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::JoinChannel,
        path: "crates/aura-terminal/src/tui/screens/chat/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::JoinChannel,
        path: "crates/aura-ui/src/app/screens/chat.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::JoinChannel,
        path: "crates/aura-web/src/main.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::SendChatMessage,
        path: "crates/aura-app/src/workflows/messaging.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::SendChatMessage,
        path: "crates/aura-terminal/src/tui/screens/chat/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::SendChatMessage,
        path: "crates/aura-ui/src/app/screens/chat.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::SendChatMessage,
        path: "crates/aura-web/src/main.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::AddDevice,
        path: "crates/aura-app/src/workflows/settings.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::AddDevice,
        path: "crates/aura-terminal/src/tui/screens/settings/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::AddDevice,
        path: "crates/aura-ui/src/app/screens/settings.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::AddDevice,
        path: "crates/aura-web/src/main.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::RemoveDevice,
        path: "crates/aura-app/src/workflows/settings.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::RemoveDevice,
        path: "crates/aura-terminal/src/tui/screens/settings/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::RemoveDevice,
        path: "crates/aura-ui/src/app/screens/settings.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::RemoveDevice,
        path: "crates/aura-web/src/main.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::SwitchAuthority,
        path: "crates/aura-app/src/workflows/settings.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::SwitchAuthority,
        path: "crates/aura-terminal/src/tui/screens/settings/screen.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::SwitchAuthority,
        path: "crates/aura-ui/src/app/screens/settings.rs",
    },
    SharedFlowSourceArea {
        flow: SharedFlowId::SwitchAuthority,
        path: "crates/aura-web/src/main.rs",
    },
];

#[must_use]
pub fn shared_flow_support(flow: SharedFlowId) -> &'static SharedFlowSupport {
    let Some(support) = SHARED_FLOW_SUPPORT
        .iter()
        .find(|support| support.flow == flow)
    else {
        panic!("shared flow support must be declared for {flow:?}");
    };
    support
}

#[must_use]
pub fn shared_flow_scenarios(flow: SharedFlowId) -> Vec<&'static str> {
    SHARED_FLOW_SCENARIO_COVERAGE
        .iter()
        .filter(|coverage| coverage.flow == flow)
        .map(|coverage| coverage.scenario_id)
        .collect()
}

#[must_use]
pub fn shared_flow_source_areas(flow: SharedFlowId) -> Vec<&'static str> {
    SHARED_FLOW_SOURCE_AREAS
        .iter()
        .filter(|area| area.flow == flow)
        .map(|area| area.path)
        .collect()
}

#[must_use]
pub fn shared_screen_support(screen: ScreenId) -> &'static SharedScreenSupport {
    let Some(support) = SHARED_SCREEN_SUPPORT
        .iter()
        .find(|support| support.screen == screen)
    else {
        panic!("shared screen support must be declared for {screen:?}");
    };
    support
}

#[must_use]
pub fn shared_modal_support(modal: ModalId) -> &'static SharedModalSupport {
    let Some(support) = SHARED_MODAL_SUPPORT
        .iter()
        .find(|support| support.modal == modal)
    else {
        panic!("shared modal support must be declared for {modal:?}");
    };
    support
}

#[must_use]
pub fn shared_list_support(list: ListId) -> &'static SharedListSupport {
    let Some(support) = SHARED_LIST_SUPPORT
        .iter()
        .find(|support| support.list == list)
    else {
        panic!("shared list support must be declared for {list:?}");
    };
    support
}

#[must_use]
pub fn shared_screen_module_map(screen: ScreenId) -> &'static SharedScreenModuleMap {
    let Some(mapping) = SHARED_SCREEN_MODULE_MAP
        .iter()
        .find(|mapping| mapping.screen == screen)
    else {
        panic!("shared screen module mapping must be declared for {screen:?}");
    };
    mapping
}
