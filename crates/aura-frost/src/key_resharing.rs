//! G_reshare: Key Resharing Choreography
//!
//! This module implements FROST key resharing and rotation protocols using the Aura effect system pattern.
//!
//! ## Protocol Overview
//!
//! The G_reshare choreography implements secure key resharing for FROST threshold signatures.
//! This allows changing the threshold policy (M-of-N) or the participant set while maintaining
//! the same group signing key. The protocol ensures forward secrecy by invalidating old shares.
//!
//! ## Architecture
//!
//! The choreography follows a 5-phase protocol:
//! 1. **Setup**: Coordinator initiates resharing with old and new participants
//! 2. **Share Preparation**: Old guardians prepare their shares for redistribution
//! 3. **Share Distribution**: Coordinator redistributes shares to new guardians
//! 4. **Verification**: New guardians verify their received shares
//! 5. **Completion**: Coordinator distributes new public key package or failure notification
//!
//! ## Security Features
//!
//! - **Forward Secrecy**: Old shares become invalid after successful resharing
//! - **Backward Compatibility**: New key package works with existing signatures
//! - **Threshold Flexibility**: Can change both threshold and participant count
//! - **Byzantine Fault Tolerance**: Handles malicious participants during resharing
//! - **Atomic Updates**: Either all participants get new shares or none do

use aura_core::frost::PublicKeyPackage;
use aura_core::{identifiers::AuthorityId, AccountId};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};

// FROST key resharing choreography protocol
//
// This choreography implements the complete FROST key resharing protocol:
// - Coordinator initiates resharing with old and new guardians
// - Old guardians prepare their shares for redistribution
// - Coordinator distributes new shares to new guardians
// - New guardians verify their received shares
// - Coordinator distributes final public key package or failure notification
//
// Supports dynamic participant sets with Byzantine fault tolerance
choreography! {
    #[namespace = "frost_key_resharing"]
    protocol FrostKeyResharing {
        roles: Coordinator, OldGuardians[*], NewGuardians[*];

        // Phase 1: Coordinator initiates key resharing
        Coordinator[guard_capability = "initiate_resharing",
                   flow_cost = 100,
                   journal_facts = "resharing_initiated"]
        -> OldGuardians[*]: ResharingInit(ResharingRequest);

        Coordinator[guard_capability = "initiate_resharing",
                   flow_cost = 100,
                   journal_facts = "resharing_initiated"]
        -> NewGuardians[*]: ResharingInit(ResharingRequest);

        // Phase 2: Old guardians prepare shares for redistribution
        OldGuardians[*][guard_capability = "prepare_shares",
                       flow_cost = 75,
                       journal_facts = "shares_prepared"]
        -> Coordinator: SharePreparation(SharePackage);

        // Phase 3: Coordinator distributes new shares to new guardians
        Coordinator[guard_capability = "distribute_shares",
                   flow_cost = 150,
                   journal_facts = "shares_distributed"]
        -> NewGuardians[*]: NewSharePackage(SharePackage);

        // Phase 4: New guardians verify their new shares
        NewGuardians[*][guard_capability = "verify_shares",
                       flow_cost = 50,
                       journal_facts = "shares_verified"]
        -> Coordinator: VerificationResult(VerificationResult);

        // Phase 5: Coordinator distributes final result
        choice Coordinator {
            success: {
                Coordinator[guard_capability = "distribute_success",
                           flow_cost = 200,
                           journal_facts = "resharing_completed",
                           journal_merge = true]
                -> OldGuardians[*]: ResharingSuccess(ResharingResponse);

                Coordinator[guard_capability = "distribute_success",
                           flow_cost = 200,
                           journal_facts = "resharing_completed",
                           journal_merge = true]
                -> NewGuardians[*]: ResharingSuccess(ResharingResponse);
            }
            failure: {
                Coordinator[guard_capability = "distribute_failure",
                           flow_cost = 100,
                           journal_facts = "resharing_failed"]
                -> OldGuardians[*]: ResharingFailure(String);

                Coordinator[guard_capability = "distribute_failure",
                           flow_cost = 100,
                           journal_facts = "resharing_failed"]
                -> NewGuardians[*]: ResharingFailure(String);
            }
        }
    }
}

// Message types for key resharing choreography

/// Resharing initiation message containing the request details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResharingInit {
    /// The resharing request with session details
    pub request: ResharingRequest,
}

/// Successful resharing result message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResharingSuccess {
    /// The final resharing response
    pub response: ResharingResponse,
}

/// Failed resharing result message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResharingFailure {
    /// Error description for the failure
    pub error: String,
}

/// Share preparation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharePreparation {
    /// Session identifier
    pub session_id: String,
    /// Prepared share data
    pub share_data: Vec<u8>,
    /// Authority who prepared this share
    pub participant_id: AuthorityId,
}

/// New share package message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSharePackage {
    /// Session identifier
    pub session_id: String,
    /// New share package data
    pub package: SharePackage,
}

/// Verification result message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Session identifier
    pub session_id: String,
    /// Whether verification was successful
    pub verified: bool,
    /// Authority who performed verification
    pub participant_id: AuthorityId,
}

/// Key resharing request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResharingRequest {
    /// Session identifier
    pub session_id: String,
    /// Account for key resharing
    pub account_id: AccountId,
    /// Current threshold configuration
    pub old_threshold: usize,
    /// New threshold configuration
    pub new_threshold: usize,
    /// Current participants
    pub old_participants: Vec<AuthorityId>,
    /// New participant set
    pub new_participants: Vec<AuthorityId>,
    /// Session timeout in seconds
    pub timeout_seconds: u64,
}

/// Key resharing response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResharingResponse {
    /// New public key package
    pub public_key_package: Option<PublicKeyPackage>,
    /// Resharing successful
    pub success: bool,
    /// New participants
    pub participants: Vec<AuthorityId>,
    /// Error message if any
    pub error: Option<String>,
}

/// Share package for redistribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharePackage {
    /// Session identifier
    pub session_id: String,
    /// Encrypted share data
    pub share_data: Vec<u8>,
    /// Target authority for this share
    pub target_participant: AuthorityId,
}

// The choreography macro generates these types and functions automatically:
// - FrostKeyResharingChoreography struct
// - Role-specific projection types for Coordinator, OldGuardians, and NewGuardians
// - Message routing and session type enforcement

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::test_utils::test_authority_id;

    #[test]
    fn test_resharing_request_serialization() {
        let request = ResharingRequest {
            session_id: "test_session".to_string(),
            account_id: AccountId::new(),
            old_threshold: 2,
            new_threshold: 3,
            old_participants: vec![test_authority_id(1), test_authority_id(2)],
            new_participants: vec![
                test_authority_id(3),
                test_authority_id(4),
                test_authority_id(5),
            ],
            timeout_seconds: 300,
        };

        let serialized = serde_json::to_vec(&request).unwrap();
        let deserialized: ResharingRequest = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(request.session_id, deserialized.session_id);
        assert_eq!(request.old_threshold, deserialized.old_threshold);
        assert_eq!(request.new_threshold, deserialized.new_threshold);
        assert_eq!(
            request.old_participants.len(),
            deserialized.old_participants.len()
        );
        assert_eq!(
            request.new_participants.len(),
            deserialized.new_participants.len()
        );
    }

    #[test]
    fn test_share_package_serialization() {
        let package = SharePackage {
            session_id: "test_session".to_string(),
            share_data: vec![1, 2, 3, 4],
            target_participant: test_authority_id(6),
        };

        let serialized = serde_json::to_vec(&package).unwrap();
        let deserialized: SharePackage = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(package.session_id, deserialized.session_id);
        assert_eq!(package.share_data, deserialized.share_data);
        assert_eq!(package.target_participant, deserialized.target_participant);
    }

    #[test]
    fn test_verification_result_serialization() {
        let result = VerificationResult {
            session_id: "test_session".to_string(),
            verified: true,
            participant_id: test_authority_id(7),
        };

        let serialized = serde_json::to_vec(&result).unwrap();
        let deserialized: VerificationResult = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(result.session_id, deserialized.session_id);
        assert_eq!(result.verified, deserialized.verified);
        assert_eq!(result.participant_id, deserialized.participant_id);
    }
}
