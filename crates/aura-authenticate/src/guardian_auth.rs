//! Guardian Authentication Choreography
//!
//! This module implements choreographic protocols for guardian-based
//! authentication during recovery operations.

use crate::{AuraError, AuraResult};
use aura_core::{AccountId, DeviceId};
use aura_verify::{IdentityProof, KeyMaterial, VerifiedIdentity};
// Guardian types from aura_wot not yet implemented, using placeholders
use aura_protocol::AuraEffectSystem;
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Guardian approval decision (internal)
#[derive(Debug, Clone)]
struct GuardianApprovalDecision {
    /// Whether to approve the request
    approved: bool,
    /// Justification for the decision
    justification: String,
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
#[allow(dead_code)]
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

impl Default for GuardianAuthState {
    fn default() -> Self {
        Self::new()
    }
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
            .or_default()
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
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

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
            .or_default()
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
pub struct GuardianAuthChoreography {
    /// Local device role
    role: GuardianRole,
    /// Choreography state
    state: Mutex<GuardianAuthState>,
    /// Effect system
    effect_system: AuraEffectSystem,
}

impl GuardianAuthChoreography {
    /// Create new guardian authentication choreography
    pub fn new(role: GuardianRole, effect_system: AuraEffectSystem) -> Self {
        Self {
            role,
            state: Mutex::new(GuardianAuthState::new()),
            effect_system,
        }
    }

    /// Validate recovery request from guardian perspective
    async fn validate_recovery_request(
        &self,
        account_id: &AccountId,
        recovery_context: &RecoveryContext,
        request_id: &str,
    ) -> AuraResult<GuardianApprovalDecision> {
        tracing::info!(
            "Guardian validating recovery request {} for account {}",
            request_id,
            account_id
        );

        // Basic validation checks
        let mut approval_reasons = Vec::new();
        let mut denial_reasons = Vec::new();

        // Check if this is an emergency request
        if recovery_context.is_emergency {
            approval_reasons.push("Emergency recovery request approved".to_string());
        }

        // Validate recovery operation type
        match recovery_context.operation_type {
            RecoveryOperationType::DeviceKeyRecovery => {
                approval_reasons.push("Device key recovery is permitted".to_string());
            }
            RecoveryOperationType::AccountAccessRecovery => {
                approval_reasons.push("Account access recovery is permitted".to_string());
            }
            RecoveryOperationType::EmergencyFreeze => {
                if recovery_context.is_emergency {
                    approval_reasons.push("Emergency freeze approved".to_string());
                } else {
                    denial_reasons
                        .push("Non-emergency freeze requires additional justification".to_string());
                }
            }
            RecoveryOperationType::GuardianSetModification => {
                // Guardian set modifications require careful consideration
                denial_reasons.push("Guardian set modification requires manual review".to_string());
            }
            RecoveryOperationType::AccountUnfreeze => {
                approval_reasons.push("Account unfreeze approved".to_string());
            }
        }

        // Check request timestamp age (reject old requests)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if now > recovery_context.timestamp + 3600 {
            // 1 hour old
            denial_reasons.push("Recovery request is too old".to_string());
        }

        // For MVP: Default approval policy for most operations unless explicitly denied
        let approved = denial_reasons.is_empty() && !approval_reasons.is_empty();

        let justification = if approved {
            format!("Approved: {}", approval_reasons.join(", "))
        } else if denial_reasons.is_empty() {
            "Insufficient grounds for approval".to_string()
        } else {
            format!("Denied: {}", denial_reasons.join(", "))
        };

        tracing::info!(
            "Guardian decision for request {}: approved={}, reason={}",
            request_id,
            approved,
            justification
        );

        Ok(GuardianApprovalDecision {
            approved,
            justification,
        })
    }

    /// Generate guardian challenge for additional verification
    async fn generate_guardian_challenge(&self, request_id: &str) -> AuraResult<Vec<u8>> {
        // Generate cryptographically secure random challenge
        // In production, this would use proper cryptographic RNG
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let challenge = format!("guardian_challenge_{}_{}", request_id, nanos);

        Ok(challenge.into_bytes())
    }

    /// Sign guardian approval decision
    async fn sign_approval_decision(
        &self,
        request_id: &str,
        account_id: &AccountId,
        approved: bool,
        justification: &str,
    ) -> AuraResult<Vec<u8>> {
        // Create message to sign
        let message = format!(
            "guardian_approval:{}:{}:{}:{}",
            request_id, account_id, approved, justification
        );

        // In production, this would use the guardian's actual private key
        // For MVP, we create a mock signature
        let device_id = self.effect_system.device_id();
        let mock_signature = format!("guardian_sig_{}_{}", device_id, message.len());

        Ok(mock_signature.into_bytes())
    }

    /// Execute the choreography
    pub async fn execute(
        &self,
        request: GuardianAuthRequest,
    ) -> AuraResult<GuardianAuthResponse> {
        let mut state = self.state.lock().await;
        state.current_request = Some(request.clone());
        drop(state);

        match self.role {
            GuardianRole::Requester => self.execute_requester(request).await,
            GuardianRole::Guardian(_) => self.execute_guardian().await,
            GuardianRole::Coordinator => self.execute_coordinator().await,
        }
    }

    /// Execute as approval requester
    #[allow(clippy::disallowed_methods)]
    async fn execute_requester(
        &self,
        request: GuardianAuthRequest,
    ) -> AuraResult<GuardianAuthResponse> {
        tracing::info!(
            "Executing guardian auth as requester for account: {}",
            request.account_id
        );

        // TODO: Implement capability-based authorization with new effect system
        // This will be implemented with aura-wot capability evaluation

        // TODO: Implement network communication with new effect system
        let _device_id = self.effect_system.device_id();

        // Generate request ID
        let request_id = uuid::Uuid::from_bytes([0u8; 16]).to_string();

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
            let _approval_request = GuardianAuthMessage::ApprovalRequest {
                guardian_id: *guardian_id,
                account_id: request.account_id,
                recovery_context: request.recovery_context.clone(),
                request_id: request_id.clone(),
            };

            // TODO: Send request via NetworkEffects
            tracing::info!(
                "Would send approval request to guardian {} (placeholder)",
                guardian_id
            );
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

                    // TODO: Receive response via NetworkEffects
                    if false { // Placeholder - network communication not implemented
                        let message: GuardianAuthMessage = GuardianAuthMessage::ApprovalDecision {
                            request_id: request_id.clone(),
                            guardian_id: *guardian_id,
                            approved: true,
                            justification: "Placeholder approval".to_string(),
                            signature: vec![0; 64],
                        };
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
                                        .map(|d| d.as_secs())
                                        .unwrap_or(0),
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

        // TODO: Implement journal state tracking with new effect system
        // This will use AuraEffectSystem's journal capabilities

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
    ) -> AuraResult<GuardianAuthResponse> {
        tracing::info!("Executing guardian auth as guardian");

        let device_id = self.effect_system.device_id();

        // TODO: Implement capability-based authorization with new effect system
        // This will be implemented with aura-wot capability evaluation

        // TODO: Implement network communication with new effect system

        tracing::info!("Guardian listening for approval requests...");

        // Wait for approval request with timeout
        let listen_timeout = tokio::time::Duration::from_secs(120); // 2 minutes

        let approval_response = tokio::time::timeout(listen_timeout, async {
            // Listen for approval requests from any requester
            loop {
                // TODO: Implement network message receiving with new effect system
                let message_received: Option<GuardianAuthMessage> = None; // Placeholder

                if let Some(message) = message_received {
                    match message {
                        GuardianAuthMessage::ApprovalRequest {
                            guardian_id: _requested_guardian,
                            account_id,
                            recovery_context,
                            request_id,
                        } => {
                            tracing::info!(
                                "Guardian {} received approval request for account {} (request: {})",
                                device_id, account_id, request_id
                            );

                            // Verify this guardian is being requested
                            // For now, we accept any approval request since GuardianId mapping is not complete

                            // Perform guardian validation of the recovery request
                            let approval_decision = self.validate_recovery_request(
                                &account_id,
                                &recovery_context,
                                &request_id
                            ).await?;

                            // Send guardian challenge for additional verification
                            let challenge = self.generate_guardian_challenge(&request_id).await?;
                            let expires_at = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0) + 300; // 5 minutes
                            let _challenge_msg = GuardianAuthMessage::GuardianChallenge {
                                request_id: request_id.clone(),
                                challenge: challenge.clone(),
                                expires_at,
                            };

                            // TODO: Send challenge via NetworkEffects
                            tracing::info!("Would send challenge (placeholder)");

                            // Generate guardian signature for approval
                            let signature = self.sign_approval_decision(
                                &request_id,
                                &account_id,
                                approval_decision.approved,
                                &approval_decision.justification
                            ).await?;

                            // Send approval decision
                            let _decision_msg = GuardianAuthMessage::ApprovalDecision {
                                request_id: request_id.clone(),
                                guardian_id: device_id,
                                approved: approval_decision.approved,
                                justification: approval_decision.justification.clone(),
                                signature: signature.clone(),
                            };

                            // TODO: Send approval decision via NetworkEffects
                            tracing::info!("Would send approval decision (placeholder)");

                            // Create guardian approval record
                            let guardian_approval = GuardianApproval {
                                guardian_id: device_id,
                                verified_identity: VerifiedIdentity {
                                    proof: IdentityProof::Device {
                                        device_id,
                                        signature: aura_verify::Ed25519Signature::from_slice(&signature)
                                            .unwrap_or_else(|_|
                                                aura_verify::Ed25519Signature::from_slice(&[0u8; 64])
                                                    .unwrap()
                                            ),
                                    },
                                    message_hash: [0u8; 32], // Placeholder
                                },
                                approved: approval_decision.approved,
                                justification: approval_decision.justification,
                                signature,
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0),
                            };

                            tracing::info!(
                                "Guardian {} processed approval request: approved={}",
                                device_id, approval_decision.approved
                            );

                            return Ok(GuardianAuthResponse {
                                guardian_approvals: vec![guardian_approval],
                                success: approval_decision.approved,
                                error: if approval_decision.approved {
                                    None
                                } else {
                                    Some("Guardian denied recovery request".to_string())
                                },
                            });
                        }
                        _ => {
                            // Ignore other message types
                            continue;
                        }
                    }
                }

                // Small delay to avoid busy-looping
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }).await;

        match approval_response {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!("Guardian timed out waiting for approval requests");
                Ok(GuardianAuthResponse {
                    guardian_approvals: Vec::new(),
                    success: false,
                    error: Some("Guardian timed out waiting for approval requests".to_string()),
                })
            }
        }
    }

    /// Execute as coordinator
    async fn execute_coordinator(
        &self,
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
pub struct GuardianAuthCoordinator {
    /// Local effect system
    effect_system: AuraEffectSystem,
    /// Current choreography
    choreography: Option<GuardianAuthChoreography>,
}

impl GuardianAuthCoordinator {
    /// Create new coordinator
    pub fn new(effect_system: AuraEffectSystem) -> Self {
        Self {
            effect_system,
            choreography: None,
        }
    }

    /// Execute guardian authentication using choreography
    pub async fn authenticate_guardians(
        &mut self,
        request: GuardianAuthRequest,
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
            GuardianAuthChoreography::new(GuardianRole::Requester, self.effect_system.clone());

        // Execute the choreography
        let result = choreography.execute(request).await;

        // Store choreography for potential follow-up operations
        self.choreography = Some(choreography);

        result
    }

    /// Get the current effect system
    pub fn effect_system(&self) -> &AuraEffectSystem {
        &self.effect_system
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
        let guardian_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let challenge = vec![1, 2, 3, 4];
        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
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
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let effect_system = AuraEffectSystem::new(device_id, aura_protocol::handlers::ExecutionMode::Testing);

        let mut coordinator = GuardianAuthCoordinator::new(effect_system);
        assert!(!coordinator.has_active_choreography());

        let request = GuardianAuthRequest {
            requesting_device: device_id,
            account_id: AccountId(uuid::Uuid::from_bytes([0u8; 16])),
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

        // Note: This will return Ok with success=false since no guardians are discovered
        let result = coordinator
            .authenticate_guardians(request)
            .await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.success);
        assert!(coordinator.has_active_choreography());
    }
}
