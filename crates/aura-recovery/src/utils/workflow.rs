//! Thin workflow helpers for recovery coordinators and handlers.
//!
//! These utilities intentionally stay mechanical: context derivation, trace-id
//! wrapping, fallback physical-time shaping, and journal persistence for
//! `RecoveryFact`.

use crate::facts::{RecoveryFact, RecoveryFactEmitter};
use aura_core::effects::{JournalEffects, PhysicalTimeEffects};
use aura_core::hash;
use aura_core::time::PhysicalTime;
use aura_core::types::identifiers::ContextId;
use aura_core::Result;
use aura_journal::DomainFact;

pub(crate) fn exact_physical_time(ts_ms: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms,
        uncertainty: None,
    }
}

pub(crate) async fn current_physical_time_or_zero(
    time_effects: &dyn PhysicalTimeEffects,
) -> PhysicalTime {
    time_effects
        .physical_time()
        .await
        .unwrap_or_else(|_| exact_physical_time(0))
}

pub(crate) fn context_id_from_operation_id(operation_id: &str) -> ContextId {
    ContextId::new_from_entropy(hash::hash(operation_id.as_bytes()))
}

pub(crate) fn trace_id(operation_id: &str) -> Option<String> {
    Some(operation_id.to_string())
}

pub(crate) async fn persist_recovery_fact(
    journal_effects: &dyn JournalEffects,
    fact: &RecoveryFact,
) -> Result<()> {
    let mut journal = journal_effects.get_journal().await?;
    journal.facts.insert_with_context(
        RecoveryFactEmitter::fact_key(fact),
        aura_core::FactValue::Bytes(DomainFact::to_bytes(fact)),
        aura_core::ActorId::synthetic(&fact.context_id().to_string()),
        aura_core::FactTimestamp::new(fact.timestamp_ms()),
        None,
    )?;
    journal_effects.persist_journal(&journal).await?;
    Ok(())
}
