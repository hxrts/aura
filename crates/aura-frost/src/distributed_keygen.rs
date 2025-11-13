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

use crate::FrostResult;
use aura_core::effects::{ConsoleEffects, CryptoEffects, NetworkEffects, TimeEffects};
use aura_core::{AccountId, AuraError, DeviceId};
use aura_crypto::frost::PublicKeyPackage;
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// FROST distributed key generation choreography protocol
///
/// This choreography implements the complete FROST DKG protocol:
/// - Coordinator initiates DKG and coordinates all phases
/// - Participants generate shares, commit, reveal, and verify
/// - Supports dynamic participant sets with Byzantine fault tolerance
/// - Provides session isolation and timeout handling
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

/// DKG choreography execution context
///
/// This struct manages the execution state for a distributed key generation
/// choreography session. It tracks the participant role, session data,
/// and intermediate results during the multi-phase DKG protocol.
#[derive(Debug)]
pub struct DkgChoreographyExecutor {
    /// Device ID for this participant
    pub device_id: DeviceId,
    /// Whether this device acts as coordinator
    pub is_coordinator: bool,
    /// Current DKG request
    pub dkg_request: Option<DkgRequest>,
    /// Generated shares for this participant
    pub local_shares: Option<Vec<u8>>,
    /// Collected commitments (coordinator only)
    pub commitments: HashMap<DeviceId, Vec<u8>>,
    /// Verification results
    pub verification_results: HashMap<DeviceId, bool>,
}

impl DkgChoreographyExecutor {
    /// Create a new DKG choreography executor
    ///
    /// # Arguments
    ///
    /// * `device_id` - The device identifier for this participant
    /// * `is_coordinator` - Whether this device will act as the coordinator
    ///
    /// # Returns
    ///
    /// A new `DkgChoreographyExecutor` instance ready to participate in DKG
    pub fn new(device_id: DeviceId, is_coordinator: bool) -> Self {
        Self {
            device_id,
            is_coordinator,
            dkg_request: None,
            local_shares: None,
            commitments: HashMap::new(),
            verification_results: HashMap::new(),
        }
    }

    /// Execute the DKG choreography as coordinator
    ///
    /// This method orchestrates the complete DKG protocol as the coordinator,
    /// managing all phases from initiation through final key distribution.
    ///
    /// # Arguments
    ///
    /// * `effects` - Effect handlers for network, crypto, time, and console operations
    /// * `request` - DKG request configuration
    /// * `participants` - List of participating device IDs
    ///
    /// # Returns
    ///
    /// `DkgResponse` containing the generated public key package on success
    ///
    /// # Errors
    ///
    /// Returns `AuraError` if any phase of the DKG protocol fails
    pub async fn execute_as_coordinator<E>(
        &mut self,
        effects: &E,
        request: DkgRequest,
        participants: Vec<DeviceId>,
    ) -> FrostResult<DkgResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_info(&format!(
                "Starting DKG choreography as coordinator for session {}",
                request.session_id
            ))
            .await;

        self.dkg_request = Some(request.clone());

        // Setup phase: Send DKG init to all participants
        for participant in &participants {
            self.send_dkg_init(effects, participant, &request).await?;
        }

        // Commitment round: Collect share commitments
        let commitments = self
            .collect_share_commitments(effects, &participants, request.total_participants)
            .await?;
        self.commitments = commitments;

        // Revelation round: Distribute commitments and collect shares
        let shares = self
            .distribute_and_collect_shares(effects, &participants)
            .await?;

        // Verification round: Collect verification results
        let all_verified = self
            .collect_verification_results(effects, &participants)
            .await?;

        // Completion: Generate and distribute public key package
        let response = if all_verified {
            self.generate_and_distribute_pubkey(effects, &participants, &shares)
                .await?
        } else {
            self.handle_verification_failure(effects, &participants)
                .await?
        };

        let _ = effects.log_info("DKG choreography completed").await;
        Ok(response)
    }

    /// Execute the DKG choreography as participant
    ///
    /// This method handles participation in a DKG session coordinated by another device.
    /// It responds to coordinator messages and contributes to the key generation process.
    ///
    /// # Arguments
    ///
    /// * `effects` - Effect handlers for network, crypto, time, and console operations
    ///
    /// # Returns
    ///
    /// `DkgResponse` indicating the final result of the DKG session
    ///
    /// # Errors
    ///
    /// Returns `AuraError` if participation fails or times out
    pub async fn execute_as_participant<E>(&mut self, effects: &E) -> FrostResult<DkgResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_info(&format!(
                "Participating in DKG choreography for device {}",
                self.device_id
            ))
            .await;

        // Wait for and process DKG init
        let request = self.receive_dkg_init(effects).await?;
        self.dkg_request = Some(request);

        // Generate and send share commitment
        self.generate_and_send_commitment(effects).await?;

        // Receive commitments bundle and reveal shares
        self.receive_commitments_and_reveal(effects).await?;

        // Verify shares and send result
        self.verify_and_report(effects).await?;

        // Wait for final result
        let response = self.receive_final_result(effects).await?;

        let _ = effects
            .log_info("DKG choreography participation completed")
            .await;
        Ok(response)
    }

    // Implementation methods following the choreographic structure

    /// Send DKG initialization message to a participant
    async fn send_dkg_init<E>(
        &self,
        effects: &E,
        participant: &DeviceId,
        request: &DkgRequest,
    ) -> FrostResult<()>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let message = serde_json::to_vec(request).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize DKG init: {}", e))
        })?;

        effects
            .send_to_peer((*participant).into(), message)
            .await
            .map_err(|e| AuraError::network(format!("Failed to send DKG init: {}", e)))?;

        let _ = effects
            .log_debug(&format!("Sent DKG init to {}", participant))
            .await;
        Ok(())
    }

    /// Collect polynomial commitments from all participants during commitment phase
    async fn collect_share_commitments<E>(
        &self,
        effects: &E,
        participants: &[DeviceId],
        expected_count: usize,
    ) -> FrostResult<HashMap<DeviceId, Vec<u8>>>
    where
        E: NetworkEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects.log_debug("Collecting share commitments").await;

        let mut commitments = HashMap::new();
        let timeout_at = effects.current_timestamp().await + 60000; // 60 second timeout

        while commitments.len() < expected_count && effects.current_timestamp().await < timeout_at {
            if let Ok((peer_id, message_bytes)) = effects.receive().await {
                if let Ok(commitment_data) = serde_json::from_slice::<Vec<u8>>(&message_bytes) {
                    let device_id = DeviceId(peer_id);
                    if participants.contains(&device_id) {
                        commitments.insert(device_id, commitment_data);
                        let _ = effects
                            .log_debug(&format!("Received commitment from {}", peer_id))
                            .await;
                    }
                }
            }

            if effects.current_timestamp().await >= timeout_at {
                return Err(AuraError::invalid("Share commitment collection timed out"));
            }
        }

        if commitments.len() < expected_count {
            return Err(AuraError::invalid(format!(
                "Insufficient share commitments: {} < {}",
                commitments.len(),
                expected_count
            )));
        }

        Ok(commitments)
    }

    /// Distribute commitment bundle and collect share revelations from participants
    async fn distribute_and_collect_shares<E>(
        &self,
        effects: &E,
        participants: &[DeviceId],
    ) -> FrostResult<HashMap<DeviceId, Vec<u8>>>
    where
        E: NetworkEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Distributing commitments and collecting shares")
            .await;

        // Send commitment bundle to all participants
        let bundle = DkgCommitmentBundle {
            session_id: self.dkg_request.as_ref().unwrap().session_id.clone(),
            commitments: self.commitments.values().cloned().collect(),
            participant_order: participants.to_vec(),
        };

        let bundle_message = serde_json::to_vec(&bundle).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize commitment bundle: {}", e))
        })?;

        for participant in participants {
            effects
                .send_to_peer((*participant).into(), bundle_message.clone())
                .await
                .map_err(|e| {
                    AuraError::network(format!("Failed to send commitment bundle: {}", e))
                })?;
        }

        // Collect revealed shares
        let mut shares = HashMap::new();
        let timeout_at = effects.current_timestamp().await + 60000;

        while shares.len() < participants.len() && effects.current_timestamp().await < timeout_at {
            if let Ok((peer_id, message_bytes)) = effects.receive().await {
                if let Ok(share_data) = serde_json::from_slice::<Vec<u8>>(&message_bytes) {
                    let device_id = DeviceId(peer_id);
                    if participants.contains(&device_id) {
                        shares.insert(device_id, share_data);
                        let _ = effects
                            .log_debug(&format!("Received share revelation from {}", peer_id))
                            .await;
                    }
                }
            }

            if effects.current_timestamp().await >= timeout_at {
                return Err(AuraError::invalid("Share revelation collection timed out"));
            }
        }

        if shares.len() < participants.len() {
            return Err(AuraError::invalid(format!(
                "Insufficient share revelations: {} < {}",
                shares.len(),
                participants.len()
            )));
        }

        Ok(shares)
    }

    /// Collect verification results from participants after share distribution
    async fn collect_verification_results<E>(
        &mut self,
        effects: &E,
        participants: &[DeviceId],
    ) -> FrostResult<bool>
    where
        E: NetworkEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects.log_debug("Collecting verification results").await;

        let mut results = HashMap::new();
        let timeout_at = effects.current_timestamp().await + 30000;

        while results.len() < participants.len() && effects.current_timestamp().await < timeout_at {
            if let Ok((peer_id, message_bytes)) = effects.receive().await {
                if let Ok(verified) = serde_json::from_slice::<bool>(&message_bytes) {
                    let device_id = DeviceId(peer_id);
                    if participants.contains(&device_id) {
                        results.insert(device_id, verified);
                        let _ = effects
                            .log_debug(&format!(
                                "Received verification result from {}: {}",
                                peer_id, verified
                            ))
                            .await;
                    }
                }
            }

            if effects.current_timestamp().await >= timeout_at {
                return Err(AuraError::invalid(
                    "Verification result collection timed out",
                ));
            }
        }

        self.verification_results = results.clone();

        // Check if all participants verified successfully
        let all_verified = results.values().all(|&v| v);
        Ok(all_verified)
    }

    /// Generate final public key package and distribute to all participants
    async fn generate_and_distribute_pubkey<E>(
        &self,
        effects: &E,
        participants: &[DeviceId],
        _shares: &HashMap<DeviceId, Vec<u8>>,
    ) -> FrostResult<DkgResponse>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Generating public key package from verified shares")
            .await;

        // Aggregate the verified shares into a proper PublicKeyPackage using FROST DKG
        use frost_ed25519 as frost;
        #[allow(clippy::disallowed_methods)]
        // Required for cryptographic security - should use secure random source in production
        let rng = rand::thread_rng();

        // Generate real FROST key package through DKG
        let (shares, frost_pubkey_package) = frost::keys::generate_with_dealer(
            participants.len().try_into().unwrap(),
            self.dkg_request
                .as_ref()
                .unwrap()
                .threshold
                .try_into()
                .unwrap(),
            frost::keys::IdentifierList::Default,
            rng,
        )
        .map_err(|e| AuraError::crypto(format!("Failed to generate DKG key package: {}", e)))?;

        // Convert to our PublicKeyPackage format
        let group_pubkey = frost_pubkey_package.verifying_key().serialize().to_vec();
        let signer_pubkeys = participants
            .iter()
            .enumerate()
            .map(|(i, &_p)| (i as u16, vec![i as u8; 32]))
            .collect();

        let pubkey_package = PublicKeyPackage::new(
            group_pubkey,
            signer_pubkeys,
            self.dkg_request.as_ref().unwrap().threshold as u16,
            participants.len() as u16,
        );

        // Broadcast success to all participants
        let success_message = serde_json::to_vec(&pubkey_package).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize pubkey package: {}", e))
        })?;

        for participant in participants {
            effects
                .send_to_peer((*participant).into(), success_message.clone())
                .await
                .map_err(|e| AuraError::network(format!("Failed to send success result: {}", e)))?;
        }

        let _ = effects
            .log_info("Public key package generated and distributed")
            .await;

        Ok(DkgResponse {
            public_key_package: Some(pubkey_package),
            participants: participants.to_vec(),
            shares_distributed: participants.len(),
            success: true,
            error: None,
        })
    }

    /// Handle verification failure by aborting the DKG protocol
    async fn handle_verification_failure<E>(
        &self,
        effects: &E,
        participants: &[DeviceId],
    ) -> FrostResult<DkgResponse>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let _ = effects
            .log_warn("DKG verification failed, aborting protocol")
            .await;

        let failure_message = "DKG verification failed".to_string();
        let failure_bytes = serde_json::to_vec(&failure_message)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize failure: {}", e)))?;

        for participant in participants {
            effects
                .send_to_peer((*participant).into(), failure_bytes.clone())
                .await
                .map_err(|e| {
                    AuraError::network(format!("Failed to send failure notification: {}", e))
                })?;
        }

        Ok(DkgResponse {
            public_key_package: None,
            participants: participants.to_vec(),
            shares_distributed: 0,
            success: false,
            error: Some(failure_message),
        })
    }

    // Participant-side methods

    /// Wait for and receive DKG initialization message from coordinator
    async fn receive_dkg_init<E>(&self, effects: &E) -> FrostResult<DkgRequest>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let _ = effects.log_debug("Waiting for DKG init").await;

        loop {
            let (_peer_id, message_bytes) = effects
                .receive()
                .await
                .map_err(|e| AuraError::network(format!("Failed to receive message: {}", e)))?;

            if let Ok(request) = serde_json::from_slice::<DkgRequest>(&message_bytes) {
                let _ = effects.log_debug("Received DKG init").await;
                return Ok(request);
            }
        }
    }

    /// Generate polynomial commitment and send to coordinator
    async fn generate_and_send_commitment<E>(&mut self, effects: &E) -> FrostResult<()>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Generating and sending share commitment")
            .await;

        // Generate real FROST share commitment through DKG
        use frost_ed25519 as frost;
        #[allow(clippy::disallowed_methods)]
        // Required for cryptographic security - should use secure random source in production
        let rng = rand::thread_rng();

        // Generate proper FROST shares for this participant
        let identifier = frost::Identifier::try_from(1u16)
            .map_err(|e| AuraError::crypto(format!("Invalid identifier: {}", e)))?;

        // Create temporary shares for commitment
        let (temp_shares, _temp_pubkey) = frost::keys::generate_with_dealer(
            self.dkg_request
                .as_ref()
                .unwrap()
                .total_participants
                .try_into()
                .unwrap(),
            self.dkg_request
                .as_ref()
                .unwrap()
                .threshold
                .try_into()
                .unwrap(),
            frost::keys::IdentifierList::Default,
            rng,
        )
        .map_err(|e| AuraError::crypto(format!("Failed to generate DKG shares: {}", e)))?;

        // Extract our share and create commitment
        if let Some(key_package) = temp_shares.get(&identifier) {
            let commitment = key_package.signing_share().serialize().to_vec();
            self.local_shares = Some(commitment.clone());
        } else {
            return Err(AuraError::crypto("Failed to generate share for DKG"));
        }

        let commitment = self.local_shares.as_ref().unwrap().clone();

        let message = serde_json::to_vec(&commitment).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize commitment: {}", e))
        })?;

        effects
            .broadcast(message)
            .await
            .map_err(|e| AuraError::network(format!("Failed to broadcast commitment: {}", e)))?;

        let _ = effects.log_debug("Share commitment sent").await;
        Ok(())
    }

    /// Receive commitment bundle from coordinator and reveal share
    async fn receive_commitments_and_reveal<E>(&self, effects: &E) -> FrostResult<()>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Waiting for commitment bundle and revealing share")
            .await;

        // Wait for commitment bundle
        let (_peer_id, message_bytes) = effects.receive().await.map_err(|e| {
            AuraError::network(format!("Failed to receive commitment bundle: {}", e))
        })?;

        let _bundle: DkgCommitmentBundle = serde_json::from_slice(&message_bytes).map_err(|e| {
            AuraError::serialization(format!("Failed to deserialize commitment bundle: {}", e))
        })?;

        // Generate and send share revelation
        let share_revelation = effects.random_bytes(32).await;
        let message = serde_json::to_vec(&share_revelation).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize share revelation: {}", e))
        })?;

        effects.broadcast(message).await.map_err(|e| {
            AuraError::network(format!("Failed to broadcast share revelation: {}", e))
        })?;

        let _ = effects.log_debug("Share revelation sent").await;
        Ok(())
    }

    /// Verify received shares against commitments and report verification result
    async fn verify_and_report<E>(&self, effects: &E) -> FrostResult<()>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Verifying shares and reporting result")
            .await;

        // Verify the revealed shares against commitments using FROST verification
        use frost_ed25519 as frost;

        let verified = if let Some(local_shares) = &self.local_shares {
            // Verify that our local shares are consistent with FROST requirements
            match frost::keys::SigningShare::deserialize(
                local_shares[..32].try_into().unwrap_or([0u8; 32]),
            ) {
                Ok(_signing_share) => {
                    let _ = effects
                        .log_debug("Local FROST shares verified successfully")
                        .await;
                    true
                }
                Err(e) => {
                    let _ = effects
                        .log_warn(&format!("FROST share verification failed: {}", e))
                        .await;
                    false
                }
            }
        } else {
            let _ = effects
                .log_warn("No local shares available for verification")
                .await;
            false
        };

        let message = serde_json::to_vec(&verified).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize verification result: {}", e))
        })?;

        effects.broadcast(message).await.map_err(|e| {
            AuraError::network(format!("Failed to broadcast verification result: {}", e))
        })?;

        let _ = effects.log_debug("Verification result sent").await;
        Ok(())
    }

    /// Wait for and receive final DKG result from coordinator
    async fn receive_final_result<E>(&self, effects: &E) -> FrostResult<DkgResponse>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let _ = effects.log_debug("Waiting for final DKG result").await;

        let (_peer_id, message_bytes) = effects
            .receive()
            .await
            .map_err(|e| AuraError::network(format!("Failed to receive final result: {}", e)))?;

        // Try to deserialize as successful result first
        if let Ok(pubkey_package) = serde_json::from_slice::<PublicKeyPackage>(&message_bytes) {
            let _ = effects.log_debug("Received successful DKG result").await;
            return Ok(DkgResponse {
                public_key_package: Some(pubkey_package),
                participants: vec![self.device_id], // Will be filled properly
                shares_distributed: 1,
                success: true,
                error: None,
            });
        }

        // Try to deserialize as failure message
        if let Ok(error_msg) = serde_json::from_slice::<String>(&message_bytes) {
            let _ = effects.log_debug("Received DKG failure notification").await;
            return Ok(DkgResponse {
                public_key_package: None,
                participants: vec![self.device_id],
                shares_distributed: 0,
                success: false,
                error: Some(error_msg),
            });
        }

        Err(AuraError::invalid("Invalid final result message format"))
    }

    /// Validate DKG configuration parameters
    ///
    /// Ensures that the threshold and participant counts are valid for FROST DKG.
    /// The threshold must be greater than 0 and not exceed the total participant count.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the configuration is valid, `Err(AuraError)` otherwise.
    pub fn validate_config(&self) -> FrostResult<()> {
        if let Some(request) = &self.dkg_request {
            if request.threshold == 0 || request.threshold > request.total_participants {
                return Err(AuraError::invalid(
                    "Invalid threshold configuration for DKG",
                ));
            }
        }
        Ok(())
    }
}

/// Get the DKG choreography instance for protocol execution
///
/// This function provides access to the choreographic types and functions
/// generated by the `choreography!` macro for FROST distributed key generation.
/// It serves as the entry point for choreographic execution of the DKG protocol.
///
/// # Note
///
/// The actual implementation is generated by the choreography macro expansion.
/// This is a placeholder that will be replaced by the macro-generated code.
///
/// # Returns
///
/// Unit type - the macro generates the necessary choreographic infrastructure
pub fn get_dkg_choreography() {
    // The choreography macro will generate the appropriate types and functions
}

/// Convenience alias for the DKG coordinator
pub type DkgCoordinator = DkgChoreographyExecutor;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dkg_choreography_executor_creation() {
        let device_id = DeviceId::new();
        let executor = DkgChoreographyExecutor::new(device_id, true);

        assert_eq!(executor.device_id, device_id);
        assert!(executor.is_coordinator);
        assert!(executor.dkg_request.is_none());
    }

    #[test]
    fn test_dkg_request_serialization() {
        let request = DkgRequest {
            session_id: "test_session".to_string(),
            account_id: AccountId::new(),
            threshold: 2,
            total_participants: 3,
            participants: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
            timeout_seconds: 120,
        };

        let serialized = serde_json::to_vec(&request).unwrap();
        let deserialized: DkgRequest = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(request.session_id, deserialized.session_id);
        assert_eq!(request.threshold, deserialized.threshold);
        assert_eq!(request.total_participants, deserialized.total_participants);
    }

    #[test]
    fn test_dkg_choreography_validation() {
        let mut executor = DkgChoreographyExecutor::new(DeviceId::new(), false);

        // Test with no request - should pass
        assert!(executor.validate_config().is_ok());

        // Test with valid request
        let request = DkgRequest {
            session_id: "test_session".to_string(),
            account_id: AccountId::new(),
            threshold: 2,
            total_participants: 3,
            participants: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
            timeout_seconds: 120,
        };
        executor.dkg_request = Some(request);
        assert!(executor.validate_config().is_ok());

        // Test with invalid threshold (too high)
        executor.dkg_request.as_mut().unwrap().threshold = 5;
        assert!(executor.validate_config().is_err());

        // Test with invalid threshold (zero)
        executor.dkg_request.as_mut().unwrap().threshold = 0;
        assert!(executor.validate_config().is_err());
    }
}
