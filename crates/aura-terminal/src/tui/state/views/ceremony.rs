//! Shared ceremony view state

use aura_core::threshold::AgreementMode;
use aura_core::types::Epoch;

/// Shared UI state for key-rotation / membership-change ceremonies.
#[derive(Clone, Debug, Default)]
pub struct KeyRotationCeremonyUiState {
    /// Ceremony identifier (filled asynchronously by the shell for some flows).
    pub ceremony_id: Option<String>,
    /// Progress counters
    pub accepted_count: u16,
    pub total_count: u16,
    pub threshold: u16,
    /// Terminal status flags
    pub is_complete: bool,
    pub has_failed: bool,
    /// Optional error message
    pub error_message: Option<String>,
    /// Pending epoch, if the ceremony created one during prepare
    pub pending_epoch: Option<Epoch>,
    /// Agreement mode (A1/A2/A3)
    pub agreement_mode: AgreementMode,
    /// Whether reversion is still possible
    pub reversion_risk: bool,
}

impl KeyRotationCeremonyUiState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn set_ceremony_id(&mut self, ceremony_id: String) {
        self.ceremony_id = Some(ceremony_id);
    }

    pub fn update_from_status(
        &mut self,
        accepted_count: u16,
        total_count: u16,
        threshold: u16,
        is_complete: bool,
        has_failed: bool,
        error_message: Option<String>,
        pending_epoch: Option<Epoch>,
        agreement_mode: AgreementMode,
        reversion_risk: bool,
    ) {
        self.accepted_count = accepted_count;
        self.total_count = total_count;
        self.threshold = threshold;
        self.is_complete = is_complete;
        self.has_failed = has_failed;
        self.error_message = error_message;
        self.agreement_mode = agreement_mode;
        self.reversion_risk = reversion_risk;
        if pending_epoch.is_some() {
            self.pending_epoch = pending_epoch;
        }
    }
}
