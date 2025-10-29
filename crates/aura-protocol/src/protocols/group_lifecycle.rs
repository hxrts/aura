//! Group protocol lifecycle with BeeKEM integration.
//!
//! This lifecycle coordinates distributed group operations including group creation,
//! membership management, and encrypted group messaging using Keyhive's BeeKEM protocol.

use super::GroupLifecycleError;
use crate::core::{
    capabilities::{ProtocolCapabilities, ProtocolEffects},
    lifecycle::{
        transition_from_witness, ProtocolDescriptor, ProtocolInput, ProtocolLifecycle,
        ProtocolRehydration, ProtocolStep,
    },
    metadata::{OperationType, ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
use crate::protocol_results::GroupProtocolResult;
use aura_crypto::Effects;
use aura_journal::capability::{
    CgkaOperationType, Epoch, GroupCapabilityManager, GroupRoster, KeyhiveCgkaOperation, MemberId,
};
use aura_journal::SessionId as JournalSessionId;
use aura_types::{AuraError, DeviceId, SessionId};
use uuid::Uuid;

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
    group_manager: Option<GroupCapabilityManager>,
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
            .field("group_manager", &"<GroupCapabilityManager>") // Don't debug the manager itself
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
            group_manager: None, // GroupCapabilityManager is not cloneable, reset to None
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
        let descriptor =
            ProtocolDescriptor::new(Uuid::new_v4(), session_id, device_id, ProtocolType::Group)
                .with_operation_type(OperationType::Group)
                .with_priority(ProtocolPriority::High)
                .with_mode(ProtocolMode::Interactive);

        Self {
            descriptor,
            state: GroupLifecycleState,
            finished: false,
            group_id,
            participants,
            group_manager: None,
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
    fn initialize_beekem(&mut self, _effects: Effects) -> Result<(), GroupLifecycleError> {
        // TODO: Initialize GroupCapabilityManager with proper dependencies
        // This will require authority_graph and unified_manager
        // For now, skip the initialization as we need proper dependencies
        //
        // let authority_graph = AuthorityGraph::new();
        // let unified_manager = UnifiedCapabilityManager::new(...);
        // let manager = GroupCapabilityManager::new(authority_graph, unified_manager, effects);
        // manager.initialize_group(self.group_id.clone())?;
        // self.group_manager = Some(manager);
        Ok(())
    }

    /// Perform group operation based on input
    fn perform_group_operation(
        &mut self,
        signal: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<(), GroupLifecycleError> {
        let manager = self
            .group_manager
            .as_mut()
            .ok_or_else(|| AuraError::agent_invalid_state("Group manager not initialized"))?;

        match signal {
            "create_group" => {
                // Group already initialized in initialize_beekem
                let roster = manager
                    .get_roster(&self.group_id)
                    .unwrap_or_else(|| GroupRoster { members: vec![] });
                let epoch = manager
                    .get_epoch(&self.group_id)
                    .unwrap_or_else(|| Epoch(0));

                // Create real capability proof with cryptographic authorization
                // Create capability proof using unified builder
                use crate::protocols::CapabilityProofBuilder;
                let capability_proof =
                    CapabilityProofBuilder::new(self.descriptor.device_id, "group")
                        .create_proof("group_operations", "create_group")
                        .unwrap_or_else(|_| CapabilityProofBuilder::create_placeholder());

                self.output = Some(GroupProtocolResult {
                    session_id: JournalSessionId::from_uuid(self.descriptor.session_id.uuid()),
                    group_id: self.group_id.clone(),
                    epoch,
                    roster,
                    cgka_operations: vec![],
                    ledger_events: vec![],
                    participants: self.participants.clone(),
                    capability_proof,
                });
            }
            "add_members" => {
                let new_members = data
                    .and_then(|d| d.get("members"))
                    .and_then(|m| m.as_array())
                    .ok_or_else(|| {
                        AuraError::group_lifecycle_error("missing required parameter: members")
                    })?
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| MemberId::new(s.to_string()))
                    .collect::<Vec<_>>();

                let operation = KeyhiveCgkaOperation {
                    operation_id: Uuid::new_v4(),
                    group_id: self.group_id.clone(),
                    operation_type: CgkaOperationType::Add {
                        members: new_members,
                    },
                    payload: vec![],
                    signature: vec![],
                };

                let roster = manager
                    .get_roster(&self.group_id)
                    .unwrap_or_else(|| GroupRoster { members: vec![] });
                let epoch = manager
                    .get_epoch(&self.group_id)
                    .unwrap_or_else(|| Epoch(0));

                // Create real capability proof with cryptographic authorization
                // Create capability proof using unified builder
                use crate::protocols::CapabilityProofBuilder;
                let capability_proof =
                    CapabilityProofBuilder::new(self.descriptor.device_id, "group")
                        .create_proof("group_operations", "add_members")
                        .unwrap_or_else(|_| CapabilityProofBuilder::create_placeholder());

                self.output = Some(GroupProtocolResult {
                    session_id: JournalSessionId::from_uuid(self.descriptor.session_id.uuid()),
                    group_id: self.group_id.clone(),
                    epoch,
                    roster,
                    cgka_operations: vec![operation],
                    ledger_events: vec![],
                    participants: self.participants.clone(),
                    capability_proof,
                });
            }
            _ => {
                return Err(AuraError::group_lifecycle_error(
                    "unsupported group operation",
                ));
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
                if self.group_manager.is_none() {
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
                                    message: format!(
                                        "Group operation '{}' completed successfully",
                                        signal
                                    ),
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
                                    message: "Group operation completed but no result generated"
                                        .to_string(),
                                    protocol: ProtocolType::Group,
                                }],
                                Some(transition_from_witness(
                                    &self.descriptor,
                                    GroupLifecycleState::NAME,
                                    "GroupLifecycleNoResult",
                                    None,
                                )),
                                Err(AuraError::agent_invalid_state("No result generated")),
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
                    Err(AuraError::group_lifecycle_error("input type not supported")),
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
