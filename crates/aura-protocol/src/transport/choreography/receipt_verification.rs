//! Receipt Verification Choreographic Protocols
//!
//! Layer 4: Multi-party receipt verification using choreographic protocols.
//! YES choreography - complex verification workflow with anti-replay protection.
//! Target: <250 lines, focused on choreographic verification patterns.

use super::{ChoreographicConfig, ChoreographicError, ChoreographicResult};
use aura_core::{DeviceId, ContextId};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// Receipt coordination protocol using choreographic patterns
#[derive(Debug, Clone)]
pub struct ReceiptCoordinationProtocol {
    device_id: DeviceId,
    config: ChoreographicConfig,
    active_verifications: HashMap<String, VerificationWorkflow>,
    replay_prevention: HashMap<Vec<u8>, SystemTime>, // Hash -> Timestamp
}

/// Verification workflow state tracking
#[derive(Debug, Clone)]
struct VerificationWorkflow {
    verification_id: String,
    receipt_hash: Vec<u8>,
    participants: Vec<DeviceId>,
    phase: VerificationPhase,
    started_at: SystemTime,
    verifications: HashMap<DeviceId, IndividualVerification>,
}

/// Verification phase enumeration
#[derive(Debug, Clone, PartialEq, Eq)]
enum VerificationPhase {
    Initiated,
    GatheringVerifications,
    ConsensusBuilding,
    AntiReplayCheck,
    Finalization,
    Completed,
    Failed(String),
}

/// Individual verification from participant
#[derive(Debug, Clone)]
struct IndividualVerification {
    verifier_id: DeviceId,
    verification_result: bool,
    verification_proof: Vec<u8>,
    timestamp: SystemTime,
    anti_replay_token: Vec<u8>,
}

/// Receipt verification initiation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptVerificationInit {
    pub verification_id: String,
    pub coordinator_id: DeviceId,
    pub receipt_data: ReceiptData,
    pub verifiers: Vec<DeviceId>,
    pub verification_deadline: SystemTime,
    pub anti_replay_nonce: Vec<u8>,
}

/// Receipt data to be verified
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptData {
    pub receipt_id: String,
    pub sender_id: DeviceId,
    pub recipient_id: DeviceId,
    pub message_hash: Vec<u8>,
    pub signature: Vec<u8>,
    pub timestamp: SystemTime,
    pub context_id: ContextId,
}

/// Individual verification response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptVerificationResponse {
    pub verification_id: String,
    pub verifier_id: DeviceId,
    pub verification_result: VerificationOutcome,
    pub verification_proof: Vec<u8>,
    pub anti_replay_token: Vec<u8>,
    pub timestamp: SystemTime,
}

/// Anti-replay protection challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiReplayChallenge {
    pub verification_id: String,
    pub challenger_id: DeviceId,
    pub challenge_nonce: Vec<u8>,
    pub participants: Vec<DeviceId>,
    pub challenge_deadline: SystemTime,
}

/// Anti-replay protection response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiReplayResponse {
    pub verification_id: String,
    pub responder_id: DeviceId,
    pub challenge_response: Vec<u8>,
    pub replay_check_result: ReplayCheckResult,
    pub timestamp: SystemTime,
}

/// Final verification consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationConsensus {
    pub verification_id: String,
    pub coordinator_id: DeviceId,
    pub consensus_result: ConsensusResult,
    pub participating_verifiers: Vec<DeviceId>,
    pub finalization_timestamp: SystemTime,
    pub anti_replay_confirmation: bool,
}

/// Verification outcome enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationOutcome {
    Valid { confidence: u8 },
    Invalid { reason: String },
    Inconclusive { reason: String },
    ReplayDetected { original_timestamp: SystemTime },
}

/// Replay check result enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplayCheckResult {
    NoReplay,
    PotentialReplay { suspicion_level: u8 },
    ConfirmedReplay { evidence: Vec<u8> },
}

/// Consensus result enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusResult {
    Valid { confirmation_count: usize },
    Invalid { rejection_count: usize },
    Split { valid_count: usize, invalid_count: usize },
    InsufficientParticipation,
}

impl ReceiptCoordinationProtocol {
    /// Create new receipt coordination protocol
    pub fn new(device_id: DeviceId, config: ChoreographicConfig) -> Self {
        Self {
            device_id,
            config,
            active_verifications: HashMap::new(),
            replay_prevention: HashMap::new(),
        }
    }
    
    /// Initiate receipt verification workflow
    pub fn initiate_verification(
        &mut self,
        receipt_data: ReceiptData,
        verifiers: Vec<DeviceId>,
    ) -> ChoreographicResult<String> {
        // Check for potential replay
        if self.replay_prevention.contains_key(&receipt_data.message_hash) {
            return Err(ChoreographicError::ExecutionFailed(
                "Potential replay attack detected".to_string()
            ));
        }
        
        let verification_id = format!(
            "verification-{}-{}",
            self.device_id.to_hex()[..8].to_string(),
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default().as_millis()
        );
        
        let workflow = VerificationWorkflow {
            verification_id: verification_id.clone(),
            receipt_hash: receipt_data.message_hash.clone(),
            participants: verifiers,
            phase: VerificationPhase::Initiated,
            started_at: SystemTime::now(),
            verifications: HashMap::new(),
        };
        
        // Record receipt hash for replay prevention
        self.replay_prevention.insert(
            receipt_data.message_hash.clone(),
            SystemTime::now()
        );
        
        self.active_verifications.insert(verification_id.clone(), workflow);
        Ok(verification_id)
    }
    
    /// Process verification response
    pub fn process_verification_response(
        &mut self,
        response: ReceiptVerificationResponse,
    ) -> ChoreographicResult<bool> {
        let workflow = self.active_verifications.get_mut(&response.verification_id)
            .ok_or_else(|| ChoreographicError::ExecutionFailed(
                format!("Verification not found: {}", response.verification_id)
            ))?;
        
        let verification = IndividualVerification {
            verifier_id: response.verifier_id,
            verification_result: matches!(response.verification_result, VerificationOutcome::Valid { .. }),
            verification_proof: response.verification_proof,
            timestamp: response.timestamp,
            anti_replay_token: response.anti_replay_token,
        };
        
        workflow.verifications.insert(response.verifier_id, verification);
        
        // Check if we have enough verifications
        let sufficient_verifications = workflow.verifications.len() >= 
            (workflow.participants.len() * 2) / 3; // 2/3 majority
        
        if sufficient_verifications {
            workflow.phase = VerificationPhase::ConsensusBuilding;
        }
        
        Ok(sufficient_verifications)
    }
    
    /// Build consensus from verifications
    pub fn build_consensus(
        &mut self,
        verification_id: &str,
    ) -> ChoreographicResult<ConsensusResult> {
        let workflow = self.active_verifications.get_mut(verification_id)
            .ok_or_else(|| ChoreographicError::ExecutionFailed(
                format!("Verification not found: {}", verification_id)
            ))?;
        
        let valid_count = workflow.verifications.values()
            .filter(|v| v.verification_result)
            .count();
        
        let invalid_count = workflow.verifications.len() - valid_count;
        
        let consensus = if valid_count > invalid_count {
            workflow.phase = VerificationPhase::Completed;
            ConsensusResult::Valid { confirmation_count: valid_count }
        } else if invalid_count > valid_count {
            workflow.phase = VerificationPhase::Failed("Majority rejection".to_string());
            ConsensusResult::Invalid { rejection_count: invalid_count }
        } else {
            workflow.phase = VerificationPhase::Failed("Split decision".to_string());
            ConsensusResult::Split { valid_count, invalid_count }
        };
        
        Ok(consensus)
    }
    
    /// Clean up old replay prevention entries
    pub fn cleanup_replay_prevention(&mut self, max_age: std::time::Duration) {
        let cutoff = SystemTime::now() - max_age;
        self.replay_prevention.retain(|_, timestamp| *timestamp > cutoff);
    }
    
    /// Get verification status
    pub fn get_verification_status(&self, verification_id: &str) -> Option<&VerificationPhase> {
        self.active_verifications.get(verification_id).map(|w| &w.phase)
    }
}

// Choreographic Protocol Definition
// Multi-party receipt verification with anti-replay protection
choreography! {
    #[namespace = "receipt_verification_coordination"]
    protocol ReceiptVerificationCoordination {
        roles: Coordinator, Verifier1, Verifier2, AntiReplayValidator;
        
        // Phase 1: Initiate verification workflow
        Coordinator[guard_capability = "initiate_receipt_verification",
                   flow_cost = 200,
                   journal_facts = "receipt_verification_initiated"]
        -> Verifier1: ReceiptVerificationInit(ReceiptVerificationInit);
        
        Coordinator[guard_capability = "initiate_receipt_verification",
                   flow_cost = 200]
        -> Verifier2: ReceiptVerificationInit(ReceiptVerificationInit);
        
        // Phase 2: Verifiers perform individual verification
        Verifier1[guard_capability = "perform_receipt_verification",
                  flow_cost = 150,
                  journal_facts = "individual_verification_completed"]
        -> Coordinator: ReceiptVerificationResponse(ReceiptVerificationResponse);
        
        Verifier2[guard_capability = "perform_receipt_verification",
                  flow_cost = 150,
                  journal_facts = "individual_verification_completed"]
        -> Coordinator: ReceiptVerificationResponse(ReceiptVerificationResponse);
        
        // Phase 3: Anti-replay protection challenge
        Coordinator[guard_capability = "initiate_anti_replay_check",
                   flow_cost = 120,
                   journal_facts = "anti_replay_check_initiated"]
        -> AntiReplayValidator: AntiReplayChallenge(AntiReplayChallenge);
        
        AntiReplayValidator[guard_capability = "validate_anti_replay",
                           flow_cost = 100,
                           journal_facts = "anti_replay_validation_completed"]
        -> Coordinator: AntiReplayResponse(AntiReplayResponse);
        
        // Phase 4: Final consensus distribution
        Coordinator[guard_capability = "distribute_verification_consensus",
                   flow_cost = 100,
                   journal_facts = "verification_consensus_finalized"]
        -> Verifier1: VerificationConsensus(VerificationConsensus);
        
        Coordinator[guard_capability = "distribute_verification_consensus",
                   flow_cost = 100,
                   journal_facts = "verification_consensus_finalized"]
        -> Verifier2: VerificationConsensus(VerificationConsensus);
    }
}
