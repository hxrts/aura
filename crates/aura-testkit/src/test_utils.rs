//! Shared test utility helpers for integration and unit tests.

use aura_core::AuraError;

/// Convert arbitrary test errors into a normalized [`AuraError`].
pub fn wrap_test_error<E: std::fmt::Display>(e: E) -> AuraError {
    AuraError::internal(e.to_string())
}
