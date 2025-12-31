//! # Wizard Step Types
//!
//! Portable wizard step enums and navigation helpers for multi-step workflows.
//! These types define the logical steps without UI-specific state.

use serde::{Deserialize, Serialize};

// ============================================================================
// Create Channel Wizard
// ============================================================================

/// Steps in the create channel wizard.
///
/// This defines the logical flow for creating a new channel:
/// 1. Details - Enter channel name and optional topic
/// 2. Members - Select contacts to add as members
/// 3. Threshold - Set the signing threshold (m-of-n)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum CreateChannelStep {
    /// Enter channel name and topic
    #[default]
    Details,
    /// Select members to add
    Members,
    /// Configure threshold
    Threshold,
}

impl CreateChannelStep {
    /// Get all steps in order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[Self::Details, Self::Members, Self::Threshold]
    }

    /// Get the next step, or None if at the last step.
    #[must_use]
    pub fn next(self) -> Option<Self> {
        match self {
            Self::Details => Some(Self::Members),
            Self::Members => Some(Self::Threshold),
            Self::Threshold => None,
        }
    }

    /// Get the previous step, or None if at the first step.
    #[must_use]
    pub fn prev(self) -> Option<Self> {
        match self {
            Self::Details => None,
            Self::Members => Some(Self::Details),
            Self::Threshold => Some(Self::Members),
        }
    }

    /// Check if this is the first step.
    #[must_use]
    pub fn is_first(self) -> bool {
        self == Self::Details
    }

    /// Check if this is the last step.
    #[must_use]
    pub fn is_last(self) -> bool {
        self == Self::Threshold
    }

    /// Get step number (1-indexed for display).
    #[must_use]
    pub fn number(self) -> u8 {
        match self {
            Self::Details => 1,
            Self::Members => 2,
            Self::Threshold => 3,
        }
    }

    /// Get total number of steps.
    #[must_use]
    pub fn total_steps() -> u8 {
        3
    }

    /// Get step title for display.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Details => "Channel Details",
            Self::Members => "Select Members",
            Self::Threshold => "Set Threshold",
        }
    }

    /// Get step description for display.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::Details => "Enter a name and optional topic for the channel",
            Self::Members => "Choose contacts to add as channel members",
            Self::Threshold => "Set the signing threshold (m-of-n)",
        }
    }
}

// ============================================================================
// Account Setup Wizard
// ============================================================================

/// Steps in the account setup wizard.
///
/// This defines the logical flow for setting up a new account:
/// 1. Welcome - Introduction and overview
/// 2. DisplayName - Enter display name
/// 3. Guardians - Configure guardian setup (optional)
/// 4. Complete - Summary and confirmation
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum AccountSetupStep {
    /// Welcome screen
    #[default]
    Welcome,
    /// Enter display name
    DisplayName,
    /// Configure guardians (optional)
    Guardians,
    /// Setup complete
    Complete,
}

impl AccountSetupStep {
    /// Get all steps in order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::Welcome,
            Self::DisplayName,
            Self::Guardians,
            Self::Complete,
        ]
    }

    /// Get the next step, or None if at the last step.
    #[must_use]
    pub fn next(self) -> Option<Self> {
        match self {
            Self::Welcome => Some(Self::DisplayName),
            Self::DisplayName => Some(Self::Guardians),
            Self::Guardians => Some(Self::Complete),
            Self::Complete => None,
        }
    }

    /// Get the previous step, or None if at the first step.
    #[must_use]
    pub fn prev(self) -> Option<Self> {
        match self {
            Self::Welcome => None,
            Self::DisplayName => Some(Self::Welcome),
            Self::Guardians => Some(Self::DisplayName),
            Self::Complete => Some(Self::Guardians),
        }
    }

    /// Check if this is the first step.
    #[must_use]
    pub fn is_first(self) -> bool {
        self == Self::Welcome
    }

    /// Check if this is the last step.
    #[must_use]
    pub fn is_last(self) -> bool {
        self == Self::Complete
    }

    /// Get step number (1-indexed for display).
    #[must_use]
    pub fn number(self) -> u8 {
        match self {
            Self::Welcome => 1,
            Self::DisplayName => 2,
            Self::Guardians => 3,
            Self::Complete => 4,
        }
    }

    /// Get total number of steps.
    #[must_use]
    pub fn total_steps() -> u8 {
        4
    }

    /// Get step title for display.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::DisplayName => "Your Name",
            Self::Guardians => "Guardians",
            Self::Complete => "Complete",
        }
    }
}

// ============================================================================
// Recovery Setup Wizard
// ============================================================================

/// Steps in the recovery setup wizard.
///
/// This defines the logical flow for configuring recovery:
/// 1. Overview - Explain recovery and guardians
/// 2. SelectGuardians - Choose guardian contacts
/// 3. SetThreshold - Configure recovery threshold
/// 4. Confirm - Review and confirm settings
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum RecoverySetupStep {
    /// Explain recovery
    #[default]
    Overview,
    /// Select guardian contacts
    SelectGuardians,
    /// Set recovery threshold
    SetThreshold,
    /// Confirm settings
    Confirm,
}

impl RecoverySetupStep {
    /// Get all steps in order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::Overview,
            Self::SelectGuardians,
            Self::SetThreshold,
            Self::Confirm,
        ]
    }

    /// Get the next step, or None if at the last step.
    #[must_use]
    pub fn next(self) -> Option<Self> {
        match self {
            Self::Overview => Some(Self::SelectGuardians),
            Self::SelectGuardians => Some(Self::SetThreshold),
            Self::SetThreshold => Some(Self::Confirm),
            Self::Confirm => None,
        }
    }

    /// Get the previous step, or None if at the first step.
    #[must_use]
    pub fn prev(self) -> Option<Self> {
        match self {
            Self::Overview => None,
            Self::SelectGuardians => Some(Self::Overview),
            Self::SetThreshold => Some(Self::SelectGuardians),
            Self::Confirm => Some(Self::SetThreshold),
        }
    }

    /// Check if this is the first step.
    #[must_use]
    pub fn is_first(self) -> bool {
        self == Self::Overview
    }

    /// Check if this is the last step.
    #[must_use]
    pub fn is_last(self) -> bool {
        self == Self::Confirm
    }

    /// Get step number (1-indexed for display).
    #[must_use]
    pub fn number(self) -> u8 {
        match self {
            Self::Overview => 1,
            Self::SelectGuardians => 2,
            Self::SetThreshold => 3,
            Self::Confirm => 4,
        }
    }

    /// Get total number of steps.
    #[must_use]
    pub fn total_steps() -> u8 {
        4
    }

    /// Get step title for display.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Recovery Overview",
            Self::SelectGuardians => "Select Guardians",
            Self::SetThreshold => "Set Threshold",
            Self::Confirm => "Confirm Setup",
        }
    }
}

// ============================================================================
// Wizard Progress Helper
// ============================================================================

/// Format wizard progress for display.
///
/// # Arguments
/// * `current` - Current step number (1-indexed)
/// * `total` - Total number of steps
///
/// # Returns
/// A formatted progress string like "Step 2 of 4".
///
/// # Example
/// ```rust
/// use aura_app::views::wizards::format_wizard_progress;
///
/// assert_eq!(format_wizard_progress(2, 4), "Step 2 of 4");
/// ```
#[must_use]
pub fn format_wizard_progress(current: u8, total: u8) -> String {
    format!("Step {current} of {total}")
}

/// Calculate progress percentage.
///
/// # Arguments
/// * `current` - Current step number (1-indexed)
/// * `total` - Total number of steps
///
/// # Returns
/// Progress percentage (0-100).
///
/// # Example
/// ```rust
/// use aura_app::views::wizards::wizard_progress_percent;
///
/// assert_eq!(wizard_progress_percent(2, 4), 50);
/// assert_eq!(wizard_progress_percent(3, 3), 100);
/// ```
#[must_use]
pub fn wizard_progress_percent(current: u8, total: u8) -> u8 {
    if total == 0 {
        return 0;
    }
    ((current as u16 * 100) / total as u16).min(100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // CreateChannelStep Tests
    // ========================================================================

    #[test]
    fn test_create_channel_step_navigation() {
        assert_eq!(CreateChannelStep::Details.next(), Some(CreateChannelStep::Members));
        assert_eq!(CreateChannelStep::Members.next(), Some(CreateChannelStep::Threshold));
        assert_eq!(CreateChannelStep::Threshold.next(), None);

        assert_eq!(CreateChannelStep::Details.prev(), None);
        assert_eq!(CreateChannelStep::Members.prev(), Some(CreateChannelStep::Details));
        assert_eq!(CreateChannelStep::Threshold.prev(), Some(CreateChannelStep::Members));
    }

    #[test]
    fn test_create_channel_step_boundaries() {
        assert!(CreateChannelStep::Details.is_first());
        assert!(!CreateChannelStep::Details.is_last());

        assert!(!CreateChannelStep::Threshold.is_first());
        assert!(CreateChannelStep::Threshold.is_last());
    }

    #[test]
    fn test_create_channel_step_numbers() {
        assert_eq!(CreateChannelStep::Details.number(), 1);
        assert_eq!(CreateChannelStep::Members.number(), 2);
        assert_eq!(CreateChannelStep::Threshold.number(), 3);
        assert_eq!(CreateChannelStep::total_steps(), 3);
    }

    #[test]
    fn test_create_channel_step_all() {
        let steps = CreateChannelStep::all();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0], CreateChannelStep::Details);
        assert_eq!(steps[2], CreateChannelStep::Threshold);
    }

    // ========================================================================
    // AccountSetupStep Tests
    // ========================================================================

    #[test]
    fn test_account_setup_step_navigation() {
        assert_eq!(AccountSetupStep::Welcome.next(), Some(AccountSetupStep::DisplayName));
        assert_eq!(AccountSetupStep::Complete.next(), None);

        assert_eq!(AccountSetupStep::Welcome.prev(), None);
        assert_eq!(AccountSetupStep::DisplayName.prev(), Some(AccountSetupStep::Welcome));
    }

    #[test]
    fn test_account_setup_step_boundaries() {
        assert!(AccountSetupStep::Welcome.is_first());
        assert!(AccountSetupStep::Complete.is_last());
    }

    // ========================================================================
    // RecoverySetupStep Tests
    // ========================================================================

    #[test]
    fn test_recovery_setup_step_navigation() {
        assert_eq!(RecoverySetupStep::Overview.next(), Some(RecoverySetupStep::SelectGuardians));
        assert_eq!(RecoverySetupStep::Confirm.next(), None);

        assert_eq!(RecoverySetupStep::Overview.prev(), None);
        assert_eq!(RecoverySetupStep::Confirm.prev(), Some(RecoverySetupStep::SetThreshold));
    }

    #[test]
    fn test_recovery_setup_step_total() {
        assert_eq!(RecoverySetupStep::total_steps(), 4);
        assert_eq!(RecoverySetupStep::all().len(), 4);
    }

    // ========================================================================
    // Progress Helper Tests
    // ========================================================================

    #[test]
    fn test_format_wizard_progress() {
        assert_eq!(format_wizard_progress(1, 4), "Step 1 of 4");
        assert_eq!(format_wizard_progress(3, 3), "Step 3 of 3");
    }

    #[test]
    fn test_wizard_progress_percent() {
        assert_eq!(wizard_progress_percent(1, 4), 25);
        assert_eq!(wizard_progress_percent(2, 4), 50);
        assert_eq!(wizard_progress_percent(4, 4), 100);
        assert_eq!(wizard_progress_percent(0, 0), 0);
    }
}
