//! OTA upgrade orchestration helpers.

use std::collections::HashMap;

use aura_core::{
    maintenance::{MaintenanceEvent, UpgradeActivated, UpgradeKind, UpgradeProposal},
    AuraError, AuraResult, DeviceId, Epoch, SemanticVersion,
};
use uuid::Uuid;

/// Tracks readiness facts for each device.
#[derive(Debug, Default)]
pub struct UpgradeReadiness {
    readiness: HashMap<DeviceId, SemanticVersion>,
}

impl UpgradeReadiness {
    /// Record that `device` supports `version`.
    pub fn record(&mut self, device: DeviceId, version: SemanticVersion) {
        let entry = self.readiness.entry(device).or_insert(version);
        if version > *entry {
            *entry = version;
        }
    }

    /// Count how many devices have opted in to `version` or higher.
    pub fn quorum_count(&self, version: &SemanticVersion) -> usize {
        self.readiness
            .values()
            .filter(|supported| *supported >= version)
            .count()
    }
}

/// Tracks ongoing upgrades and emits activation events when ready.
#[derive(Debug, Default)]
pub struct UpgradeCoordinator {
    proposals: HashMap<Uuid, UpgradeProposal>,
    readiness: UpgradeReadiness,
}

impl UpgradeCoordinator {
    /// Create a new coordinator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new upgrade proposal.
    pub fn propose(&mut self, proposal: UpgradeProposal) -> AuraResult<()> {
        proposal.validate()?;
        if self.proposals.contains_key(&proposal.package_id) {
            return Err(AuraError::coordination_failed(
                "upgrade proposal already registered",
            ));
        }
        self.proposals.insert(proposal.package_id, proposal);
        Ok(())
    }

    /// Record device readiness (e.g., after operator approval or auto opt-in).
    pub fn record_readiness(&mut self, device: DeviceId, version: SemanticVersion) {
        self.readiness.record(device, version);
    }

    /// Try to activate the given package. Returns a MaintenanceEvent if activation is allowed.
    pub fn try_activate(
        &mut self,
        package_id: Uuid,
        quorum_threshold: usize,
        current_epoch: Epoch,
    ) -> AuraResult<Option<MaintenanceEvent>> {
        let proposal = self
            .proposals
            .get(&package_id)
            .ok_or_else(|| AuraError::coordination_failed("unknown upgrade package"))?;

        match proposal.kind {
            UpgradeKind::SoftFork => {
                if self.readiness.quorum_count(&proposal.version) == 0 {
                    return Ok(None);
                }
                // Soft forks simply advertise readiness; no activation fence event is emitted.
                Ok(None)
            }
            UpgradeKind::HardFork => {
                let fence = proposal.activation_fence.ok_or_else(|| {
                    AuraError::coordination_failed("hard fork proposal missing fence")
                })?;
                if fence.epoch > current_epoch {
                    return Ok(None);
                }
                if self.readiness.quorum_count(&proposal.version) < quorum_threshold {
                    return Ok(None);
                }
                let event = MaintenanceEvent::UpgradeActivated(UpgradeActivated::new(
                    proposal.package_id,
                    proposal.version,
                    fence,
                ));
                Ok(Some(event))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{hash_canonical, maintenance::IdentityEpochFence, AccountId};

    fn dummy_proposal(kind: UpgradeKind, fence: Option<IdentityEpochFence>) -> UpgradeProposal {
        UpgradeProposal {
            package_id: Uuid::new_v4(),
            version: SemanticVersion::new(1, 2, 0),
            artifact_hash: aura_core::Hash32(hash_canonical(b"bundle").unwrap()),
            artifact_uri: Some("https://example.com/bundle".into()),
            kind,
            activation_fence: fence,
        }
    }

    #[test]
    fn hard_fork_requires_fence_and_quorum() {
        let fence = IdentityEpochFence::new(AccountId::from_bytes([0u8; 32]), 10_u64);
        let mut coordinator = UpgradeCoordinator::new();
        let proposal = dummy_proposal(UpgradeKind::HardFork, Some(fence));
        let package = proposal.package_id;
        coordinator.propose(proposal).unwrap();

        // before readiness or epoch => no activation
        assert!(coordinator
            .try_activate(package, 1, 5_u64)
            .unwrap()
            .is_none());

        coordinator.record_readiness(DeviceId::new(), SemanticVersion::new(1, 2, 0));
        // epoch still below fence
        assert!(coordinator
            .try_activate(package, 1, 5_u64)
            .unwrap()
            .is_none());

        // fence reached -> activation event emitted
        let event = coordinator
            .try_activate(package, 1, 10_u64)
            .unwrap()
            .expect("activation event");
        matches!(event, MaintenanceEvent::UpgradeActivated(_));
    }
}
