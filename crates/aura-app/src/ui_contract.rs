//! Shared UI-facing semantic contract for Aura frontends and harnesses.
//!
//! This module defines stable application-facing UI identifiers and snapshot
//! types that can be shared across the web UI, TUI, and harness tooling.

#![allow(missing_docs)] // Shared contract surface - refined incrementally during migration.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenId {
    Neighborhood,
    Chat,
    Contacts,
    Notifications,
    Settings,
}

impl ScreenId {
    #[must_use]
    pub const fn help_label(self) -> &'static str {
        match self {
            Self::Neighborhood => "Neighborhood",
            Self::Chat => "Chat",
            Self::Contacts => "Contacts",
            Self::Notifications => "Notifications",
            Self::Settings => "Settings",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModalId {
    Help,
    CreateInvitation,
    AcceptInvitation,
    CreateHome,
    CreateChannel,
    SetChannelTopic,
    ChannelInfo,
    EditNickname,
    RemoveContact,
    GuardianSetup,
    RequestRecovery,
    AddDevice,
    ImportDeviceEnrollmentCode,
    SelectDeviceToRemove,
    ConfirmRemoveDevice,
    MfaSetup,
    AssignModerator,
    SwitchAuthority,
    AccessOverride,
    CapabilityConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldId {
    AccountName,
    InvitationCode,
    InvitationReceiver,
    ChatInput,
    HomeName,
    CreateChannelName,
    CreateChannelTopic,
    ThresholdInput,
    Nickname,
    DeviceName,
    DeviceImportCode,
    CapabilityFull,
    CapabilityPartial,
    CapabilityLimited,
}

impl FieldId {
    #[must_use]
    pub const fn web_dom_id(self) -> Option<&'static str> {
        match self {
            Self::AccountName => Some("aura-account-name-input"),
            Self::DeviceImportCode => Some("aura-account-import-code-input"),
            Self::InvitationCode => Some("aura-field-invitation-code"),
            Self::InvitationReceiver => Some("aura-field-invitation-receiver"),
            Self::ChatInput => Some("aura-field-chat-input"),
            Self::HomeName => Some("aura-field-home-name"),
            Self::CreateChannelName => Some("aura-field-create-channel-name"),
            Self::CreateChannelTopic => Some("aura-field-create-channel-topic"),
            Self::ThresholdInput => Some("aura-field-threshold-input"),
            Self::Nickname => Some("aura-field-nickname"),
            Self::DeviceName => Some("aura-field-device-name"),
            Self::CapabilityFull => Some("aura-field-capability-full"),
            Self::CapabilityPartial => Some("aura-field-capability-partial"),
            Self::CapabilityLimited => Some("aura-field-capability-limited"),
        }
    }

    #[must_use]
    pub fn web_selector(self) -> Option<String> {
        self.web_dom_id().map(|id| format!("#{id}"))
    }
}

fn sanitize_dom_segment(raw: &str) -> String {
    raw.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListId {
    Navigation,
    Channels,
    Contacts,
    Notifications,
    Homes,
    NeighborhoodMembers,
    Devices,
    Authorities,
    SettingsSections,
}

impl ListId {
    #[must_use]
    pub const fn dom_segment(self) -> &'static str {
        match self {
            Self::Navigation => "navigation",
            Self::Channels => "channels",
            Self::Contacts => "contacts",
            Self::Notifications => "notifications",
            Self::Homes => "homes",
            Self::NeighborhoodMembers => "neighborhood-members",
            Self::Devices => "devices",
            Self::Authorities => "authorities",
            Self::SettingsSections => "settings-sections",
        }
    }
}

#[must_use]
pub fn list_item_dom_id(list_id: ListId, item_id: &str) -> String {
    format!(
        "aura-list-{}-item-{}",
        list_id.dom_segment(),
        sanitize_dom_segment(item_id)
    )
}

#[must_use]
pub fn list_item_selector(list_id: ListId, item_id: &str) -> String {
    format!("#{}", list_item_dom_id(list_id, item_id))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlId {
    AppRoot,
    OnboardingRoot,
    ModalRegion,
    ToastRegion,
    OnboardingCreateAccountButton,
    OnboardingImportDeviceButton,
    NavRoot,
    NavNeighborhood,
    NavChat,
    NavContacts,
    NavNotifications,
    NavSettings,
    Screen(ScreenId),
    Field(FieldId),
    List(ListId),
    Modal(ModalId),
    ModalConfirmButton,
    ModalCancelButton,
    ContactsAcceptInvitationButton,
}

impl ControlId {
    #[must_use]
    pub const fn web_dom_id(self) -> Option<&'static str> {
        match self {
            Self::AppRoot => Some("aura-app-root"),
            Self::OnboardingRoot => Some("aura-onboarding-root"),
            Self::ModalRegion => Some("aura-modal-region"),
            Self::ToastRegion => Some("aura-toast-region"),
            Self::OnboardingCreateAccountButton => Some("aura-onboarding-create-account-button"),
            Self::OnboardingImportDeviceButton => Some("aura-onboarding-import-device-button"),
            Self::NavRoot => Some("aura-nav-root"),
            Self::NavNeighborhood => Some("aura-nav-neighborhood"),
            Self::NavChat => Some("aura-nav-chat"),
            Self::NavContacts => Some("aura-nav-contacts"),
            Self::NavNotifications => Some("aura-nav-notifications"),
            Self::NavSettings => Some("aura-nav-settings"),
            Self::ModalConfirmButton => Some("aura-modal-confirm-button"),
            Self::ModalCancelButton => Some("aura-modal-cancel-button"),
            Self::ContactsAcceptInvitationButton => Some("aura-contacts-accept-invitation"),
            Self::Screen(ScreenId::Neighborhood) => Some("aura-screen-neighborhood"),
            Self::Screen(ScreenId::Chat) => Some("aura-screen-chat"),
            Self::Screen(ScreenId::Contacts) => Some("aura-screen-contacts"),
            Self::Screen(ScreenId::Notifications) => Some("aura-screen-notifications"),
            Self::Screen(ScreenId::Settings) => Some("aura-screen-settings"),
            Self::Field(_) | Self::List(_) | Self::Modal(_) => None,
        }
    }

    #[must_use]
    pub fn web_selector(self) -> Option<String> {
        self.web_dom_id().map(|id| format!("#{id}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastKind {
    Success,
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToastId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiReadiness {
    Loading,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Idle,
    Submitting,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationState {
    PendingLocal,
    Confirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToastSnapshot {
    pub id: ToastId,
    pub kind: ToastKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListItemSnapshot {
    pub id: String,
    pub selected: bool,
    pub confirmation: ConfirmationState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListSnapshot {
    pub id: ListId,
    pub items: Vec<ListItemSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionSnapshot {
    pub list: ListId,
    pub item_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSnapshot {
    pub id: OperationId,
    pub state: OperationState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiSnapshot {
    pub screen: ScreenId,
    pub focused_control: Option<ControlId>,
    pub open_modal: Option<ModalId>,
    pub readiness: UiReadiness,
    pub selections: Vec<SelectionSnapshot>,
    pub lists: Vec<ListSnapshot>,
    pub operations: Vec<OperationSnapshot>,
    pub toasts: Vec<ToastSnapshot>,
}

impl UiSnapshot {
    #[must_use]
    pub fn loading(screen: ScreenId) -> Self {
        Self {
            screen,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Loading,
            selections: Vec::new(),
            lists: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        list_item_dom_id, list_item_selector, ConfirmationState, ControlId, FieldId, ListId,
        ScreenId, UiReadiness, UiSnapshot,
    };

    #[test]
    fn screen_ids_have_stable_help_labels() {
        assert_eq!(ScreenId::Neighborhood.help_label(), "Neighborhood");
        assert_eq!(ScreenId::Chat.help_label(), "Chat");
        assert_eq!(ScreenId::Contacts.help_label(), "Contacts");
        assert_eq!(ScreenId::Notifications.help_label(), "Notifications");
        assert_eq!(ScreenId::Settings.help_label(), "Settings");
    }

    #[test]
    fn loading_snapshot_starts_empty_and_loading() {
        let snapshot = UiSnapshot::loading(ScreenId::Settings);
        assert_eq!(snapshot.screen, ScreenId::Settings);
        assert_eq!(snapshot.readiness, UiReadiness::Loading);
        assert_eq!(snapshot.focused_control, None);
        assert_eq!(snapshot.open_modal, None);
        assert!(snapshot.selections.is_empty());
        assert!(snapshot.lists.is_empty());
        assert!(snapshot.operations.is_empty());
        assert!(snapshot.toasts.is_empty());
    }

    #[test]
    fn control_ids_are_hashable_and_equatable() {
        assert_eq!(
            ControlId::Screen(ScreenId::Chat),
            ControlId::Screen(ScreenId::Chat)
        );
        assert_ne!(
            ControlId::Screen(ScreenId::Chat),
            ControlId::Screen(ScreenId::Contacts)
        );
    }

    #[test]
    fn web_dom_ids_are_stable_for_shared_controls() {
        assert_eq!(
            ControlId::OnboardingCreateAccountButton.web_dom_id(),
            Some("aura-onboarding-create-account-button")
        );
        assert_eq!(ControlId::AppRoot.web_dom_id(), Some("aura-app-root"));
        assert_eq!(ControlId::ModalRegion.web_dom_id(), Some("aura-modal-region"));
        assert_eq!(ControlId::ToastRegion.web_dom_id(), Some("aura-toast-region"));
        assert_eq!(ControlId::NavContacts.web_dom_id(), Some("aura-nav-contacts"));
        assert_eq!(
            ControlId::Screen(ScreenId::Settings).web_dom_id(),
            Some("aura-screen-settings")
        );
        assert_eq!(
            ControlId::ModalConfirmButton.web_selector().as_deref(),
            Some("#aura-modal-confirm-button")
        );
    }

    #[test]
    fn web_dom_ids_are_stable_for_shared_fields() {
        assert_eq!(FieldId::AccountName.web_dom_id(), Some("aura-account-name-input"));
        assert_eq!(
            FieldId::InvitationCode.web_selector().as_deref(),
            Some("#aura-field-invitation-code")
        );
    }

    #[test]
    fn list_item_dom_ids_are_stable_and_sanitized() {
        assert_eq!(
            list_item_dom_id(ListId::Contacts, "authority:abc/DEF"),
            "aura-list-contacts-item-authority-abc-def"
        );
        assert_eq!(
            list_item_selector(ListId::SettingsSections, "devices"),
            "#aura-list-settings-sections-item-devices"
        );
    }
}
