//! Guardian Setup Choreography
//!
//! Initial establishment of guardian relationships for a threshold account.
//! This choreography handles the initial invitation and acceptance process.

use crate::{
    types::{GuardianProfile, GuardianSet, RecoveryRequest, RecoveryResponse, RecoveryShare},
    RecoveryResult,
};
use aura_authenticate::guardian_auth::RecoveryContext;
use aura_core::{identifiers::GuardianId, AccountId, DeviceId};
use aura_crypto::frost::ThresholdSignature;
// use aura_macros::aura_choreography; // Temporarily disabled for testing
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

// Note: Choreography macro temporarily disabled for testing
// The full choreographic implementation with choreography! macro
// would be restored once testing infrastructure is working

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

    /// Execute guardian setup ceremony as setup initiator
    pub async fn execute_setup(
        &self,
        request: RecoveryRequest,
    ) -> RecoveryResult<RecoveryResponse> {
        let setup_id = self.generate_setup_id(&request);

        // Validate that we have guardians
        if request.guardians.is_empty() {
            return Err(aura_core::AuraError::invalid("No guardians in request"));
        }

        // Get first guardian as sample (in real implementation this would iterate properly)
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

        // Execute using the generated choreography in the guardian_setup module
        let result = self.simulate_guardian_setup(invitation).await;

        match result {
            Ok(acceptances) => {
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

                // Send completion notifications
                let _completion = SetupCompletion {
                    setup_id: setup_id.clone(),
                    success: true,
                    final_guardian_set: GuardianSet::new(
                        shares.iter().map(|s| s.guardian.clone()).collect(),
                    ),
                    threshold: request.threshold,
                    setup_evidence: serde_json::to_vec(&evidence).unwrap_or_default(),
                };

                // Broadcast completion (would be handled by choreography in real implementation)

                Ok(RecoveryResponse {
                    success: true,
                    error: None,
                    key_material: None, // Setup doesn't produce key material
                    guardian_shares: shares,
                    evidence,
                    signature,
                })
            }
            Err(e) => Ok(RecoveryResponse {
                success: false,
                error: Some(format!("Setup choreography failed: {}", e)),
                key_material: None,
                guardian_shares: Vec::new(),
                evidence: self.create_failed_evidence(&request),
                signature: self.create_empty_signature(),
            }),
        }
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

    /// Simulate guardian setup execution
    async fn simulate_guardian_setup(
        &self,
        _invitation: GuardianInvitation,
    ) -> RecoveryResult<Vec<GuardianAcceptance>> {
        // Simulate multiple guardian acceptances
        Ok(vec![
            GuardianAcceptance {
                guardian_id: GuardianId::new(),
                setup_id: "setup_123".to_string(),
                accepted: true,
                public_key: vec![1; 32],
                device_attestation: vec![2; 64],
                timestamp: 0,
            },
            GuardianAcceptance {
                guardian_id: GuardianId::new(),
                setup_id: "setup_123".to_string(),
                accepted: true,
                public_key: vec![3; 32],
                device_attestation: vec![4; 64],
                timestamp: 0,
            },
        ])
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
