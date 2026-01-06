#![deny(clippy::dbg_macro)]
#![deny(clippy::todo)]
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
    clippy::disallowed_methods,
    deprecated
)]
//! # Aura AMP - Layer 4: Authenticated Messaging Protocol
//!
//! This crate provides the complete AMP implementation including:
//! - Journal adapters and reduction helpers
//! - Channel lifecycle management
//! - Transport protocol (send/recv)
//! - Telemetry and observability
//! - Consensus integration for epoch bumps
//! - Choreography annotations for MPST integration
//!
//! These glue Layer 4 orchestration to Layer 2 facts without leaking domain types
//! outward. Backed by core `JournalEffects` and storage effects.

// ============================================================================
// Submodules
// ============================================================================

pub mod channel;
pub mod choreography;
pub mod config;
pub mod consensus;
pub mod core;
pub mod evidence;
pub mod journal;
pub mod prelude;
pub mod transport;
pub mod wire;

// ============================================================================
// Re-exports: Journal
// ============================================================================

pub use journal::{get_channel_state, AmpContextStore, AmpJournalEffects};

// ============================================================================
// Re-exports: Evidence
// ============================================================================

pub use evidence::{
    AmpEvidenceEffects, AmpEvidenceStore, EvidenceDelta, EvidenceRecord, AMP_EVIDENCE_KEY_PREFIX,
};

// ============================================================================
// Re-exports: Channel
// ============================================================================

pub use channel::{AmpChannelCoordinator, ChannelMembershipFact, ChannelParticipantEvent};

// ============================================================================
// Re-exports: Transport
// ============================================================================

pub use transport::{
    amp_recv, amp_recv_with_receipt, amp_send, commit_bump_with_consensus, emit_proposed_bump,
    emit_soft_safe_bump, prepare_send, validate_header, AmpDelivery, AmpReceipt, AmpTelemetry,
    WindowValidationResult, AMP_TELEMETRY,
};

// ============================================================================
// Re-exports: Wire
// ============================================================================

pub use wire::{
    deserialize_message as deserialize_amp_message, serialize_message as serialize_amp_message,
    AmpMessage,
};

// ============================================================================
// Re-exports: Consensus
// ============================================================================

pub use consensus::{
    finalize_amp_bump_with_journal, finalize_amp_bump_with_journal_default,
    run_amp_channel_epoch_bump, run_amp_channel_epoch_bump_default,
};

// =============================================================================
// Generated Runner Re-exports for execute_as Pattern
// =============================================================================

/// Re-exports for AmpTransport choreography runners
pub mod amp_runners {
    pub use crate::choreography::rumpsteak_session_types_amp_transport::amp_transport::AmpTransportRole;
    pub use crate::choreography::rumpsteak_session_types_amp_transport::amp_transport::runners::{
        execute_as, run_receiver, run_sender, ReceiverOutput, SenderOutput,
    };
}
