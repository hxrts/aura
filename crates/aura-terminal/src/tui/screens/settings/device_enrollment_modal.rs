//! # Device Enrollment Modal
//!
//! Shows the out-of-band enrollment code and ceremony progress for Settings → Add device.
//! Uses the shared CodeDisplayModal component.

use iocraft::prelude::*;

use crate::tui::components::{CodeDisplayModal, CodeDisplayStatus};
use aura_core::threshold::AgreementMode;

#[derive(Default, Props)]
pub struct DeviceEnrollmentModalProps {
    pub visible: bool,
    pub nickname_suggestion: String,
    pub enrollment_code: String,
    pub accepted_count: u16,
    pub total_count: u16,
    pub threshold: u16,
    pub is_complete: bool,
    pub has_failed: bool,
    pub error_message: String,
    pub agreement_mode: AgreementMode,
    pub reversion_risk: bool,
    /// Whether code was copied to clipboard
    pub copied: bool,
    /// Demo mode: show hint for 'm' key to simulate mobile import
    pub is_demo_mode: bool,
}

#[component]
pub fn DeviceEnrollmentModal(props: &DeviceEnrollmentModalProps) -> impl Into<AnyElement<'static>> {
    let status = if props.has_failed {
        CodeDisplayStatus::Error
    } else if props.is_complete {
        CodeDisplayStatus::Success
    } else {
        CodeDisplayStatus::Pending
    };

    let status_text = if props.has_failed {
        "Enrollment failed".to_string()
    } else if props.is_complete {
        "Enrollment complete".to_string()
    } else {
        match props.agreement_mode {
            AgreementMode::Provisional => "Waiting for acceptance…".to_string(),
            AgreementMode::CoordinatorSoftSafe => {
                if props.reversion_risk {
                    "Soft-safe (reversion risk)".to_string()
                } else {
                    "Soft-safe".to_string()
                }
            }
            AgreementMode::ConsensusFinalized => "Finalized".to_string(),
        }
    };

    let step_title = if props.is_complete {
        format!("Add Device — Step 3 of 3: {}", props.nickname_suggestion)
    } else {
        "Import this code on the new device".to_string()
    };

    let progress_text = format!(
        "{}/{} accepted (need {})",
        props.accepted_count, props.total_count, props.threshold
    );

    element! {
        CodeDisplayModal(
            visible: props.visible,
            title: step_title,
            status: status,
            status_text: status_text,
            progress_text: progress_text,
            instruction: String::new(),
            code: props.enrollment_code.clone(),
            error_message: props.error_message.clone(),
            copied: props.copied,
            show_mobile_hint: props.is_demo_mode,
        )
    }
}
