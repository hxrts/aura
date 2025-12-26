//! # Recovery Modals
//!
//! Guardian setup and threshold modals shared across screens.

mod guardian_setup_modal;
mod threshold_modal;

// Modal exports
pub use guardian_setup_modal::{GuardianCandidateProps, GuardianSetupKind, GuardianSetupModal};
pub use threshold_modal::{ThresholdModal, ThresholdState};
