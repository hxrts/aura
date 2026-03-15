//! UI state model and controller for the Aura web interface.
//!
//! Defines the core UI model (screens, selections, modals, toasts) and the
//! controller that bridges the model to the application core and input handlers.

#![allow(clippy::disallowed_types)]

use crate::clipboard::ClipboardPort;
use crate::keyboard::{apply_named_key, apply_text_keys};
use crate::snapshot::render_canonical_snapshot;
use async_lock::RwLock as AsyncRwLock;
use aura_app::ui_contract::{
    next_projection_revision, InvitationFactKind, QuiescenceSnapshot, RuntimeFact,
    SemanticOperationPhase, SemanticOperationStatus,
};
use aura_app::views::chat::{NOTE_TO_SELF_CHANNEL_NAME, NOTE_TO_SELF_CHANNEL_TOPIC};
use aura_app::{
    ui::contract::{
        ConfirmationState, ControlId, FieldId, ListId, ListItemSnapshot, ListSnapshot,
        MessageSnapshot, ModalId, OperationId, OperationInstanceId, OperationSnapshot,
        OperationState, RuntimeEventId, RuntimeEventSnapshot, SelectionSnapshot, ToastId,
        ToastKind, ToastSnapshot, UiReadiness, UiSnapshot,
    },
    AppCore,
};
use aura_core::types::identifiers::{AuthorityId, CeremonyId};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub use aura_app::ui::contract::ScreenId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeighborhoodMode {
    Map,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDepth {
    Full,
    Partial,
    Limited,
}

impl AccessDepth {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Partial => "Partial",
            Self::Limited => "Limited",
        }
    }

    #[must_use]
    pub const fn compact(self) -> &'static str {
        match self {
            Self::Full => "D:Full",
            Self::Partial => "D:Par",
            Self::Limited => "D:Lim",
        }
    }

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Limited => Self::Partial,
            Self::Partial => Self::Full,
            Self::Full => Self::Limited,
        }
    }
}

fn demo_authority_id(seed: &str) -> AuthorityId {
    let mut entropy = [0_u8; 32];
    for (idx, byte) in seed.as_bytes().iter().copied().enumerate() {
        entropy[idx % entropy.len()] ^= byte;
    }
    AuthorityId::new_from_entropy(entropy)
}

#[derive(Debug, Clone)]
pub struct ChannelRow {
    pub name: String,
    pub selected: bool,
    pub topic: String,
}

#[derive(Debug, Clone)]
pub struct ContactRow {
    pub authority_id: AuthorityId,
    pub name: String,
    pub selected: bool,
    pub is_guardian: bool,
    pub confirmation: ConfirmationState,
}

#[derive(Debug, Clone)]
pub struct AuthorityRow {
    pub id: AuthorityId,
    pub label: String,
    pub selected: bool,
    pub is_current: bool,
}

#[derive(Debug, Clone)]
pub struct ToastState {
    pub icon: char,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedHome {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NotificationSelectionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NeighborhoodMemberSelectionKey(pub String);

#[derive(Debug, Clone, Copy)]
pub enum ModalState {
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
    AddDeviceStep1,
    ImportDeviceEnrollmentCode,
    SelectDeviceToRemove,
    ConfirmRemoveDevice,
    MfaSetup,
    AssignModerator,
    SwitchAuthority,
    AccessOverride,
    CapabilityConfig,
}

impl ModalState {
    #[must_use]
    pub const fn contract_id(self) -> ModalId {
        match self {
            Self::Help => ModalId::Help,
            Self::CreateInvitation => ModalId::CreateInvitation,
            Self::AcceptInvitation => ModalId::AcceptInvitation,
            Self::CreateHome => ModalId::CreateHome,
            Self::CreateChannel => ModalId::CreateChannel,
            Self::SetChannelTopic => ModalId::SetChannelTopic,
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
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TextModalState {
    pub value: String,
}

#[derive(Debug, Clone, Default)]
pub struct CreateInvitationModalState {
    pub receiver_id: String,
    pub receiver_label: Option<String>,
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

#[derive(Debug, Clone)]
pub enum ActiveModal {
    Help,
    CreateInvitation(CreateInvitationModalState),
    AcceptInvitation(TextModalState),
    CreateHome(TextModalState),
    CreateChannel(CreateChannelModalState),
    SetChannelTopic(TextModalState),
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
}

impl ActiveModal {
    #[must_use]
    pub const fn state(&self) -> ModalState {
        match self {
            Self::Help => ModalState::Help,
            Self::CreateInvitation(_) => ModalState::CreateInvitation,
            Self::AcceptInvitation(_) => ModalState::AcceptInvitation,
            Self::CreateHome(_) => ModalState::CreateHome,
            Self::CreateChannel(_) => ModalState::CreateChannel,
            Self::SetChannelTopic(_) => ModalState::SetChannelTopic,
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
        }
    }
}

pub const DEFAULT_CAPABILITY_FULL: &str =
    "send_dm, send_message, update_contact, view_members, join_channel, leave_context, invite, manage_channel, pin_content, moderate:kick, moderate:ban, moderate:mute, grant_moderator";
pub const DEFAULT_CAPABILITY_PARTIAL: &str =
    "send_dm, send_message, update_contact, view_members, join_channel, leave_context";
pub const DEFAULT_CAPABILITY_LIMITED: &str = "send_dm, view_members";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Profile,
    GuardianThreshold,
    RequestRecovery,
    Devices,
    Authority,
    Appearance,
    Info,
}

impl SettingsSection {
    pub const ALL: [Self; 7] = [
        Self::Profile,
        Self::GuardianThreshold,
        Self::RequestRecovery,
        Self::Devices,
        Self::Authority,
        Self::Appearance,
        Self::Info,
    ];

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Profile => "Profile",
            Self::GuardianThreshold => "Guardian Threshold",
            Self::RequestRecovery => "Request Recovery",
            Self::Devices => "Devices",
            Self::Authority => "Authority",
            Self::Appearance => "Appearance",
            Self::Info => "Info",
        }
    }

    #[must_use]
    pub const fn subtitle(self) -> &'static str {
        match self {
            Self::Profile => "Configure profile settings",
            Self::GuardianThreshold => "Configure guardian policy",
            Self::RequestRecovery => "Configure recovery operations",
            Self::Devices => "Configure devices",
            Self::Authority => "Authority scope",
            Self::Appearance => "Theme and display",
            Self::Info => "Application and environment details",
        }
    }

    #[must_use]
    pub const fn dom_id(self) -> &'static str {
        match self {
            Self::Profile => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                    aura_app::ui_contract::SharedSettingsSectionId::Profile,
                ),
            ),
            Self::GuardianThreshold => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                    aura_app::ui_contract::SharedSettingsSectionId::GuardianThreshold,
                ),
            ),
            Self::RequestRecovery => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                    aura_app::ui_contract::SharedSettingsSectionId::RequestRecovery,
                ),
            ),
            Self::Devices => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                    aura_app::ui_contract::SharedSettingsSectionId::Devices,
                ),
            ),
            Self::Authority => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                    aura_app::ui_contract::SharedSettingsSectionId::Authority,
                ),
            ),
            Self::Appearance => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::FrontendSpecific(
                    aura_app::ui_contract::FrontendSpecificSettingsSectionId::Appearance,
                ),
            ),
            Self::Info => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::FrontendSpecific(
                    aura_app::ui_contract::FrontendSpecificSettingsSectionId::Info,
                ),
            ),
        }
    }

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Profile => 0,
            Self::GuardianThreshold => 1,
            Self::RequestRecovery => 2,
            Self::Devices => 3,
            Self::Authority => 4,
            Self::Appearance => 5,
            Self::Info => 6,
        }
    }

    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Profile,
            1 => Self::GuardianThreshold,
            2 => Self::RequestRecovery,
            3 => Self::Devices,
            4 => Self::Authority,
            5 => Self::Appearance,
            _ => Self::Info,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityTier {
    Full,
    Partial,
    Limited,
}

impl CapabilityTier {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Partial => "Partial",
            Self::Limited => "Limited",
        }
    }

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Full => Self::Partial,
            Self::Partial => Self::Limited,
            Self::Limited => Self::Full,
        }
    }

    #[must_use]
    pub const fn prev(self) -> Self {
        match self {
            Self::Full => Self::Limited,
            Self::Partial => Self::Full,
            Self::Limited => Self::Partial,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessOverrideLevel {
    Limited,
    Partial,
}

impl AccessOverrideLevel {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Limited => "Limited",
            Self::Partial => "Partial",
        }
    }

    #[must_use]
    pub const fn toggle(self) -> Self {
        match self {
            Self::Limited => Self::Partial,
            Self::Partial => Self::Limited,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UiModel {
    pub account_ready: bool,
    pub account_setup_name: String,
    pub account_setup_error: Option<String>,
    pub screen: ScreenId,
    pub settings_section: SettingsSection,
    pub channels: Vec<ChannelRow>,
    pub contacts: Vec<ContactRow>,
    pub authorities: Vec<AuthorityRow>,
    pub messages: Vec<String>,
    pub notifications: Vec<String>,
    pub notification_ids: Vec<NotificationSelectionId>,
    pub logs: Vec<String>,
    pub operations: Vec<OperationSnapshot>,
    pub runtime_events: Vec<RuntimeEventSnapshot>,
    pub toast: Option<ToastState>,
    pub toast_key: u64,
    pub operation_instance_key: u64,
    pub runtime_event_key: u64,
    pub input_mode: bool,
    pub input_buffer: String,
    pub modal_hint: String,
    pub active_modal: Option<ActiveModal>,
    pub device_enrollment_counter: u64,
    pub selected_home: Option<SelectedHome>,
    pub neighborhood_mode: NeighborhoodMode,
    pub access_depth: AccessDepth,
    pub authority_id: String,
    pub profile_nickname: String,
    pub invite_counter: u64,
    pub last_invite_code: Option<String>,
    pub last_scan: String,
    pub has_secondary_device: bool,
    pub secondary_device_name: Option<String>,
    pub selected_contact_id: Option<AuthorityId>,
    pub selected_authority_id: Option<AuthorityId>,
    pub selected_channel: Option<String>,
    pub selected_neighborhood_member_key: Option<NeighborhoodMemberSelectionKey>,
    pub selected_notification_id: Option<NotificationSelectionId>,
    pub contact_details: bool,
}

impl UiModel {
    pub fn new(authority_id: String) -> Self {
        Self {
            account_ready: true,
            account_setup_name: String::new(),
            account_setup_error: None,
            screen: ScreenId::Neighborhood,
            settings_section: SettingsSection::Profile,
            channels: vec![ChannelRow {
                name: NOTE_TO_SELF_CHANNEL_NAME.to_string(),
                selected: true,
                topic: NOTE_TO_SELF_CHANNEL_TOPIC.to_string(),
            }],
            contacts: Vec::new(),
            authorities: Vec::new(),
            messages: Vec::new(),
            notifications: Vec::new(),
            notification_ids: Vec::new(),
            logs: vec!["Aura web shell initialized".to_string()],
            operations: Vec::new(),
            runtime_events: Vec::new(),
            toast: None,
            toast_key: 0,
            operation_instance_key: 0,
            runtime_event_key: 0,
            input_mode: false,
            input_buffer: String::new(),
            modal_hint: String::new(),
            active_modal: None,
            device_enrollment_counter: 0,
            selected_home: None,
            neighborhood_mode: NeighborhoodMode::Map,
            access_depth: AccessDepth::Limited,
            authority_id,
            profile_nickname: "Ops".to_string(),
            invite_counter: 0,
            last_invite_code: None,
            last_scan: "never".to_string(),
            has_secondary_device: false,
            secondary_device_name: None,
            selected_contact_id: None,
            selected_authority_id: None,
            selected_channel: Some(NOTE_TO_SELF_CHANNEL_NAME.to_string()),
            selected_neighborhood_member_key: None,
            selected_notification_id: None,
            contact_details: false,
        }
    }

    pub fn selected_channel_name(&self) -> Option<&str> {
        self.selected_channel.as_deref()
    }

    fn set_operation_state(&mut self, operation_id: OperationId, state: OperationState) {
        if let Some(operation) = self.operations.iter_mut().find(|op| op.id == operation_id) {
            if state == OperationState::Submitting {
                self.operation_instance_key = self.operation_instance_key.saturating_add(1);
                operation.instance_id =
                    OperationInstanceId(format!("op-{}", self.operation_instance_key));
            }
            operation.state = state;
            return;
        }
        self.operation_instance_key = self.operation_instance_key.saturating_add(1);
        self.operations.push(OperationSnapshot {
            id: operation_id,
            instance_id: OperationInstanceId(format!("op-{}", self.operation_instance_key)),
            state,
        });
    }

    fn set_authoritative_operation_state(
        &mut self,
        operation_id: OperationId,
        state: OperationState,
    ) {
        let needs_new_instance = state == OperationState::Submitting
            && self
                .operations
                .iter()
                .find(|operation| operation.id == operation_id)
                .is_some_and(|operation| {
                    matches!(
                        operation.state,
                        OperationState::Succeeded | OperationState::Failed
                    )
                });
        if needs_new_instance {
            self.set_operation_state(operation_id, state);
            return;
        }

        if let Some(operation) = self.operations.iter_mut().find(|op| op.id == operation_id) {
            operation.state = state;
            return;
        }

        self.set_operation_state(operation_id, state);
    }

    fn clear_operation(&mut self, operation_id: &OperationId) {
        self.operations
            .retain(|operation| &operation.id != operation_id);
    }

    fn push_runtime_fact(&mut self, fact: RuntimeFact) {
        let fact_key = fact.key();
        self.runtime_event_key = self.runtime_event_key.saturating_add(1);
        self.runtime_events.retain(|event| event.key() != fact_key);
        self.runtime_events.push(RuntimeEventSnapshot {
            id: RuntimeEventId(format!("runtime-event-{}", self.runtime_event_key)),
            fact,
        });
        if self.runtime_events.len() > 32 {
            let drain = self.runtime_events.len().saturating_sub(32);
            self.runtime_events.drain(0..drain);
        }
    }

    pub fn set_screen(&mut self, screen: ScreenId) {
        self.screen = screen;
        // Screen changes should always exit chat insert mode so global actions
        // (including settings buttons) are not swallowed as hidden text input.
        self.input_mode = false;
        self.input_buffer.clear();
        if matches!(screen, ScreenId::Neighborhood) {
            self.neighborhood_mode = NeighborhoodMode::Map;
        }
        if matches!(self.modal_state(), Some(ModalState::Help)) {
            self.modal_hint = format!("Help - {}", screen.help_label());
        }
    }

    #[must_use]
    pub fn modal_state(&self) -> Option<ModalState> {
        self.active_modal.as_ref().map(ActiveModal::state)
    }

    #[must_use]
    pub fn create_invitation_modal(&self) -> Option<&CreateInvitationModalState> {
        match self.active_modal.as_ref() {
            Some(ActiveModal::CreateInvitation(state)) => Some(state),
            _ => None,
        }
    }

    #[must_use]
    pub fn create_channel_modal(&self) -> Option<&CreateChannelModalState> {
        match self.active_modal.as_ref() {
            Some(ActiveModal::CreateChannel(state)) => Some(state),
            _ => None,
        }
    }

    pub fn create_channel_modal_mut(&mut self) -> Option<&mut CreateChannelModalState> {
        match self.active_modal.as_mut() {
            Some(ActiveModal::CreateChannel(state)) => Some(state),
            _ => None,
        }
    }

    #[must_use]
    pub fn add_device_modal(&self) -> Option<&AddDeviceModalState> {
        match self.active_modal.as_ref() {
            Some(ActiveModal::AddDevice(state)) => Some(state),
            _ => None,
        }
    }

    pub fn add_device_modal_mut(&mut self) -> Option<&mut AddDeviceModalState> {
        match self.active_modal.as_mut() {
            Some(ActiveModal::AddDevice(state)) => Some(state),
            _ => None,
        }
    }

    #[must_use]
    pub fn guardian_setup_modal(&self) -> Option<&ThresholdWizardModalState> {
        match self.active_modal.as_ref() {
            Some(ActiveModal::GuardianSetup(state)) => Some(state),
            _ => None,
        }
    }

    pub fn guardian_setup_modal_mut(&mut self) -> Option<&mut ThresholdWizardModalState> {
        match self.active_modal.as_mut() {
            Some(ActiveModal::GuardianSetup(state)) => Some(state),
            _ => None,
        }
    }

    #[must_use]
    pub fn mfa_setup_modal(&self) -> Option<&ThresholdWizardModalState> {
        match self.active_modal.as_ref() {
            Some(ActiveModal::MfaSetup(state)) => Some(state),
            _ => None,
        }
    }

    pub fn mfa_setup_modal_mut(&mut self) -> Option<&mut ThresholdWizardModalState> {
        match self.active_modal.as_mut() {
            Some(ActiveModal::MfaSetup(state)) => Some(state),
            _ => None,
        }
    }

    #[must_use]
    pub fn capability_config_modal(&self) -> Option<&CapabilityConfigModalState> {
        match self.active_modal.as_ref() {
            Some(ActiveModal::CapabilityConfig(state)) => Some(state),
            _ => None,
        }
    }

    pub fn capability_config_modal_mut(&mut self) -> Option<&mut CapabilityConfigModalState> {
        match self.active_modal.as_mut() {
            Some(ActiveModal::CapabilityConfig(state)) => Some(state),
            _ => None,
        }
    }

    #[must_use]
    pub fn access_override_modal(&self) -> Option<&AccessOverrideModalState> {
        match self.active_modal.as_ref() {
            Some(ActiveModal::AccessOverride(state)) => Some(state),
            _ => None,
        }
    }

    pub fn access_override_modal_mut(&mut self) -> Option<&mut AccessOverrideModalState> {
        match self.active_modal.as_mut() {
            Some(ActiveModal::AccessOverride(state)) => Some(state),
            _ => None,
        }
    }

    #[must_use]
    pub fn selected_device_modal(&self) -> Option<&SelectDeviceModalState> {
        match self.active_modal.as_ref() {
            Some(
                ActiveModal::SelectDeviceToRemove(state) | ActiveModal::ConfirmRemoveDevice(state),
            ) => Some(state),
            _ => None,
        }
    }

    #[must_use]
    pub fn modal_field_id(&self) -> Option<FieldId> {
        let active_modal = self.active_modal.as_ref()?;
        match active_modal {
            ActiveModal::CreateInvitation(_) => Some(FieldId::InvitationReceiver),
            ActiveModal::AcceptInvitation(_) => Some(FieldId::InvitationCode),
            ActiveModal::CreateHome(_) => Some(FieldId::HomeName),
            ActiveModal::SetChannelTopic(_) => Some(FieldId::CreateChannelTopic),
            ActiveModal::EditNickname(_) => Some(FieldId::Nickname),
            ActiveModal::ImportDeviceEnrollmentCode(_) => Some(FieldId::DeviceImportCode),
            ActiveModal::CreateChannel(state) => match state.step {
                CreateChannelWizardStep::Details => Some(match state.active_field {
                    CreateChannelDetailsField::Name => FieldId::CreateChannelName,
                    CreateChannelDetailsField::Topic => FieldId::CreateChannelTopic,
                }),
                CreateChannelWizardStep::Threshold => Some(FieldId::ThresholdInput),
                CreateChannelWizardStep::Members => None,
            },
            ActiveModal::GuardianSetup(state) | ActiveModal::MfaSetup(state) => {
                if matches!(state.step, ThresholdWizardStep::Threshold) {
                    Some(FieldId::ThresholdInput)
                } else {
                    None
                }
            }
            ActiveModal::AddDevice(state) => {
                if matches!(state.step, AddDeviceWizardStep::Name) {
                    Some(FieldId::DeviceName)
                } else {
                    None
                }
            }
            ActiveModal::CapabilityConfig(state) => Some(match state.active_tier {
                CapabilityTier::Full => FieldId::CapabilityFull,
                CapabilityTier::Partial => FieldId::CapabilityPartial,
                CapabilityTier::Limited => FieldId::CapabilityLimited,
            }),
            _ => None,
        }
    }

    #[must_use]
    pub fn modal_text_value(&self) -> Option<String> {
        let active_modal = self.active_modal.as_ref()?;
        match active_modal {
            ActiveModal::CreateInvitation(state) => Some(state.receiver_id.clone()),
            ActiveModal::AcceptInvitation(state)
            | ActiveModal::CreateHome(state)
            | ActiveModal::SetChannelTopic(state)
            | ActiveModal::EditNickname(state)
            | ActiveModal::ImportDeviceEnrollmentCode(state) => Some(state.value.clone()),
            ActiveModal::CreateChannel(state) => match state.step {
                CreateChannelWizardStep::Details => match state.active_field {
                    CreateChannelDetailsField::Name => Some(state.name.clone()),
                    CreateChannelDetailsField::Topic => Some(state.topic.clone()),
                },
                CreateChannelWizardStep::Threshold => Some(state.threshold.to_string()),
                CreateChannelWizardStep::Members => None,
            },
            ActiveModal::GuardianSetup(state) | ActiveModal::MfaSetup(state) => {
                if matches!(state.step, ThresholdWizardStep::Threshold) {
                    Some(state.threshold_input.clone())
                } else {
                    None
                }
            }
            ActiveModal::AddDevice(state) => {
                if matches!(state.step, AddDeviceWizardStep::Name) {
                    Some(state.name_input.clone())
                } else {
                    None
                }
            }
            ActiveModal::CapabilityConfig(state) => Some(match state.active_tier {
                CapabilityTier::Full => state.full_caps.clone(),
                CapabilityTier::Partial => state.partial_caps.clone(),
                CapabilityTier::Limited => state.limited_caps.clone(),
            }),
            _ => None,
        }
    }

    pub fn set_modal_text_value(&mut self, value: impl Into<String>) {
        let value = value.into();
        let Some(active_modal) = self.active_modal.as_mut() else {
            return;
        };
        match active_modal {
            ActiveModal::CreateInvitation(state) => state.receiver_id = value,
            ActiveModal::AcceptInvitation(state)
            | ActiveModal::CreateHome(state)
            | ActiveModal::SetChannelTopic(state)
            | ActiveModal::EditNickname(state)
            | ActiveModal::ImportDeviceEnrollmentCode(state) => state.value = value,
            ActiveModal::CreateChannel(state) => match state.step {
                CreateChannelWizardStep::Details => match state.active_field {
                    CreateChannelDetailsField::Name => state.name = value,
                    CreateChannelDetailsField::Topic => state.topic = value,
                },
                CreateChannelWizardStep::Threshold => {
                    state.threshold = value.trim().parse::<u8>().unwrap_or(state.threshold.max(1));
                }
                CreateChannelWizardStep::Members => {}
            },
            ActiveModal::GuardianSetup(state) | ActiveModal::MfaSetup(state) => {
                if matches!(state.step, ThresholdWizardStep::Threshold) {
                    state.threshold_input = value;
                }
            }
            ActiveModal::AddDevice(state) => {
                if matches!(state.step, AddDeviceWizardStep::Name) {
                    state.name_input = value;
                }
            }
            ActiveModal::CapabilityConfig(state) => match state.active_tier {
                CapabilityTier::Full => state.full_caps = value,
                CapabilityTier::Partial => state.partial_caps = value,
                CapabilityTier::Limited => state.limited_caps = value,
            },
            _ => {}
        }
    }

    pub fn set_modal_field_value(&mut self, field_id: FieldId, value: impl Into<String>) {
        let value = value.into();
        if let Some(ActiveModal::CreateChannel(state)) = self.active_modal.as_mut() {
            if matches!(state.step, CreateChannelWizardStep::Details) {
                match field_id {
                    FieldId::CreateChannelName => {
                        state.active_field = CreateChannelDetailsField::Name;
                        state.name = value;
                        return;
                    }
                    FieldId::CreateChannelTopic => {
                        state.active_field = CreateChannelDetailsField::Topic;
                        state.topic = value;
                        return;
                    }
                    _ => {}
                }
            }
        }
        self.set_modal_text_value(value);
    }

    pub fn set_modal_active_field(&mut self, field_id: FieldId) {
        let Some(ActiveModal::CreateChannel(state)) = self.active_modal.as_mut() else {
            return;
        };
        if !matches!(state.step, CreateChannelWizardStep::Details) {
            return;
        }
        state.active_field = match field_id {
            FieldId::CreateChannelTopic => CreateChannelDetailsField::Topic,
            _ => CreateChannelDetailsField::Name,
        };
    }

    pub fn append_modal_text_char(&mut self, ch: char) {
        let mut value = self.modal_text_value().unwrap_or_default();
        value.push(ch);
        self.set_modal_text_value(value);
    }

    pub fn pop_modal_text_char(&mut self) {
        let mut value = self.modal_text_value().unwrap_or_default();
        value.pop();
        self.set_modal_text_value(value);
    }

    #[must_use]
    pub fn modal_accepts_text(&self) -> bool {
        matches!(
            self.active_modal,
            Some(
                ActiveModal::CreateInvitation(_)
                    | ActiveModal::AcceptInvitation(_)
                    | ActiveModal::CreateHome(_)
                    | ActiveModal::SetChannelTopic(_)
                    | ActiveModal::EditNickname(_)
                    | ActiveModal::ImportDeviceEnrollmentCode(_)
                    | ActiveModal::CapabilityConfig(_)
            )
        ) || matches!(
            self.active_modal,
            Some(
                ActiveModal::CreateChannel(CreateChannelModalState {
                    step: CreateChannelWizardStep::Details | CreateChannelWizardStep::Threshold,
                    ..
                }) | ActiveModal::GuardianSetup(ThresholdWizardModalState {
                    step: ThresholdWizardStep::Threshold,
                    ..
                }) | ActiveModal::MfaSetup(ThresholdWizardModalState {
                    step: ThresholdWizardStep::Threshold,
                    ..
                }) | ActiveModal::AddDevice(AddDeviceModalState {
                    step: AddDeviceWizardStep::Name,
                    ..
                })
            )
        )
    }

    pub fn dismiss_modal(&mut self) {
        self.modal_hint.clear();
        self.active_modal = None;
    }

    pub fn select_channel_by_name(&mut self, name: &str) {
        let mut found = false;
        for row in &mut self.channels {
            let matches = row.name.eq_ignore_ascii_case(name);
            row.selected = matches;
            if matches {
                found = true;
            }
        }
        if !found {
            for row in &mut self.channels {
                row.selected = false;
            }
            self.channels.push(ChannelRow {
                name: name.to_string(),
                selected: true,
                topic: String::new(),
            });
        }
        self.selected_channel = Some(name.to_string());
    }

    pub fn select_home(&mut self, id: impl Into<String>, name: impl Into<String>) {
        self.selected_home = Some(SelectedHome {
            id: id.into(),
            name: name.into(),
        });
        self.selected_neighborhood_member_key = None;
    }

    pub fn ensure_contact(&mut self, name: &str) {
        let authority_id = demo_authority_id(name);
        if self
            .contacts
            .iter()
            .any(|row| row.authority_id == authority_id || row.name.eq_ignore_ascii_case(name))
        {
            return;
        }
        self.contacts.push(ContactRow {
            authority_id,
            name: name.to_string(),
            selected: self.contacts.is_empty(),
            is_guardian: false,
            confirmation: ConfirmationState::Confirmed,
        });
        if self.contacts.len() == 1 {
            self.selected_contact_id = Some(authority_id);
        }
    }

    pub fn ensure_runtime_contact(
        &mut self,
        authority_id: AuthorityId,
        name: String,
        is_guardian: bool,
    ) {
        if let Some(existing) = self
            .contacts
            .iter_mut()
            .find(|row| row.authority_id == authority_id)
        {
            existing.name = name;
            existing.is_guardian = is_guardian;
            existing.confirmation = ConfirmationState::Confirmed;
            return;
        }

        self.contacts.push(ContactRow {
            authority_id,
            name,
            selected: self.contacts.is_empty(),
            is_guardian,
            confirmation: ConfirmationState::PendingLocal,
        });

        if self.selected_contact_id.is_none() {
            self.selected_contact_id = Some(authority_id);
        }
    }

    pub fn selected_contact_name(&self) -> Option<&str> {
        self.selected_contact_index()
            .and_then(|index| self.contacts.get(index))
            .map(|row| row.name.as_str())
    }

    pub fn selected_home_name(&self) -> Option<&str> {
        self.selected_home.as_ref().map(|home| home.name.as_str())
    }

    pub fn selected_home_id(&self) -> Option<&str> {
        self.selected_home.as_ref().map(|home| home.id.as_str())
    }

    pub fn selected_contact_authority_id(&self) -> Option<AuthorityId> {
        self.selected_contact_id
    }

    pub fn set_selected_contact_name(&mut self, value: String) {
        if let Some(contact) = self
            .selected_contact_index()
            .and_then(|index| self.contacts.get_mut(index))
        {
            contact.name = value;
        }
    }

    pub fn selected_contact_index(&self) -> Option<usize> {
        let selected = self.selected_contact_id?;
        self.contacts
            .iter()
            .position(|contact| contact.authority_id == selected)
    }

    pub fn selected_authority_index(&self) -> Option<usize> {
        let selected = self.selected_authority_id?;
        self.authorities
            .iter()
            .position(|authority| authority.id == selected)
    }

    pub fn set_selected_contact_index(&mut self, index: usize) {
        if self.contacts.is_empty() {
            self.selected_contact_id = None;
            return;
        }

        let selected_index = index.min(self.contacts.len().saturating_sub(1));
        let selected_contact_id = self.contacts[selected_index].authority_id;
        self.selected_contact_id = Some(selected_contact_id);
        for (idx, contact) in self.contacts.iter_mut().enumerate() {
            contact.selected = idx == selected_index;
        }
    }

    pub fn set_selected_contact_authority_id(&mut self, authority_id: AuthorityId) {
        if self.contacts.is_empty() {
            self.selected_contact_id = None;
            return;
        }

        let selected_index = self
            .contacts
            .iter()
            .position(|contact| contact.authority_id == authority_id)
            .unwrap_or(0);
        self.set_selected_contact_index(selected_index);
    }

    pub fn set_selected_authority_index(&mut self, index: usize) {
        if self.authorities.is_empty() {
            self.selected_authority_id = None;
            return;
        }

        let selected_index = index.min(self.authorities.len().saturating_sub(1));
        let selected_authority_id = self.authorities[selected_index].id;
        self.selected_authority_id = Some(selected_authority_id);
        for (idx, authority) in self.authorities.iter_mut().enumerate() {
            authority.selected = idx == selected_index;
        }
    }

    pub fn set_selected_neighborhood_member_key(
        &mut self,
        key: Option<NeighborhoodMemberSelectionKey>,
    ) {
        self.selected_neighborhood_member_key = key;
    }

    pub fn selected_notification_index(&self) -> Option<usize> {
        let selected = self.selected_notification_id.as_ref()?;
        self.notification_ids.iter().position(|id| id == selected)
    }

    pub fn set_selected_notification_index(&mut self, index: usize, count: usize) {
        if count == 0 || self.notification_ids.is_empty() {
            self.selected_notification_id = None;
            return;
        }

        let selected_index = index.min(count.saturating_sub(1));
        self.selected_notification_id = self.notification_ids.get(selected_index).cloned();
    }

    pub fn sync_runtime_notifications(
        &mut self,
        notifications: Vec<(NotificationSelectionId, String)>,
    ) {
        let previous = self.selected_notification_id.clone();
        self.notification_ids = notifications.iter().map(|(id, _)| id.clone()).collect();
        self.notifications = notifications.into_iter().map(|(_, title)| title).collect();
        self.selected_notification_id = previous
            .and_then(|id| {
                self.notification_ids
                    .iter()
                    .find(|item| **item == id)
                    .cloned()
            })
            .or_else(|| self.notification_ids.first().cloned());
    }

    pub fn replace_channels(&mut self, channels: Vec<(String, String)>) {
        let previous = self.selected_channel.clone();
        self.channels = channels
            .into_iter()
            .map(|(name, topic)| ChannelRow {
                name,
                selected: false,
                topic,
            })
            .collect();

        if self.channels.is_empty() {
            self.selected_channel = None;
            return;
        }

        let selected_name = previous
            .and_then(|name| {
                self.channels
                    .iter()
                    .find(|row| row.name.eq_ignore_ascii_case(&name))
                    .map(|row| row.name.clone())
            })
            .unwrap_or_else(|| self.channels[0].name.clone());
        self.selected_channel = Some(selected_name.clone());
        for row in &mut self.channels {
            row.selected = row.name == selected_name;
        }
    }

    pub fn replace_contacts(&mut self, contacts: Vec<(AuthorityId, String, bool)>) {
        let previous = self.selected_contact_id;
        self.contacts = contacts
            .into_iter()
            .map(|(authority_id, name, is_guardian)| ContactRow {
                authority_id,
                name,
                selected: false,
                is_guardian,
                confirmation: ConfirmationState::Confirmed,
            })
            .collect();

        if self.contacts.is_empty() {
            self.selected_contact_id = None;
            return;
        }

        let selected_index = previous
            .and_then(|authority_id| {
                self.contacts
                    .iter()
                    .position(|row| row.authority_id == authority_id)
            })
            .unwrap_or(0);
        self.set_selected_contact_index(selected_index);
    }

    pub fn replace_authorities(&mut self, authorities: Vec<(AuthorityId, String, bool)>) {
        let previous = self.selected_authority_id;
        self.authorities = authorities
            .into_iter()
            .map(|(id, label, is_current)| AuthorityRow {
                id,
                label,
                selected: false,
                is_current,
            })
            .collect();

        if self.authorities.is_empty() {
            self.selected_authority_id = None;
            return;
        }

        let selected_index = previous
            .and_then(|id| self.authorities.iter().position(|row| row.id == id))
            .or_else(|| self.authorities.iter().position(|row| row.is_current))
            .unwrap_or(0);
        self.set_selected_authority_index(selected_index);
    }

    pub fn sync_profile(&mut self, authority_id: String, nickname: String) {
        if !authority_id.trim().is_empty() {
            self.authority_id = authority_id;
        }
        if !nickname.trim().is_empty() {
            self.profile_nickname = nickname;
        }
    }

    pub fn sync_devices(&mut self, devices: Vec<(String, bool)>) {
        let secondary = devices.into_iter().find(|(_, is_current)| !*is_current);
        self.has_secondary_device = secondary.is_some();
        self.secondary_device_name = secondary.map(|(name, _)| name);
    }

    pub fn selected_channel_topic(&self) -> &str {
        self.channels
            .iter()
            .find(|row| Some(row.name.as_str()) == self.selected_channel_name())
            .map(|row| row.topic.as_str())
            .unwrap_or("")
    }

    pub fn set_selected_channel_topic(&mut self, value: String) {
        let selected_name = self.selected_channel.clone();
        if let Some(channel) = selected_name
            .as_deref()
            .and_then(|name| self.channels.iter_mut().find(|row| row.name == name))
        {
            channel.topic = value;
        }
    }

    pub fn move_channel_selection(&mut self, delta: i32) {
        if self.channels.is_empty() {
            return;
        }
        let max = self.channels.len() as i32 - 1;
        let current_index = self
            .selected_channel_name()
            .and_then(|name| self.channels.iter().position(|row| row.name == name))
            .unwrap_or_default();
        let mut next = current_index as i32 + delta;
        if next < 0 {
            next = max;
        }
        if next > max {
            next = 0;
        }
        let selected_name = self.channels[next as usize].name.clone();
        self.selected_channel = Some(selected_name.clone());
        for row in &mut self.channels {
            row.selected = row.name == selected_name;
        }
    }

    pub fn secondary_device_name(&self) -> Option<&str> {
        self.secondary_device_name.as_deref()
    }

    pub fn set_secondary_device_name(&mut self, value: Option<String>) {
        self.secondary_device_name = value;
    }

    #[must_use]
    pub fn semantic_snapshot(&self) -> UiSnapshot {
        let mut lists = Vec::new();
        let mut selections = Vec::new();

        let navigation_items = [
            ScreenId::Neighborhood,
            ScreenId::Chat,
            ScreenId::Contacts,
            ScreenId::Notifications,
            ScreenId::Settings,
        ]
        .into_iter()
        .map(|screen| ListItemSnapshot {
            id: screen.help_label().to_ascii_lowercase(),
            selected: self.screen == screen,
            confirmation: ConfirmationState::Confirmed,
        })
        .collect::<Vec<_>>();
        lists.push(ListSnapshot {
            id: ListId::Navigation,
            items: navigation_items,
        });
        selections.push(SelectionSnapshot {
            list: ListId::Navigation,
            item_id: self.screen.help_label().to_ascii_lowercase(),
        });

        let channel_items = self
            .channels
            .iter()
            .map(|channel| ListItemSnapshot {
                id: channel.name.clone(),
                selected: channel.selected,
                confirmation: ConfirmationState::Confirmed,
            })
            .collect::<Vec<_>>();
        if !channel_items.is_empty() {
            lists.push(ListSnapshot {
                id: ListId::Channels,
                items: channel_items,
            });
        }
        if let Some(channel) = self.selected_channel_name() {
            selections.push(SelectionSnapshot {
                list: ListId::Channels,
                item_id: channel.to_string(),
            });
        }

        let contact_items = self
            .contacts
            .iter()
            .map(|contact| ListItemSnapshot {
                id: contact.authority_id.to_string(),
                selected: contact.selected,
                confirmation: contact.confirmation,
            })
            .collect::<Vec<_>>();
        if !contact_items.is_empty() {
            lists.push(ListSnapshot {
                id: ListId::Contacts,
                items: contact_items,
            });
        }
        if let Some(contact_id) = self.selected_contact_authority_id() {
            selections.push(SelectionSnapshot {
                list: ListId::Contacts,
                item_id: contact_id.to_string(),
            });
        }

        let authority_items = self
            .authorities
            .iter()
            .map(|authority| ListItemSnapshot {
                id: authority.id.to_string(),
                selected: authority.selected,
                confirmation: ConfirmationState::Confirmed,
            })
            .collect::<Vec<_>>();
        if !authority_items.is_empty() {
            lists.push(ListSnapshot {
                id: ListId::Authorities,
                items: authority_items,
            });
        }
        if let Some(authority_id) = self.selected_authority_id {
            selections.push(SelectionSnapshot {
                list: ListId::Authorities,
                item_id: authority_id.to_string(),
            });
        }

        let notification_items = self
            .notification_ids
            .iter()
            .map(|notification| ListItemSnapshot {
                id: notification.0.clone(),
                selected: self.selected_notification_id.as_ref() == Some(notification),
                confirmation: ConfirmationState::Confirmed,
            })
            .collect::<Vec<_>>();
        if !notification_items.is_empty() {
            lists.push(ListSnapshot {
                id: ListId::Notifications,
                items: notification_items,
            });
        }
        if let Some(notification_id) = &self.selected_notification_id {
            selections.push(SelectionSnapshot {
                list: ListId::Notifications,
                item_id: notification_id.0.clone(),
            });
        }

        let settings_items = SettingsSection::ALL
            .into_iter()
            .map(|section| ListItemSnapshot {
                id: section.dom_id().to_string(),
                selected: self.settings_section == section,
                confirmation: ConfirmationState::Confirmed,
            })
            .collect::<Vec<_>>();
        lists.push(ListSnapshot {
            id: ListId::SettingsSections,
            items: settings_items,
        });
        selections.push(SelectionSnapshot {
            list: ListId::SettingsSections,
            item_id: self.settings_section.dom_id().to_string(),
        });

        let mut toasts = Vec::new();
        if let Some(toast) = &self.toast {
            let kind = match toast.icon {
                '✓' => ToastKind::Success,
                'ℹ' => ToastKind::Info,
                _ => ToastKind::Error,
            };
            toasts.push(ToastSnapshot {
                id: ToastId(format!("toast-{}", self.toast_key)),
                kind,
                message: toast.message.clone(),
            });
        }

        let messages = self
            .messages
            .iter()
            .enumerate()
            .map(|(idx, content)| MessageSnapshot {
                id: format!("local-message-{idx}"),
                content: content.clone(),
            })
            .collect::<Vec<_>>();
        let open_modal = self.modal_state().map(ModalState::contract_id);
        let focused_control = if self.modal_field_id().is_some() {
            Some(ControlId::ModalInput)
        } else if let Some(open_modal) = open_modal {
            Some(ControlId::Modal(open_modal))
        } else if self.account_ready {
            Some(ControlId::Screen(self.screen))
        } else {
            Some(ControlId::OnboardingRoot)
        };

        UiSnapshot {
            screen: if self.account_ready {
                self.screen
            } else {
                ScreenId::Onboarding
            },
            focused_control,
            open_modal,
            readiness: if self.account_ready {
                UiReadiness::Ready
            } else {
                UiReadiness::Loading
            },
            revision: next_projection_revision(None),
            quiescence: QuiescenceSnapshot::derive(
                if self.account_ready {
                    UiReadiness::Ready
                } else {
                    UiReadiness::Loading
                },
                open_modal,
                &self.operations,
            ),
            selections,
            lists,
            messages,
            operations: self.operations.clone(),
            toasts,
            runtime_events: self.runtime_events.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderedHarnessSnapshot {
    pub screen: String,
    pub authoritative_screen: String,
    pub normalized_screen: String,
    pub raw_screen: String,
}

pub struct UiController {
    app_core: Arc<AsyncRwLock<AppCore>>,
    model: RwLock<UiModel>,
    clipboard: Arc<dyn ClipboardPort>,
    authority_switcher: Option<Arc<dyn Fn(AuthorityId) + Send + Sync>>,
    ui_snapshot_sink: Mutex<Option<UiSnapshotSink>>,
    rerender: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

type UiSnapshotSink = Arc<dyn Fn(UiSnapshot) + Send + Sync>;

impl PartialEq for UiController {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for UiController {}

fn set_toast(model: &mut UiModel, icon: char, message: impl Into<String>) {
    model.toast_key = model.toast_key.saturating_add(1);
    model.toast = Some(ToastState {
        icon,
        message: message.into(),
    });
}

fn dismiss_modal(model: &mut UiModel) {
    model.dismiss_modal();
}

impl UiController {
    pub fn new(app_core: Arc<AsyncRwLock<AppCore>>, clipboard: Arc<dyn ClipboardPort>) -> Self {
        Self::with_authority_switcher(app_core, clipboard, None)
    }

    pub fn with_authority_switcher(
        app_core: Arc<AsyncRwLock<AppCore>>,
        clipboard: Arc<dyn ClipboardPort>,
        authority_switcher: Option<Arc<dyn Fn(AuthorityId) + Send + Sync>>,
    ) -> Self {
        let authority_id = app_core
            .try_read()
            .and_then(|core| core.authority().cloned())
            .map(|id| id.to_string())
            .unwrap_or_else(|| "authority-local".to_string());

        Self {
            app_core,
            model: RwLock::new(UiModel::new(authority_id)),
            clipboard,
            authority_switcher,
            ui_snapshot_sink: Mutex::new(None),
            rerender: Mutex::new(None),
        }
    }

    pub fn set_rerender_callback(&self, rerender: Arc<dyn Fn() + Send + Sync>) {
        if let Ok(mut slot) = self.rerender.lock() {
            *slot = Some(rerender);
        }
    }

    pub fn request_rerender(&self) {
        if let Ok(slot) = self.rerender.lock() {
            let rerender = slot.as_ref().cloned();
            drop(slot);
            if let Some(rerender) = rerender {
                rerender();
            }
        }
    }

    pub fn set_ui_snapshot_sink(&self, sink: Arc<dyn Fn(UiSnapshot) + Send + Sync>) {
        if let Ok(mut slot) = self.ui_snapshot_sink.lock() {
            *slot = Some(sink);
        }
    }

    pub fn send_keys(&self, keys: &str) {
        let mut model = write_model(&self.model);
        apply_text_keys(&mut model, keys, self.clipboard.as_ref());
        drop(model);
        self.request_rerender();
    }

    pub fn send_action_keys(&self, keys: &str) {
        let mut model = write_model(&self.model);
        model.input_mode = false;
        model.input_buffer.clear();
        apply_text_keys(&mut model, keys, self.clipboard.as_ref());
        drop(model);
        self.request_rerender();
    }

    pub fn send_key_named(&self, key: &str, repeat: u16) {
        let mut model = write_model(&self.model);
        apply_named_key(&mut model, key, repeat, self.clipboard.as_ref());
        drop(model);
        self.request_rerender();
    }

    pub fn set_screen(&self, screen: ScreenId) {
        write_model(&self.model).set_screen(screen);
        self.request_rerender();
    }

    pub fn select_channel_by_name(&self, name: &str) {
        write_model(&self.model).select_channel_by_name(name);
        self.request_rerender();
    }

    pub fn select_home(&self, id: impl Into<String>, name: impl Into<String>) {
        write_model(&self.model).select_home(id, name);
        self.request_rerender();
    }

    pub fn set_modal_buffer(&self, value: &str) {
        write_model(&self.model).set_modal_text_value(value);
        self.request_rerender();
    }

    pub fn set_modal_field_value(&self, field_id: FieldId, value: &str) {
        write_model(&self.model).set_modal_field_value(field_id, value);
        self.request_rerender();
    }

    pub fn set_modal_active_field(&self, field_id: FieldId) {
        write_model(&self.model).set_modal_active_field(field_id);
        self.request_rerender();
    }

    pub fn clear_input_buffer(&self) {
        write_model(&self.model).input_buffer.clear();
        self.request_rerender();
    }

    pub fn exit_input_mode(&self) {
        let mut model = write_model(&self.model);
        model.input_mode = false;
        model.input_buffer.clear();
        drop(model);
        self.request_rerender();
    }

    pub fn set_input_buffer(&self, value: impl Into<String>) {
        write_model(&self.model).input_buffer = value.into();
        self.request_rerender();
    }

    pub fn set_selected_contact_index(&self, index: usize) {
        write_model(&self.model).set_selected_contact_index(index);
        self.request_rerender();
    }

    pub fn set_selected_contact_authority_id(&self, authority_id: AuthorityId) {
        write_model(&self.model).set_selected_contact_authority_id(authority_id);
        self.request_rerender();
    }

    pub fn set_selected_authority_index(&self, index: usize) {
        write_model(&self.model).set_selected_authority_index(index);
        self.request_rerender();
    }

    pub fn set_selected_neighborhood_member_key(
        &self,
        key: Option<NeighborhoodMemberSelectionKey>,
    ) {
        write_model(&self.model).set_selected_neighborhood_member_key(key);
        self.request_rerender();
    }

    pub fn set_selected_notification_index(&self, index: usize, count: usize) {
        write_model(&self.model).set_selected_notification_index(index, count);
        self.request_rerender();
    }

    pub fn publish_runtime_notifications_projection(
        &self,
        notifications: Vec<(NotificationSelectionId, String)>,
        facts: Vec<RuntimeFact>,
    ) {
        self.try_update_model(|model| {
            model.sync_runtime_notifications(notifications);
            for fact in facts {
                model.push_runtime_fact(fact);
            }
        });
    }

    pub fn publish_runtime_channels_projection(
        &self,
        channels: Vec<(String, String)>,
        facts: Vec<RuntimeFact>,
    ) {
        self.try_update_model(|model| {
            model.replace_channels(channels);
            for fact in facts {
                model.push_runtime_fact(fact);
            }
        });
    }

    pub fn publish_runtime_contacts_projection(
        &self,
        contacts: Vec<(AuthorityId, String, bool)>,
        facts: Vec<RuntimeFact>,
    ) {
        self.try_update_model(|model| {
            model.replace_contacts(contacts);
            for fact in facts {
                model.push_runtime_fact(fact);
            }
        });
    }

    pub fn push_runtime_fact(&self, fact: RuntimeFact) {
        self.try_update_model(|model| model.push_runtime_fact(fact));
    }

    pub fn ensure_runtime_contact(
        &self,
        authority_id: AuthorityId,
        name: String,
        is_guardian: bool,
    ) {
        self.try_update_model(|model| {
            model.ensure_runtime_contact(authority_id, name, is_guardian);
        });
        self.request_rerender();
    }

    pub fn open_create_invitation_modal(
        &self,
        receiver_id: Option<&AuthorityId>,
        receiver_label: Option<&str>,
    ) {
        let mut model = write_model(&self.model);
        model.clear_operation(&OperationId::invitation_create());
        model.active_modal = Some(ActiveModal::CreateInvitation(CreateInvitationModalState {
            receiver_id: receiver_id.map(ToString::to_string).unwrap_or_default(),
            receiver_label: receiver_label.map(str::to_string),
        }));
        drop(model);
        self.request_rerender();
    }

    pub fn sync_runtime_authorities(&self, authorities: Vec<(AuthorityId, String, bool)>) {
        self.try_update_model(|model| model.replace_authorities(authorities));
    }

    pub fn sync_runtime_profile(&self, authority_id: String, nickname: String) {
        self.try_update_model(|model| model.sync_profile(authority_id, nickname));
    }

    pub fn sync_runtime_devices(&self, devices: Vec<(String, bool)>) {
        self.try_update_model(|model| model.sync_devices(devices));
    }

    pub fn request_authority_switch(&self, authority_id: AuthorityId) -> bool {
        if let Some(switcher) = &self.authority_switcher {
            switcher(authority_id);
            true
        } else {
            false
        }
    }

    pub(crate) fn complete_runtime_home_created(&self, name: &str) {
        let mut model = write_model(&self.model);
        model.select_home(
            format!("home-{}", name.to_lowercase().replace(' ', "-")),
            name.to_string(),
        );
        model.access_depth = AccessDepth::Full;
        model.neighborhood_mode = NeighborhoodMode::Map;
        model.push_runtime_fact(RuntimeFact::HomeCreated {
            name: name.to_string(),
        });
        set_toast(&mut model, '✓', format!("Home '{name}' created"));
        dismiss_modal(&mut model);
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn apply_authoritative_operation_status(
        &self,
        operation_id: OperationId,
        status: SemanticOperationStatus,
    ) {
        let next_state = match status.phase {
            SemanticOperationPhase::Succeeded => OperationState::Succeeded,
            SemanticOperationPhase::Failed | SemanticOperationPhase::Cancelled => {
                OperationState::Failed
            }
            _ => OperationState::Submitting,
        };
        let mut model = write_model(&self.model);
        model.set_authoritative_operation_state(operation_id, next_state);
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn complete_runtime_modal_operation_success(
        &self,
        _operation_id: OperationId,
        message: impl Into<String>,
    ) {
        let mut model = write_model(&self.model);
        set_toast(&mut model, '✓', message);
        dismiss_modal(&mut model);
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn complete_runtime_invitation_operation(&self) {
        let mut model = write_model(&self.model);
        dismiss_modal(&mut model);
        model.push_runtime_fact(RuntimeFact::InvitationAccepted {
            invitation_kind: InvitationFactKind::Generic,
            authority_id: None,
            operation_state: None,
        });
        let snapshot = model.semantic_snapshot();
        let snapshot_modal = snapshot
            .open_modal
            .map(|modal| format!("{modal:?}"))
            .unwrap_or_else(|| "None".to_string());
        let invitation_state = snapshot
            .operations
            .iter()
            .find(|operation| operation.id == OperationId::invitation_accept())
            .map(|operation| format!("{:?}", operation.state))
            .unwrap_or_else(|| "Missing".to_string());
        tracing::info!(
            modal = %snapshot_modal,
            invitation_accept = %invitation_state,
            "complete_runtime_invitation_operation snapshot"
        );
        model.logs.push(format!(
            "complete_runtime_invitation_operation snapshot modal={snapshot_modal} invitation_accept={invitation_state}"
        ));
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn complete_runtime_contact_invitation_acceptance(
        &self,
        authority_id: AuthorityId,
        display_name: String,
    ) {
        let mut model = write_model(&self.model);
        model.ensure_runtime_contact(authority_id, display_name, false);
        model.push_runtime_fact(RuntimeFact::InvitationAccepted {
            invitation_kind: InvitationFactKind::Contact,
            authority_id: Some(authority_id.to_string()),
            operation_state: None,
        });
        model.push_runtime_fact(RuntimeFact::ContactLinkReady {
            authority_id: Some(authority_id.to_string()),
            contact_count: None,
        });
        model.set_selected_contact_authority_id(authority_id);
        dismiss_modal(&mut model);
        let snapshot = model.semantic_snapshot();
        let snapshot_modal = snapshot
            .open_modal
            .map(|modal| format!("{modal:?}"))
            .unwrap_or_else(|| "None".to_string());
        let invitation_state = snapshot
            .operations
            .iter()
            .find(|operation| operation.id == OperationId::invitation_accept())
            .map(|operation| format!("{:?}", operation.state))
            .unwrap_or_else(|| "Missing".to_string());
        tracing::info!(
            modal = %snapshot_modal,
            invitation_accept = %invitation_state,
            "complete_runtime_invitation_operation snapshot"
        );
        model.logs.push(format!(
            "complete_runtime_invitation_operation snapshot modal={snapshot_modal} invitation_accept={invitation_state}"
        ));
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn complete_runtime_modal_success(&self, message: impl Into<String>) {
        let mut model = write_model(&self.model);
        set_toast(&mut model, '✓', message);
        dismiss_modal(&mut model);
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn complete_runtime_device_enrollment_started(
        &self,
        name: &str,
        enrollment_code: &str,
    ) {
        let mut model = write_model(&self.model);
        model.modal_hint = "Add Device — Step 2 of 3".to_string();
        model.active_modal = Some(ActiveModal::AddDevice(AddDeviceModalState {
            step: AddDeviceWizardStep::ShareCode,
            device_name: name.to_string(),
            enrollment_code: enrollment_code.to_string(),
            code_copied: false,
            ..AddDeviceModalState::default()
        }));
        model.push_runtime_fact(RuntimeFact::DeviceEnrollmentCodeReady {
            device_name: Some(name.to_string()),
            code_len: Some(enrollment_code.len()),
            code: Some(enrollment_code.to_string()),
        });
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn set_runtime_device_enrollment_ceremony_id(&self, ceremony_id: CeremonyId) {
        let mut model = write_model(&self.model);
        if let Some(ActiveModal::AddDevice(state)) = model.active_modal.as_mut() {
            state.ceremony_id = Some(ceremony_id);
        }
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn update_runtime_device_enrollment_status(
        &self,
        accepted_count: u16,
        total_count: u16,
        threshold: u16,
        is_complete: bool,
        has_failed: bool,
        error_message: Option<String>,
    ) {
        let mut model = write_model(&self.model);
        if let Some(ActiveModal::AddDevice(state)) = model.active_modal.as_mut() {
            state.accepted_count = accepted_count;
            state.total_count = total_count;
            state.threshold = threshold;
            state.is_complete = is_complete;
            state.has_failed = has_failed;
            state.error_message = error_message;
            let device_name = state.device_name.clone();
            if is_complete {
                let should_set_name =
                    model.secondary_device_name.is_none() && !device_name.trim().is_empty();
                model.has_secondary_device = true;
                if should_set_name {
                    model.secondary_device_name = Some(device_name);
                }
            }
        }
        drop(model);
        self.request_rerender();
    }

    pub fn mark_add_device_code_copied(&self) {
        let mut model = write_model(&self.model);
        if let Some(ActiveModal::AddDevice(state)) = model.active_modal.as_mut() {
            state.code_copied = true;
        }
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn advance_runtime_device_enrollment_share(&self) {
        let mut model = write_model(&self.model);
        model.modal_hint = "Add Device — Step 3 of 3".to_string();
        if let Some(ActiveModal::AddDevice(state)) = model.active_modal.as_mut() {
            state.step = AddDeviceWizardStep::Confirm;
        }
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn complete_runtime_device_enrollment_ready(&self) {
        let mut model = write_model(&self.model);
        dismiss_modal(&mut model);
        drop(model);
        self.request_rerender();
    }

    pub fn complete_runtime_enter_home(&self, name: &str, depth: AccessDepth) {
        let mut model = write_model(&self.model);
        model.select_home(
            format!("home-{}", name.to_lowercase().replace(' ', "-")),
            name.to_string(),
        );
        model.access_depth = depth;
        model.neighborhood_mode = NeighborhoodMode::Detail;
        model.selected_neighborhood_member_key = None;
        model.push_runtime_fact(RuntimeFact::HomeEntered {
            name: name.to_string(),
            access_depth: Some(depth.label().to_string()),
        });
        set_toast(
            &mut model,
            '✓',
            format!("Entered '{name}' with {} access", depth.label()),
        );
        drop(model);
        self.request_rerender();
    }

    pub fn runtime_error_toast(&self, message: impl Into<String>) {
        let mut model = write_model(&self.model);
        set_toast(&mut model, '✗', message);
        drop(model);
        self.request_rerender();
    }

    pub fn info_toast(&self, message: impl Into<String>) {
        let mut model = write_model(&self.model);
        set_toast(&mut model, '✓', message);
        drop(model);
        self.request_rerender();
    }

    pub fn snapshot(&self) -> RenderedHarnessSnapshot {
        let screen = self
            .model
            .try_read()
            .ok()
            .map(|model| render_canonical_snapshot(&model))
            .unwrap_or_else(|| {
                let snapshot = self.ui_snapshot();
                format!(
                    "[harness-snapshot-busy]\nscreen={:?}\nreadiness={:?}\nopen_modal={:?}\nfocused_control={:?}",
                    snapshot.screen, snapshot.readiness, snapshot.open_modal, snapshot.focused_control
                )
            });
        let normalized_screen = screen
            .replace('\r', "")
            .lines()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n");

        RenderedHarnessSnapshot {
            screen: normalized_screen.clone(),
            authoritative_screen: normalized_screen.clone(),
            normalized_screen,
            raw_screen: screen,
        }
    }

    pub fn ui_snapshot(&self) -> UiSnapshot {
        self.model
            .read()
            .ok()
            .map(|model| model.semantic_snapshot())
            .unwrap_or_else(|| UiSnapshot::loading(ScreenId::Neighborhood))
    }

    pub fn semantic_model_snapshot(&self) -> UiSnapshot {
        let snapshot = self
            .model
            .read()
            .ok()
            .map(|model| model.semantic_snapshot())
            .unwrap_or_else(|| UiSnapshot::loading(ScreenId::Neighborhood));
        snapshot
            .validate_invariants()
            .unwrap_or_else(|error| panic!("invalid semantic model snapshot export: {error}"));
        snapshot
    }

    pub fn publish_ui_snapshot(&self, snapshot: UiSnapshot) {
        snapshot
            .validate_invariants()
            .unwrap_or_else(|error| panic!("invalid published UI snapshot: {error}"));
        if let Ok(slot) = self.ui_snapshot_sink.lock() {
            let sink = slot.as_ref().cloned();
            drop(slot);
            if let Some(sink) = sink {
                sink(snapshot);
            }
        }
    }

    pub fn set_ui_snapshot(&self, snapshot: UiSnapshot) {
        self.publish_ui_snapshot(snapshot);
    }

    pub fn read_clipboard(&self) -> String {
        self.clipboard.read()
    }

    pub fn write_clipboard(&self, text: &str) {
        self.clipboard.write(text);
    }

    pub fn tail_log(&self, lines: usize) -> Vec<String> {
        let model = read_model(&self.model);
        let mut output = model.logs.clone();
        if output.len() > lines {
            output = output.split_off(output.len() - lines);
        }
        output
    }

    pub fn inject_message(&self, message: &str) {
        write_model(&self.model).messages.push(message.to_string());
    }

    pub fn push_log(&self, line: &str) {
        write_model(&self.model).logs.push(line.to_string());
    }

    pub fn set_account_setup_state(
        &self,
        account_ready: bool,
        account_setup_name: impl Into<String>,
        account_setup_error: Option<String>,
    ) {
        let mut model = write_model(&self.model);
        model.account_ready = account_ready;
        model.account_setup_name = account_setup_name.into();
        model.account_setup_error = account_setup_error;
        let snapshot = model.semantic_snapshot();
        drop(model);
        self.publish_ui_snapshot(snapshot);
        self.request_rerender();
    }

    pub fn set_authority_id(&self, authority_id: &str) {
        write_model(&self.model).authority_id = authority_id.to_string();
        self.request_rerender();
    }

    pub fn set_settings_section(&self, section: SettingsSection) {
        write_model(&self.model).settings_section = section;
        self.request_rerender();
    }

    pub fn authority_id(&self) -> String {
        read_model(&self.model).authority_id.clone()
    }

    fn try_update_model(&self, update: impl FnOnce(&mut UiModel)) {
        if let Ok(mut model) = self.model.try_write() {
            update(&mut model);
        }
    }

    pub fn ui_model(&self) -> Option<UiModel> {
        Some(read_model(&self.model).clone())
    }

    pub fn app_core(&self) -> &Arc<AsyncRwLock<AppCore>> {
        &self.app_core
    }
}

fn read_model(model: &RwLock<UiModel>) -> RwLockReadGuard<'_, UiModel> {
    model.read().unwrap_or_else(|poison| poison.into_inner())
}

fn write_model(model: &RwLock<UiModel>) -> RwLockWriteGuard<'_, UiModel> {
    model.write().unwrap_or_else(|poison| poison.into_inner())
}

#[cfg(test)]
mod tests {
    use super::{NeighborhoodMode, UiModel, ScreenId};
    use aura_app::ui::contract::{OperationId, OperationState, RuntimeEventKind};
    use aura_app::ui_contract::{InvitationFactKind, RuntimeFact};

    #[test]
    fn set_screen_clears_input_mode_and_buffer() {
        let mut model = UiModel::new("authority-local".to_string());
        model.input_mode = true;
        model.input_buffer = "pending text".to_string();

        model.set_screen(ScreenId::Settings);

        assert!(!model.input_mode);
        assert!(model.input_buffer.is_empty());
        assert!(matches!(model.screen, ScreenId::Settings));
    }

    #[test]
    fn entering_neighborhood_screen_resets_to_map_mode() {
        let mut model = UiModel::new("authority-local".to_string());
        model.neighborhood_mode = NeighborhoodMode::Detail;

        model.set_screen(ScreenId::Neighborhood);

        assert!(matches!(model.neighborhood_mode, NeighborhoodMode::Map));
    }

    #[test]
    fn semantic_snapshot_includes_tracked_operation_state() {
        let mut model = UiModel::new("authority-local".to_string());
        model.set_operation_state(OperationId::invitation_accept(), OperationState::Submitting);

        let snapshot = model.semantic_snapshot();
        let operation_state = snapshot
            .operations
            .iter()
            .find(|operation| operation.id == OperationId::invitation_accept())
            .map(|operation| operation.state);

        assert_eq!(operation_state, Some(OperationState::Submitting));
    }

    #[test]
    fn restarting_operation_generates_new_operation_instance_id() {
        let mut model = UiModel::new("authority-local".to_string());
        model.set_operation_state(OperationId::invitation_accept(), OperationState::Submitting);
        let Some(first_instance) = model
            .semantic_snapshot()
            .operations
            .into_iter()
            .find(|operation| operation.id == OperationId::invitation_accept())
        else {
            panic!("first operation should exist");
        };
        let first_instance = first_instance.instance_id;

        model.set_operation_state(OperationId::invitation_accept(), OperationState::Succeeded);
        model.set_operation_state(OperationId::invitation_accept(), OperationState::Submitting);
        let Some(second_instance) = model
            .semantic_snapshot()
            .operations
            .into_iter()
            .find(|operation| operation.id == OperationId::invitation_accept())
        else {
            panic!("second operation should exist");
        };
        let second_instance = second_instance.instance_id;

        assert_ne!(first_instance, second_instance);
    }

    #[test]
    fn semantic_snapshot_includes_runtime_events() {
        let mut model = UiModel::new("authority-local".to_string());
        model.push_runtime_fact(RuntimeFact::InvitationAccepted {
            invitation_kind: InvitationFactKind::Contact,
            authority_id: None,
            operation_state: None,
        });

        let snapshot = model.semantic_snapshot();
        let Some(event) = snapshot.runtime_events.last() else {
            panic!("runtime event should be present");
        };

        assert_eq!(event.kind(), RuntimeEventKind::InvitationAccepted);
    }
}
