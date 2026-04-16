//! UI state model and controller for the Aura web interface.
//!
//! Defines the core UI model (screens, selections, modals, toasts) and the
//! controller that bridges the model to the application core and input handlers.

#![allow(clippy::disallowed_types)]

mod modal;
mod modal_fields;
mod operations;
mod runtime_events;
mod settings;
mod snapshot;

use crate::keyboard::{apply_named_key, apply_text_keys};
use crate::readiness_owner;
use crate::snapshot::render_canonical_snapshot;
use async_lock::RwLock as AsyncRwLock;
use aura_app::frontend_primitives::ClipboardPort;
use aura_app::ui_contract::{
    next_projection_revision, InvitationFactKind, ProjectionRevision, QuiescenceSnapshot,
    RuntimeFact, SemanticOperationCausality, SemanticOperationPhase, SemanticOperationStatus,
};
use aura_app::{
    ui::contract::{
        ConfirmationState, ControlId, FieldId, ListId, ListItemSnapshot, ListSnapshot,
        MessageSnapshot, ModalId, OperationId, OperationInstanceId, OperationSnapshot,
        OperationState, RuntimeEventId, RuntimeEventSnapshot, SelectionSnapshot, ToastId,
        ToastKind, ToastSnapshot, UiSnapshot,
    },
    ui::types::ContactRelationshipState,
    AppCore,
};
use aura_core::types::identifiers::AuthorityId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub use aura_app::ui::contract::ScreenId;
pub use modal::{
    AccessOverrideModalState, ActiveModal, AddDeviceModalState, AddDeviceWizardStep,
    CapabilityConfigModalState, CreateChannelDetailsField, CreateChannelModalState,
    CreateChannelWizardStep, CreateInvitationModalState, EditChannelInfoModalState, ModalState,
    SelectDeviceModalState, TextModalState, ThresholdWizardModalState, ThresholdWizardStep,
};
pub use settings::{
    AccessOverrideLevel, CapabilityTier, SettingsSection, DEFAULT_CAPABILITY_FULL,
    DEFAULT_CAPABILITY_LIMITED, DEFAULT_CAPABILITY_PARTIAL,
};
pub use snapshot::RenderedHarnessSnapshot;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoContactShortcuts {
    pub alice_invite_code: String,
    pub carol_invite_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoDeviceShortcut {
    pub name: String,
    pub invitee_authority_id: AuthorityId,
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
    pub id: String,
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
    pub relationship_state: ContactRelationshipState,
    pub confirmation: ConfirmationState,
    /// Invitation code used to establish this contact, if known.
    ///
    /// Populated by the outbound create-invitation flow (with the generated
    /// code) and the inbound accept flow (with the pasted code). Session
    /// state only — not yet persisted across restarts. Preserved across
    /// projections from the authoritative runtime contacts view.
    pub invitation_code: Option<String>,
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

#[derive(Debug, Clone)]
pub struct UiModel {
    pub semantic_revision: ProjectionRevision,
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
    pub operation_causalities: HashMap<OperationId, Option<SemanticOperationCausality>>,
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
    pub demo_contact_shortcuts: Option<DemoContactShortcuts>,
    pub demo_device_shortcut: Option<DemoDeviceShortcut>,
}

macro_rules! modal_state_accessors {
    ($(($ref_name:ident, $mut_name:ident, $accessor:ident, $accessor_mut:ident, $state:ty)),+ $(,)?) => {
        $(
            #[must_use]
            pub fn $ref_name(&self) -> Option<&$state> {
                self.active_modal.as_ref().and_then(ActiveModal::$accessor)
            }

            pub fn $mut_name(&mut self) -> Option<&mut $state> {
                self.active_modal.as_mut().and_then(ActiveModal::$accessor_mut)
            }
        )+
    };
}

impl UiModel {
    pub fn new(authority_id: String) -> Self {
        Self {
            semantic_revision: next_projection_revision(None),
            account_ready: true,
            account_setup_name: String::new(),
            account_setup_error: None,
            screen: ScreenId::Neighborhood,
            settings_section: SettingsSection::Profile,
            channels: Vec::new(),
            contacts: Vec::new(),
            authorities: Vec::new(),
            messages: Vec::new(),
            notifications: Vec::new(),
            notification_ids: Vec::new(),
            logs: vec!["Aura web shell initialized".to_string()],
            operations: Vec::new(),
            operation_causalities: HashMap::new(),
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
            selected_channel: None,
            selected_neighborhood_member_key: None,
            selected_notification_id: None,
            contact_details: false,
            demo_contact_shortcuts: None,
            demo_device_shortcut: None,
        }
    }

    fn advance_semantic_revision(&mut self) {
        self.semantic_revision = next_projection_revision(None);
    }

    pub fn selected_channel_name(&self) -> Option<&str> {
        let selected_id = self.selected_channel.as_deref()?;
        self.channels
            .iter()
            .find(|row| row.id == selected_id)
            .map(|row| row.name.as_str())
    }

    pub fn selected_channel_id(&self) -> Option<&str> {
        self.selected_channel.as_deref()
    }

    fn canonical_ready_screen(screen: ScreenId) -> ScreenId {
        match screen {
            ScreenId::Onboarding => ScreenId::Neighborhood,
            other => other,
        }
    }

    pub fn set_screen(&mut self, screen: ScreenId) {
        let screen = if self.account_ready {
            Self::canonical_ready_screen(screen)
        } else {
            screen
        };
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
        self.active_modal
            .as_ref()
            .and_then(ActiveModal::create_invitation)
    }

    modal_state_accessors!(
        (
            create_channel_modal,
            create_channel_modal_mut,
            create_channel,
            create_channel_mut,
            CreateChannelModalState
        ),
        (
            add_device_modal,
            add_device_modal_mut,
            add_device,
            add_device_mut,
            AddDeviceModalState
        ),
        (
            guardian_setup_modal,
            guardian_setup_modal_mut,
            guardian_setup,
            guardian_setup_mut,
            ThresholdWizardModalState
        ),
        (
            mfa_setup_modal,
            mfa_setup_modal_mut,
            mfa_setup,
            mfa_setup_mut,
            ThresholdWizardModalState
        ),
        (
            capability_config_modal,
            capability_config_modal_mut,
            capability_config,
            capability_config_mut,
            CapabilityConfigModalState
        ),
        (
            access_override_modal,
            access_override_modal_mut,
            access_override,
            access_override_mut,
            AccessOverrideModalState
        ),
        (
            edit_channel_info,
            edit_channel_info_mut,
            edit_channel_info,
            edit_channel_info_mut,
            EditChannelInfoModalState
        )
    );

    #[must_use]
    pub fn selected_device_modal(&self) -> Option<&SelectDeviceModalState> {
        self.active_modal
            .as_ref()
            .and_then(ActiveModal::selected_device)
    }

    #[must_use]
    pub fn modal_field_id(&self) -> Option<FieldId> {
        self.active_modal
            .as_ref()
            .and_then(ActiveModal::field_descriptor)
            .map(|field| field.field_id())
    }

    #[must_use]
    pub fn modal_text_value(&self) -> Option<String> {
        self.active_modal.as_ref().and_then(ActiveModal::text_value)
    }

    #[must_use]
    pub fn modal_text_value_or_empty(&self) -> String {
        self.modal_text_value().unwrap_or_default()
    }

    pub fn set_modal_text_value(&mut self, value: impl Into<String>) {
        let Some(active_modal) = self.active_modal.as_mut() else {
            return;
        };
        active_modal.set_text_value(value.into());
    }

    pub fn set_modal_field_value(&mut self, field_id: FieldId, value: impl Into<String>) {
        let Some(active_modal) = self.active_modal.as_mut() else {
            return;
        };
        active_modal.set_field_value(field_id, value.into());
    }

    pub fn set_modal_active_field(&mut self, field_id: FieldId) {
        let Some(active_modal) = self.active_modal.as_mut() else {
            return;
        };
        active_modal.set_active_field(field_id);
    }

    pub fn append_modal_text_char(&mut self, ch: char) {
        let mut value = self.modal_text_value_or_empty();
        value.push(ch);
        self.set_modal_text_value(value);
    }

    pub fn pop_modal_text_char(&mut self) {
        let mut value = self.modal_text_value_or_empty();
        value.pop();
        self.set_modal_text_value(value);
    }

    #[must_use]
    pub fn modal_accepts_text(&self) -> bool {
        self.active_modal
            .as_ref()
            .is_some_and(ActiveModal::accepts_text)
    }

    pub fn dismiss_modal(&mut self) {
        self.modal_hint.clear();
        self.active_modal = None;
    }

    pub fn select_channel_id(&mut self, id: Option<&str>) {
        self.selected_channel = id.map(ToString::to_string);
        for row in &mut self.channels {
            row.selected = Some(row.id.as_str()) == id;
        }
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
            relationship_state: ContactRelationshipState::Contact,
            confirmation: ConfirmationState::Confirmed,
            invitation_code: None,
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
        relationship_state: ContactRelationshipState,
        invitation_code: Option<String>,
    ) {
        if let Some(existing) = self
            .contacts
            .iter_mut()
            .find(|row| row.authority_id == authority_id)
        {
            existing.name = name;
            existing.is_guardian = is_guardian;
            existing.relationship_state = relationship_state;
            existing.confirmation = match relationship_state {
                ContactRelationshipState::PendingOutbound => ConfirmationState::PendingLocal,
                _ => ConfirmationState::Confirmed,
            };
            if let Some(code) = invitation_code {
                existing.invitation_code = Some(code);
            }
            return;
        }

        self.contacts.push(ContactRow {
            authority_id,
            name,
            selected: self.contacts.is_empty(),
            is_guardian,
            relationship_state,
            confirmation: match relationship_state {
                ContactRelationshipState::PendingOutbound => ConfirmationState::PendingLocal,
                _ => ConfirmationState::Confirmed,
            },
            invitation_code,
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

    pub fn selected_authority_id(&self) -> Option<AuthorityId> {
        self.selected_authority_id
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

    pub fn replace_channels(&mut self, channels: Vec<(String, String, String)>) {
        let previous = self.selected_channel.clone();
        self.channels = channels
            .into_iter()
            .map(|(id, name, topic)| ChannelRow {
                id,
                name,
                selected: false,
                topic,
            })
            .collect();

        if self.channels.is_empty() {
            self.selected_channel = None;
            return;
        }

        let selected_id = previous
            .and_then(|id| {
                self.channels
                    .iter()
                    .find(|row| row.id == id)
                    .map(|row| row.id.clone())
            })
            .unwrap_or_else(|| self.channels[0].id.clone());
        self.select_channel_id(Some(&selected_id));
    }

    pub fn replace_contacts(
        &mut self,
        contacts: Vec<(
            AuthorityId,
            String,
            bool,
            ContactRelationshipState,
            Option<String>,
        )>,
    ) {
        let previous = self.selected_contact_id;
        // Preserve invitation_code from the previous list as a fallback —
        // outbound/inbound UI flows may populate the code session-locally
        // before the authoritative fact has been projected into the view.
        // Prefer the projection's value when present.
        let previous_invitation_codes: std::collections::HashMap<AuthorityId, String> = self
            .contacts
            .iter()
            .filter_map(|row| {
                row.invitation_code
                    .as_ref()
                    .map(|code| (row.authority_id, code.clone()))
            })
            .collect();
        self.contacts = contacts
            .into_iter()
            .map(
                |(authority_id, name, is_guardian, relationship_state, invitation_code)| {
                    ContactRow {
                        authority_id,
                        name,
                        selected: false,
                        is_guardian,
                        relationship_state,
                        confirmation: match relationship_state {
                            ContactRelationshipState::PendingOutbound => {
                                ConfirmationState::PendingLocal
                            }
                            _ => ConfirmationState::Confirmed,
                        },
                        invitation_code: invitation_code
                            .or_else(|| previous_invitation_codes.get(&authority_id).cloned()),
                    }
                },
            )
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
            .find(|row| Some(row.id.as_str()) == self.selected_channel_id())
            .map(|row| row.topic.as_str())
            .unwrap_or("")
    }

    pub fn set_selected_channel_topic(&mut self, value: String) {
        let selected_id = self.selected_channel.clone();
        if let Some(channel) = selected_id
            .as_deref()
            .and_then(|id| self.channels.iter_mut().find(|row| row.id == id))
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
            .selected_channel_id()
            .and_then(|id| self.channels.iter().position(|row| row.id == id))
            .unwrap_or_default();
        let mut next = current_index as i32 + delta;
        if next < 0 {
            next = max;
        }
        if next > max {
            next = 0;
        }
        let selected_id = self.channels[next as usize].id.clone();
        self.select_channel_id(Some(&selected_id));
    }

    pub fn secondary_device_name(&self) -> Option<&str> {
        self.secondary_device_name.as_deref()
    }

    pub fn demo_contact_shortcuts(&self) -> Option<&DemoContactShortcuts> {
        self.demo_contact_shortcuts.as_ref()
    }

    pub fn demo_device_shortcut(&self) -> Option<&DemoDeviceShortcut> {
        self.demo_device_shortcut.as_ref()
    }

    pub fn configure_demo_contact_shortcuts(
        &mut self,
        alice_invite_code: impl Into<String>,
        carol_invite_code: impl Into<String>,
    ) {
        self.demo_contact_shortcuts = Some(DemoContactShortcuts {
            alice_invite_code: alice_invite_code.into(),
            carol_invite_code: carol_invite_code.into(),
        });
    }

    pub fn configure_demo_device_shortcut(
        &mut self,
        name: impl Into<String>,
        invitee_authority_id: AuthorityId,
    ) {
        self.demo_device_shortcut = Some(DemoDeviceShortcut {
            name: name.into(),
            invitee_authority_id,
        });
    }

    pub fn demo_device_invitee_authority_id(&self, device_name: &str) -> Option<AuthorityId> {
        self.demo_device_shortcut.as_ref().and_then(|shortcut| {
            shortcut
                .name
                .eq_ignore_ascii_case(device_name)
                .then_some(shortcut.invitee_authority_id)
        })
    }

    pub fn set_secondary_device_name(&mut self, value: Option<String>) {
        self.secondary_device_name = value;
    }
}

pub struct UiController {
    app_core: Arc<AsyncRwLock<AppCore>>,
    model: RwLock<UiModel>,
    clipboard: Arc<dyn ClipboardPort>,
    authority_switcher: Option<Arc<dyn Fn(AuthorityId) + Send + Sync>>,
    ui_snapshot_sink: Mutex<Option<UiSnapshotSink>>,
    last_published_ui_snapshot: Mutex<Option<UiSnapshot>>,
    rerender: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    runtime_device_enrollment_ceremony:
        Mutex<Option<runtime_events::RuntimeDeviceEnrollmentCeremony>>,
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
            last_published_ui_snapshot: Mutex::new(None),
            rerender: Mutex::new(None),
            runtime_device_enrollment_ceremony: Mutex::new(None),
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

    pub fn select_channel_by_id(&self, id: &str) {
        write_model(&self.model).select_channel_id(Some(id));
        self.request_rerender();
    }

    pub fn selected_channel_id(&self) -> Option<String> {
        read_model(&self.model)
            .selected_channel_id()
            .map(str::to_string)
    }

    pub fn selected_authority_id(&self) -> Option<AuthorityId> {
        read_model(&self.model).selected_authority_id()
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

    pub fn open_create_invitation_modal(
        &self,
        _receiver_id: Option<&AuthorityId>,
        _receiver_label: Option<&str>,
    ) {
        let mut model = write_model(&self.model);
        model.clear_operation(&OperationId::invitation_create());
        model.last_invite_code = None;
        model.active_modal = Some(ActiveModal::CreateInvitation(CreateInvitationModalState {
            message: String::new(),
            ttl_hours: 24,
            active_field: FieldId::InvitationMessage,
        }));
        drop(model);
        self.request_rerender();
    }

    pub fn request_authority_switch(&self, authority_id: AuthorityId) -> bool {
        if let Some(switcher) = &self.authority_switcher {
            switcher(authority_id);
            true
        } else {
            false
        }
    }

    pub fn read_clipboard(&self) -> String {
        self.clipboard.read()
    }

    pub fn write_clipboard(&self, text: &str) {
        self.clipboard.write(text);
    }

    pub fn remember_invitation_code(&self, code: &str) {
        let mut model = write_model(&self.model);
        model.last_invite_code = Some(code.to_string());
        drop(model);
        self.request_rerender();
    }

    /// Record the invitation code associated with a specific contact.
    ///
    /// Called by the outbound create-invitation flow with the target
    /// authority and the generated code, and by the inbound accept flow
    /// with the sender authority and the pasted code. Updates the contact
    /// row if it exists; contacts added later will pick up the code via
    /// `ensure_runtime_contact`. Phase 2 session-scoped; Phase 3 will
    /// persist it.
    pub fn set_contact_invitation_code(&self, authority_id: AuthorityId, code: String) {
        self.try_update_model(|model| {
            if let Some(row) = model
                .contacts
                .iter_mut()
                .find(|row| row.authority_id == authority_id)
            {
                row.invitation_code = Some(code);
            }
        });
        self.request_rerender();
    }

    pub fn close_modal(&self) {
        let mut model = write_model(&self.model);
        model.dismiss_modal();
        drop(model);
        self.request_rerender();
    }

    pub fn toggle_selectable_item(&self, index: usize) {
        let mut model = write_model(&self.model);
        match model.active_modal.as_mut() {
            Some(ActiveModal::CreateChannel(state)) => {
                if let Some(position) = state
                    .selected_members
                    .iter()
                    .position(|selected| *selected == index)
                {
                    state.selected_members.remove(position);
                } else {
                    state.selected_members.push(index);
                    state.selected_members.sort_unstable();
                }
            }
            Some(ActiveModal::GuardianSetup(state) | ActiveModal::MfaSetup(state)) => {
                if let Some(position) = state
                    .selected_indices
                    .iter()
                    .position(|selected| *selected == index)
                {
                    state.selected_indices.remove(position);
                } else {
                    state.selected_indices.push(index);
                    state.selected_indices.sort_unstable();
                }
            }
            _ => {}
        }
        drop(model);
        self.request_rerender();
    }

    pub fn configure_demo_contact_shortcuts(
        &self,
        alice_invite_code: impl Into<String>,
        carol_invite_code: impl Into<String>,
    ) {
        let mut model = write_model(&self.model);
        model.configure_demo_contact_shortcuts(alice_invite_code, carol_invite_code);
        drop(model);
        self.request_rerender();
    }

    pub fn configure_demo_device_shortcut(
        &self,
        name: impl Into<String>,
        invitee_authority_id: AuthorityId,
    ) {
        let mut model = write_model(&self.model);
        model.configure_demo_device_shortcut(name, invitee_authority_id);
        drop(model);
        self.request_rerender();
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
        let mut model = write_model(&self.model);
        model.messages.push(message.to_string());
        let snapshot = model.semantic_snapshot();
        drop(model);
        self.publish_ui_snapshot(snapshot);
        self.request_rerender();
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
        if account_ready && model.screen == ScreenId::Onboarding {
            model.set_screen(ScreenId::Neighborhood);
        }
        model.account_setup_name = account_setup_name.into();
        model.account_setup_error = account_setup_error;
        let snapshot = model.semantic_snapshot();
        drop(model);
        self.publish_ui_snapshot(snapshot);
        self.request_rerender();
    }

    pub fn finalize_account_setup(&self, screen: ScreenId) {
        let mut model = write_model(&self.model);
        model.account_ready = true;
        model.account_setup_name.clear();
        model.account_setup_error = None;
        model.set_screen(screen);
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
    let mut guard = model.write().unwrap_or_else(|poison| poison.into_inner());
    guard.advance_semantic_revision();
    guard
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveModal, ContactRelationshipState, NeighborhoodMode, ScreenId, TextModalState, UiModel,
    };
    use aura_app::ui::contract::{
        OperationId, OperationInstanceId, OperationState, RuntimeEventKind,
    };
    use aura_app::ui_contract::{InvitationFactKind, RuntimeFact};
    use aura_core::types::identifiers::AuthorityId;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[test]
    fn ensure_runtime_contact_records_invitation_code_on_insert() {
        let mut model = UiModel::new("local".to_string());
        let alice = test_authority(1);

        model.ensure_runtime_contact(
            alice,
            "Alice".to_string(),
            false,
            ContactRelationshipState::Contact,
            Some("aura:v1:ABC".to_string()),
        );

        let Some(row) = model.contacts.iter().find(|r| r.authority_id == alice) else {
            panic!("contact should exist");
        };
        assert_eq!(row.invitation_code, Some("aura:v1:ABC".to_string()));
    }

    #[test]
    fn ensure_runtime_contact_preserves_existing_code_when_none_supplied() {
        let mut model = UiModel::new("local".to_string());
        let alice = test_authority(1);

        model.ensure_runtime_contact(
            alice,
            "Alice".to_string(),
            false,
            ContactRelationshipState::Contact,
            Some("aura:v1:KEEP".to_string()),
        );
        // Subsequent update without a code (e.g. a refresh projection
        // that doesn't carry the code) must preserve the existing value.
        model.ensure_runtime_contact(
            alice,
            "Alice Updated".to_string(),
            false,
            ContactRelationshipState::Contact,
            None,
        );

        let Some(row) = model.contacts.iter().find(|r| r.authority_id == alice) else {
            panic!("contact should exist");
        };
        assert_eq!(row.invitation_code, Some("aura:v1:KEEP".to_string()));
        assert_eq!(row.name, "Alice Updated".to_string());
    }

    #[test]
    fn ensure_runtime_contact_overwrites_code_on_reissue() {
        let mut model = UiModel::new("local".to_string());
        let alice = test_authority(1);

        model.ensure_runtime_contact(
            alice,
            "Alice".to_string(),
            false,
            ContactRelationshipState::Contact,
            Some("aura:v1:OLD".to_string()),
        );
        model.ensure_runtime_contact(
            alice,
            "Alice".to_string(),
            false,
            ContactRelationshipState::Contact,
            Some("aura:v1:NEW".to_string()),
        );

        let Some(row) = model.contacts.iter().find(|r| r.authority_id == alice) else {
            panic!("contact should exist");
        };
        assert_eq!(row.invitation_code, Some("aura:v1:NEW".to_string()));
    }

    #[test]
    fn replace_contacts_carries_invitation_code_from_projection() {
        let mut model = UiModel::new("local".to_string());
        let alice = test_authority(1);
        let bob = test_authority(2);

        model.replace_contacts(vec![
            (
                alice,
                "Alice".to_string(),
                false,
                ContactRelationshipState::Contact,
                Some("aura:v1:FROM_PROJECTION".to_string()),
            ),
            (
                bob,
                "Bob".to_string(),
                false,
                ContactRelationshipState::Contact,
                None,
            ),
        ]);

        assert_eq!(
            model
                .contacts
                .iter()
                .find(|r| r.authority_id == alice)
                .and_then(|r| r.invitation_code.clone()),
            Some("aura:v1:FROM_PROJECTION".to_string())
        );
        assert!(model
            .contacts
            .iter()
            .find(|r| r.authority_id == bob)
            .and_then(|r| r.invitation_code.clone())
            .is_none());
    }

    #[test]
    fn replace_contacts_preserves_ui_local_code_when_projection_is_none() {
        let mut model = UiModel::new("local".to_string());
        let alice = test_authority(1);

        // Session-local code set by the UI flow before the authoritative
        // fact is projected back.
        model.ensure_runtime_contact(
            alice,
            "Alice".to_string(),
            false,
            ContactRelationshipState::Contact,
            Some("aura:v1:SESSION".to_string()),
        );

        // Refresh projection from the runtime view that does not yet
        // carry the code — the UI-local code should not be wiped.
        model.replace_contacts(vec![(
            alice,
            "Alice".to_string(),
            false,
            ContactRelationshipState::Contact,
            None,
        )]);

        assert_eq!(
            model
                .contacts
                .iter()
                .find(|r| r.authority_id == alice)
                .and_then(|r| r.invitation_code.clone()),
            Some("aura:v1:SESSION".to_string())
        );
    }

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
    fn ready_models_canonicalize_onboarding_screen() {
        let mut model = UiModel::new("authority-local".to_string());
        model.account_ready = true;

        model.set_screen(ScreenId::Onboarding);

        assert!(matches!(model.screen, ScreenId::Neighborhood));
        assert!(matches!(
            model.semantic_snapshot().screen,
            ScreenId::Neighborhood
        ));
    }

    #[test]
    fn semantic_snapshot_revision_only_advances_on_mutation() {
        let mut model = UiModel::new("authority-local".to_string());

        let initial = model.semantic_snapshot().revision;
        let repeated = model.semantic_snapshot().revision;
        assert_eq!(repeated, initial);

        model.advance_semantic_revision();
        let mutated = model.semantic_snapshot().revision;
        assert!(mutated.is_newer_than(initial));
    }

    #[test]
    fn semantic_snapshot_includes_tracked_operation_state() {
        let mut model = UiModel::new("authority-local".to_string());
        model.set_authoritative_operation_state(
            OperationId::invitation_accept_contact(),
            None,
            None,
            OperationState::Submitting,
        );

        let snapshot = model.semantic_snapshot();
        let operation_state = snapshot
            .operations
            .iter()
            .find(|operation| operation.id == OperationId::invitation_accept_contact())
            .map(|operation| operation.state);

        assert_eq!(operation_state, Some(OperationState::Submitting));
    }

    #[test]
    fn restarting_operation_generates_new_operation_instance_id() {
        let mut model = UiModel::new("authority-local".to_string());
        model.set_authoritative_operation_state(
            OperationId::invitation_accept_contact(),
            None,
            None,
            OperationState::Submitting,
        );
        let Some(first_instance) = model
            .semantic_snapshot()
            .operations
            .into_iter()
            .find(|operation| operation.id == OperationId::invitation_accept_contact())
        else {
            panic!("first operation should exist");
        };
        let first_instance = first_instance.instance_id;

        model.set_authoritative_operation_state(
            OperationId::invitation_accept_contact(),
            None,
            None,
            OperationState::Succeeded,
        );
        model.set_authoritative_operation_state(
            OperationId::invitation_accept_contact(),
            None,
            None,
            OperationState::Submitting,
        );
        let Some(second_instance) = model
            .semantic_snapshot()
            .operations
            .into_iter()
            .find(|operation| operation.id == OperationId::invitation_accept_contact())
        else {
            panic!("second operation should exist");
        };
        let second_instance = second_instance.instance_id;

        assert_ne!(first_instance, second_instance);
    }

    #[test]
    fn authoritative_operation_replay_does_not_regress_terminal_state_for_same_instance() {
        let mut model = UiModel::new("authority-local".to_string());
        let instance_id = OperationInstanceId("op-1".to_string());
        let operation_id = OperationId::invitation_accept_contact();

        model.set_authoritative_operation_state(
            operation_id.clone(),
            Some(instance_id.clone()),
            None,
            OperationState::Submitting,
        );
        model.set_authoritative_operation_state(
            operation_id.clone(),
            Some(instance_id.clone()),
            None,
            OperationState::Succeeded,
        );
        model.set_authoritative_operation_state(
            operation_id.clone(),
            Some(instance_id),
            None,
            OperationState::Submitting,
        );

        let operation_state = model
            .semantic_snapshot()
            .operations
            .into_iter()
            .find(|operation| operation.id == operation_id)
            .map(|operation| operation.state);

        assert_eq!(operation_state, Some(OperationState::Succeeded));
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

    #[test]
    fn repeated_runtime_fact_reuses_existing_runtime_event_id() -> Result<(), &'static str> {
        let mut model = UiModel::new("authority-local".to_string());
        let fact = RuntimeFact::InvitationAccepted {
            invitation_kind: InvitationFactKind::Contact,
            authority_id: None,
            operation_state: None,
        };

        model.push_runtime_fact(fact.clone());
        let first_id = model
            .semantic_snapshot()
            .runtime_events
            .last()
            .map(|event| event.id.clone())
            .ok_or("runtime event should exist after first push")?;

        model.push_runtime_fact(fact);
        let second_id = model
            .semantic_snapshot()
            .runtime_events
            .last()
            .map(|event| event.id.clone())
            .ok_or("runtime event should exist after second push")?;

        assert_eq!(second_id, first_id);
        Ok(())
    }

    #[test]
    fn modal_text_value_or_empty_handles_missing_and_present_modal_text() {
        let mut model = UiModel::new("authority-local".to_string());
        assert_eq!(model.modal_text_value_or_empty(), "");

        model.active_modal = Some(ActiveModal::AcceptContactInvitation(TextModalState {
            value: "invite-code".to_string(),
        }));

        assert_eq!(model.modal_text_value_or_empty(), "invite-code");
    }
}
