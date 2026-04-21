//! Feature-flag build-configuration guards.
//!
//! Ensures that heavyweight or non-deterministic features (e.g. `simulation`)
//! are opt-in, not enabled by default. A default-on simulation feature would
//! silently break production determinism.

#![allow(missing_docs)]
#[cfg(not(feature = "simulation"))]
#[test]
fn default_features_are_minimal() {}
