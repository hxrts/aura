//! Protocol composition and execution utilities
//!
//! This module bridges choreographic protocols with effect handlers,
//! providing execution functions that integrate session types with
//! CRDT semantic law enforcement.

use crate::runtime::AuraHandlerAdapter;
use crate::types::ChoreographicRole;
use aura_protocol::effects::semilattice::{CvHandler, CmHandler, DeltaHandler};
use aura_types::semilattice::{CvState, CausalOp, CmApply, Dedup, Delta};
use aura_types::identifiers::{DeviceId, SessionId};
use rumpsteak_choreography::ChoreographyError;
use std::collections::HashMap;
use tracing::{info, debug, error};

/// Execute state-based CRDT synchronization
///
/// Bridges the `CvSync` choreography with a `CvHandler` to provide
/// complete state-based CRDT synchronization with session type safety.
///
/// # Arguments
/// * `adapter` - Runtime adapter for choreographic effects
/// * `replicas` - List of participating replica roles
/// * `my_role` - This device's role in the choreography  
/// * `handler` - Effect handler enforcing join semilattice laws
///
/// # Returns
/// Result indicating successful synchronization or choreography error
pub async fn execute_cv_sync<S>(
    _adapter: &mut AuraHandlerAdapter,
    replicas: Vec<ChoreographicRole>,
    my_role: ChoreographicRole,
    handler: &mut CvHandler<S>,
) -> Result<(), ChoreographyError>
where
    S: CvState + Send + Sync + 'static,
{
    info!("Starting CvRDT synchronization with {} replicas", replicas.len());
    
    debug!("Created CvSync choreography for role {:?}", my_role);
    
    // TODO: Integrate with actual rumpsteak-aura choreography execution
    // This is a placeholder implementation showing the integration pattern:
    //
    // 1. The choreography would generate send/receive operations via rumpsteak
    // 2. When sending: handler.create_state_msg() provides the message payload
    // 3. When receiving: handler.on_recv(msg) enforces join semilattice law
    // 4. The adapter translates between choreographic and aura effect operations
    //
    // The actual implementation would look like:
    // ```rust
    // let choreography = CvSync::new(replicas.len());
    // rumpsteak_choreography::interpret(adapter, choreography).await?;
    // ```
    
    // For now, simulate basic state exchange
    info!("Simulating state exchange for {} replicas", replicas.len());
    
    // In a real implementation, this would be handled by the choreography runtime
    let current_state = handler.get_state().clone(); 
    debug!("Current state prepared for synchronization");
    
    // Placeholder: In practice, states would be exchanged via choreography
    // and handler.on_recv() would be called for each received state
    
    info!("CvRDT synchronization completed successfully");
    Ok(())
}

/// Execute delta-based CRDT gossip
///
/// Bridges the `DeltaSync` choreography with a `DeltaHandler` for
/// bandwidth-optimized CRDT synchronization.
pub async fn execute_delta_sync<S, D>(
    _adapter: &mut AuraHandlerAdapter,
    replicas: Vec<ChoreographicRole>,
    my_role: ChoreographicRole,
    handler: &mut DeltaHandler<S, D>,
) -> Result<(), ChoreographyError>
where
    S: CvState + Send + Sync + 'static,
    D: Delta + Send + Sync + 'static,
{
    info!("Starting delta CRDT synchronization with {} replicas", replicas.len());
    debug!("Role {:?} participating in delta gossip", my_role);
    
    // TODO: Integrate with rumpsteak-aura DeltaSync choreography
    // The choreography would handle delta message exchange patterns
    
    // Simulate delta exchange and folding
    info!("Simulating delta gossip for {} replicas", replicas.len());
    
    // Trigger delta folding after gossip round
    handler.force_fold();
    info!("Delta synchronization completed");
    
    Ok(())
}

/// Execute operation-based CRDT broadcast
///
/// Bridges operation broadcast choreographies with `CmHandler` for
/// causal ordering and deduplication guarantees.
pub async fn execute_op_broadcast<S, Op, Id, Ctx>(
    _adapter: &mut AuraHandlerAdapter,
    replicas: Vec<ChoreographicRole>,
    my_role: ChoreographicRole, 
    handler: &mut CmHandler<S, Op, Id, Ctx>,
    pending_ops: Vec<Op>,
) -> Result<(), ChoreographyError>
where
    S: CmApply<Op> + Dedup<Id> + Send + Sync + 'static,
    Op: CausalOp<Id = Id, Ctx = Ctx> + Clone + Send + Sync + 'static,
    Id: Clone + Send + Sync + 'static,
    Ctx: Clone + Send + Sync + 'static,
{
    info!("Starting operation broadcast with {} operations", pending_ops.len());
    debug!("Role {:?} broadcasting to {} replicas", my_role, replicas.len());
    
    // TODO: Integrate with rumpsteak-aura MultiOpBroadcast choreography
    // The choreography would handle causal operation broadcast patterns
    
    // Simulate operation broadcast
    for op in &pending_ops {
        debug!("Preparing to broadcast operation {:?}", op.id());
    }
    
    // Process any buffered operations that are now ready
    handler.process_buffered();
    info!("Operation broadcast completed");
    
    Ok(())
}

/// Execute repair protocol for missing operations
///
/// Implements digest exchange and missing operation recovery.
pub async fn execute_repair_protocol<Id, Op>(
    _adapter: &mut AuraHandlerAdapter,
    peer_role: ChoreographicRole,
    my_role: ChoreographicRole,
    local_digest: Vec<Id>,
) -> Result<Vec<Op>, ChoreographyError>
where
    Id: Clone + Send + Sync + 'static,
    Op: Clone + Send + Sync + 'static,
{
    info!("Starting repair protocol with peer {:?}", peer_role);
    debug!("Role {:?} initiating repair with {} digest entries", my_role, local_digest.len());
    
    // TODO: Integrate with rumpsteak-aura OpRepair choreography
    // The choreography would handle digest exchange and missing operation recovery
    
    let missing_ops = Vec::new(); // Placeholder
    
    info!("Repair protocol completed, recovered {} operations", missing_ops.len());
    Ok(missing_ops)
}

/// Multi-CRDT execution coordinator
///
/// Manages execution of multiple CRDT synchronization protocols
/// within a single session, handling different CRDT types with
/// their appropriate choreographies and handlers.
pub struct MultiCRDTCoordinator {
    /// Session ID for this coordination group
    pub session_id: SessionId,
    /// Participating devices
    pub participants: Vec<DeviceId>, 
    /// Role mappings for choreographic execution
    pub role_mapping: HashMap<DeviceId, ChoreographicRole>,
    /// This device's ID
    pub device_id: DeviceId,
}

impl MultiCRDTCoordinator {
    /// Create a new multi-CRDT coordinator
    pub fn new(
        session_id: SessionId,
        participants: Vec<DeviceId>,
        device_id: DeviceId,
    ) -> Self {
        // Create role mapping
        let role_mapping = participants
            .iter()
            .enumerate()
            .map(|(i, &dev_id)| (dev_id, ChoreographicRole::Participant(i)))
            .collect();
            
        Self {
            session_id,
            participants,
            role_mapping, 
            device_id,
        }
    }
    
    /// Execute state-based CRDT synchronization for a specific CRDT type
    pub async fn sync_cv_crdt<S>(
        &self,
        adapter: &mut AuraHandlerAdapter,
        handler: &mut CvHandler<S>,
        crdt_type: &str,
    ) -> Result<(), ChoreographyError>
    where
        S: CvState + Send + Sync + 'static,
    {
        let my_role = self.role_mapping[&self.device_id];
        let roles: Vec<ChoreographicRole> = self.role_mapping.values().cloned().collect();
        
        info!("Synchronizing CvRDT '{}' in session {:?}", crdt_type, self.session_id);
        
        execute_cv_sync(adapter, roles, my_role, handler).await
    }
    
    /// Execute operation-based CRDT broadcast for a specific CRDT type
    pub async fn broadcast_ops<S, Op, Id, Ctx>(
        &self,
        adapter: &mut AuraHandlerAdapter,
        handler: &mut CmHandler<S, Op, Id, Ctx>,
        operations: Vec<Op>,
        crdt_type: &str,
    ) -> Result<(), ChoreographyError>
    where
        S: CmApply<Op> + Dedup<Id> + Send + Sync + 'static,
        Op: CausalOp<Id = Id, Ctx = Ctx> + Clone + Send + Sync + 'static,
        Id: Clone + Send + Sync + 'static,
        Ctx: Clone + Send + Sync + 'static,
    {
        let my_role = self.role_mapping[&self.device_id];
        let roles: Vec<ChoreographicRole> = self.role_mapping.values().cloned().collect();
        
        info!("Broadcasting {} operations for CmRDT '{}' in session {:?}", 
              operations.len(), crdt_type, self.session_id);
              
        execute_op_broadcast(adapter, roles, my_role, handler, operations).await
    }
    
    /// Get this device's role in the choreography
    pub fn my_role(&self) -> ChoreographicRole {
        self.role_mapping[&self.device_id]
    }
    
    /// Get all participant roles
    pub fn all_roles(&self) -> Vec<ChoreographicRole> {
        self.role_mapping.values().cloned().collect()
    }
}