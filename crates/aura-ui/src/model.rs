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

#[derive(Debug, Clone)]
pub struct ChannelRow {
    pub name: String,
    pub selected: bool,
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

#[derive(Debug, Clone)]
pub enum ModalState {
    CreateInvitation,
    AcceptInvitation,
    CreateHome,
}

#[derive(Debug, Clone)]
pub struct UiModel {
    pub screen: UiScreen,
    pub settings_index: usize,
    pub channels: Vec<ChannelRow>,
    pub contacts: Vec<ContactRow>,
    pub messages: Vec<String>,
    pub logs: Vec<String>,
    pub toast: Option<ToastState>,
    pub input_mode: bool,
    pub input_buffer: String,
    pub modal: Option<ModalState>,
    pub modal_buffer: String,
    pub selected_home: Option<String>,
    pub authority_id: String,
    pub invite_counter: u64,
    pub last_invite_code: Option<String>,
    pub selected_contact_index: usize,
}

impl UiModel {
    pub fn new(authority_id: String) -> Self {
        Self {
            screen: UiScreen::Neighborhood,
            settings_index: 0,
            channels: vec![ChannelRow {
                name: "general".to_string(),
                selected: true,
            }],
            contacts: Vec::new(),
            messages: Vec::new(),
            logs: vec!["Aura web shell initialized".to_string()],
            toast: None,
            input_mode: false,
            input_buffer: String::new(),
            modal: None,
            modal_buffer: String::new(),
            selected_home: None,
            authority_id,
            invite_counter: 0,
            last_invite_code: None,
            selected_contact_index: 0,
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
            });
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
        let repeat = repeat.max(1);
        if let Ok(mut model) = self.model.write() {
            for _ in 0..repeat {
                apply_named_key(&mut model, key, self.clipboard.as_ref());
            }
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

    pub fn app_core(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
    }
}
