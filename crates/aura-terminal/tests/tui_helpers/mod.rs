//! # TUI Test Helpers
//!
//! Testing infrastructure for reactive TUI components.
//!
//! This module provides:
//! - `TuiTestHarness`: Main test harness for injecting facts and observing views
//! - Assertion helpers for view state validation
//! - Fact factory functions for creating test data
//!
//! ## Example
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

mod assertions;
mod fact_factory;
mod harness;

pub use assertions::assert_view_eventually;
pub use fact_factory::*;
pub use harness::TuiTestHarness;
