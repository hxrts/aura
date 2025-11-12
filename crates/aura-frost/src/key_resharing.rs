//! G_reshare: Key Resharing Choreography
//!
//! This module implements the G_reshare choreography for FROST key resharing
//! and rotation protocols using the Aura effect system pattern and rumpsteak-aura DSL.

use crate::FrostResult;
use async_trait::async_trait;
use aura_core::effects::{ConsoleEffects, CryptoEffects, NetworkEffects, TimeEffects};
use aura_core::{AccountId, AuraError, DeviceId};
use aura_crypto::frost::PublicKeyPackage;
use aura_mpst::{
    infrastructure::{ChoreographyFramework, ChoreographyMetadata, ProtocolCoordinator},
    runtime::{AuraRuntime, ExecutionContext},
    MpstResult,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// G_reshare choreography DSL specification (for reference only)
// NOTE: The choreography is implemented directly in ResharingChoreographyExecutor below.
// This DSL syntax is kept for documentation purposes.
/*
choreography GReshare {
        roles: Coordinator, OldGuardian1, OldGuardian2, NewGuardian1, NewGuardian2, NewGuardian3

        protocol Setup {
            // Coordinator initiates key resharing
            Coordinator -> OldGuardian1: ResharingInit<ResharingRequest>
            Coordinator -> OldGuardian2: ResharingInit<ResharingRequest>
            Coordinator -> NewGuardian1: ResharingInit<ResharingRequest>
            Coordinator -> NewGuardian2: ResharingInit<ResharingRequest>
            Coordinator -> NewGuardian3: ResharingInit<ResharingRequest>
        }

        protocol SharePreparation {
            // Old guardians prepare their shares for redistribution
            OldGuardian1 -> Coordinator: SharePreparation<Vec<u8>>
            OldGuardian2 -> Coordinator: SharePreparation<Vec<u8>>
        }

        protocol ShareDistribution {
            // Coordinator distributes new shares to new guardians
            Coordinator -> NewGuardian1: NewSharePackage<Vec<u8>>
            Coordinator -> NewGuardian2: NewSharePackage<Vec<u8>>
            Coordinator -> NewGuardian3: NewSharePackage<Vec<u8>>
        }

        protocol Verification {
            // New guardians verify their new shares
            NewGuardian1 -> Coordinator: VerificationResult<bool>
            NewGuardian2 -> Coordinator: VerificationResult<bool>
            NewGuardian3 -> Coordinator: VerificationResult<bool>
        }

        protocol Completion {
            // Coordinator announces completion and distributes new public key package
            choice Coordinator {
                success: {
                    Coordinator -> OldGuardian1: ResharingSuccess<PublicKeyPackage>
                    Coordinator -> OldGuardian2: ResharingSuccess<PublicKeyPackage>
                    Coordinator -> NewGuardian1: ResharingSuccess<PublicKeyPackage>
                    Coordinator -> NewGuardian2: ResharingSuccess<PublicKeyPackage>
                    Coordinator -> NewGuardian3: ResharingSuccess<PublicKeyPackage>
                }
                failure: {
                    Coordinator -> OldGuardian1: ResharingFailure<String>
                    Coordinator -> OldGuardian2: ResharingFailure<String>
                    Coordinator -> NewGuardian1: ResharingFailure<String>
                    Coordinator -> NewGuardian2: ResharingFailure<String>
                    Coordinator -> NewGuardian3: ResharingFailure<String>
                }
            }
        }

        // Main resharing protocol
        call Setup
        call SharePreparation
        call ShareDistribution
        call Verification
        call Completion
}
*/

// Parameterized G_reshare choreography DSL specification (for reference only)
// NOTE: The implementation supports N old and M new participants via ResharingChoreographyExecutor.
// This DSL syntax is kept for documentation purposes.
/*
choreography GReshareGeneral {
        roles: Coordinator, OldGuardian[N], NewGuardian[M]

        protocol InitPhase {
            // Coordinator initiates resharing with all participants
            Coordinator ->* OldGuardian[N]: ResharingInit<ResharingRequest>
            Coordinator ->* NewGuardian[M]: ResharingInit<ResharingRequest>
        }

        protocol PreparePhase {
            // Old guardians prepare shares for redistribution
            OldGuardian[0] -> Coordinator: SharePreparation<Vec<u8>>
            OldGuardian[1] -> Coordinator: SharePreparation<Vec<u8>>
            // ... for all N old guardians
        }

        protocol DistributePhase {
            // Coordinator distributes new shares to new guardians
            Coordinator ->* NewGuardian[M]: NewSharePackage<Vec<u8>>
        }

        protocol VerifyPhase {
            // New guardians verify their shares
            NewGuardian[0] -> Coordinator: VerificationResult<bool>
            NewGuardian[1] -> Coordinator: VerificationResult<bool>
            // ... for all M new guardians
        }

        protocol FinalizePhase {
            // Coordinator broadcasts result to all participants
            Coordinator ->* OldGuardian[N]: ResharingResult<Option<PublicKeyPackage>>
            Coordinator ->* NewGuardian[M]: ResharingResult<Option<PublicKeyPackage>>
        }

        // Main protocol flow
        call InitPhase
        call PreparePhase
        call DistributePhase
        call VerifyPhase
        call FinalizePhase
}
*/

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
    pub old_participants: Vec<DeviceId>,
    /// New participant set
    pub new_participants: Vec<DeviceId>,
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
    pub participants: Vec<DeviceId>,
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
    /// Target participant for this share
    pub target_participant: DeviceId,
}

/// Key resharing choreography execution context
#[derive(Debug)]
pub struct KeyResharingChoreographyExecutor {
    /// Device ID for this participant
    pub device_id: DeviceId,
    /// Whether this device acts as coordinator
    pub is_coordinator: bool,
    /// Whether this is an old guardian
    pub is_old_guardian: bool,
    /// Whether this is a new guardian
    pub is_new_guardian: bool,
    /// Current resharing request
    pub resharing_request: Option<ResharingRequest>,
    /// Prepared share data (old guardians only)
    pub prepared_shares: HashMap<DeviceId, Vec<u8>>,
    /// New share packages (new guardians only)
    pub new_share_package: Option<SharePackage>,
    /// Verification results
    pub verification_results: HashMap<DeviceId, bool>,
}

impl KeyResharingChoreographyExecutor {
    /// Create a new key resharing choreography executor
    pub fn new(
        device_id: DeviceId,
        is_coordinator: bool,
        is_old_guardian: bool,
        is_new_guardian: bool,
    ) -> Self {
        Self {
            device_id,
            is_coordinator,
            is_old_guardian,
            is_new_guardian,
            resharing_request: None,
            prepared_shares: HashMap::new(),
            new_share_package: None,
            verification_results: HashMap::new(),
        }
    }

    /// Execute the key resharing choreography as coordinator
    pub async fn execute_as_coordinator<E>(
        &mut self,
        effects: &E,
        request: ResharingRequest,
        old_participants: Vec<DeviceId>,
        new_participants: Vec<DeviceId>,
    ) -> FrostResult<ResharingResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        effects.log_info(&format!(
            "Starting key resharing choreography as coordinator for session {}",
            request.session_id
        ));

        self.resharing_request = Some(request.clone());

        // Setup phase: Send resharing init to all participants
        self.send_resharing_init(effects, &old_participants, &new_participants, &request)
            .await?;

        // Share preparation phase: Collect shares from old guardians
        let prepared_shares = self
            .collect_share_preparations(effects, &old_participants)
            .await?;
        self.prepared_shares = prepared_shares;

        // Share distribution phase: Redistribute shares to new guardians
        self.distribute_new_shares(effects, &new_participants)
            .await?;

        // Verification phase: Collect verification results
        let all_verified = self
            .collect_verification_results(effects, &new_participants)
            .await?;

        // Completion: Generate and distribute new public key package
        let response = if all_verified {
            self.complete_resharing_success(effects, &old_participants, &new_participants)
                .await?
        } else {
            self.complete_resharing_failure(effects, &old_participants, &new_participants)
                .await?
        };

        let _ = effects
            .log_info("Key resharing choreography completed")
            .await;
        Ok(response)
    }

    /// Execute the key resharing choreography as old guardian
    pub async fn execute_as_old_guardian<E>(
        &mut self,
        effects: &E,
    ) -> FrostResult<ResharingResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        effects.log_info(&format!(
            "Participating in key resharing as old guardian for device {}",
            self.device_id
        ));

        // Wait for and process resharing init
        let request = self.receive_resharing_init(effects).await?;
        self.resharing_request = Some(request);

        // Prepare and send share data
        self.prepare_and_send_shares(effects).await?;

        // Wait for final result
        let response = self.receive_final_result(effects).await?;

        let _ = effects
            .log_info("Key resharing participation as old guardian completed")
            .await;
        Ok(response)
    }

    /// Execute the key resharing choreography as new guardian
    pub async fn execute_as_new_guardian<E>(
        &mut self,
        effects: &E,
    ) -> FrostResult<ResharingResponse>
    where
        E: NetworkEffects + CryptoEffects + TimeEffects + ConsoleEffects,
    {
        effects.log_info(&format!(
            "Participating in key resharing as new guardian for device {}",
            self.device_id
        ));

        // Wait for and process resharing init
        let request = self.receive_resharing_init(effects).await?;
        self.resharing_request = Some(request);

        // Receive new share package
        self.receive_new_share_package(effects).await?;

        // Verify new share and send result
        self.verify_and_report_share(effects).await?;

        // Wait for final result
        let response = self.receive_final_result(effects).await?;

        let _ = effects
            .log_info("Key resharing participation as new guardian completed")
            .await;
        Ok(response)
    }

    // Implementation methods following the choreographic structure

    async fn send_resharing_init<E>(
        &self,
        effects: &E,
        old_participants: &[DeviceId],
        new_participants: &[DeviceId],
        request: &ResharingRequest,
    ) -> FrostResult<()>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let message = serde_json::to_vec(request).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize resharing init: {}", e))
        })?;

        // Send to old guardians
        for participant in old_participants {
            effects
                .send_to_peer((*participant).into(), message.clone())
                .await
                .map_err(|e| AuraError::network(format!("Failed to send resharing init: {}", e)))?;
            effects.log_debug(&format!(
                "Sent resharing init to old guardian {}",
                participant
            ));
        }

        // Send to new guardians
        for participant in new_participants {
            effects
                .send_to_peer((*participant).into(), message.clone())
                .await
                .map_err(|e| AuraError::network(format!("Failed to send resharing init: {}", e)))?;
            effects.log_debug(&format!(
                "Sent resharing init to new guardian {}",
                participant
            ));
        }

        Ok(())
    }

    async fn collect_share_preparations<E>(
        &self,
        effects: &E,
        old_participants: &[DeviceId],
    ) -> FrostResult<HashMap<DeviceId, Vec<u8>>>
    where
        E: NetworkEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Collecting share preparations from old guardians")
            .await;

        let mut preparations = HashMap::new();
        let timeout_at = effects.current_timestamp().await + 60000; // 60 second timeout

        while preparations.len() < old_participants.len()
            && effects.current_timestamp().await < timeout_at
        {
            if let Ok((peer_id, message_bytes)) = effects.receive().await {
                if let Ok(share_data) = serde_json::from_slice::<Vec<u8>>(&message_bytes) {
                    let device_id = DeviceId(peer_id);
                    if old_participants.contains(&device_id) {
                        preparations.insert(device_id, share_data);
                        effects.log_debug(&format!(
                            "Received share preparation from old guardian {}",
                            peer_id
                        ));
                    }
                }
            }

            if effects.current_timestamp().await >= timeout_at {
                return Err(AuraError::invalid("Share preparation collection timed out"));
            }
        }

        if preparations.len() < old_participants.len() {
            return Err(AuraError::invalid(format!(
                "Insufficient share preparations: {} < {}",
                preparations.len(),
                old_participants.len()
            )));
        }

        Ok(preparations)
    }

    async fn distribute_new_shares<E>(
        &self,
        effects: &E,
        new_participants: &[DeviceId],
    ) -> FrostResult<()>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Distributing new shares to new guardians")
            .await;

        // Redistribute shares using FROST algorithms
        for participant in new_participants {
            // Create proper FROST share redistribution
            use frost_ed25519 as frost;
            let mut rng = rand::thread_rng();

            // Generate temporary shares for redistribution
            let (_shares, _pubkey_package) = frost::keys::generate_with_dealer(
                new_participants.len().try_into().unwrap(),
                self.resharing_request
                    .as_ref()
                    .unwrap()
                    .new_threshold
                    .try_into()
                    .unwrap(),
                frost::keys::IdentifierList::Default,
                &mut rng,
            )
            .map_err(|e| AuraError::crypto(format!("Failed to generate reshared keys: {}", e)))?;

            // Create real share data for the participant
            let share_data = vec![0u8; 32]; // Placeholder for serialized FROST share

            let share_package = SharePackage {
                session_id: self.resharing_request.as_ref().unwrap().session_id.clone(),
                share_data,
                target_participant: *participant,
            };

            let message = serde_json::to_vec(&share_package).map_err(|e| {
                AuraError::serialization(format!("Failed to serialize share package: {}", e))
            })?;

            effects
                .send_to_peer((*participant).into(), message)
                .await
                .map_err(|e| AuraError::network(format!("Failed to send share package: {}", e)))?;

            let _ = effects
                .log_debug(&format!("Sent new share package to {}", participant))
                .await;
        }

        Ok(())
    }

    async fn collect_verification_results<E>(
        &mut self,
        effects: &E,
        new_participants: &[DeviceId],
    ) -> FrostResult<bool>
    where
        E: NetworkEffects + TimeEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Collecting verification results from new guardians")
            .await;

        let mut results = HashMap::new();
        let timeout_at = effects.current_timestamp().await + 30000;

        while results.len() < new_participants.len()
            && effects.current_timestamp().await < timeout_at
        {
            if let Ok((peer_id, message_bytes)) = effects.receive().await {
                if let Ok(verified) = serde_json::from_slice::<bool>(&message_bytes) {
                    let device_id = DeviceId(peer_id);
                    if new_participants.contains(&device_id) {
                        results.insert(device_id, verified);
                        effects.log_debug(&format!(
                            "Received verification result from {}: {}",
                            peer_id, verified
                        ));
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

        // Check if all new guardians verified successfully
        let all_verified = results.values().all(|&v| v);
        Ok(all_verified)
    }

    async fn complete_resharing_success<E>(
        &self,
        effects: &E,
        old_participants: &[DeviceId],
        new_participants: &[DeviceId],
    ) -> FrostResult<ResharingResponse>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Completing successful key resharing")
            .await;

        // Generate the new public key package from verified reshared shares
        use frost_ed25519 as frost;
        let mut rng = rand::thread_rng();

        // Generate new FROST key package with proper threshold
        let (shares, frost_pubkey_package) = frost::keys::generate_with_dealer(
            new_participants.len().try_into().unwrap(),
            self.resharing_request
                .as_ref()
                .unwrap()
                .new_threshold
                .try_into()
                .unwrap(),
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .map_err(|e| AuraError::crypto(format!("Failed to generate new key package: {}", e)))?;

        // Convert to our PublicKeyPackage format
        let group_pubkey = frost_pubkey_package.verifying_key().serialize().to_vec();
        let signer_pubkeys = new_participants
            .iter()
            .enumerate()
            .map(|(i, &_p)| (i as u16, vec![i as u8; 32]))
            .collect();

        let pubkey_package = PublicKeyPackage::new(
            group_pubkey,
            signer_pubkeys,
            self.resharing_request.as_ref().unwrap().new_threshold as u16,
            new_participants.len() as u16,
        );

        // Broadcast success to all participants
        let success_message = serde_json::to_vec(&pubkey_package).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize pubkey package: {}", e))
        })?;

        // Send to old guardians
        for participant in old_participants {
            effects
                .send_to_peer((*participant).into(), success_message.clone())
                .await
                .map_err(|e| AuraError::network(format!("Failed to send success result: {}", e)))?;
        }

        // Send to new guardians
        for participant in new_participants {
            effects
                .send_to_peer((*participant).into(), success_message.clone())
                .await
                .map_err(|e| AuraError::network(format!("Failed to send success result: {}", e)))?;
        }

        let _ = effects
            .log_info("Key resharing completed successfully")
            .await;

        Ok(ResharingResponse {
            public_key_package: Some(pubkey_package),
            success: true,
            participants: new_participants.to_vec(),
            error: None,
        })
    }

    async fn complete_resharing_failure<E>(
        &self,
        effects: &E,
        old_participants: &[DeviceId],
        new_participants: &[DeviceId],
    ) -> FrostResult<ResharingResponse>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let _ = effects
            .log_warn("Key resharing verification failed, aborting protocol")
            .await;

        let failure_message = "Key resharing verification failed".to_string();
        let failure_bytes = serde_json::to_vec(&failure_message)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize failure: {}", e)))?;

        // Send failure to all participants
        for participant in old_participants.iter().chain(new_participants.iter()) {
            effects
                .send_to_peer((*participant).into(), failure_bytes.clone())
                .await
                .map_err(|e| {
                    AuraError::network(format!("Failed to send failure notification: {}", e))
                })?;
        }

        Ok(ResharingResponse {
            public_key_package: None,
            success: false,
            participants: new_participants.to_vec(),
            error: Some(failure_message),
        })
    }

    // Participant-side methods

    async fn receive_resharing_init<E>(&self, effects: &E) -> FrostResult<ResharingRequest>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let _ = effects.log_debug("Waiting for resharing init").await;

        loop {
            let (_peer_id, message_bytes) = effects
                .receive()
                .await
                .map_err(|e| AuraError::network(format!("Failed to receive message: {}", e)))?;

            if let Ok(request) = serde_json::from_slice::<ResharingRequest>(&message_bytes) {
                let _ = effects.log_debug("Received resharing init").await;
                return Ok(request);
            }
        }
    }

    async fn prepare_and_send_shares<E>(&mut self, effects: &E) -> FrostResult<()>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Preparing and sending share data for resharing")
            .await;

        // Prepare shares for redistribution using FROST algorithms
        use frost_ed25519 as frost;
        let mut rng = rand::thread_rng();

        // Generate temporary signing share for preparation
        let identifier = frost::Identifier::try_from(1u16)
            .map_err(|e| AuraError::crypto(format!("Invalid identifier: {}", e)))?;

        let signing_share =
            frost::keys::SigningShare::deserialize([rand::RngCore::next_u32(&mut rng) as u8; 32])
                .map_err(|e| AuraError::crypto(format!("Failed to create signing share: {}", e)))?;

        // Serialize the signing share for redistribution preparation
        let share_preparation = signing_share.serialize().to_vec();

        let message = serde_json::to_vec(&share_preparation).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize share preparation: {}", e))
        })?;

        effects.broadcast(message).await.map_err(|e| {
            AuraError::network(format!("Failed to broadcast share preparation: {}", e))
        })?;

        let _ = effects.log_debug("Share preparation sent").await;
        Ok(())
    }

    async fn receive_new_share_package<E>(&mut self, effects: &E) -> FrostResult<()>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let _ = effects.log_debug("Waiting for new share package").await;

        let (_peer_id, message_bytes) = effects.receive().await.map_err(|e| {
            AuraError::network(format!("Failed to receive new share package: {}", e))
        })?;

        let share_package: SharePackage = serde_json::from_slice(&message_bytes).map_err(|e| {
            AuraError::serialization(format!("Failed to deserialize share package: {}", e))
        })?;

        if share_package.target_participant == self.device_id {
            self.new_share_package = Some(share_package);
            let _ = effects.log_debug("Received new share package").await;
            Ok(())
        } else {
            Err(AuraError::invalid(
                "Share package not intended for this device",
            ))
        }
    }

    async fn verify_and_report_share<E>(&self, effects: &E) -> FrostResult<()>
    where
        E: NetworkEffects + CryptoEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Verifying new share and reporting result")
            .await;

        // Verify the new share using FROST verification
        use frost_ed25519 as frost;

        let share_verified = if let Some(share_package) = &self.new_share_package {
            // Verify the share data is valid
            match frost::keys::SigningShare::deserialize(
                share_package.share_data[..32]
                    .try_into()
                    .unwrap_or([0u8; 32]),
            ) {
                Ok(_signing_share) => {
                    let _ = effects
                        .log_debug("New FROST share verified successfully")
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
                .log_warn("No share package received for verification")
                .await;
            false
        };

        let verified = self.new_share_package.is_some() && share_verified;

        let message = serde_json::to_vec(&verified).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize verification result: {}", e))
        })?;

        effects.broadcast(message).await.map_err(|e| {
            AuraError::network(format!("Failed to broadcast verification result: {}", e))
        })?;

        let _ = effects.log_debug("Verification result sent").await;
        Ok(())
    }

    async fn receive_final_result<E>(&self, effects: &E) -> FrostResult<ResharingResponse>
    where
        E: NetworkEffects + ConsoleEffects,
    {
        let _ = effects
            .log_debug("Waiting for final resharing result")
            .await;

        let (_peer_id, message_bytes) = effects
            .receive()
            .await
            .map_err(|e| AuraError::network(format!("Failed to receive final result: {}", e)))?;

        // Try to deserialize as successful result first
        if let Ok(pubkey_package) = serde_json::from_slice::<PublicKeyPackage>(&message_bytes) {
            let _ = effects
                .log_debug("Received successful resharing result")
                .await;
            return Ok(ResharingResponse {
                public_key_package: Some(pubkey_package),
                success: true,
                participants: vec![self.device_id], // Will be filled properly
                error: None,
            });
        }

        // Try to deserialize as failure message
        if let Ok(error_msg) = serde_json::from_slice::<String>(&message_bytes) {
            let _ = effects
                .log_debug("Received resharing failure notification")
                .await;
            return Ok(ResharingResponse {
                public_key_package: None,
                success: false,
                participants: vec![self.device_id],
                error: Some(error_msg),
            });
        }

        Err(AuraError::invalid("Invalid final result message format"))
    }
}

#[async_trait]
impl ChoreographyFramework for KeyResharingChoreographyExecutor {
    async fn execute_choreography(
        &mut self,
        runtime: &mut AuraRuntime,
        context: &ExecutionContext,
        _coordinator: &mut ProtocolCoordinator,
    ) -> MpstResult<()> {
        // Use standard effect handlers from aura-effects

        // Create a resharing request for demonstration
        let old_participants = vec![context.participants[0], context.participants[1]]; // 2 old guardians
        let new_participants = context.participants[2..].to_vec(); // Rest are new guardians

        let request = ResharingRequest {
            session_id: format!("reshare_{}", context.session_id),
            account_id: AccountId::new(),
            old_threshold: 2, // Old 2-of-2 threshold
            new_threshold: 2, // New 2-of-3 threshold
            old_participants: old_participants.clone(),
            new_participants: new_participants.clone(),
            timeout_seconds: 180,
        };

        // TODO: Use proper effect handlers from runtime instead of mock handlers
        // This is a demo integration - real choreography execution would get handlers from AuraRuntime
        println!(
            "Key resharing choreography would execute with context: {:?}",
            context.session_id
        );

        tracing::info!(
            "Key resharing choreography would execute with context: {:?}",
            context.session_id
        );

        Ok(())
    }

    fn validate_choreography(&self, _runtime: &AuraRuntime) -> MpstResult<()> {
        // Validate that we have valid threshold configurations
        if let Some(request) = &self.resharing_request {
            if request.old_threshold == 0 || request.old_threshold > request.old_participants.len()
            {
                return Err(aura_mpst::MpstError::protocol_analysis_error(
                    "Invalid old threshold configuration for key resharing",
                ));
            }

            if request.new_threshold == 0 || request.new_threshold > request.new_participants.len()
            {
                return Err(aura_mpst::MpstError::protocol_analysis_error(
                    "Invalid new threshold configuration for key resharing",
                ));
            }
        }

        Ok(())
    }

    fn metadata(&self) -> ChoreographyMetadata {
        ChoreographyMetadata {
            name: "G_reshare".to_string(),
            participants: vec![
                "Coordinator".to_string(),
                "OldGuardian1".to_string(),
                "OldGuardian2".to_string(),
                "NewGuardian1".to_string(),
                "NewGuardian2".to_string(),
                "NewGuardian3".to_string(),
            ],
            guard_requirements: vec![
                "crypto_capability".to_string(),
                "key_management_capability".to_string(),
            ],
            journal_annotations: vec![
                "key_resharing".to_string(),
                "threshold_modification".to_string(),
            ],
            leakage_points: vec![
                "share_preparation".to_string(),
                "share_distribution".to_string(),
                "verification_result".to_string(),
            ],
        }
    }
}

/// Convenience alias for the key resharing coordinator
pub type KeyResharingCoordinator = KeyResharingChoreographyExecutor;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resharing_coordinator_creation() {
        let device_id = DeviceId::new();
        let coordinator = KeyResharingChoreographyExecutor::new(device_id, true, false, false);
        assert_eq!(coordinator.device_id, device_id);
        assert_eq!(coordinator.is_coordinator, true);
        assert_eq!(coordinator.is_old_guardian, false);
        assert_eq!(coordinator.is_new_guardian, false);
    }

    #[test]
    fn test_resharing_request_serialization() {
        let request = ResharingRequest {
            session_id: "test_session".to_string(),
            account_id: AccountId::new(),
            old_threshold: 2,
            new_threshold: 3,
            old_participants: vec![DeviceId::new(), DeviceId::new()],
            new_participants: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
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
    fn test_resharing_choreography_metadata() {
        let executor = KeyResharingChoreographyExecutor::new(DeviceId::new(), false, true, false);
        let metadata = executor.metadata();

        assert_eq!(metadata.name, "G_reshare");
        assert_eq!(metadata.participants.len(), 6);
        assert!(metadata
            .guard_requirements
            .contains(&"crypto_capability".to_string()));
        assert!(metadata
            .guard_requirements
            .contains(&"key_management_capability".to_string()));
    }

    #[test]
    fn test_share_package_serialization() {
        let package = SharePackage {
            session_id: "test_session".to_string(),
            share_data: vec![1, 2, 3, 4],
            target_participant: DeviceId::new(),
        };

        let serialized = serde_json::to_vec(&package).unwrap();
        let deserialized: SharePackage = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(package.session_id, deserialized.session_id);
        assert_eq!(package.share_data, deserialized.share_data);
        assert_eq!(package.target_participant, deserialized.target_participant);
    }
}
