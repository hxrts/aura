//! Settings screen view state

use super::KeyRotationCeremonyUiState;
use crate::tui::navigation::TwoPanelFocus;
use crate::tui::types::{MfaPolicy, SettingsSection};

/// Settings screen state
#[derive(Clone, Debug, Default)]
pub struct SettingsViewState {
    /// Panel focus (menu or detail)
    pub focus: TwoPanelFocus,
    /// Current section
    pub section: SettingsSection,
    /// Selected item in current section
    pub selected_index: usize,
    /// Current MFA policy
    pub mfa_policy: MfaPolicy,
    // Note: Modal state is now stored in ModalQueue, not here.
    // Use modal_queue.enqueue(QueuedModal::SettingsDisplayName/Threshold/AddDevice/RemoveDevice(...)) to show modals.
}

/// State for display name edit modal (settings screen)
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct DisplayNameModalState {
    /// Display name input buffer
    pub value: String,
    /// Error message if any
    pub error: Option<String>,
}

impl DisplayNameModalState {
    /// Create initialized state with current name
    pub fn with_name(current_name: &str) -> Self {
        Self {
            value: current_name.to_string(),
            error: None,
        }
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.value.clear();
        self.error = None;
    }

    pub fn can_submit(&self) -> bool {
        !self.value.trim().is_empty()
    }
}

/// State for threshold config modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct ThresholdModalState {
    /// Threshold K (required signatures)
    pub k: u8,
    /// Threshold N (total guardians)
    pub n: u8,
    /// Active field (0 = k, 1 = n)
    pub active_field: usize,
    /// Error message if any
    pub error: Option<String>,
}

impl ThresholdModalState {
    /// Create initialized state with current threshold
    pub fn with_threshold(current_k: u8, current_n: u8) -> Self {
        Self {
            k: current_k,
            n: current_n,
            active_field: 0,
            error: None,
        }
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.k = 0;
        self.n = 0;
        self.active_field = 0;
        self.error = None;
    }

    pub fn increment_k(&mut self) {
        if self.k < self.n {
            self.k += 1;
        }
    }

    pub fn decrement_k(&mut self) {
        if self.k > 1 {
            self.k -= 1;
        }
    }

    pub fn increment_n(&mut self) {
        self.n = self.n.saturating_add(1);
    }

    pub fn decrement_n(&mut self) {
        if self.n > self.k {
            self.n -= 1;
        }
    }

    pub fn can_submit(&self) -> bool {
        self.k > 0 && self.k <= self.n && self.n > 0
    }
}

/// State for add device modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct AddDeviceModalState {
    /// Device name input
    pub name: String,
    /// Error message if any
    pub error: Option<String>,
}

impl AddDeviceModalState {
    /// Create new modal state
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.name.clear();
        self.error = None;
    }

    pub fn can_submit(&self) -> bool {
        !self.name.trim().is_empty()
    }
}

/// State for the device enrollment ("add device") ceremony modal.
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct DeviceEnrollmentCeremonyModalState {
    /// Ceremony UI state (id/progress/pending epoch)
    pub ceremony: KeyRotationCeremonyUiState,
    /// Device name being enrolled (for display)
    pub device_name: String,
    /// Enrollment code to import on the new device
    pub enrollment_code: String,
}

impl DeviceEnrollmentCeremonyModalState {
    pub fn started(ceremony_id: String, device_name: String, enrollment_code: String) -> Self {
        Self {
            ceremony: KeyRotationCeremonyUiState {
                ceremony_id: Some(ceremony_id),
                ..KeyRotationCeremonyUiState::default()
            },
            device_name,
            enrollment_code,
        }
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
        self.ceremony.update_from_status(
            accepted_count,
            total_count,
            threshold,
            is_complete,
            has_failed,
            error_message,
            pending_epoch,
        );
    }
}

/// State for confirm remove device modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct ConfirmRemoveModalState {
    /// Device ID to remove
    pub device_id: String,
    /// Device name (for display)
    pub device_name: String,
    /// Whether confirm button is focused (vs cancel)
    pub confirm_focused: bool,
}

impl ConfirmRemoveModalState {
    /// Create initialized state for device removal confirmation
    pub fn for_device(device_id: &str, device_name: &str) -> Self {
        Self {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            confirm_focused: false,
        }
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.device_id.clear();
        self.device_name.clear();
        self.confirm_focused = false;
    }

    pub fn toggle_focus(&mut self) {
        self.confirm_focused = !self.confirm_focused;
    }
}
