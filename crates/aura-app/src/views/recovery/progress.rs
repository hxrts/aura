//! Portable ceremony-progress tracking.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct CeremonyProgress {
    pub accepted_count: u32,
    pub total_count: u32,
    pub threshold: u32,
}

impl CeremonyProgress {
    #[must_use]
    pub fn new(accepted_count: u32, total_count: u32, threshold: u32) -> Self {
        Self {
            accepted_count,
            total_count,
            threshold,
        }
    }

    #[must_use]
    pub fn is_threshold_met(&self) -> bool {
        self.accepted_count >= self.threshold
    }

    #[must_use]
    pub fn progress_fraction(&self) -> f64 {
        if self.threshold == 0 {
            return 1.0;
        }
        f64::from(self.accepted_count) / f64::from(self.threshold)
    }

    #[must_use]
    pub fn progress_percentage(&self) -> u32 {
        if self.threshold == 0 {
            return 100;
        }
        ((self.accepted_count * 100) / self.threshold).min(100)
    }

    #[must_use]
    pub fn approvals_needed(&self) -> u32 {
        self.threshold.saturating_sub(self.accepted_count)
    }

    #[must_use]
    pub fn can_complete(&self) -> bool {
        self.is_threshold_met()
    }

    #[must_use]
    pub fn status_text(&self) -> String {
        if self.is_threshold_met() {
            format!("{}/{} (ready)", self.accepted_count, self.threshold)
        } else {
            format!(
                "{}/{} ({} more needed)",
                self.accepted_count,
                self.threshold,
                self.approvals_needed()
            )
        }
    }

    pub fn record_acceptance(&mut self) {
        self.accepted_count = self.accepted_count.saturating_add(1);
    }
}
