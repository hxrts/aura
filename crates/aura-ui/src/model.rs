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
    AssignModerator,
    AccessOverride,
    CapabilityConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateChannelWizardStep {
    Name,
    Topic,
    InviteContacts,
    Threshold,
}

#[derive(Debug, Clone)]
pub struct UiModel {
    pub screen: UiScreen,
    pub settings_index: usize,
    pub channels: Vec<ChannelRow>,
    pub contacts: Vec<ContactRow>,
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
    pub create_channel_name: String,
    pub create_channel_topic: String,
    pub create_channel_invitee: String,
    pub create_channel_threshold: u8,
    pub selected_home: Option<String>,
    pub neighborhood_mode: NeighborhoodMode,
    pub access_depth: AccessDepth,
    pub authority_id: String,
    pub profile_nickname: String,
    pub invite_counter: u64,
    pub last_invite_code: Option<String>,
    pub last_scan: String,
    pub selected_contact_index: usize,
    pub selected_channel_index: usize,
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
            create_channel_step: CreateChannelWizardStep::Name,
            create_channel_name: String::new(),
            create_channel_topic: String::new(),
            create_channel_invitee: String::new(),
            create_channel_threshold: 1,
            selected_home: None,
            neighborhood_mode: NeighborhoodMode::Map,
            access_depth: AccessDepth::Limited,
            authority_id,
            profile_nickname: "Ops".to_string(),
            invite_counter: 0,
            last_invite_code: None,
            last_scan: "never".to_string(),
            selected_contact_index: 0,
            selected_channel_index: 0,
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
        self.create_channel_step = CreateChannelWizardStep::Name;
        self.create_channel_name.clear();
        self.create_channel_topic.clear();
        self.create_channel_invitee.clear();
        self.create_channel_threshold = 1;
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
}

impl PartialEq for UiController {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for UiController {}

impl UiController {
    pub fn new(app_core: Arc<AsyncRwLock<AppCore>>, clipboard: Arc<dyn ClipboardPort>) -> Self {
        let authority_id = app_core
            .try_read()
            .and_then(|core| core.authority().cloned())
            .map(|id| id.to_string())
            .unwrap_or_else(|| "authority-local".to_string());

        Self {
            app_core,
            model: AsyncRwLock::new(UiModel::new(authority_id)),
            clipboard,
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

    pub fn set_modal_buffer(&self, value: &str) {
        write_model(&self.model).modal_buffer = value.to_string();
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
