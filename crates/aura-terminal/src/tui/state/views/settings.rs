//! Settings screen view state

use super::KeyRotationCeremonyUiState;
use crate::tui::navigation::TwoPanelFocus;
use crate::tui::types::{AuthorityInfo, AuthoritySubSection, MfaPolicy, SettingsSection};
use aura_core::types::Epoch;

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
    /// Most recent device enrollment code (demo helper)
    pub last_device_enrollment_code: String,
    /// Demo Mobile device id (for MFA shortcuts)
    pub demo_mobile_device_id: String,
    /// Whether to auto-fill the next device enrollment code into the import modal
    pub pending_mobile_enrollment_autofill: bool,

    // === Authority panel state ===
    /// Sub-section within Authority panel (Info or Mfa)
    pub authority_sub_section: AuthoritySubSection,
    /// Available authorities for this device
    pub authorities: Vec<AuthorityInfo>,
    /// Index of the currently active authority in the authorities list
    pub current_authority_index: usize,
    // Note: Modal state is now stored in ModalQueue, not here.
    // Use modal_queue.enqueue(QueuedModal::SettingsDisplayName/AddDevice/RemoveDevice(...)) to show modals.
    // For threshold/guardian changes, use OpenGuardianSetup dispatch which shows GuardianSetup modal.
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
    /// Whether code was copied to clipboard
    pub copied: bool,
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
            copied: false,
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
        pending_epoch: Option<Epoch>,
        agreement_mode: aura_core::threshold::AgreementMode,
        reversion_risk: bool,
    ) {
        self.ceremony.update_from_status(
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

    /// Mark code as copied to clipboard
    pub fn set_copied(&mut self) {
        self.copied = true;
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
