//! Guardian Setup Choreography
//!
//! Initial establishment of guardian relationships for a threshold account.
//! This choreography handles the initial invitation and acceptance process.

use crate::{
    types::{GuardianProfile, GuardianSet, RecoveryRequest, RecoveryResponse, RecoveryShare},
    RecoveryResult,
};
use aura_authenticate::guardian_auth::RecoveryContext;
use aura_core::effects::TimeEffects;
use aura_core::{identifiers::GuardianId, AccountId, DeviceId};
use aura_crypto::frost::ThresholdSignature;
use aura_macros::choreography;
use aura_protocol::AuraEffectSystem;
use serde::{Deserialize, Serialize};

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
    pub timestamp: u64,
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
                       flow_cost = 300,
                       journal_facts = "guardian_setup_initiated",
                       leakage_budget = [1, 0, 0]]
        -> Guardian1: SendInvitation(GuardianInvitation);

        SetupInitiator[guard_capability = "initiate_guardian_setup",
                       flow_cost = 300]
        -> Guardian2: SendInvitation(GuardianInvitation);

        SetupInitiator[guard_capability = "initiate_guardian_setup",
                       flow_cost = 300]
        -> Guardian3: SendInvitation(GuardianInvitation);

        // Phase 2: Guardian acceptances back to setup initiator
        Guardian1[guard_capability = "accept_guardian_invitation,verify_setup_context",
                   flow_cost = 200,
                   journal_facts = "guardian_setup_accepted",
                   leakage_budget = [0, 1, 0]]
        -> SetupInitiator: AcceptInvitation(GuardianAcceptance);

        Guardian2[guard_capability = "accept_guardian_invitation,verify_setup_context",
                   flow_cost = 200,
                   journal_facts = "guardian_setup_accepted"]
        -> SetupInitiator: AcceptInvitation(GuardianAcceptance);

        Guardian3[guard_capability = "accept_guardian_invitation,verify_setup_context",
                   flow_cost = 200,
                   journal_facts = "guardian_setup_accepted"]
        -> SetupInitiator: AcceptInvitation(GuardianAcceptance);

        // Phase 3: Setup completion broadcast
        SetupInitiator[guard_capability = "complete_guardian_setup",
                       flow_cost = 150,
                       journal_facts = "guardian_setup_completed",
                       journal_merge = true]
        -> Guardian1: CompleteSetup(SetupCompletion);

        SetupInitiator[guard_capability = "complete_guardian_setup",
                       flow_cost = 150,
                       journal_merge = true]
        -> Guardian2: CompleteSetup(SetupCompletion);

        SetupInitiator[guard_capability = "complete_guardian_setup",
                       flow_cost = 150,
                       journal_merge = true]
        -> Guardian3: CompleteSetup(SetupCompletion);
    }
}

/// Guardian setup coordinator
pub struct GuardianSetupCoordinator {
    _effect_system: AuraEffectSystem,
}

impl GuardianSetupCoordinator {
    /// Create new coordinator
    pub fn new(effect_system: AuraEffectSystem) -> Self {
        Self {
            _effect_system: effect_system,
        }
    }

    /// Execute guardian setup ceremony as setup initiator using choreography
    pub async fn execute_setup(
        &self,
        request: RecoveryRequest,
    ) -> RecoveryResult<RecoveryResponse> {
        let setup_id = self.generate_setup_id(&request);

        // Validate that we have guardians
        if request.guardians.is_empty() {
            return Err(aura_core::AuraError::invalid("No guardians in request"));
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
            return Ok(RecoveryResponse {
                success: false,
                error: Some(format!(
                    "Insufficient guardian acceptances: got {}, need {}",
                    acceptances.len(),
                    request.threshold
                )),
                key_material: None,
                guardian_shares: Vec::new(),
                evidence: self.create_failed_evidence(&request),
                signature: self.create_empty_signature(),
            });
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
                    issued_at: acceptance.timestamp,
                }
            })
            .collect::<Vec<_>>();

        // Create evidence and signature
        let evidence = self.create_evidence(&request, &shares);
        let signature = self.aggregate_signature(&shares);

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

        Ok(RecoveryResponse {
            success: true,
            error: None,
            key_material: None, // Setup doesn't produce key material
            guardian_shares: shares,
            evidence,
            signature,
        })
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
            timestamp: 0,                    // Placeholder timestamp
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
        let timestamp = self.current_timestamp().await;
        let acceptances = vec![
            GuardianAcceptance {
                guardian_id: GuardianId::new(),
                setup_id: invitation.setup_id.clone(),
                accepted: true,
                public_key: vec![1; 32],
                device_attestation: vec![2; 64],
                timestamp,
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

    /// Get current timestamp
    async fn current_timestamp(&self) -> u64 {
        self._effect_system.current_timestamp().await
    }

    /// Generate unique setup ID
    fn generate_setup_id(&self, request: &RecoveryRequest) -> String {
        format!("setup_{}_{}", request.account_id, request.requesting_device)
    }

    fn aggregate_signature(&self, shares: &[RecoveryShare]) -> ThresholdSignature {
        let mut combined_signature = Vec::new();
        for share in shares {
            combined_signature.extend_from_slice(&share.partial_signature);
        }

        let signature_bytes = if combined_signature.len() >= 64 {
            combined_signature[..64].to_vec()
        } else {
            let mut padded = combined_signature;
            padded.resize(64, 0);
            padded
        };

        let signers: Vec<u16> = shares
            .iter()
            .enumerate()
            .map(|(idx, _)| idx as u16)
            .collect();

        ThresholdSignature::new(signature_bytes, signers)
    }

    fn create_evidence(
        &self,
        _request: &RecoveryRequest,
        _shares: &[RecoveryShare],
    ) -> crate::types::RecoveryEvidence {
        // Placeholder implementation
        crate::types::RecoveryEvidence {
            account_id: AccountId::new(),
            recovering_device: DeviceId::new(),
            guardians: Vec::new(),
            issued_at: 0,
            cooldown_expires_at: 0,
            dispute_window_ends_at: 0,
            guardian_profiles: Vec::new(),
            disputes: Vec::new(),
            threshold_signature: None,
        }
    }

    fn create_failed_evidence(&self, _request: &RecoveryRequest) -> crate::types::RecoveryEvidence {
        // Placeholder implementation
        crate::types::RecoveryEvidence {
            account_id: AccountId::new(),
            recovering_device: DeviceId::new(),
            guardians: Vec::new(),
            issued_at: 0,
            cooldown_expires_at: 0,
            dispute_window_ends_at: 0,
            guardian_profiles: Vec::new(),
            disputes: Vec::new(),
            threshold_signature: None,
        }
    }

    fn create_empty_signature(&self) -> ThresholdSignature {
        ThresholdSignature::new(vec![0; 64], vec![])
    }
}
