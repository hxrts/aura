//! Shared ceremony view state

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
    pub pending_epoch: Option<u64>,
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
        pending_epoch: Option<u64>,
    ) {
        self.accepted_count = accepted_count;
        self.total_count = total_count;
        self.threshold = threshold;
        self.is_complete = is_complete;
        self.has_failed = has_failed;
        self.error_message = error_message;
        if pending_epoch.is_some() {
            self.pending_epoch = pending_epoch;
        }
    }
}
