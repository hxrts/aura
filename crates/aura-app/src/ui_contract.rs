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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlId {
    AppRoot,
    OnboardingRoot,
    NavRoot,
    Screen(ScreenId),
    Field(FieldId),
    List(ListId),
    Modal(ModalId),
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
    use super::{ControlId, ScreenId, UiReadiness, UiSnapshot};

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
}
