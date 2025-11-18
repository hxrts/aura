//! Foundation Test Utilities
//!
//! Core testing infrastructure that provides the foundation for all other test utilities.
//! This module contains the base types and functions that other test modules depend on.

// Re-export all foundation functionality from the context module
pub use crate::infrastructure::context::{
    create_integration_context, create_mock_test_context, create_simulation_context,
    SimpleTestContext, TestEffectComposer,
};

// Re-export ExecutionMode from aura-core
pub use aura_core::effects::ExecutionMode;

// Re-export account building functions
pub use crate::builders::account::{
    test_account_with_group_key, test_account_with_id, test_account_with_seed,
    test_account_with_seed_sync, test_account_with_threshold,
};
