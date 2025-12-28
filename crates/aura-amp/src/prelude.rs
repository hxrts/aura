//! Aura AMP prelude.
//!
//! Curated re-exports for AMP orchestration.

pub use crate::consensus::{
    finalize_amp_bump_with_journal, finalize_amp_bump_with_journal_default,
    run_amp_channel_epoch_bump, run_amp_channel_epoch_bump_default,
};
pub use crate::{
    amp_recv, amp_recv_with_receipt, amp_send, commit_bump_with_consensus, emit_proposed_bump,
    prepare_send, validate_header, AmpChannelCoordinator, AmpDelivery, AmpEvidenceEffects,
    AmpJournalEffects, AmpMessage, AmpReceipt, AmpTelemetry, ChannelMembershipFact,
    ChannelParticipantEvent,
};

/// Composite effect requirements for AMP orchestration (excludes StorageEffects by default).
pub trait AmpEffects:
    AmpJournalEffects
    + aura_core::effects::OrderClockEffects
    + aura_core::effects::RandomEffects
    + aura_core::effects::time::PhysicalTimeEffects
    + aura_guards::GuardEffects
    + aura_guards::GuardContextProvider
    + aura_core::effects::NetworkEffects
    + aura_core::effects::CryptoEffects
{
}

impl<T> AmpEffects for T where
    T: AmpJournalEffects
        + aura_core::effects::OrderClockEffects
        + aura_core::effects::RandomEffects
        + aura_core::effects::time::PhysicalTimeEffects
        + aura_guards::GuardEffects
        + aura_guards::GuardContextProvider
        + aura_core::effects::NetworkEffects
        + aura_core::effects::CryptoEffects
{
}
