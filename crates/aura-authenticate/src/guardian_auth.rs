//! Guardian Authentication Choreography
//!
//! This module implements choreographic protocols for guardian-based
//! authentication during recovery operations.

use crate::{AuraError, AuraResult};
use aura_core::{AccountId, Cap, DeviceId};
use aura_verify::{IdentityProof, KeyMaterial, VerifiedIdentity};
// Guardian types from aura_wot not yet implemented, using placeholders
use aura_mpst::{AuraRuntime, CapabilityGuard, JournalAnnotation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::Mutex;

/// Guardian authentication request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianAuthRequest {
    /// Device requesting guardian authentication
    pub requesting_device: DeviceId,
    /// Account being recovered
    pub account_id: AccountId,
    /// Recovery context information
    pub recovery_context: RecoveryContext,
    /// Required guardian threshold
    pub required_guardians: usize,
}

/// Recovery context for guardian authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryContext {
    /// Recovery operation type
    pub operation_type: RecoveryOperationType,
    /// Recovery justification
    pub justification: String,
    /// Emergency status
    pub is_emergency: bool,
    /// Recovery timestamp
    pub timestamp: u64,
}

/// Types of recovery operations requiring guardian approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryOperationType {
    /// Device key recovery
    DeviceKeyRecovery,
    /// Account access recovery
    AccountAccessRecovery,
    /// Guardian set modification
    GuardianSetModification,
    /// Emergency account freeze
    EmergencyFreeze,
    /// Account unfreezing
    AccountUnfreeze,
}

/// Guardian authentication response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianAuthResponse {
    /// Guardian approvals received
    pub guardian_approvals: Vec<GuardianApproval>,
    /// Success indicator
    pub success: bool,
    /// Error message if any
    pub error: Option<String>,
}

/// Individual guardian approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianApproval {
    /// Guardian device ID
    pub guardian_id: DeviceId,
    /// Verified guardian identity
    pub verified_identity: VerifiedIdentity,
    /// Approval decision
    pub approved: bool,
    /// Approval justification
    pub justification: String,
    /// Guardian signature
    pub signature: Vec<u8>,
    /// Timestamp of approval
    pub timestamp: u64,
}

/// Message types for guardian authentication choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GuardianAuthMessage {
    /// Request guardian approval
    ApprovalRequest {
        /// Guardian being requested
        guardian_id: DeviceId,
        /// Account context
        account_id: AccountId,
        /// Recovery context
        recovery_context: RecoveryContext,
        /// Request ID for tracking
        request_id: String,
    },

    /// Guardian challenge for verification
    GuardianChallenge {
        /// Request ID
        request_id: String,
        /// Challenge nonce
        challenge: Vec<u8>,
        /// Challenge expiry
        expires_at: u64,
    },

    /// Guardian proof submission
    GuardianProofSubmission {
        /// Request ID
        request_id: String,
        /// Guardian identity proof
        identity_proof: IdentityProof,
        /// Guardian key material
        key_material: KeyMaterial,
    },

    /// Guardian approval decision
    ApprovalDecision {
        /// Request ID
        request_id: String,
        /// Guardian making decision
        guardian_id: DeviceId,
        /// Decision (approve/deny)
        approved: bool,
        /// Justification
        justification: String,
        /// Guardian signature
        signature: Vec<u8>,
    },

    /// Final approval result
    ApprovalResult {
        /// Request ID
        request_id: String,
        /// All guardian approvals
        approvals: Vec<GuardianApproval>,
        /// Success status
        success: bool,
        /// Error if failed
        error: Option<String>,
    },
}

/// Roles in guardian authentication choreography
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuardianRole {
    /// Device requesting guardian approval
    Requester,
    /// Guardian providing approval
    Guardian(u32),
    /// Coordinator managing approval process
    Coordinator,
}

impl GuardianRole {
    /// Get the name of this role
    pub fn name(&self) -> String {
        match self {
            GuardianRole::Requester => "Requester".to_string(),
            GuardianRole::Guardian(id) => format!("Guardian_{}", id),
            GuardianRole::Coordinator => "Coordinator".to_string(),
        }
    }
}

/// Guardian authentication choreography state
#[derive(Debug)]
pub struct GuardianAuthState {
    /// Current request being processed
    current_request: Option<GuardianAuthRequest>,
    /// Guardian challenges by request ID
    guardian_challenges: HashMap<String, HashMap<DeviceId, (Vec<u8>, u64)>>,
    /// Guardian approvals by request ID
    guardian_approvals: HashMap<String, Vec<GuardianApproval>>,
    /// Guardian verification status
    #[allow(dead_code)] // Used for audit and recovery tracking
    verified_guardians: HashMap<String, HashMap<DeviceId, VerifiedIdentity>>,
}

impl GuardianAuthState {
    /// Create new state
    pub fn new() -> Self {
        Self {
            current_request: None,
            guardian_challenges: HashMap::new(),
            guardian_approvals: HashMap::new(),
            verified_guardians: HashMap::new(),
        }
    }

    /// Add guardian challenge
    pub fn add_guardian_challenge(
        &mut self,
        request_id: String,
        guardian_id: DeviceId,
        challenge: Vec<u8>,
        expires_at: u64,
    ) {
        self.guardian_challenges
            .entry(request_id)
            .or_insert_with(HashMap::new)
            .insert(guardian_id, (challenge, expires_at));
    }

    /// Verify guardian challenge
    #[allow(clippy::disallowed_methods)]
    pub fn verify_guardian_challenge(
        &self,
        request_id: &str,
        guardian_id: DeviceId,
    ) -> Option<&Vec<u8>> {
        self.guardian_challenges
            .get(request_id)
            .and_then(|challenges| challenges.get(&guardian_id))
            .and_then(|(challenge, expires_at)| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                if now > *expires_at {
                    None // Expired
                } else {
                    Some(challenge)
                }
            })
    }

    /// Add guardian approval
    pub fn add_guardian_approval(&mut self, request_id: String, approval: GuardianApproval) {
        self.guardian_approvals
            .entry(request_id)
            .or_insert_with(Vec::new)
            .push(approval);
    }

    /// Check if sufficient guardians have approved
    pub fn has_sufficient_approvals(&self, request_id: &str, required: usize) -> bool {
        self.guardian_approvals
            .get(request_id)
            .map(|approvals| {
                approvals
                    .iter()
                    .filter(|approval| approval.approved)
                    .count()
                    >= required
            })
            .unwrap_or(false)
    }

    /// Get all approvals for a request
    pub fn get_approvals(&self, request_id: &str) -> Vec<GuardianApproval> {
        self.guardian_approvals
            .get(request_id)
            .cloned()
            .unwrap_or_default()
    }
}

/// Guardian authentication choreography
#[derive(Debug)]
pub struct GuardianAuthChoreography {
    /// Local device role
    role: GuardianRole,
    /// Choreography state
    state: Mutex<GuardianAuthState>,
    /// MPST runtime
    runtime: AuraRuntime,
}

impl GuardianAuthChoreography {
    /// Create new guardian authentication choreography
    pub fn new(role: GuardianRole, runtime: AuraRuntime) -> Self {
        Self {
            role,
            state: Mutex::new(GuardianAuthState::new()),
            runtime,
        }
    }

    /// Execute the choreography
    pub async fn execute(
        &self,
        request: GuardianAuthRequest,
        effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<GuardianAuthResponse> {
        let mut state = self.state.lock().await;
        state.current_request = Some(request.clone());
        drop(state);

        match self.role {
            GuardianRole::Requester => self.execute_requester(request, effect_system).await,
            GuardianRole::Guardian(_) => self.execute_guardian(effect_system).await,
            GuardianRole::Coordinator => self.execute_coordinator(effect_system).await,
        }
    }

    /// Execute as approval requester
    #[allow(clippy::disallowed_methods)]
    async fn execute_requester(
        &self,
        request: GuardianAuthRequest,
        effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<GuardianAuthResponse> {
        tracing::info!(
            "Executing guardian auth as requester for account: {}",
            request.account_id
        );

        // Apply capability guard
        let recovery_cap = Cap::with_permissions(vec![
            "recovery:request".to_string(),
            "network:send".to_string(),
            "network:receive".to_string(),
        ]);
        let guard = CapabilityGuard::new(recovery_cap.clone());

        // For MVP, grant recovery permissions to authenticated devices
        let device_capabilities = recovery_cap; // Placeholder
        guard.enforce(&device_capabilities).map_err(|e| {
            AuraError::invalid(format!(
                "Insufficient capabilities for guardian auth: {}",
                e
            ))
        })?;

        // Create handler adapter for communication
        let device_id = self.runtime.device_id();
        let mut adapter = aura_protocol::choreography::AuraHandlerAdapter::new(
            device_id,
            effect_system.execution_mode(),
        );

        // Generate request ID
        let request_id = uuid::Uuid::new_v4().to_string();

        // Discover guardians from web of trust
        // For MVP, we would need to query the ledger/journal for guardian devices
        // For now, we'll use a placeholder guardian list
        tracing::info!(
            "Requesting guardian approvals: {} guardians required",
            request.required_guardians
        );

        // In production, guardians would be discovered from:
        // 1. Account's guardian list in journal
        // 2. Web of trust relationships
        // 3. Recovery configuration
        let guardian_devices: Vec<DeviceId> = Vec::new(); // Placeholder - needs WoT integration

        if guardian_devices.is_empty() {
            tracing::warn!("No guardians discovered for account - needs WoT integration");
        }

        // Send approval requests to discovered guardians
        for guardian_id in &guardian_devices {
            let approval_request = GuardianAuthMessage::ApprovalRequest {
                guardian_id: *guardian_id,
                account_id: request.account_id,
                recovery_context: request.recovery_context.clone(),
                request_id: request_id.clone(),
            };

            if let Err(e) = adapter.send(*guardian_id, approval_request).await {
                tracing::warn!(
                    "Failed to send approval request to guardian {}: {}",
                    guardian_id,
                    e
                );
            }
        }

        // Wait for guardian approvals with timeout
        let mut collected_approvals: Vec<GuardianApproval> = Vec::new();
        let approval_timeout = tokio::time::Duration::from_secs(300); // 5 minutes

        tokio::time::timeout(approval_timeout, async {
            while collected_approvals.len() < request.required_guardians
                && collected_approvals.len() < guardian_devices.len()
            {
                // Try to receive from each guardian
                let mut received_any = false;
                for guardian_id in &guardian_devices {
                    // Skip if we already got approval from this guardian
                    if collected_approvals
                        .iter()
                        .any(|a| a.guardian_id == *guardian_id)
                    {
                        continue;
                    }

                    if let Ok(message) =
                        adapter.recv_from::<GuardianAuthMessage>(*guardian_id).await
                    {
                        received_any = true;
                        match message {
                            GuardianAuthMessage::ApprovalDecision {
                                request_id: resp_id,
                                guardian_id: resp_guardian,
                                approved,
                                justification,
                                signature,
                            } if resp_id == request_id => {
                                // Create approval record
                                let approval = GuardianApproval {
                                    guardian_id: resp_guardian,
                                    verified_identity: VerifiedIdentity {
                                        proof: aura_verify::IdentityProof::Device {
                                            device_id: resp_guardian,
                                            signature: aura_verify::Ed25519Signature::from_slice(
                                                &signature,
                                            )
                                            .unwrap_or_else(|_| {
                                                aura_verify::Ed25519Signature::from_slice(
                                                    &[0u8; 64],
                                                )
                                                .unwrap()
                                            }),
                                        },
                                        message_hash: [0u8; 32], // Placeholder
                                    },
                                    approved,
                                    justification,
                                    signature,
                                    timestamp: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap()
                                        .as_secs(),
                                };

                                collected_approvals.push(approval);
                                tracing::info!(
                                    "Received approval from guardian {}: {}",
                                    resp_guardian,
                                    approved
                                );
                            }
                            _ => {
                                // Ignore other message types
                                continue;
                            }
                        }
                    }
                }

                // Avoid busy-looping if no messages received
                if !received_any {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        })
        .await
        .ok(); // Don't fail on timeout, return what we collected

        // Check if we have sufficient approvals
        let approved_count = collected_approvals.iter().filter(|a| a.approved).count();
        let success = approved_count >= request.required_guardians;

        // Apply journal annotation
        let journal_annotation = JournalAnnotation::add_facts(format!(
            "Guardian authentication: {}/{} approvals",
            approved_count, request.required_guardians
        ));
        tracing::info!("Applied journal annotation: {:?}", journal_annotation);

        tracing::info!(
            "Guardian authentication complete: {} approvals collected, {} required, success: {}",
            approved_count,
            request.required_guardians,
            success
        );

        Ok(GuardianAuthResponse {
            guardian_approvals: collected_approvals,
            success,
            error: if success {
                None
            } else if guardian_devices.is_empty() {
                Some("No guardians discovered - needs WoT integration".to_string())
            } else {
                Some(format!(
                    "Insufficient approvals: {}/{} received",
                    approved_count, request.required_guardians
                ))
            },
        })
    }

    /// Execute as guardian
    async fn execute_guardian(
        &self,
        effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<GuardianAuthResponse> {
        tracing::info!("Executing guardian auth as guardian");

        let _device_id = self.runtime.device_id();

        // Wait for approval request from requester
        // In production, we would know the requester from context or receive from any device
        tracing::info!("Guardian waiting for approval request...");

        // Placeholder: In production, guardian would listen for requests using AuraHandlerAdapter
        // For now, we return an error indicating the guardian role is passive
        tracing::warn!(
            "Guardian role requires incoming request - this is typically driven by requester"
        );

        Ok(GuardianAuthResponse {
            guardian_approvals: Vec::new(),
            success: false,
            error: Some(
                "Guardian role is passive - awaits approval requests from requester".to_string(),
            ),
        })
    }

    /// Execute as coordinator
    async fn execute_coordinator(
        &self,
        _effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<GuardianAuthResponse> {
        tracing::info!("Executing guardian auth as coordinator");

        // Coordinate approval process across guardians
        // The coordinator role is primarily handled by the requester in this choreography
        // A separate coordinator would be needed for multi-requester scenarios where
        // multiple devices are attempting recovery simultaneously
        //
        // For now, the coordinator role is a placeholder since the requester handles
        // coordination directly by collecting approvals from guardians

        tracing::warn!("Coordinator role not fully implemented - requester handles coordination");

        Ok(GuardianAuthResponse {
            guardian_approvals: Vec::new(),
            success: false,
            error: Some(
                "Coordinator role requires multi-requester coordination scenario".to_string(),
            ),
        })
    }
}

/// Guardian authentication coordinator
#[derive(Debug)]
pub struct GuardianAuthCoordinator {
    /// Local runtime
    runtime: AuraRuntime,
    /// Current choreography
    choreography: Option<GuardianAuthChoreography>,
}

impl GuardianAuthCoordinator {
    /// Create new coordinator
    pub fn new(runtime: AuraRuntime) -> Self {
        Self {
            runtime,
            choreography: None,
        }
    }

    /// Execute guardian authentication using choreography
    pub async fn authenticate_guardians(
        &mut self,
        request: GuardianAuthRequest,
        effect_system: &aura_protocol::effects::system::AuraEffectSystem,
    ) -> AuraResult<GuardianAuthResponse> {
        tracing::info!(
            "Starting guardian authentication for account: {}",
            request.account_id
        );

        // Validate guardian requirements
        if request.required_guardians == 0 {
            return Err(AuraError::invalid(
                "Required guardians must be greater than 0",
            ));
        }

        // Create choreography with requester role
        let choreography =
            GuardianAuthChoreography::new(GuardianRole::Requester, self.runtime.clone());

        // Execute the choreography with effect system
        let result = choreography.execute(request, effect_system).await;

        // Store choreography for potential follow-up operations
        self.choreography = Some(choreography);

        result
    }

    /// Get the current runtime
    pub fn runtime(&self) -> &AuraRuntime {
        &self.runtime
    }

    /// Check if a choreography is currently active
    pub fn has_active_choreography(&self) -> bool {
        self.choreography.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AccountId, Cap, DeviceId, Journal};

    #[tokio::test]
    async fn test_guardian_auth_state() {
        let mut state = GuardianAuthState::new();

        let request_id = "test_request".to_string();
        let guardian_id = DeviceId(uuid::Uuid::new_v4());
        let challenge = vec![1, 2, 3, 4];
        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 300; // 5 minutes from now

        state.add_guardian_challenge(
            request_id.clone(),
            guardian_id,
            challenge.clone(),
            expires_at,
        );

        let verified_challenge = state.verify_guardian_challenge(&request_id, guardian_id);
        assert_eq!(verified_challenge, Some(&challenge));

        assert!(!state.has_sufficient_approvals(&request_id, 1));

        let approval = GuardianApproval {
            guardian_id,
            verified_identity: VerifiedIdentity {
                proof: aura_verify::IdentityProof::Device {
                    device_id: guardian_id,
                    signature: aura_verify::Ed25519Signature::from_slice(&[0u8; 64]).unwrap(),
                },
                message_hash: [0u8; 32],
            },
            approved: true,
            justification: "Test approval".to_string(),
            signature: vec![5, 6, 7, 8],
            timestamp: expires_at,
        };

        state.add_guardian_approval(request_id.clone(), approval);
        assert!(state.has_sufficient_approvals(&request_id, 1));
    }

    #[tokio::test]
    async fn test_guardian_auth_coordinator() {
        let device_id = DeviceId(uuid::Uuid::new_v4());
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());

        let mut coordinator = GuardianAuthCoordinator::new(runtime);
        assert!(!coordinator.has_active_choreography());

        let request = GuardianAuthRequest {
            requesting_device: device_id,
            account_id: AccountId(uuid::Uuid::new_v4()),
            recovery_context: RecoveryContext {
                operation_type: RecoveryOperationType::DeviceKeyRecovery,
                justification: "Lost device".to_string(),
                is_emergency: false,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            },
            required_guardians: 2,
        };

        // Create effect system for test
        let effect_system = aura_protocol::effects::system::AuraEffectSystem::new(
            device_id,
            aura_protocol::context::ExecutionMode::Testing,
        );

        // Note: This will return Ok with success=false since no guardians are discovered
        let result = coordinator
            .authenticate_guardians(request, &effect_system)
            .await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.success);
        assert!(coordinator.has_active_choreography());
    }
}
