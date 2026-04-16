use super::*;
use aura_app::ui::types::ContactRelationshipState;
use aura_app::ui::workflows::ceremonies::{CeremonyHandle, CeremonyStatusHandle};
use aura_core::types::identifiers::CeremonyId;

impl UiModel {
    pub(super) fn push_runtime_fact(&mut self, fact: RuntimeFact) {
        let fact_key = fact.key();
        let existing_id = self
            .runtime_events
            .iter()
            .find(|event| event.key() == fact_key)
            .map(|event| event.id.clone());
        let id = existing_id.unwrap_or_else(|| {
            self.runtime_event_key = self.runtime_event_key.saturating_add(1);
            RuntimeEventId(format!("runtime-event-{}", self.runtime_event_key))
        });
        self.runtime_events.retain(|event| event.key() != fact_key);
        self.runtime_events.push(RuntimeEventSnapshot { id, fact });
        if self.runtime_events.len() > 32 {
            let drain = self.runtime_events.len().saturating_sub(32);
            self.runtime_events.drain(0..drain);
        }
    }
}

pub(super) struct RuntimeDeviceEnrollmentCeremony {
    status_handle: CeremonyStatusHandle,
    cancel_handle: Option<CeremonyHandle>,
}

impl UiController {
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
        channels: Vec<(String, String, String)>,
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
        contacts: Vec<(
            AuthorityId,
            String,
            bool,
            ContactRelationshipState,
            Option<String>,
        )>,
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
        relationship_state: ContactRelationshipState,
    ) {
        self.try_update_model(|model| {
            model.ensure_runtime_contact(authority_id, name, is_guardian, relationship_state, None);
        });
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

    /// Apply the shared UI completion effects for a successful imported contact
    /// invitation acceptance.
    ///
    /// `invitation_code` is the code that was pasted/accepted — it is
    /// recorded on the new contact's row so the Details panel can surface
    /// the code used to establish the link. Phase 2 session-scoped; Phase 3
    /// will persist this through the authoritative contact fact.
    pub fn complete_runtime_contact_invitation_acceptance(
        &self,
        authority_id: AuthorityId,
        display_name: String,
        invitation_code: Option<String>,
    ) {
        let mut model = write_model(&self.model);
        model.ensure_runtime_contact(
            authority_id,
            display_name,
            false,
            ContactRelationshipState::Contact,
            invitation_code,
        );
        model.push_runtime_fact(RuntimeFact::InvitationAccepted {
            invitation_kind: InvitationFactKind::Contact,
            authority_id: Some(authority_id.to_string()),
            operation_state: Some(OperationState::Succeeded),
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
            .find(|operation| operation.id == OperationId::invitation_accept_contact())
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

    pub(crate) fn set_runtime_device_enrollment_ceremony(&self, handle: CeremonyHandle) {
        if let Ok(mut slot) = self.runtime_device_enrollment_ceremony.lock() {
            *slot = Some(RuntimeDeviceEnrollmentCeremony {
                status_handle: handle.status_handle(),
                cancel_handle: Some(handle),
            });
        }
    }

    pub(crate) fn set_runtime_device_enrollment_ceremony_id(&self, ceremony_id: CeremonyId) {
        let mut model = write_model(&self.model);
        if let Some(ActiveModal::AddDevice(state)) = model.active_modal.as_mut() {
            state.ceremony_id = Some(ceremony_id);
        }
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn set_runtime_guardian_ceremony_id(&self, ceremony_id: CeremonyId) {
        let mut model = write_model(&self.model);
        if let Some(ActiveModal::GuardianSetup(state)) = model.active_modal.as_mut() {
            state.ceremony_id = Some(ceremony_id);
        }
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn clear_runtime_guardian_ceremony_id(&self) {
        let mut model = write_model(&self.model);
        if let Some(ActiveModal::GuardianSetup(state)) = model.active_modal.as_mut() {
            state.ceremony_id = None;
        }
        drop(model);
        self.request_rerender();
    }

    pub(crate) fn runtime_device_enrollment_status_handle(&self) -> Option<CeremonyStatusHandle> {
        self.runtime_device_enrollment_ceremony
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|ceremony| ceremony.status_handle.clone()))
    }

    pub(crate) fn take_runtime_device_enrollment_ceremony(&self) -> Option<CeremonyHandle> {
        self.runtime_device_enrollment_ceremony
            .lock()
            .ok()
            .and_then(|mut slot| {
                slot.as_mut()
                    .and_then(|ceremony| ceremony.cancel_handle.take())
            })
    }

    pub(crate) fn clear_runtime_device_enrollment_ceremony(&self) {
        if let Ok(mut slot) = self.runtime_device_enrollment_ceremony.lock() {
            *slot = None;
        }
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
        self.clear_runtime_device_enrollment_ceremony();
        let mut model = write_model(&self.model);
        dismiss_modal(&mut model);
        drop(model);
        self.request_rerender();
    }

    pub fn complete_runtime_enter_home(&self, home_id: &str, name: &str, depth: AccessDepth) {
        let mut model = write_model(&self.model);
        model.select_home(home_id.to_string(), name.to_string());
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
}
