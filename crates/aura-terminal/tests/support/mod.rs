//! Integration-test shared helpers for aura-terminal.
//!
//! This module provides reusable test infrastructure including:
//!
//! - [`env`]: Test environment setup (`SimpleTestEnv`, `FullTestEnv`)
//! - [`state_machine`]: Pure TUI state machine testing (`TestTui`)
//! - [`signals`]: Signal waiting and polling helpers
//! - [`demo`]: Demo-specific helpers (invitation codes, agent IDs)
//!
//! # Quick Start
//!
//! ```ignore
//! mod support;
//!
//! use support::{SimpleTestEnv, TestTui, wait_for_chat};
//!
//! #[tokio::test]
//! async fn my_test() {
//!     let env = SimpleTestEnv::new("my-test").await;
//!     // ... test logic
//! }
//!
//! #[test]
//! fn my_state_machine_test() {
//!     let mut tui = TestTui::new();
//!     tui.send_char('2');
//!     tui.assert_screen(Screen::Neighborhood);
//! }
//! ```

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

// ============================================================================
// Submodules
// ============================================================================

pub mod demo;
pub mod env;
pub mod signals;
pub mod state_machine;

// ============================================================================
// Re-exports for convenience
// ============================================================================

// Environment types
pub use env::{cleanup_test_dir, setup_test_env, FullTestEnv, FullTestEnvConfig, SimpleTestEnv};

// State machine testing
pub use state_machine::TestTui;

// Signal helpers - commonly used
pub use signals::{
    wait_for_chat, wait_for_contact, wait_for_contacts, wait_for_device, wait_for_devices,
    wait_for_invitations, wait_for_neighborhood, wait_for_recovery, wait_for_settings,
    wait_for_signal, DEFAULT_TIMEOUT, EXTENDED_TIMEOUT,
};

// Demo helpers
pub use demo::{
    alice_authority_id, alice_invite_code, carol_authority_id, carol_invite_code,
    generate_demo_invite_code, DEFAULT_DEMO_SEED,
};

// ============================================================================
// Utility Functions
// ============================================================================

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Create a unique, empty directory under the OS temp directory.
///
/// Avoids disallowed randomness (`Uuid::new_v4`) while still remaining safe for
/// parallel test execution by using `create_dir` collision avoidance.
pub fn unique_test_dir(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir();
    let mut attempt = COUNTER.fetch_add(1, Ordering::Relaxed);

    for _ in 0..1024 {
        let dir = root.join(format!("{prefix}-{attempt}"));
        match std::fs::create_dir(&dir) {
            Ok(()) => return dir,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                attempt = attempt.wrapping_add(1);
                continue;
            }
            Err(_) => {
                attempt = attempt.wrapping_add(1);
                continue;
            }
        }
    }

    root.join(prefix)
}
