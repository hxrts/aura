//! CLI visualization utilities for rich terminal output.

/// Recovery state visualization
pub mod recovery_status;

pub use recovery_status::{
    format_evidence_list, format_recovery_evidence, format_session_list, format_session_state,
    RecoverySessionState, RecoverySessionStatus,
};
