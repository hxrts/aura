//! Privacy analysis and observer models for tree protocols.
//!
//! This module provides tools to measure information leakage in distributed
//! protocols through different observer models with varying capabilities.

pub mod tree_observers;

pub use tree_observers::{
    ExternalObserver, InGroupObserver, NeighborObserver, ObservationEvent, PrivacyAuditReport,
    PrivacyBudget, PrivacyLeakage,
};
