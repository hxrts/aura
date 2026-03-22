//! Shared UI-facing semantic contract for Aura frontends and harnesses.
//!
//! This module defines stable application-facing UI identifiers and snapshot
//! types that can be shared across the web UI, TUI, and harness tooling.

#![allow(missing_docs)] // Shared contract surface - refined incrementally during migration.

use aura_core::{OwnerEpoch, PublicationSequence};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::scenario_contract::SemanticCommandValue;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HarnessUiCommand {
    Ping,
    NavigateScreen {
        screen: ScreenId,
    },
    OpenSettingsSection {
        section: crate::scenario_contract::SettingsSection,
    },
    DismissTransient,
    ActivateControl {
        control_id: ControlId,
    },
    ActivateListItem {
        list_id: ListId,
        item_id: String,
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
    SelectHome {
        home_id: String,
    },
    StartDeviceEnrollment {
        device_name: String,
        invitee_authority_id: String,
    },
    ImportDeviceEnrollmentCode {
        code: String,
    },
    RemoveSelectedDevice {
        #[serde(default)]
        device_id: Option<String>,
    },
    SwitchAuthority {
        authority_id: String,
    },
    CreateContactInvitation {
        receiver_authority_id: String,
    },
    ImportInvitation {
        code: String,
    },
    InviteActorToChannel {
        authority_id: String,
        channel_id: String,
    },
    AcceptPendingChannelInvitation,
    JoinChannel {
        channel_name: String,
    },
    SelectChannel {
        channel_id: String,
    },
    SendChatMessage {
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessUiOperationHandle {
    operation_id: OperationId,
    instance_id: OperationInstanceId,
}

impl HarnessUiOperationHandle {
    #[must_use]
    pub const fn new(operation_id: OperationId, instance_id: OperationInstanceId) -> Self {
        Self {
            operation_id,
            instance_id,
        }
    }

    #[must_use]
    pub const fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    #[must_use]
    pub const fn instance_id(&self) -> &OperationInstanceId {
        &self.instance_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "submission", rename_all = "snake_case")]
pub enum HarnessUiCommandReceipt {
    Accepted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<SemanticCommandValue>,
    },
    AcceptedWithOperation {
        operation: HarnessUiOperationHandle,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<SemanticCommandValue>,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelBindingWitness {
    pub channel_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
}

impl ChannelBindingWitness {
    #[must_use]
    pub fn new(channel_id: impl Into<String>, context_id: Option<String>) -> Self {
        Self {
            channel_id: channel_id.into(),
            context_id,
        }
    }

    #[must_use]
    pub fn semantic_value(&self) -> SemanticCommandValue {
        match &self.context_id {
            Some(context_id) => SemanticCommandValue::AuthoritativeChannelBinding {
                channel_id: self.channel_id.clone(),
                context_id: context_id.clone(),
            },
            None => SemanticCommandValue::ChannelSelection {
                channel_id: self.channel_id.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedPendingChannelBinding {
    pub invitation_id: String,
    pub binding: ChannelBindingWitness,
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

fn is_placeholder_semantic_id(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.starts_with("placeholder:") {
        return true;
    }
    trimmed
        .rsplit_once(':')
        .map(|(_, suffix)| suffix.len() >= 8 && suffix.chars().all(|ch| ch == '0'))
        .unwrap_or(false)
}

fn is_override_semantic_id(raw: &str) -> bool {
    raw.trim().starts_with("override:")
}

fn is_row_index_semantic_id(raw: &str) -> bool {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.chars().all(|ch| ch.is_ascii_digit())
        || trimmed
            .strip_prefix("row-")
            .or_else(|| trimmed.strip_prefix("row_"))
            .or_else(|| trimmed.strip_prefix("row:"))
            .or_else(|| trimmed.strip_prefix("idx-"))
            .or_else(|| trimmed.strip_prefix("idx_"))
            .or_else(|| trimmed.strip_prefix("idx:"))
            .or_else(|| trimmed.strip_prefix("index-"))
            .or_else(|| trimmed.strip_prefix("index_"))
            .or_else(|| trimmed.strip_prefix("index:"))
            .map(|suffix| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()))
            .unwrap_or(false)
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
pub enum SharedSettingsSectionId {
    Profile,
    GuardianThreshold,
    RequestRecovery,
    Devices,
    Authority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendSpecificSettingsSectionId {
    Appearance,
    Info,
    Observability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSectionSurfaceId {
    Shared(SharedSettingsSectionId),
    FrontendSpecific(FrontendSpecificSettingsSectionId),
}

pub const PARITY_CRITICAL_SETTINGS_SECTIONS: &[SharedSettingsSectionId] = &[
    SharedSettingsSectionId::Profile,
    SharedSettingsSectionId::GuardianThreshold,
    SharedSettingsSectionId::RequestRecovery,
    SharedSettingsSectionId::Devices,
    SharedSettingsSectionId::Authority,
];

pub const FRONTEND_SPECIFIC_SETTINGS_SECTIONS: &[FrontendSpecificSettingsSectionId] = &[
    FrontendSpecificSettingsSectionId::Appearance,
    FrontendSpecificSettingsSectionId::Info,
    FrontendSpecificSettingsSectionId::Observability,
];

#[must_use]
pub const fn screen_item_id(screen: ScreenId) -> &'static str {
    match screen {
        ScreenId::Onboarding => "onboarding",
        ScreenId::Neighborhood => "neighborhood",
        ScreenId::Chat => "chat",
        ScreenId::Contacts => "contacts",
        ScreenId::Notifications => "notifications",
        ScreenId::Settings => "settings",
    }
}

#[must_use]
pub fn classify_screen_item_id(item_id: &str) -> Option<ScreenId> {
    match item_id.trim() {
        "onboarding" => Some(ScreenId::Onboarding),
        "neighborhood" => Some(ScreenId::Neighborhood),
        "chat" => Some(ScreenId::Chat),
        "contacts" => Some(ScreenId::Contacts),
        "notifications" => Some(ScreenId::Notifications),
        "settings" => Some(ScreenId::Settings),
        _ => None,
    }
}

#[must_use]
pub const fn nav_control_id_for_screen(screen: ScreenId) -> ControlId {
    match screen {
        ScreenId::Onboarding => ControlId::OnboardingRoot,
        ScreenId::Neighborhood => ControlId::NavNeighborhood,
        ScreenId::Chat => ControlId::NavChat,
        ScreenId::Contacts => ControlId::NavContacts,
        ScreenId::Notifications => ControlId::NavNotifications,
        ScreenId::Settings => ControlId::NavSettings,
    }
}

#[must_use]
pub const fn settings_section_item_id(surface: SettingsSectionSurfaceId) -> &'static str {
    match surface {
        SettingsSectionSurfaceId::Shared(SharedSettingsSectionId::Profile) => "profile",
        SettingsSectionSurfaceId::Shared(SharedSettingsSectionId::GuardianThreshold) => {
            "guardian-threshold"
        }
        SettingsSectionSurfaceId::Shared(SharedSettingsSectionId::RequestRecovery) => {
            "request-recovery"
        }
        SettingsSectionSurfaceId::Shared(SharedSettingsSectionId::Devices) => "devices",
        SettingsSectionSurfaceId::Shared(SharedSettingsSectionId::Authority) => "authority",
        SettingsSectionSurfaceId::FrontendSpecific(
            FrontendSpecificSettingsSectionId::Appearance,
        ) => "appearance",
        SettingsSectionSurfaceId::FrontendSpecific(FrontendSpecificSettingsSectionId::Info) => {
            "info"
        }
        SettingsSectionSurfaceId::FrontendSpecific(
            FrontendSpecificSettingsSectionId::Observability,
        ) => "observability",
    }
}

#[must_use]
pub fn classify_settings_section_item_id(item_id: &str) -> Option<SettingsSectionSurfaceId> {
    match item_id.trim() {
        "profile" => Some(SettingsSectionSurfaceId::Shared(
            SharedSettingsSectionId::Profile,
        )),
        "guardian-threshold" => Some(SettingsSectionSurfaceId::Shared(
            SharedSettingsSectionId::GuardianThreshold,
        )),
        "request-recovery" => Some(SettingsSectionSurfaceId::Shared(
            SharedSettingsSectionId::RequestRecovery,
        )),
        "devices" => Some(SettingsSectionSurfaceId::Shared(
            SharedSettingsSectionId::Devices,
        )),
        "authority" => Some(SettingsSectionSurfaceId::Shared(
            SharedSettingsSectionId::Authority,
        )),
        "appearance" => Some(SettingsSectionSurfaceId::FrontendSpecific(
            FrontendSpecificSettingsSectionId::Appearance,
        )),
        "info" => Some(SettingsSectionSurfaceId::FrontendSpecific(
            FrontendSpecificSettingsSectionId::Info,
        )),
        "observability" => Some(SettingsSectionSurfaceId::FrontendSpecific(
            FrontendSpecificSettingsSectionId::Observability,
        )),
        _ => None,
    }
}

#[must_use]
pub const fn semantic_settings_section_surface_id(
    section: crate::scenario_contract::SettingsSection,
) -> SettingsSectionSurfaceId {
    match section {
        crate::scenario_contract::SettingsSection::Devices => {
            SettingsSectionSurfaceId::Shared(SharedSettingsSectionId::Devices)
        }
    }
}

#[must_use]
pub const fn semantic_settings_section_item_id(
    section: crate::scenario_contract::SettingsSection,
) -> &'static str {
    settings_section_item_id(semantic_settings_section_surface_id(section))
}

#[must_use]
pub fn classify_semantic_settings_section_item_id(
    item_id: &str,
) -> Option<crate::scenario_contract::SettingsSection> {
    match classify_settings_section_item_id(item_id) {
        Some(SettingsSectionSurfaceId::Shared(SharedSettingsSectionId::Devices)) => {
            Some(crate::scenario_contract::SettingsSection::Devices)
        }
        _ => None,
    }
}

pub struct ParityUiIdentity;

impl ParityUiIdentity {
    #[must_use]
    pub const fn control_dom_id(control_id: ControlId) -> Option<&'static str> {
        control_id.web_dom_id()
    }

    #[must_use]
    pub const fn field_dom_id(field_id: FieldId) -> Option<&'static str> {
        field_id.web_dom_id()
    }

    #[must_use]
    pub const fn list_dom_id(list_id: ListId) -> &'static str {
        list_id.web_dom_id()
    }

    #[must_use]
    pub const fn modal_dom_id(modal_id: ModalId) -> &'static str {
        modal_id.web_dom_id()
    }

    #[must_use]
    pub fn list_item_dom_id(list_id: ListId, item_id: &str) -> String {
        list_item_dom_id(list_id, item_id)
    }
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

#[aura_macros::ownership_lifecycle(
    initial = "Idle",
    ordered = "Idle,Submitting",
    terminals = "Succeeded,Failed"
)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOperationKind {
    CreateAccount,
    CreateHome,
    CreateChannel,
    StartDeviceEnrollment,
    ImportDeviceEnrollmentCode,
    CreateContactInvitation,
    AcceptContactInvitation,
    InviteActorToChannel,
    AcceptPendingChannelInvitation,
    JoinChannel,
    SendChatMessage,
}

#[aura_macros::ownership_lifecycle(
    initial = "Submitted",
    ordered = "Submitted,WorkflowDispatched,AuthoritativeContextReady,ContactLinkReady,MembershipReady,RecipientResolutionReady,PeerChannelReady,DeliveryReady",
    terminals = "Succeeded,Failed,Cancelled"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOperationPhase {
    Submitted,
    WorkflowDispatched,
    AuthoritativeContextReady,
    ContactLinkReady,
    MembershipReady,
    RecipientResolutionReady,
    PeerChannelReady,
    DeliveryReady,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticFailureDomain {
    Command,
    Invitation,
    ChannelContext,
    Transport,
    Delivery,
    Projection,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticFailureCode {
    UnsupportedCommand,
    MissingAuthoritativeContext,
    ContactLinkDidNotConverge,
    ChannelBootstrapUnavailable,
    PeerChannelNotEstablished,
    DeliveryReadinessNotReached,
    OperationTimedOut,
    ShellDeclaredSuccessIllegally,
    InternalError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticOperationError {
    pub domain: SemanticFailureDomain,
    pub code: SemanticFailureCode,
    pub detail: Option<String>,
}

impl SemanticOperationError {
    #[must_use]
    pub fn new(domain: SemanticFailureDomain, code: SemanticFailureCode) -> Self {
        Self {
            domain,
            code,
            detail: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticOperationStatus {
    pub kind: SemanticOperationKind,
    pub phase: SemanticOperationPhase,
    pub error: Option<SemanticOperationError>,
}

impl SemanticOperationStatus {
    #[must_use]
    pub fn new(kind: SemanticOperationKind, phase: SemanticOperationPhase) -> Self {
        Self {
            kind,
            phase,
            error: None,
        }
    }

    #[must_use]
    pub fn failed(kind: SemanticOperationKind, error: SemanticOperationError) -> Self {
        Self {
            kind,
            phase: SemanticOperationPhase::Failed,
            error: Some(error),
        }
    }

    #[must_use]
    pub fn cancelled(kind: SemanticOperationKind) -> Self {
        Self {
            kind,
            phase: SemanticOperationPhase::Cancelled,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowTerminalStatus {
    pub causality: Option<SemanticOperationCausality>,
    pub status: SemanticOperationStatus,
}

#[derive(Debug)]
pub struct WorkflowTerminalOutcome<T> {
    pub result: Result<T, aura_core::AuraError>,
    pub terminal: Option<WorkflowTerminalStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticOperationCausality {
    pub owner_epoch: OwnerEpoch,
    pub publication_sequence: PublicationSequence,
}

impl SemanticOperationCausality {
    #[must_use]
    pub const fn new(owner_epoch: OwnerEpoch, publication_sequence: PublicationSequence) -> Self {
        Self {
            owner_epoch,
            publication_sequence,
        }
    }

    #[must_use]
    pub fn is_older_than(self, other: Self) -> bool {
        (self.owner_epoch.value(), self.publication_sequence.value())
            < (
                other.owner_epoch.value(),
                other.publication_sequence.value(),
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoritativeSemanticFactKind {
    OperationStatus,
    ContactLinkReady,
    PendingHomeInvitationReady,
    ChannelMembershipReady,
    RecipientPeersResolved,
    PeerChannelReady,
    MessageCommitted,
    MessageDeliveryReady,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthoritativeSemanticFact {
    OperationStatus {
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        causality: Option<SemanticOperationCausality>,
        status: SemanticOperationStatus,
    },
    ContactLinkReady {
        authority_id: String,
        contact_count: u32,
    },
    PendingHomeInvitationReady,
    ChannelMembershipReady {
        channel: ChannelFactKey,
        member_count: u32,
    },
    RecipientPeersResolved {
        channel: ChannelFactKey,
        member_count: u32,
    },
    PeerChannelReady {
        channel: ChannelFactKey,
        peer_authority_id: String,
        context_id: Option<String>,
    },
    MessageCommitted {
        channel: ChannelFactKey,
        content: String,
    },
    MessageDeliveryReady {
        channel: ChannelFactKey,
        member_count: u32,
    },
}

impl AuthoritativeSemanticFact {
    #[must_use]
    pub fn kind(&self) -> AuthoritativeSemanticFactKind {
        match self {
            Self::OperationStatus { .. } => AuthoritativeSemanticFactKind::OperationStatus,
            Self::ContactLinkReady { .. } => AuthoritativeSemanticFactKind::ContactLinkReady,
            Self::PendingHomeInvitationReady => {
                AuthoritativeSemanticFactKind::PendingHomeInvitationReady
            }
            Self::ChannelMembershipReady { .. } => {
                AuthoritativeSemanticFactKind::ChannelMembershipReady
            }
            Self::RecipientPeersResolved { .. } => {
                AuthoritativeSemanticFactKind::RecipientPeersResolved
            }
            Self::PeerChannelReady { .. } => AuthoritativeSemanticFactKind::PeerChannelReady,
            Self::MessageCommitted { .. } => AuthoritativeSemanticFactKind::MessageCommitted,
            Self::MessageDeliveryReady { .. } => {
                AuthoritativeSemanticFactKind::MessageDeliveryReady
            }
        }
    }

    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::OperationStatus {
                operation_id,
                instance_id,
                ..
            } => format!(
                "operation_status:{}:{}",
                operation_id.0,
                instance_id
                    .as_ref()
                    .map(|value| value.0.as_str())
                    .unwrap_or("*")
            ),
            Self::ContactLinkReady { authority_id, .. } => {
                format!("contact_link_ready:{authority_id}")
            }
            Self::PendingHomeInvitationReady => "pending_home_invitation_ready".to_string(),
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
            Self::PeerChannelReady {
                channel,
                peer_authority_id,
                ..
            } => format!(
                "peer_channel_ready:{}:{peer_authority_id}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
            Self::MessageCommitted { channel, content } => format!(
                "message_committed:{}:{}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*"),
                content
            ),
            Self::MessageDeliveryReady { channel, .. } => format!(
                "message_delivery_ready:{}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
        }
    }

    #[must_use]
    pub fn runtime_fact_bridge(&self) -> Option<(RuntimeEventKind, RuntimeFact)> {
        match self {
            Self::ContactLinkReady {
                authority_id,
                contact_count,
            } => Some((
                RuntimeEventKind::ContactLinkReady,
                RuntimeFact::ContactLinkReady {
                    authority_id: Some(authority_id.clone()),
                    contact_count: Some(*contact_count as usize),
                },
            )),
            Self::PendingHomeInvitationReady => Some((
                RuntimeEventKind::PendingHomeInvitationReady,
                RuntimeFact::PendingHomeInvitationReady,
            )),
            Self::ChannelMembershipReady {
                channel,
                member_count,
            } => Some((
                RuntimeEventKind::ChannelMembershipReady,
                RuntimeFact::ChannelMembershipReady {
                    channel: channel.clone(),
                    member_count: Some(*member_count),
                },
            )),
            Self::RecipientPeersResolved {
                channel,
                member_count,
            } => Some((
                RuntimeEventKind::RecipientPeersResolved,
                RuntimeFact::RecipientPeersResolved {
                    channel: channel.clone(),
                    member_count: *member_count,
                },
            )),
            Self::MessageCommitted { channel, content } => Some((
                RuntimeEventKind::MessageCommitted,
                RuntimeFact::MessageCommitted {
                    channel: channel.clone(),
                    content: content.clone(),
                },
            )),
            Self::MessageDeliveryReady {
                channel,
                member_count,
            } => Some((
                RuntimeEventKind::MessageDeliveryReady,
                RuntimeFact::MessageDeliveryReady {
                    channel: channel.clone(),
                    member_count: *member_count,
                },
            )),
            Self::OperationStatus { .. } | Self::PeerChannelReady { .. } => None,
        }
    }

    #[must_use]
    pub fn operation_status_bridge(
        &self,
    ) -> Option<(
        OperationId,
        Option<OperationInstanceId>,
        Option<SemanticOperationCausality>,
        SemanticOperationStatus,
    )> {
        match self {
            Self::OperationStatus {
                operation_id,
                instance_id,
                causality,
                status,
            } => Some((
                operation_id.clone(),
                instance_id.clone(),
                *causality,
                status.clone(),
            )),
            _ => None,
        }
    }
}

#[must_use]
pub fn bridged_operation_statuses(
    facts: &[AuthoritativeSemanticFact],
) -> Vec<(
    OperationId,
    Option<OperationInstanceId>,
    Option<SemanticOperationCausality>,
    SemanticOperationStatus,
)> {
    let mut bridged = facts
        .iter()
        .filter_map(AuthoritativeSemanticFact::operation_status_bridge)
        .collect::<Vec<_>>();

    let contact_link_ready = facts
        .iter()
        .any(|fact| matches!(fact, AuthoritativeSemanticFact::ContactLinkReady { .. }));

    if contact_link_ready {
        for (operation_id, _instance_id, _causality, status) in &mut bridged {
            if *operation_id == OperationId::invitation_accept()
                && status.kind == SemanticOperationKind::AcceptContactInvitation
                && !status.phase.is_terminal()
            {
                *status = SemanticOperationStatus::new(
                    SemanticOperationKind::AcceptContactInvitation,
                    SemanticOperationPhase::Succeeded,
                );
            }
        }
    }

    bridged
}

impl OperationId {
    #[must_use]
    pub fn account_create() -> Self {
        Self("account_create".to_string())
    }

    #[must_use]
    pub fn create_home() -> Self {
        Self("create_home".to_string())
    }

    #[must_use]
    pub fn create_channel() -> Self {
        Self("create_channel".to_string())
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

    #[must_use]
    pub fn send_message() -> Self {
        Self("send_message".to_string())
    }

    #[must_use]
    pub fn join_channel() -> Self {
        Self("join_channel".to_string())
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
    #[serde(default)]
    pub is_current: bool,
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
        code: Option<String>,
    },
    PendingHomeInvitationReady,
    DeviceEnrollmentCodeReady {
        device_name: Option<String>,
        code_len: Option<usize>,
        code: Option<String>,
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
        member_count: Option<u32>,
    },
    RecipientPeersResolved {
        channel: ChannelFactKey,
        member_count: u32,
    },
    MessageCommitted {
        channel: ChannelFactKey,
        content: String,
    },
    MessageDeliveryReady {
        channel: ChannelFactKey,
        member_count: u32,
    },
    RemoteFactsPulled {
        contact_count: u32,
        lan_peer_count: u32,
    },
    ChatSignalUpdated {
        active_channel: String,
        channel_count: u32,
        message_count: u32,
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
                ..
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
                code,
            } => {
                receiver_authority_id
                    .as_deref()
                    .is_some_and(|value| value.contains(needle))
                    || source_operation.0.contains(needle)
                    || code.as_deref().is_some_and(|value| value.contains(needle))
            }
            Self::PendingHomeInvitationReady => needle.contains("pending_home_invitation"),
            Self::DeviceEnrollmentCodeReady {
                device_name,
                code_len,
                code,
            } => {
                device_name
                    .as_deref()
                    .is_some_and(|value| value.contains(needle))
                    || code_len.is_some_and(|value| value.to_string().contains(needle))
                    || code.as_deref().is_some_and(|value| value.contains(needle))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessShellMode {
    App,
    Onboarding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessShellStructureSnapshot {
    pub screen: ScreenId,
    pub app_root_count: u32,
    pub modal_region_count: u32,
    pub onboarding_root_count: u32,
    pub toast_region_count: u32,
    pub active_screen_root_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionRevision {
    pub semantic_seq: u64,
    pub render_seq: Option<u64>,
}

impl ProjectionRevision {
    #[must_use]
    pub const fn has_sequence_metadata(self) -> bool {
        self.semantic_seq > 0 || self.render_seq.is_some()
    }

    #[must_use]
    pub const fn is_newer_than(self, previous: Self) -> bool {
        let self_render_seq = match self.render_seq {
            Some(value) => value,
            None => 0,
        };
        let previous_render_seq = match previous.render_seq {
            Some(value) => value,
            None => 0,
        };
        self.semantic_seq > previous.semantic_seq
            || (self.semantic_seq == previous.semantic_seq && self_render_seq > previous_render_seq)
    }

    #[must_use]
    pub const fn is_stale_against(self, baseline: Self) -> bool {
        !self.is_newer_than(baseline)
    }
}

static SEMANTIC_REVISION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[must_use]
pub fn next_projection_revision(render_seq: Option<u64>) -> ProjectionRevision {
    let counter = SEMANTIC_REVISION_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    ProjectionRevision {
        semantic_seq: counter,
        render_seq,
    }
}

pub fn validate_render_convergence(
    snapshot: &UiSnapshot,
    heartbeat: &RenderHeartbeat,
) -> Result<(), String> {
    let Some(snapshot_render_seq) = snapshot.revision.render_seq else {
        return Err(format!(
            "semantic snapshot {:?} is missing render_seq metadata",
            snapshot.screen
        ));
    };
    if heartbeat.render_seq < snapshot_render_seq {
        return Err(format!(
            "semantic snapshot {:?} is ahead of renderer heartbeat {} < {}",
            snapshot.screen, heartbeat.render_seq, snapshot_render_seq
        ));
    }
    if heartbeat.screen != snapshot.screen {
        return Err(format!(
            "semantic snapshot screen {:?} diverges from renderer {:?}",
            snapshot.screen, heartbeat.screen
        ));
    }
    if heartbeat.open_modal != snapshot.open_modal {
        return Err(format!(
            "semantic snapshot modal {:?} diverges from renderer {:?}",
            snapshot.open_modal, heartbeat.open_modal
        ));
    }
    Ok(())
}

pub fn validate_harness_shell_structure(
    snapshot: &HarnessShellStructureSnapshot,
) -> Result<HarnessShellMode, String> {
    let onboarding_valid = snapshot.onboarding_root_count == 1
        && snapshot.app_root_count == 0
        && snapshot.modal_region_count == 0
        && snapshot.toast_region_count == 0
        && snapshot.active_screen_root_count == 0;
    if onboarding_valid {
        return Ok(HarnessShellMode::Onboarding);
    }

    let app_shell_valid = snapshot.app_root_count == 1
        && snapshot.modal_region_count == 1
        && snapshot.toast_region_count == 1
        && snapshot.active_screen_root_count == 1
        && snapshot.onboarding_root_count == 0;
    if app_shell_valid {
        return Ok(HarnessShellMode::App);
    }

    Err(format!(
        "invalid harness shell structure for {:?}: app_root_count={}, modal_region_count={}, onboarding_root_count={}, toast_region_count={}, active_screen_root_count={}",
        snapshot.screen,
        snapshot.app_root_count,
        snapshot.modal_region_count,
        snapshot.onboarding_root_count,
        snapshot.toast_region_count,
        snapshot.active_screen_root_count
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuiescenceState {
    Settled,
    Busy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuiescenceSnapshot {
    pub state: QuiescenceState,
    pub reason_codes: Vec<String>,
}

impl QuiescenceSnapshot {
    #[must_use]
    pub fn settled() -> Self {
        Self {
            state: QuiescenceState::Settled,
            reason_codes: Vec::new(),
        }
    }

    #[must_use]
    pub fn derive(
        readiness: UiReadiness,
        open_modal: Option<ModalId>,
        operations: &[OperationSnapshot],
    ) -> Self {
        let mut reason_codes = Vec::new();
        if readiness != UiReadiness::Ready {
            reason_codes.push("readiness_loading".to_string());
        }
        if let Some(modal_id) = open_modal {
            reason_codes.push(format!("modal_open:{modal_id:?}").to_ascii_lowercase());
        }
        for operation in operations {
            if operation.state == OperationState::Submitting {
                reason_codes.push(format!("operation_submitting:{}", operation.id.0));
            }
        }
        if reason_codes.is_empty() {
            Self::settled()
        } else {
            Self {
                state: QuiescenceState::Busy,
                reason_codes,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendId {
    Web,
    Tui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserCacheBoundary {
    SessionStart,
    AuthoritySwitch,
    DeviceImport,
    StorageReset,
    NavigationRecovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserCacheBoundaryMetadata {
    pub boundary: BrowserCacheBoundary,
    pub reason_code: &'static str,
}

pub const BROWSER_CACHE_BOUNDARIES: &[BrowserCacheBoundaryMetadata] = &[
    BrowserCacheBoundaryMetadata {
        boundary: BrowserCacheBoundary::SessionStart,
        reason_code: "session_start",
    },
    BrowserCacheBoundaryMetadata {
        boundary: BrowserCacheBoundary::AuthoritySwitch,
        reason_code: "authority_switch",
    },
    BrowserCacheBoundaryMetadata {
        boundary: BrowserCacheBoundary::DeviceImport,
        reason_code: "device_import",
    },
    BrowserCacheBoundaryMetadata {
        boundary: BrowserCacheBoundary::StorageReset,
        reason_code: "storage_reset",
    },
    BrowserCacheBoundaryMetadata {
        boundary: BrowserCacheBoundary::NavigationRecovery,
        reason_code: "navigation_recovery",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserHarnessBridgeMethodKind {
    Action,
    ReadState,
    Diagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserHarnessBridgeMethod {
    pub name: &'static str,
    pub kind: BrowserHarnessBridgeMethodKind,
    pub deterministic: bool,
    pub returns_semantic_state: bool,
    pub returns_render_signal: bool,
}

pub const BROWSER_HARNESS_BRIDGE_API_VERSION: u32 = 3;

pub const BROWSER_HARNESS_BRIDGE_METHODS: &[BrowserHarnessBridgeMethod] = &[
    BrowserHarnessBridgeMethod {
        name: "send_keys",
        kind: BrowserHarnessBridgeMethodKind::Action,
        deterministic: false,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "send_key",
        kind: BrowserHarnessBridgeMethodKind::Action,
        deterministic: false,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "navigate_screen",
        kind: BrowserHarnessBridgeMethodKind::Action,
        deterministic: false,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "open_settings_section",
        kind: BrowserHarnessBridgeMethodKind::Action,
        deterministic: false,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "snapshot",
        kind: BrowserHarnessBridgeMethodKind::ReadState,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    BrowserHarnessBridgeMethod {
        name: "ui_state",
        kind: BrowserHarnessBridgeMethodKind::ReadState,
        deterministic: true,
        returns_semantic_state: true,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "read_clipboard",
        kind: BrowserHarnessBridgeMethodKind::ReadState,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "submit_semantic_command",
        kind: BrowserHarnessBridgeMethodKind::Action,
        deterministic: false,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "get_authority_id",
        kind: BrowserHarnessBridgeMethodKind::ReadState,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "tail_log",
        kind: BrowserHarnessBridgeMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "root_structure",
        kind: BrowserHarnessBridgeMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    BrowserHarnessBridgeMethod {
        name: "inject_message",
        kind: BrowserHarnessBridgeMethodKind::Diagnostic,
        deterministic: false,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessObservationSurface {
    Browser,
    Tui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationMethodKind {
    SemanticState,
    RenderSignal,
    Clipboard,
    Diagnostic,
    Identity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationSurfaceMethod {
    pub name: &'static str,
    pub kind: ObservationMethodKind,
    pub deterministic: bool,
    pub returns_semantic_state: bool,
    pub returns_render_signal: bool,
}

pub const BROWSER_OBSERVATION_SURFACE_GLOBAL: &str = "__AURA_HARNESS_OBSERVE__";
pub const BROWSER_OBSERVATION_SURFACE_API_VERSION: u32 = 1;

pub const BROWSER_OBSERVATION_SURFACE_METHODS: &[ObservationSurfaceMethod] = &[
    ObservationSurfaceMethod {
        name: "snapshot",
        kind: ObservationMethodKind::RenderSignal,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    ObservationSurfaceMethod {
        name: "ui_state",
        kind: ObservationMethodKind::SemanticState,
        deterministic: true,
        returns_semantic_state: true,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "render_heartbeat",
        kind: ObservationMethodKind::RenderSignal,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    ObservationSurfaceMethod {
        name: "read_clipboard",
        kind: ObservationMethodKind::Clipboard,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "get_authority_id",
        kind: ObservationMethodKind::Identity,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "tail_log",
        kind: ObservationMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "root_structure",
        kind: ObservationMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
];

pub const TUI_OBSERVATION_SURFACE_API_VERSION: u32 = 1;

pub const TUI_OBSERVATION_SURFACE_METHODS: &[ObservationSurfaceMethod] = &[
    ObservationSurfaceMethod {
        name: "snapshot",
        kind: ObservationMethodKind::RenderSignal,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    ObservationSurfaceMethod {
        name: "snapshot_dom",
        kind: ObservationMethodKind::RenderSignal,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    ObservationSurfaceMethod {
        name: "ui_snapshot",
        kind: ObservationMethodKind::SemanticState,
        deterministic: true,
        returns_semantic_state: true,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "wait_for_ui_snapshot_event",
        kind: ObservationMethodKind::SemanticState,
        deterministic: true,
        returns_semantic_state: true,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "wait_for_dom_patterns",
        kind: ObservationMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    ObservationSurfaceMethod {
        name: "wait_for_target",
        kind: ObservationMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    ObservationSurfaceMethod {
        name: "tail_log",
        kind: ObservationMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "read_clipboard",
        kind: ObservationMethodKind::Clipboard,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessModeChangeKind {
    Observation,
    TimingDiscipline,
    RenderingStability,
    Instrumentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessModeAllowance {
    pub path: &'static str,
    pub kind: HarnessModeChangeKind,
    pub owner: &'static str,
    pub design_ref: &'static str,
}

pub const HARNESS_MODE_ALLOWLIST: &[HarnessModeAllowance] = &[
    HarnessModeAllowance {
        path: "crates/aura-app/src/workflows/runtime.rs",
        kind: HarnessModeChangeKind::TimingDiscipline,
        owner: "aura-app-runtime",
        design_ref: "docs/804_testing_guide.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-app/src/workflows/invitation.rs",
        kind: HarnessModeChangeKind::Instrumentation,
        owner: "aura-app-invitation",
        design_ref: "docs/804_testing_guide.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-agent/src/handlers/invitation.rs",
        kind: HarnessModeChangeKind::Instrumentation,
        owner: "aura-agent-invitation",
        design_ref: "docs/804_testing_guide.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-agent/src/runtime/effects.rs",
        kind: HarnessModeChangeKind::Instrumentation,
        owner: "aura-agent-runtime-effects",
        design_ref: "docs/804_testing_guide.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-agent/src/runtime_bridge/mod.rs",
        kind: HarnessModeChangeKind::Instrumentation,
        owner: "aura-agent-runtime-bridge",
        design_ref: "docs/804_testing_guide.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-terminal/src/tui/context/io_context.rs",
        kind: HarnessModeChangeKind::Instrumentation,
        owner: "aura-terminal-tui-context",
        design_ref: "crates/aura-terminal/ARCHITECTURE.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-web/src/main.rs",
        kind: HarnessModeChangeKind::Instrumentation,
        owner: "aura-web-main",
        design_ref: "docs/804_testing_guide.md",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendExecutionBoundaryKind {
    DriverBackend,
    ScenarioExecutor,
    ScenarioEntrypoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontendExecutionBoundary {
    pub path: &'static str,
    pub kind: FrontendExecutionBoundaryKind,
    pub owner: &'static str,
}

pub const FRONTEND_EXECUTION_BOUNDARIES: &[FrontendExecutionBoundary] = &[
    FrontendExecutionBoundary {
        path: "crates/aura-harness/src/backend/local_pty.rs",
        kind: FrontendExecutionBoundaryKind::DriverBackend,
        owner: "aura-harness-backend-local-pty",
    },
    FrontendExecutionBoundary {
        path: "crates/aura-harness/src/backend/playwright_browser.rs",
        kind: FrontendExecutionBoundaryKind::DriverBackend,
        owner: "aura-harness-backend-playwright",
    },
    FrontendExecutionBoundary {
        path: "crates/aura-harness/src/executor.rs",
        kind: FrontendExecutionBoundaryKind::ScenarioExecutor,
        owner: "aura-harness-executor",
    },
    FrontendExecutionBoundary {
        path: "scripts/harness/run-matrix.sh",
        kind: FrontendExecutionBoundaryKind::ScenarioEntrypoint,
        owner: "aura-harness-matrix",
    },
    FrontendExecutionBoundary {
        path: ".github/workflows/harness.yml",
        kind: FrontendExecutionBoundaryKind::ScenarioEntrypoint,
        owner: "aura-harness-ci",
    },
];

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
];

/// Keep `docs/997_flow_coverage.md` aligned with this canonical shared-flow mapping.
// Coverage metadata stays co-located with the shared flow contract so CI can
// ratchet flow-relevant source changes against reviewed scenario coverage.
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
        scenario_id: "shared-notifications-and-authority",
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
        path: "crates/aura-ui/src/app.rs",
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
        path: "crates/aura-ui/src/app.rs",
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
        path: "crates/aura-ui/src/app.rs",
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
        path: "crates/aura-ui/src/app.rs",
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
        path: "crates/aura-ui/src/app.rs",
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
        path: "crates/aura-ui/src/app.rs",
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
        path: "crates/aura-ui/src/app.rs",
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
        path: "crates/aura-ui/src/app.rs",
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
        path: "crates/aura-ui/src/app.rs",
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
        path: "crates/aura-ui/src/app.rs",
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
        path: "crates/aura-ui/src/app.rs",
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
        path: "crates/aura-ui/src/app.rs",
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
        path: "crates/aura-ui/src/app.rs",
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

type ParityListItemSignature = (String, bool, ConfirmationState);
type ParityListSignature = (ListId, Vec<ParityListItemSignature>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiSnapshot {
    pub screen: ScreenId,
    pub focused_control: Option<ControlId>,
    pub open_modal: Option<ModalId>,
    pub readiness: UiReadiness,
    pub revision: ProjectionRevision,
    pub quiescence: QuiescenceSnapshot,
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
            revision: ProjectionRevision {
                semantic_seq: 0,
                render_seq: None,
            },
            quiescence: QuiescenceSnapshot {
                state: QuiescenceState::Busy,
                reason_codes: vec!["readiness_loading".to_string()],
            },
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        }
    }

    pub fn validate_invariants(&self) -> Result<(), String> {
        let mut list_ids = HashSet::new();
        for list in &self.lists {
            if !list_ids.insert(list.id) {
                return Err(format!("duplicate list snapshot for {:?}", list.id));
            }
            if list.items.iter().any(|item| item.id.trim().is_empty()) {
                return Err(format!("list {:?} contains empty item id", list.id));
            }
            if let Some(item) = list.items.iter().find(|item| {
                is_placeholder_semantic_id(&item.id)
                    || is_override_semantic_id(&item.id)
                    || is_row_index_semantic_id(&item.id)
            }) {
                return Err(format!(
                    "list {:?} contains placeholder, override, or row-index item id {}",
                    list.id, item.id
                ));
            }
            if list.items.iter().filter(|item| item.selected).count() > 1 {
                return Err(format!(
                    "list {:?} exported multiple selected items",
                    list.id
                ));
            }
        }

        for selection in &self.selections {
            let Some(list) = self.lists.iter().find(|list| list.id == selection.list) else {
                return Err(format!(
                    "selection for {:?} has no corresponding list export",
                    selection.list
                ));
            };
            if !list.items.iter().any(|item| item.id == selection.item_id) {
                return Err(format!(
                    "selection for {:?} references missing item {}",
                    selection.list, selection.item_id
                ));
            }
            if is_placeholder_semantic_id(&selection.item_id)
                || is_override_semantic_id(&selection.item_id)
                || is_row_index_semantic_id(&selection.item_id)
            {
                return Err(format!(
                    "selection for {:?} references placeholder, override, or row-index item {}",
                    selection.list, selection.item_id
                ));
            }
        }

        if let Some(ControlId::Modal(modal)) = self.focused_control {
            if self.open_modal != Some(modal) {
                return Err(format!(
                    "focused modal {:?} does not match open modal {:?}",
                    modal, self.open_modal
                ));
            }
        }
        if let Some(ControlId::Screen(focused_screen)) = self.focused_control {
            if focused_screen != self.screen {
                return Err(format!(
                    "focused screen {:?} does not match current screen {:?}",
                    focused_screen, self.screen
                ));
            }
        }
        if self.open_modal.is_some() && matches!(self.focused_control, Some(ControlId::Screen(_))) {
            return Err("modal cannot be open while focus remains on a screen root".to_string());
        }
        if let Some(event) = self.runtime_events.iter().find(|event| {
            event.id.0.starts_with("inferred:") || event.id.0.starts_with("synthetic:")
        }) {
            return Err(format!(
                "runtime event {} uses inferred/synthetic success id",
                event.id.0
            ));
        }

        Ok(())
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

fn parity_list_signature(snapshot: &UiSnapshot) -> Vec<ParityListSignature> {
    let relevant_lists = parity_relevant_lists(snapshot.screen);
    let mut lists = snapshot
        .lists
        .iter()
        .filter(|list| relevant_lists.contains(&list.id))
        .map(|list| {
            let mut items = list
                .items
                .iter()
                .filter(|item| {
                    !(snapshot.screen == ScreenId::Settings
                        && list.id == ListId::SettingsSections
                        && !matches!(
                            classify_settings_section_item_id(&item.id),
                            Some(SettingsSectionSurfaceId::Shared(_))
                        ))
                })
                .map(|item| (item.id.clone(), item.selected, item.confirmation))
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
        .filter(|selection| {
            !(snapshot.screen == ScreenId::Settings
                && selection.list == ListId::SettingsSections
                && !matches!(
                    classify_settings_section_item_id(&selection.item_id),
                    Some(SettingsSectionSurfaceId::Shared(_))
                ))
        })
        .map(|selection| (selection.list, selection.item_id.clone()))
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

fn parity_toast_signature(snapshot: &UiSnapshot) -> Vec<(ToastKind, String)> {
    let mut toasts = snapshot
        .toasts
        .iter()
        .map(|toast| (toast.kind, toast.message.clone()))
        .collect::<Vec<_>>();
    toasts.sort_by(|left, right| {
        format!("{:?}", left.0)
            .cmp(&format!("{:?}", right.0))
            .then_with(|| left.1.cmp(&right.1))
    });
    toasts
}

fn parity_runtime_event_signature(snapshot: &UiSnapshot) -> Vec<(RuntimeEventKind, String)> {
    let mut events = snapshot
        .runtime_events
        .iter()
        .map(|event| (event.kind(), event.key()))
        .collect::<Vec<_>>();
    events.sort_by(|left, right| {
        format!("{:?}", left.0)
            .cmp(&format!("{:?}", right.0))
            .then_with(|| left.1.cmp(&right.1))
    });
    events
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
    if web.focused_control != tui.focused_control {
        mismatches.push(UiParityMismatch {
            field: "focused_control",
            web: format!("{:?}", web.focused_control),
            tui: format!("{:?}", tui.focused_control),
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

    let web_toasts = parity_toast_signature(web);
    let tui_toasts = parity_toast_signature(tui);
    if web_toasts != tui_toasts {
        mismatches.push(UiParityMismatch {
            field: "toasts",
            web: format!("{web_toasts:?}"),
            tui: format!("{tui_toasts:?}"),
        });
    }

    let web_runtime_events = parity_runtime_event_signature(web);
    let tui_runtime_events = parity_runtime_event_signature(tui);
    if web_runtime_events != tui_runtime_events {
        mismatches.push(UiParityMismatch {
            field: "runtime_events",
            web: format!("{web_runtime_events:?}"),
            tui: format!("{tui_runtime_events:?}"),
        });
    }

    mismatches
}

fn parity_mismatch_is_covered_by_exception(
    web: &UiSnapshot,
    tui: &UiSnapshot,
    mismatch: &UiParityMismatch,
) -> bool {
    matches!(
        (
            mismatch.field,
            web.screen,
            tui.screen,
            web.focused_control,
            tui.focused_control
        ),
        (
            "focused_control",
            ScreenId::Settings,
            ScreenId::Settings,
            Some(ControlId::SettingsToggleThemeButton),
            _
        ) | (
            "focused_control",
            ScreenId::Settings,
            ScreenId::Settings,
            _,
            Some(ControlId::SettingsToggleThemeButton)
        )
    )
}

#[must_use]
pub fn uncovered_ui_parity_mismatches(web: &UiSnapshot, tui: &UiSnapshot) -> Vec<UiParityMismatch> {
    compare_ui_snapshots_for_parity(web, tui)
        .into_iter()
        .filter(|mismatch| !parity_mismatch_is_covered_by_exception(web, tui, mismatch))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::next_projection_revision;
    use super::{
        classify_screen_item_id, classify_semantic_settings_section_item_id,
        classify_settings_section_item_id, compare_ui_snapshots_for_parity, list_item_dom_id,
        list_item_selector, nav_control_id_for_screen, screen_item_id,
        semantic_settings_section_item_id, semantic_settings_section_surface_id,
        settings_section_item_id, shared_flow_scenarios, shared_flow_source_areas,
        shared_list_support, shared_modal_support, shared_screen_module_map, shared_screen_support,
        uncovered_ui_parity_mismatches, validate_harness_shell_structure,
        validate_render_convergence, AuthoritativeSemanticFact, AuthoritativeSemanticFactKind,
        BrowserHarnessBridgeMethodKind, ChannelFactKey, ConfirmationState, ControlId, FieldId,
        FlowAvailability, FrontendExecutionBoundaryKind, FrontendSpecificSettingsSectionId,
        HarnessModeChangeKind, HarnessShellMode, HarnessShellStructureSnapshot, ListId,
        ListItemSnapshot, ListSnapshot, MessageSnapshot, ModalId, OperationId, OperationInstanceId,
        OperationSnapshot, OperationState, ParityUiIdentity, ProjectionRevision,
        QuiescenceSnapshot, RenderHeartbeat, RuntimeEventId, RuntimeEventKind,
        RuntimeEventSnapshot, RuntimeFact, ScreenId, SelectionSnapshot, SemanticFailureCode,
        SemanticFailureDomain, SemanticOperationCausality, SemanticOperationError,
        SemanticOperationKind, SemanticOperationPhase, SemanticOperationStatus,
        SettingsSectionSurfaceId, SharedSettingsSectionId, ToastId, ToastKind, ToastSnapshot,
        UiParityMismatch, UiReadiness, UiSnapshot, ALL_SHARED_FLOW_IDS, BROWSER_CACHE_BOUNDARIES,
        BROWSER_HARNESS_BRIDGE_METHODS, BROWSER_OBSERVATION_SURFACE_GLOBAL,
        BROWSER_OBSERVATION_SURFACE_METHODS, FRONTEND_EXECUTION_BOUNDARIES,
        FRONTEND_SPECIFIC_SETTINGS_SECTIONS, HARNESS_MODE_ALLOWLIST,
        PARITY_CRITICAL_SETTINGS_SECTIONS, PARITY_EXCEPTION_METADATA,
        SHARED_FLOW_SCENARIO_COVERAGE, SHARED_FLOW_SOURCE_AREAS, SHARED_FLOW_SUPPORT,
        SHARED_LIST_SUPPORT, SHARED_MODAL_SUPPORT, SHARED_SCREEN_MODULE_MAP, SHARED_SCREEN_SUPPORT,
        TUI_OBSERVATION_SURFACE_METHODS,
    };
    use aura_core::{OwnerEpoch, PublicationSequence};
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
    fn snapshot_invariants_reject_placeholder_ids() {
        let snapshot = UiSnapshot {
            screen: ScreenId::Chat,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: next_projection_revision(Some(1)),
            quiescence: QuiescenceSnapshot::settled(),
            selections: vec![SelectionSnapshot {
                list: ListId::Channels,
                item_id: "channel:0000000000000000".to_string(),
            }],
            lists: vec![ListSnapshot {
                id: ListId::Channels,
                items: vec![ListItemSnapshot {
                    id: "channel:0000000000000000".to_string(),
                    selected: true,
                    confirmation: ConfirmationState::Confirmed,
                    is_current: false,
                }],
            }],
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };

        assert!(snapshot
            .validate_invariants()
            .expect_err("placeholder ids must be rejected")
            .contains("placeholder, override, or row-index item id"));
    }

    #[test]
    fn snapshot_invariants_reject_override_backed_ids() {
        let snapshot = UiSnapshot {
            screen: ScreenId::Chat,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: next_projection_revision(Some(4)),
            quiescence: QuiescenceSnapshot::settled(),
            selections: vec![SelectionSnapshot {
                list: ListId::Channels,
                item_id: "override:channel-list".to_string(),
            }],
            lists: vec![ListSnapshot {
                id: ListId::Channels,
                items: vec![ListItemSnapshot {
                    id: "override:channel-list".to_string(),
                    selected: true,
                    confirmation: ConfirmationState::Confirmed,
                    is_current: false,
                }],
            }],
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };

        assert!(snapshot
            .validate_invariants()
            .expect_err("override-backed ids must be rejected")
            .contains("placeholder, override, or row-index"));
    }

    #[test]
    fn snapshot_invariants_reject_row_index_ids() {
        let snapshot = UiSnapshot {
            screen: ScreenId::Contacts,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: next_projection_revision(Some(7)),
            quiescence: QuiescenceSnapshot::settled(),
            selections: vec![SelectionSnapshot {
                list: ListId::Contacts,
                item_id: "row-2".to_string(),
            }],
            lists: vec![ListSnapshot {
                id: ListId::Contacts,
                items: vec![ListItemSnapshot {
                    id: "row-2".to_string(),
                    selected: true,
                    confirmation: ConfirmationState::Confirmed,
                    is_current: false,
                }],
            }],
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };

        assert!(snapshot
            .validate_invariants()
            .expect_err("row-index ids must be rejected")
            .contains("row-index item"));
    }

    #[test]
    fn snapshot_invariants_reject_inferred_runtime_events() {
        let snapshot = UiSnapshot {
            screen: ScreenId::Contacts,
            focused_control: None,
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: next_projection_revision(Some(2)),
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: vec![RuntimeEventSnapshot {
                id: RuntimeEventId("inferred:contact_link_ready".to_string()),
                fact: RuntimeFact::ContactLinkReady {
                    authority_id: Some("alice".to_string()),
                    contact_count: Some(1),
                },
            }],
        };

        assert!(snapshot
            .validate_invariants()
            .expect_err("inferred runtime events must be rejected")
            .contains("inferred/synthetic"));
    }

    #[test]
    fn snapshot_invariants_reject_contradictory_focus_and_modal_state() {
        let snapshot = UiSnapshot {
            screen: ScreenId::Neighborhood,
            focused_control: Some(ControlId::Screen(ScreenId::Chat)),
            open_modal: Some(ModalId::CreateHome),
            readiness: UiReadiness::Ready,
            revision: next_projection_revision(Some(3)),
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };

        let error = snapshot
            .validate_invariants()
            .expect_err("contradictory focus must be rejected");
        assert!(
            error.contains("focused screen") || error.contains("modal cannot be open"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn semantic_operation_status_round_trips_through_json() {
        let status = SemanticOperationStatus::failed(
            SemanticOperationKind::SendChatMessage,
            SemanticOperationError::new(
                SemanticFailureDomain::Delivery,
                SemanticFailureCode::DeliveryReadinessNotReached,
            )
            .with_detail("peer channel never became ready"),
        );

        let encoded =
            serde_json::to_string(&status).unwrap_or_else(|error| panic!("serialize: {error}"));
        let decoded: SemanticOperationStatus =
            serde_json::from_str(&encoded).unwrap_or_else(|error| panic!("deserialize: {error}"));

        assert_eq!(decoded, status);
        assert!(decoded.phase.is_terminal());
    }

    #[test]
    fn semantic_operation_status_supports_non_terminal_progress() {
        let status = SemanticOperationStatus::new(
            SemanticOperationKind::InviteActorToChannel,
            SemanticOperationPhase::AuthoritativeContextReady,
        );

        assert_eq!(status.error, None);
        assert!(!status.phase.is_terminal());
    }

    #[test]
    fn semantic_operation_status_supports_cancelled_terminality() {
        let status = SemanticOperationStatus::cancelled(SemanticOperationKind::CreateAccount);

        assert_eq!(status.error, None);
        assert!(status.phase.is_terminal());
        assert_eq!(status.phase, SemanticOperationPhase::Cancelled);
    }

    #[test]
    fn semantic_operation_phase_generated_lifecycle_rejects_terminal_regression() {
        assert!(SemanticOperationPhase::Submitted
            .can_transition_to(SemanticOperationPhase::WorkflowDispatched));
        assert!(SemanticOperationPhase::DeliveryReady
            .can_transition_to(SemanticOperationPhase::Succeeded));
        assert!(
            !SemanticOperationPhase::Succeeded.can_transition_to(SemanticOperationPhase::Failed)
        );
    }

    #[test]
    fn operation_state_generated_lifecycle_requires_new_instance_after_terminal() {
        assert!(OperationState::Idle.can_transition_to(OperationState::Submitting));
        assert!(OperationState::Submitting.can_transition_to(OperationState::Succeeded));
        assert!(!OperationState::Succeeded.can_transition_to(OperationState::Submitting));
    }

    #[test]
    fn authoritative_semantic_fact_round_trips_through_json() {
        let fact = AuthoritativeSemanticFact::OperationStatus {
            operation_id: OperationId::send_message(),
            instance_id: Some(OperationInstanceId("semantic-op-1".to_string())),
            causality: Some(SemanticOperationCausality::new(
                OwnerEpoch::new(3),
                PublicationSequence::new(9),
            )),
            status: SemanticOperationStatus::new(
                SemanticOperationKind::SendChatMessage,
                SemanticOperationPhase::PeerChannelReady,
            ),
        };

        let encoded =
            serde_json::to_string(&fact).unwrap_or_else(|error| panic!("serialize: {error}"));
        let decoded: AuthoritativeSemanticFact =
            serde_json::from_str(&encoded).unwrap_or_else(|error| panic!("deserialize: {error}"));

        assert_eq!(decoded, fact);
        assert_eq!(
            decoded.kind(),
            AuthoritativeSemanticFactKind::OperationStatus
        );
    }

    #[test]
    fn authoritative_semantic_fact_distinguishes_peer_channel_readiness() {
        let fact = AuthoritativeSemanticFact::PeerChannelReady {
            channel: ChannelFactKey::named("shared"),
            peer_authority_id: "authority:peer".to_string(),
            context_id: Some("context:test".to_string()),
        };

        assert_eq!(fact.kind(), AuthoritativeSemanticFactKind::PeerChannelReady);
    }

    #[test]
    fn authoritative_semantic_fact_runtime_fact_bridge_maps_delivery_readiness() {
        let fact = AuthoritativeSemanticFact::MessageDeliveryReady {
            channel: ChannelFactKey::named("shared"),
            member_count: 2,
        };

        assert_eq!(
            fact.runtime_fact_bridge(),
            Some((
                RuntimeEventKind::MessageDeliveryReady,
                RuntimeFact::MessageDeliveryReady {
                    channel: ChannelFactKey::named("shared"),
                    member_count: 2,
                },
            ))
        );
    }

    #[test]
    fn authoritative_semantic_fact_operation_status_bridge_extracts_status() {
        let fact = AuthoritativeSemanticFact::OperationStatus {
            operation_id: OperationId::invitation_accept(),
            instance_id: Some(OperationInstanceId("accept-1".to_string())),
            causality: Some(SemanticOperationCausality::new(
                OwnerEpoch::new(1),
                PublicationSequence::new(4),
            )),
            status: SemanticOperationStatus::new(
                SemanticOperationKind::AcceptContactInvitation,
                SemanticOperationPhase::Succeeded,
            ),
        };

        assert_eq!(
            fact.operation_status_bridge(),
            Some((
                OperationId::invitation_accept(),
                Some(OperationInstanceId("accept-1".to_string())),
                Some(SemanticOperationCausality::new(
                    OwnerEpoch::new(1),
                    PublicationSequence::new(4),
                )),
                SemanticOperationStatus::new(
                    SemanticOperationKind::AcceptContactInvitation,
                    SemanticOperationPhase::Succeeded,
                ),
            ))
        );
    }

    #[test]
    fn authoritative_operation_status_key_replaces_prior_kind_for_same_operation() {
        let contact_invite = AuthoritativeSemanticFact::OperationStatus {
            operation_id: OperationId::invitation_create(),
            instance_id: None,
            causality: None,
            status: SemanticOperationStatus::new(
                SemanticOperationKind::CreateContactInvitation,
                SemanticOperationPhase::Succeeded,
            ),
        };
        let channel_invite = AuthoritativeSemanticFact::OperationStatus {
            operation_id: OperationId::invitation_create(),
            instance_id: None,
            causality: None,
            status: SemanticOperationStatus::new(
                SemanticOperationKind::InviteActorToChannel,
                SemanticOperationPhase::WorkflowDispatched,
            ),
        };

        assert_eq!(contact_invite.key(), channel_invite.key());
    }

    #[test]
    fn bridged_operation_statuses_finalize_contact_accept_on_contact_link_ready() {
        let statuses = super::bridged_operation_statuses(&[
            AuthoritativeSemanticFact::OperationStatus {
                operation_id: OperationId::invitation_accept(),
                instance_id: None,
                causality: None,
                status: SemanticOperationStatus::new(
                    SemanticOperationKind::AcceptContactInvitation,
                    SemanticOperationPhase::WorkflowDispatched,
                ),
            },
            AuthoritativeSemanticFact::ContactLinkReady {
                authority_id: "authority:test".to_string(),
                contact_count: 1,
            },
        ]);

        assert_eq!(
            statuses,
            vec![(
                OperationId::invitation_accept(),
                None,
                None,
                SemanticOperationStatus::new(
                    SemanticOperationKind::AcceptContactInvitation,
                    SemanticOperationPhase::Succeeded,
                ),
            )]
        );
    }

    #[test]
    fn browser_cache_boundaries_are_declared() {
        assert_eq!(BROWSER_CACHE_BOUNDARIES.len(), 5);
        assert!(BROWSER_CACHE_BOUNDARIES
            .iter()
            .any(|boundary| boundary.reason_code == "authority_switch"));
        assert!(BROWSER_CACHE_BOUNDARIES
            .iter()
            .any(|boundary| boundary.reason_code == "device_import"));
    }

    #[test]
    fn projection_revision_detects_stale_snapshots_by_revision() {
        let baseline = next_projection_revision(Some(10));
        let newer = next_projection_revision(Some(11));
        let render_only_newer = super::ProjectionRevision {
            semantic_seq: newer.semantic_seq,
            render_seq: Some(12),
        };

        assert!(newer.has_sequence_metadata());
        assert!(newer.is_newer_than(baseline));
        assert!(baseline.is_stale_against(newer));
        assert!(render_only_newer.is_newer_than(newer));
    }

    #[test]
    fn onboarding_is_declared_in_the_shared_snapshot_model() {
        assert_eq!(ScreenId::Onboarding.help_label(), "Onboarding");
        assert_eq!(
            shared_screen_support(ScreenId::Onboarding).web,
            FlowAvailability::Supported
        );
        assert_eq!(
            shared_screen_support(ScreenId::Onboarding).tui,
            FlowAvailability::Supported
        );
        assert_eq!(
            UiSnapshot::loading(ScreenId::Onboarding).screen,
            ScreenId::Onboarding
        );
    }

    #[test]
    fn onboarding_uses_canonical_snapshot_publication_path() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let web_main_path = repo_root.join("crates/aura-web/src/main.rs");
        let web_main = std::fs::read_to_string(&web_main_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", web_main_path.display()));

        assert!(
            !web_main.contains("publish_onboarding_snapshot"),
            "web onboarding must not use a bespoke snapshot publication path"
        );
        assert!(
            web_main.contains("controller.set_account_setup_state("),
            "web onboarding must publish through the canonical controller snapshot pipeline"
        );
    }

    #[test]
    fn onboarding_harness_paths_have_no_bespoke_recovery_logic() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let harness_bridge_path = repo_root.join("crates/aura-web/src/harness_bridge.rs");
        let driver_path =
            repo_root.join("crates/aura-harness/playwright-driver/playwright_driver.mjs");
        let local_pty_path = repo_root.join("crates/aura-harness/src/backend/local_pty.rs");

        let harness_bridge =
            std::fs::read_to_string(&harness_bridge_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", harness_bridge_path.display())
            });
        let driver = std::fs::read_to_string(&driver_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", driver_path.display()));
        let local_pty = std::fs::read_to_string(&local_pty_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", local_pty_path.display()));

        assert!(
            !harness_bridge.contains("stale_onboarding_publish"),
            "browser harness bridge must not repair stale onboarding publications"
        );
        assert!(
            !driver.contains("staleOnboardingCache") && !driver.contains("stale_onboarding_"),
            "playwright driver must not carry stale-onboarding recovery heuristics"
        );
        assert!(
            !local_pty.contains("synthetic_onboarding_snapshot"),
            "local PTY backend must not fabricate onboarding snapshots"
        );
    }

    #[test]
    fn shared_flow_source_area_metadata_points_to_existing_paths() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let source_area_unique: HashSet<_> = SHARED_FLOW_SOURCE_AREAS
            .iter()
            .map(|area| (area.flow, area.path))
            .collect();
        assert_eq!(source_area_unique.len(), SHARED_FLOW_SOURCE_AREAS.len());

        for flow in ALL_SHARED_FLOW_IDS {
            assert!(
                !shared_flow_source_areas(*flow).is_empty(),
                "shared flow {flow:?} must declare at least one owned source area"
            );
        }

        for area in SHARED_FLOW_SOURCE_AREAS {
            let path = workspace_root.join(area.path);
            assert!(
                path.exists(),
                "shared flow {:?} source area path does not exist: {}",
                area.flow,
                path.display()
            );
        }
    }

    #[test]
    fn browser_harness_bridge_contract_is_versioned_and_complete() {
        let names = BROWSER_HARNESS_BRIDGE_METHODS
            .iter()
            .map(|method| method.name)
            .collect::<HashSet<_>>();
        assert_eq!(names.len(), BROWSER_HARNESS_BRIDGE_METHODS.len());
        for required in [
            "send_keys",
            "send_key",
            "navigate_screen",
            "snapshot",
            "ui_state",
            "read_clipboard",
            "submit_semantic_command",
            "get_authority_id",
            "tail_log",
            "root_structure",
            "inject_message",
        ] {
            assert!(names.contains(required), "missing bridge method {required}");
        }
    }

    #[test]
    fn browser_harness_bridge_read_methods_are_declared_deterministic() {
        for method in BROWSER_HARNESS_BRIDGE_METHODS {
            if method.returns_semantic_state || method.returns_render_signal {
                assert!(
                    method.deterministic,
                    "bridge method {} must be deterministic for semantic or render observation",
                    method.name
                );
                assert_ne!(method.kind, BrowserHarnessBridgeMethodKind::Action);
            }
        }
    }

    #[test]
    fn browser_observation_surface_contract_is_versioned_and_read_only() {
        assert_eq!(
            BROWSER_OBSERVATION_SURFACE_GLOBAL,
            "__AURA_HARNESS_OBSERVE__"
        );
        let names = BROWSER_OBSERVATION_SURFACE_METHODS
            .iter()
            .map(|method| method.name)
            .collect::<HashSet<_>>();
        assert_eq!(names.len(), BROWSER_OBSERVATION_SURFACE_METHODS.len());
        for required in [
            "snapshot",
            "ui_state",
            "render_heartbeat",
            "read_clipboard",
            "get_authority_id",
            "tail_log",
            "root_structure",
        ] {
            assert!(
                names.contains(required),
                "missing observation method {required}"
            );
        }
        for method in BROWSER_OBSERVATION_SURFACE_METHODS {
            assert!(
                method.deterministic,
                "observation method {} must be deterministic",
                method.name
            );
        }
    }

    #[test]
    fn tui_observation_surface_contract_is_versioned_and_read_only() {
        let names = TUI_OBSERVATION_SURFACE_METHODS
            .iter()
            .map(|method| method.name)
            .collect::<HashSet<_>>();
        assert_eq!(names.len(), TUI_OBSERVATION_SURFACE_METHODS.len());
        for required in [
            "snapshot",
            "snapshot_dom",
            "ui_snapshot",
            "wait_for_ui_snapshot_event",
            "wait_for_dom_patterns",
            "wait_for_target",
            "tail_log",
            "read_clipboard",
        ] {
            assert!(
                names.contains(required),
                "missing TUI observation method {required}"
            );
        }
        for method in TUI_OBSERVATION_SURFACE_METHODS {
            assert!(
                method.deterministic,
                "TUI observation method {} must be deterministic",
                method.name
            );
        }
    }

    #[test]
    fn observation_surface_methods_do_not_overlap_action_surface() {
        let action_methods = BROWSER_HARNESS_BRIDGE_METHODS
            .iter()
            .filter(|method| method.kind == BrowserHarnessBridgeMethodKind::Action)
            .map(|method| method.name)
            .collect::<HashSet<_>>();
        for method in BROWSER_OBSERVATION_SURFACE_METHODS {
            assert!(
                !action_methods.contains(method.name),
                "browser observation method {} must not also be exported on the action surface",
                method.name
            );
        }
    }

    #[test]
    fn harness_mode_allowlist_is_scoped_to_non_semantic_categories() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        assert!(!HARNESS_MODE_ALLOWLIST.is_empty());
        for entry in HARNESS_MODE_ALLOWLIST {
            assert!(
                workspace_root.join(entry.path).exists(),
                "missing harness-mode allowlist path {}",
                entry.path
            );
            assert!(
                workspace_root.join(entry.design_ref).exists(),
                "missing harness-mode design reference {}",
                entry.design_ref
            );
            assert!(
                matches!(
                    entry.kind,
                    HarnessModeChangeKind::Observation
                        | HarnessModeChangeKind::TimingDiscipline
                        | HarnessModeChangeKind::RenderingStability
                        | HarnessModeChangeKind::Instrumentation
                ),
                "invalid harness-mode allowlist kind for {}",
                entry.path
            );
            assert!(
                !entry.owner.trim().is_empty(),
                "missing harness-mode owner for {}",
                entry.path
            );
        }
    }

    #[test]
    fn frontend_execution_boundaries_are_defined_and_exist() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        assert!(!FRONTEND_EXECUTION_BOUNDARIES.is_empty());
        assert!(FRONTEND_EXECUTION_BOUNDARIES
            .iter()
            .any(|entry| { entry.kind == FrontendExecutionBoundaryKind::DriverBackend }));
        assert!(FRONTEND_EXECUTION_BOUNDARIES
            .iter()
            .any(|entry| { entry.kind == FrontendExecutionBoundaryKind::ScenarioExecutor }));
        assert!(FRONTEND_EXECUTION_BOUNDARIES
            .iter()
            .any(|entry| { entry.kind == FrontendExecutionBoundaryKind::ScenarioEntrypoint }));
        for entry in FRONTEND_EXECUTION_BOUNDARIES {
            assert!(
                workspace_root.join(entry.path).exists(),
                "missing frontend execution boundary path {}",
                entry.path
            );
            assert!(
                !entry.owner.trim().is_empty(),
                "missing frontend execution boundary owner for {}",
                entry.path
            );
        }
    }

    #[test]
    fn parity_ui_identity_helpers_match_contract_ids() {
        assert_eq!(
            ParityUiIdentity::control_dom_id(ControlId::AppRoot),
            ControlId::AppRoot.web_dom_id()
        );
        assert_eq!(
            ParityUiIdentity::field_dom_id(FieldId::InvitationCode),
            FieldId::InvitationCode.web_dom_id()
        );
        assert_eq!(
            ParityUiIdentity::list_dom_id(ListId::Contacts),
            ListId::Contacts.web_dom_id()
        );
        assert_eq!(
            ParityUiIdentity::modal_dom_id(ModalId::CreateInvitation),
            ModalId::CreateInvitation.web_dom_id()
        );
        assert_eq!(
            ParityUiIdentity::list_item_dom_id(ListId::Contacts, "authority:abc/DEF"),
            list_item_dom_id(ListId::Contacts, "authority:abc/DEF")
        );
    }

    #[test]
    fn frontend_sources_reference_shared_identity_helpers() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let web_source =
            std::fs::read_to_string(workspace_root.join("crates/aura-web/src/harness_bridge.rs"))
                .unwrap_or_else(|error| panic!("failed to read harness bridge source: {error}"));
        let ui_source = std::fs::read_to_string(workspace_root.join("crates/aura-ui/src/app.rs"))
            .unwrap_or_else(|error| panic!("failed to read aura-ui source: {error}"));

        assert!(
            web_source.contains(".web_dom_id()"),
            "web harness bridge must reference shared contract DOM id helpers"
        );
        assert!(
            ui_source.contains("list_item_dom_id(") && ui_source.contains(".web_dom_id()"),
            "aura-ui must reference shared identity helpers for parity-critical ids"
        );
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
    fn render_convergence_accepts_matching_snapshot_and_heartbeat() {
        let snapshot = UiSnapshot {
            screen: ScreenId::Chat,
            focused_control: Some(ControlId::Screen(ScreenId::Chat)),
            open_modal: Some(ModalId::AcceptInvitation),
            readiness: UiReadiness::Ready,
            revision: ProjectionRevision {
                semantic_seq: 1,
                render_seq: Some(7),
            },
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };
        let heartbeat = RenderHeartbeat {
            screen: ScreenId::Chat,
            open_modal: Some(ModalId::AcceptInvitation),
            render_seq: 7,
        };

        validate_render_convergence(&snapshot, &heartbeat)
            .unwrap_or_else(|error| panic!("render convergence should hold: {error}"));
    }

    #[test]
    fn render_convergence_rejects_semantic_state_published_ahead_of_renderer() {
        let snapshot = UiSnapshot {
            screen: ScreenId::Settings,
            focused_control: Some(ControlId::Screen(ScreenId::Settings)),
            open_modal: Some(ModalId::CreateInvitation),
            readiness: UiReadiness::Ready,
            revision: ProjectionRevision {
                semantic_seq: 2,
                render_seq: Some(9),
            },
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };
        let heartbeat = RenderHeartbeat {
            screen: ScreenId::Settings,
            open_modal: None,
            render_seq: 8,
        };

        let error = validate_render_convergence(&snapshot, &heartbeat)
            .expect_err("semantic state ahead of renderer must fail");
        assert!(error.contains("ahead of renderer") || error.contains("diverges from renderer"));
    }

    #[test]
    fn harness_shell_structure_accepts_exactly_one_app_shell() {
        let shell = HarnessShellStructureSnapshot {
            screen: ScreenId::Chat,
            app_root_count: 1,
            modal_region_count: 1,
            onboarding_root_count: 0,
            toast_region_count: 1,
            active_screen_root_count: 1,
        };

        assert_eq!(
            validate_harness_shell_structure(&shell).expect("single app shell should be valid"),
            HarnessShellMode::App
        );
    }

    #[test]
    fn harness_shell_structure_accepts_single_onboarding_shell() {
        let shell = HarnessShellStructureSnapshot {
            screen: ScreenId::Onboarding,
            app_root_count: 0,
            modal_region_count: 0,
            onboarding_root_count: 1,
            toast_region_count: 0,
            active_screen_root_count: 0,
        };

        assert_eq!(
            validate_harness_shell_structure(&shell)
                .expect("single onboarding shell should be valid"),
            HarnessShellMode::Onboarding
        );
    }

    #[test]
    fn harness_shell_structure_rejects_duplicate_or_ambiguous_roots() {
        let shell = HarnessShellStructureSnapshot {
            screen: ScreenId::Settings,
            app_root_count: 2,
            modal_region_count: 1,
            onboarding_root_count: 0,
            toast_region_count: 1,
            active_screen_root_count: 1,
        };

        let error = validate_harness_shell_structure(&shell)
            .expect_err("duplicate app roots must fail the shell contract");
        assert!(error.contains("invalid harness shell structure"));
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

        let unique_exception_metadata: HashSet<_> = PARITY_EXCEPTION_METADATA
            .iter()
            .map(|metadata| metadata.exception)
            .collect();
        assert_eq!(
            unique_exception_metadata.len(),
            PARITY_EXCEPTION_METADATA.len(),
            "parity exception metadata must stay unique"
        );
    }

    #[test]
    fn shared_settings_section_surface_is_explicit() {
        assert_eq!(
            PARITY_CRITICAL_SETTINGS_SECTIONS,
            &[
                SharedSettingsSectionId::Profile,
                SharedSettingsSectionId::GuardianThreshold,
                SharedSettingsSectionId::RequestRecovery,
                SharedSettingsSectionId::Devices,
                SharedSettingsSectionId::Authority,
            ]
        );
        assert_eq!(
            FRONTEND_SPECIFIC_SETTINGS_SECTIONS,
            &[
                FrontendSpecificSettingsSectionId::Appearance,
                FrontendSpecificSettingsSectionId::Info,
                FrontendSpecificSettingsSectionId::Observability,
            ]
        );

        for shared in PARITY_CRITICAL_SETTINGS_SECTIONS {
            let item_id = settings_section_item_id(SettingsSectionSurfaceId::Shared(*shared));
            assert_eq!(
                classify_settings_section_item_id(item_id),
                Some(SettingsSectionSurfaceId::Shared(*shared))
            );
        }

        for frontend_specific in FRONTEND_SPECIFIC_SETTINGS_SECTIONS {
            let item_id = settings_section_item_id(SettingsSectionSurfaceId::FrontendSpecific(
                *frontend_specific,
            ));
            assert_eq!(
                classify_settings_section_item_id(item_id),
                Some(SettingsSectionSurfaceId::FrontendSpecific(
                    *frontend_specific
                ))
            );
        }
    }

    #[test]
    fn screen_surface_ids_are_explicit() {
        for screen in [
            ScreenId::Onboarding,
            ScreenId::Neighborhood,
            ScreenId::Chat,
            ScreenId::Contacts,
            ScreenId::Notifications,
            ScreenId::Settings,
        ] {
            let item_id = screen_item_id(screen);
            assert_eq!(classify_screen_item_id(item_id), Some(screen));
        }

        assert_eq!(
            nav_control_id_for_screen(ScreenId::Onboarding),
            ControlId::OnboardingRoot
        );
        assert_eq!(
            nav_control_id_for_screen(ScreenId::Neighborhood),
            ControlId::NavNeighborhood
        );
        assert_eq!(
            nav_control_id_for_screen(ScreenId::Chat),
            ControlId::NavChat
        );
        assert_eq!(
            nav_control_id_for_screen(ScreenId::Contacts),
            ControlId::NavContacts
        );
        assert_eq!(
            nav_control_id_for_screen(ScreenId::Notifications),
            ControlId::NavNotifications
        );
        assert_eq!(
            nav_control_id_for_screen(ScreenId::Settings),
            ControlId::NavSettings
        );
    }

    #[test]
    fn semantic_settings_sections_use_shared_surface_ids() {
        let section = crate::scenario_contract::SettingsSection::Devices;
        assert_eq!(
            semantic_settings_section_surface_id(section),
            SettingsSectionSurfaceId::Shared(SharedSettingsSectionId::Devices)
        );
        assert_eq!(semantic_settings_section_item_id(section), "devices");
        assert_eq!(
            classify_semantic_settings_section_item_id("devices"),
            Some(section)
        );
        assert_eq!(
            classify_semantic_settings_section_item_id("appearance"),
            None
        );
    }

    #[test]
    fn frontend_settings_sources_use_shared_section_ids() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let web_model_path = repo_root.join("crates/aura-ui/src/model.rs");
        let tui_types_path = repo_root.join("crates/aura-terminal/src/tui/types.rs");
        let tui_export_path =
            repo_root.join("crates/aura-terminal/src/tui/harness_state/snapshot.rs");

        let web_model = std::fs::read_to_string(&web_model_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", web_model_path.display()));
        let tui_types = std::fs::read_to_string(&tui_types_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", tui_types_path.display()));
        let tui_export = std::fs::read_to_string(&tui_export_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", tui_export_path.display())
        });

        assert!(
            web_model.contains("settings_section_item_id("),
            "web settings must use shared settings_section_item_id helper"
        );
        assert!(
            web_model.contains("SharedSettingsSectionId::GuardianThreshold")
                && web_model.contains("SharedSettingsSectionId::RequestRecovery")
                && web_model.contains("FrontendSpecificSettingsSectionId::Appearance")
                && web_model.contains("FrontendSpecificSettingsSectionId::Info"),
            "web settings must classify shared and frontend-specific sections explicitly"
        );

        assert!(
            tui_types.contains("fn surface_id(self)")
                && tui_types.contains("SettingsSectionSurfaceId::Shared")
                && tui_types.contains("SettingsSectionSurfaceId::FrontendSpecific"),
            "tui settings must classify settings sections through shared surface ids"
        );
        assert!(
            tui_types.contains("SharedSettingsSectionId::GuardianThreshold")
                && tui_types.contains("SharedSettingsSectionId::RequestRecovery")
                && tui_types.contains("FrontendSpecificSettingsSectionId::Observability"),
            "tui settings must classify shared and frontend-specific sections explicitly"
        );

        assert!(
            tui_export.contains("settings_section_item_id(section.surface_id()).to_string()"),
            "tui settings export must use the canonical parity item id"
        );
        assert!(
            !tui_export.contains("to_ascii_lowercase().replace(' ', \"_\")"),
            "tui settings export must not derive parity ids from section titles"
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
    fn parity_exception_metadata_is_complete_and_documented() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root");

        let declared_exceptions: HashSet<_> = SHARED_FLOW_SUPPORT
            .iter()
            .flat_map(|support| [support.web, support.tui])
            .filter_map(|availability| match availability {
                FlowAvailability::Exception(exception) => Some(exception),
                FlowAvailability::Supported => None,
            })
            .collect();

        let metadata_exceptions: HashSet<_> = PARITY_EXCEPTION_METADATA
            .iter()
            .map(|metadata| metadata.exception)
            .collect();
        assert_eq!(
            metadata_exceptions, declared_exceptions,
            "parity exception metadata must stay exhaustive"
        );

        for metadata in PARITY_EXCEPTION_METADATA {
            assert!(
                !metadata.reason_code.trim().is_empty(),
                "parity exception {:?} must declare a reason code",
                metadata.exception
            );
            assert!(
                !metadata.scope.trim().is_empty(),
                "parity exception {:?} must declare a scope",
                metadata.exception
            );
            assert!(
                !metadata.affected_surface.trim().is_empty(),
                "parity exception {:?} must declare an affected surface",
                metadata.exception
            );
            assert!(
                metadata.doc_reference.starts_with("docs/"),
                "parity exception {:?} must point at authoritative docs",
                metadata.exception
            );
            assert!(
                workspace_root.join(metadata.doc_reference).is_file(),
                "parity exception {:?} references missing doc {}",
                metadata.exception,
                metadata.doc_reference
            );
        }
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
            revision: ProjectionRevision {
                semantic_seq: 1,
                render_seq: Some(1),
            },
            quiescence: QuiescenceSnapshot::settled(),
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
                    is_current: false,
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
        tui.messages[0].id = "tui-1".to_string();
        tui.operations = vec![OperationSnapshot {
            instance_id: OperationInstanceId("tui-op".to_string()),
            ..tui.operations[0].clone()
        }];

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
    fn ui_snapshot_parity_detects_focus_semantic_drift() {
        let web = UiSnapshot {
            screen: ScreenId::Chat,
            focused_control: Some(ControlId::Screen(ScreenId::Chat)),
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: ProjectionRevision {
                semantic_seq: 1,
                render_seq: Some(1),
            },
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };
        let mut tui = web.clone();
        tui.focused_control = Some(ControlId::List(ListId::Channels));

        let mismatches = compare_ui_snapshots_for_parity(&web, &tui);
        assert_eq!(
            mismatches,
            vec![UiParityMismatch {
                field: "focused_control",
                web: "Some(Screen(Chat))".to_string(),
                tui: "Some(List(Channels))".to_string(),
            }]
        );
    }

    #[test]
    fn ui_snapshot_parity_reports_undeclared_drift() {
        let web = UiSnapshot {
            screen: ScreenId::Chat,
            focused_control: Some(ControlId::Screen(ScreenId::Chat)),
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: ProjectionRevision {
                semantic_seq: 1,
                render_seq: Some(1),
            },
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: Vec::new(),
        };
        let mut tui = web.clone();
        tui.screen = ScreenId::Contacts;
        tui.focused_control = Some(ControlId::Screen(ScreenId::Contacts));

        let mismatches = uncovered_ui_parity_mismatches(&web, &tui);
        assert!(mismatches.iter().any(|mismatch| mismatch.field == "screen"));
    }

    #[test]
    fn ui_snapshot_parity_detects_runtime_event_shape_drift() {
        let web = UiSnapshot {
            screen: ScreenId::Chat,
            focused_control: Some(ControlId::Screen(ScreenId::Chat)),
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: ProjectionRevision {
                semantic_seq: 1,
                render_seq: Some(1),
            },
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: Vec::new(),
            runtime_events: vec![RuntimeEventSnapshot {
                id: RuntimeEventId("event-1".to_string()),
                fact: RuntimeFact::MessageCommitted {
                    channel: ChannelFactKey {
                        id: Some("channel:shared".to_string()),
                        name: Some("shared".to_string()),
                    },
                    content: "hello".to_string(),
                },
            }],
        };
        let mut tui = web.clone();
        tui.runtime_events[0] = RuntimeEventSnapshot {
            id: RuntimeEventId("event-2".to_string()),
            fact: RuntimeFact::MessageDeliveryReady {
                channel: ChannelFactKey {
                    id: Some("channel:shared".to_string()),
                    name: Some("shared".to_string()),
                },
                member_count: 2,
            },
        };

        let mismatches = compare_ui_snapshots_for_parity(&web, &tui);
        assert_eq!(
            mismatches,
            vec![UiParityMismatch {
                field: "runtime_events",
                web: "[(MessageCommitted, \"message_committed:shared:hello\")]".to_string(),
                tui: "[(MessageDeliveryReady, \"message_delivery_ready:shared\")]".to_string(),
            }]
        );
    }

    #[test]
    fn ui_snapshot_parity_detects_toast_drift() {
        let web = UiSnapshot {
            screen: ScreenId::Chat,
            focused_control: Some(ControlId::Screen(ScreenId::Chat)),
            open_modal: None,
            readiness: UiReadiness::Ready,
            revision: ProjectionRevision {
                semantic_seq: 1,
                render_seq: Some(1),
            },
            quiescence: QuiescenceSnapshot::settled(),
            selections: Vec::new(),
            lists: Vec::new(),
            messages: Vec::new(),
            operations: Vec::new(),
            toasts: vec![ToastSnapshot {
                id: ToastId("toast-1".to_string()),
                kind: ToastKind::Success,
                message: "Saved".to_string(),
            }],
            runtime_events: Vec::new(),
        };
        let mut tui = web.clone();
        tui.toasts[0].message = "Failed".to_string();

        let mismatches = compare_ui_snapshots_for_parity(&web, &tui);
        assert_eq!(
            mismatches,
            vec![UiParityMismatch {
                field: "toasts",
                web: "[(Success, \"Saved\")]".to_string(),
                tui: "[(Success, \"Failed\")]".to_string(),
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
            revision: ProjectionRevision {
                semantic_seq: 1,
                render_seq: Some(1),
            },
            quiescence: QuiescenceSnapshot::settled(),
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
                        is_current: false,
                    }],
                },
                ListSnapshot {
                    id: ListId::SettingsSections,
                    items: vec![
                        ListItemSnapshot {
                            id: "guardian-threshold".to_string(),
                            selected: true,
                            confirmation: ConfirmationState::Confirmed,
                            is_current: false,
                        },
                        ListItemSnapshot {
                            id: "appearance".to_string(),
                            selected: false,
                            confirmation: ConfirmationState::Confirmed,
                            is_current: false,
                        },
                    ],
                },
                ListSnapshot {
                    id: ListId::Homes,
                    items: vec![ListItemSnapshot {
                        id: "stale-home".to_string(),
                        selected: true,
                        confirmation: ConfirmationState::Confirmed,
                        is_current: false,
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
            revision: ProjectionRevision {
                semantic_seq: 1,
                render_seq: Some(1),
            },
            quiescence: QuiescenceSnapshot::settled(),
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
                        is_current: false,
                    }],
                },
                ListSnapshot {
                    id: ListId::SettingsSections,
                    items: vec![
                        ListItemSnapshot {
                            id: "guardian-threshold".to_string(),
                            selected: true,
                            confirmation: ConfirmationState::Confirmed,
                            is_current: false,
                        },
                        ListItemSnapshot {
                            id: "observability".to_string(),
                            selected: false,
                            confirmation: ConfirmationState::Confirmed,
                            is_current: false,
                        },
                    ],
                },
                ListSnapshot {
                    id: ListId::NeighborhoodMembers,
                    items: vec![ListItemSnapshot {
                        id: "stale-member".to_string(),
                        selected: true,
                        confirmation: ConfirmationState::Confirmed,
                        is_current: false,
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
