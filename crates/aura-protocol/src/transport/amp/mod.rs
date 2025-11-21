//! AMP (Aura Messaging Protocol) Transport Integration
//!
//! This module provides the complete AMP implementation including:
//! - Core transport functions (send/recv) 
//! - Centralized telemetry and observability
//! - Guard chain integration for authorization and flow budgets
//! - Journal operations for fact-based state management

pub mod telemetry;
pub mod transport;

// Re-export main transport functions
pub use transport::{
    amp_recv, amp_recv_with_receipt, amp_send, commit_bump_with_consensus, emit_proposed_bump,
    prepare_send, validate_header, AmpDelivery, AmpMessage, AmpReceipt,
};

// Re-export telemetry for observability
pub use telemetry::{
    AmpFlowTelemetry, AmpMetrics, AmpProtocolStats, AmpReceiveTelemetry, AmpSendTelemetry,
    AmpTelemetry, WindowValidationResult, AMP_TELEMETRY,
};