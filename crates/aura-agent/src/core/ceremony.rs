//! Ceremony-related agent helpers.

use super::agent::AuraAgent;
use super::AgentResult;
use crate::core::ceremony_processor::CeremonyProcessor;

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

    /// Process guardian ceremony acceptances and auto-complete when threshold is reached
    ///
    /// This method should be called periodically (e.g., in a background task) to:
    /// 1. Poll for incoming guardian acceptance messages via transport
    /// 2. Update the ceremony tracker with each acceptance
    /// 3. Automatically commit ceremonies when threshold is reached
    ///
    /// # Returns
    /// Number of acceptances processed and number of ceremonies completed
    pub async fn process_ceremony_acceptances(&self) -> AgentResult<(usize, usize)> {
        let ceremony_tracker = self.ceremony_tracker().await;
        let authority_id = self.authority_id();
        let effects = self.runtime().effects();
        let signing_service = self.threshold_signing();

        CeremonyProcessor::new(
            authority_id,
            effects.as_ref(),
            ceremony_tracker,
            signing_service,
        )
        .process_all()
        .await
    }
}
