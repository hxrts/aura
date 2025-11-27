//! Guardian Setup Choreography
//!
//! Initial establishment of guardian relationships for a threshold account.
//! This choreography handles the initial invitation and acceptance process.

use crate::{
    coordinator::{BaseCoordinator, BaseCoordinatorAccess, RecoveryCoordinator},
    types::{GuardianProfile, GuardianSet, RecoveryRequest, RecoveryResponse, RecoveryShare},
    RecoveryResult,
};
use async_trait::async_trait;
use aura_authenticate::guardian_auth::RecoveryContext;
use aura_core::scope::ContextOp;
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::{identifiers::GuardianId, AccountId, DeviceId};
use aura_macros::choreography;
use aura_protocol::effects::AuraEffects;
use aura_protocol::guards::BiscuitGuardEvaluator;
use aura_wot::BiscuitTokenManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Guardian setup invitation data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianInvitation {
    /// Unique identifier for this setup ceremony
    pub setup_id: String,
    /// Account being set up for guardian protection
    pub account_id: AccountId,
    /// Device initiating the setup invitation
    pub inviting_device: DeviceId,
    /// Guardian profile being invited
    pub guardian_role: GuardianProfile,
    /// Required threshold of guardian approvals
    pub threshold: usize,
    /// Total number of guardians in the set
    pub total_guardians: usize,
    /// Recovery context and justification
    pub context: RecoveryContext,
}

/// Guardian acceptance response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianAcceptance {
    /// Guardian identifier for the accepting party
    pub guardian_id: GuardianId,
    /// Unique identifier for the setup ceremony
    pub setup_id: String,
    /// Whether the guardian accepted the invitation
    pub accepted: bool,
    /// Guardian's public key for verification
    pub public_key: Vec<u8>,
    /// Cryptographic attestation from the guardian's device
    pub device_attestation: Vec<u8>,
    /// Timestamp when acceptance was generated
    pub timestamp: TimeStamp,
}

/// Setup completion notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupCompletion {
    /// Unique identifier for the setup ceremony
    pub setup_id: String,
    /// Whether the setup was successful
    pub success: bool,
    /// Final guardian set after successful setup
    pub final_guardian_set: GuardianSet,
    /// Required threshold for guardian operations
    pub threshold: usize,
    /// Serialized evidence of the setup completion
    pub setup_evidence: Vec<u8>,
}

// Guardian Setup Choreography
// 3-phase protocol: Invitation -> Acceptance -> Completion

// Guardian Setup Choreography - 3 phase protocol
choreography! {
    #[namespace = "guardian_setup"]
    protocol GuardianSetup {
        roles: SetupInitiator, Guardian1, Guardian2, Guardian3;

        // Phase 1: Setup invitation to all guardians
        SetupInitiator[guard_capability = "initiate_guardian_setup",
                       journal_facts = "guardian_setup_initiated",
                       leakage_budget = [1, 0, 0]]
        -> Guardian1: SendInvitation(GuardianInvitation);

        SetupInitiator[guard_capability = "initiate_guardian_setup",
                       journal_facts = "guardian_setup_initiated",
                       leakage_budget = [1, 0, 0]]
        -> Guardian2: SendInvitation(GuardianInvitation);

        SetupInitiator[guard_capability = "initiate_guardian_setup",
                       journal_facts = "guardian_setup_initiated",
                       leakage_budget = [1, 0, 0]]
        -> Guardian3: SendInvitation(GuardianInvitation);

        // Phase 2: Guardian acceptances back to setup initiator
        Guardian1[guard_capability = "accept_guardian_invitation,verify_setup_context",
                   journal_facts = "guardian_setup_accepted",
                   leakage_budget = [0, 1, 0]]
        -> SetupInitiator: AcceptInvitation(GuardianAcceptance);

        Guardian2[guard_capability = "accept_guardian_invitation,verify_setup_context",
                   journal_facts = "guardian_setup_accepted"]
        -> SetupInitiator: AcceptInvitation(GuardianAcceptance);

        Guardian3[guard_capability = "accept_guardian_invitation,verify_setup_context",
                   journal_facts = "guardian_setup_accepted"]
        -> SetupInitiator: AcceptInvitation(GuardianAcceptance);

        // Phase 3: Setup completion broadcast
        SetupInitiator[guard_capability = "complete_guardian_setup",
                       journal_facts = "guardian_setup_completed",
                       journal_merge = true]
        -> Guardian1: CompleteSetup(SetupCompletion);

        SetupInitiator[guard_capability = "complete_guardian_setup",
                       journal_merge = true]
        -> Guardian2: CompleteSetup(SetupCompletion);

        SetupInitiator[guard_capability = "complete_guardian_setup",
                       journal_merge = true]
        -> Guardian3: CompleteSetup(SetupCompletion);
    }
}

/// Guardian setup coordinator
pub struct GuardianSetupCoordinator<E>
where
    E: AuraEffects + 'static,
{
    base: BaseCoordinator<E>,
}

impl<E> GuardianSetupCoordinator<E>
where
    E: AuraEffects + 'static,
{
    /// Create new coordinator
    pub fn new(effect_system: Arc<E>) -> Self {
        Self {
            base: BaseCoordinator::new(effect_system),
        }
    }

    /// Create new coordinator with Biscuit authorization
    pub fn new_with_biscuit(
        effect_system: Arc<E>,
        token_manager: BiscuitTokenManager,
        guard_evaluator: BiscuitGuardEvaluator,
    ) -> Self {
        Self {
            base: BaseCoordinator::new_with_biscuit(effect_system, token_manager, guard_evaluator),
        }
    }
}

impl<E> BaseCoordinatorAccess<E> for GuardianSetupCoordinator<E>
where
    E: AuraEffects + 'static,
{
    fn base(&self) -> &BaseCoordinator<E> {
        &self.base
    }
}

#[async_trait]
impl<E> RecoveryCoordinator<E> for GuardianSetupCoordinator<E>
where
    E: AuraEffects + 'static,
{
    type Request = RecoveryRequest;
    type Response = RecoveryResponse;

    fn effect_system(&self) -> &Arc<E> {
        self.base_effect_system()
    }

    fn token_manager(&self) -> Option<&BiscuitTokenManager> {
        self.base_token_manager()
    }

    fn guard_evaluator(&self) -> Option<&BiscuitGuardEvaluator> {
        self.base_guard_evaluator()
    }

    fn operation_name(&self) -> &str {
        "guardian_setup"
    }

    async fn execute_recovery(&self, request: Self::Request) -> RecoveryResult<Self::Response> {
        self.execute_setup(request).await
    }
}

impl<E> GuardianSetupCoordinator<E>
where
    E: AuraEffects + 'static,
{
    /// Execute guardian setup ceremony as setup initiator using choreography
    pub async fn execute_setup(
        &self,
        request: RecoveryRequest,
    ) -> RecoveryResult<RecoveryResponse> {
        // Check authorization using the common helper
        if let Err(auth_error) = self
            .check_authorization(&request.account_id, ContextOp::UpdateGuardianSet)
            .await
        {
            return Ok(self.base.create_error_response(
                format!("Authorization failed: {}", auth_error),
                request.account_id,
                request.requesting_device,
            ));
        }

        let setup_id = self.generate_operation_id(&request.account_id, &request.requesting_device);

        // Validate that we have guardians
        if request.guardians.is_empty() {
            return Ok(self.base.create_error_response(
                "No guardians in request".to_string(),
                request.account_id,
                request.requesting_device,
            ));
        }

        // Get first guardian as sample for choreography structure
        let sample_guardian = request
            .guardians
            .iter()
            .next()
            .ok_or_else(|| aura_core::AuraError::invalid("No guardians available"))?;

        // Convert generic request to choreography-specific invitation
        let invitation = GuardianInvitation {
            setup_id: setup_id.clone(),
            account_id: request.account_id,
            inviting_device: request.requesting_device,
            guardian_role: sample_guardian.clone(),
            threshold: request.threshold,
            total_guardians: request.guardians.len(),
            context: request.context.clone(),
        };

        // Execute the choreographic protocol
        // Note: In full implementation, this would use the generated choreography runtime
        // For now, we maintain the simulation structure but with choreographic intent
        let acceptances = self.execute_choreographic_setup(invitation).await?;

        // Check if we have enough acceptances
        if acceptances.len() < request.threshold {
            return Ok(self.base.create_error_response(
                format!(
                    "Insufficient guardian acceptances: got {}, need {}",
                    acceptances.len(),
                    request.threshold
                ),
                request.account_id,
                request.requesting_device,
            ));
        }

        // Create guardian shares from acceptances
        let shares = acceptances
            .into_iter()
            .map(|acceptance| {
                RecoveryShare {
                    guardian: GuardianProfile {
                        guardian_id: acceptance.guardian_id,
                        device_id: DeviceId::new(), // Would use actual device ID
                        label: "Guardian".to_string(),
                        trust_level: aura_core::TrustLevel::High,
                        cooldown_secs: 900,
                    },
                    share: acceptance.public_key,
                    partial_signature: acceptance.device_attestation,
                    issued_at: acceptance.timestamp.to_index_ms() as u64,
                }
            })
            .collect::<Vec<_>>();

        // Create evidence using the common utility
        let evidence =
            self.create_success_evidence(request.account_id, request.requesting_device, &shares);

        // Create completion message for final phase
        let completion = SetupCompletion {
            setup_id: setup_id.clone(),
            success: true,
            final_guardian_set: GuardianSet::new(
                shares.iter().map(|s| s.guardian.clone()).collect(),
            ),
            threshold: request.threshold,
            setup_evidence: serde_json::to_vec(&evidence).unwrap_or_default(),
        };

        // Phase 3 would broadcast completion through choreography
        self.broadcast_completion(completion).await?;

        // Use the common response builder
        Ok(self.base.create_success_response(
            None, // Setup doesn't produce key material
            shares, evidence,
        ))
    }

    /// Execute as guardian (accept setup invitation)
    pub async fn accept_as_guardian(
        &self,
        invitation: GuardianInvitation,
    ) -> RecoveryResult<GuardianAcceptance> {
        // For now, simulate guardian acceptance
        // In real implementation, this would run the guardian side of the choreography

        Ok(GuardianAcceptance {
            guardian_id: GuardianId::new(), // Would be actual guardian ID
            setup_id: invitation.setup_id,
            accepted: true,
            public_key: vec![1; 32],         // Placeholder public key
            device_attestation: vec![2; 64], // Placeholder attestation
            timestamp: TimeStamp::PhysicalClock(
                self.effect_system()
                    .physical_time()
                    .await
                    .unwrap_or(PhysicalTime {
                        ts_ms: 0,
                        uncertainty: None,
                    }),
            ),
        })
    }

    /// Execute choreographic setup protocol (Phase 1-2)
    async fn execute_choreographic_setup(
        &self,
        invitation: GuardianInvitation,
    ) -> RecoveryResult<Vec<GuardianAcceptance>> {
        // Phase 1: Send invitations to all guardians (choreographic send operations)
        // This would be handled by the generated choreography runtime

        // Phase 2: Collect guardian acceptances (choreographic receive operations)
        // For now, simulate the expected responses that would come through choreography
        let physical_time = self
            .effect_system()
            .physical_time()
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("Time error: {}", e)))?;
        let timestamp = TimeStamp::PhysicalClock(physical_time);
        let acceptances = vec![
            GuardianAcceptance {
                guardian_id: GuardianId::new(),
                setup_id: invitation.setup_id.clone(),
                accepted: true,
                public_key: vec![1; 32],
                device_attestation: vec![2; 64],
                timestamp: timestamp.clone(),
            },
            GuardianAcceptance {
                guardian_id: GuardianId::new(),
                setup_id: invitation.setup_id.clone(),
                accepted: true,
                public_key: vec![3; 32],
                device_attestation: vec![4; 64],
                timestamp,
            },
        ];

        Ok(acceptances)
    }

    /// Broadcast setup completion (Phase 3)
    async fn broadcast_completion(&self, _completion: SetupCompletion) -> RecoveryResult<()> {
        // This would be handled by the choreographic broadcast in the generated code
        // The choreography runtime would send completion messages to all guardians
        Ok(())
    }
}
