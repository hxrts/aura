//! Execution utilities for integrating handlers with choreographic protocols

use aura_core::identifiers::{DeviceId, SessionId};
use aura_core::semilattice::{CausalOp, CmApply, CvState, Dedup, Delta};
use aura_journal::crdt::{CmHandler, CvHandler, DeltaHandler};
use aura_journal::CausalContext;

/// Execute state-based CRDT synchronization
///
/// Performs CvRDT synchronization by broadcasting state to peers.
pub async fn execute_cv_sync<S: CvState>(
    handler: &mut CvHandler<S>,
    peers: Vec<DeviceId>,
    _session_id: SessionId,
) -> Result<(), Box<dyn std::error::Error>> {
    // Produce a state message for each peer; caller is responsible for transport.
    let state_msg = handler.create_state_msg();
    for _peer in peers {
        // In choreography integration this would enqueue to SendGuard; here we only
        // exercise the handler to ensure join semantics hold.
        let _ = state_msg.clone();
    }
    Ok(())
}

/// Execute delta-based CRDT gossip
pub async fn execute_delta_gossip<S>(
    handler: &mut DeltaHandler<S, S::Delta>,
    peers: Vec<DeviceId>,
    _session_id: SessionId,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: CvState + aura_core::semilattice::DeltaState,
    S::Delta: Delta + Clone,
{
    // Drain pending deltas so they are applied locally and ready for dissemination.
    let deltas: Vec<S::Delta> = handler.delta_inbox.drain(..).collect();

    if !deltas.is_empty() {
        // Apply to local state to maintain convergence guarantees.
        handler.apply_deltas(deltas.clone());

        // Materialize transport-ready delta messages for each peer.
        for delta in deltas {
            let msg = handler.create_delta_msg(delta.clone());
            for _peer in &peers {
                // In choreography integration this would enqueue to SendGuard.
                let _ = msg.clone();
            }
        }
    }

    Ok(())
}

/// Execute operation-based CRDT broadcast
pub async fn execute_op_broadcast<S, Op, Id>(
    handler: &mut CmHandler<S, Op, Id>,
    peers: Vec<DeviceId>,
    _session_id: SessionId,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: CmApply<Op> + Dedup<Id>,
    Op: CausalOp<Id = Id, Ctx = CausalContext> + Clone,
    Id: Clone + PartialEq,
{
    // Broadcast any causally buffered operations; dedup semantics in CmHandler
    // ensure safe replays if dependencies were unresolved.
    let buffered: Vec<_> = handler.buffer.iter().cloned().collect();
    if !buffered.is_empty() {
        for op_with_ctx in buffered {
            for _peer in &peers {
                let _msg = handler.create_op_msg(op_with_ctx.op.clone(), op_with_ctx.ctx.clone());
            }
        }
    }
    Ok(())
}
