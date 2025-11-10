//! Choreographic Tree Handler
//!
//! This handler replaces the old hand-coded tree coordination with
//! choreographic implementations from aura-identity.

use crate::effects::{
    tree::{Cut, Partial, ProposalId, Snapshot},
    TreeEffects,
};
use async_trait::async_trait;
use aura_core::{
    tree::{AttestedOp, Epoch, LeafId, LeafNode, NodeIndex, Policy, TreeOp},
    AccountId, AuraError, AuraResult, DeviceId, Hash32,
};
use aura_identity::{
    handlers::{ChoreographicTreeHandler, TreeHandlerConfig, TreeOperationContext},
    verification::{DeviceInfo, DeviceStatus},
};
use aura_journal::ratchet_tree::TreeState;
use std::collections::HashMap;
use tracing::{error, info, warn};

/// Choreographic tree handler that implements TreeEffects
///
/// This handler replaces old hand-coded tree coordination by delegating
/// to the choreographic implementations in aura-identity.
pub struct ChoreographicTreeEffectHandler {
    /// The choreographic handler from aura-identity
    inner: ChoreographicTreeHandler,
    /// Device ID for this handler
    device_id: DeviceId,
}

impl ChoreographicTreeEffectHandler {
    /// Create a new choreographic tree effect handler
    pub fn new(inner: ChoreographicTreeHandler, device_id: DeviceId) -> Self {
        Self { inner, device_id }
    }

    /// Convert identity operations that now return AuraError directly

    /// Create a basic operation context
    fn create_context(
        &self,
        account_id: AccountId,
        participants: Vec<DeviceId>,
    ) -> TreeOperationContext {
        TreeOperationContext {
            requester: self.device_id,
            account_id,
            participants,
            metadata: HashMap::new(),
        }
    }
}

#[async_trait]
impl TreeEffects for ChoreographicTreeEffectHandler {
    async fn get_current_state(&self) -> AuraResult<TreeState> {
        info!("Getting current tree state");

        // TODO: Integrate with journal to get actual tree state
        // TODO fix - For now, return an error since TreeState implementation is complex
        warn!("Tree state retrieval not fully implemented");
        Err(AuraError::not_found("Tree state retrieval not implemented"))
    }

    async fn get_current_commitment(&self) -> AuraResult<Hash32> {
        info!("Getting current tree commitment");

        // TODO: Implement actual commitment retrieval
        warn!("Tree commitment retrieval not implemented - returning zeros");
        Ok(Hash32([0u8; 32]))
    }

    async fn get_current_epoch(&self) -> AuraResult<u64> {
        info!("Getting current epoch");

        // TODO: Implement actual epoch retrieval
        warn!("Current epoch retrieval not implemented - returning 0");
        Ok(0)
    }

    async fn apply_attested_op(&self, op: AttestedOp) -> AuraResult<Hash32> {
        info!("Applying attested tree operation: {:?}", op.op.op);

        // TODO: Implement actual application via journal
        // TODO fix - For now, just log the operation and return a placeholder commitment
        info!(
            "Tree operation applied: epoch={}, op={:?}",
            op.op.parent_epoch, op.op.op
        );
        Ok(Hash32([0u8; 32])) // Placeholder commitment
    }

    async fn verify_aggregate_sig(&self, op: &AttestedOp, state: &TreeState) -> AuraResult<bool> {
        info!(
            "Verifying aggregate signature for operation: {:?}",
            op.op.op
        );

        // Use the verification system from aura-identity
        let verification_result = self
            .inner
            .verifier()
            .verify_tree_operation(&op.op, self.device_id);

        match verification_result {
            Ok(result) => {
                info!(
                    "Signature verification result: verified={}, confidence={}",
                    result.verified, result.confidence
                );
                Ok(result.verified)
            }
            Err(err) => {
                error!("Signature verification error: {}", err);
                Err(err)
            }
        }
    }

    async fn add_leaf(
        &self,
        leaf: LeafNode,
        under: NodeIndex,
    ) -> AuraResult<aura_core::tree::TreeOpKind> {
        info!("Creating add leaf operation");
        Ok(aura_core::tree::TreeOpKind::AddLeaf { leaf, under })
    }

    async fn remove_leaf(
        &self,
        leaf_id: LeafId,
        reason: u8,
    ) -> AuraResult<aura_core::tree::TreeOpKind> {
        info!("Creating remove leaf operation");
        Ok(aura_core::tree::TreeOpKind::RemoveLeaf {
            leaf: leaf_id,
            reason,
        })
    }

    async fn change_policy(
        &self,
        node: NodeIndex,
        new_policy: Policy,
    ) -> AuraResult<aura_core::tree::TreeOpKind> {
        info!("Creating change policy operation");
        Ok(aura_core::tree::TreeOpKind::ChangePolicy { node, new_policy })
    }

    async fn rotate_epoch(
        &self,
        affected: Vec<NodeIndex>,
    ) -> AuraResult<aura_core::tree::TreeOpKind> {
        info!(
            "Creating rotate epoch operation for {} affected nodes",
            affected.len()
        );
        Ok(aura_core::tree::TreeOpKind::RotateEpoch { affected })
    }

    // Snapshot operations - using placeholder implementations TODO fix - For now

    async fn propose_snapshot(&self, _cut: Cut) -> AuraResult<ProposalId> {
        warn!("Snapshot operations not implemented in choreographic handler");
        Err(AuraError::not_found("Snapshot operations not implemented"))
    }

    async fn approve_snapshot(&self, _proposal_id: ProposalId) -> AuraResult<Partial> {
        warn!("Snapshot operations not implemented in choreographic handler");
        Err(AuraError::not_found("Snapshot operations not implemented"))
    }

    async fn finalize_snapshot(&self, _proposal_id: ProposalId) -> AuraResult<Snapshot> {
        warn!("Snapshot operations not implemented in choreographic handler");
        Err(AuraError::not_found("Snapshot operations not implemented"))
    }

    async fn apply_snapshot(&self, _snapshot: &Snapshot) -> AuraResult<()> {
        warn!("Snapshot operations not implemented in choreographic handler");
        Err(AuraError::not_found("Snapshot operations not implemented"))
    }
}

/// Factory for creating choreographic tree effect handlers
pub struct ChoreographicTreeEffectHandlerFactory;

impl ChoreographicTreeEffectHandlerFactory {
    /// Create a new choreographic tree effect handler
    pub fn create(
        device_id: DeviceId,
        runtime: aura_identity::AuraRuntime,
    ) -> AuraResult<ChoreographicTreeEffectHandler> {
        let config = TreeHandlerConfig {
            default_threshold: 2,
            max_participants: 10,
            strict_verification: true,
        };

        let inner = ChoreographicTreeHandler::new(runtime, device_id, config);

        Ok(ChoreographicTreeEffectHandler::new(inner, device_id))
    }

    /// Register a device with the handler
    pub fn register_device(
        handler: &mut ChoreographicTreeEffectHandler,
        device_info: DeviceInfo,
    ) -> AuraResult<()> {
        handler.inner.register_device(device_info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{Cap, Journal};
    use aura_identity::verification::DeviceInfo;
    // TODO: Add aura-mpst dependency when ready
    // use aura_mpst::AuraRuntime;

    #[tokio::test]
    #[ignore] // TODO: Enable when aura-mpst dependency is added
    async fn test_choreographic_handler_creation() {
        let device_id = DeviceId::new();
        // let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());

        // let handler = ChoreographicTreeEffectHandlerFactory::create(device_id, runtime);
        // assert!(handler.is_ok());
    }

    #[tokio::test]
    async fn test_device_registration() {
        let device_id = DeviceId::new();
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());

        let mut handler =
            ChoreographicTreeEffectHandlerFactory::create(device_id, runtime).unwrap();

        let device_info = DeviceInfo {
            device_id,
            public_key: vec![1, 2, 3, 4],
            capabilities: Cap::top(),
            status: DeviceStatus::Active,
        };

        let result =
            ChoreographicTreeEffectHandlerFactory::register_device(&mut handler, device_info);
        assert!(result.is_ok());
    }
}
