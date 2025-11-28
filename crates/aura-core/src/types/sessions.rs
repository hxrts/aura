//! Compatibility shim for Epoch types.
//!
//! The epoch types live in `epochs.rs`; this module preserves the legacy
//! `epochs` path for downstream crates during the migration.

pub use crate::types::epochs::*;
