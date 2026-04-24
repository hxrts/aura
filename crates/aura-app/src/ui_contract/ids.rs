//! Shared UI identifiers, controls, and handle-shaped command surfaces.

use super::{OperationId, OperationInstanceId, RuntimeFact};
use serde::{Deserialize, Serialize};

use crate::scenario_contract::SemanticCommandValue;
use crate::views::contacts::ContactRelationshipState;

pub const HARNESS_AUTH_TOKEN_MIN_LEN: usize = 16;
pub const HARNESS_COMMAND_MAX_FRAME_BYTES: usize = 64 * 1024;

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
    AcceptContactInvitation,
    AcceptChannelInvitation,
    CreateHome,
    CreateChannel,
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
    EditChannelInfo,
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
    ObserveRuntimeFact {
        fact: Box<RuntimeFact>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedHarnessUiCommand {
    pub token: String,
    pub command: HarnessUiCommand,
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
    pub channel_name: Option<String>,
}

impl ModalId {
    #[must_use]
    pub const fn blocks_quiescence(self) -> bool {
        !matches!(self, Self::InvitationCode)
    }

    #[must_use]
    pub const fn web_dom_id(self) -> &'static str {
        match self {
            Self::Help => "aura-modal-help",
            Self::CreateInvitation => "aura-modal-create-invitation",
            Self::InvitationCode => "aura-modal-invitation-code",
            Self::AcceptContactInvitation => "aura-modal-accept-contact-invitation",
            Self::AcceptChannelInvitation => "aura-modal-accept-channel-invitation",
            Self::CreateHome => "aura-modal-create-home",
            Self::CreateChannel => "aura-modal-create-channel",
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
            Self::EditChannelInfo => "aura-modal-edit-channel-info",
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
    InvitationReceiverNickname,
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
            Self::InvitationReceiverNickname => Some("aura-field-invitation-receiver-nickname"),
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

pub(crate) fn is_placeholder_semantic_id(raw: &str) -> bool {
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

pub(crate) fn is_override_semantic_id(raw: &str) -> bool {
    raw.trim().starts_with("override:")
}

pub(crate) fn is_row_index_semantic_id(raw: &str) -> bool {
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

impl SharedSettingsSectionId {
    pub const ALL: [Self; 5] = [
        Self::Profile,
        Self::GuardianThreshold,
        Self::RequestRecovery,
        Self::Devices,
        Self::Authority,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendSpecificSettingsSectionId {
    Appearance,
    Info,
    Observability,
}

impl FrontendSpecificSettingsSectionId {
    pub const ALL: [Self; 3] = [Self::Appearance, Self::Info, Self::Observability];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSectionSurfaceId {
    Shared(SharedSettingsSectionId),
    FrontendSpecific(FrontendSpecificSettingsSectionId),
}

pub const PARITY_CRITICAL_SETTINGS_SECTIONS: &[SharedSettingsSectionId] =
    &SharedSettingsSectionId::ALL;

pub const FRONTEND_SPECIFIC_SETTINGS_SECTIONS: &[FrontendSpecificSettingsSectionId] =
    &FrontendSpecificSettingsSectionId::ALL;

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
    ContactsSendFriendRequestButton,
    ContactsAcceptFriendRequestButton,
    ContactsDeclineFriendRequestButton,
    ContactsRemoveFriendButton,
    ContactsInviteToChannelButton,
    ContactsAddGuardianButton,
    ContactsEditNicknameButton,
    ContactsRemoveContactButton,
    ChatNewGroupButton,
    ChatEditChannelButton,
    ChatCloseChannelButton,
    ChatRetryMessageButton,
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
    AmpRaiseEmergencyAlarmButton,
    AmpApproveQuarantineButton,
    AmpApproveCryptoshredButton,
    AmpViewConflictEvidenceButton,
    AmpViewFinalizationStatusButton,
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
            Self::ContactsSendFriendRequestButton => Some("aura-contacts-send-friend-request"),
            Self::ContactsAcceptFriendRequestButton => Some("aura-contacts-accept-friend-request"),
            Self::ContactsDeclineFriendRequestButton => {
                Some("aura-contacts-decline-friend-request")
            }
            Self::ContactsRemoveFriendButton => Some("aura-contacts-remove-friend"),
            Self::ContactsInviteToChannelButton => Some("aura-contacts-invite-channel"),
            Self::ContactsAddGuardianButton => Some("aura-contacts-add-guardian"),
            Self::ContactsEditNicknameButton => Some("aura-contacts-edit-nickname"),
            Self::ContactsRemoveContactButton => Some("aura-contacts-remove-contact"),
            Self::ChatNewGroupButton => Some("aura-chat-new-group"),
            Self::ChatEditChannelButton => Some("aura-chat-edit-channel"),
            Self::ChatCloseChannelButton => Some("aura-chat-close-channel"),
            Self::ChatRetryMessageButton => Some("aura-chat-retry-message"),
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
            Self::AmpRaiseEmergencyAlarmButton => Some("aura-amp-raise-emergency-alarm"),
            Self::AmpApproveQuarantineButton => Some("aura-amp-approve-quarantine"),
            Self::AmpApproveCryptoshredButton => Some("aura-amp-approve-cryptoshred"),
            Self::AmpViewConflictEvidenceButton => Some("aura-amp-view-conflict-evidence"),
            Self::AmpViewFinalizationStatusButton => Some("aura-amp-view-finalization-status"),
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
            Self::ContactsSendFriendRequestButton => Some("f"),
            Self::ContactsAcceptFriendRequestButton => Some("y"),
            Self::ContactsDeclineFriendRequestButton => Some("x"),
            Self::ContactsRemoveFriendButton => Some("r"),
            Self::ContactsInviteToChannelButton => Some("i"),
            Self::ContactsAddGuardianButton => Some("g"),
            Self::ChatNewGroupButton => Some("n"),
            Self::ChatEditChannelButton => Some("e"),
            Self::ChatCloseChannelButton => Some("x"),
            Self::ChatRetryMessageButton => Some("r"),
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

#[must_use]
pub fn contacts_friend_action_controls(
    relationship_state: ContactRelationshipState,
) -> &'static [ControlId] {
    match relationship_state {
        ContactRelationshipState::Contact => &[ControlId::ContactsSendFriendRequestButton],
        ContactRelationshipState::PendingOutbound => &[ControlId::ContactsRemoveFriendButton],
        ContactRelationshipState::PendingInbound => &[
            ControlId::ContactsAcceptFriendRequestButton,
            ControlId::ContactsDeclineFriendRequestButton,
        ],
        ContactRelationshipState::Friend => &[ControlId::ContactsRemoveFriendButton],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_settings_section_item_id, settings_section_item_id, ControlId,
        FrontendSpecificSettingsSectionId, SettingsSectionSurfaceId, SharedSettingsSectionId,
        FRONTEND_SPECIFIC_SETTINGS_SECTIONS, PARITY_CRITICAL_SETTINGS_SECTIONS,
    };
    use std::collections::BTreeSet;

    #[test]
    fn settings_section_inventories_round_trip_through_item_ids() {
        let mut item_ids = BTreeSet::new();

        for section in SharedSettingsSectionId::ALL {
            assert!(
                PARITY_CRITICAL_SETTINGS_SECTIONS.contains(&section),
                "shared settings section inventory should include {section:?}",
            );
            let surface = SettingsSectionSurfaceId::Shared(section);
            let item_id = settings_section_item_id(surface);
            assert!(item_ids.insert(item_id), "settings item ids must be unique");
            assert_eq!(classify_settings_section_item_id(item_id), Some(surface));
        }

        for section in FrontendSpecificSettingsSectionId::ALL {
            assert!(
                FRONTEND_SPECIFIC_SETTINGS_SECTIONS.contains(&section),
                "frontend-specific settings inventory should include {section:?}",
            );
            let surface = SettingsSectionSurfaceId::FrontendSpecific(section);
            let item_id = settings_section_item_id(surface);
            assert!(item_ids.insert(item_id), "settings item ids must be unique");
            assert_eq!(classify_settings_section_item_id(item_id), Some(surface));
        }
    }

    #[test]
    fn amp_transition_action_controls_have_shared_dom_ids() {
        assert_eq!(
            ControlId::AmpRaiseEmergencyAlarmButton.web_dom_id(),
            Some("aura-amp-raise-emergency-alarm")
        );
        assert_eq!(
            ControlId::AmpApproveQuarantineButton.web_dom_id(),
            Some("aura-amp-approve-quarantine")
        );
        assert_eq!(
            ControlId::AmpApproveCryptoshredButton.web_dom_id(),
            Some("aura-amp-approve-cryptoshred")
        );
        assert_eq!(
            ControlId::AmpViewConflictEvidenceButton.web_dom_id(),
            Some("aura-amp-view-conflict-evidence")
        );
        assert_eq!(
            ControlId::AmpViewFinalizationStatusButton.web_dom_id(),
            Some("aura-amp-view-finalization-status")
        );
    }
}
