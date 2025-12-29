//! AMP (Aura Messaging Protocol) Transport Integration
//!
//! This module provides the complete AMP implementation including:
//! - Core transport functions (send/recv)
//! - Centralized telemetry and observability
//! - Guard chain integration for authorization and flow budgets
//! - Journal operations for fact-based state management

pub mod protocol;
pub mod telemetry;

// Re-export main transport functions
pub use crate::wire::AmpMessage;
pub use protocol::{
    amp_recv, amp_recv_with_receipt, amp_send, commit_bump_with_consensus, emit_proposed_bump,
    emit_soft_safe_bump, prepare_send, validate_header, AmpDelivery, AmpReceipt,
};

// Re-export telemetry for observability
pub use telemetry::{AmpTelemetry, WindowValidationResult, AMP_TELEMETRY};
