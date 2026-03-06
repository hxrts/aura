//! UI state model and controller for the Aura web interface.
//!
//! Defines the core UI model (screens, selections, modals, toasts) and the
//! controller that bridges the model to the application core and input handlers.

use crate::clipboard::ClipboardPort;
use crate::keyboard::{apply_named_key, apply_text_keys};
use crate::snapshot::render_canonical_snapshot;
use async_lock::RwLock as AsyncRwLock;
use aura_app::AppCore;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiScreen {
    Neighborhood,
    Chat,
    Contacts,
    Notifications,
    Settings,
}

impl UiScreen {
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

#[derive(Debug, Clone)]
pub struct ChannelRow {
    pub name: String,
    pub selected: bool,
    pub topic: String,
}

#[derive(Debug, Clone)]
pub struct ContactRow {
    pub name: String,
    pub selected: bool,
    pub is_guardian: bool,
}

#[derive(Debug, Clone)]
pub struct AuthorityRow {
    pub id: String,
    pub label: String,
    pub selected: bool,
    pub is_current: bool,
}

#[derive(Debug, Clone)]
pub struct ToastState {
    pub icon: char,
    pub message: String,
}

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

#[derive(Debug, Clone)]
pub struct UiModel {
    pub screen: UiScreen,
    pub settings_index: usize,
    pub channels: Vec<ChannelRow>,
    pub contacts: Vec<ContactRow>,
    pub authorities: Vec<AuthorityRow>,
    pub messages: Vec<String>,
    pub notifications: Vec<String>,
    pub logs: Vec<String>,
    pub toast: Option<ToastState>,
    pub toast_key: u64,
    pub input_mode: bool,
    pub input_buffer: String,
    pub modal: Option<ModalState>,
    pub modal_buffer: String,
    pub modal_hint: String,
    pub create_channel_step: CreateChannelWizardStep,
    pub add_device_step: AddDeviceWizardStep,
    pub add_device_name: String,
    pub add_device_enrollment_code: String,
    pub add_device_code_copied: bool,
    pub add_device_ceremony_id: Option<String>,
    pub add_device_accepted_count: u16,
    pub add_device_total_count: u16,
    pub add_device_threshold: u16,
    pub add_device_is_complete: bool,
    pub add_device_has_failed: bool,
    pub add_device_error_message: Option<String>,
    pub device_enrollment_counter: u64,
    pub guardian_wizard_step: ThresholdWizardStep,
    pub guardian_focus_index: usize,
    pub guardian_selected_indices: Vec<usize>,
    pub guardian_selected_count: u8,
    pub guardian_threshold_k: u8,
    pub mfa_wizard_step: ThresholdWizardStep,
    pub mfa_focus_index: usize,
    pub mfa_selected_indices: Vec<usize>,
    pub mfa_selected_count: u8,
    pub mfa_threshold_k: u8,
    pub remove_device_candidate_name: String,
    pub create_channel_active_field: CreateChannelDetailsField,
    pub create_channel_member_focus: usize,
    pub create_channel_selected_members: Vec<usize>,
    pub create_channel_name: String,
    pub create_channel_topic: String,
    pub create_channel_threshold: u8,
    pub capability_full_caps: String,
    pub capability_partial_caps: String,
    pub capability_limited_caps: String,
    pub capability_active_field: usize,
    pub access_override_partial: bool,
    pub selected_home: Option<String>,
    pub neighborhood_mode: NeighborhoodMode,
    pub access_depth: AccessDepth,
    pub authority_id: String,
    pub profile_nickname: String,
    pub invite_counter: u64,
    pub last_invite_code: Option<String>,
    pub last_scan: String,
    pub has_secondary_device: bool,
    pub secondary_device_name: Option<String>,
    pub selected_contact_index: usize,
    pub selected_authority_index: usize,
    pub selected_channel_index: usize,
    pub selected_neighborhood_member_index: usize,
    pub selected_notification_index: usize,
    pub contact_details: bool,
}

impl UiModel {
    pub fn new(authority_id: String) -> Self {
        Self {
            screen: UiScreen::Neighborhood,
            settings_index: 0,
            channels: vec![
                ChannelRow {
                    name: "general".to_string(),
                    selected: true,
                    topic: "bootstrap-topic".to_string(),
                },
                ChannelRow {
                    name: "dm".to_string(),
                    selected: false,
                    topic: String::new(),
                },
            ],
            contacts: Vec::new(),
            authorities: Vec::new(),
            messages: Vec::new(),
            notifications: Vec::new(),
            logs: vec!["Aura web shell initialized".to_string()],
            toast: None,
            toast_key: 0,
            input_mode: false,
            input_buffer: String::new(),
            modal: None,
            modal_buffer: String::new(),
            modal_hint: String::new(),
            create_channel_step: CreateChannelWizardStep::Details,
            add_device_step: AddDeviceWizardStep::Name,
            add_device_name: String::new(),
            add_device_enrollment_code: String::new(),
            add_device_code_copied: false,
            add_device_ceremony_id: None,
            add_device_accepted_count: 0,
            add_device_total_count: 0,
            add_device_threshold: 0,
            add_device_is_complete: false,
            add_device_has_failed: false,
            add_device_error_message: None,
            device_enrollment_counter: 0,
            guardian_wizard_step: ThresholdWizardStep::Selection,
            guardian_focus_index: 0,
            guardian_selected_indices: Vec::new(),
            guardian_selected_count: 2,
            guardian_threshold_k: 2,
            mfa_wizard_step: ThresholdWizardStep::Selection,
            mfa_focus_index: 0,
            mfa_selected_indices: Vec::new(),
            mfa_selected_count: 1,
            mfa_threshold_k: 1,
            remove_device_candidate_name: String::new(),
            create_channel_active_field: CreateChannelDetailsField::Name,
            create_channel_member_focus: 0,
            create_channel_selected_members: Vec::new(),
            create_channel_name: String::new(),
            create_channel_topic: String::new(),
            create_channel_threshold: 1,
            capability_full_caps: DEFAULT_CAPABILITY_FULL.to_string(),
            capability_partial_caps: DEFAULT_CAPABILITY_PARTIAL.to_string(),
            capability_limited_caps: DEFAULT_CAPABILITY_LIMITED.to_string(),
            capability_active_field: 0,
            access_override_partial: false,
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
            selected_contact_index: 0,
            selected_authority_index: 0,
            selected_channel_index: 0,
            selected_neighborhood_member_index: 0,
            selected_notification_index: 0,
            contact_details: false,
        }
    }

    pub fn selected_channel_name(&self) -> Option<&str> {
        self.channels
            .iter()
            .find(|row| row.selected)
            .map(|row| row.name.as_str())
    }

    pub fn set_screen(&mut self, screen: UiScreen) {
        self.screen = screen;
        // Screen changes should always exit chat insert mode so global actions
        // (including settings buttons) are not swallowed as hidden text input.
        self.input_mode = false;
        self.input_buffer.clear();
        if matches!(screen, UiScreen::Neighborhood) {
            self.neighborhood_mode = NeighborhoodMode::Map;
        }
        if matches!(self.modal, Some(ModalState::Help)) {
            self.modal_hint = format!("Help - {}", screen.help_label());
        }
    }

    pub fn select_channel_by_name(&mut self, name: &str) {
        let mut found = false;
        for (idx, row) in self.channels.iter_mut().enumerate() {
            let matches = row.name.eq_ignore_ascii_case(name);
            row.selected = matches;
            if matches {
                found = true;
                self.selected_channel_index = idx;
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
            self.selected_channel_index = self.channels.len().saturating_sub(1);
        }
    }

    pub fn select_home_by_name(&mut self, name: &str) {
        self.selected_home = Some(name.to_string());
        self.selected_neighborhood_member_index = 0;
    }

    pub fn ensure_contact(&mut self, name: &str) {
        if self
            .contacts
            .iter()
            .any(|row| row.name.eq_ignore_ascii_case(name))
        {
            return;
        }
        self.contacts.push(ContactRow {
            name: name.to_string(),
            selected: self.contacts.is_empty(),
            is_guardian: false,
        });
        if self.contacts.len() == 1 {
            self.selected_contact_index = 0;
        }
    }

    pub fn selected_contact_name(&self) -> Option<&str> {
        self.contacts
            .get(self.selected_contact_index)
            .map(|row| row.name.as_str())
    }

    pub fn set_selected_contact_name(&mut self, value: String) {
        if let Some(contact) = self.contacts.get_mut(self.selected_contact_index) {
            contact.name = value;
        }
    }

    pub fn set_selected_contact_index(&mut self, index: usize) {
        if self.contacts.is_empty() {
            self.selected_contact_index = 0;
            return;
        }

        self.selected_contact_index = index.min(self.contacts.len().saturating_sub(1));
        for (idx, contact) in self.contacts.iter_mut().enumerate() {
            contact.selected = idx == self.selected_contact_index;
        }
    }

    pub fn set_selected_authority_index(&mut self, index: usize) {
        if self.authorities.is_empty() {
            self.selected_authority_index = 0;
            return;
        }

        self.selected_authority_index = index.min(self.authorities.len().saturating_sub(1));
        for (idx, authority) in self.authorities.iter_mut().enumerate() {
            authority.selected = idx == self.selected_authority_index;
        }
    }

    pub fn set_selected_neighborhood_member_index(&mut self, index: usize) {
        self.selected_neighborhood_member_index = index;
    }

    pub fn replace_channels(&mut self, channels: Vec<(String, String)>) {
        let previous = self.selected_channel_name().map(str::to_string);
        self.channels = channels
            .into_iter()
            .map(|(name, topic)| ChannelRow {
                name,
                selected: false,
                topic,
            })
            .collect();

        if self.channels.is_empty() {
            self.selected_channel_index = 0;
            return;
        }

        let selected_index = previous
            .as_deref()
            .and_then(|name| {
                self.channels
                    .iter()
                    .position(|row| row.name.eq_ignore_ascii_case(name))
            })
            .unwrap_or(0);
        self.selected_channel_index = selected_index;
        for (idx, row) in self.channels.iter_mut().enumerate() {
            row.selected = idx == selected_index;
        }
    }

    pub fn replace_contacts(&mut self, contacts: Vec<(String, bool)>) {
        let previous = self.selected_contact_name().map(str::to_string);
        self.contacts = contacts
            .into_iter()
            .map(|(name, is_guardian)| ContactRow {
                name,
                selected: false,
                is_guardian,
            })
            .collect();

        if self.contacts.is_empty() {
            self.selected_contact_index = 0;
            return;
        }

        let selected_index = previous
            .as_deref()
            .and_then(|name| {
                self.contacts
                    .iter()
                    .position(|row| row.name.eq_ignore_ascii_case(name))
            })
            .unwrap_or(0);
        self.set_selected_contact_index(selected_index);
    }

    pub fn replace_authorities(&mut self, authorities: Vec<(String, String, bool)>) {
        let previous = self
            .authorities
            .get(self.selected_authority_index)
            .map(|row| row.id.clone());
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
            self.selected_authority_index = 0;
            return;
        }

        let selected_index = previous
            .as_deref()
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
            .get(self.selected_channel_index)
            .map(|row| row.topic.as_str())
            .unwrap_or("")
    }

    pub fn set_selected_channel_topic(&mut self, value: String) {
        if let Some(channel) = self.channels.get_mut(self.selected_channel_index) {
            channel.topic = value;
        }
    }

    pub fn move_channel_selection(&mut self, delta: i32) {
        if self.channels.is_empty() {
            return;
        }
        let max = self.channels.len() as i32 - 1;
        let mut next = self.selected_channel_index as i32 + delta;
        if next < 0 {
            next = max;
        }
        if next > max {
            next = 0;
        }
        self.selected_channel_index = next as usize;
        for (idx, row) in self.channels.iter_mut().enumerate() {
            row.selected = idx == self.selected_channel_index;
        }
    }

    pub fn reset_create_channel_wizard(&mut self) {
        self.create_channel_step = CreateChannelWizardStep::Details;
        self.create_channel_active_field = CreateChannelDetailsField::Name;
        self.create_channel_member_focus = 0;
        self.create_channel_selected_members.clear();
        self.create_channel_name.clear();
        self.create_channel_topic.clear();
        self.create_channel_threshold = 1;
    }

    pub fn reset_add_device_wizard(&mut self) {
        self.add_device_step = AddDeviceWizardStep::Name;
        self.add_device_name.clear();
        self.add_device_enrollment_code.clear();
        self.add_device_code_copied = false;
        self.add_device_ceremony_id = None;
        self.add_device_accepted_count = 0;
        self.add_device_total_count = 0;
        self.add_device_threshold = 0;
        self.add_device_is_complete = false;
        self.add_device_has_failed = false;
        self.add_device_error_message = None;
    }

    pub fn reset_guardian_wizard(&mut self) {
        self.guardian_wizard_step = ThresholdWizardStep::Selection;
        self.guardian_focus_index = 0;
        self.guardian_selected_indices.clear();
        self.guardian_selected_count = 2;
        self.guardian_threshold_k = 2;
    }

    pub fn reset_mfa_wizard(&mut self) {
        self.mfa_wizard_step = ThresholdWizardStep::Selection;
        self.mfa_focus_index = 0;
        self.mfa_selected_indices.clear();
        self.mfa_selected_count = 1;
        self.mfa_threshold_k = 1;
    }

    pub fn reset_remove_device_flow(&mut self) {
        self.remove_device_candidate_name.clear();
    }

    pub fn reset_capability_config_editor(&mut self) {
        self.capability_full_caps = DEFAULT_CAPABILITY_FULL.to_string();
        self.capability_partial_caps = DEFAULT_CAPABILITY_PARTIAL.to_string();
        self.capability_limited_caps = DEFAULT_CAPABILITY_LIMITED.to_string();
        self.capability_active_field = 0;
    }

    pub fn reset_access_override_editor(&mut self) {
        self.access_override_partial = false;
    }

    pub fn secondary_device_name(&self) -> Option<&str> {
        self.secondary_device_name.as_deref()
    }

    pub fn set_secondary_device_name(&mut self, value: Option<String>) {
        self.secondary_device_name = value;
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
    model: AsyncRwLock<UiModel>,
    clipboard: Arc<dyn ClipboardPort>,
    authority_switcher: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

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
    model.modal = None;
    model.modal_buffer.clear();
    model.modal_hint.clear();
    model.reset_create_channel_wizard();
    model.reset_add_device_wizard();
    model.reset_guardian_wizard();
    model.reset_mfa_wizard();
    model.reset_remove_device_flow();
    model.reset_capability_config_editor();
    model.reset_access_override_editor();
}

impl UiController {
    pub fn new(app_core: Arc<AsyncRwLock<AppCore>>, clipboard: Arc<dyn ClipboardPort>) -> Self {
        Self::with_authority_switcher(app_core, clipboard, None)
    }

    pub fn with_authority_switcher(
        app_core: Arc<AsyncRwLock<AppCore>>,
        clipboard: Arc<dyn ClipboardPort>,
        authority_switcher: Option<Arc<dyn Fn(String) + Send + Sync>>,
    ) -> Self {
        let authority_id = app_core
            .try_read()
            .and_then(|core| core.authority().cloned())
            .map(|id| id.to_string())
            .unwrap_or_else(|| "authority-local".to_string());

        Self {
            app_core,
            model: AsyncRwLock::new(UiModel::new(authority_id)),
            clipboard,
            authority_switcher,
        }
    }

    pub fn send_keys(&self, keys: &str) {
        let mut model = write_model(&self.model);
        apply_text_keys(&mut model, keys, self.clipboard.as_ref());
    }

    pub fn send_action_keys(&self, keys: &str) {
        let mut model = write_model(&self.model);
        model.input_mode = false;
        model.input_buffer.clear();
        apply_text_keys(&mut model, keys, self.clipboard.as_ref());
    }

    pub fn send_key_named(&self, key: &str, repeat: u16) {
        let mut model = write_model(&self.model);
        apply_named_key(&mut model, key, repeat, self.clipboard.as_ref());
    }

    pub fn set_screen(&self, screen: UiScreen) {
        write_model(&self.model).set_screen(screen);
    }

    pub fn select_channel_by_name(&self, name: &str) {
        write_model(&self.model).select_channel_by_name(name);
    }

    pub fn select_home_by_name(&self, name: &str) {
        write_model(&self.model).select_home_by_name(name);
    }

    pub fn set_modal_buffer(&self, value: &str) {
        write_model(&self.model).modal_buffer = value.to_string();
    }

    pub fn clear_input_buffer(&self) {
        write_model(&self.model).input_buffer.clear();
    }

    pub fn set_selected_contact_index(&self, index: usize) {
        write_model(&self.model).set_selected_contact_index(index);
    }

    pub fn set_selected_authority_index(&self, index: usize) {
        write_model(&self.model).set_selected_authority_index(index);
    }

    pub fn set_selected_neighborhood_member_index(&self, index: usize) {
        write_model(&self.model).set_selected_neighborhood_member_index(index);
    }

    pub fn sync_runtime_channels(&self, channels: Vec<(String, String)>) {
        write_model(&self.model).replace_channels(channels);
    }

    pub fn sync_runtime_contacts(&self, contacts: Vec<(String, bool)>) {
        write_model(&self.model).replace_contacts(contacts);
    }

    pub fn sync_runtime_authorities(&self, authorities: Vec<(String, String, bool)>) {
        write_model(&self.model).replace_authorities(authorities);
    }

    pub fn sync_runtime_profile(&self, authority_id: String, nickname: String) {
        write_model(&self.model).sync_profile(authority_id, nickname);
    }

    pub fn sync_runtime_devices(&self, devices: Vec<(String, bool)>) {
        write_model(&self.model).sync_devices(devices);
    }

    pub fn request_authority_switch(&self, authority_id: &str) -> bool {
        if let Some(switcher) = &self.authority_switcher {
            switcher(authority_id.to_string());
            true
        } else {
            false
        }
    }

    pub fn complete_runtime_home_created(&self, name: &str) {
        let mut model = write_model(&self.model);
        model.selected_home = Some(name.to_string());
        model.access_depth = AccessDepth::Full;
        model.neighborhood_mode = NeighborhoodMode::Map;
        set_toast(&mut model, '✓', format!("Home '{name}' created"));
        dismiss_modal(&mut model);
    }

    pub fn complete_runtime_invitation_import(&self) {
        let mut model = write_model(&self.model);
        set_toast(&mut model, '✓', "Invitation imported");
        dismiss_modal(&mut model);
    }

    pub fn complete_runtime_modal_success(&self, message: impl Into<String>) {
        let mut model = write_model(&self.model);
        set_toast(&mut model, '✓', message);
        dismiss_modal(&mut model);
    }

    pub fn complete_runtime_device_enrollment_started(&self, name: &str, enrollment_code: &str) {
        let mut model = write_model(&self.model);
        model.add_device_name = name.to_string();
        model.add_device_enrollment_code = enrollment_code.to_string();
        model.add_device_code_copied = false;
        model.add_device_step = AddDeviceWizardStep::ShareCode;
        model.modal = Some(ModalState::AddDeviceStep1);
        model.modal_buffer.clear();
        model.modal_hint = "Add Device — Step 2 of 3".to_string();
    }

    pub fn set_runtime_device_enrollment_ceremony_id(&self, ceremony_id: &str) {
        let mut model = write_model(&self.model);
        model.add_device_ceremony_id = Some(ceremony_id.to_string());
    }

    pub fn update_runtime_device_enrollment_status(
        &self,
        accepted_count: u16,
        total_count: u16,
        threshold: u16,
        is_complete: bool,
        has_failed: bool,
        error_message: Option<String>,
    ) {
        let mut model = write_model(&self.model);
        model.add_device_accepted_count = accepted_count;
        model.add_device_total_count = total_count;
        model.add_device_threshold = threshold;
        model.add_device_is_complete = is_complete;
        model.add_device_has_failed = has_failed;
        model.add_device_error_message = error_message;
        if is_complete {
            model.has_secondary_device = true;
            if model.secondary_device_name.is_none() && !model.add_device_name.trim().is_empty() {
                model.secondary_device_name = Some(model.add_device_name.clone());
            }
        }
    }

    pub fn mark_add_device_code_copied(&self) {
        write_model(&self.model).add_device_code_copied = true;
    }

    pub fn advance_runtime_device_enrollment_share(&self) {
        let mut model = write_model(&self.model);
        model.add_device_step = AddDeviceWizardStep::Confirm;
        model.modal = Some(ModalState::AddDeviceStep1);
        model.modal_buffer.clear();
        model.modal_hint = "Add Device — Step 3 of 3".to_string();
    }

    pub fn complete_runtime_device_enrollment_ready(&self) {
        let mut model = write_model(&self.model);
        dismiss_modal(&mut model);
    }

    pub fn complete_runtime_enter_home(&self, name: &str, depth: AccessDepth) {
        let mut model = write_model(&self.model);
        model.selected_home = Some(name.to_string());
        model.access_depth = depth;
        model.neighborhood_mode = NeighborhoodMode::Detail;
        model.selected_neighborhood_member_index = 0;
    }

    pub fn runtime_error_toast(&self, message: impl Into<String>) {
        let mut model = write_model(&self.model);
        set_toast(&mut model, '✗', message);
    }

    pub fn info_toast(&self, message: impl Into<String>) {
        let mut model = write_model(&self.model);
        set_toast(&mut model, '✓', message);
    }

    pub fn snapshot(&self) -> RenderedHarnessSnapshot {
        let screen = render_canonical_snapshot(&read_model(&self.model));
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

    pub fn set_authority_id(&self, authority_id: &str) {
        write_model(&self.model).authority_id = authority_id.to_string();
    }

    pub fn set_settings_index(&self, index: usize) {
        write_model(&self.model).settings_index = index;
    }

    pub fn authority_id(&self) -> String {
        read_model(&self.model).authority_id.clone()
    }

    pub fn ui_model(&self) -> Option<UiModel> {
        Some(read_model(&self.model).clone())
    }

    pub fn app_core(&self) -> &Arc<AsyncRwLock<AppCore>> {
        &self.app_core
    }
}

fn read_model(model: &AsyncRwLock<UiModel>) -> async_lock::RwLockReadGuard<'_, UiModel> {
    loop {
        if let Some(guard) = model.try_read() {
            return guard;
        }
        std::hint::spin_loop();
    }
}

fn write_model(model: &AsyncRwLock<UiModel>) -> async_lock::RwLockWriteGuard<'_, UiModel> {
    loop {
        if let Some(guard) = model.try_write() {
            return guard;
        }
        std::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::{NeighborhoodMode, UiModel, UiScreen};

    #[test]
    fn set_screen_clears_input_mode_and_buffer() {
        let mut model = UiModel::new("authority-local".to_string());
        model.input_mode = true;
        model.input_buffer = "pending text".to_string();

        model.set_screen(UiScreen::Settings);

        assert!(!model.input_mode);
        assert!(model.input_buffer.is_empty());
        assert!(matches!(model.screen, UiScreen::Settings));
    }

    #[test]
    fn entering_neighborhood_screen_resets_to_map_mode() {
        let mut model = UiModel::new("authority-local".to_string());
        model.neighborhood_mode = NeighborhoodMode::Detail;

        model.set_screen(UiScreen::Neighborhood);

        assert!(matches!(model.neighborhood_mode, NeighborhoodMode::Map));
    }
}
