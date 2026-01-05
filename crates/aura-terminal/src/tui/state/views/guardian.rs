//! Guardian setup modal state
//!
//! Uses a sum type pattern where each wizard phase is a variant
//! containing only the fields relevant to that phase.

use super::KeyRotationCeremonyUiState;
use aura_app::ui::prelude::*;

// ============================================================================
// Supporting Types
// ============================================================================

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

/// A selected guardian with resolved info
#[derive(Clone, Debug)]
pub struct SelectedGuardian {
    /// Contact ID
    pub id: String,
    /// Display name
    pub name: String,
}

// ============================================================================
// Wizard Phase Sum Type
// ============================================================================

/// Guardian setup wizard phase - sum type with step-specific fields.
///
/// Each variant contains only the data relevant to that phase,
/// preventing invalid field access at compile time.
#[derive(Clone, Debug)]
pub enum GuardianSetupPhase {
    /// Step 1: Select contacts to become guardians
    SelectContacts {
        /// Available contacts for selection
        contacts: Vec<GuardianCandidate>,
        /// Currently focused contact index
        focused_index: usize,
        /// Indices of selected contacts (using Vec for order preservation)
        selected_indices: Vec<usize>,
    },
    /// Step 2: Choose threshold (k of n)
    ChooseThreshold {
        /// Selected guardians with their info
        selected_guardians: Vec<SelectedGuardian>,
        /// Selected threshold k (required signers) - minimum 2 for FROST
        threshold_k: u8,
    },
    /// Step 3: Ceremony in progress, waiting for responses
    CeremonyInProgress {
        /// Selected guardians
        selected_guardians: Vec<SelectedGuardian>,
        /// Threshold k
        threshold_k: u8,
        /// Ceremony UI state (id/progress/pending epoch)
        ceremony: KeyRotationCeremonyUiState,
        /// Responses from guardians (id, name, response)
        responses: Vec<(String, String, GuardianCeremonyResponse)>,
    },
    /// Ceremony completed successfully
    Complete,
    /// Error state with message
    Error {
        /// Error message
        message: String,
        /// Whether retry is possible
        can_retry: bool,
        /// Previous phase to return to on retry (simplified - just track if was in ceremony)
        was_in_ceremony: bool,
    },
}

impl Default for GuardianSetupPhase {
    fn default() -> Self {
        Self::SelectContacts {
            contacts: Vec::new(),
            focused_index: 0,
            selected_indices: Vec::new(),
        }
    }
}

impl GuardianSetupPhase {
    /// Create the initial selection phase with contacts
    #[must_use]
    pub fn with_contacts(contacts: Vec<GuardianCandidate>) -> Self {
        let mut selected_indices = Vec::new();
        // Pre-select current guardians
        for (idx, contact) in contacts.iter().enumerate() {
            if contact.is_current_guardian {
                selected_indices.push(idx);
            }
        }

        Self::SelectContacts {
            contacts,
            focused_index: 0,
            selected_indices,
        }
    }

    /// Check which step we're on (for backward compatibility)
    #[must_use]
    pub fn step(&self) -> GuardianSetupStep {
        match self {
            Self::SelectContacts { .. } => GuardianSetupStep::SelectContacts,
            Self::ChooseThreshold { .. } => GuardianSetupStep::ChooseThreshold,
            Self::CeremonyInProgress { .. } => GuardianSetupStep::CeremonyInProgress,
            Self::Complete => GuardianSetupStep::Complete,
            Self::Error { .. } => GuardianSetupStep::Error,
        }
    }
}

/// Step identifier for backward compatibility and simple step checks
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum GuardianSetupStep {
    #[default]
    SelectContacts,
    ChooseThreshold,
    CeremonyInProgress,
    Complete,
    Error,
}

// ============================================================================
// Modal State Wrapper
// ============================================================================

/// State for guardian setup modal.
///
/// Wraps the sum-type phase and provides a stable API for the TUI.
/// Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug)]
pub struct GuardianSetupModalState {
    /// Current phase of the wizard
    phase: GuardianSetupPhase,
}

impl Default for GuardianSetupModalState {
    fn default() -> Self {
        Self {
            phase: GuardianSetupPhase::default(),
        }
    }
}

impl GuardianSetupModalState {
    // =========================================================================
    // Constructors
    // =========================================================================

    /// Create initialized state with contacts, pre-selecting current guardians
    #[must_use]
    pub fn with_contacts(contacts: Vec<GuardianCandidate>) -> Self {
        Self {
            phase: GuardianSetupPhase::with_contacts(contacts),
        }
    }

    /// Create initialized state with contacts and explicit selection
    #[must_use]
    pub fn from_contacts_with_selection(
        contacts: Vec<GuardianCandidate>,
        selected_indices: Vec<usize>,
    ) -> Self {
        Self {
            phase: GuardianSetupPhase::SelectContacts {
                contacts,
                focused_index: 0,
                selected_indices,
            },
        }
    }

    /// Create initialized state for MFA setup (all devices selected, with threshold)
    #[must_use]
    pub fn for_mfa_setup(contacts: Vec<GuardianCandidate>, threshold_k: u8) -> Self {
        let selected_indices: Vec<usize> = (0..contacts.len()).collect();
        let n = selected_indices.len() as u8;
        let normalized_k = normalize_guardian_threshold(threshold_k, n);

        // Create in ChooseThreshold phase directly since all are pre-selected
        let selected_guardians: Vec<SelectedGuardian> = contacts
            .iter()
            .map(|c| SelectedGuardian {
                id: c.id.clone(),
                name: c.name.clone(),
            })
            .collect();

        Self {
            phase: GuardianSetupPhase::ChooseThreshold {
                selected_guardians,
                threshold_k: normalized_k,
            },
        }
    }

    /// Create state directly in ceremony phase (for testing)
    #[must_use]
    pub fn in_ceremony(ceremony_id: Option<String>) -> Self {
        Self {
            phase: GuardianSetupPhase::CeremonyInProgress {
                selected_guardians: Vec::new(),
                threshold_k: 2,
                ceremony: {
                    let mut c = KeyRotationCeremonyUiState::default();
                    if let Some(id) = ceremony_id {
                        c.set_ceremony_id(id);
                    }
                    c
                },
                responses: Vec::new(),
            },
        }
    }

    // =========================================================================
    // Phase Access
    // =========================================================================

    /// Get the current phase
    #[must_use]
    pub fn phase(&self) -> &GuardianSetupPhase {
        &self.phase
    }

    /// Get a mutable reference to the current phase
    pub fn phase_mut(&mut self) -> &mut GuardianSetupPhase {
        &mut self.phase
    }

    /// Get the current step (for backward compatibility)
    #[must_use]
    pub fn step(&self) -> GuardianSetupStep {
        self.phase.step()
    }

    // =========================================================================
    // SelectContacts Phase Accessors
    // =========================================================================

    /// Get contacts (only valid in SelectContacts phase)
    #[must_use]
    pub fn contacts(&self) -> Option<&[GuardianCandidate]> {
        match &self.phase {
            GuardianSetupPhase::SelectContacts { contacts, .. } => Some(contacts),
            _ => None,
        }
    }

    /// Get total number of available contacts
    #[must_use]
    pub fn contact_count(&self) -> usize {
        match &self.phase {
            GuardianSetupPhase::SelectContacts { contacts, .. } => contacts.len(),
            _ => 0,
        }
    }

    /// Check if there are no contacts available
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contact_count() == 0
    }

    /// Get focused index (only in SelectContacts)
    #[must_use]
    pub fn focused_index(&self) -> usize {
        match &self.phase {
            GuardianSetupPhase::SelectContacts { focused_index, .. } => *focused_index,
            _ => 0,
        }
    }

    /// Set focused index (only in SelectContacts)
    pub fn set_focused_index(&mut self, index: usize) {
        if let GuardianSetupPhase::SelectContacts {
            focused_index,
            contacts,
            ..
        } = &mut self.phase
        {
            *focused_index = index.min(contacts.len().saturating_sub(1));
        }
    }

    /// Move focus up (only in SelectContacts)
    pub fn move_focus_up(&mut self) {
        if let GuardianSetupPhase::SelectContacts { focused_index, .. } = &mut self.phase {
            if *focused_index > 0 {
                *focused_index -= 1;
            }
        }
    }

    /// Move focus down (only in SelectContacts)
    pub fn move_focus_down(&mut self) {
        if let GuardianSetupPhase::SelectContacts {
            focused_index,
            contacts,
            ..
        } = &mut self.phase
        {
            if *focused_index + 1 < contacts.len() {
                *focused_index += 1;
            }
        }
    }

    /// Get selected indices (only in SelectContacts)
    #[must_use]
    pub fn selected_indices(&self) -> Option<&[usize]> {
        match &self.phase {
            GuardianSetupPhase::SelectContacts {
                selected_indices, ..
            } => Some(selected_indices),
            _ => None,
        }
    }

    /// Check if a contact index is selected
    #[must_use]
    pub fn is_selected(&self, index: usize) -> bool {
        match &self.phase {
            GuardianSetupPhase::SelectContacts {
                selected_indices, ..
            } => selected_indices.contains(&index),
            _ => false,
        }
    }

    /// Toggle selection of the currently focused contact
    pub fn toggle_selection(&mut self) {
        if let GuardianSetupPhase::SelectContacts {
            focused_index,
            selected_indices,
            ..
        } = &mut self.phase
        {
            if let Some(pos) = selected_indices.iter().position(|&i| i == *focused_index) {
                selected_indices.remove(pos);
            } else {
                selected_indices.push(*focused_index);
            }
        }
    }

    /// Select a contact by ID and focus it (returns true if found)
    pub fn select_by_id(&mut self, contact_id: &str) -> bool {
        if let GuardianSetupPhase::SelectContacts {
            contacts,
            focused_index,
            selected_indices,
        } = &mut self.phase
        {
            if let Some(idx) = contacts.iter().position(|c| c.id == contact_id) {
                if !selected_indices.contains(&idx) {
                    selected_indices.push(idx);
                }
                *focused_index = idx;
                return true;
            }
        }
        false
    }

    // =========================================================================
    // Threshold Accessors
    // =========================================================================

    /// Get total selected guardians count (n)
    #[must_use]
    pub fn threshold_n(&self) -> u8 {
        match &self.phase {
            GuardianSetupPhase::SelectContacts {
                selected_indices, ..
            } => selected_indices.len() as u8,
            GuardianSetupPhase::ChooseThreshold {
                selected_guardians, ..
            } => selected_guardians.len() as u8,
            GuardianSetupPhase::CeremonyInProgress {
                selected_guardians, ..
            } => selected_guardians.len() as u8,
            _ => 0,
        }
    }

    /// Get current threshold k
    #[must_use]
    pub fn threshold_k(&self) -> u8 {
        match &self.phase {
            GuardianSetupPhase::ChooseThreshold { threshold_k, .. } => *threshold_k,
            GuardianSetupPhase::CeremonyInProgress { threshold_k, .. } => *threshold_k,
            GuardianSetupPhase::SelectContacts {
                selected_indices, ..
            } => default_guardian_threshold(selected_indices.len() as u8),
            _ => 2,
        }
    }

    /// Increment threshold k (only in ChooseThreshold)
    pub fn increment_k(&mut self) {
        if let GuardianSetupPhase::ChooseThreshold {
            threshold_k,
            selected_guardians,
        } = &mut self.phase
        {
            let n = selected_guardians.len() as u8;
            let next = threshold_k.saturating_add(1);
            *threshold_k = normalize_guardian_threshold(next, n);
        }
    }

    /// Decrement threshold k (only in ChooseThreshold)
    pub fn decrement_k(&mut self) {
        if let GuardianSetupPhase::ChooseThreshold {
            threshold_k,
            selected_guardians,
        } = &mut self.phase
        {
            let n = selected_guardians.len() as u8;
            let next = threshold_k.saturating_sub(1);
            *threshold_k = normalize_guardian_threshold(next, n);
        }
    }

    // =========================================================================
    // Ceremony Accessors
    // =========================================================================

    /// Get ceremony state (only in CeremonyInProgress)
    #[must_use]
    pub fn ceremony(&self) -> Option<&KeyRotationCeremonyUiState> {
        match &self.phase {
            GuardianSetupPhase::CeremonyInProgress { ceremony, .. } => Some(ceremony),
            _ => None,
        }
    }

    /// Get ceremony responses
    #[must_use]
    pub fn ceremony_responses(&self) -> Option<&[(String, String, GuardianCeremonyResponse)]> {
        match &self.phase {
            GuardianSetupPhase::CeremonyInProgress { responses, .. } => Some(responses),
            _ => None,
        }
    }

    /// Get ceremony responses as cloneable Vec (for props)
    #[must_use]
    pub fn ceremony_responses_vec(&self) -> Vec<(String, String, GuardianCeremonyResponse)> {
        match &self.phase {
            GuardianSetupPhase::CeremonyInProgress { responses, .. } => responses.clone(),
            _ => Vec::new(),
        }
    }

    /// Get ceremony ID
    #[must_use]
    pub fn ceremony_id(&self) -> Option<&String> {
        match &self.phase {
            GuardianSetupPhase::CeremonyInProgress { ceremony, .. } => ceremony.ceremony_id.as_ref(),
            _ => None,
        }
    }

    /// Get accepted count
    #[must_use]
    pub fn ceremony_accepted_count(&self) -> u16 {
        self.ceremony().map_or(0, |c| c.accepted_count)
    }

    /// Get total count
    #[must_use]
    pub fn ceremony_total_count(&self) -> u16 {
        self.ceremony().map_or(0, |c| c.total_count)
    }

    /// Get ceremony threshold
    #[must_use]
    pub fn ceremony_threshold(&self) -> u16 {
        self.ceremony().map_or(0, |c| c.threshold)
    }

    /// Check if ceremony is complete
    #[must_use]
    pub fn ceremony_is_complete(&self) -> bool {
        self.ceremony().is_some_and(|c| c.is_complete)
    }

    /// Check if ceremony has failed
    #[must_use]
    pub fn ceremony_has_failed(&self) -> bool {
        self.ceremony().is_some_and(|c| c.has_failed)
    }

    /// Get ceremony error message
    #[must_use]
    pub fn ceremony_error_message(&self) -> Option<String> {
        self.ceremony().and_then(|c| c.error_message.clone())
    }

    /// Get agreement mode
    #[must_use]
    pub fn ceremony_agreement_mode(&self) -> aura_core::threshold::AgreementMode {
        self.ceremony()
            .map_or(aura_core::threshold::AgreementMode::default(), |c| {
                c.agreement_mode
            })
    }

    /// Get reversion risk
    #[must_use]
    pub fn ceremony_reversion_risk(&self) -> bool {
        self.ceremony().is_some_and(|c| c.reversion_risk)
    }

    /// Set ceremony ID (only in CeremonyInProgress)
    pub fn set_ceremony_id(&mut self, id: String) {
        if let GuardianSetupPhase::CeremonyInProgress { ceremony, .. } = &mut self.phase {
            ceremony.set_ceremony_id(id);
        }
    }

    /// Set ceremony ID only if not already set
    pub fn ensure_ceremony_id(&mut self, id: String) {
        if let GuardianSetupPhase::CeremonyInProgress { ceremony, .. } = &mut self.phase {
            if ceremony.ceremony_id.is_none() {
                ceremony.set_ceremony_id(id);
            }
        }
    }

    /// Update ceremony from status signal
    #[allow(clippy::too_many_arguments)]
    pub fn update_ceremony_from_status(
        &mut self,
        accepted_count: u16,
        total_count: u16,
        threshold: u16,
        is_complete: bool,
        has_failed: bool,
        error_message: Option<String>,
        pending_epoch: Option<aura_core::Epoch>,
        agreement_mode: aura_core::threshold::AgreementMode,
        reversion_risk: bool,
    ) {
        if let GuardianSetupPhase::CeremonyInProgress { ceremony, .. } = &mut self.phase {
            ceremony.update_from_status(
                accepted_count,
                total_count,
                threshold,
                is_complete,
                has_failed,
                error_message,
                pending_epoch,
                agreement_mode,
                reversion_risk,
            );
        }
    }

    /// Update guardian responses based on list of accepted guardian IDs
    pub fn update_responses_from_accepted(&mut self, accepted_guardian_ids: &[String]) {
        if let GuardianSetupPhase::CeremonyInProgress { responses, .. } = &mut self.phase {
            for (id, _, response) in responses.iter_mut() {
                if accepted_guardian_ids.iter().any(|g| g == id) {
                    *response = GuardianCeremonyResponse::Accepted;
                } else if matches!(response, GuardianCeremonyResponse::Accepted) {
                    // Revert to pending if was marked accepted but isn't in the accepted list
                    *response = GuardianCeremonyResponse::Pending;
                }
            }
        }
    }

    /// Record a guardian's response
    pub fn record_response(&mut self, guardian_id: &str, accepted: bool) {
        if let GuardianSetupPhase::CeremonyInProgress { responses, .. } = &mut self.phase {
            for (id, _, response) in responses.iter_mut() {
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
    }

    /// Check if all guardians have accepted
    #[must_use]
    pub fn all_accepted(&self) -> bool {
        match &self.phase {
            GuardianSetupPhase::CeremonyInProgress { responses, .. } => {
                !responses.is_empty()
                    && responses
                        .iter()
                        .all(|(_, _, r)| *r == GuardianCeremonyResponse::Accepted)
            }
            _ => false,
        }
    }

    /// Check if any guardian has declined
    #[must_use]
    pub fn any_declined(&self) -> bool {
        match &self.phase {
            GuardianSetupPhase::CeremonyInProgress { responses, .. } => {
                responses
                    .iter()
                    .any(|(_, _, r)| *r == GuardianCeremonyResponse::Declined)
            }
            _ => false,
        }
    }

    // =========================================================================
    // Error Accessors
    // =========================================================================

    /// Get error message if in Error phase
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        match &self.phase {
            GuardianSetupPhase::Error { message, .. } => Some(message),
            _ => None,
        }
    }

    // =========================================================================
    // Transition Validators
    // =========================================================================

    /// Check if can proceed from contact selection to threshold step
    #[must_use]
    pub fn can_proceed_to_threshold(&self) -> bool {
        match &self.phase {
            GuardianSetupPhase::SelectContacts {
                selected_indices, ..
            } => selected_indices.len() >= 2,
            _ => false,
        }
    }

    /// Check if can start ceremony
    #[must_use]
    pub fn can_start_ceremony(&self) -> bool {
        match &self.phase {
            GuardianSetupPhase::ChooseThreshold {
                threshold_k,
                selected_guardians,
            } => {
                let n = selected_guardians.len() as u8;
                *threshold_k >= 2 && *threshold_k <= n && n >= 2
            }
            _ => false,
        }
    }

    // =========================================================================
    // State Transitions
    // =========================================================================

    /// Advance from SelectContacts to ChooseThreshold
    pub fn advance_to_threshold(&mut self) {
        if let GuardianSetupPhase::SelectContacts {
            contacts,
            selected_indices,
            ..
        } = &self.phase
        {
            let selected_guardians: Vec<SelectedGuardian> = selected_indices
                .iter()
                .filter_map(|&idx| {
                    contacts.get(idx).map(|c| SelectedGuardian {
                        id: c.id.clone(),
                        name: c.name.clone(),
                    })
                })
                .collect();

            let n = selected_guardians.len() as u8;
            let threshold_k = default_guardian_threshold(n);

            self.phase = GuardianSetupPhase::ChooseThreshold {
                selected_guardians,
                threshold_k,
            };
        }
    }

    /// Go back from ChooseThreshold to SelectContacts
    pub fn back_to_selection(&mut self) {
        if let GuardianSetupPhase::ChooseThreshold {
            selected_guardians, ..
        } = &self.phase
        {
            // We need to recreate the contacts - for now, create from selected guardians
            // In practice, the modal should be re-opened with fresh contacts
            let contacts: Vec<GuardianCandidate> = selected_guardians
                .iter()
                .map(|g| GuardianCandidate {
                    id: g.id.clone(),
                    name: g.name.clone(),
                    is_current_guardian: true,
                })
                .collect();
            let selected_indices: Vec<usize> = (0..contacts.len()).collect();

            self.phase = GuardianSetupPhase::SelectContacts {
                contacts,
                focused_index: 0,
                selected_indices,
            };
        }
    }

    /// Start the ceremony (transition from ChooseThreshold to CeremonyInProgress)
    pub fn begin_ceremony(&mut self) {
        if let GuardianSetupPhase::ChooseThreshold {
            selected_guardians,
            threshold_k,
        } = &self.phase
        {
            let responses: Vec<(String, String, GuardianCeremonyResponse)> = selected_guardians
                .iter()
                .map(|g| {
                    (
                        g.id.clone(),
                        g.name.clone(),
                        GuardianCeremonyResponse::Pending,
                    )
                })
                .collect();

            self.phase = GuardianSetupPhase::CeremonyInProgress {
                selected_guardians: selected_guardians.clone(),
                threshold_k: *threshold_k,
                ceremony: KeyRotationCeremonyUiState::default(),
                responses,
            };
        }
    }

    /// Complete the ceremony successfully
    pub fn complete_ceremony(&mut self) {
        self.phase = GuardianSetupPhase::Complete;
    }

    /// Fail/cancel the ceremony with an error
    pub fn fail_ceremony(&mut self, reason: &str) {
        let was_in_ceremony = matches!(self.phase, GuardianSetupPhase::CeremonyInProgress { .. });
        self.phase = GuardianSetupPhase::Error {
            message: reason.to_string(),
            can_retry: true,
            was_in_ceremony,
        };
    }

    /// Reset from ceremony to threshold selection (on ceremony failure, allows retry)
    pub fn reset_to_threshold_after_failure(&mut self) {
        if let GuardianSetupPhase::CeremonyInProgress {
            selected_guardians,
            threshold_k,
            ..
        } = &self.phase
        {
            self.phase = GuardianSetupPhase::ChooseThreshold {
                selected_guardians: selected_guardians.clone(),
                threshold_k: *threshold_k,
            };
        }
    }

    /// Reset state completely (called when dismissed)
    pub fn reset(&mut self) {
        self.phase = GuardianSetupPhase::default();
    }

    // =========================================================================
    // Utility Methods for Backward Compatibility
    // =========================================================================

    /// Get list of selected contact IDs
    #[must_use]
    pub fn selected_contact_ids(&self) -> Vec<String> {
        match &self.phase {
            GuardianSetupPhase::SelectContacts {
                contacts,
                selected_indices,
                ..
            } => selected_indices
                .iter()
                .filter_map(|&idx| contacts.get(idx).map(|c| c.id.clone()))
                .collect(),
            GuardianSetupPhase::ChooseThreshold {
                selected_guardians, ..
            } => selected_guardians.iter().map(|g| g.id.clone()).collect(),
            GuardianSetupPhase::CeremonyInProgress {
                selected_guardians, ..
            } => selected_guardians.iter().map(|g| g.id.clone()).collect(),
            _ => Vec::new(),
        }
    }

    /// Get the selected guardians (for display in threshold and ceremony phases)
    #[must_use]
    pub fn selected_guardians(&self) -> Option<&[SelectedGuardian]> {
        match &self.phase {
            GuardianSetupPhase::ChooseThreshold {
                selected_guardians, ..
            } => Some(selected_guardians),
            GuardianSetupPhase::CeremonyInProgress {
                selected_guardians, ..
            } => Some(selected_guardians),
            _ => None,
        }
    }
}
