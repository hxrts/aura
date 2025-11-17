//! G_dkg: Distributed Key Generation Choreography
//!
//! This module implements distributed threshold key generation using the Aura effect system pattern.
//!
//! ## Protocol Overview
//!
//! The G_dkg choreography implements a secure distributed key generation (DKG) protocol
//! for FROST threshold signatures. The protocol ensures that no single participant can
//! learn the complete secret key, while enabling threshold signing operations.
//!
//! ## Architecture
//!
//! The choreography follows a 5-phase protocol:
//! 1. **Setup**: Coordinator initiates DKG with all participants
//! 2. **Commitment**: Participants generate and commit to polynomial shares
//! 3. **Revelation**: Coordinator broadcasts commitments, participants reveal shares
//! 4. **Verification**: Participants verify received shares against commitments
//! 5. **Completion**: Coordinator distributes final public key package
//!
//! ## Security Features
//!
//! - **Verifiable Secret Sharing (VSS)**: Ensures shares are valid before commitment
//! - **Byzantine Fault Tolerance**: Handles up to threshold-1 malicious participants
//! - **Zero Trust**: No participant needs to trust any other participant
//! - **Session Isolation**: Each DKG session is cryptographically isolated
//! - **Timeout Protection**: Built-in timeout handling prevents DoS attacks

use aura_core::frost::PublicKeyPackage;
use aura_core::{AccountId, DeviceId};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};

/// DKG initialization message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgInit {
    /// The DKG request with session details
    pub request: DkgRequest,
}

/// Share commitment message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareCommitment {
    /// Session identifier
    pub session_id: String,
    /// Commitment data from participant
    pub commitment_data: Vec<u8>,
    /// Participant who created this commitment
    pub participant_id: DeviceId,
}

/// Share revelation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareRevelation {
    /// Session identifier
    pub session_id: String,
    /// Revealed share data
    pub share_data: Vec<u8>,
    /// Participant who revealed this share
    pub participant_id: DeviceId,
}

/// Verification result message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Session identifier
    pub session_id: String,
    /// Whether verification was successful
    pub verified: bool,
    /// Participant who performed verification
    pub participant_id: DeviceId,
}

/// DKG success message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgSuccess {
    /// Session identifier
    pub session_id: String,
    /// Generated public key package
    pub public_key_package: PublicKeyPackage,
}

/// DKG failure message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgFailure {
    /// Session identifier
    pub session_id: String,
    /// Error message describing the failure
    pub error: String,
}

/// Distributed key generation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgRequest {
    /// Session identifier
    pub session_id: String,
    /// Account for key generation
    pub account_id: AccountId,
    /// Required threshold (M in M-of-N)
    pub threshold: usize,
    /// Total number of participants
    pub total_participants: usize,
    /// Participating devices
    pub participants: Vec<DeviceId>,
    /// Session timeout in seconds
    pub timeout_seconds: u64,
}

/// Distributed key generation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgResponse {
    /// Generated public key package
    pub public_key_package: Option<PublicKeyPackage>,
    /// Participating devices
    pub participants: Vec<DeviceId>,
    /// Individual shares distributed to participants
    pub shares_distributed: usize,
    /// Key generation successful
    pub success: bool,
    /// Error message if any
    pub error: Option<String>,
}

/// Bundle of commitments from all participants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgCommitmentBundle {
    /// Session identifier
    pub session_id: String,
    /// All collected commitments
    pub commitments: Vec<Vec<u8>>,
    /// Participant order
    pub participant_order: Vec<DeviceId>,
}

// FROST distributed key generation choreography protocol
//
// This choreography implements the complete FROST DKG protocol:
// - Coordinator initiates DKG and coordinates all phases
// - Participants generate shares, commit, reveal, and verify
// - Supports dynamic participant sets with Byzantine fault tolerance
// - Provides session isolation and timeout handling
choreography! {
    #[namespace = "frost_distributed_keygen"]
    protocol FrostDistributedKeygen {
        roles: Coordinator, Participants[*];

        // Phase 1: Coordinator initiates DKG with all participants
        Coordinator[guard_capability = "initiate_dkg",
                   flow_cost = 100,
                   journal_facts = "dkg_initiated"]
        -> Participants[*]: DkgInit(DkgInit);

        // Phase 2: Participants generate and send share commitments
        Participants[0..threshold][guard_capability = "commit_share",
                                  flow_cost = 75,
                                  journal_facts = "share_committed"]
        -> Coordinator: ShareCommitment(ShareCommitment);

        // Phase 3: Coordinator broadcasts commitments, participants reveal shares
        Coordinator[guard_capability = "distribute_commitments",
                   flow_cost = 150,
                   journal_facts = "commitments_distributed"]
        -> Participants[*]: CommitmentBundle(DkgCommitmentBundle);

        Participants[0..threshold][guard_capability = "reveal_share",
                                  flow_cost = 75,
                                  journal_facts = "share_revealed"]
        -> Coordinator: ShareRevelation(ShareRevelation);

        // Phase 4: Participants verify shares and report results
        Participants[0..threshold][guard_capability = "verify_share",
                                  flow_cost = 50,
                                  journal_facts = "share_verified"]
        -> Coordinator: VerificationResult(VerificationResult);

        // Phase 5: Coordinator distributes final result
        choice Coordinator {
            success: {
                Coordinator[guard_capability = "distribute_success",
                           flow_cost = 200,
                           journal_facts = "dkg_completed",
                           journal_merge = true]
                -> Participants[*]: DkgSuccess(DkgSuccess);
            }
            failure: {
                Coordinator[guard_capability = "distribute_failure",
                           flow_cost = 100,
                           journal_facts = "dkg_failed"]
                -> Participants[*]: DkgFailure(DkgFailure);
            }
        }
    }
}

// The choreography macro generates these types and functions automatically:
// - FrostDistributedKeygenChoreography struct
// - Role-specific projection types for Coordinator and Participants
// - Message routing and session type enforcement

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::test_utils::test_device_id;

    #[test]
    fn test_dkg_request_serialization() {
        let request = DkgRequest {
            session_id: "test_session".to_string(),
            account_id: AccountId::new(),
            threshold: 2,
            total_participants: 3,
            participants: vec![test_device_id(1), test_device_id(2), test_device_id(3)],
            timeout_seconds: 120,
        };

        let serialized = serde_json::to_vec(&request).unwrap();
        let deserialized: DkgRequest = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(request.session_id, deserialized.session_id);
        assert_eq!(request.threshold, deserialized.threshold);
        assert_eq!(request.total_participants, deserialized.total_participants);
    }
}
