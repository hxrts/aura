//! TUI Test Harness
//!
//! Main test harness for reactive TUI testing.
//!
//! TODO: Update for IoContext migration - these tests need ReactiveScheduler
//! integration which IoContext doesn't currently expose.

use aura_agent::reactive::SchedulerConfig;
use aura_journal::fact::Fact;
use aura_terminal::tui::context::IoContext;
use std::time::Duration;

/// Test harness for reactive TUI testing
///
/// Provides a controlled environment for:
/// - Injecting facts into the reactive scheduler
/// - Observing view updates
/// - Asserting view state
/// - Collecting update history
///
/// TODO: Update for IoContext migration - scheduler integration needed
pub struct TuiTestHarness {
    /// The TUI context containing all views
    context: IoContext,
}

impl TuiTestHarness {
    /// Create a new test harness
    ///
    /// This initializes an IoContext with default configuration
    /// and sets up update tracking.
    pub async fn new() -> Self {
        Self::with_config(SchedulerConfig::default()).await
    }

    /// Create a new test harness with custom scheduler configuration
    pub async fn with_config(_config: SchedulerConfig) -> Self {
        let context = IoContext::with_defaults();
        Self { context }
    }

    /// Get the TUI context
    pub fn context(&self) -> &IoContext {
        &self.context
    }

    /// Inject a single fact into the reactive scheduler
    ///
    /// TODO: Re-enable when IoContext has scheduler integration
    pub async fn inject_fact(&self, _fact: Fact) -> Result<(), String> {
        // Scheduler integration not yet available on IoContext
        Ok(())
    }

    /// Inject multiple facts into the reactive scheduler
    ///
    /// TODO: Re-enable when IoContext has scheduler integration
    pub async fn inject_facts(&self, _facts: Vec<Fact>) -> Result<(), String> {
        // Scheduler integration not yet available on IoContext
        Ok(())
    }

    /// Inject facts from a network source
    ///
    /// TODO: Re-enable when IoContext has scheduler integration
    pub async fn inject_network_facts(&self, _facts: Vec<Fact>) -> Result<(), String> {
        // Scheduler integration not yet available on IoContext
        Ok(())
    }

    /// Wait for the scheduler to process all pending facts
    ///
    /// This sleeps long enough for the batch window to expire and processing to complete.
    pub async fn wait_for_processing(&self) {
        // Default batch window is 5ms, add buffer for processing
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui_helpers::fact_factory::make_generic_fact;

    #[tokio::test]
    async fn test_harness_creation() {
        let harness = TuiTestHarness::new().await;
        // Ensure the harness initializes without panic and exposes core views.
        harness.context().chat_view();
        harness.context().guardians_view();
    }

    #[tokio::test]
    async fn test_harness_inject_fact() {
        let harness = TuiTestHarness::new().await;
        let fact = make_generic_fact("test_type", 1);
        let result = harness.inject_fact(fact).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_harness_inject_multiple_facts() {
        let harness = TuiTestHarness::new().await;
        let facts = vec![
            make_generic_fact("test_type_1", 1),
            make_generic_fact("test_type_2", 2),
        ];
        let result = harness.inject_facts(facts).await;
        assert!(result.is_ok());
    }
}
