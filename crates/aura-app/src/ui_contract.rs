//! Shared UI-facing semantic contract for Aura frontends and harnesses.
//!
//! This module defines stable application-facing UI identifiers and snapshot
//! types that can be shared across the web UI, TUI, and harness tooling.

#![allow(missing_docs)] // Shared contract surface - refined incrementally during migration.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenId {
    Onboarding,
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
            Self::Onboarding => "Onboarding",
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
    InvitationCode,
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

impl ModalId {
    #[must_use]
    pub const fn web_dom_id(self) -> &'static str {
        match self {
            Self::Help => "aura-modal-help",
            Self::CreateInvitation => "aura-modal-create-invitation",
            Self::InvitationCode => "aura-modal-invitation-code",
            Self::AcceptInvitation => "aura-modal-accept-invitation",
            Self::CreateHome => "aura-modal-create-home",
            Self::CreateChannel => "aura-modal-create-channel",
            Self::SetChannelTopic => "aura-modal-set-channel-topic",
            Self::ChannelInfo => "aura-modal-channel-info",
            Self::EditNickname => "aura-modal-edit-nickname",
            Self::RemoveContact => "aura-modal-remove-contact",
            Self::GuardianSetup => "aura-modal-guardian-setup",
            Self::RequestRecovery => "aura-modal-request-recovery",
            Self::AddDevice => "aura-modal-add-device",
            Self::ImportDeviceEnrollmentCode => "aura-modal-import-device-enrollment-code",
            Self::SelectDeviceToRemove => "aura-modal-select-device-to-remove",
            Self::ConfirmRemoveDevice => "aura-modal-confirm-remove-device",
            Self::MfaSetup => "aura-modal-mfa-setup",
            Self::AssignModerator => "aura-modal-assign-moderator",
            Self::SwitchAuthority => "aura-modal-switch-authority",
            Self::AccessOverride => "aura-modal-access-override",
            Self::CapabilityConfig => "aura-modal-capability-config",
        }
    }

    #[must_use]
    pub fn web_selector(self) -> String {
        format!("#{}", self.web_dom_id())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldId {
    AccountName,
    InvitationCode,
    InvitationReceiver,
    InvitationType,
    InvitationMessage,
    InvitationTtl,
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
            Self::InvitationType => Some("aura-field-invitation-type"),
            Self::InvitationMessage => Some("aura-field-invitation-message"),
            Self::InvitationTtl => Some("aura-field-invitation-ttl"),
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
    InvitationTypes,
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
            Self::InvitationTypes => "invitation-types",
            Self::Homes => "homes",
            Self::NeighborhoodMembers => "neighborhood-members",
            Self::Devices => "devices",
            Self::Authorities => "authorities",
            Self::SettingsSections => "settings-sections",
        }
    }

    #[must_use]
    pub const fn web_dom_id(self) -> &'static str {
        match self {
            Self::Navigation => "aura-list-navigation",
            Self::Channels => "aura-list-channels",
            Self::Contacts => "aura-list-contacts",
            Self::Notifications => "aura-list-notifications",
            Self::InvitationTypes => "aura-list-invitation-types",
            Self::Homes => "aura-list-homes",
            Self::NeighborhoodMembers => "aura-list-neighborhood-members",
            Self::Devices => "aura-list-devices",
            Self::Authorities => "aura-list-authorities",
            Self::SettingsSections => "aura-list-settings-sections",
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
    ModalCopyButton,
    NavRoot,
    NavNeighborhood,
    NavChat,
    NavContacts,
    NavNotifications,
    NavSettings,
    NeighborhoodNewHomeButton,
    NeighborhoodAcceptInvitationButton,
    ContactsCreateInvitationButton,
    ContactsInviteToChannelButton,
    ContactsEditNicknameButton,
    ContactsRemoveContactButton,
    NeighborhoodEnterAsButton,
    ChatNewGroupButton,
    ContactsStartChatButton,
    SettingsEditNicknameButton,
    SettingsConfigureThresholdButton,
    SettingsRequestRecoveryButton,
    SettingsImportDeviceCodeButton,
    SettingsSwitchAuthorityButton,
    SettingsConfigureMfaButton,
    SettingsToggleThemeButton,
    DeviceEnrollmentCancelButton,
    DeviceEnrollmentPrimaryButton,
    AuthorityPickerCancelButton,
    AuthorityPickerConfirmButton,
    Screen(ScreenId),
    Field(FieldId),
    List(ListId),
    Modal(ModalId),
    ModalConfirmButton,
    ModalCancelButton,
    ContactsAcceptInvitationButton,
    ModalInput,
    SettingsAddDeviceButton,
    SettingsRemoveDeviceButton,
    ChatSendMessageButton,
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
            Self::ModalCopyButton => Some("aura-modal-copy-button"),
            Self::NavRoot => Some("aura-nav-root"),
            Self::NavNeighborhood => Some("aura-nav-neighborhood"),
            Self::NavChat => Some("aura-nav-chat"),
            Self::NavContacts => Some("aura-nav-contacts"),
            Self::NavNotifications => Some("aura-nav-notifications"),
            Self::NavSettings => Some("aura-nav-settings"),
            Self::NeighborhoodNewHomeButton => Some("aura-neighborhood-new-home"),
            Self::NeighborhoodAcceptInvitationButton => Some("aura-neighborhood-accept-invitation"),
            Self::ContactsCreateInvitationButton => Some("aura-contacts-create-invitation"),
            Self::ContactsInviteToChannelButton => Some("aura-contacts-invite-channel"),
            Self::ContactsEditNicknameButton => Some("aura-contacts-edit-nickname"),
            Self::ContactsRemoveContactButton => Some("aura-contacts-remove-contact"),
            Self::NeighborhoodEnterAsButton => Some("aura-neighborhood-enter-as"),
            Self::ChatNewGroupButton => Some("aura-chat-new-group"),
            Self::ContactsStartChatButton => Some("aura-contacts-start-chat"),
            Self::SettingsEditNicknameButton => Some("aura-settings-edit-nickname"),
            Self::SettingsConfigureThresholdButton => Some("aura-settings-configure-threshold"),
            Self::SettingsRequestRecoveryButton => Some("aura-settings-request-recovery"),
            Self::SettingsImportDeviceCodeButton => Some("aura-settings-import-device-code"),
            Self::SettingsSwitchAuthorityButton => Some("aura-settings-switch-authority"),
            Self::SettingsConfigureMfaButton => Some("aura-settings-configure-mfa"),
            Self::SettingsToggleThemeButton => Some("aura-settings-toggle-theme"),
            Self::DeviceEnrollmentCancelButton => Some("aura-device-enrollment-cancel-button"),
            Self::DeviceEnrollmentPrimaryButton => Some("aura-device-enrollment-primary-button"),
            Self::AuthorityPickerCancelButton => Some("aura-authority-picker-cancel-button"),
            Self::AuthorityPickerConfirmButton => Some("aura-authority-picker-confirm-button"),
            Self::ModalConfirmButton => Some("aura-modal-confirm-button"),
            Self::ModalCancelButton => Some("aura-modal-cancel-button"),
            Self::ModalInput => Some("aura-modal-input"),
            Self::ContactsAcceptInvitationButton => Some("aura-contacts-accept-invitation"),
            Self::SettingsAddDeviceButton => Some("aura-settings-add-device"),
            Self::SettingsRemoveDeviceButton => Some("aura-settings-remove-device"),
            Self::ChatSendMessageButton => Some("aura-chat-send-message"),
            Self::Screen(ScreenId::Onboarding) => Some("aura-screen-onboarding"),
            Self::Screen(ScreenId::Neighborhood) => Some("aura-screen-neighborhood"),
            Self::Screen(ScreenId::Chat) => Some("aura-screen-chat"),
            Self::Screen(ScreenId::Contacts) => Some("aura-screen-contacts"),
            Self::Screen(ScreenId::Notifications) => Some("aura-screen-notifications"),
            Self::Screen(ScreenId::Settings) => Some("aura-screen-settings"),
            Self::Field(field_id) => field_id.web_dom_id(),
            Self::List(list_id) => Some(list_id.web_dom_id()),
            Self::Modal(modal_id) => Some(modal_id.web_dom_id()),
        }
    }

    #[must_use]
    pub fn web_selector(self) -> Option<String> {
        self.web_dom_id().map(|id| format!("#{id}"))
    }

    #[must_use]
    pub const fn activation_key(self) -> Option<&'static str> {
        match self {
            Self::NavNeighborhood => Some("1"),
            Self::NavChat => Some("2"),
            Self::NavContacts => Some("3"),
            Self::NavNotifications => Some("4"),
            Self::NavSettings => Some("5"),
            Self::OnboardingCreateAccountButton => Some("\r"),
            Self::OnboardingImportDeviceButton => Some("\r"),
            Self::ModalCopyButton => Some("c"),
            Self::NeighborhoodNewHomeButton => Some("n"),
            Self::NeighborhoodAcceptInvitationButton => Some("a"),
            Self::ContactsCreateInvitationButton => Some("n"),
            Self::ContactsInviteToChannelButton => Some("i"),
            Self::NeighborhoodEnterAsButton => Some("d"),
            Self::ChatNewGroupButton => Some("n"),
            Self::ContactsStartChatButton => Some("c"),
            Self::SettingsEditNicknameButton => Some("e"),
            Self::SettingsConfigureThresholdButton => Some("t"),
            Self::SettingsRequestRecoveryButton => Some("s"),
            Self::SettingsImportDeviceCodeButton => Some("i"),
            Self::SettingsSwitchAuthorityButton => Some("s"),
            Self::SettingsConfigureMfaButton => Some("m"),
            Self::ContactsEditNicknameButton => Some("e"),
            Self::ContactsRemoveContactButton => Some("r"),
            Self::DeviceEnrollmentCancelButton => Some("\x1b"),
            Self::DeviceEnrollmentPrimaryButton => None,
            Self::AuthorityPickerCancelButton => Some("\x1b"),
            Self::AuthorityPickerConfirmButton => Some("\r"),
            Self::ContactsAcceptInvitationButton => Some("a"),
            Self::ModalConfirmButton => Some("\t\r"),
            Self::ModalCancelButton => Some("\x1b"),
            Self::SettingsAddDeviceButton => Some("a"),
            Self::SettingsRemoveDeviceButton => Some("r"),
            _ => None,
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationInstanceId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeEventId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelFactKey {
    pub id: Option<String>,
    pub name: Option<String>,
}

impl ChannelFactKey {
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            id: None,
            name: Some(name.into()),
        }
    }

    #[must_use]
    pub fn identified(id: impl Into<String>) -> Self {
        Self {
            id: Some(id.into()),
            name: None,
        }
    }

    #[must_use]
    pub fn matches_needle(&self, needle: &str) -> bool {
        self.id.as_deref().is_some_and(|id| id.contains(needle))
            || self
                .name
                .as_deref()
                .is_some_and(|name| name.contains(needle))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvitationFactKind {
    Generic,
    Contact,
}

impl OperationId {
    #[must_use]
    pub fn create_home() -> Self {
        Self("create_home".to_string())
    }

    #[must_use]
    pub fn invitation_create() -> Self {
        Self("invitation_create".to_string())
    }

    #[must_use]
    pub fn invitation_accept() -> Self {
        Self("invitation_accept".to_string())
    }

    #[must_use]
    pub fn device_enrollment() -> Self {
        Self("device_enrollment".to_string())
    }
}

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
    pub instance_id: OperationInstanceId,
    pub state: OperationState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeEventKind {
    InvitationAccepted,
    InvitationCodeReady,
    PendingHomeInvitationReady,
    DeviceEnrollmentCodeReady,
    ContactLinkReady,
    HomeCreated,
    HomeEntered,
    ChannelJoined,
    ChannelMembershipReady,
    RecipientPeersResolved,
    MessageCommitted,
    MessageDeliveryReady,
    RemoteFactsPulled,
    ChatSignalUpdated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeFact {
    InvitationAccepted {
        invitation_kind: InvitationFactKind,
        authority_id: Option<String>,
        operation_state: Option<OperationState>,
    },
    InvitationCodeReady {
        receiver_authority_id: Option<String>,
        source_operation: OperationId,
    },
    PendingHomeInvitationReady,
    DeviceEnrollmentCodeReady {
        device_name: Option<String>,
        code_len: Option<usize>,
    },
    ContactLinkReady {
        authority_id: Option<String>,
        contact_count: Option<usize>,
    },
    HomeCreated {
        name: String,
    },
    HomeEntered {
        name: String,
        access_depth: Option<String>,
    },
    ChannelJoined {
        channel: Option<ChannelFactKey>,
        source: Option<String>,
    },
    ChannelMembershipReady {
        channel: ChannelFactKey,
        member_count: Option<usize>,
    },
    RecipientPeersResolved {
        channel: ChannelFactKey,
        member_count: usize,
    },
    MessageCommitted {
        channel: ChannelFactKey,
        content: String,
    },
    MessageDeliveryReady {
        channel: ChannelFactKey,
        member_count: usize,
    },
    RemoteFactsPulled {
        contact_count: usize,
        lan_peer_count: usize,
    },
    ChatSignalUpdated {
        active_channel: String,
        channel_count: usize,
        message_count: usize,
    },
}

impl RuntimeFact {
    #[must_use]
    pub fn kind(&self) -> RuntimeEventKind {
        match self {
            Self::InvitationAccepted { .. } => RuntimeEventKind::InvitationAccepted,
            Self::InvitationCodeReady { .. } => RuntimeEventKind::InvitationCodeReady,
            Self::PendingHomeInvitationReady => RuntimeEventKind::PendingHomeInvitationReady,
            Self::DeviceEnrollmentCodeReady { .. } => RuntimeEventKind::DeviceEnrollmentCodeReady,
            Self::ContactLinkReady { .. } => RuntimeEventKind::ContactLinkReady,
            Self::HomeCreated { .. } => RuntimeEventKind::HomeCreated,
            Self::HomeEntered { .. } => RuntimeEventKind::HomeEntered,
            Self::ChannelJoined { .. } => RuntimeEventKind::ChannelJoined,
            Self::ChannelMembershipReady { .. } => RuntimeEventKind::ChannelMembershipReady,
            Self::RecipientPeersResolved { .. } => RuntimeEventKind::RecipientPeersResolved,
            Self::MessageCommitted { .. } => RuntimeEventKind::MessageCommitted,
            Self::MessageDeliveryReady { .. } => RuntimeEventKind::MessageDeliveryReady,
            Self::RemoteFactsPulled { .. } => RuntimeEventKind::RemoteFactsPulled,
            Self::ChatSignalUpdated { .. } => RuntimeEventKind::ChatSignalUpdated,
        }
    }

    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::InvitationAccepted {
                invitation_kind,
                authority_id,
                ..
            } => format!(
                "invitation_accepted:{invitation_kind:?}:{}",
                authority_id.as_deref().unwrap_or("*")
            ),
            Self::InvitationCodeReady {
                receiver_authority_id,
                source_operation,
            } => format!(
                "invitation_code_ready:{}:{}",
                source_operation.0,
                receiver_authority_id.as_deref().unwrap_or("*")
            ),
            Self::PendingHomeInvitationReady => "pending_home_invitation_ready".to_string(),
            Self::DeviceEnrollmentCodeReady { device_name, .. } => format!(
                "device_enrollment_code_ready:{}",
                device_name.as_deref().unwrap_or("*")
            ),
            Self::ContactLinkReady {
                authority_id,
                contact_count,
            } => format!(
                "contact_link_ready:{}:{}",
                authority_id.as_deref().unwrap_or("*"),
                contact_count
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "*".to_string())
            ),
            Self::HomeCreated { name } => format!("home_created:{name}"),
            Self::HomeEntered { name, .. } => format!("home_entered:{name}"),
            Self::ChannelJoined { channel, source } => format!(
                "channel_joined:{}:{}",
                channel
                    .as_ref()
                    .and_then(|channel| channel.name.clone().or(channel.id.clone()))
                    .unwrap_or_else(|| "*".to_string()),
                source.as_deref().unwrap_or("*")
            ),
            Self::ChannelMembershipReady { channel, .. } => format!(
                "channel_membership_ready:{}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
            Self::RecipientPeersResolved { channel, .. } => format!(
                "recipient_peers_resolved:{}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
            Self::MessageCommitted { channel, content } => format!(
                "message_committed:{}:{content}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
            Self::MessageDeliveryReady { channel, .. } => format!(
                "message_delivery_ready:{}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
            Self::RemoteFactsPulled {
                contact_count,
                lan_peer_count,
            } => format!("remote_facts_pulled:{contact_count}:{lan_peer_count}"),
            Self::ChatSignalUpdated { active_channel, .. } => {
                format!("chat_signal_updated:{active_channel}")
            }
        }
    }

    #[must_use]
    pub fn matches_needle(&self, needle: &str) -> bool {
        match self {
            Self::InvitationAccepted {
                authority_id,
                operation_state,
                ..
            } => {
                authority_id
                    .as_deref()
                    .is_some_and(|value| value.contains(needle))
                    || operation_state.is_some_and(|state| format!("{state:?}").contains(needle))
            }
            Self::InvitationCodeReady {
                receiver_authority_id,
                source_operation,
            } => {
                receiver_authority_id
                    .as_deref()
                    .is_some_and(|value| value.contains(needle))
                    || source_operation.0.contains(needle)
            }
            Self::PendingHomeInvitationReady => needle.contains("pending_home_invitation"),
            Self::DeviceEnrollmentCodeReady {
                device_name,
                code_len,
            } => {
                device_name
                    .as_deref()
                    .is_some_and(|value| value.contains(needle))
                    || code_len.is_some_and(|value| value.to_string().contains(needle))
            }
            Self::ContactLinkReady {
                authority_id,
                contact_count,
            } => {
                authority_id
                    .as_deref()
                    .is_some_and(|value| value.contains(needle))
                    || contact_count.is_some_and(|value| value.to_string().contains(needle))
            }
            Self::HomeCreated { name } | Self::HomeEntered { name, .. } => name.contains(needle),
            Self::ChannelJoined { channel, source } => {
                channel
                    .as_ref()
                    .is_some_and(|channel| channel.matches_needle(needle))
                    || source
                        .as_deref()
                        .is_some_and(|value| value.contains(needle))
            }
            Self::ChannelMembershipReady {
                channel,
                member_count,
            } => {
                channel.matches_needle(needle)
                    || member_count.is_some_and(|value| value.to_string().contains(needle))
            }
            Self::RecipientPeersResolved {
                channel,
                member_count,
            }
            | Self::MessageDeliveryReady {
                channel,
                member_count,
            } => channel.matches_needle(needle) || member_count.to_string().contains(needle),
            Self::MessageCommitted { channel, content } => {
                channel.matches_needle(needle) || content.contains(needle)
            }
            Self::RemoteFactsPulled {
                contact_count,
                lan_peer_count,
            } => {
                contact_count.to_string().contains(needle)
                    || lan_peer_count.to_string().contains(needle)
            }
            Self::ChatSignalUpdated {
                active_channel,
                channel_count,
                message_count,
            } => {
                active_channel.contains(needle)
                    || channel_count.to_string().contains(needle)
                    || message_count.to_string().contains(needle)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeEventSnapshot {
    pub id: RuntimeEventId,
    pub fact: RuntimeFact,
}

impl RuntimeEventSnapshot {
    #[must_use]
    pub fn kind(&self) -> RuntimeEventKind {
        self.fact.kind()
    }

    #[must_use]
    pub fn matches_needle(&self, needle: &str) -> bool {
        self.fact.matches_needle(needle)
    }

    #[must_use]
    pub fn key(&self) -> String {
        self.fact.key()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSnapshot {
    pub id: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderHeartbeat {
    pub screen: ScreenId,
    pub open_modal: Option<ModalId>,
    pub render_seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendId {
    Web,
    Tui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParityException {
    BrowserThemeControl,
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
    ThemeAppearance,
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
    SharedFlowId::ThemeAppearance,
];

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

pub const SHARED_SCREEN_SUPPORT: &[SharedScreenSupport] = &[
    SharedScreenSupport {
        screen: ScreenId::Onboarding,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedScreenSupport {
        screen: ScreenId::Neighborhood,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedScreenSupport {
        screen: ScreenId::Chat,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedScreenSupport {
        screen: ScreenId::Contacts,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedScreenSupport {
        screen: ScreenId::Notifications,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedScreenSupport {
        screen: ScreenId::Settings,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
];

pub const SHARED_MODAL_SUPPORT: &[SharedModalSupport] = &[
    SharedModalSupport {
        modal: ModalId::Help,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::CreateInvitation,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::AcceptInvitation,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::CreateHome,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::CreateChannel,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::SetChannelTopic,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::ChannelInfo,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::EditNickname,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::GuardianSetup,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::RequestRecovery,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::AddDevice,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::ImportDeviceEnrollmentCode,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::SelectDeviceToRemove,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::ConfirmRemoveDevice,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::MfaSetup,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::AssignModerator,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::SwitchAuthority,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::AccessOverride,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedModalSupport {
        modal: ModalId::CapabilityConfig,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
];

pub const SHARED_LIST_SUPPORT: &[SharedListSupport] = &[
    SharedListSupport {
        list: ListId::Navigation,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedListSupport {
        list: ListId::Contacts,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedListSupport {
        list: ListId::Channels,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedListSupport {
        list: ListId::Notifications,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedListSupport {
        list: ListId::SettingsSections,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedListSupport {
        list: ListId::Homes,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedListSupport {
        list: ListId::NeighborhoodMembers,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
];

pub const SHARED_SCREEN_MODULE_MAP: &[SharedScreenModuleMap] = &[
    SharedScreenModuleMap {
        screen: ScreenId::Onboarding,
        web_symbol: "OnboardingScreen",
        web_path: "crates/aura-ui/src/app.rs",
        tui_symbol: "AccountSetupModal",
        tui_path: "crates/aura-terminal/src/tui/components/account_setup_modal_template.rs",
    },
    SharedScreenModuleMap {
        screen: ScreenId::Neighborhood,
        web_symbol: "NeighborhoodScreen",
        web_path: "crates/aura-ui/src/app.rs",
        tui_symbol: "NeighborhoodScreen",
        tui_path: "crates/aura-terminal/src/tui/screens/neighborhood/screen.rs",
    },
    SharedScreenModuleMap {
        screen: ScreenId::Chat,
        web_symbol: "ChatScreen",
        web_path: "crates/aura-ui/src/app.rs",
        tui_symbol: "ChatScreen",
        tui_path: "crates/aura-terminal/src/tui/screens/chat/screen.rs",
    },
    SharedScreenModuleMap {
        screen: ScreenId::Contacts,
        web_symbol: "ContactsScreen",
        web_path: "crates/aura-ui/src/app.rs",
        tui_symbol: "ContactsScreen",
        tui_path: "crates/aura-terminal/src/tui/screens/contacts/screen.rs",
    },
    SharedScreenModuleMap {
        screen: ScreenId::Notifications,
        web_symbol: "NotificationsScreen",
        web_path: "crates/aura-ui/src/app.rs",
        tui_symbol: "NotificationsScreen",
        tui_path: "crates/aura-terminal/src/tui/screens/notifications/screen.rs",
    },
    SharedScreenModuleMap {
        screen: ScreenId::Settings,
        web_symbol: "SettingsScreen",
        web_path: "crates/aura-ui/src/app.rs",
        tui_symbol: "SettingsScreen",
        tui_path: "crates/aura-terminal/src/tui/screens/settings/screen.rs",
    },
];

pub const SHARED_FLOW_SUPPORT: &[SharedFlowSupport] = &[
    SharedFlowSupport {
        flow: SharedFlowId::NavigateNeighborhood,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::NavigateChat,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::NavigateContacts,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::NavigateNotifications,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::NavigateSettings,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::CreateInvitation,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::AcceptInvitation,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::CreateHome,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::JoinChannel,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::SendChatMessage,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::AddDevice,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::RemoveDevice,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::SwitchAuthority,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Supported,
    },
    SharedFlowSupport {
        flow: SharedFlowId::ThemeAppearance,
        web: FlowAvailability::Supported,
        tui: FlowAvailability::Exception(ParityException::BrowserThemeControl),
    },
];

pub const SHARED_FLOW_SCENARIO_COVERAGE: &[SharedFlowScenarioCoverage] = &[
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateNeighborhood,
        scenario_id: "real-runtime-mixed-startup-smoke",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateChat,
        scenario_id: "scenario13-mixed-contact-channel-message-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateContacts,
        scenario_id: "scenario13-mixed-contact-channel-message-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateNotifications,
        scenario_id: "scenario10-recovery-and-notifications-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::NavigateSettings,
        scenario_id: "shared-settings-parity",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::CreateInvitation,
        scenario_id: "scenario13-mixed-contact-channel-message-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::AcceptInvitation,
        scenario_id: "scenario13-mixed-contact-channel-message-e2e",
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
        scenario_id: "scenario8-settings-devices-authority-e2e",
    },
    SharedFlowScenarioCoverage {
        flow: SharedFlowId::ThemeAppearance,
        scenario_id: "shared-settings-parity",
    },
];

#[must_use]
pub fn shared_flow_support(flow: SharedFlowId) -> &'static SharedFlowSupport {
    SHARED_FLOW_SUPPORT
        .iter()
        .find(|support| support.flow == flow)
        .expect("shared flow support must be declared")
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
pub fn shared_screen_support(screen: ScreenId) -> &'static SharedScreenSupport {
    SHARED_SCREEN_SUPPORT
        .iter()
        .find(|support| support.screen == screen)
        .expect("shared screen support must be declared")
}

#[must_use]
pub fn shared_modal_support(modal: ModalId) -> &'static SharedModalSupport {
    SHARED_MODAL_SUPPORT
        .iter()
        .find(|support| support.modal == modal)
        .expect("shared modal support must be declared")
}

#[must_use]
pub fn shared_list_support(list: ListId) -> &'static SharedListSupport {
    SHARED_LIST_SUPPORT
        .iter()
        .find(|support| support.list == list)
        .expect("shared list support must be declared")
}

#[must_use]
pub fn shared_screen_module_map(screen: ScreenId) -> &'static SharedScreenModuleMap {
    SHARED_SCREEN_MODULE_MAP
        .iter()
        .find(|mapping| mapping.screen == screen)
        .expect("shared screen module mapping must be declared")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiSnapshot {
    pub screen: ScreenId,
    pub focused_control: Option<ControlId>,
    pub open_modal: Option<ModalId>,
    pub readiness: UiReadiness,
    pub selections: Vec<SelectionSnapshot>,
    pub lists: Vec<ListSnapshot>,
    pub messages: Vec<MessageSnapshot>,
    pub operations: Vec<OperationSnapshot>,
    pub toasts: Vec<ToastSnapshot>,
    pub runtime_events: Vec<RuntimeEventSnapshot>,
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
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        }
    }

    #[must_use]
    pub fn message_contains(&self, needle: &str) -> bool {
        self.messages
            .iter()
            .any(|message| message.content.contains(needle))
    }

    #[must_use]
    pub fn has_runtime_event(&self, kind: RuntimeEventKind, detail_needle: Option<&str>) -> bool {
        self.runtime_events.iter().any(|event| {
            event.kind() == kind
                && detail_needle
                    .map(|needle| event.matches_needle(needle))
                    .unwrap_or(true)
        })
    }

    #[must_use]
    pub fn operation_state(&self, operation_id: &OperationId) -> Option<OperationState> {
        self.operations
            .iter()
            .find(|candidate| &candidate.id == operation_id)
            .map(|operation| operation.state)
    }

    #[must_use]
    pub fn operation_state_for_instance(
        &self,
        operation_id: &OperationId,
        instance_id: &OperationInstanceId,
    ) -> Option<OperationState> {
        self.operations
            .iter()
            .find(|candidate| {
                &candidate.id == operation_id && &candidate.instance_id == instance_id
            })
            .map(|operation| operation.state)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiParityMismatch {
    pub field: &'static str,
    pub web: String,
    pub tui: String,
}

fn normalize_parity_item_id(list_id: ListId, item_id: &str) -> Option<String> {
    match list_id {
        ListId::SettingsSections => match item_id.replace('_', "-").as_str() {
            "appearance" | "observability" => None,
            normalized => Some(normalized.to_string()),
        },
        _ => Some(item_id.to_string()),
    }
}

fn parity_relevant_lists(screen: ScreenId) -> &'static [ListId] {
    match screen {
        ScreenId::Onboarding => &[],
        ScreenId::Neighborhood => &[
            ListId::Navigation,
            ListId::Homes,
            ListId::NeighborhoodMembers,
        ],
        ScreenId::Chat => &[ListId::Navigation, ListId::Channels],
        ScreenId::Contacts => &[ListId::Navigation, ListId::Contacts],
        ScreenId::Notifications => &[ListId::Navigation, ListId::Notifications],
        ScreenId::Settings => &[ListId::Navigation, ListId::SettingsSections],
    }
}

fn parity_list_signature(
    snapshot: &UiSnapshot,
) -> Vec<(ListId, Vec<(String, bool, ConfirmationState)>)> {
    let relevant_lists = parity_relevant_lists(snapshot.screen);
    let mut lists = snapshot
        .lists
        .iter()
        .filter(|list| relevant_lists.contains(&list.id))
        .map(|list| {
            let mut items = list
                .items
                .iter()
                .filter_map(|item| {
                    normalize_parity_item_id(list.id, &item.id)
                        .map(|normalized| (normalized, item.selected, item.confirmation))
                })
                .collect::<Vec<_>>();
            items.sort_by(|left, right| {
                left.0
                    .cmp(&right.0)
                    .then_with(|| left.1.cmp(&right.1))
                    .then_with(|| format!("{:?}", left.2).cmp(&format!("{:?}", right.2)))
            });
            (list.id, items)
        })
        .collect::<Vec<_>>();
    lists.sort_by_key(|(list_id, _)| list_id.dom_segment());
    lists
}

fn parity_selection_signature(snapshot: &UiSnapshot) -> Vec<(ListId, String)> {
    let relevant_lists = parity_relevant_lists(snapshot.screen);
    let mut selections = snapshot
        .selections
        .iter()
        .filter(|selection| relevant_lists.contains(&selection.list))
        .filter_map(|selection| {
            normalize_parity_item_id(selection.list, &selection.item_id)
                .map(|normalized| (selection.list, normalized))
        })
        .collect::<Vec<_>>();
    selections.sort_by(|left, right| {
        left.0
            .dom_segment()
            .cmp(right.0.dom_segment())
            .then_with(|| left.1.cmp(&right.1))
    });
    selections
}

fn parity_operation_signature(snapshot: &UiSnapshot) -> Vec<(String, OperationState)> {
    let mut operations = snapshot
        .operations
        .iter()
        .map(|operation| (operation.id.0.clone(), operation.state))
        .collect::<Vec<_>>();
    operations.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| format!("{:?}", left.1).cmp(&format!("{:?}", right.1)))
    });
    operations
}

fn parity_message_signature(snapshot: &UiSnapshot) -> Vec<String> {
    let mut messages = snapshot
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect::<Vec<_>>();
    messages.sort();
    messages
}

#[must_use]
pub fn compare_ui_snapshots_for_parity(
    web: &UiSnapshot,
    tui: &UiSnapshot,
) -> Vec<UiParityMismatch> {
    let mut mismatches = Vec::new();

    if web.screen != tui.screen {
        mismatches.push(UiParityMismatch {
            field: "screen",
            web: format!("{:?}", web.screen),
            tui: format!("{:?}", tui.screen),
        });
    }
    if web.readiness != tui.readiness {
        mismatches.push(UiParityMismatch {
            field: "readiness",
            web: format!("{:?}", web.readiness),
            tui: format!("{:?}", tui.readiness),
        });
    }
    if web.open_modal != tui.open_modal {
        mismatches.push(UiParityMismatch {
            field: "open_modal",
            web: format!("{:?}", web.open_modal),
            tui: format!("{:?}", tui.open_modal),
        });
    }

    let web_selections = parity_selection_signature(web);
    let tui_selections = parity_selection_signature(tui);
    if web_selections != tui_selections {
        mismatches.push(UiParityMismatch {
            field: "selections",
            web: format!("{web_selections:?}"),
            tui: format!("{tui_selections:?}"),
        });
    }

    let web_lists = parity_list_signature(web);
    let tui_lists = parity_list_signature(tui);
    if web_lists != tui_lists {
        mismatches.push(UiParityMismatch {
            field: "lists",
            web: format!("{web_lists:?}"),
            tui: format!("{tui_lists:?}"),
        });
    }

    let web_operations = parity_operation_signature(web);
    let tui_operations = parity_operation_signature(tui);
    if web_operations != tui_operations {
        mismatches.push(UiParityMismatch {
            field: "operations",
            web: format!("{web_operations:?}"),
            tui: format!("{tui_operations:?}"),
        });
    }

    let web_messages = parity_message_signature(web);
    let tui_messages = parity_message_signature(tui);
    if web_messages != tui_messages {
        mismatches.push(UiParityMismatch {
            field: "messages",
            web: format!("{web_messages:?}"),
            tui: format!("{tui_messages:?}"),
        });
    }

    mismatches
}

#[cfg(test)]
mod tests {
    use super::{
        compare_ui_snapshots_for_parity, list_item_dom_id, list_item_selector,
        shared_flow_scenarios, shared_flow_support, shared_list_support, shared_modal_support,
        shared_screen_module_map, shared_screen_support, ConfirmationState, ControlId, FieldId,
        FlowAvailability, ListId, ListItemSnapshot, ListSnapshot, MessageSnapshot, ModalId,
        OperationId, OperationInstanceId, OperationSnapshot, OperationState, ParityException,
        RenderHeartbeat, ScreenId, SelectionSnapshot, SharedFlowId, UiParityMismatch, UiReadiness,
        UiSnapshot, ALL_SHARED_FLOW_IDS, SHARED_FLOW_SCENARIO_COVERAGE, SHARED_FLOW_SUPPORT,
        SHARED_LIST_SUPPORT, SHARED_MODAL_SUPPORT, SHARED_SCREEN_MODULE_MAP, SHARED_SCREEN_SUPPORT,
    };
    use std::collections::HashSet;
    use std::path::Path;

    #[test]
    fn screen_ids_have_stable_help_labels() {
        assert_eq!(ScreenId::Onboarding.help_label(), "Onboarding");
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
        assert!(snapshot.messages.is_empty());
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
        assert_eq!(
            ControlId::ModalRegion.web_dom_id(),
            Some("aura-modal-region")
        );
        assert_eq!(
            ControlId::ToastRegion.web_dom_id(),
            Some("aura-toast-region")
        );
        assert_eq!(
            ControlId::NavContacts.web_dom_id(),
            Some("aura-nav-contacts")
        );
        assert_eq!(
            ControlId::Screen(ScreenId::Settings).web_dom_id(),
            Some("aura-screen-settings")
        );
        assert_eq!(
            ControlId::ModalConfirmButton.web_selector().as_deref(),
            Some("#aura-modal-confirm-button")
        );
        assert_eq!(
            ControlId::NeighborhoodEnterAsButton.web_dom_id(),
            Some("aura-neighborhood-enter-as")
        );
        assert_eq!(
            ControlId::ChatNewGroupButton.web_dom_id(),
            Some("aura-chat-new-group")
        );
        assert_eq!(
            ControlId::ContactsStartChatButton.web_dom_id(),
            Some("aura-contacts-start-chat")
        );
        assert_eq!(
            ControlId::ContactsInviteToChannelButton.web_dom_id(),
            Some("aura-contacts-invite-channel")
        );
        assert_eq!(
            ControlId::SettingsToggleThemeButton.web_dom_id(),
            Some("aura-settings-toggle-theme")
        );
        assert_eq!(
            ControlId::AuthorityPickerCancelButton.web_dom_id(),
            Some("aura-authority-picker-cancel-button")
        );
        assert_eq!(
            ControlId::DeviceEnrollmentPrimaryButton.web_dom_id(),
            Some("aura-device-enrollment-primary-button")
        );
        assert_eq!(
            ControlId::ContactsEditNicknameButton.web_dom_id(),
            Some("aura-contacts-edit-nickname")
        );
        assert_eq!(
            ControlId::ContactsRemoveContactButton.web_dom_id(),
            Some("aura-contacts-remove-contact")
        );
        assert_eq!(ControlId::ModalInput.web_dom_id(), Some("aura-modal-input"));
    }

    #[test]
    fn web_dom_ids_are_stable_for_shared_fields() {
        assert_eq!(
            FieldId::AccountName.web_dom_id(),
            Some("aura-account-name-input")
        );
        assert_eq!(
            FieldId::InvitationCode.web_selector().as_deref(),
            Some("#aura-field-invitation-code")
        );
        assert_eq!(
            FieldId::InvitationType.web_dom_id(),
            Some("aura-field-invitation-type")
        );
        assert_eq!(
            FieldId::InvitationMessage.web_dom_id(),
            Some("aura-field-invitation-message")
        );
        assert_eq!(
            FieldId::InvitationTtl.web_dom_id(),
            Some("aura-field-invitation-ttl")
        );
    }

    #[test]
    fn web_dom_ids_are_present_for_wrappers() {
        assert_eq!(
            ControlId::Field(FieldId::InvitationType).web_dom_id(),
            FieldId::InvitationType.web_dom_id()
        );
        assert_eq!(
            ControlId::List(ListId::Contacts).web_dom_id(),
            Some("aura-list-contacts")
        );
        assert_eq!(
            ControlId::Modal(ModalId::CreateInvitation).web_dom_id(),
            Some("aura-modal-create-invitation")
        );
    }

    #[test]
    fn all_contract_control_ids_have_web_dom_id() {
        let controls = [
            ControlId::AppRoot,
            ControlId::OnboardingRoot,
            ControlId::ModalRegion,
            ControlId::ToastRegion,
            ControlId::OnboardingCreateAccountButton,
            ControlId::OnboardingImportDeviceButton,
            ControlId::ModalCopyButton,
            ControlId::NavRoot,
            ControlId::NavNeighborhood,
            ControlId::NavChat,
            ControlId::NavContacts,
            ControlId::NavNotifications,
            ControlId::NavSettings,
            ControlId::NeighborhoodNewHomeButton,
            ControlId::NeighborhoodAcceptInvitationButton,
            ControlId::ContactsCreateInvitationButton,
            ControlId::ContactsInviteToChannelButton,
            ControlId::NeighborhoodEnterAsButton,
            ControlId::ChatNewGroupButton,
            ControlId::ContactsStartChatButton,
            ControlId::SettingsEditNicknameButton,
            ControlId::SettingsConfigureThresholdButton,
            ControlId::SettingsRequestRecoveryButton,
            ControlId::SettingsImportDeviceCodeButton,
            ControlId::SettingsSwitchAuthorityButton,
            ControlId::SettingsConfigureMfaButton,
            ControlId::SettingsToggleThemeButton,
            ControlId::DeviceEnrollmentCancelButton,
            ControlId::DeviceEnrollmentPrimaryButton,
            ControlId::AuthorityPickerCancelButton,
            ControlId::AuthorityPickerConfirmButton,
            ControlId::ModalConfirmButton,
            ControlId::ModalCancelButton,
            ControlId::ContactsAcceptInvitationButton,
            ControlId::SettingsAddDeviceButton,
            ControlId::SettingsRemoveDeviceButton,
            ControlId::ChatSendMessageButton,
            ControlId::ContactsEditNicknameButton,
            ControlId::ContactsRemoveContactButton,
            ControlId::Screen(ScreenId::Onboarding),
            ControlId::Screen(ScreenId::Neighborhood),
            ControlId::Screen(ScreenId::Chat),
            ControlId::Screen(ScreenId::Contacts),
            ControlId::Screen(ScreenId::Notifications),
            ControlId::Screen(ScreenId::Settings),
            ControlId::Field(FieldId::AccountName),
            ControlId::Field(FieldId::InvitationCode),
            ControlId::Field(FieldId::InvitationReceiver),
            ControlId::Field(FieldId::InvitationType),
            ControlId::Field(FieldId::InvitationMessage),
            ControlId::Field(FieldId::InvitationTtl),
            ControlId::Field(FieldId::ChatInput),
            ControlId::Field(FieldId::HomeName),
            ControlId::Field(FieldId::CreateChannelName),
            ControlId::Field(FieldId::CreateChannelTopic),
            ControlId::Field(FieldId::ThresholdInput),
            ControlId::Field(FieldId::Nickname),
            ControlId::Field(FieldId::DeviceName),
            ControlId::Field(FieldId::DeviceImportCode),
            ControlId::Field(FieldId::CapabilityFull),
            ControlId::Field(FieldId::CapabilityPartial),
            ControlId::Field(FieldId::CapabilityLimited),
            ControlId::ModalInput,
            ControlId::List(ListId::Navigation),
            ControlId::List(ListId::Channels),
            ControlId::List(ListId::Contacts),
            ControlId::List(ListId::Notifications),
            ControlId::List(ListId::InvitationTypes),
            ControlId::List(ListId::Homes),
            ControlId::List(ListId::NeighborhoodMembers),
            ControlId::List(ListId::Devices),
            ControlId::List(ListId::Authorities),
            ControlId::List(ListId::SettingsSections),
            ControlId::Modal(ModalId::Help),
            ControlId::Modal(ModalId::CreateInvitation),
            ControlId::Modal(ModalId::InvitationCode),
            ControlId::Modal(ModalId::AcceptInvitation),
            ControlId::Modal(ModalId::CreateHome),
            ControlId::Modal(ModalId::CreateChannel),
            ControlId::Modal(ModalId::SetChannelTopic),
            ControlId::Modal(ModalId::ChannelInfo),
            ControlId::Modal(ModalId::EditNickname),
            ControlId::Modal(ModalId::RemoveContact),
            ControlId::Modal(ModalId::GuardianSetup),
            ControlId::Modal(ModalId::RequestRecovery),
            ControlId::Modal(ModalId::AddDevice),
            ControlId::Modal(ModalId::ImportDeviceEnrollmentCode),
            ControlId::Modal(ModalId::SelectDeviceToRemove),
            ControlId::Modal(ModalId::ConfirmRemoveDevice),
            ControlId::Modal(ModalId::MfaSetup),
            ControlId::Modal(ModalId::AssignModerator),
            ControlId::Modal(ModalId::SwitchAuthority),
            ControlId::Modal(ModalId::AccessOverride),
            ControlId::Modal(ModalId::CapabilityConfig),
        ];

        assert!(
            controls
                .iter()
                .all(|control| control.web_dom_id().is_some()),
            "all contract controls must be addressable"
        );
    }

    #[test]
    fn render_heartbeat_is_typed_and_serializable() {
        let heartbeat = RenderHeartbeat {
            screen: ScreenId::Chat,
            open_modal: Some(ModalId::AcceptInvitation),
            render_seq: 7,
        };
        let json = serde_json::to_string(&heartbeat).expect("heartbeat should serialize");
        assert!(json.contains("\"screen\":\"chat\""));
        assert!(json.contains("\"open_modal\":\"accept_invitation\""));
        assert!(json.contains("\"render_seq\":7"));
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

    #[test]
    fn shared_flow_support_contract_is_consistent() {
        let all_declared: HashSet<_> = ALL_SHARED_FLOW_IDS.iter().copied().collect();
        let unique: HashSet<_> = SHARED_FLOW_SUPPORT
            .iter()
            .map(|support| support.flow)
            .collect();
        assert_eq!(unique.len(), SHARED_FLOW_SUPPORT.len());
        assert_eq!(
            unique, all_declared,
            "shared flow support manifest must stay exhaustive"
        );

        let scenario_coverage_unique: HashSet<_> = SHARED_FLOW_SCENARIO_COVERAGE
            .iter()
            .map(|coverage| (coverage.flow, coverage.scenario_id))
            .collect();
        assert_eq!(
            scenario_coverage_unique.len(),
            SHARED_FLOW_SCENARIO_COVERAGE.len()
        );

        for support in SHARED_FLOW_SUPPORT {
            if support.web == FlowAvailability::Supported
                && support.tui == FlowAvailability::Supported
            {
                assert!(
                    !shared_flow_scenarios(support.flow).is_empty(),
                    "shared flow {:?} must declare at least one parity scenario",
                    support.flow
                );
            }
        }

        let theme_support = shared_flow_support(SharedFlowId::ThemeAppearance);
        assert_eq!(theme_support.web, FlowAvailability::Supported);
        assert_eq!(
            theme_support.tui,
            FlowAvailability::Exception(ParityException::BrowserThemeControl)
        );
    }

    #[test]
    fn shared_flow_scenario_coverage_points_to_existing_scenarios() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root");
        let scenarios_dir = workspace_root.join("scenarios").join("harness");
        let entries = std::fs::read_dir(&scenarios_dir).expect("scenario directory should exist");
        let mut known_ids = HashSet::new();
        for entry in entries {
            let entry = entry.expect("scenario dir entry");
            if entry.path().extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            let body = std::fs::read_to_string(entry.path()).expect("scenario file should read");
            for line in body.lines() {
                let trimmed = line.trim();
                if let Some(id) = trimmed.strip_prefix("id = \"") {
                    if let Some(id) = id.strip_suffix('"') {
                        known_ids.insert(id.to_string());
                    }
                    break;
                }
            }
        }

        for coverage in SHARED_FLOW_SCENARIO_COVERAGE {
            assert!(
                known_ids.contains(coverage.scenario_id),
                "shared flow {:?} references missing scenario id {}",
                coverage.flow,
                coverage.scenario_id
            );
        }
    }

    #[test]
    fn shared_screen_modal_and_list_support_is_unique_and_addressable() {
        let unique_screens: HashSet<_> = SHARED_SCREEN_SUPPORT
            .iter()
            .map(|support| support.screen)
            .collect();
        assert_eq!(unique_screens.len(), SHARED_SCREEN_SUPPORT.len());

        let unique_modals: HashSet<_> = SHARED_MODAL_SUPPORT
            .iter()
            .map(|support| support.modal)
            .collect();
        assert_eq!(unique_modals.len(), SHARED_MODAL_SUPPORT.len());

        let unique_lists: HashSet<_> = SHARED_LIST_SUPPORT
            .iter()
            .map(|support| support.list)
            .collect();
        assert_eq!(unique_lists.len(), SHARED_LIST_SUPPORT.len());

        assert_eq!(
            shared_screen_support(ScreenId::Settings).web,
            FlowAvailability::Supported
        );
        assert_eq!(
            shared_modal_support(ModalId::AcceptInvitation).tui,
            FlowAvailability::Supported
        );
        assert_eq!(
            shared_list_support(ListId::Contacts).web,
            FlowAvailability::Supported
        );
    }

    #[test]
    fn shared_screen_module_map_uses_canonical_screen_names() {
        let unique_screens: HashSet<_> = SHARED_SCREEN_MODULE_MAP
            .iter()
            .map(|mapping| mapping.screen)
            .collect();
        assert_eq!(unique_screens.len(), SHARED_SCREEN_MODULE_MAP.len());

        let chat = shared_screen_module_map(ScreenId::Chat);
        assert_eq!(chat.web_symbol, "ChatScreen");
        assert_eq!(chat.tui_symbol, "ChatScreen");
        assert!(chat.web_path.ends_with("crates/aura-ui/src/app.rs"));
        assert!(chat
            .tui_path
            .ends_with("crates/aura-terminal/src/tui/screens/chat/screen.rs"));
    }

    #[test]
    fn ui_snapshot_parity_ignores_occurrence_ids_but_catches_state_drift() {
        let web = UiSnapshot {
            screen: ScreenId::Chat,
            focused_control: Some(ControlId::Screen(ScreenId::Chat)),
            open_modal: None,
            readiness: UiReadiness::Ready,
            selections: vec![SelectionSnapshot {
                list: ListId::Channels,
                item_id: "amp-bridge".to_string(),
            }],
            lists: vec![ListSnapshot {
                id: ListId::Channels,
                items: vec![ListItemSnapshot {
                    id: "amp-bridge".to_string(),
                    selected: true,
                    confirmation: ConfirmationState::Confirmed,
                }],
            }],
            messages: vec![MessageSnapshot {
                id: "web-1".to_string(),
                content: "hello".to_string(),
            }],
            operations: vec![OperationSnapshot {
                id: OperationId::invitation_accept(),
                instance_id: OperationInstanceId("web-op".to_string()),
                state: OperationState::Succeeded,
            }],
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };
        let mut tui = web.clone();
        tui.focused_control = Some(ControlId::List(ListId::Channels));
        tui.messages[0].id = "tui-1".to_string();
        tui.operations[0].instance_id = OperationInstanceId("tui-op".to_string());

        assert!(compare_ui_snapshots_for_parity(&web, &tui).is_empty());

        tui.messages[0].content = "different".to_string();
        let mismatches = compare_ui_snapshots_for_parity(&web, &tui);
        assert_eq!(
            mismatches,
            vec![UiParityMismatch {
                field: "messages",
                web: "[\"hello\"]".to_string(),
                tui: "[\"different\"]".to_string(),
            }]
        );
    }

    #[test]
    fn parity_module_map_points_to_existing_frontend_symbols() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");

        for mapping in SHARED_SCREEN_MODULE_MAP {
            let web_path = workspace_root.join(mapping.web_path);
            let tui_path = workspace_root.join(mapping.tui_path);
            assert!(
                web_path.exists(),
                "missing web parity path for {:?}: {}",
                mapping.screen,
                web_path.display()
            );
            assert!(
                tui_path.exists(),
                "missing tui parity path for {:?}: {}",
                mapping.screen,
                tui_path.display()
            );

            let web_source = std::fs::read_to_string(&web_path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", web_path.display()));
            let tui_source = std::fs::read_to_string(&tui_path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", tui_path.display()));

            assert!(
                web_source.contains(mapping.web_symbol),
                "web parity symbol {:?} missing from {}",
                mapping.web_symbol,
                web_path.display()
            );
            assert!(
                tui_source.contains(mapping.tui_symbol),
                "tui parity symbol {:?} missing from {}",
                mapping.tui_symbol,
                tui_path.display()
            );
        }
    }

    #[test]
    fn parity_ignores_non_active_screen_lists_and_normalizes_settings_sections() {
        let web = UiSnapshot {
            screen: ScreenId::Settings,
            focused_control: Some(ControlId::Screen(ScreenId::Settings)),
            open_modal: None,
            readiness: UiReadiness::Ready,
            selections: vec![
                SelectionSnapshot {
                    list: ListId::Navigation,
                    item_id: "settings".to_string(),
                },
                SelectionSnapshot {
                    list: ListId::SettingsSections,
                    item_id: "guardian-threshold".to_string(),
                },
                SelectionSnapshot {
                    list: ListId::Homes,
                    item_id: "stale-home".to_string(),
                },
            ],
            lists: vec![
                ListSnapshot {
                    id: ListId::Navigation,
                    items: vec![ListItemSnapshot {
                        id: "settings".to_string(),
                        selected: true,
                        confirmation: ConfirmationState::Confirmed,
                    }],
                },
                ListSnapshot {
                    id: ListId::SettingsSections,
                    items: vec![
                        ListItemSnapshot {
                            id: "guardian-threshold".to_string(),
                            selected: true,
                            confirmation: ConfirmationState::Confirmed,
                        },
                        ListItemSnapshot {
                            id: "appearance".to_string(),
                            selected: false,
                            confirmation: ConfirmationState::Confirmed,
                        },
                    ],
                },
                ListSnapshot {
                    id: ListId::Homes,
                    items: vec![ListItemSnapshot {
                        id: "stale-home".to_string(),
                        selected: true,
                        confirmation: ConfirmationState::Confirmed,
                    }],
                },
            ],
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };
        let tui = UiSnapshot {
            screen: ScreenId::Settings,
            focused_control: Some(ControlId::Screen(ScreenId::Settings)),
            open_modal: None,
            readiness: UiReadiness::Ready,
            selections: vec![
                SelectionSnapshot {
                    list: ListId::Navigation,
                    item_id: "settings".to_string(),
                },
                SelectionSnapshot {
                    list: ListId::SettingsSections,
                    item_id: "guardian_threshold".to_string(),
                },
                SelectionSnapshot {
                    list: ListId::NeighborhoodMembers,
                    item_id: "stale-member".to_string(),
                },
            ],
            lists: vec![
                ListSnapshot {
                    id: ListId::Navigation,
                    items: vec![ListItemSnapshot {
                        id: "settings".to_string(),
                        selected: true,
                        confirmation: ConfirmationState::Confirmed,
                    }],
                },
                ListSnapshot {
                    id: ListId::SettingsSections,
                    items: vec![
                        ListItemSnapshot {
                            id: "guardian_threshold".to_string(),
                            selected: true,
                            confirmation: ConfirmationState::Confirmed,
                        },
                        ListItemSnapshot {
                            id: "observability".to_string(),
                            selected: false,
                            confirmation: ConfirmationState::Confirmed,
                        },
                    ],
                },
                ListSnapshot {
                    id: ListId::NeighborhoodMembers,
                    items: vec![ListItemSnapshot {
                        id: "stale-member".to_string(),
                        selected: true,
                        confirmation: ConfirmationState::Confirmed,
                    }],
                },
            ],
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };

        assert!(compare_ui_snapshots_for_parity(&web, &tui).is_empty());
    }
}
