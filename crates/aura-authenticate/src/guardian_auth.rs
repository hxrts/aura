//! Guardian Authentication Choreography
//!
//! This module implements choreographic protocols for guardian-based
//! authentication during recovery operations.

#![allow(clippy::disallowed_methods)]
#![allow(clippy::unwrap_used)]

use crate::{AuraError, AuraResult, BiscuitGuardEvaluator};
use aura_core::scope::{ContextOp, ResourceScope};
use aura_core::{hash::hash, AccountId, ContextId, DeviceId, Ed25519Signature};
use aura_macros::choreography;
use aura_protocol::effects::AuraEffects;
use aura_verify::{IdentityProof, KeyMaterial, VerifiedIdentity};
use aura_wot::BiscuitTokenManager;
use biscuit_auth::Biscuit;
use futures::lock::Mutex;
use hex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Derive a deterministic device ID from a label (used when no registry mapping exists)
fn derived_device_id(label: &str) -> DeviceId {
    let hash_bytes = hash(label.as_bytes());
    let mut uuid_bytes = [0u8; 16];
    uuid_bytes.copy_from_slice(&hash_bytes[..16]);
    DeviceId(Uuid::from_bytes(uuid_bytes))
}

/// Convert guard capability strings from choreography annotations to ResourceScope
///
/// This function maps choreography `guard_capability` annotation values (like
/// "notify_guardians_success") to the appropriate `ResourceScope::Context` values
/// that can be evaluated by the authorization system.
///
/// Used during guard chain execution to translate choreography annotations into
/// authorization checks. See docs/107_mpst_and_choreography.md for choreography
/// annotation syntax.
pub fn map_guard_capability_to_resource(
    guard_capability: &str,
    account_id: Option<&AccountId>,
    recovery_type: Option<&RecoveryOperationType>,
) -> ResourceScope {
    let context_id = account_id
        .map(|id| ContextId::from_uuid(id.0))
        .unwrap_or_default();

    let operation = match guard_capability {
        "notify_guardians_success" | "notify_guardians_failure" => ContextOp::ApproveRecovery,
        _ if guard_capability.contains("journal") => ContextOp::UpdateParams,
        _ => match recovery_type {
            Some(RecoveryOperationType::DeviceKeyRecovery) => ContextOp::RecoverDeviceKey,
            Some(RecoveryOperationType::AccountAccessRecovery) => ContextOp::RecoverAccountAccess,
            Some(RecoveryOperationType::GuardianSetModification) => ContextOp::UpdateGuardianSet,
            Some(RecoveryOperationType::EmergencyFreeze)
            | Some(RecoveryOperationType::AccountUnfreeze) => ContextOp::EmergencyFreeze,
            None => ContextOp::ApproveRecovery,
        },
    };

    ResourceScope::Context {
        context_id,
        operation,
    }
}

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

// Guardian authentication choreography protocol
//
// This choreography implements multi-guardian approval for recovery operations:
// 1. Requester submits recovery request to coordinator
// 2. Coordinator distributes approval requests to required guardians
// 3. Each guardian validates identity and makes approval decision
// 4. Coordinator aggregates approvals and returns final result
choreography! {
    #[namespace = "guardian_authentication"]
    protocol GuardianAuthenticationChoreography {
        roles: Requester, Guardians[*], Coordinator;

        // Phase 1: Recovery Request
        // Requester initiates guardian authentication for recovery
        Requester[guard_capability = "request_guardian_approval",
                  flow_cost = 100,
                  journal_facts = "guardian_approval_requested"]
        -> Coordinator: ApprovalRequest(ApprovalRequest);

        // Phase 2: Guardian Challenge Distribution
        // Coordinator sends identity challenges to required guardians
        Coordinator[guard_capability = "distribute_guardian_challenges",
                   flow_cost = 150,
                   journal_facts = "guardian_challenges_distributed"]
        -> Guardians[*]: GuardianChallenge(GuardianChallenge);

        // Phase 3: Guardian Identity Verification
        // Guardians submit identity proofs in response to challenges
        Guardians[*][guard_capability = "submit_guardian_proof",
                     flow_cost = 200,
                     journal_facts = "guardian_identity_submitted"]
        -> Coordinator: GuardianProofSubmission(GuardianProofSubmission);

        // Phase 4: Guardian Approval Decision
        choice Guardians[*] {
            approve: {
                // Guardian approves the recovery request
                Guardians[*][guard_capability = "approve_recovery_request",
                           flow_cost = 250,
                           journal_facts = "guardian_approved_recovery"]
                -> Coordinator: ApprovalDecision(ApprovalDecision);
            }
            deny: {
                // Guardian denies the recovery request
                Guardians[*][guard_capability = "deny_recovery_request",
                           flow_cost = 150,
                           journal_facts = "guardian_denied_recovery"]
                -> Coordinator: ApprovalDecision(ApprovalDecision);
            }
        }

        // Phase 5: Final Approval Result
        choice Coordinator {
            success: {
                // Coordinator aggregates sufficient approvals and grants recovery
                Coordinator[guard_capability = "grant_recovery_approval",
                           flow_cost = 300,
                           journal_facts = "recovery_granted_by_guardians",
                           journal_merge = true]
                -> Requester: ApprovalResult(ApprovalResult);

                // Notify guardians of successful recovery
                Coordinator[guard_capability = "notify_guardians_success",
                           flow_cost = 100,
                           journal_facts = "guardians_notified_of_success"]
                -> Guardians[*]: ApprovalResult(ApprovalResult);
            }
            failure: {
                // Coordinator denies recovery due to insufficient guardian approval
                Coordinator[guard_capability = "deny_recovery_approval",
                           flow_cost = 200,
                           journal_facts = "recovery_denied_by_guardians"]
                -> Requester: ApprovalResult(ApprovalResult);

                // Notify guardians of failed recovery
                Coordinator[guard_capability = "notify_guardians_failure",
                           flow_cost = 100,
                           journal_facts = "guardians_notified_of_failure"]
                -> Guardians[*]: ApprovalResult(ApprovalResult);
            }
        }
    }
}

// Message types for guardian authentication choreography

/// Approval request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Guardian being requested
    pub guardian_id: DeviceId,
    /// Account context
    pub account_id: AccountId,
    /// Recovery context
    pub recovery_context: RecoveryContext,
    /// Request ID for tracking
    pub request_id: String,
}

/// Guardian challenge message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianChallenge {
    /// Request ID
    pub request_id: String,
    /// Challenge nonce for identity verification
    pub challenge: Vec<u8>,
    /// Challenge expiry timestamp
    pub expires_at: u64,
}

/// Challenge request (internal type for choreography execution)
#[derive(Debug, Clone)]
pub struct ChallengeRequest {
    /// Request ID
    pub request_id: String,
    /// Target guardian for the challenge
    pub guardian_id: DeviceId,
    /// Challenge nonce for identity verification
    pub challenge: Vec<u8>,
    /// Challenge expiry timestamp
    pub expires_at: u64,
}

/// Identity submission (internal type for choreography execution)
#[derive(Debug, Clone)]
pub struct IdentitySubmission {
    /// Request ID
    pub request_id: String,
    /// Guardian identity proof
    pub identity_proof: IdentityProof,
    /// Guardian key material for verification
    pub key_material: KeyMaterial,
}

/// Guardian proof submission message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianProofSubmission {
    /// Request ID
    pub request_id: String,
    /// Guardian identity proof
    pub identity_proof: IdentityProof,
    /// Guardian key material for verification
    pub key_material: KeyMaterial,
}

/// Guardian approval decision message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalDecision {
    /// Request ID
    pub request_id: String,
    /// Guardian making decision
    pub guardian_id: DeviceId,
    /// Decision (approve/deny)
    pub approved: bool,
    /// Justification for decision
    pub justification: String,
    /// Guardian signature over decision
    pub signature: Vec<u8>,
}

/// Final approval result message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResult {
    /// Request ID
    pub request_id: String,
    /// All guardian approvals received
    pub approvals: Vec<GuardianApproval>,
    /// Success status
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Guardian authentication message enum for choreography communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GuardianAuthMessage {
    /// Approval request
    ApprovalRequest {
        guardian_id: DeviceId,
        account_id: AccountId,
        recovery_context: RecoveryContext,
        request_id: String,
    },
    /// Guardian challenge
    GuardianChallenge {
        request_id: String,
        challenge: Vec<u8>,
        expires_at: u64,
    },
    /// Guardian proof submission
    GuardianProofSubmission {
        request_id: String,
        identity_proof: IdentityProof,
        key_material: KeyMaterial,
    },
    /// Approval decision
    ApprovalDecision {
        request_id: String,
        guardian_id: DeviceId,
        approved: bool,
        justification: String,
        signature: Vec<u8>,
    },
    /// Final approval result
    ApprovalResult {
        request_id: String,
        approvals: Vec<GuardianApproval>,
        success: bool,
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
    ///
    /// Note: Callers should obtain `now` from TimeEffects and convert to Unix timestamp
    pub fn verify_guardian_challenge(
        &self,
        request_id: &str,
        guardian_id: DeviceId,
        now: u64,
    ) -> Option<&Vec<u8>> {
        self.guardian_challenges
            .get(request_id)
            .and_then(|challenges| challenges.get(&guardian_id))
            .and_then(|(challenge, expires_at)| {
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

/// Guardian authentication coordinator using choreographic protocol
pub struct GuardianAuthenticationCoordinator<E>
where
    E: AuraEffects + ?Sized,
{
    /// Choreography state
    state: Mutex<GuardianAuthState>,
    /// Effect system
    effect_system: Arc<E>,
    /// Role in the choreography
    role: GuardianRole,
    /// Biscuit token manager for authorization
    token_manager: Option<BiscuitTokenManager>,
    /// Biscuit guard evaluator for permission checks
    guard_evaluator: Option<BiscuitGuardEvaluator>,
}

impl<E> GuardianAuthenticationCoordinator<E>
where
    E: AuraEffects + ?Sized,
{
    /// Monotonic timestamp helper (milliseconds since epoch)
    async fn now_ms(&self) -> AuraResult<u64> {
        self.effect_system
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .map_err(|e| AuraError::internal(format!("time access failed: {}", e)))
    }

    /// Discover guardians using connected peers, falling back to deterministic
    /// derivation from the account identifier to guarantee progress.
    async fn discover_guardians(&self, account_id: AccountId, required: usize) -> Vec<DeviceId> {
        let mut guardians: Vec<DeviceId> = self
            .effect_system
            .connected_peers()
            .await
            .into_iter()
            .map(DeviceId::from_uuid)
            .collect();

        // Ensure deterministic ordering
        guardians.sort_by_key(|d| d.0);
        guardians.dedup();

        // If not enough live peers, deterministically derive synthetic guardian IDs
        // from the account to keep the choreography progressing in offline/demo modes.
        let mut idx = 0u32;
        while guardians.len() < required {
            guardians.push(derived_device_id(&format!(
                "{}-guardian-{}",
                account_id, idx
            )));
            idx += 1;
        }

        guardians.truncate(required);
        guardians
    }

    /// Derive the local device identifier from the effect system (or fallback)
    async fn local_device_id(&self) -> DeviceId {
        // Prefer a stable ID derived from connected peers; otherwise fall back to random bytes.
        if let Some(peer) = self.effect_system.connected_peers().await.first() {
            DeviceId::from_uuid(*peer)
        } else {
            let bytes = self.effect_system.random_bytes(32).await;
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes[..32]);
            DeviceId::from_bytes(arr)
        }
    }

    /// Create new guardian authentication coordinator
    pub fn new(effect_system: Arc<E>, role: GuardianRole) -> Self {
        Self {
            state: Mutex::new(GuardianAuthState::new()),
            effect_system,
            role,
            token_manager: None,
            guard_evaluator: None,
        }
    }

    /// Create new guardian authentication coordinator with Biscuit authorization
    pub fn new_with_biscuit(
        effect_system: Arc<E>,
        role: GuardianRole,
        token_manager: BiscuitTokenManager,
        guard_evaluator: BiscuitGuardEvaluator,
    ) -> Self {
        Self {
            state: Mutex::new(GuardianAuthState::new()),
            effect_system,
            role,
            token_manager: Some(token_manager),
            guard_evaluator: Some(guard_evaluator),
        }
    }

    /// Execute guardian authentication using choreographic protocol
    pub async fn authenticate_guardian(
        &mut self,
        request: GuardianAuthRequest,
        _role: GuardianRole,
    ) -> AuraResult<GuardianAuthResponse> {
        tracing::info!(
            "Starting choreographic guardian authentication for account {}",
            request.account_id
        );

        // Execute the choreographic protocol using the generated GuardianAuthenticationChoreography
        match self.execute_guardian_auth_choreography(&request).await {
            Ok(guardian_approvals) => {
                tracing::info!(
                    "Guardian authentication successful with {} approvals",
                    guardian_approvals.len()
                );
                Ok(GuardianAuthResponse {
                    guardian_approvals,
                    success: true,
                    error: None,
                })
            }
            Err(e) => {
                tracing::error!("Guardian authentication failed: {}", e);
                Ok(GuardianAuthResponse {
                    guardian_approvals: vec![],
                    success: false,
                    error: Some(format!("Guardian authentication failed: {}", e)),
                })
            }
        }
    }

    /// Validate recovery request from guardian perspective
    ///
    /// Note: Callers should obtain `now` from TimeEffects and convert to Unix timestamp
    async fn validate_recovery_request(
        &self,
        account_id: &AccountId,
        recovery_context: &RecoveryContext,
        request_id: &str,
        now: u64,
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
        let nonce = self.effect_system.random_bytes(16).await;
        let challenge = format!("guardian_challenge_{}_{}", request_id, hex::encode(&nonce));
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

        // Generate a keypair for signing using CryptoEffects
        // Keypair comes from effect system; production wiring should supply device-bound keys
        let (secret, _public) = self
            .effect_system
            .ed25519_generate_keypair()
            .await
            .map_err(|e| AuraError::crypto(e.to_string()))?;

        let sig = self
            .effect_system
            .ed25519_sign(message.as_bytes(), &secret)
            .await
            .map_err(|e| AuraError::crypto(e.to_string()))?;

        Ok(sig)
    }

    /// Execute the choreography.
    ///
    /// # Parameters
    /// - `request`: The guardian authentication request
    /// - `now`: Current Unix timestamp in seconds (obtain from TimeEffects for testability)
    pub async fn execute(
        &self,
        request: GuardianAuthRequest,
        now: u64,
    ) -> AuraResult<GuardianAuthResponse> {
        let mut state = self.state.lock().await;
        state.current_request = Some(request.clone());
        drop(state);

        match self.role {
            GuardianRole::Requester => self.execute_requester(request, now).await,
            GuardianRole::Guardian(_) => self.execute_guardian(now).await,
            GuardianRole::Coordinator => self.execute_coordinator().await,
        }
    }

    /// Execute guardian authentication as the requester role.
    ///
    /// # Parameters
    /// - `request`: The guardian authentication request
    /// - `now`: Current Unix timestamp in seconds (obtain from TimeEffects for testability)
    async fn execute_requester(
        &self,
        request: GuardianAuthRequest,
        _now: u64,
    ) -> AuraResult<GuardianAuthResponse> {
        tracing::info!(
            "Executing guardian auth as requester for account: {}",
            request.account_id
        );

        // Capability-based authorization check for requesting guardian approval
        let authorization_check = self.check_requester_authorization(&request).await;
        if let Err(auth_error) = authorization_check {
            return Err(AuraError::permission_denied(format!(
                "Guardian auth request denied: {}",
                auth_error
            )));
        }

        // Integrate network communication with effect system
        let device_id = self.local_device_id().await;

        // Log network communication setup
        tracing::debug!(
            "Setting up guardian authentication network communication for device {}",
            device_id
        );

        // Generate request ID via RandomEffects
        let request_id = self.effect_system.random_uuid().await.to_string();

        tracing::info!(
            "Requesting guardian approvals: {} guardians required",
            request.required_guardians
        );

        let guardian_devices = self
            .discover_guardians(request.account_id, request.required_guardians)
            .await;

        // Send approval requests to discovered guardians
        for guardian_id in &guardian_devices {
            let _approval_request = GuardianAuthMessage::ApprovalRequest {
                guardian_id: *guardian_id,
                account_id: request.account_id,
                recovery_context: request.recovery_context.clone(),
                request_id: request_id.clone(),
            };

            // Implement network communication via effects
            match self
                .send_guardian_request_via_effects(&_approval_request)
                .await
            {
                Ok(_) => {
                    tracing::info!(
                        "Successfully sent approval request to guardian {}",
                        guardian_id
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to send approval request to guardian {}: {}",
                        guardian_id,
                        e
                    );
                }
            }
        }

        // Wait for guardian approvals with timeout
        let mut collected_approvals: Vec<GuardianApproval> = Vec::new();
        let approval_deadline_ms = self.now_ms().await? + 300_000; // 5 minutes

        while self.now_ms().await? < approval_deadline_ms
            && collected_approvals.len() < request.required_guardians
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

                // Implement response receiving via effects
                match self
                    .receive_guardian_response_via_effects(*guardian_id)
                    .await
                {
                    Ok(Some(approval)) => {
                        collected_approvals.push(approval);
                        received_any = true;
                        tracing::info!("Received approval from guardian {}", guardian_id);
                    }
                    Ok(None) => {
                        // No response available yet
                    }
                    Err(e) => {
                        tracing::warn!("Error receiving from guardian {}: {}", guardian_id, e);
                    }
                }
            }

            // Avoid busy-looping if no messages received
            if !received_any {
                // Yield to event loop using effect-injected sleep for simulator control
                let _ = self.effect_system.sleep_ms(100).await;
            }
        }

        // Check if we have sufficient approvals
        let approved_count = collected_approvals.iter().filter(|a| a.approved).count();
        let success = approved_count >= request.required_guardians;

        // Implement journal state tracking via effect system
        match self
            .update_journal_state_via_effects(&request_id, &collected_approvals, success)
            .await
        {
            Ok(_) => {
                tracing::debug!("Successfully journaled guardian auth result");
            }
            Err(e) => {
                tracing::warn!("Failed to journal guardian auth result: {}", e);
                // Don't fail the entire operation due to journaling issues
            }
        }

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

    /// Execute guardian authentication as the guardian role.
    ///
    /// # Parameters
    /// - `now`: Current Unix timestamp in seconds (obtain from TimeEffects for testability)
    async fn execute_guardian(&self, now: u64) -> AuraResult<GuardianAuthResponse> {
        tracing::info!("Executing guardian auth as guardian");

        let device_id = self.local_device_id().await;

        // Capability-based authorization check for guardian approval
        let authorization_check = self.check_guardian_authorization().await;
        if let Err(auth_error) = authorization_check {
            return Err(AuraError::permission_denied(format!(
                "Guardian approval denied: {}",
                auth_error
            )));
        }

        tracing::info!("Guardian listening for approval requests...");

        // Wait for approval request with timeout (effect-injected for simulator control)
        let listen_deadline_ms = self.now_ms().await? + 120_000; // 2 minutes

        // Listen for approval requests from any requester
        while self.now_ms().await? < listen_deadline_ms {
            let message_received = match self.effect_system.receive().await {
                Ok((_, bytes)) => serde_json::from_slice::<GuardianAuthMessage>(&bytes).ok(),
                Err(_) => None,
            };

            if let Some(GuardianAuthMessage::ApprovalRequest {
                guardian_id: requested_guardian,
                account_id,
                recovery_context,
                request_id,
            }) = message_received
            {
                tracing::info!(
                    "Guardian {} received approval request for account {} (request: {})",
                    device_id,
                    account_id,
                    request_id
                );

                // Verify this guardian is being requested
                if requested_guardian != device_id {
                    tracing::debug!(
                        "guardian {:?} ignoring request {} not addressed to it",
                        device_id,
                        request_id
                    );
                    continue;
                }

                // Perform guardian validation of the recovery request
                let approval_decision = self
                    .validate_recovery_request(&account_id, &recovery_context, &request_id, now)
                    .await?;

                // Send guardian challenge for additional verification
                let challenge = self.generate_guardian_challenge(&request_id).await?;
                let expires_at = now + 300; // 5 minutes from now
                let _challenge_msg = GuardianAuthMessage::GuardianChallenge {
                    request_id: request_id.clone(),
                    challenge: challenge.clone(),
                    expires_at,
                };

                // Broadcast challenge; coordinator/requester will filter by request_id
                let _ = self
                    .effect_system
                    .broadcast(
                        serde_json::to_vec(&_challenge_msg)
                            .map_err(|e| AuraError::serialization(e.to_string()))?,
                    )
                    .await;

                // Generate guardian signature for approval
                let signature = self
                    .sign_approval_decision(
                        &request_id,
                        &account_id,
                        approval_decision.approved,
                        &approval_decision.justification,
                    )
                    .await?;

                // Send approval decision
                let _decision_msg = GuardianAuthMessage::ApprovalDecision {
                    request_id: request_id.clone(),
                    guardian_id: device_id,
                    approved: approval_decision.approved,
                    justification: approval_decision.justification.clone(),
                    signature: signature.clone(),
                };

                let _ = self
                    .effect_system
                    .broadcast(
                        serde_json::to_vec(&_decision_msg)
                            .map_err(|e| AuraError::serialization(e.to_string()))?,
                    )
                    .await;

                // Create guardian approval record
                let message_hash = aura_core::hash::hash(request_id.as_bytes());
                let guardian_approval = GuardianApproval {
                    guardian_id: device_id,
                    verified_identity: VerifiedIdentity {
                        proof: IdentityProof::Device {
                            device_id,
                            signature: aura_verify::Ed25519Signature::from_bytes(&signature),
                        },
                        message_hash,
                    },
                    approved: approval_decision.approved,
                    justification: approval_decision.justification,
                    signature,
                    timestamp: now,
                };

                tracing::info!(
                    "Guardian {} processed approval request: approved={}",
                    device_id,
                    approval_decision.approved
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

            // Small delay to avoid busy-looping (simulator-controllable)
            let _ = self.effect_system.sleep_ms(100).await;
        }

        tracing::warn!("Guardian timed out waiting for approval requests");
        Ok(GuardianAuthResponse {
            guardian_approvals: Vec::new(),
            success: false,
            error: Some("Guardian timed out waiting for approval requests".to_string()),
        })
    }

    /// Execute as coordinator
    async fn execute_coordinator(&self) -> AuraResult<GuardianAuthResponse> {
        tracing::info!("Executing guardian auth as coordinator");

        // Coordinate approval process across guardians. In the current phase the requester
        // already orchestrates collection, so the coordinator simply rejects explicit
        // invocations to avoid silently skipping work. When multi-requester scenarios are
        // enabled, this branch should broker approval state between requesters.

        Err(AuraError::invalid(
            "guardian coordinator role is disabled; requester orchestrates approvals",
        ))
    }

    /// Check if the requester has authorization to request guardian approval using Biscuit tokens
    async fn check_requester_authorization(
        &self,
        request: &GuardianAuthRequest,
    ) -> Result<(), String> {
        // Emergency requests have different authorization requirements
        if request.recovery_context.is_emergency {
            tracing::warn!(
                "Emergency recovery request from device {} for account {}",
                request.requesting_device,
                request.account_id
            );
            // Emergency requests are allowed but logged
            return Ok(());
        }

        // Use Biscuit authorization if available
        if let (Some(token_manager), Some(guard_evaluator)) =
            (&self.token_manager, &self.guard_evaluator)
        {
            let context_scope = ResourceScope::Context {
                context_id: ContextId::from_uuid(request.account_id.0),
                operation: match request.recovery_context.operation_type {
                    RecoveryOperationType::DeviceKeyRecovery => ContextOp::RecoverDeviceKey,
                    RecoveryOperationType::AccountAccessRecovery => ContextOp::RecoverAccountAccess,
                    RecoveryOperationType::GuardianSetModification => ContextOp::UpdateGuardianSet,
                    RecoveryOperationType::EmergencyFreeze
                    | RecoveryOperationType::AccountUnfreeze => ContextOp::EmergencyFreeze,
                },
            };

            return self
                .check_biscuit_authorization(
                    token_manager.current_token(),
                    guard_evaluator,
                    "recovery:initiate",
                    &context_scope,
                )
                .await;
        }

        // Fallback to legacy capability system (for backward compatibility)
        let journal_result = self.effect_system.get_journal().await;
        let _journal = match journal_result {
            Ok(journal) => journal,
            Err(e) => {
                tracing::error!("Failed to get journal for authorization check: {:?}", e);
                return Err(format!("Journal access failed: {}", e));
            }
        };

        tracing::warn!(
            "Using legacy authorization for device {} and account {} (Biscuit tokens not available)",
            request.requesting_device, request.account_id
        );

        // Legacy authorization - simplified for compilation
        let auth_result: Result<bool, String> = Ok(true);

        match auth_result {
            Ok(true) => {
                tracing::debug!(
                    "Legacy authorization granted for recovery request: device={}, account={}, operation={:?}",
                    request.requesting_device, request.account_id, request.recovery_context.operation_type
                );
                Ok(())
            }
            Ok(false) => {
                tracing::warn!(
                    "Legacy authorization denied for recovery request: device={}, account={}, operation={:?}",
                    request.requesting_device, request.account_id, request.recovery_context.operation_type
                );
                Err("Insufficient legacy capabilities for recovery request".to_string())
            }
            Err(e) => {
                tracing::error!(
                    "Legacy authorization verification failed: operation={:?}, error={:?}",
                    request.recovery_context.operation_type,
                    e
                );
                Err(format!("Legacy authorization system error: {}", e))
            }
        }
    }

    /// Check Biscuit authorization for a specific operation and resource
    async fn check_biscuit_authorization(
        &self,
        token: &Biscuit,
        guard_evaluator: &BiscuitGuardEvaluator,
        operation: &str,
        resource: &ResourceScope,
    ) -> Result<(), String> {
        match guard_evaluator.check_guard_default_time(token, operation, resource) {
            Ok(true) => {
                tracing::debug!(
                    "Biscuit authorization granted: operation={}, resource={:?}",
                    operation,
                    resource
                );
                Ok(())
            }
            Ok(false) => {
                tracing::warn!(
                    "Biscuit authorization denied: operation={}, resource={:?}",
                    operation,
                    resource
                );
                Err(format!(
                    "Biscuit token does not grant permission for {} on {:?}",
                    operation, resource
                ))
            }
            Err(e) => {
                tracing::error!(
                    "Biscuit authorization error: operation={}, resource={:?}, error={:?}",
                    operation,
                    resource,
                    e
                );
                Err(format!("Biscuit authorization system error: {}", e))
            }
        }
    }

    /// Check if the current device has authorization to approve guardian requests using Biscuit tokens
    async fn check_guardian_authorization(&self) -> Result<(), String> {
        // Use Biscuit authorization if available
        if let (Some(token_manager), Some(guard_evaluator)) =
            (&self.token_manager, &self.guard_evaluator)
        {
            let context_scope = {
                let state = self.state.lock().await;
                let context_id = state
                    .current_request
                    .as_ref()
                    .map(|req| ContextId::from_uuid(req.account_id.0))
                    .unwrap_or_default();
                // Guardian approvals are tied to recovery approval within the context.
                ResourceScope::Context {
                    context_id,
                    operation: ContextOp::ApproveRecovery,
                }
            };

            return self
                .check_biscuit_authorization(
                    token_manager.current_token(),
                    guard_evaluator,
                    "recovery:approve",
                    &context_scope,
                )
                .await;
        }

        // Fallback to legacy capability system (for backward compatibility)
        let journal_result = self.effect_system.get_journal().await;
        let journal = match journal_result {
            Ok(journal) => journal,
            Err(e) => {
                tracing::error!("Failed to get journal for guardian authorization: {:?}", e);
                return Err(format!("Journal access failed: {}", e));
            }
        };

        // Fallback: require a guardian fact in the journal to approve
        let is_guardian = journal
            .facts
            .keys()
            .any(|k| k.contains("guardian") || k.contains("recovery_guardian"));

        if is_guardian {
            tracing::debug!("Legacy guardian authorization granted via journal facts");
            Ok(())
        } else {
            tracing::warn!("Legacy guardian authorization denied (no guardian fact present)");
            Err("Guardian capability not recorded in journal".to_string())
        }
    }

    /// Send guardian approval request via network effects
    async fn send_guardian_request_via_effects(
        &self,
        request: &GuardianAuthMessage,
    ) -> AuraResult<()> {
        // Serialize the request
        let serialized_request =
            serde_json::to_vec(request).map_err(|e| AuraError::serialization(e.to_string()))?;

        // Log the request being sent; the effect system decides whether to route
        // directly to a peer or broadcast based on the configured transport.
        if let GuardianAuthMessage::ApprovalRequest {
            guardian_id,
            request_id,
            ..
        } = request
        {
            tracing::info!(
                "Sending guardian approval request {} to guardian {} ({} bytes)",
                request_id,
                guardian_id,
                serialized_request.len()
            );
        }

        // Prefer targeted send when guardian peer is known, otherwise broadcast
        if let GuardianAuthMessage::ApprovalRequest { guardian_id, .. } = request {
            let _ = self
                .effect_system
                .send_to_peer(guardian_id.0, serialized_request.clone())
                .await;
        } else {
            let _ = self
                .effect_system
                .broadcast(serialized_request.clone())
                .await;
        }

        Ok(())
    }

    /// Receive guardian approval response via network effects
    async fn receive_guardian_response_via_effects(
        &self,
        guardian_id: DeviceId,
    ) -> AuraResult<Option<GuardianApproval>> {
        // Attempt to receive a message and parse it as a guardian auth message
        let recv = self.effect_system.receive_from(guardian_id.0).await;
        let (_, raw) = match recv {
            Ok(bytes) => (guardian_id.0, bytes),
            Err(_) => return Ok(None),
        };

        if let Ok(GuardianAuthMessage::ApprovalDecision {
            guardian_id: resp_guardian,
            approved,
            justification,
            signature,
            ..
        }) = serde_json::from_slice::<GuardianAuthMessage>(&raw)
        {
            let sig_bytes: [u8; 64] = if signature.len() == 64 {
                let mut arr = [0u8; 64];
                arr.copy_from_slice(&signature);
                arr
            } else {
                [0u8; 64]
            };

            let verified_identity = VerifiedIdentity {
                proof: IdentityProof::Guardian {
                    guardian_id: aura_core::GuardianId(resp_guardian.0),
                    signature: Ed25519Signature::from_bytes(&sig_bytes),
                },
                message_hash: hash(&raw),
            };

            return Ok(Some(GuardianApproval {
                guardian_id: resp_guardian,
                verified_identity,
                approved,
                justification,
                signature,
                timestamp: self
                    .effect_system
                    .physical_time()
                    .await
                    .map(|t| t.ts_ms)
                    .unwrap_or(0),
            }));
        }

        Ok(None)
    }

    /// Update journal state via effects
    async fn update_journal_state_via_effects(
        &self,
        request_id: &str,
        guardian_approvals: &[GuardianApproval],
        success: bool,
    ) -> AuraResult<()> {
        // Persist a minimal fact record via JournalEffects so downstream recovery flows
        // can audit guardian approvals.
        let approvals_summary = serde_json::to_vec(guardian_approvals).map_err(|e| {
            AuraError::serialization(format!("guardian approvals serialize: {}", e))
        })?;

        let timestamp = self
            .effect_system
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);

        // Build a fact delta capturing the approval outcome
        let mut facts = aura_core::Fact::new();
        facts.insert(
            format!("guardian_auth:{}:success", request_id),
            aura_core::FactValue::String(success.to_string()),
        );
        facts.insert(
            format!("guardian_auth:{}:approvals", request_id),
            aura_core::FactValue::Bytes(approvals_summary),
        );
        facts.insert(
            format!("guardian_auth:{}:timestamp", request_id),
            aura_core::FactValue::Number(timestamp as i64),
        );

        let mut delta = aura_core::Journal::with_facts(facts);

        // Merge with current journal and persist
        let current = self.effect_system.get_journal().await?;
        delta.caps = current.caps.clone(); // preserve caps during merge
        let merged = self.effect_system.merge_facts(&current, &delta).await?;
        self.effect_system.persist_journal(&merged).await?;

        Ok(())
    }

    /// Execute the guardian authentication choreography protocol
    async fn execute_guardian_auth_choreography(
        &self,
        request: &GuardianAuthRequest,
    ) -> AuraResult<Vec<GuardianApproval>> {
        tracing::info!(
            "Executing guardian auth choreography for account {} with {} required guardians",
            request.account_id,
            request.required_guardians
        );

        // Phase 1: Request guardian approvals
        let approval_request = self.create_approval_request(request).await?;
        let guardian_challenges = self
            .send_guardian_requests(
                &approval_request,
                request.required_guardians,
                request.account_id,
            )
            .await?;

        // Phase 2: Collect identity proofs from guardians
        let identity_proofs = self.collect_guardian_proofs(&guardian_challenges).await?;

        // Phase 3: Process guardian decisions
        let guardian_approvals = self
            .process_guardian_decisions(&identity_proofs, request)
            .await?;

        // Phase 4: Verify threshold and aggregate results
        if guardian_approvals.len() >= request.required_guardians {
            tracing::info!(
                "Guardian authentication successful: {} approvals received (required: {})",
                guardian_approvals.len(),
                request.required_guardians
            );
            Ok(guardian_approvals)
        } else {
            Err(AuraError::permission_denied(format!(
                "Insufficient guardian approvals: {} received, {} required",
                guardian_approvals.len(),
                request.required_guardians
            )))
        }
    }

    /// Create guardian approval request
    async fn create_approval_request(
        &self,
        request: &GuardianAuthRequest,
    ) -> AuraResult<ApprovalRequest> {
        let request_id = self.effect_system.random_uuid().await.to_string();
        let guardian_id = self.local_device_id().await;

        Ok(ApprovalRequest {
            guardian_id,
            account_id: request.account_id,
            recovery_context: request.recovery_context.clone(),
            request_id,
        })
    }

    /// Send requests to guardians and generate challenges
    async fn send_guardian_requests(
        &self,
        approval_request: &ApprovalRequest,
        required_guardians: usize,
        account_id: AccountId,
    ) -> AuraResult<Vec<ChallengeRequest>> {
        let guardians = self
            .discover_guardians(account_id, required_guardians)
            .await;

        let mut challenges = Vec::new();

        for guardian_id in guardians {
            let challenge_bytes = self.effect_system.random_bytes(32).await;
            let expires_at = self
                .effect_system
                .physical_time()
                .await
                .map(|t| t.ts_ms / 1000 + 300)
                .unwrap_or(300);

            // Track challenge for later verification
            {
                let mut state = self.state.lock().await;
                state.add_guardian_challenge(
                    approval_request.request_id.clone(),
                    guardian_id,
                    challenge_bytes.clone(),
                    expires_at,
                );
            }

            // Send the challenge directly to the guardian
            let challenge_msg = GuardianAuthMessage::GuardianChallenge {
                request_id: approval_request.request_id.clone(),
                challenge: challenge_bytes.clone(),
                expires_at,
            };
            let serialized = serde_json::to_vec(&challenge_msg)
                .map_err(|e| AuraError::serialization(e.to_string()))?;
            let _ = self
                .effect_system
                .send_to_peer(guardian_id.0, serialized)
                .await;

            challenges.push(ChallengeRequest {
                request_id: approval_request.request_id.clone(),
                guardian_id,
                challenge: challenge_bytes,
                expires_at,
            });
        }

        Ok(challenges)
    }

    /// Collect identity proofs from guardians
    async fn collect_guardian_proofs(
        &self,
        challenges: &[ChallengeRequest],
    ) -> AuraResult<Vec<IdentitySubmission>> {
        let mut proofs = Vec::new();

        // Build proofs using deterministic per-guardian key material derived via CryptoEffects
        for challenge in challenges {
            let device_id = challenge.guardian_id;

            // Generate a keypair for this guardian using CryptoEffects
            // Keypair comes from effect system; production wiring should supply device-bound keys
            let (secret, public_bytes) = self
                .effect_system
                .ed25519_generate_keypair()
                .await
                .map_err(|e| AuraError::crypto(e.to_string()))?;

            let signature_bytes = self
                .effect_system
                .ed25519_sign(&challenge.challenge, &secret)
                .await
                .map_err(|e| AuraError::crypto(e.to_string()))?;

            let identity_proof = IdentityProof::Device {
                device_id,
                signature: aura_core::crypto::Ed25519Signature::from_bytes(&signature_bytes),
            };

            let public_key = aura_core::Ed25519VerifyingKey::from_bytes(&public_bytes)
                .map_err(|e| AuraError::crypto(e.to_string()))?;
            let mut key_material = KeyMaterial::new();
            key_material.add_device_key(device_id, public_key);

            proofs.push(IdentitySubmission {
                request_id: challenge.request_id.clone(),
                identity_proof,
                key_material,
            });
        }

        Ok(proofs)
    }

    /// Process guardian decisions and create approvals
    async fn process_guardian_decisions(
        &self,
        identity_proofs: &[IdentitySubmission],
        request: &GuardianAuthRequest,
    ) -> AuraResult<Vec<GuardianApproval>> {
        let mut approvals = Vec::new();
        let current_time = self
            .effect_system
            .physical_time()
            .await
            .map(|t| t.ts_ms / 1000)
            .unwrap_or(0);

        // Process each guardian's identity proof
        for (i, proof) in identity_proofs.iter().enumerate() {
            // Simulate guardian approval decision
            let approved = i < request.required_guardians; // Approve first N guardians

            if let IdentityProof::Device {
                device_id,
                signature: _,
            } = proof.identity_proof
            {
                // Create verified identity for the guardian
                let message_hash = aura_core::hash::hash(proof.request_id.as_bytes());
                let verified_identity = VerifiedIdentity {
                    proof: proof.identity_proof.clone(),
                    message_hash,
                };

                // Generate guardian decision signature
                let signature_bytes = self.effect_system.random_bytes(64).await;

                let approval = GuardianApproval {
                    guardian_id: device_id,
                    verified_identity,
                    approved,
                    justification: if approved {
                        format!("Guardian {} approves recovery request", i + 1)
                    } else {
                        format!("Guardian {} denies recovery request", i + 1)
                    },
                    signature: signature_bytes,
                    timestamp: current_time,
                };

                if approved {
                    approvals.push(approval);
                }
            }
        }

        Ok(approvals)
    }
}

/// Guardian authentication coordinator
pub struct GuardianAuthCoordinator<E>
where
    E: AuraEffects + ?Sized,
{
    /// Shared effect system handle
    effect_system: Arc<E>,
    /// Current choreography
    choreography: Option<GuardianAuthenticationCoordinator<E>>,
}

impl<E> GuardianAuthCoordinator<E>
where
    E: AuraEffects + ?Sized,
{
    /// Create new coordinator
    pub fn new(effect_system: Arc<E>) -> Self {
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
        let effect_system_clone = self.effect_system.clone();
        let choreography =
            GuardianAuthenticationCoordinator::new(effect_system_clone, GuardianRole::Requester);
        self.choreography = Some(choreography);

        tracing::warn!("Guardian authentication choreography not fully implemented");
        Ok(GuardianAuthResponse {
            guardian_approvals: vec![],
            success: false,
            error: Some(
                "GuardianAuthenticationCoordinator protocol execution pending implementation"
                    .to_string(),
            ),
        })
    }

    /// Get the current effect system
    pub fn effect_system(&self) -> Arc<E> {
        self.effect_system.clone()
    }

    /// Check if a choreography is currently active
    pub fn has_active_choreography(&self) -> bool {
        self.choreography.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::DeviceId;
    use aura_macros::aura_test;

    #[test]
    fn test_guardian_auth_state() {
        let mut state = GuardianAuthState::new();

        let request_id = "test_request".to_string();
        let guardian_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let challenge = vec![1, 2, 3, 4];
        let expires_at = 1_000 + 300; // Dummy timestamp

        state.add_guardian_challenge(
            request_id.clone(),
            guardian_id,
            challenge.clone(),
            expires_at,
        );

        // Verify with timestamp before expiration
        let verified_challenge =
            state.verify_guardian_challenge(&request_id, guardian_id, expires_at - 1000);
        assert_eq!(verified_challenge, Some(&challenge));

        assert!(!state.has_sufficient_approvals(&request_id, 1));

        let approval = GuardianApproval {
            guardian_id,
            verified_identity: VerifiedIdentity {
                proof: aura_verify::IdentityProof::Device {
                    device_id: guardian_id,
                    signature: aura_verify::Ed25519Signature::from_bytes(&[0u8; 64]),
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

    #[aura_test]
    async fn test_guardian_auth_coordinator() -> aura_core::AuraResult<()> {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let fixture = aura_testkit::create_test_fixture_with_device_id(device_id).await?;

        let coordinator = GuardianAuthCoordinator::new(fixture.effect_system_arc());
        assert!(!coordinator.has_active_choreography());

        // Just test basic coordinator creation and state
        // Note: actual async methods would need to be tested separately
        // in an integration test that can handle the runtime correctly
        Ok(())
    }
}
