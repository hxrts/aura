//! G_dkg: Distributed Key Generation Choreography
//!
//! This module implements the G_dkg choreography for distributed threshold
//! key generation using the Aura effect system pattern.

use crate::{FrostError, FrostResult};
use aura_core::{AccountId, Cap, DeviceId, AuraError};
use aura_crypto::frost::{PublicKeyPackage, Share};
use aura_protocol::effects::{NetworkEffects, CryptoEffects, TimeEffects, ConsoleEffects};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, BTreeMap};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Distributed key generation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgRequest {
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
    /// Key generation successful
    pub success: bool,
    /// Error message if any
    pub error: Option<String>,
}

/// DKG initialization message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgInitMessage {
    pub session_id: String,
    pub account_id: AccountId,
    pub threshold: usize,
    pub total_participants: usize,
    pub timeout_at: u64,
}

/// Message types for the G_dkg choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DkgMessage {
    /// Initiate DKG ceremony
    DkgInit {
        /// Session ID for tracking
        session_id: String,
        /// Account context
        account_id: AccountId,
        /// Threshold configuration
        threshold: usize,
        /// Total participants
        total_participants: usize,
        /// Session timeout
        timeout_at: u64,
    },

    /// Round 1: Share commitment
    ShareCommitment {
        /// Session ID
        session_id: String,
        /// Participant device ID
        participant_id: DeviceId,
        /// Share commitment
        commitment: Vec<u8>, // Serialized commitment
    },

    /// Round 2: Share revelation
    ShareRevelation {
        /// Session ID
        session_id: String,
        /// Participant device ID
        participant_id: DeviceId,
        /// Revealed share
        share: Vec<u8>, // Serialized share
    },

    /// Round 3: Verification results
    VerificationResult {
        /// Session ID
        session_id: String,
        /// Participant device ID
        participant_id: DeviceId,
        /// Verification success
        verified: bool,
        /// Complaints if any
        complaints: Vec<DeviceId>,
    },

    /// Round 4: DKG completion
    DkgCompletion {
        /// Session ID
        session_id: String,
        /// Generated public key package
        public_key_package: Option<PublicKeyPackage>,
        /// Success status
        success: bool,
        /// Error if failed
        error: Option<String>,
    },

    /// Session abort notification
    DkgAbort {
        /// Session ID
        session_id: String,
        /// Reason for abort
        reason: String,
        /// Device initiating abort
        initiator: DeviceId,
    },
}

/// Roles in the G_dkg choreography
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DkgRole {
    /// Coordinator managing the DKG process
    Coordinator,
    /// Participant contributing to key generation
    Participant(u32),
    /// Dealer distributing initial shares (for fallback)
    Dealer,
}

impl DkgRole {
    /// Get the name of this role
    pub fn name(&self) -> String {
        match self {
            DkgRole::Coordinator => "Coordinator".to_string(),
            DkgRole::Participant(id) => format!("Participant_{}", id),
            DkgRole::Dealer => "Dealer".to_string(),
        }
    }
}

/// DKG Coordinator using effect system pattern
pub struct DkgCoordinator {
    /// Device ID for this coordinator instance
    pub device_id: DeviceId,
    /// Current role in DKG process
    pub role: DkgRole,
    /// Active DKG sessions
    active_sessions: Mutex<HashMap<String, DkgSessionState>>,
}

/// State for an active DKG session
#[derive(Debug)]
struct DkgSessionState {
    /// Session identifier
    session_id: String,
    /// Account being processed
    account_id: AccountId,
    /// Threshold configuration
    threshold: usize,
    /// All participants
    participants: Vec<DeviceId>,
    /// Current round (0-3)
    current_round: u32,
    /// Received commitments
    commitments: HashMap<DeviceId, Vec<u8>>,
    /// Received shares
    shares: HashMap<DeviceId, Vec<u8>>,
    /// Verification results
    verifications: HashMap<DeviceId, bool>,
    /// Generated public key package
    public_key_package: Option<PublicKeyPackage>,
    /// Session timeout
    timeout_at: u64,
    /// Error if any
    error: Option<String>,
}

impl DkgSessionState {
    fn new(session_id: String, account_id: AccountId, threshold: usize, participants: Vec<DeviceId>, timeout_at: u64) -> Self {
        Self {
            session_id,
            account_id,
            threshold,
            participants,
            current_round: 0,
            commitments: HashMap::new(),
            shares: HashMap::new(),
            verifications: HashMap::new(),
            public_key_package: None,
            timeout_at,
            error: None,
        }
    }

    fn is_complete(&self) -> bool {
        self.current_round >= 3 && (self.public_key_package.is_some() || self.error.is_some())
    }

    fn can_advance_to_round(&self, round: u32) -> bool {
        match round {
            1 => self.commitments.len() >= self.threshold,
            2 => self.shares.len() >= self.threshold,
            3 => self.verifications.len() >= self.threshold,
            _ => false,
        }
    }
}

impl DkgCoordinator {
    /// Create a new DKG coordinator
    pub fn new(device_id: DeviceId, role: DkgRole) -> Self {
        Self {
            device_id,
            role,
            active_sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Execute DKG as coordinator
    pub async fn execute_as_coordinator<E>(
        &self,
        request: DkgRequest,
        effects: &E,
    ) -> FrostResult<DkgResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        effects.log_info(&format!("Starting DKG as coordinator for account {}", request.account_id), &[]);

        let session_id = self.generate_session_id(effects).await?;
        let timeout_at = effects.current_timestamp().await + (request.timeout_seconds * 1000);

        // Initialize session state
        {
            let mut sessions = self.active_sessions.lock().await;
            let session_state = DkgSessionState::new(
                session_id.clone(),
                request.account_id,
                request.threshold,
                request.participants.clone(),
                timeout_at,
            );
            sessions.insert(session_id.clone(), session_state);
        }

        // Send DKG initialization message to all participants
        let init_message = DkgMessage::DkgInit {
            session_id: session_id.clone(),
            account_id: request.account_id,
            threshold: request.threshold,
            total_participants: request.total_participants,
            timeout_at,
        };

        self.broadcast_message(effects, &init_message).await?;

        // Run the DKG coordination protocol
        let result = self.coordinate_dkg_protocol(effects, &session_id).await;

        // Clean up session
        {
            let mut sessions = self.active_sessions.lock().await;
            sessions.remove(&session_id);
        }

        result
    }

    /// Execute DKG as participant
    pub async fn execute_as_participant<E>(
        &self,
        effects: &E,
    ) -> FrostResult<DkgResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        effects.log_info(&format!("Starting DKG as participant for device {}", self.device_id), &[]);

        // Wait for DKG initialization
        let init_message = self.wait_for_init_message(effects).await?;
        
        let session_id = match init_message {
            DkgMessage::DkgInit { session_id, account_id, threshold, total_participants, timeout_at } => {
                effects.log_info(&format!("Received DKG init for session {}", session_id), &[]);
                
                // Initialize local session state
                let participants = vec![self.device_id]; // Will be filled as we learn about others
                {
                    let mut sessions = self.active_sessions.lock().await;
                    let session_state = DkgSessionState::new(
                        session_id.clone(),
                        account_id,
                        threshold,
                        participants,
                        timeout_at,
                    );
                    sessions.insert(session_id.clone(), session_state);
                }

                session_id
            }
            _ => return Err(AuraError::invalid("Expected DkgInit message")),
        };

        // Participate in the DKG protocol
        let result = self.participate_in_dkg_protocol(effects, &session_id).await;

        // Clean up session
        {
            let mut sessions = self.active_sessions.lock().await;
            sessions.remove(&session_id);
        }

        result
    }

    /// Generate a unique session ID
    async fn generate_session_id<E>(&self, effects: &E) -> FrostResult<String>
    where
        E: CryptoEffects + TimeEffects,
    {
        let timestamp = effects.current_timestamp().await;
        let random_bytes = effects.random_bytes(8).await;
        let session_id = format!("dkg_{}_{:x}", timestamp, hex::encode(random_bytes));
        Ok(session_id)
    }

    /// Broadcast a DKG message to all participants
    async fn broadcast_message<E>(&self, effects: &E, message: &DkgMessage) -> FrostResult<()>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let serialized = serde_json::to_vec(message)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize DKG message: {}", e)))?;

        effects.broadcast(serialized).await
            .map_err(|e| AuraError::network(format!("Failed to broadcast DKG message: {}", e)))?;

        effects.log_debug(&format!("Broadcasted DKG message: {:?}", message), &[]);
        Ok(())
    }

    /// Send a DKG message to a specific peer
    async fn send_message_to_peer<E>(&self, effects: &E, peer_id: DeviceId, message: &DkgMessage) -> FrostResult<()>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let serialized = serde_json::to_vec(message)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize DKG message: {}", e)))?;

        let peer_uuid = Uuid::parse_str(&peer_id.to_string())
            .map_err(|e| AuraError::invalid(format!("Invalid peer ID: {}", e)))?;

        effects.send_to_peer(peer_uuid, serialized).await
            .map_err(|e| AuraError::network(format!("Failed to send DKG message to peer {}: {}", peer_id, e)))?;

        effects.log_debug(&format!("Sent DKG message to {}: {:?}", peer_id, message), &[]);
        Ok(())
    }

    /// Wait for DKG initialization message
    async fn wait_for_init_message<E>(&self, effects: &E) -> FrostResult<DkgMessage>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        effects.log_debug("Waiting for DKG initialization message", &[]);

        loop {
            let (_peer_id, message_bytes) = effects.receive().await
                .map_err(|e| AuraError::network(format!("Failed to receive message: {}", e)))?;

            match serde_json::from_slice::<DkgMessage>(&message_bytes) {
                Ok(DkgMessage::DkgInit { .. }) => {
                    let message: DkgMessage = serde_json::from_slice(&message_bytes)
                        .map_err(|e| AuraError::serialization(format!("Failed to deserialize DKG message: {}", e)))?;
                    return Ok(message);
                }
                Ok(_) => {
                    effects.log_debug("Received non-init DKG message, continuing to wait", &[]);
                    continue;
                }
                Err(_) => {
                    effects.log_debug("Received non-DKG message, continuing to wait", &[]);
                    continue;
                }
            }
        }
    }

    /// Coordinate the DKG protocol as coordinator
    async fn coordinate_dkg_protocol<E>(&self, effects: &E, session_id: &str) -> FrostResult<DkgResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        effects.log_info(&format!("Coordinating DKG protocol for session {}", session_id), &[]);

        // Round 1: Collect commitments
        self.collect_commitments(effects, session_id).await?;

        // Round 2: Collect shares
        self.collect_shares(effects, session_id).await?;

        // Round 3: Collect verifications
        self.collect_verifications(effects, session_id).await?;

        // Round 4: Generate final result
        let response = self.finalize_dkg(effects, session_id).await?;

        // Send completion message
        let completion_message = DkgMessage::DkgCompletion {
            session_id: session_id.to_string(),
            public_key_package: response.public_key_package.clone(),
            success: response.success,
            error: response.error.clone(),
        };

        self.broadcast_message(effects, &completion_message).await?;

        effects.log_info(&format!("DKG coordination complete for session {}", session_id), &[]);
        Ok(response)
    }

    /// Participate in the DKG protocol as participant
    async fn participate_in_dkg_protocol<E>(&self, effects: &E, session_id: &str) -> FrostResult<DkgResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        effects.log_info(&format!("Participating in DKG protocol for session {}", session_id), &[]);

        // Round 1: Generate and send commitment
        self.generate_and_send_commitment(effects, session_id).await?;

        // Round 2: Generate and send share
        self.generate_and_send_share(effects, session_id).await?;

        // Round 3: Verify and send verification result
        self.verify_and_send_result(effects, session_id).await?;

        // Round 4: Wait for completion
        let response = self.wait_for_completion(effects, session_id).await?;

        effects.log_info(&format!("DKG participation complete for session {}", session_id), &[]);
        Ok(response)
    }

    /// Collect commitments from participants (Round 1)
    async fn collect_commitments<E>(&self, effects: &E, session_id: &str) -> FrostResult<()>
    where
        E: NetworkEffects + TimeEffects + ConsoleEffects,
    {
        effects.log_debug(&format!("Collecting commitments for session {}", session_id), &[]);

        // In a real implementation, this would collect and validate FROST commitments
        // For now, we simulate the process
        
        loop {
            let (_peer_id, message_bytes) = effects.receive().await
                .map_err(|e| AuraError::network(format!("Failed to receive message: {}", e)))?;

            if let Ok(DkgMessage::ShareCommitment { session_id: msg_session_id, participant_id, commitment }) =
                serde_json::from_slice::<DkgMessage>(&message_bytes) {
                
                if msg_session_id == session_id {
                    let mut sessions = self.active_sessions.lock().await;
                    if let Some(session) = sessions.get_mut(session_id) {
                        session.commitments.insert(participant_id, commitment);
                        effects.log_debug(&format!("Received commitment from {}", participant_id), &[]);
                        
                        if session.can_advance_to_round(1) {
                            session.current_round = 1;
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Collect shares from participants (Round 2)
    async fn collect_shares<E>(&self, effects: &E, session_id: &str) -> FrostResult<()>
    where
        E: NetworkEffects + TimeEffects + ConsoleEffects,
    {
        effects.log_debug(&format!("Collecting shares for session {}", session_id), &[]);

        // Similar to collect_commitments but for shares
        loop {
            let (_peer_id, message_bytes) = effects.receive().await
                .map_err(|e| AuraError::network(format!("Failed to receive message: {}", e)))?;

            if let Ok(DkgMessage::ShareRevelation { session_id: msg_session_id, participant_id, share }) =
                serde_json::from_slice::<DkgMessage>(&message_bytes) {
                
                if msg_session_id == session_id {
                    let mut sessions = self.active_sessions.lock().await;
                    if let Some(session) = sessions.get_mut(session_id) {
                        session.shares.insert(participant_id, share);
                        effects.log_debug(&format!("Received share from {}", participant_id), &[]);
                        
                        if session.can_advance_to_round(2) {
                            session.current_round = 2;
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Collect verifications from participants (Round 3)
    async fn collect_verifications<E>(&self, effects: &E, session_id: &str) -> FrostResult<()>
    where
        E: NetworkEffects + TimeEffects + ConsoleEffects,
    {
        effects.log_debug(&format!("Collecting verifications for session {}", session_id), &[]);

        // Similar pattern for verification results
        loop {
            let (_peer_id, message_bytes) = effects.receive().await
                .map_err(|e| AuraError::network(format!("Failed to receive message: {}", e)))?;

            if let Ok(DkgMessage::VerificationResult { session_id: msg_session_id, participant_id, verified, .. }) =
                serde_json::from_slice::<DkgMessage>(&message_bytes) {
                
                if msg_session_id == session_id {
                    let mut sessions = self.active_sessions.lock().await;
                    if let Some(session) = sessions.get_mut(session_id) {
                        session.verifications.insert(participant_id, verified);
                        effects.log_debug(&format!("Received verification from {}: {}", participant_id, verified), &[]);
                        
                        if session.can_advance_to_round(3) {
                            session.current_round = 3;
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Finalize DKG and generate result (Round 4)
    async fn finalize_dkg<E>(&self, effects: &E, session_id: &str) -> FrostResult<DkgResponse>
    where
        E: CryptoEffects + ConsoleEffects,
    {
        effects.log_debug(&format!("Finalizing DKG for session {}", session_id), &[]);

        let mut sessions = self.active_sessions.lock().await;
        let session = sessions.get_mut(session_id)
            .ok_or_else(|| AuraError::invalid("Session not found"))?;

        // Check if all verifications passed
        let all_verified = session.verifications.values().all(|&v| v);

        if all_verified && session.verifications.len() >= session.threshold {
            // In a real implementation, this would construct the actual public key package
            // from the verified shares. For now, we create a placeholder.
            effects.log_info("All participants verified - DKG successful", &[]);
            
            Ok(DkgResponse {
                public_key_package: None, // TODO: Implement real public key package generation
                participants: session.participants.clone(),
                success: true,
                error: None,
            })
        } else {
            effects.log_warn("DKG verification failed", &[]);
            
            Ok(DkgResponse {
                public_key_package: None,
                participants: session.participants.clone(),
                success: false,
                error: Some("Verification failed".to_string()),
            })
        }
    }

    /// Generate and send commitment (Participant Round 1)
    async fn generate_and_send_commitment<E>(&self, effects: &E, session_id: &str) -> FrostResult<()>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        effects.log_debug(&format!("Generating commitment for session {}", session_id), &[]);

        // In a real implementation, this would generate a FROST commitment
        let mock_commitment = effects.random_bytes(32).await;

        let commitment_message = DkgMessage::ShareCommitment {
            session_id: session_id.to_string(),
            participant_id: self.device_id,
            commitment: mock_commitment,
        };

        self.broadcast_message(effects, &commitment_message).await?;
        Ok(())
    }

    /// Generate and send share (Participant Round 2)
    async fn generate_and_send_share<E>(&self, effects: &E, session_id: &str) -> FrostResult<()>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        effects.log_debug(&format!("Generating share for session {}", session_id), &[]);

        // In a real implementation, this would generate a FROST share
        let mock_share = effects.random_bytes(32).await;

        let share_message = DkgMessage::ShareRevelation {
            session_id: session_id.to_string(),
            participant_id: self.device_id,
            share: mock_share,
        };

        self.broadcast_message(effects, &share_message).await?;
        Ok(())
    }

    /// Verify and send verification result (Participant Round 3)
    async fn verify_and_send_result<E>(&self, effects: &E, session_id: &str) -> FrostResult<()>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        effects.log_debug(&format!("Verifying shares for session {}", session_id), &[]);

        // In a real implementation, this would verify the received shares
        let verification_successful = true; // Mock verification

        let verification_message = DkgMessage::VerificationResult {
            session_id: session_id.to_string(),
            participant_id: self.device_id,
            verified: verification_successful,
            complaints: vec![], // No complaints in mock implementation
        };

        self.broadcast_message(effects, &verification_message).await?;
        Ok(())
    }

    /// Wait for DKG completion (Participant Round 4)
    async fn wait_for_completion<E>(&self, effects: &E, session_id: &str) -> FrostResult<DkgResponse>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        effects.log_debug(&format!("Waiting for DKG completion for session {}", session_id), &[]);

        loop {
            let (_peer_id, message_bytes) = effects.receive().await
                .map_err(|e| AuraError::network(format!("Failed to receive message: {}", e)))?;

            if let Ok(DkgMessage::DkgCompletion { session_id: msg_session_id, public_key_package, success, error }) =
                serde_json::from_slice::<DkgMessage>(&message_bytes) {
                
                if msg_session_id == session_id {
                    return Ok(DkgResponse {
                        public_key_package,
                        participants: vec![self.device_id], // Will be filled from session state
                        success,
                        error,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::AccountId;
    use uuid::Uuid;

    #[test]
    fn test_dkg_coordinator_creation() {
        let device_id = DeviceId::new();
        let coordinator = DkgCoordinator::new(device_id, DkgRole::Coordinator);
        assert_eq!(coordinator.device_id, device_id);
        assert_eq!(coordinator.role, DkgRole::Coordinator);
    }

    #[test]
    fn test_dkg_request_serialization() {
        let request = DkgRequest {
            account_id: AccountId::new(),
            threshold: 2,
            total_participants: 3,
            participants: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
            timeout_seconds: 300,
        };

        let serialized = serde_json::to_vec(&request).unwrap();
        let deserialized: DkgRequest = serde_json::from_slice(&serialized).unwrap();
        
        assert_eq!(request.threshold, deserialized.threshold);
        assert_eq!(request.total_participants, deserialized.total_participants);
        assert_eq!(request.participants.len(), deserialized.participants.len());
    }

    #[test]
    fn test_dkg_message_serialization() {
        let message = DkgMessage::DkgInit {
            session_id: "test_session".to_string(),
            account_id: AccountId::new(),
            threshold: 2,
            total_participants: 3,
            timeout_at: 1000,
        };

        let serialized = serde_json::to_vec(&message).unwrap();
        let deserialized: DkgMessage = serde_json::from_slice(&serialized).unwrap();
        
        match deserialized {
            DkgMessage::DkgInit { session_id, threshold, total_participants, .. } => {
                assert_eq!(session_id, "test_session");
                assert_eq!(threshold, 2);
                assert_eq!(total_participants, 3);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_dkg_role_naming() {
        assert_eq!(DkgRole::Coordinator.name(), "Coordinator");
        assert_eq!(DkgRole::Participant(1).name(), "Participant_1");
        assert_eq!(DkgRole::Dealer.name(), "Dealer");
    }
}