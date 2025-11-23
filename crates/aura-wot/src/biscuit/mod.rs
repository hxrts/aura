//! Layer 2: Biscuit Cryptographic Authorization
//!
//! Cryptographic authorization infrastructure for Aura's Web of Trust system using Biscuit tokens.
//! Biscuit provides attenuated, cryptographically-verifiable capabilities with Datalog policy evaluation.
//!
//! **Key Components** (per docs/109_authorization.md):
//! - **BiscuitAuthorizationBridge**: Token generation, attenuation, verification
//! - **Datalog Policy Evaluation**: Logic programming for fine-grained authorization
//! - **Capability Attenuation**: Restrict tokens to specific scope/time/resources
//!
//! **Integration Point**: CapGuard in aura-protocol/guards evaluates Biscuit tokens at message
//! entry point (first guard in chain); enables delegation without trusted intermediaries.

pub mod authorization;

// Re-export the main types for convenience
pub use authorization::{AuthorizationResult, BiscuitAuthorizationBridge};
