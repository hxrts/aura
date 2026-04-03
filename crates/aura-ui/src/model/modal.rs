use super::settings::{
    AccessOverrideLevel, CapabilityTier, DEFAULT_CAPABILITY_FULL, DEFAULT_CAPABILITY_LIMITED,
    DEFAULT_CAPABILITY_PARTIAL,
};
use super::*;
use aura_core::types::identifiers::CeremonyId;

#[derive(Debug, Clone, Copy)]
pub enum ModalState {
    Help,
    CreateInvitation,
    AcceptContactInvitation,
    AcceptChannelInvitation,
    CreateHome,
    CreateChannel,
    ChannelInfo,
    EditNickname,
    RemoveContact,
    GuardianSetup,
    RequestRecovery,
    AddDeviceStep1,
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

impl ModalState {
    #[must_use]
    pub const fn contract_id(self) -> ModalId {
        match self {
            Self::Help => ModalId::Help,
            Self::CreateInvitation => ModalId::CreateInvitation,
            Self::AcceptContactInvitation => ModalId::AcceptContactInvitation,
            Self::AcceptChannelInvitation => ModalId::AcceptChannelInvitation,
            Self::CreateHome => ModalId::CreateHome,
            Self::CreateChannel => ModalId::CreateChannel,
            Self::ChannelInfo => ModalId::ChannelInfo,
            Self::EditNickname => ModalId::EditNickname,
            Self::RemoveContact => ModalId::RemoveContact,
            Self::GuardianSetup => ModalId::GuardianSetup,
            Self::RequestRecovery => ModalId::RequestRecovery,
            Self::AddDeviceStep1 => ModalId::AddDevice,
            Self::ImportDeviceEnrollmentCode => ModalId::ImportDeviceEnrollmentCode,
            Self::SelectDeviceToRemove => ModalId::SelectDeviceToRemove,
            Self::ConfirmRemoveDevice => ModalId::ConfirmRemoveDevice,
            Self::MfaSetup => ModalId::MfaSetup,
            Self::AssignModerator => ModalId::AssignModerator,
            Self::SwitchAuthority => ModalId::SwitchAuthority,
            Self::AccessOverride => ModalId::AccessOverride,
            Self::CapabilityConfig => ModalId::CapabilityConfig,
            Self::EditChannelInfo => ModalId::EditChannelInfo,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TextModalState {
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct CreateInvitationModalState {
    pub message: String,
    pub ttl_hours: u64,
    pub active_field: FieldId,
}

impl Default for CreateInvitationModalState {
    fn default() -> Self {
        Self {
            message: String::new(),
            ttl_hours: 24,
            active_field: FieldId::InvitationMessage,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateChannelModalState {
    pub step: CreateChannelWizardStep,
    pub active_field: CreateChannelDetailsField,
    pub member_focus: usize,
    pub selected_members: Vec<usize>,
    pub name: String,
    pub topic: String,
    pub threshold: u8,
}

impl Default for CreateChannelModalState {
    fn default() -> Self {
        Self {
            step: CreateChannelWizardStep::Details,
            active_field: CreateChannelDetailsField::Name,
            member_focus: 0,
            selected_members: Vec::new(),
            name: String::new(),
            topic: String::new(),
            threshold: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddDeviceModalState {
    pub step: AddDeviceWizardStep,
    pub device_name: String,
    pub enrollment_code: String,
    pub code_copied: bool,
    pub ceremony_id: Option<CeremonyId>,
    pub accepted_count: u16,
    pub total_count: u16,
    pub threshold: u16,
    pub is_complete: bool,
    pub has_failed: bool,
    pub error_message: Option<String>,
    pub name_input: String,
}

impl Default for AddDeviceModalState {
    fn default() -> Self {
        Self {
            step: AddDeviceWizardStep::Name,
            device_name: String::new(),
            enrollment_code: String::new(),
            code_copied: false,
            ceremony_id: None,
            accepted_count: 0,
            total_count: 0,
            threshold: 0,
            is_complete: false,
            has_failed: false,
            error_message: None,
            name_input: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThresholdWizardModalState {
    pub step: ThresholdWizardStep,
    pub focus_index: usize,
    pub selected_indices: Vec<usize>,
    pub selected_count: u8,
    pub threshold_k: u8,
    pub threshold_input: String,
    pub ceremony_id: Option<CeremonyId>,
}

impl ThresholdWizardModalState {
    #[must_use]
    pub fn with_defaults(selected_count: u8, threshold_k: u8) -> Self {
        Self {
            step: ThresholdWizardStep::Selection,
            focus_index: 0,
            selected_indices: Vec::new(),
            selected_count,
            threshold_k,
            threshold_input: String::new(),
            ceremony_id: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SelectDeviceModalState {
    pub candidate_name: String,
}

#[derive(Debug, Clone)]
pub struct CapabilityConfigModalState {
    pub full_caps: String,
    pub partial_caps: String,
    pub limited_caps: String,
    pub active_tier: CapabilityTier,
}

impl Default for CapabilityConfigModalState {
    fn default() -> Self {
        Self {
            full_caps: DEFAULT_CAPABILITY_FULL.to_string(),
            partial_caps: DEFAULT_CAPABILITY_PARTIAL.to_string(),
            limited_caps: DEFAULT_CAPABILITY_LIMITED.to_string(),
            active_tier: CapabilityTier::Full,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AccessOverrideModalState {
    pub level: AccessOverrideLevel,
}

impl Default for AccessOverrideModalState {
    fn default() -> Self {
        Self {
            level: AccessOverrideLevel::Limited,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EditChannelInfoModalState {
    pub name: String,
    pub topic: String,
}

#[derive(Debug, Clone)]
pub enum ActiveModal {
    Help,
    CreateInvitation(CreateInvitationModalState),
    AcceptContactInvitation(TextModalState),
    AcceptChannelInvitation(TextModalState),
    CreateHome(TextModalState),
    CreateChannel(CreateChannelModalState),
    ChannelInfo,
    EditNickname(TextModalState),
    RemoveContact,
    GuardianSetup(ThresholdWizardModalState),
    RequestRecovery,
    AddDevice(AddDeviceModalState),
    ImportDeviceEnrollmentCode(TextModalState),
    SelectDeviceToRemove(SelectDeviceModalState),
    ConfirmRemoveDevice(SelectDeviceModalState),
    MfaSetup(ThresholdWizardModalState),
    AssignModerator,
    SwitchAuthority,
    AccessOverride(AccessOverrideModalState),
    CapabilityConfig(CapabilityConfigModalState),
    EditChannelInfo(EditChannelInfoModalState),
}

impl ActiveModal {
    #[must_use]
    pub const fn state(&self) -> ModalState {
        match self {
            Self::Help => ModalState::Help,
            Self::CreateInvitation(_) => ModalState::CreateInvitation,
            Self::AcceptContactInvitation(_) => ModalState::AcceptContactInvitation,
            Self::AcceptChannelInvitation(_) => ModalState::AcceptChannelInvitation,
            Self::CreateHome(_) => ModalState::CreateHome,
            Self::CreateChannel(_) => ModalState::CreateChannel,
            Self::ChannelInfo => ModalState::ChannelInfo,
            Self::EditNickname(_) => ModalState::EditNickname,
            Self::RemoveContact => ModalState::RemoveContact,
            Self::GuardianSetup(_) => ModalState::GuardianSetup,
            Self::RequestRecovery => ModalState::RequestRecovery,
            Self::AddDevice(_) => ModalState::AddDeviceStep1,
            Self::ImportDeviceEnrollmentCode(_) => ModalState::ImportDeviceEnrollmentCode,
            Self::SelectDeviceToRemove(_) => ModalState::SelectDeviceToRemove,
            Self::ConfirmRemoveDevice(_) => ModalState::ConfirmRemoveDevice,
            Self::MfaSetup(_) => ModalState::MfaSetup,
            Self::AssignModerator => ModalState::AssignModerator,
            Self::SwitchAuthority => ModalState::SwitchAuthority,
            Self::AccessOverride(_) => ModalState::AccessOverride,
            Self::CapabilityConfig(_) => ModalState::CapabilityConfig,
            Self::EditChannelInfo(_) => ModalState::EditChannelInfo,
        }
    }
}

macro_rules! active_modal_accessors {
    ($(($ref_name:ident, $mut_name:ident, $variant:ident, $state:ty)),+ $(,)?) => {
        impl ActiveModal {
            $(
                #[must_use]
                pub fn $ref_name(&self) -> Option<&$state> {
                    match self {
                        Self::$variant(state) => Some(state),
                        _ => None,
                    }
                }

                pub fn $mut_name(&mut self) -> Option<&mut $state> {
                    match self {
                        Self::$variant(state) => Some(state),
                        _ => None,
                    }
                }
            )+
        }
    };
}

active_modal_accessors!(
    (
        create_invitation,
        create_invitation_mut,
        CreateInvitation,
        CreateInvitationModalState
    ),
    (
        create_channel,
        create_channel_mut,
        CreateChannel,
        CreateChannelModalState
    ),
    (add_device, add_device_mut, AddDevice, AddDeviceModalState),
    (
        guardian_setup,
        guardian_setup_mut,
        GuardianSetup,
        ThresholdWizardModalState
    ),
    (
        mfa_setup,
        mfa_setup_mut,
        MfaSetup,
        ThresholdWizardModalState
    ),
    (
        capability_config,
        capability_config_mut,
        CapabilityConfig,
        CapabilityConfigModalState
    ),
    (
        access_override,
        access_override_mut,
        AccessOverride,
        AccessOverrideModalState
    ),
    (
        edit_channel_info,
        edit_channel_info_mut,
        EditChannelInfo,
        EditChannelInfoModalState
    )
);

impl ActiveModal {
    #[must_use]
    pub fn selected_device(&self) -> Option<&SelectDeviceModalState> {
        match self {
            Self::SelectDeviceToRemove(state) | Self::ConfirmRemoveDevice(state) => Some(state),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateChannelWizardStep {
    Details,
    Members,
    Threshold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateChannelDetailsField {
    Name,
    Topic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddDeviceWizardStep {
    Name,
    ShareCode,
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdWizardStep {
    Selection,
    Threshold,
    Ceremony,
}
