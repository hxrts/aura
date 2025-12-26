#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods, // Orchestration layer coordinates time/random effects
    deprecated // Deprecated time/random functions used intentionally for effect coordination
)]
//! Session orchestration types
//!
//! Types for managing protocol sessions across distributed participants.
//! Moved from aura-core as these represent orchestration-level concerns.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Session status enumeration
///
/// Represents the current state of a protocol session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionStatus {
    /// Session is initializing (before active execution)
    Initializing,
    /// Session is currently active and executing
    Active,
    /// Session is waiting for responses from participants
    Waiting,
    /// Session completed successfully
    Completed,
    /// Session failed with an error
    Failed,
    /// Session expired due to timeout
    Expired,
    /// Session timed out during execution
    TimedOut,
    /// Session was cancelled
    Cancelled,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionStatus::Initializing => write!(f, "initializing"),
            SessionStatus::Active => write!(f, "active"),
            SessionStatus::Waiting => write!(f, "waiting"),
            SessionStatus::Completed => write!(f, "completed"),
            SessionStatus::Failed => write!(f, "failed"),
            SessionStatus::Expired => write!(f, "expired"),
            SessionStatus::TimedOut => write!(f, "timed-out"),
            SessionStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Session outcome enumeration
///
/// Represents the final result of a protocol session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionOutcome {
    /// Session completed successfully
    Success,
    /// Session failed
    Failed,
    /// Session was aborted
    Aborted,
}

impl fmt::Display for SessionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionOutcome::Success => write!(f, "success"),
            SessionOutcome::Failed => write!(f, "failed"),
            SessionOutcome::Aborted => write!(f, "aborted"),
        }
    }
}
