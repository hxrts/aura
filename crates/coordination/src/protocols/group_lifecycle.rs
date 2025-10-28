//! Group protocol lifecycle with BeeKEM integration.
//!
//! This lifecycle coordinates distributed group operations including group creation,
//! membership management, and encrypted group messaging using Keyhive's BeeKEM protocol.

use crate::protocol_results::GroupProtocolResult;
use aura_crypto::Effects;
use aura_groups::{BeeKemManager, Epoch, GroupRoster, KeyhiveCgkaOperation, MemberId, CgkaOperationType};
use aura_journal::{capability::authority_graph::AuthorityGraph, SessionId as JournalSessionId};
use aura_types::{DeviceId, SessionId};
use protocol_core::{
    capabilities::{ProtocolCapabilities, ProtocolEffects},
    lifecycle::{
        transition_from_witness, ProtocolDescriptor, ProtocolInput, ProtocolLifecycle,
        ProtocolRehydration, ProtocolStep,
    },
    metadata::{OperationType, ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
use uuid::Uuid;

/// Error type surfaced by the group lifecycle.
#[derive(Debug, thiserror::Error)]
pub enum GroupLifecycleError {
    #[error("BeeKEM operation failed: {0}")]
    BeeKemError(String),
    #[error("invalid group operation: {0}")]
    InvalidOperation(String),
    #[error("unexpected input for group lifecycle: {0}")]
    UnsupportedInput(&'static str),
    #[error("missing required parameter: {0}")]
    MissingParameter(&'static str),
}

/// Typestate marker for the group lifecycle.
#[derive(Debug, Clone)]
pub struct GroupLifecycleState;

impl SessionState for GroupLifecycleState {
    const NAME: &'static str = "GroupLifecycle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Group lifecycle implementation with BeeKEM integration.
pub struct GroupLifecycle {
    descriptor: ProtocolDescriptor,
    state: GroupLifecycleState,
    finished: bool,
    group_id: String,
    participants: Vec<DeviceId>,
    beekem_manager: Option<BeeKemManager>,
    output: Option<GroupProtocolResult>,
}

impl std::fmt::Debug for GroupLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GroupLifecycle")
            .field("descriptor", &self.descriptor)
            .field("state", &self.state)
            .field("finished", &self.finished)
            .field("group_id", &self.group_id)
            .field("participants", &self.participants)
            .field("beekem_manager", &"<BeeKemManager>") // Don't debug the manager itself
            .field("output", &self.output)
            .finish()
    }
}

impl Clone for GroupLifecycle {
    fn clone(&self) -> Self {
        Self {
            descriptor: self.descriptor.clone(),
            state: self.state.clone(),
            finished: self.finished,
            group_id: self.group_id.clone(),
            participants: self.participants.clone(),
            beekem_manager: None, // BeeKemManager is not cloneable, reset to None
            output: self.output.clone(),
        }
    }
}

impl GroupLifecycle {
    /// Create a new group lifecycle for group creation/management.
    pub fn new(
        session_id: SessionId,
        device_id: DeviceId,
        group_id: String,
        participants: Vec<DeviceId>,
    ) -> Self {
        let descriptor = ProtocolDescriptor::new(
            Uuid::new_v4(),
            session_id,
            device_id,
            ProtocolType::Group,
        )
        .with_operation_type(OperationType::Group)
        .with_priority(ProtocolPriority::High)
        .with_mode(ProtocolMode::Interactive);

        Self {
            descriptor,
            state: GroupLifecycleState,
            finished: false,
            group_id,
            participants,
            beekem_manager: None,
            output: None,
        }
    }

    /// Convenience constructor for ephemeral sessions.
    #[allow(clippy::disallowed_methods)]
    pub fn new_ephemeral(
        device_id: DeviceId,
        group_id: String,
        participants: Vec<DeviceId>,
    ) -> Self {
        Self::new(SessionId::new(), device_id, group_id, participants)
    }

    /// Initialize BeeKEM manager with effects
    fn initialize_beekem(&mut self, effects: Effects) -> Result<(), GroupLifecycleError> {
        let mut manager = BeeKemManager::new(effects);
        
        // For now, create a minimal authority graph for group initialization
        let authority_graph = AuthorityGraph::new();
        
        manager.initialize_group(self.group_id.clone(), &authority_graph)
            .map_err(|e| GroupLifecycleError::BeeKemError(e.to_string()))?;
        
        self.beekem_manager = Some(manager);
        Ok(())
    }
    
    /// Perform group operation based on input
    fn perform_group_operation(&mut self, signal: &str, data: Option<&serde_json::Value>) -> Result<(), GroupLifecycleError> {
        let manager = self.beekem_manager.as_mut()
            .ok_or_else(|| GroupLifecycleError::InvalidOperation("BeeKEM manager not initialized".to_string()))?;
        
        match signal {
            "create_group" => {
                // Group already initialized in initialize_beekem
                let roster = manager.get_roster(&self.group_id)
                    .unwrap_or_else(|| GroupRoster { members: vec![] });
                let epoch = manager.get_epoch(&self.group_id)
                    .unwrap_or_else(|| Epoch(0));
                
                self.output = Some(GroupProtocolResult {
                    session_id: JournalSessionId::from_uuid(self.descriptor.session_id.uuid()),
                    group_id: self.group_id.clone(),
                    epoch,
                    roster,
                    cgka_operations: vec![],
                    ledger_events: vec![],
                    participants: self.participants.clone(),
                });
            }
            "add_members" => {
                let new_members = data
                    .and_then(|d| d.get("members"))
                    .and_then(|m| m.as_array())
                    .ok_or_else(|| GroupLifecycleError::MissingParameter("members"))?
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| MemberId::new(s.to_string()))
                    .collect::<Vec<_>>();
                
                let operation = KeyhiveCgkaOperation {
                    operation_id: Uuid::new_v4(),
                    group_id: self.group_id.clone(),
                    operation_type: CgkaOperationType::Add { members: new_members },
                    payload: vec![],
                    signature: vec![],
                };
                
                let roster = manager.get_roster(&self.group_id)
                    .unwrap_or_else(|| GroupRoster { members: vec![] });
                let epoch = manager.get_epoch(&self.group_id)
                    .unwrap_or_else(|| Epoch(0));
                
                self.output = Some(GroupProtocolResult {
                    session_id: JournalSessionId::from_uuid(self.descriptor.session_id.uuid()),
                    group_id: self.group_id.clone(),
                    epoch,
                    roster,
                    cgka_operations: vec![operation],
                    ledger_events: vec![],
                    participants: self.participants.clone(),
                });
            }
            _ => {
                return Err(GroupLifecycleError::UnsupportedInput("unsupported group operation"));
            }
        }
        
        Ok(())
    }
}

impl ProtocolLifecycle for GroupLifecycle {
    type State = GroupLifecycleState;
    type Output = GroupProtocolResult;
    type Error = GroupLifecycleError;

    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn step(
        &mut self,
        input: ProtocolInput<'_>,
        _caps: &mut ProtocolCapabilities<'_>,
    ) -> ProtocolStep<Self::Output, Self::Error> {
        match input {
            ProtocolInput::LocalSignal { signal, data } => {
                // Initialize BeeKEM manager if not already done
                if self.beekem_manager.is_none() {
                    // Get effects from capabilities
                    let effects = aura_crypto::Effects::production();
                    if let Err(e) = self.initialize_beekem(effects) {
                        self.finished = true;
                        return ProtocolStep::completed(
                            vec![ProtocolEffects::Trace {
                                message: format!("Failed to initialize BeeKEM: {}", e),
                                protocol: ProtocolType::Group,
                            }],
                            Some(transition_from_witness(
                                &self.descriptor,
                                GroupLifecycleState::NAME,
                                "GroupLifecycleInitializationFailed",
                                None,
                            )),
                            Err(e),
                        );
                    }
                }

                // Perform the group operation
                match self.perform_group_operation(signal, data) {
                    Ok(()) => {
                        self.finished = true;
                        if let Some(result) = self.output.clone() {
                            ProtocolStep::completed(
                                vec![ProtocolEffects::Trace {
                                    message: format!("Group operation '{}' completed successfully", signal),
                                    protocol: ProtocolType::Group,
                                }],
                                Some(transition_from_witness(
                                    &self.descriptor,
                                    GroupLifecycleState::NAME,
                                    "GroupLifecycleCompleted",
                                    None,
                                )),
                                Ok(result),
                            )
                        } else {
                            ProtocolStep::completed(
                                vec![ProtocolEffects::Trace {
                                    message: "Group operation completed but no result generated".to_string(),
                                    protocol: ProtocolType::Group,
                                }],
                                Some(transition_from_witness(
                                    &self.descriptor,
                                    GroupLifecycleState::NAME,
                                    "GroupLifecycleNoResult",
                                    None,
                                )),
                                Err(GroupLifecycleError::InvalidOperation("No result generated".to_string())),
                            )
                        }
                    }
                    Err(e) => {
                        self.finished = true;
                        ProtocolStep::completed(
                            vec![ProtocolEffects::Trace {
                                message: format!("Group operation '{}' failed: {}", signal, e),
                                protocol: ProtocolType::Group,
                            }],
                            Some(transition_from_witness(
                                &self.descriptor,
                                GroupLifecycleState::NAME,
                                "GroupLifecycleFailed",
                                None,
                            )),
                            Err(e),
                        )
                    }
                }
            }
            _ => {
                self.finished = true;
                ProtocolStep::completed(
                    vec![ProtocolEffects::Trace {
                        message: "Unsupported input type for group lifecycle".to_string(),
                        protocol: ProtocolType::Group,
                    }],
                    Some(transition_from_witness(
                        &self.descriptor,
                        GroupLifecycleState::NAME,
                        "GroupLifecycleUnsupportedInput",
                        None,
                    )),
                    Err(GroupLifecycleError::UnsupportedInput("input type not supported")),
                )
            }
        }
    }

    fn is_final(&self) -> bool {
        self.finished
    }
}

impl ProtocolRehydration for GroupLifecycle {
    type Evidence = (String, Vec<DeviceId>); // (group_id, participants)

    fn validate_evidence(evidence: &Self::Evidence) -> bool {
        !evidence.0.is_empty() && !evidence.1.is_empty()
    }

    fn rehydrate(
        device_id: DeviceId,
        _account_id: aura_types::AccountId,
        evidence: Self::Evidence,
    ) -> Result<Self, Self::Error> {
        let (group_id, participants) = evidence;
        Ok(Self::new_ephemeral(device_id, group_id, participants))
    }
}
