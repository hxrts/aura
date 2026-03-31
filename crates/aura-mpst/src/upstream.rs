//! Aura-owned boundary for upstream Telltale surfaces.
//!
//! This module is the single sanctioned place where Layer 2 code names
//! upstream crates directly. Downstream Aura crates should prefer
//! `aura_mpst::upstream::*` over importing upstream Telltale crates by name.
//!
//! The boundary is intentionally small, typed, and version-conscious:
//!
//! - expose only the upstream crates Aura currently needs at the protocol and
//!   compile-time boundary
//! - do not mirror the entire upstream workspace surface here
//! - do not pull VM/runtime embedding or bridge ownership into Layer 2
//!
//! Aura owns the higher-level protocol metadata, capability annotations,
//! ownership, and runtime admission semantics above this layer.

pub use telltale as api;
pub use telltale_choreography as choreography;
pub use telltale_theory as theory;
pub use telltale_types as types;
