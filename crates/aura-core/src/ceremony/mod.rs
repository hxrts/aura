//! Ceremony Types - Layer 1
//!
//! Core ceremony types for Category C operations. Ceremonies are blocking,
//! multi-step operations that must either commit atomically or abort cleanly.
//!
//! See `docs/117_operation_categories.md` and `docs/118_key_rotation_ceremonies.md`
//! for the ceremony contract and lifecycle.

pub mod facts;
pub mod supersession;

pub use facts::{apply_standard_fact, CeremonyFactMeta, StandardCeremonyFact};
pub use supersession::{SupersessionReason, SupersessionRecord};
