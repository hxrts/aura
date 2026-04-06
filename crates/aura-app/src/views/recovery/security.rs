//! Recovery-threshold security classification.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum SecurityLevel {
    #[default]
    None,
    Low,
    Medium,
    High,
    Maximum,
}

impl SecurityLevel {
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::None => "No guardians configured yet",
            Self::Low => "Low security: Any single guardian can recover",
            Self::Medium => "Medium security: Less than majority required",
            Self::High => "High security: Majority required",
            Self::Maximum => "Maximum security: All guardians required",
        }
    }

    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Maximum => "Maximum",
        }
    }

    #[must_use]
    pub fn is_recommended(&self) -> bool {
        matches!(self, Self::Medium | Self::High | Self::Maximum)
    }
}

#[must_use]
pub fn classify_threshold_security(threshold: u32, guardian_count: u32) -> SecurityLevel {
    if guardian_count == 0 {
        SecurityLevel::None
    } else if threshold == guardian_count {
        SecurityLevel::Maximum
    } else if threshold == 1 {
        SecurityLevel::Low
    } else {
        let majority = (guardian_count / 2) + 1;
        if threshold >= majority {
            SecurityLevel::High
        } else {
            SecurityLevel::Medium
        }
    }
}

#[must_use]
pub fn security_level_hint(threshold: u32, guardian_count: u32) -> String {
    classify_threshold_security(threshold, guardian_count)
        .description()
        .to_string()
}
