//! Journal registration for Layer 2 maintenance facts.
//!
//! `aura-maintenance` is a Layer 2 crate and cannot depend on `aura-journal`,
//! so the `DomainFact` registration lives here. `MaintenanceJournalFact` is a
//! transparent wrapper: its derive-codec payload is byte-identical to
//! `MaintenanceFact::to_envelope`, so facts committed by the app workflows
//! decode through the same shared registry path.

use aura_core::types::identifiers::ContextId;
use aura_journal::reduction::{RelationalBinding, RelationalBindingType};
use aura_journal::{DomainFact, FactReducer};
use aura_macros::DomainFact;
use aura_maintenance::{
    MaintenanceFact, MAINTENANCE_FACT_SCHEMA_VERSION, MAINTENANCE_FACT_TYPE_ID,
};
use serde::{Deserialize, Serialize};

/// Registry type id for maintenance facts.
pub fn maintenance_journal_fact_type_id() -> &'static str {
    MAINTENANCE_FACT_TYPE_ID.as_str()
}

/// Journal-registered view of a [`MaintenanceFact`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DomainFact)]
#[serde(transparent)]
#[domain_fact(
    type_id = maintenance_journal_fact_type_id(),
    schema_version = MAINTENANCE_FACT_SCHEMA_VERSION,
    context_fn = "fact_context_id"
)]
pub struct MaintenanceJournalFact(pub MaintenanceFact);

impl MaintenanceJournalFact {
    fn fact_context_id(&self) -> ContextId {
        self.0.context_id()
    }
}

/// Registry reducer for maintenance facts.
#[derive(Debug, Clone, Default)]
pub struct MaintenanceJournalFactReducer;

impl FactReducer for MaintenanceJournalFactReducer {
    fn handles_type(&self) -> &'static str {
        maintenance_journal_fact_type_id()
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<RelationalBinding> {
        let fact = MaintenanceJournalFact::from_envelope(envelope)?;
        if fact.0.context_id() != context_id {
            return None;
        }
        let key = fact.0.binding_key();
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
            context_id,
            data: key.data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::Epoch;
    use aura_core::AuthorityId;
    use aura_maintenance::{CacheInvalidated, CacheKey};

    #[test]
    fn wrapper_envelope_matches_layer2_envelope() {
        let fact = MaintenanceFact::CacheInvalidated(CacheInvalidated::new(
            AuthorityId::new_from_entropy([7; 32]),
            vec![CacheKey("k".to_string())],
            Epoch::new(3),
        ));
        let layer2 = fact.to_envelope().expect("envelope");
        let wrapped = MaintenanceJournalFact(fact.clone()).to_envelope();
        assert_eq!(layer2, wrapped);

        let binding = MaintenanceJournalFactReducer
            .reduce_envelope(fact.context_id(), &layer2)
            .expect("binding");
        assert_eq!(
            binding.binding_type,
            RelationalBindingType::Generic("cache-invalidated".to_string())
        );
        assert!(MaintenanceJournalFactReducer
            .reduce_envelope(ContextId::new_from_entropy([1; 32]), &layer2)
            .is_none());
    }
}
