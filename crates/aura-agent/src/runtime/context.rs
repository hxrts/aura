//! Execution context for effect operations.
//!
//! Aura has two distinct notions of "context":
//! - `AuthorityContext` (aura-agent/core) is identity scope (authority/account + known contexts).
//! - `EffectContext` (aura-core/context) is operation scope (authority + context + session + mode).
//!
//! `EffectContext` is defined canonically in `aura-core` and re-exported here for convenience.

pub use aura_core::context::EffectContext;
