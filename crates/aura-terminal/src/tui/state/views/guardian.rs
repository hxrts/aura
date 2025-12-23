//! Guardian setup modal state

use super::KeyRotationCeremonyUiState;

/// Step in the guardian setup wizard
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum GuardianSetupStep {
    /// Step 1: Select contacts to become guardians
    #[default]
    SelectContacts,
    /// Step 2: Choose threshold (k of n)
    ChooseThreshold,
    /// Step 3: Ceremony in progress, waiting for responses
    CeremonyInProgress,
}

/// Response status for a guardian in a ceremony
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuardianCeremonyResponse {
    /// Waiting for response
    Pending,
    /// Guardian accepted
    Accepted,
    /// Guardian declined
    Declined,
}

/// A contact that can be selected as a guardian
#[derive(Clone, Debug, Default)]
pub struct GuardianCandidate {
    /// Contact ID
    pub id: String,
    /// Display name
    pub name: String,
    /// Whether this contact is currently a guardian
    pub is_current_guardian: bool,
}

/// State for guardian setup modal (multi-select + threshold + ceremony)
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct GuardianSetupModalState {
    /// Current step in the wizard
    pub step: GuardianSetupStep,
    /// Available contacts for selection
    pub contacts: Vec<GuardianCandidate>,
    /// Indices of selected contacts (using Vec for order preservation)
    pub selected_indices: Vec<usize>,
    /// Currently focused contact index
    pub focused_index: usize,
    /// Selected threshold k (required signers)
    pub threshold_k: u8,
    /// Ceremony UI state (id/progress/pending epoch)
    pub ceremony: KeyRotationCeremonyUiState,
    /// Responses from guardians during ceremony (contact_id, name, response)
    pub ceremony_responses: Vec<(String, String, GuardianCeremonyResponse)>,
    /// Error message if any
    pub error: Option<String>,
}

impl GuardianSetupModalState {
    /// Get total selected guardians (n)
    pub fn threshold_n(&self) -> u8 {
        self.selected_indices.len() as u8
    }

    /// Create initialized state with contacts, pre-selecting current guardians
    pub fn with_contacts(contacts: Vec<GuardianCandidate>) -> Self {
        let mut selected_indices = Vec::new();
        // Pre-select current guardians
        for (idx, contact) in contacts.iter().enumerate() {
            if contact.is_current_guardian {
                selected_indices.push(idx);
            }
        }
        // Default threshold: majority (n/2 + 1) or 1 if no selection
        let n = selected_indices.len() as u8;
        let threshold_k = if n > 0 { (n / 2) + 1 } else { 1 };

        Self {
            step: GuardianSetupStep::SelectContacts,
            contacts,
            selected_indices,
            focused_index: 0,
            threshold_k,
            ceremony: KeyRotationCeremonyUiState::default(),
            ceremony_responses: Vec::new(),
            error: None,
        }
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.step = GuardianSetupStep::SelectContacts;
        self.contacts.clear();
        self.selected_indices.clear();
        self.focused_index = 0;
        self.threshold_k = 1;
        self.ceremony.clear();
        self.ceremony_responses.clear();
        self.error = None;
    }

    /// Toggle selection of the currently focused contact
    pub fn toggle_selection(&mut self) {
        if let Some(pos) = self
            .selected_indices
            .iter()
            .position(|&i| i == self.focused_index)
        {
            self.selected_indices.remove(pos);
        } else {
            self.selected_indices.push(self.focused_index);
        }
        // Adjust threshold_k if it exceeds new n
        let n = self.threshold_n();
        if self.threshold_k > n && n > 0 {
            self.threshold_k = n;
        }
    }

    /// Check if a contact index is selected
    pub fn is_selected(&self, index: usize) -> bool {
        self.selected_indices.contains(&index)
    }

    /// Increment threshold k (up to n)
    pub fn increment_k(&mut self) {
        let n = self.threshold_n();
        if self.threshold_k < n {
            self.threshold_k += 1;
        }
    }

    /// Decrement threshold k (down to 1)
    pub fn decrement_k(&mut self) {
        if self.threshold_k > 1 {
            self.threshold_k -= 1;
        }
    }

    /// Check if can proceed from contact selection to threshold step
    pub fn can_proceed_to_threshold(&self) -> bool {
        self.selected_indices.len() >= 2 // Need at least 2 guardians
    }

    /// Check if can start ceremony
    pub fn can_start_ceremony(&self) -> bool {
        let n = self.threshold_n();
        self.threshold_k >= 1
            && self.threshold_k <= n
            && n >= 2
            && !matches!(self.step, GuardianSetupStep::CeremonyInProgress)
            && self.ceremony.ceremony_id.is_none()
    }

    /// Transition into the in-progress ceremony step and initialize responses.
    ///
    /// Note: `ceremony_id` is filled asynchronously by the shell once the ceremony is initiated.
    pub fn begin_ceremony(&mut self) {
        self.step = GuardianSetupStep::CeremonyInProgress;
        self.error = None;
        self.ceremony.clear();

        // Initialize responses for all selected contacts
        self.ceremony_responses.clear();
        for &idx in &self.selected_indices {
            if let Some(contact) = self.contacts.get(idx) {
                self.ceremony_responses.push((
                    contact.id.clone(),
                    contact.name.clone(),
                    GuardianCeremonyResponse::Pending,
                ));
            }
        }
    }

    /// Set the ceremony ID once available.
    pub fn set_ceremony_id(&mut self, ceremony_id: String) {
        self.ceremony.set_ceremony_id(ceremony_id);
    }

    /// Record a guardian's response
    pub fn record_response(&mut self, guardian_id: &str, accepted: bool) {
        for (id, _, response) in &mut self.ceremony_responses {
            if id == guardian_id {
                *response = if accepted {
                    GuardianCeremonyResponse::Accepted
                } else {
                    GuardianCeremonyResponse::Declined
                };
                break;
            }
        }
    }

    /// Check if all guardians have accepted
    pub fn all_accepted(&self) -> bool {
        !self.ceremony_responses.is_empty()
            && self
                .ceremony_responses
                .iter()
                .all(|(_, _, r)| *r == GuardianCeremonyResponse::Accepted)
    }

    /// Check if any guardian has declined
    pub fn any_declined(&self) -> bool {
        self.ceremony_responses
            .iter()
            .any(|(_, _, r)| *r == GuardianCeremonyResponse::Declined)
    }

    /// Get list of selected contact IDs
    pub fn selected_contact_ids(&self) -> Vec<String> {
        self.selected_indices
            .iter()
            .filter_map(|&idx| self.contacts.get(idx).map(|c| c.id.clone()))
            .collect()
    }

    /// Complete the ceremony successfully
    pub fn complete_ceremony(&mut self) {
        self.reset();
    }

    /// Fail/cancel the ceremony
    pub fn fail_ceremony(&mut self, reason: &str) {
        self.error = Some(reason.to_string());
        self.step = GuardianSetupStep::SelectContacts;
        self.ceremony.clear();
        self.ceremony_responses.clear();
    }
}
