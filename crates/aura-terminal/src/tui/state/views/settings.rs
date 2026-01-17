//! Settings screen view state

use super::KeyRotationCeremonyUiState;
use crate::tui::navigation::TwoPanelFocus;
use crate::tui::state::form::{Validatable, ValidationError};
use crate::tui::types::{AuthoritySubSection, Device, MfaPolicy, SettingsSection};
use aura_core::types::Epoch;

/// Settings screen state
///
/// Note: Authority context (authorities list, current_authority_index) is stored
/// in TuiState root since authority switching is app-global and affects all screens.
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

    // === Authority panel state (screen-local navigation) ===
    /// Sub-section within Authority panel (Info or Mfa)
    pub authority_sub_section: AuthoritySubSection,
    // Note: Modal state is now stored in ModalQueue, not here.
    // Use modal_queue.enqueue(QueuedModal::SettingsDisplayName/AddDevice/RemoveDevice(...)) to show modals.
    // For threshold/guardian changes, use OpenGuardianSetup dispatch which shows GuardianSetup modal.
    //
    // Note: Authority list and current authority index are now in TuiState root,
    // not here, since authority switching is app-global context.
}

/// State for nickname suggestion edit modal (settings screen)
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct NicknameSuggestionModalState {
    /// Nickname suggestion input buffer
    pub value: String,
    /// Error message if any
    pub error: Option<String>,
}

impl NicknameSuggestionModalState {
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

/// Which field is focused in the add device modal
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AddDeviceField {
    #[default]
    Name,
    InviteeAuthority,
}

/// State for add device modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
///
/// ## Two-step exchange
///
/// For secure device enrollment, the invitee device must first create its own
/// authority and share it with the initiator. The `invitee_authority_id` field
/// enables this two-step exchange:
///
/// 1. Invitee creates authority and shows their authority ID (via QR/text)
/// 2. Initiator enters the invitee's authority ID here
/// 3. Invitation is cryptographically bound to that specific authority
///
/// If `invitee_authority_id` is empty, falls back to legacy bearer token mode.
#[derive(Clone, Debug, Default)]
pub struct AddDeviceModalState {
    /// Device name input
    pub name: String,
    /// Invitee's authority ID for two-step exchange (optional)
    ///
    /// If provided, the enrollment invitation will be addressed to this
    /// specific authority, enabling the DeviceEnrollment choreography.
    /// If empty, falls back to legacy self-addressed (bearer token) mode.
    pub invitee_authority_id: String,
    /// Which field is currently focused
    pub focused_field: AddDeviceField,
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
        self.invitee_authority_id.clear();
        self.focused_field = AddDeviceField::Name;
        self.error = None;
    }

    pub fn can_submit(&self) -> bool {
        !self.name.trim().is_empty()
    }

    /// Get the invitee authority ID if provided, or None for legacy mode
    pub fn invitee_authority(&self) -> Option<&str> {
        let trimmed = self.invitee_authority_id.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }
}

/// State for the device enrollment ("add device") ceremony modal.
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct DeviceEnrollmentCeremonyModalState {
    /// Ceremony UI state (id/progress/pending epoch)
    pub ceremony: KeyRotationCeremonyUiState,
    /// Nickname suggestion for the device being enrolled (what it wants to be called)
    pub nickname_suggestion: String,
    /// Enrollment code to import on the new device
    pub enrollment_code: String,
    /// Whether code was copied to clipboard
    pub copied: bool,
}

impl DeviceEnrollmentCeremonyModalState {
    pub fn started(
        ceremony_id: String,
        nickname_suggestion: String,
        enrollment_code: String,
    ) -> Self {
        Self {
            ceremony: KeyRotationCeremonyUiState {
                ceremony_id: Some(ceremony_id),
                ..KeyRotationCeremonyUiState::default()
            },
            nickname_suggestion,
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
    /// Device display name (effective name for UI)
    pub display_name: String,
    /// Whether confirm button is focused (vs cancel)
    pub confirm_focused: bool,
}

impl ConfirmRemoveModalState {
    /// Create initialized state for device removal confirmation
    pub fn for_device(device_id: &str, display_name: &str) -> Self {
        Self {
            device_id: device_id.to_string(),
            display_name: display_name.to_string(),
            confirm_focused: false,
        }
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.device_id.clear();
        self.display_name.clear();
        self.confirm_focused = false;
    }

    pub fn toggle_focus(&mut self) {
        self.confirm_focused = !self.confirm_focused;
    }
}

/// State for device selection modal (for removal)
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct DeviceSelectModalState {
    /// Available devices to select from
    pub devices: Vec<Device>,
    /// Currently selected index (among selectable devices, excludes current device)
    pub selected_index: usize,
}

impl DeviceSelectModalState {
    /// Create initialized state with devices list
    pub fn with_devices(devices: Vec<Device>) -> Self {
        Self {
            devices,
            selected_index: 0,
        }
    }

    /// Get selectable devices (non-current)
    pub fn selectable_devices(&self) -> Vec<&Device> {
        self.devices.iter().filter(|d| !d.is_current).collect()
    }

    /// Get currently selected device (if any)
    pub fn selected_device(&self) -> Option<&Device> {
        self.selectable_devices().get(self.selected_index).copied()
    }

    /// Move selection up (wrapping)
    pub fn select_prev(&mut self) {
        let count = self.selectable_devices().len();
        if count > 0 && self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down (wrapping)
    pub fn select_next(&mut self) {
        let count = self.selectable_devices().len();
        if count > 0 && self.selected_index + 1 < count {
            self.selected_index += 1;
        }
    }

    /// Check if selection is valid (has selectable devices)
    pub fn can_select(&self) -> bool {
        !self.selectable_devices().is_empty()
    }
}

// ============================================================================
// Form Data Types with Validation
// ============================================================================

/// Form data for nickname suggestion editing (portable, validatable)
#[derive(Clone, Debug, Default)]
pub struct NicknameSuggestionFormData {
    /// Nickname suggestion value (required)
    pub value: String,
}

impl Validatable for NicknameSuggestionFormData {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = vec![];
        if self.value.trim().is_empty() {
            errors.push(ValidationError::required("nickname_suggestion"));
        } else if self.value.len() > 100 {
            errors.push(ValidationError::too_long("nickname_suggestion", 100));
        }
        errors
    }
}

/// Form data for device name (portable, validatable)
#[derive(Clone, Debug, Default)]
pub struct DeviceNameFormData {
    /// Device name (required)
    pub name: String,
}

impl Validatable for DeviceNameFormData {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = vec![];
        if self.name.trim().is_empty() {
            errors.push(ValidationError::required("name"));
        } else if self.name.len() > 50 {
            errors.push(ValidationError::too_long("name", 50));
        }
        errors
    }
}
