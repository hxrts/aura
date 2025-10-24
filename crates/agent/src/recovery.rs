// Account recovery protocol with guardian approval and cooldown
//
// Implements the recovery flow as specified in docs/050_recovery_and_policy.md:
// 1. User initiates recovery request
// 2. Guardians approve by providing their shares
// 3. Cooldown period enforced (24-48 hours, configurable)
// 4. User can cancel during cooldown
// 5. After cooldown, shares reconstructed and new device added
// 6. Session epoch bumped to invalidate old tickets

use crate::{AgentError, Result};
use aura_journal::{AccountId, DeviceId, GuardianId, SessionEpoch};
use aura_coordination::KeyShare;
#[allow(unused_imports)] // Reserved for future recovery implementation
use aura_coordination::ParticipantId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[allow(unused_imports)] // Reserved for future recovery implementation
use std::collections::HashSet;

/// Recovery request initiated by a user who lost access
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRequest {
    /// Unique recovery request ID
    pub request_id: uuid::Uuid,
    /// Account being recovered
    pub account_id: AccountId,
    /// New device requesting access
    pub new_device_id: DeviceId,
    /// Guardians being contacted for approval
    pub guardian_ids: Vec<GuardianId>,
    /// Required number of guardian approvals
    pub required_approvals: usize,
    /// Cooldown duration in seconds (default: 86400 = 24 hours)
    pub cooldown_seconds: u64,
    /// Timestamp when recovery was initiated
    pub initiated_at: u64,
    /// Timestamp when cooldown completes
    pub cooldown_completes_at: u64,
    /// Current status
    pub status: RecoveryStatus,
    /// Optional reason/context for recovery
    pub reason: Option<String>,
}

impl RecoveryRequest {
    /// Create a new recovery request
    ///
    /// # Arguments
    ///
    /// * `account_id` - Account to recover
    /// * `new_device_id` - Device requesting recovery
    /// * `guardian_ids` - Guardians to contact
    /// * `required_approvals` - Minimum approvals needed (typically 2 for 2-of-3)
    /// * `cooldown_seconds` - Cooldown duration (default: 24 hours)
    /// * `reason` - Optional reason for recovery
    pub fn new(
        account_id: AccountId,
        new_device_id: DeviceId,
        guardian_ids: Vec<GuardianId>,
        required_approvals: usize,
        cooldown_seconds: Option<u64>,
        reason: Option<String>,
        effects: &aura_crypto::Effects,
    ) -> Result<Self> {
        let now = effects.now().unwrap_or(0);
        let cooldown = cooldown_seconds.unwrap_or(86400); // 24 hours default
        
        Ok(RecoveryRequest {
            request_id: effects.gen_uuid(),
            account_id,
            new_device_id,
            guardian_ids,
            required_approvals,
            cooldown_seconds: cooldown,
            initiated_at: now,
            cooldown_completes_at: now + cooldown,
            status: RecoveryStatus::PendingApprovals { approvals: vec![] },
            reason,
        })
    }
    
    /// Check if cooldown period has elapsed
    pub fn cooldown_elapsed(&self, effects: &aura_crypto::Effects) -> Result<bool> {
        Ok(effects.now().unwrap_or(0) >= self.cooldown_completes_at)
    }
    
    /// Get remaining cooldown time in seconds
    pub fn remaining_cooldown(&self, effects: &aura_crypto::Effects) -> Result<u64> {
        let now = effects.now().unwrap_or(0);
        if now >= self.cooldown_completes_at {
            Ok(0)
        } else {
            Ok(self.cooldown_completes_at - now)
        }
    }
    
    /// Check if recovery can proceed (enough approvals + cooldown elapsed)
    pub fn can_proceed(&self, effects: &aura_crypto::Effects) -> bool {
        match &self.status {
            RecoveryStatus::CooldownActive { approvals } => {
                approvals.len() >= self.required_approvals && self.cooldown_elapsed(effects).unwrap_or(false)
            }
            RecoveryStatus::ReadyToExecute { approvals } => {
                approvals.len() >= self.required_approvals
            }
            _ => false,
        }
    }
}

/// Recovery request status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RecoveryStatus {
    /// Waiting for guardian approvals
    PendingApprovals {
        /// List of guardian approvals received so far
        approvals: Vec<GuardianApproval>,
    },
    /// Approvals received, cooldown active
    CooldownActive {
        /// Complete list of guardian approvals
        approvals: Vec<GuardianApproval>,
    },
    /// Cooldown complete, ready to execute
    ReadyToExecute {
        /// Complete list of guardian approvals
        approvals: Vec<GuardianApproval>,
    },
    /// Recovery completed successfully
    Completed {
        /// New session epoch after recovery
        new_session_epoch: SessionEpoch,
        /// Unix timestamp when recovery completed
        completed_at: u64,
    },
    /// Recovery cancelled by user
    Cancelled {
        /// Unix timestamp when recovery was cancelled
        cancelled_at: u64,
        /// Optional reason for cancellation
        reason: Option<String>,
    },
    /// Recovery vetoed by a guardian
    Vetoed {
        /// Guardian who vetoed the recovery
        vetoed_by: GuardianId,
        /// Unix timestamp when recovery was vetoed
        vetoed_at: u64,
        /// Optional reason for veto
        reason: Option<String>,
    },
    /// Recovery failed
    Failed {
        /// Unix timestamp when recovery failed
        failed_at: u64,
        /// Reason for failure
        reason: String,
    },
}

/// Guardian approval for recovery
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardianApproval {
    /// Guardian who approved
    pub guardian_id: GuardianId,
    /// Timestamp of approval
    pub approved_at: u64,
    /// Guardian's recovery share (encrypted until reconstruction)
    pub encrypted_share: Vec<u8>,
    /// Signature over (request_id || guardian_id || timestamp)
    pub signature: Vec<u8>,
}

/// Recovery veto by a guardian
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryVeto {
    /// Guardian issuing the veto
    pub guardian_id: GuardianId,
    /// Recovery request being vetoed
    pub request_id: uuid::Uuid,
    /// Timestamp of veto
    pub vetoed_at: u64,
    /// Reason for veto
    pub reason: Option<String>,
    /// Signature over (request_id || guardian_id || timestamp)
    pub signature: Vec<u8>,
}

/// Recovery cancellation by the account owner
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryCancellation {
    /// Device cancelling the recovery
    pub device_id: DeviceId,
    /// Recovery request being cancelled
    pub request_id: uuid::Uuid,
    /// Timestamp of cancellation
    pub cancelled_at: u64,
    /// Reason for cancellation
    pub reason: Option<String>,
    /// Signature from threshold of existing devices
    pub signature: Vec<u8>,
}

/// Recovery manager
///
/// Manages the recovery workflow including approval collection,
/// cooldown enforcement, and share reconstruction.
pub struct RecoveryManager {
    /// Active recovery requests (request_id -> request)
    active_requests: HashMap<uuid::Uuid, RecoveryRequest>,
    /// Completed requests (for audit trail)
    completed_requests: Vec<RecoveryRequest>,
    /// Maximum concurrent recovery requests per account
    max_concurrent_per_account: usize,
}

impl RecoveryManager {
    /// Create a new recovery manager
    pub fn new() -> Self {
        RecoveryManager {
            active_requests: HashMap::new(),
            completed_requests: Vec::new(),
            max_concurrent_per_account: 1, // Only one recovery at a time per account
        }
    }
    
    /// Initiate a recovery request
    ///
    /// # Arguments
    ///
    /// * `account_id` - Account to recover
    /// * `new_device_id` - New device requesting access
    /// * `guardian_ids` - Guardians to contact for approval
    /// * `required_approvals` - Minimum approvals needed
    /// * `cooldown_seconds` - Cooldown duration
    /// * `reason` - Optional reason
    ///
    /// # Returns
    ///
    /// The recovery request, or error if limits exceeded
    pub fn initiate_recovery(
        &mut self,
        account_id: AccountId,
        new_device_id: DeviceId,
        guardian_ids: Vec<GuardianId>,
        required_approvals: usize,
        cooldown_seconds: Option<u64>,
        reason: Option<String>,
        effects: &aura_crypto::Effects,
    ) -> Result<RecoveryRequest> {
        // Check if account already has active recovery
        let active_count = self.active_requests
            .values()
            .filter(|req| req.account_id == account_id)
            .count();
        
        if active_count >= self.max_concurrent_per_account {
            return Err(AgentError::device_not_found(
                "Account already has an active recovery request"
            ));
        }
        
        // Validate parameters
        if guardian_ids.len() < required_approvals {
            return Err(AgentError::invalid_context(
                format!("Not enough guardians: need {} approvals but only {} guardians", 
                    required_approvals, guardian_ids.len())
            ));
        }
        
        let request = RecoveryRequest::new(
            account_id,
            new_device_id,
            guardian_ids,
            required_approvals,
            cooldown_seconds,
            reason,
            effects,
        )?;
        
        let request_id = request.request_id;
        self.active_requests.insert(request_id, request.clone());
        
        Ok(request)
    }
    
    /// Submit guardian approval
    ///
    /// # Arguments
    ///
    /// * `request_id` - Recovery request ID
    /// * `approval` - Guardian approval with encrypted share
    ///
    /// # Returns
    ///
    /// Updated recovery request
    pub fn submit_approval(
        &mut self,
        request_id: uuid::Uuid,
        approval: GuardianApproval,
    ) -> Result<RecoveryRequest> {
        let request = self.active_requests
            .get_mut(&request_id)
            .ok_or_else(|| AgentError::device_not_found("Recovery request not found"))?;
        
        // Verify guardian is authorized
        if !request.guardian_ids.contains(&approval.guardian_id) {
            return Err(AgentError::invalid_context("Guardian not authorized for this recovery"));
        }
        
        // Add approval based on current status
        match &mut request.status {
            RecoveryStatus::PendingApprovals { approvals } => {
                // Check for duplicate approval
                if approvals.iter().any(|a| a.guardian_id == approval.guardian_id) {
                    return Err(AgentError::device_not_found("Guardian already approved"));
                }
                
                approvals.push(approval.clone());
                
                // Check if we have enough approvals to start cooldown
                if approvals.len() >= request.required_approvals {
                    request.status = RecoveryStatus::CooldownActive {
                        approvals: approvals.clone(),
                    };
                }
            }
            RecoveryStatus::CooldownActive { .. } => {
                return Err(AgentError::device_not_found("Recovery already in cooldown"));
            }
            _ => {
                return Err(AgentError::device_not_found("Recovery not accepting approvals"));
            }
        }
        
        Ok(request.clone())
    }
    
    /// Veto a recovery request
    ///
    /// Any authorized guardian can veto during cooldown period.
    pub fn submit_veto(
        &mut self,
        request_id: uuid::Uuid,
        veto: RecoveryVeto,
    ) -> Result<RecoveryRequest> {
        // Get and verify request first
        {
            let request = self.active_requests
                .get(&request_id)
                .ok_or_else(|| AgentError::device_not_found("Recovery request not found"))?;
            
            // Verify guardian is authorized
            if !request.guardian_ids.contains(&veto.guardian_id) {
                return Err(AgentError::invalid_context("Guardian not authorized for this recovery"));
            }
            
            // Can only veto during cooldown or pending approvals
            if !matches!(request.status, RecoveryStatus::PendingApprovals { .. } | RecoveryStatus::CooldownActive { .. }) {
                return Err(AgentError::device_not_found("Recovery cannot be vetoed in current state"));
            }
        }
        
        // Remove from active and update status
        let mut request = self.active_requests
            .remove(&request_id)
            .ok_or_else(|| AgentError::device_not_found("Recovery request not found"))?;
        
        request.status = RecoveryStatus::Vetoed {
            vetoed_by: veto.guardian_id,
            vetoed_at: veto.vetoed_at,
            reason: veto.reason.clone(),
        };
        
        // Move to completed
        self.completed_requests.push(request.clone());
        
        Ok(request)
    }
    
    /// Cancel a recovery request
    ///
    /// Can be done by the account owner during cooldown.
    pub fn cancel_recovery(
        &mut self,
        request_id: uuid::Uuid,
        cancellation: RecoveryCancellation,
    ) -> Result<RecoveryRequest> {
        // Get and verify request first
        {
            let request = self.active_requests
                .get(&request_id)
                .ok_or_else(|| AgentError::device_not_found("Recovery request not found"))?;
            
            // Can only cancel during cooldown or pending approvals
            if !matches!(request.status, RecoveryStatus::PendingApprovals { .. } | RecoveryStatus::CooldownActive { .. }) {
                return Err(AgentError::device_not_found("Recovery cannot be cancelled in current state"));
            }
        }
        
        // Remove from active and update status
        let mut request = self.active_requests
            .remove(&request_id)
            .ok_or_else(|| AgentError::device_not_found("Recovery request not found"))?;
        
        request.status = RecoveryStatus::Cancelled {
            cancelled_at: cancellation.cancelled_at,
            reason: cancellation.reason.clone(),
        };
        
        // Move to completed
        self.completed_requests.push(request.clone());
        
        Ok(request)
    }
    
    /// Check cooldown status and update if complete
    ///
    /// Should be called periodically to transition requests from CooldownActive to ReadyToExecute.
    pub fn check_cooldown_status(&mut self, request_id: uuid::Uuid, effects: &aura_crypto::Effects) -> Result<RecoveryRequest> {
        let request = self.active_requests
            .get_mut(&request_id)
            .ok_or_else(|| AgentError::device_not_found("Recovery request not found"))?;
        
        if let RecoveryStatus::CooldownActive { approvals } = &request.status {
            if request.cooldown_elapsed(effects)? {
                request.status = RecoveryStatus::ReadyToExecute {
                    approvals: approvals.clone(),
                };
            }
        }
        
        Ok(request.clone())
    }
    
    /// Execute recovery (reconstruct shares and add new device)
    ///
    /// # Arguments
    ///
    /// * `request_id` - Recovery request ID
    /// * `reconstructed_share` - New share for the recovering device
    /// * `new_session_epoch` - New session epoch after recovery
    ///
    /// # Security
    ///
    /// This should only be called after:
    /// 1. Sufficient guardian approvals collected
    /// 2. Cooldown period elapsed
    /// 3. Shares successfully reconstructed via MPC
    /// 4. New device share generated
    ///
    /// The caller is responsible for:
    /// - Reconstructing shares from guardian approvals
    /// - Running resharing protocol to generate new shares
    /// - Bumping session epoch
    /// - Invalidating old presence tickets
    pub fn execute_recovery(
        &mut self,
        request_id: uuid::Uuid,
        _reconstructed_share: KeyShare, // For future MPC integration
        new_session_epoch: SessionEpoch,
        effects: &aura_crypto::Effects,
    ) -> Result<RecoveryRequest> {
        // Get and verify request first
        {
            let request = self.active_requests
                .get(&request_id)
                .ok_or_else(|| AgentError::device_not_found("Recovery request not found"))?;
            
            // Verify ready to execute
            if !matches!(request.status, RecoveryStatus::ReadyToExecute { .. }) {
                return Err(AgentError::device_not_found("Recovery not ready to execute"));
            }
            
            if !request.can_proceed(effects) {
                return Err(AgentError::device_not_found("Recovery cannot proceed (cooldown not complete or insufficient approvals)"));
            }
        }
        
        // Remove from active and update status
        let mut request = self.active_requests
            .remove(&request_id)
            .ok_or_else(|| AgentError::device_not_found("Recovery request not found"))?;
        
        request.status = RecoveryStatus::Completed {
            new_session_epoch,
            completed_at: effects.now().unwrap_or(0),
        };
        
        // Move to completed
        self.completed_requests.push(request.clone());
        
        Ok(request)
    }
    
    /// Get all active recovery requests for an account
    pub fn get_active_requests(&self, account_id: AccountId) -> Vec<&RecoveryRequest> {
        self.active_requests
            .values()
            .filter(|req| req.account_id == account_id)
            .collect()
    }
    
    /// Get recovery request by ID
    pub fn get_request(&self, request_id: uuid::Uuid) -> Option<&RecoveryRequest> {
        self.active_requests.get(&request_id)
    }
    
    /// Clean up old completed requests
    ///
    /// Keeps last N completed requests for audit purposes.
    pub fn cleanup_completed(&mut self, keep_last: usize) {
        if self.completed_requests.len() > keep_last {
            self.completed_requests.drain(0..self.completed_requests.len() - keep_last);
        }
    }
}

impl Default for RecoveryManager {
    fn default() -> Self {
        Self::new()
    }
}

// Removed deprecated current_timestamp() function - use effects.now() instead

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_recovery_request_creation() {
        let effects = aura_crypto::Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        let guardians = vec![GuardianId::new_with_effects(&effects), GuardianId::new_with_effects(&effects), GuardianId::new_with_effects(&effects)];
        
        let request = RecoveryRequest::new(
            account_id,
            device_id,
            guardians.clone(),
            2, // 2-of-3
            Some(100), // 100 seconds for testing
            Some("Lost my phone".to_string()),
            &effects,
        ).unwrap();
        
        assert_eq!(request.required_approvals, 2);
        assert_eq!(request.guardian_ids.len(), 3);
        assert_eq!(request.cooldown_seconds, 100);
        assert!(matches!(request.status, RecoveryStatus::PendingApprovals { .. }));
    }
    
    #[test]
    fn test_recovery_workflow() {
        let effects = aura_crypto::Effects::test();
        let mut manager = RecoveryManager::new();
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        let guardians = vec![GuardianId::new_with_effects(&effects), GuardianId::new_with_effects(&effects), GuardianId::new_with_effects(&effects)];
        
        // Initiate recovery
        let request = manager.initiate_recovery(
            account_id,
            device_id,
            guardians.clone(),
            2,
            Some(1), // 1 second cooldown for testing
            None,
            &effects,
        ).unwrap();
        
        let request_id = request.request_id;
        
        // First guardian approval
        let approval1 = GuardianApproval {
            guardian_id: guardians[0],
            approved_at: effects.now().unwrap_or(0),
            encrypted_share: vec![1, 2, 3],
            signature: vec![],
        };
        
        let updated = manager.submit_approval(request_id, approval1).unwrap();
        assert!(matches!(updated.status, RecoveryStatus::PendingApprovals { .. }));
        
        // Second guardian approval (should trigger cooldown)
        let approval2 = GuardianApproval {
            guardian_id: guardians[1],
            approved_at: effects.now().unwrap_or(0),
            encrypted_share: vec![4, 5, 6],
            signature: vec![],
        };
        
        let updated = manager.submit_approval(request_id, approval2).unwrap();
        assert!(matches!(updated.status, RecoveryStatus::CooldownActive { .. }));
        
        // For testing purposes, manually transition to ready state
        // In practice, this would happen after cooldown time has elapsed
        let approvals = if let Some(active_request) = manager.get_request(request_id) {
            match &active_request.status {
                RecoveryStatus::CooldownActive { approvals } => approvals.clone(),
                _ => panic!("Expected CooldownActive status"),
            }
        } else {
            panic!("Request not found");
        };
        
        // Update status to ReadyToExecute
        if let Some(request) = manager.active_requests.get_mut(&request_id) {
            request.status = RecoveryStatus::ReadyToExecute { approvals };
        }
        
        let updated = manager.get_request(request_id).unwrap();
        assert!(matches!(updated.status, RecoveryStatus::ReadyToExecute { .. }));
    }
    
    #[test]
    fn test_recovery_veto() {
        let effects = aura_crypto::Effects::test();
        let mut manager = RecoveryManager::new();
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        let guardians = vec![GuardianId::new_with_effects(&effects), GuardianId::new_with_effects(&effects)];
        
        let request = manager.initiate_recovery(
            account_id,
            device_id,
            guardians.clone(),
            2,
            Some(100),
            None,
            &effects,
        ).unwrap();
        
        let request_id = request.request_id;
        
        // Guardian vetoes
        let veto = RecoveryVeto {
            guardian_id: guardians[0],
            request_id,
            vetoed_at: 3000,
            reason: Some("Suspicious activity".to_string()),
            signature: vec![],
        };
        
        let updated = manager.submit_veto(request_id, veto).unwrap();
        assert!(matches!(updated.status, RecoveryStatus::Vetoed { .. }));
        
        // Request should be removed from active
        assert!(manager.get_request(request_id).is_none());
    }
    
    #[test]
    fn test_recovery_cancellation() {
        let effects = aura_crypto::Effects::test();
        let mut manager = RecoveryManager::new();
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        let guardians = vec![GuardianId::new_with_effects(&effects)];
        
        let request = manager.initiate_recovery(
            account_id,
            device_id,
            guardians,
            1,
            Some(100),
            None,
            &effects,
        ).unwrap();
        
        let request_id = request.request_id;
        
        // Cancel recovery
        let cancellation = RecoveryCancellation {
            device_id,
            request_id,
            cancelled_at: 4000,
            reason: Some("Found my device".to_string()),
            signature: vec![],
        };
        
        let updated = manager.cancel_recovery(request_id, cancellation).unwrap();
        assert!(matches!(updated.status, RecoveryStatus::Cancelled { .. }));
    }
    
    #[test]
    fn test_duplicate_approval_rejected() {
        let effects = aura_crypto::Effects::test();
        let mut manager = RecoveryManager::new();
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        let guardians = vec![GuardianId::new_with_effects(&effects)];
        
        let request = manager.initiate_recovery(
            account_id,
            device_id,
            guardians.clone(),
            1,
            Some(100),
            None,
            &effects,
        ).unwrap();
        
        let approval = GuardianApproval {
            guardian_id: guardians[0],
            approved_at: 5000,
            encrypted_share: vec![],
            signature: vec![],
        };
        
        // First approval should succeed
        manager.submit_approval(request.request_id, approval.clone()).unwrap();
        
        // Duplicate approval should fail
        let result = manager.submit_approval(request.request_id, approval);
        assert!(result.is_err());
    }
}

