//! Ceremony-related agent helpers.

use super::agent::AuraAgent;

impl AuraAgent {
    /// Get the ceremony tracker for guardian ceremony coordination
    ///
    /// The ceremony tracker manages state for in-progress guardian ceremonies,
    /// including tracking which guardians have accepted invitations and whether
    /// the threshold has been reached.
    ///
    /// # Returns
    /// A cloneable reference to the ceremony tracker service
    pub async fn ceremony_tracker(&self) -> crate::runtime::services::CeremonyTracker {
        self.runtime().ceremony_tracker().clone()
    }

    /// Get the ceremony runner for Category C orchestration
    pub async fn ceremony_runner(
        &self,
    ) -> crate::runtime::services::ceremony_runner::CeremonyRunner {
        self.runtime().ceremony_runner().clone()
    }
}
