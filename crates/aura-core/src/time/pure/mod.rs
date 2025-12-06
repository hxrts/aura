//! Pure time comparison functions - thin wrappers for Aeneas translation
//!
//! This module exposes the actual `TimeStamp::compare` implementation as
//! free functions for potential Aeneas translation.
//!
//! # Actual Implementation
//!
//! The real comparison logic is in `super::TimeStamp::compare()`:
//!
//! ```rust,ignore
//! impl TimeStamp {
//!     pub fn compare(&self, other: &TimeStamp, policy: OrderingPolicy) -> TimeOrdering {
//!         match (self, other) {
//!             (TimeStamp::PhysicalClock(a), TimeStamp::PhysicalClock(b)) => ...
//!             (TimeStamp::LogicalClock(a), TimeStamp::LogicalClock(b)) => ...
//!             (TimeStamp::OrderClock(a), TimeStamp::OrderClock(b)) => ...
//!             (TimeStamp::Range(a), TimeStamp::Range(b)) => ...
//!             _ => TimeOrdering::Incomparable,
//!         }
//!     }
//! }
//! ```
//!
//! # Lean Correspondence
//!
//! The Lean model in `verification/lean/Aura/TimeSystem.lean` proves:
//! - **Reflexivity**: `compare policy t t = Ordering.eq`
//! - **Transitivity**: proper ordering chains
//! - **Privacy**: OrderClock comparison doesn't leak physical time
//!
//! # Aeneas Notes
//!
//! The main challenges for Aeneas translation:
//! - `TimeStamp` is an enum with 4 variants, each with different types
//! - `VectorClock` has complex partial ordering
//! - Cross-domain comparisons return `Incomparable`

pub mod compare;

pub use compare::*;
