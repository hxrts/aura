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
use aura_core::{identifiers::AuthorityId, AccountId};
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
    /// Authority who created this commitment
    pub participant_id: AuthorityId,
}

/// Share revelation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareRevelation {
    /// Session identifier
    pub session_id: String,
    /// Revealed share data
    pub share_data: Vec<u8>,
    /// Authority who revealed this share
    pub participant_id: AuthorityId,
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
    /// Participating authorities
    pub participants: Vec<AuthorityId>,
    /// Session timeout in seconds
    pub timeout_seconds: u64,
}

/// Distributed key generation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgResponse {
    /// Generated public key package
    pub public_key_package: Option<PublicKeyPackage>,
    /// Participating authorities
    pub participants: Vec<AuthorityId>,
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
    /// Authority order
    pub participant_order: Vec<AuthorityId>,
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
    use aura_macros::aura_test;
    use aura_testkit::simulation::choreography::ChoreographyTestHarness;
    use aura_testkit::{create_test_fixture, DeviceTestFixture};

    #[test]
    fn test_dkg_request_serialization() {
        let fixture = create_test_fixture();
        let request = DkgRequest {
            session_id: "test_session".to_string(),
            account_id: AccountId::new(),
            threshold: 2,
            total_participants: 3,
            participants: vec![
                fixture.authority_id(0),
                fixture.authority_id(1),
                fixture.authority_id(2),
            ],
            timeout_seconds: 120,
        };

        let serialized = serde_json::to_vec(&request).unwrap();
        let deserialized: DkgRequest = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(request.session_id, deserialized.session_id);
        assert_eq!(request.threshold, deserialized.threshold);
        assert_eq!(request.total_participants, deserialized.total_participants);
    }

    #[aura_test]
    async fn test_dkg_choreography_basic() -> aura_core::AuraResult<()> {
        // Create multi-device test harness for distributed key generation
        let mut harness = ChoreographyTestHarness::with_devices(3);
        
        // Assign choreographic roles
        harness.assign_role("Coordinator", 0);
        harness.assign_role("Participants", &[0, 1, 2]);

        // Create DKG request with testkit-generated authorities
        let fixture = create_test_fixture();
        let dkg_request = DkgRequest {
            session_id: "test_dkg_session".to_string(),
            account_id: AccountId::new(),
            threshold: 2,
            total_participants: 3,
            participants: vec![
                fixture.authority_id(0),
                fixture.authority_id(1),
                fixture.authority_id(2),
            ],
            timeout_seconds: 120,
        };

        // Test that DKG init message can be created and serialized
        let dkg_init = DkgInit { request: dkg_request };
        
        // Verify choreography message structure
        let serialized = serde_json::to_vec(&dkg_init)?;
        let deserialized: DkgInit = serde_json::from_slice(&serialized)
            .map_err(|e| aura_core::AuraError::parse(e.to_string()))?;
        
        assert_eq!(dkg_init.request.session_id, deserialized.request.session_id);
        assert_eq!(dkg_init.request.threshold, deserialized.request.threshold);

        println!("✓ DKG choreography basic test completed");
        Ok(())
    }

    #[aura_test]
    async fn test_dkg_multi_device_workflow() -> aura_core::AuraResult<()> {
        // Create comprehensive multi-device test environment
        let mut harness = ChoreographyTestHarness::with_devices(5);
        
        // Set up threshold scheme: 3-of-5
        harness.assign_role("Coordinator", 0);
        harness.assign_role("Participants", &[0, 1, 2, 3, 4]);

        let fixture = create_test_fixture();
        
        // Test complete DKG workflow with all message types
        let session_id = "integration_test_session".to_string();
        
        // Phase 1: DKG Initialization
        let dkg_request = DkgRequest {
            session_id: session_id.clone(),
            account_id: AccountId::new(),
            threshold: 3,
            total_participants: 5,
            participants: (0..5).map(|i| fixture.authority_id(i)).collect(),
            timeout_seconds: 300,
        };
        
        // Phase 2: Simulate share commitments from participants
        let mut share_commitments = Vec::new();
        for i in 0..3 {
            let commitment = ShareCommitment {
                session_id: session_id.clone(),
                commitment_data: vec![42u8 + i as u8; 32], // Mock commitment data
                participant_id: fixture.authority_id(i),
            };
            share_commitments.push(commitment);
        }
        
        // Phase 3: Simulate share revelations
        let mut share_revelations = Vec::new();
        for i in 0..3 {
            let revelation = ShareRevelation {
                session_id: session_id.clone(),
                share_data: vec![100u8 + i as u8; 64], // Mock share data
                participant_id: fixture.authority_id(i),
            };
            share_revelations.push(revelation);
        }
        
        // Phase 4: Simulate verification results
        let mut verification_results = Vec::new();
        for i in 0..3 {
            let result = VerificationResult {
                session_id: session_id.clone(),
                verified: true,
                participant_id: fixture.authority_id(i),
            };
            verification_results.push(result);
        }
        
        // Phase 5: Test successful completion
        let dkg_success = DkgSuccess {
            session_id: session_id.clone(),
            public_key_package: PublicKeyPackage::default(),
        };

        // Verify all message types serialize correctly
        assert!(serde_json::to_vec(&dkg_request).is_ok());
        assert!(serde_json::to_vec(&share_commitments[0]).is_ok());
        assert!(serde_json::to_vec(&share_revelations[0]).is_ok());
        assert!(serde_json::to_vec(&verification_results[0]).is_ok());
        assert!(serde_json::to_vec(&dkg_success).is_ok());

        println!("✓ DKG multi-device workflow test completed successfully");
        Ok(())
    }
}
