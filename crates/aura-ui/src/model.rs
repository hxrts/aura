use crate::clipboard::ClipboardPort;
use crate::keyboard::{apply_named_key, apply_text_keys};
use crate::snapshot::render_canonical_snapshot;
use async_lock::RwLock;
use aura_app::AppCore;
use std::sync::{Arc, RwLock as StdRwLock};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiScreen {
    Neighborhood,
    Chat,
    Contacts,
    Notifications,
    Settings,
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
    AddDeviceStep1,
    ImportDeviceEnrollmentCode,
    AssignModerator,
    AccessOverride,
    CapabilityConfig,
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
    pub input_mode: bool,
    pub input_buffer: String,
    pub modal: Option<ModalState>,
    pub modal_buffer: String,
    pub modal_hint: String,
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
            channels: vec![ChannelRow {
                name: "general".to_string(),
                selected: true,
                topic: "bootstrap-topic".to_string(),
            }],
            contacts: Vec::new(),
            messages: Vec::new(),
            notifications: Vec::new(),
            logs: vec!["Aura web shell initialized".to_string()],
            toast: None,
            input_mode: false,
            input_buffer: String::new(),
            modal: None,
            modal_buffer: String::new(),
            modal_hint: String::new(),
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
}

#[derive(Debug, Clone)]
pub struct RenderedHarnessSnapshot {
    pub screen: String,
    pub authoritative_screen: String,
    pub normalized_screen: String,
    pub raw_screen: String,
}

pub struct UiController {
    app_core: Arc<RwLock<AppCore>>,
    model: StdRwLock<UiModel>,
    clipboard: Arc<dyn ClipboardPort>,
}

impl PartialEq for UiController {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for UiController {}

impl UiController {
    pub fn new(app_core: Arc<RwLock<AppCore>>, clipboard: Arc<dyn ClipboardPort>) -> Self {
        let authority_id = app_core
            .try_read()
            .and_then(|core| core.authority().cloned())
            .map(|id| id.to_string())
            .unwrap_or_else(|| format!("authority-{}", Uuid::new_v4()));

        Self {
            app_core,
            model: StdRwLock::new(UiModel::new(authority_id)),
            clipboard,
        }
    }

    pub fn send_keys(&self, keys: &str) {
        if let Ok(mut model) = self.model.write() {
            apply_text_keys(&mut model, keys, self.clipboard.as_ref());
        }
    }

    pub fn send_key_named(&self, key: &str, repeat: u16) {
        if let Ok(mut model) = self.model.write() {
            apply_named_key(&mut model, key, repeat, self.clipboard.as_ref());
        }
    }

    pub fn snapshot(&self) -> RenderedHarnessSnapshot {
        let screen = if let Ok(model) = self.model.read() {
            render_canonical_snapshot(&model)
        } else {
            String::new()
        };
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
        if let Ok(model) = self.model.read() {
            let mut output = model.logs.clone();
            if output.len() > lines {
                output = output.split_off(output.len() - lines);
            }
            return output;
        }
        Vec::new()
    }

    pub fn inject_message(&self, message: &str) {
        if let Ok(mut model) = self.model.write() {
            model.messages.push(message.to_string());
        }
    }

    pub fn push_log(&self, line: &str) {
        if let Ok(mut model) = self.model.write() {
            model.logs.push(line.to_string());
        }
    }

    pub fn set_authority_id(&self, authority_id: &str) {
        if let Ok(mut model) = self.model.write() {
            model.authority_id = authority_id.to_string();
        }
    }

    pub fn authority_id(&self) -> String {
        if let Ok(model) = self.model.read() {
            return model.authority_id.clone();
        }
        String::new()
    }

    pub fn ui_model(&self) -> Option<UiModel> {
        self.model.read().ok().map(|model| model.clone())
    }

    pub fn app_core(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
    }
}
