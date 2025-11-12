//! Tree Operation Coordinator
//!
//! Coordinates distributed tree operations using choreographies.

use crate::tree_ops::choreography_impl::{TreeOpChoreography, TreeOpRole};
use aura_core::{
    tree::{AttestedOp, TreeOp},
    AuraResult, Cap, DeviceId, Policy,
};
use aura_mpst::AuraRuntime;
use std::collections::HashMap;

/// Coordinates tree operations across multiple devices
pub struct TreeOperationCoordinator {
    runtime: AuraRuntime,
    device_id: DeviceId,
}

impl TreeOperationCoordinator {
    /// Create a new coordinator
    pub fn new(runtime: AuraRuntime, device_id: DeviceId) -> Self {
        Self { runtime, device_id }
    }

    /// Execute a tree operation using choreography
    pub async fn execute(
        &self,
        operation: TreeOp,
        epoch: u64,
        policy: Policy,
        participants: Vec<DeviceId>,
        capabilities: HashMap<DeviceId, Cap>,
    ) -> AuraResult<AttestedOp> {
        let role = TreeOpRole::Proposer(self.device_id);
        let choreography =
            TreeOpChoreography::new(role, epoch, policy, capabilities, self.runtime.clone());

        choreography.execute(operation, participants).await
    }
}
