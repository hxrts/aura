//! Runtime capability inventory handler.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use aura_core::effects::{AdmissionError, CapabilityKey, RuntimeCapabilityEffects};

#[cfg(feature = "telltale-runtime-capability")]
use telltale_machine::capabilities::protocol_critical_capability_boundary;

#[cfg(any(test, not(feature = "telltale-runtime-capability")))]
const PROTOCOL_SURFACE_RUNTIME_ADMISSION: &str = "runtime_admission";
#[cfg(any(test, not(feature = "telltale-runtime-capability")))]
const PROTOCOL_SURFACE_THEOREM_PACK_CAPABILITIES: &str = "theorem_pack_capabilities";
#[cfg(any(test, not(feature = "telltale-runtime-capability")))]
const PROTOCOL_SURFACE_OWNERSHIP_CAPABILITY: &str = "ownership_capability";
#[cfg(any(test, not(feature = "telltale-runtime-capability")))]
const PROTOCOL_SURFACE_READINESS_WITNESS: &str = "readiness_witness";
#[cfg(any(test, not(feature = "telltale-runtime-capability")))]
const PROTOCOL_SURFACE_AUTHORITATIVE_READ: &str = "authoritative_read";
#[cfg(any(test, not(feature = "telltale-runtime-capability")))]
const PROTOCOL_SURFACE_MATERIALIZATION_PROOF: &str = "materialization_proof";
#[cfg(any(test, not(feature = "telltale-runtime-capability")))]
const PROTOCOL_SURFACE_CANONICAL_HANDLE: &str = "canonical_handle";
const PROTOCOL_SURFACE_OWNERSHIP_RECEIPT: &str = "ownership_receipt";
const PROTOCOL_SURFACE_SEMANTIC_HANDOFF: &str = "semantic_handoff";
const PROTOCOL_SURFACE_RECONFIGURATION_TRANSITION: &str = "reconfiguration_transition";

/// Immutable runtime capability inventory captured at runtime boot.
#[derive(Debug, Clone, Default)]
pub struct RuntimeCapabilityHandler {
    inventory: Arc<BTreeMap<CapabilityKey, bool>>,
    protocol_critical_surfaces: Arc<BTreeMap<String, bool>>,
}

impl RuntimeCapabilityHandler {
    /// Create a handler from an explicit capability snapshot.
    pub fn new(snapshot: Vec<(CapabilityKey, bool)>) -> Self {
        let inventory = snapshot.into_iter().collect::<BTreeMap<_, _>>();
        Self {
            inventory: Arc::new(inventory),
            protocol_critical_surfaces: Arc::new(protocol_surface_inventory_from_aura_capabilities(
                &BTreeMap::new(),
            )),
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
            protocol_critical_surfaces: Arc::new(protocol_surface_inventory_from_aura_capabilities(
                &inventory,
            )),
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

    /// Returns whether one public protocol-critical surface is admitted.
    pub fn protocol_critical_surface_admitted(&self, surface: &str) -> bool {
        self.protocol_critical_surfaces
            .get(surface)
            .copied()
            .unwrap_or(false)
    }

    /// Require that the given public protocol-critical surfaces are admitted.
    pub fn require_protocol_critical_surfaces(
        &self,
        required: &[&str],
    ) -> Result<(), AdmissionError> {
        for surface in required {
            if !self.protocol_critical_surface_admitted(surface) {
                return Err(AdmissionError::MissingCapability {
                    capability: CapabilityKey::new(*surface),
                });
            }
        }
        Ok(())
    }
}

#[cfg(feature = "telltale-runtime-capability")]
impl RuntimeCapabilityHandler {
    /// Build from the current ProtocolMachine runtime contracts/admission surface.
    pub fn from_protocol_machine_runtime_contracts(
        contracts: &telltale_machine::runtime_contracts::RuntimeContracts,
    ) -> Self {
        let mut inventory =
            telltale_machine::runtime_contracts::runtime_capability_snapshot(contracts)
                .into_iter()
                .map(|(key, admitted)| (CapabilityKey::new(key), admitted))
                .collect::<BTreeMap<_, _>>();
        let protocol_critical_surfaces = protocol_surface_inventory_from_runtime_contracts(contracts);

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
            contracts
                .capabilities
                .contains(&telltale_machine::runtime_contracts::RuntimeCapability::LiveMigration)
                && contracts.capabilities.contains(
                    &telltale_machine::runtime_contracts::RuntimeCapability::PlacementRefinement,
                ),
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
            protocol_critical_surfaces: Arc::new(protocol_critical_surfaces),
        }
    }
}

fn protocol_surface_inventory_from_aura_capabilities(
    inventory: &BTreeMap<CapabilityKey, bool>,
) -> BTreeMap<String, bool> {
    let reconfiguration_enabled = inventory
        .get(&CapabilityKey::new("reconfiguration"))
        .copied()
        .unwrap_or(false);
    #[cfg(feature = "telltale-runtime-capability")]
    {
        protocol_surface_inventory_from_boundary(reconfiguration_enabled)
    }
    #[cfg(not(feature = "telltale-runtime-capability"))]
    {
        protocol_surface_inventory_from_known_surfaces(reconfiguration_enabled)
    }
}

#[cfg(feature = "telltale-runtime-capability")]
fn protocol_surface_inventory_from_runtime_contracts(
    contracts: &telltale_machine::runtime_contracts::RuntimeContracts,
) -> BTreeMap<String, bool> {
    let reconfiguration_enabled = contracts
        .capabilities
        .contains(&telltale_machine::runtime_contracts::RuntimeCapability::LiveMigration)
        && contracts.capabilities.contains(
            &telltale_machine::runtime_contracts::RuntimeCapability::PlacementRefinement,
        );
    protocol_surface_inventory_from_boundary(reconfiguration_enabled)
}

#[cfg(feature = "telltale-runtime-capability")]
fn protocol_surface_inventory_from_boundary(reconfiguration_enabled: bool) -> BTreeMap<String, bool> {
    protocol_critical_capability_boundary()
        .into_iter()
        .map(|entry| {
            let admitted = match entry.surface.as_str() {
                PROTOCOL_SURFACE_OWNERSHIP_RECEIPT
                | PROTOCOL_SURFACE_SEMANTIC_HANDOFF
                | PROTOCOL_SURFACE_RECONFIGURATION_TRANSITION => reconfiguration_enabled,
                _ => true,
            };
            (entry.surface, admitted)
        })
        .collect()
}

#[cfg(not(feature = "telltale-runtime-capability"))]
fn protocol_surface_inventory_from_known_surfaces(
    reconfiguration_enabled: bool,
) -> BTreeMap<String, bool> {
    let always_admitted = [
        PROTOCOL_SURFACE_RUNTIME_ADMISSION,
        PROTOCOL_SURFACE_THEOREM_PACK_CAPABILITIES,
        PROTOCOL_SURFACE_OWNERSHIP_CAPABILITY,
        PROTOCOL_SURFACE_READINESS_WITNESS,
        PROTOCOL_SURFACE_AUTHORITATIVE_READ,
        PROTOCOL_SURFACE_MATERIALIZATION_PROOF,
        PROTOCOL_SURFACE_CANONICAL_HANDLE,
    ];
    let transition_surfaces = [
        PROTOCOL_SURFACE_OWNERSHIP_RECEIPT,
        PROTOCOL_SURFACE_SEMANTIC_HANDOFF,
        PROTOCOL_SURFACE_RECONFIGURATION_TRANSITION,
    ];

    let mut admitted = always_admitted
        .into_iter()
        .map(|surface| (surface.to_string(), true))
        .collect::<BTreeMap<_, _>>();
    for surface in transition_surfaces {
        admitted.insert(surface.to_string(), reconfiguration_enabled);
    }
    admitted
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
#[allow(clippy::expect_used)]
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
    fn protocol_machine_runtime_contract_mapping_exposes_derived_aura_capabilities() {
        let contracts = telltale_machine::runtime_contracts::RuntimeContracts::full();
        let handler = RuntimeCapabilityHandler::from_protocol_machine_runtime_contracts(&contracts);
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
        assert!(
            handler.protocol_critical_surface_admitted(PROTOCOL_SURFACE_OWNERSHIP_CAPABILITY),
            "public ownership capability surface should be tracked"
        );
        assert!(
            handler.protocol_critical_surface_admitted(PROTOCOL_SURFACE_RECONFIGURATION_TRANSITION),
            "full contracts should admit public reconfiguration transition surface"
        );
    }

    #[test]
    fn missing_public_protocol_surface_is_rejected() {
        let handler = RuntimeCapabilityHandler::from_pairs([("reconfiguration", false)]);
        let result =
            handler.require_protocol_critical_surfaces(&[PROTOCOL_SURFACE_RECONFIGURATION_TRANSITION]);
        assert!(matches!(
            result,
            Err(AdmissionError::MissingCapability { capability })
                if capability == CapabilityKey::new(PROTOCOL_SURFACE_RECONFIGURATION_TRANSITION)
        ));
    }
}
