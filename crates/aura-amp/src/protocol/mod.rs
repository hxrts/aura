//! AMP (Aura Messaging Protocol) Protocol Orchestration
//!
//! This module orchestrates AMP send/recv through the guard chain, journal,
//! and consensus layers. Telemetry provides structured observability.

pub mod orchestration;
pub mod telemetry;

// Re-export protocol orchestration functions
pub use crate::wire::AmpMessage;
pub use orchestration::{
    amp_recv, amp_recv_with_receipt, amp_send, commit_bump_with_consensus, emit_proposed_bump,
    emit_soft_safe_bump, prepare_send, validate_header, AmpDelivery, AmpReceipt,
};

// Re-export telemetry for observability
pub use telemetry::{AmpTelemetry, WindowValidationResult, AMP_TELEMETRY};
