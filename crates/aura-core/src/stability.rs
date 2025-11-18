//! API Stability Annotations
//!
//! This module provides stability annotations for the Aura API surface.
//! These annotations help users understand which APIs they can rely on
//! and which may change in future versions.

/// Stability level for API elements
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiStability {
    /// Stable API with semver guarantees
    Stable,
    /// Unstable API that may change
    Unstable,
    /// Internal implementation detail
    Internal,
}

/// Trait for types that have stability annotations
pub trait HasStability {
    /// Get the stability level of this type
    fn stability() -> ApiStability;
}

// For now, these are documentation-only markers.
// In the future, we could implement proc macros for compile-time enforcement.

/// Marks an API as stable with semver guarantees
///
/// Stable APIs follow semantic versioning:
/// - Breaking changes require major version bump
/// - New features added in minor versions
/// - Bug fixes in patch versions
pub struct Stable;

/// Marks an API as unstable and subject to change
///
/// Unstable APIs may change in any release without notice:
/// - Function signatures may change
/// - Types may be removed or restructured
/// - Behavior may change significantly
pub struct Unstable;

/// Marks an API as internal implementation detail
///
/// Internal APIs have no stability guarantees:
/// - May be removed without notice
/// - Not part of the public API contract
/// - Should not be used by external code
pub struct Internal;
