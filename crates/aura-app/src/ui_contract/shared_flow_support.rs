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

impl SharedFlowId {
    pub const ALL: [Self; 13] = [
        Self::NavigateNeighborhood,
        Self::NavigateChat,
        Self::NavigateContacts,
        Self::NavigateNotifications,
        Self::NavigateSettings,
        Self::CreateInvitation,
        Self::AcceptInvitation,
        Self::CreateHome,
        Self::JoinChannel,
        Self::SendChatMessage,
        Self::AddDevice,
        Self::RemoveDevice,
        Self::SwitchAuthority,
    ];
}

pub const ALL_SHARED_FLOW_IDS: &[SharedFlowId] = &SharedFlowId::ALL;

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

macro_rules! shared_screen_module {
    ($screen:ident, $web_symbol:expr, $web_path:expr, $tui_symbol:expr, $tui_path:expr) => {
        SharedScreenModuleMap {
            screen: ScreenId::$screen,
            web_symbol: $web_symbol,
            web_path: $web_path,
            tui_symbol: $tui_symbol,
            tui_path: $tui_path,
        }
    };
}

macro_rules! shared_flow_scenario_table {
    ($($flow:ident => [$($scenario_id:expr),+ $(,)?]),+ $(,)?) => {
        &[
            $(
                $(SharedFlowScenarioCoverage {
                    flow: SharedFlowId::$flow,
                    scenario_id: $scenario_id,
                },)+
            )+
        ]
    };
}

macro_rules! shared_flow_source_table {
    ($($flow:ident => [$($path:expr),+ $(,)?]),+ $(,)?) => {
        &[
            $(
                $(SharedFlowSourceArea {
                    flow: SharedFlowId::$flow,
                    path: $path,
                },)+
            )+
        ]
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
    shared_screen_module!(
        Onboarding,
        "OnboardingScreen",
        "crates/aura-ui/src/app/screens/mod.rs",
        "AccountSetupModal",
        "crates/aura-terminal/src/tui/components/account_setup_modal_template.rs"
    ),
    shared_screen_module!(
        Neighborhood,
        "NeighborhoodScreen",
        "crates/aura-ui/src/app/screens/neighborhood.rs",
        "NeighborhoodScreen",
        "crates/aura-terminal/src/tui/screens/neighborhood/screen.rs"
    ),
    shared_screen_module!(
        Chat,
        "ChatScreen",
        "crates/aura-ui/src/app/screens/chat.rs",
        "ChatScreen",
        "crates/aura-terminal/src/tui/screens/chat/screen.rs"
    ),
    shared_screen_module!(
        Contacts,
        "ContactsScreen",
        "crates/aura-ui/src/app/screens/contacts.rs",
        "ContactsScreen",
        "crates/aura-terminal/src/tui/screens/contacts/screen.rs"
    ),
    shared_screen_module!(
        Notifications,
        "NotificationsScreen",
        "crates/aura-ui/src/app/screens/notifications.rs",
        "NotificationsScreen",
        "crates/aura-terminal/src/tui/screens/notifications/screen.rs"
    ),
    shared_screen_module!(
        Settings,
        "SettingsScreen",
        "crates/aura-ui/src/app/screens/settings.rs",
        "SettingsScreen",
        "crates/aura-terminal/src/tui/screens/settings/screen.rs"
    ),
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

pub const SHARED_FLOW_SCENARIO_COVERAGE: &[SharedFlowScenarioCoverage] = shared_flow_scenario_table! {
    NavigateNeighborhood => [
        "scenario13-mixed-contact-channel-message-e2e",
        "semantic-observation-browser-smoke",
        "semantic-observation-tui-smoke",
    ],
    NavigateChat => [
        "scenario13-mixed-contact-channel-message-e2e",
        "semantic-observation-browser-smoke",
        "semantic-observation-tui-smoke",
    ],
    NavigateContacts => [
        "scenario13-mixed-contact-channel-message-e2e",
        "semantic-observation-browser-smoke",
        "semantic-observation-tui-smoke",
    ],
    NavigateNotifications => [
        "shared-notifications-and-authority",
    ],
    NavigateSettings => [
        "shared-notifications-and-authority",
    ],
    CreateInvitation => [
        "scenario13-mixed-contact-channel-message-e2e",
        "semantic-observation-browser-smoke",
        "semantic-observation-tui-smoke",
    ],
    AcceptInvitation => [
        "scenario13-mixed-contact-channel-message-e2e",
        "semantic-observation-browser-smoke",
        "semantic-observation-tui-smoke",
    ],
    CreateHome => [
        "scenario13-mixed-contact-channel-message-e2e",
    ],
    JoinChannel => [
        "scenario13-mixed-contact-channel-message-e2e",
    ],
    SendChatMessage => [
        "scenario13-mixed-contact-channel-message-e2e",
    ],
    AddDevice => [
        "scenario12-mixed-device-enrollment-removal-e2e",
    ],
    RemoveDevice => [
        "scenario12-mixed-device-enrollment-removal-e2e",
    ],
    SwitchAuthority => [
        "shared-notifications-and-authority",
    ],
};

pub const SHARED_FLOW_SOURCE_AREAS: &[SharedFlowSourceArea] = shared_flow_source_table! {
    NavigateNeighborhood => [
        "crates/aura-app/src/workflows/context.rs",
        "crates/aura-app/src/workflows/context/neighborhood.rs",
        "crates/aura-terminal/src/tui/screens/neighborhood/screen.rs",
        "crates/aura-ui/src/app/screens/neighborhood.rs",
        "crates/aura-web/src/main.rs",
    ],
    NavigateChat => [
        "crates/aura-app/src/workflows/messaging.rs",
        "crates/aura-app/src/workflows/messaging/channel_refs.rs",
        "crates/aura-app/src/workflows/messaging/channels.rs",
        "crates/aura-app/src/workflows/messaging/send.rs",
        "crates/aura-terminal/src/tui/screens/chat/screen.rs",
        "crates/aura-ui/src/app/screens/chat.rs",
        "crates/aura-web/src/main.rs",
    ],
    NavigateContacts => [
        "crates/aura-app/src/workflows/invitation.rs",
        "crates/aura-app/src/workflows/invitation/accept.rs",
        "crates/aura-app/src/workflows/invitation/create.rs",
        "crates/aura-app/src/workflows/invitation/readiness.rs",
        "crates/aura-terminal/src/tui/screens/contacts/screen.rs",
        "crates/aura-ui/src/app/screens/contacts.rs",
        "crates/aura-web/src/main.rs",
    ],
    NavigateNotifications => [
        "crates/aura-app/src/workflows/recovery.rs",
        "crates/aura-terminal/src/tui/screens/notifications/screen.rs",
        "crates/aura-ui/src/app/screens/notifications.rs",
        "crates/aura-web/src/main.rs",
    ],
    NavigateSettings => [
        "crates/aura-app/src/workflows/settings.rs",
        "crates/aura-terminal/src/tui/screens/settings/screen.rs",
        "crates/aura-ui/src/app/screens/settings.rs",
        "crates/aura-web/src/main.rs",
    ],
    CreateInvitation => [
        "crates/aura-app/src/workflows/invitation.rs",
        "crates/aura-app/src/workflows/invitation/create.rs",
        "crates/aura-terminal/src/tui/screens/contacts/screen.rs",
        "crates/aura-ui/src/app/screens/contacts.rs",
        "crates/aura-web/src/main.rs",
    ],
    AcceptInvitation => [
        "crates/aura-app/src/workflows/invitation.rs",
        "crates/aura-app/src/workflows/invitation/accept.rs",
        "crates/aura-app/src/workflows/invitation/readiness.rs",
        "crates/aura-terminal/src/tui/screens/contacts/screen.rs",
        "crates/aura-terminal/src/tui/screens/notifications/screen.rs",
        "crates/aura-ui/src/app/screens/contacts.rs",
        "crates/aura-web/src/main.rs",
    ],
    CreateHome => [
        "crates/aura-app/src/workflows/context.rs",
        "crates/aura-app/src/workflows/context/neighborhood.rs",
        "crates/aura-terminal/src/tui/screens/neighborhood/screen.rs",
        "crates/aura-ui/src/app/screens/neighborhood.rs",
        "crates/aura-web/src/main.rs",
    ],
    JoinChannel => [
        "crates/aura-app/src/workflows/messaging.rs",
        "crates/aura-app/src/workflows/messaging/channel_refs.rs",
        "crates/aura-app/src/workflows/messaging/channels.rs",
        "crates/aura-terminal/src/tui/screens/chat/screen.rs",
        "crates/aura-ui/src/app/screens/chat.rs",
        "crates/aura-web/src/main.rs",
    ],
    SendChatMessage => [
        "crates/aura-app/src/workflows/messaging.rs",
        "crates/aura-app/src/workflows/messaging/send.rs",
        "crates/aura-terminal/src/tui/screens/chat/screen.rs",
        "crates/aura-ui/src/app/screens/chat.rs",
        "crates/aura-web/src/main.rs",
    ],
    AddDevice => [
        "crates/aura-app/src/workflows/settings.rs",
        "crates/aura-terminal/src/tui/screens/settings/screen.rs",
        "crates/aura-ui/src/app/screens/settings.rs",
        "crates/aura-web/src/main.rs",
    ],
    RemoveDevice => [
        "crates/aura-app/src/workflows/settings.rs",
        "crates/aura-terminal/src/tui/screens/settings/screen.rs",
        "crates/aura-ui/src/app/screens/settings.rs",
        "crates/aura-web/src/main.rs",
    ],
    SwitchAuthority => [
        "crates/aura-app/src/workflows/settings.rs",
        "crates/aura-terminal/src/tui/screens/settings/screen.rs",
        "crates/aura-ui/src/app/screens/settings.rs",
        "crates/aura-web/src/main.rs",
    ],
};

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

#[cfg(test)]
mod tests {
    use super::{
        shared_flow_scenarios, shared_flow_source_areas, shared_flow_support, FlowAvailability,
        SharedFlowId,
    };

    #[test]
    fn shared_flow_inventory_is_addressable_and_non_empty() {
        for flow in SharedFlowId::ALL {
            let support = shared_flow_support(flow);
            assert_eq!(support.flow, flow);
            assert_eq!(support.web, FlowAvailability::Supported);
            assert_eq!(support.tui, FlowAvailability::Supported);
            assert!(
                !shared_flow_scenarios(flow).is_empty(),
                "shared flow {flow:?} should declare at least one scenario",
            );
            assert!(
                !shared_flow_source_areas(flow).is_empty(),
                "shared flow {flow:?} should declare at least one source area",
            );
        }
    }
}
