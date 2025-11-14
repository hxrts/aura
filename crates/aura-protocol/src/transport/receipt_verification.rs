//! Receipt Verification Protocol
//!
//! Layer 4: Multi-party receipt coordination using choreographic protocols.
//! YES choreography - complex multi-phase verification with multiple participants.
//! Target: <250 lines, focused on choreographic coordination.

use super::{TransportCoordinationError, CoordinationResult};
use aura_core::{DeviceId, ContextId};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// Receipt verification coordinator using choreographic protocols
#[derive(Debug, Clone)]
pub struct ReceiptVerificationCoordinator {
    device_id: DeviceId,
    verification_config: VerificationConfig,
}

/// Configuration for receipt verification
#[derive(Debug, Clone)]
pub struct VerificationConfig {
    pub verification_timeout: std::time::Duration,
    pub required_confirmations: usize,
    pub max_verification_attempts: u32,
}

/// Receipt verification request data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptData {
    pub receipt_id: String,
    pub sender_id: DeviceId,
    pub recipient_id: DeviceId,
    pub message_hash: Vec<u8>,
    pub timestamp: SystemTime,
    pub context_id: ContextId,
}

/// Verification response from participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResponse {
    pub receipt_id: String,
    pub verifier_id: DeviceId,
    pub verification_result: VerificationResult,
    pub verification_proof: Vec<u8>,
    pub timestamp: SystemTime,
}

/// Verification result enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationResult {
    Valid,
    Invalid { reason: String },
    Pending,
}

/// Verification completion notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationComplete {
    pub receipt_id: String,
    pub final_result: VerificationResult,
    pub confirmations: Vec<VerificationResponse>,
    pub completion_timestamp: SystemTime,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            verification_timeout: std::time::Duration::from_secs(30),
            required_confirmations: 2,
            max_verification_attempts: 3,
        }
    }
}

impl ReceiptVerificationCoordinator {
    /// Create new receipt verification coordinator
    pub fn new(device_id: DeviceId, config: VerificationConfig) -> Self {
        Self {
            device_id,
            verification_config: config,
        }
    }
    
    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }
    
    /// Get verification configuration
    pub fn config(&self) -> &VerificationConfig {
        &self.verification_config
    }
    
    /// Validate receipt data
    pub fn validate_receipt(&self, receipt: &ReceiptData) -> CoordinationResult<()> {
        // Basic validation
        if receipt.receipt_id.is_empty() {
            return Err(TransportCoordinationError::ProtocolFailed(
                "Receipt ID cannot be empty".to_string()
            ));
        }
        
        if receipt.message_hash.is_empty() {
            return Err(TransportCoordinationError::ProtocolFailed(
                "Message hash cannot be empty".to_string()
            ));
        }
        
        if receipt.sender_id == receipt.recipient_id {
            return Err(TransportCoordinationError::ProtocolFailed(
                "Sender and recipient cannot be the same".to_string()
            ));
        }
        
        Ok(())
    }
    
    /// Create verification response
    pub fn create_verification_response(
        &self,
        receipt: &ReceiptData,
        is_valid: bool,
        proof: Vec<u8>,
    ) -> VerificationResponse {
        let verification_result = if is_valid {
            VerificationResult::Valid
        } else {
            VerificationResult::Invalid {
                reason: "Verification failed".to_string(),
            }
        };
        
        VerificationResponse {
            receipt_id: receipt.receipt_id.clone(),
            verifier_id: self.device_id,
            verification_result,
            verification_proof: proof,
            timestamp: SystemTime::now(),
        }
    }
    
    /// Aggregate verification responses
    pub fn aggregate_verifications(
        &self,
        receipt_id: &str,
        responses: Vec<VerificationResponse>,
    ) -> CoordinationResult<VerificationComplete> {
        if responses.len() < self.verification_config.required_confirmations {
            return Err(TransportCoordinationError::ProtocolFailed(
                format!(
                    "Insufficient confirmations: {} < {}",
                    responses.len(),
                    self.verification_config.required_confirmations
                )
            ));
        }
        
        // Count valid vs invalid responses
        let valid_count = responses
            .iter()
            .filter(|r| matches!(r.verification_result, VerificationResult::Valid))
            .count();
        
        let final_result = if valid_count >= self.verification_config.required_confirmations {
            VerificationResult::Valid
        } else {
            VerificationResult::Invalid {
                reason: format!("Insufficient valid confirmations: {}", valid_count),
            }
        };
        
        Ok(VerificationComplete {
            receipt_id: receipt_id.to_string(),
            final_result,
            confirmations: responses,
            completion_timestamp: SystemTime::now(),
        })
    }
}

// Choreographic Protocol Definition
// Multi-party receipt verification with coordinated responses
choreography! {
    #[namespace = "receipt_verification"]
    protocol ReceiptVerificationProtocol {
        roles: Coordinator, Verifier1, Verifier2, ReceiptSender;
        
        // Phase 1: Coordinator initiates verification
        Coordinator[guard_capability = "coordinate_verification",
                   flow_cost = 100,
                   journal_facts = "verification_initiated"]
        -> Verifier1: VerifyReceiptRequest(ReceiptData);
        
        Coordinator[guard_capability = "coordinate_verification",
                   flow_cost = 100]
        -> Verifier2: VerifyReceiptRequest(ReceiptData);
        
        // Phase 2: Verifiers respond with verification results
        Verifier1[guard_capability = "verify_receipt",
                  flow_cost = 50,
                  journal_facts = "verification_completed"]
        -> Coordinator: VerificationResponse(VerificationResponse);
        
        Verifier2[guard_capability = "verify_receipt",
                  flow_cost = 50,
                  journal_facts = "verification_completed"]
        -> Coordinator: VerificationResponse(VerificationResponse);
        
        // Phase 3: Coordinator aggregates and notifies completion
        Coordinator[guard_capability = "finalize_verification",
                   flow_cost = 75,
                   journal_facts = "verification_finalized"]
        -> ReceiptSender: VerificationComplete(VerificationComplete);
        
        Coordinator[guard_capability = "finalize_verification",
                   flow_cost = 50]
        -> Verifier1: VerificationComplete(VerificationComplete);
        
        Coordinator[guard_capability = "finalize_verification",
                   flow_cost = 50]
        -> Verifier2: VerificationComplete(VerificationComplete);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_receipt_validation() {
        let coordinator = ReceiptVerificationCoordinator::new(
            DeviceId::from([1u8; 32]),
            VerificationConfig::default(),
        );
        
        let valid_receipt = ReceiptData {
            receipt_id: "test-receipt".to_string(),
            sender_id: DeviceId::from([1u8; 32]),
            recipient_id: DeviceId::from([2u8; 32]),
            message_hash: vec![1, 2, 3, 4],
            timestamp: SystemTime::now(),
            context_id: ContextId::new("test"),
        };
        
        assert!(coordinator.validate_receipt(&valid_receipt).is_ok());
    }
    
    #[test]
    fn test_verification_aggregation() {
        let coordinator = ReceiptVerificationCoordinator::new(
            DeviceId::from([1u8; 32]),
            VerificationConfig::default(),
        );
        
        let responses = vec![
            VerificationResponse {
                receipt_id: "test-receipt".to_string(),
                verifier_id: DeviceId::from([2u8; 32]),
                verification_result: VerificationResult::Valid,
                verification_proof: vec![1, 2, 3],
                timestamp: SystemTime::now(),
            },
            VerificationResponse {
                receipt_id: "test-receipt".to_string(),
                verifier_id: DeviceId::from([3u8; 32]),
                verification_result: VerificationResult::Valid,
                verification_proof: vec![4, 5, 6],
                timestamp: SystemTime::now(),
            },
        ];
        
        let result = coordinator.aggregate_verifications("test-receipt", responses);
        assert!(result.is_ok());
        
        let completion = result.unwrap();
        assert!(matches!(completion.final_result, VerificationResult::Valid));
        assert_eq!(completion.confirmations.len(), 2);
    }
}
