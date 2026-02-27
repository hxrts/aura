//! Runtime capability inventory handler.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use aura_core::effects::{AdmissionError, CapabilityKey, RuntimeCapabilityEffects};

/// Immutable runtime capability inventory captured at runtime boot.
#[derive(Debug, Clone, Default)]
pub struct RuntimeCapabilityHandler {
    inventory: Arc<BTreeMap<CapabilityKey, bool>>,
}

impl RuntimeCapabilityHandler {
    /// Create a handler from an explicit capability snapshot.
    pub fn new(snapshot: Vec<(CapabilityKey, bool)>) -> Self {
        let inventory = snapshot.into_iter().collect::<BTreeMap<_, _>>();
        Self {
            inventory: Arc::new(inventory),
        }
    }

    /// Create a handler from borrowed string capability pairs.
    pub fn from_pairs(
        snapshot: impl IntoIterator<Item = (impl Into<CapabilityKey>, bool)>,
    ) -> Self {
        let inventory = snapshot
            .into_iter()
            .map(|(key, admitted)| (key.into(), admitted))
            .collect::<BTreeMap<_, _>>();
        Self {
            inventory: Arc::new(inventory),
        }
    }

    /// Snapshot size for telemetry/testing.
    pub fn len(&self) -> usize {
        self.inventory.len()
    }

    /// Returns true if the snapshot is empty.
    pub fn is_empty(&self) -> bool {
        self.inventory.is_empty()
    }
}

#[cfg(feature = "telltale-runtime-capability")]
impl RuntimeCapabilityHandler {
    /// Build from Telltale runtime contracts/admission surface.
    pub fn from_runtime_contracts(
        contracts: &telltale_vm::runtime_contracts::RuntimeContracts,
    ) -> Self {
        let mut inventory = telltale_vm::runtime_contracts::runtime_capability_snapshot(contracts)
            .into_iter()
            .map(|(key, admitted)| (CapabilityKey::new(key), admitted))
            .collect::<BTreeMap<_, _>>();

        // Derived Aura capability keys mapped from theorem-pack/runtime contracts.
        inventory.insert(
            CapabilityKey::new("byzantine_envelope"),
            contracts.determinism_artifacts.full,
        );
        inventory.insert(
            CapabilityKey::new("termination_bounded"),
            contracts.determinism_artifacts.replay || contracts.determinism_artifacts.full,
        );
        inventory.insert(
            CapabilityKey::new("reconfiguration"),
            contracts.live_migration && contracts.placement_refinement,
        );
        inventory.insert(
            CapabilityKey::new("mixed_determinism"),
            contracts.can_use_mixed_determinism_profiles,
        );
        inventory.insert(
            CapabilityKey::new("vmEnvelopeAdherence"),
            contracts.determinism_artifacts.full,
        );

        Self {
            inventory: Arc::new(inventory),
        }
    }
}

#[async_trait]
impl RuntimeCapabilityEffects for RuntimeCapabilityHandler {
    async fn capability_inventory(&self) -> Result<Vec<(CapabilityKey, bool)>, AdmissionError> {
        Ok(self
            .inventory
            .iter()
            .map(|(key, admitted)| (key.clone(), *admitted))
            .collect())
    }

    async fn require_capabilities(&self, required: &[CapabilityKey]) -> Result<(), AdmissionError> {
        for required_key in required {
            let admitted = self.inventory.get(required_key).copied().unwrap_or(false);
            if !admitted {
                return Err(AdmissionError::MissingCapability {
                    capability: required_key.clone(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_capability_is_rejected() {
        let handler = RuntimeCapabilityHandler::from_pairs([
            ("vmEnvelopeAdherence", true),
            ("byzantineSafety", false),
        ]);
        let result = handler
            .require_capabilities(&[
                CapabilityKey::new("vmEnvelopeAdherence"),
                CapabilityKey::new("byzantineSafety"),
            ])
            .await;
        assert!(matches!(
            result,
            Err(AdmissionError::MissingCapability { capability })
                if capability == CapabilityKey::new("byzantineSafety")
        ));
    }

    #[tokio::test]
    async fn inventory_round_trip_is_stable() {
        let handler =
            RuntimeCapabilityHandler::from_pairs([("capA", true), ("capB", false), ("capC", true)]);
        let inventory = handler
            .capability_inventory()
            .await
            .expect("inventory should be available");
        assert_eq!(inventory.len(), 3);
        assert!(inventory.contains(&(CapabilityKey::new("capA"), true)));
        assert!(inventory.contains(&(CapabilityKey::new("capB"), false)));
        assert!(inventory.contains(&(CapabilityKey::new("capC"), true)));
    }

    #[cfg(feature = "telltale-runtime-capability")]
    #[test]
    fn runtime_contract_mapping_exposes_derived_aura_capabilities() {
        let contracts = telltale_vm::runtime_contracts::RuntimeContracts::full();
        let handler = RuntimeCapabilityHandler::from_runtime_contracts(&contracts);
        assert!(
            handler
                .inventory
                .get(&CapabilityKey::new("byzantine_envelope"))
                .copied()
                .unwrap_or(false),
            "full contracts should admit byzantine_envelope"
        );
        assert!(
            handler
                .inventory
                .contains_key(&CapabilityKey::new("termination_bounded")),
            "derived termination_bounded key should be present"
        );
        assert!(
            handler
                .inventory
                .contains_key(&CapabilityKey::new("mixed_determinism")),
            "derived mixed_determinism key should be present"
        );
    }
}
