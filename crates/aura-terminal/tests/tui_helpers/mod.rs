//! # TUI Test Helpers
//!
//! Testing infrastructure for reactive TUI components.
//!
//! This module provides:
//! - `TuiTestHarness`: Main test harness for injecting facts and observing views
//! - `TuiTestRunner`: Runner for executing key sequences against the TUI
//! - Assertion helpers for view state validation
//! - Fact factory functions for creating test data
//!
//! ## Example: Reactive Testing
//!
//! ```ignore
//! #[tokio::test]
//! async fn test_guardian_view_updates() {
//!     let harness = TuiTestHarness::new().await;
//!
//!     harness.inject_fact(make_guardian_binding_fact()).await;
//!
//!     harness.assert_view_eventually(
//!         harness.context().guardians_view(),
//!         |view| async move { !view.guardians().await.is_empty() },
//!         Duration::from_millis(100),
//!     ).await.expect("Guardian should be added");
//! }
//! ```
//!
//! ## Example: Key Sequence Testing
//!
//! ```ignore
//! #[test]
//! fn test_tui_navigation() {
//!     let runner = TuiTestRunner::new(RunnerConfig::demo());
//!     let steps = TestSequenceBuilder::new()
//!         .press("Go to Chat screen", Key::Num(2))
//!         .press_expect("Enter insert mode", Key::Char('i'), "Type a message")
//!         .type_text("Type hello", "hello")
//!         .press("Send message", Key::Enter)
//!         .build();
//!
//!     let result = runner.run_sequence(&steps);
//!     assert!(result.success);
//! }
//! ```

mod assertions;
mod fact_factory;
mod harness;
pub mod mock_runner;
pub mod runner;

#[allow(unused_imports)]
pub use assertions::assert_view_eventually;
#[allow(unused_imports)]
pub use fact_factory::*;
#[allow(unused_imports)]
pub use harness::TuiTestHarness;
pub use mock_runner::{run_mock_test, MockTestBuilder, MockTestResult, MockTestStep};
pub use runner::{Key, RunnerConfig, TestSequenceBuilder, TestStep, TuiTestRunner, VerifyCriteria};
