//! Authorization contract tests.
//!
//! Verify Biscuit token behavior, attenuation monotonicity, cross-authority
//! isolation, and policy evaluation correctness.

#[allow(clippy::expect_used)]
#[path = "contracts/token_attenuation.rs"]
mod token_attenuation;

#[path = "contracts/common.rs"]
mod common;

#[path = "contracts/biscuit_bridge.rs"]
mod biscuit_bridge;

#[path = "contracts/authorization_isolation.rs"]
mod authorization_isolation;
