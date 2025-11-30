//! # Demo Module
//!
//! Provides a simulated backend for the TUI demo mode.
//!
//! The demo system works by injecting a `SimulatedBridge` instead of a production
//! `EffectBridge`. The TUI code is identical - only the backend differs.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                          TuiApp                             │
//! │  (Identical code for production and demo)                   │
//! └───────────────────────────┬─────────────────────────────────┘
//!                             │
//!                             ▼ TuiContext
//!               ┌─────────────────────────────┐
//!               │        EffectBridge         │ (trait)
//!               └─────────────────────────────┘
//!                      │              │
//!          ┌───────────┘              └───────────┐
//!          ▼                                      ▼
//! ┌─────────────────────┐              ┌─────────────────────┐
//! │  ProductionBridge   │              │  SimulatedBridge    │
//! │  (real network)     │              │  (mock responses)   │
//! └─────────────────────┘              └─────────────────────┘
//! ```

mod mock_store;
mod simulated_bridge;
mod tip_provider;

pub use mock_store::MockStore;
pub use simulated_bridge::SimulatedBridge;
pub use tip_provider::{DemoTipProvider, Tip, TipContext, TipProvider};

/// Demo scenario configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DemoScenario {
    /// Happy path - guardians respond quickly
    #[default]
    HappyPath,
    /// One guardian is slow to respond
    SlowGuardian,
    /// Recovery fails (for error handling demo)
    FailedRecovery,
    /// Interactive - no auto-responses, user triggers everything
    Interactive,
}

impl DemoScenario {
    /// Get the description of this scenario
    pub fn description(&self) -> &'static str {
        match self {
            Self::HappyPath => "Happy path demo with quick guardian responses",
            Self::SlowGuardian => "Demo with one slow guardian to show async behavior",
            Self::FailedRecovery => "Demo showing recovery failure handling",
            Self::Interactive => "Interactive demo - user controls all actions",
        }
    }

    /// Get guardian response delays for this scenario (in milliseconds)
    pub fn guardian_delays(&self) -> (u64, u64) {
        match self {
            Self::HappyPath => (500, 800),      // Fast responses
            Self::SlowGuardian => (500, 5000),  // One slow guardian
            Self::FailedRecovery => (500, 500), // Normal timing, will fail
            Self::Interactive => (0, 0),        // No auto-responses
        }
    }

    /// Whether this scenario auto-advances
    pub fn auto_advance(&self) -> bool {
        !matches!(self, Self::Interactive)
    }
}
