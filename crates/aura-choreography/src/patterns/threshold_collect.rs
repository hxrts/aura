//! Generic ThresholdCollect choreographic pattern
//!
//! This module provides a reusable choreographic pattern for threshold operations
//! that follow the common flow:
//! 1. Agree on a context/message
//! 2. Exchange cryptographic materials (shares/nonces/commitments)
//! 3. Aggregate the results locally
//! 4. Verify consistency across participants
//!
//! The pattern is parameterized by:
//! - The type of share/material to be exchanged
//! - The logic for generating the share
//! - The logic for aggregating the shares
//! - The logic for verifying the final result
//!
//! This enables reuse across DKD, FROST, and other threshold protocols.

use aura_protocol::effects::choreographic::ChoreographicRole;
use aura_protocol::effects::Effects;
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::time::Duration;

/// Generic message type for threshold collect operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    bound = "TContext: Clone + Serialize + DeserializeOwned + Debug, TMaterial: Clone + Serialize + DeserializeOwned + Debug"
)]
pub enum ThresholdCollectMessage<TContext, TMaterial>
where
    TContext: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    TMaterial: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
{
    /// Initialize the threshold operation with context
    InitiateOperation {
        context: TContext,
        participants: Vec<ChoreographicRole>,
        threshold: u16,
        epoch: u64,
        operation_id: String,
    },
    /// Response confirming participation
    ParticipationConfirm {
        participant: ChoreographicRole,
        context_hash: [u8; 32],
        epoch: u64,
        operation_id: String,
    },
    /// Exchange cryptographic material
    MaterialExchange {
        participant: ChoreographicRole,
        material: TMaterial,
        material_hash: [u8; 32],
        epoch: u64,
        operation_id: String,
    },
    /// Final result verification
    ResultVerification {
        participant: ChoreographicRole,
        result_hash: [u8; 32],
        success: bool,
        epoch: u64,
        operation_id: String,
    },
}

/// Configuration for threshold collect operations
#[derive(Debug, Clone)]
pub struct ThresholdCollectConfig {
    /// Required threshold for operation
    pub threshold: u16,
    /// Timeout for each phase
    pub phase_timeout_seconds: u64,
    /// Maximum number of participants
    pub max_participants: usize,
    /// Enable Byzantine fault tolerance checks
    pub enable_byzantine_detection: bool,
    /// Epoch for anti-replay protection
    pub epoch: u64,
}

/// Result of threshold collect operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "TResult: Clone + Serialize + DeserializeOwned + Debug")]
pub struct ThresholdCollectResult<TResult>
where
    TResult: Clone + Serialize + DeserializeOwned + Debug,
{
    /// Final aggregated result
    pub result: TResult,
    /// Participants who contributed
    pub participants: Vec<ChoreographicRole>,
    /// Operation epoch
    pub epoch: u64,
    /// Success status
    pub success: bool,
    /// Operation duration
    pub duration_ms: u64,
}

/// Trait for threshold operation providers
pub trait ThresholdOperationProvider<TContext, TMaterial, TResult>
where
    TContext: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    TMaterial: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    TResult: Clone + Serialize + DeserializeOwned + Debug,
{
    /// Validate the operation context
    fn validate_context(&self, context: &TContext) -> Result<(), String>;

    /// Generate cryptographic material for this participant
    fn generate_material(
        &self,
        context: &TContext,
        participant: ChoreographicRole,
        effects: &Effects,
    ) -> Result<TMaterial, String>;

    /// Validate received material from another participant
    fn validate_material(
        &self,
        context: &TContext,
        participant: ChoreographicRole,
        material: &TMaterial,
        effects: &Effects,
    ) -> Result<(), String>;

    /// Aggregate all collected materials into final result
    fn aggregate_materials(
        &self,
        context: &TContext,
        materials: &BTreeMap<ChoreographicRole, TMaterial>,
        effects: &Effects,
    ) -> Result<TResult, String>;

    /// Verify the final result is valid
    fn verify_result(
        &self,
        context: &TContext,
        result: &TResult,
        participants: &[ChoreographicRole],
        effects: &Effects,
    ) -> Result<bool, String>;

    /// Get operation name for logging
    fn operation_name(&self) -> &str;
}

/// Generic threshold collect choreography
pub struct ThresholdCollectChoreography<TContext, TMaterial, TResult, TProvider>
where
    TContext: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    TMaterial: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    TResult: Clone + Serialize + DeserializeOwned + Debug,
    TProvider: ThresholdOperationProvider<TContext, TMaterial, TResult>,
{
    config: ThresholdCollectConfig,
    context: TContext,
    participants: Vec<ChoreographicRole>,
    provider: TProvider,
    operation_id: String,
    _phantom_material: PhantomData<TMaterial>,
    _phantom_result: PhantomData<TResult>,
    effects: Effects,
}

impl<TContext, TMaterial, TResult, TProvider>
    ThresholdCollectChoreography<TContext, TMaterial, TResult, TProvider>
where
    TContext: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    TMaterial: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    TResult: Clone + Serialize + DeserializeOwned + Debug,
    TProvider: ThresholdOperationProvider<TContext, TMaterial, TResult>,
{
    /// Create new threshold collect choreography
    pub fn new(
        config: ThresholdCollectConfig,
        context: TContext,
        participants: Vec<ChoreographicRole>,
        provider: TProvider,
        effects: Effects,
    ) -> Result<Self, ChoreographyError> {
        if participants.len() < config.threshold as usize {
            return Err(ChoreographyError::ProtocolViolation(format!(
                "Insufficient participants: {} < {}",
                participants.len(),
                config.threshold
            )));
        }

        if participants.len() > config.max_participants {
            return Err(ChoreographyError::ProtocolViolation(format!(
                "Too many participants: {} > {}",
                participants.len(),
                config.max_participants
            )));
        }

        // Validate context with provider
        provider
            .validate_context(&context)
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Invalid context: {}", e)))?;

        let operation_id = uuid::Uuid::new_v4().to_string();

        Ok(Self {
            config,
            context,
            participants,
            provider,
            operation_id,
            _phantom_material: PhantomData,
            _phantom_result: PhantomData,
            effects,
        })
    }

    /// Execute the threshold collect choreography
    pub async fn execute<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        my_role: ChoreographicRole,
    ) -> Result<ThresholdCollectResult<TResult>, ChoreographyError> {
        let start_time = tokio::time::Instant::now();
        let phase_timeout = Duration::from_secs(self.config.phase_timeout_seconds);

        tracing::info!(
            operation = self.provider.operation_name(),
            operation_id = self.operation_id,
            participant = ?my_role,
            "Starting threshold collect choreography"
        );

        // Phase 1: Context agreement and participation confirmation
        let context_hash = self.hash_context(&self.context)?;

        let initiate_msg: ThresholdCollectMessage<TContext, TMaterial> = ThresholdCollectMessage::InitiateOperation {
            context: self.context.clone(),
            participants: self.participants.clone(),
            threshold: self.config.threshold,
            epoch: self.config.epoch,
            operation_id: self.operation_id.clone(),
        };

        // Broadcast initiation to all participants
        for participant in &self.participants {
            if *participant != my_role {
                handler.send(endpoint, *participant, &initiate_msg).await?;
            }
        }

        // Collect participation confirmations
        let mut confirmed_participants = vec![my_role];

        for participant in &self.participants {
            if *participant != my_role {
                self.check_timeout(start_time, phase_timeout)?;

                let received: ThresholdCollectMessage<TContext, TMaterial> =
                    handler.recv(endpoint, *participant).await?;

                if let ThresholdCollectMessage::ParticipationConfirm {
                    participant: recv_participant,
                    context_hash: recv_hash,
                    epoch,
                    operation_id,
                } = received
                {
                    // Verify epoch and operation ID
                    if epoch != self.config.epoch {
                        return Err(ChoreographyError::ProtocolViolation(
                            "Epoch mismatch".to_string(),
                        ));
                    }

                    if operation_id != self.operation_id {
                        return Err(ChoreographyError::ProtocolViolation(
                            "Operation ID mismatch".to_string(),
                        ));
                    }

                    // Verify context hash
                    if recv_hash != context_hash {
                        if self.config.enable_byzantine_detection {
                            tracing::warn!(
                                operation_id = self.operation_id,
                                participant = ?recv_participant,
                                "Context hash mismatch - potential Byzantine behavior"
                            );
                        }
                        return Err(ChoreographyError::ProtocolViolation(
                            "Context agreement failed".to_string(),
                        ));
                    }

                    confirmed_participants.push(recv_participant);
                }
            }
        }

        // Send own confirmation
        let confirm_msg: ThresholdCollectMessage<TContext, TMaterial> = ThresholdCollectMessage::ParticipationConfirm {
            participant: my_role,
            context_hash,
            epoch: self.config.epoch,
            operation_id: self.operation_id.clone(),
        };

        for participant in &self.participants {
            if *participant != my_role {
                handler.send(endpoint, *participant, &confirm_msg).await?;
            }
        }

        tracing::debug!(
            operation_id = self.operation_id,
            confirmed_count = confirmed_participants.len(),
            threshold = self.config.threshold,
            "Phase 1 complete: Context agreement"
        );

        // Phase 2: Material generation and exchange
        let my_material = self
            .provider
            .generate_material(&self.context, my_role, &self.effects)
            .map_err(|e| {
                ChoreographyError::ProtocolViolation(format!("Material generation failed: {}", e))
            })?;

        let material_hash = self.hash_material(&my_material)?;

        let material_msg: ThresholdCollectMessage<TContext, TMaterial> = ThresholdCollectMessage::MaterialExchange {
            participant: my_role,
            material: my_material.clone(),
            material_hash,
            epoch: self.config.epoch,
            operation_id: self.operation_id.clone(),
        };

        // Broadcast material to all participants
        for participant in &confirmed_participants {
            if *participant != my_role {
                handler.send(endpoint, *participant, &material_msg).await?;
            }
        }

        // Collect materials from all participants
        let mut all_materials = BTreeMap::new();
        all_materials.insert(my_role, my_material);

        for participant in &confirmed_participants {
            if *participant != my_role {
                self.check_timeout(start_time, phase_timeout)?;

                let received: ThresholdCollectMessage<TContext, TMaterial> =
                    handler.recv(endpoint, *participant).await?;

                if let ThresholdCollectMessage::MaterialExchange {
                    participant: recv_participant,
                    material,
                    material_hash: recv_hash,
                    epoch,
                    operation_id,
                } = received
                {
                    // Verify epoch and operation ID
                    if epoch != self.config.epoch {
                        return Err(ChoreographyError::ProtocolViolation(
                            "Epoch mismatch".to_string(),
                        ));
                    }

                    if operation_id != self.operation_id {
                        return Err(ChoreographyError::ProtocolViolation(
                            "Operation ID mismatch".to_string(),
                        ));
                    }

                    // Verify material hash
                    let expected_hash = self.hash_material(&material)?;
                    if recv_hash != expected_hash {
                        if self.config.enable_byzantine_detection {
                            tracing::warn!(
                                operation_id = self.operation_id,
                                participant = ?recv_participant,
                                "Material hash mismatch - potential Byzantine behavior"
                            );
                        }
                        return Err(ChoreographyError::ProtocolViolation(
                            "Material integrity check failed".to_string(),
                        ));
                    }

                    // Validate material with provider
                    self.provider
                        .validate_material(
                            &self.context,
                            recv_participant,
                            &material,
                            &self.effects,
                        )
                        .map_err(|e| {
                            ChoreographyError::ProtocolViolation(format!(
                                "Material validation failed: {}",
                                e
                            ))
                        })?;

                    all_materials.insert(recv_participant, material);
                }
            }
        }

        tracing::debug!(
            operation_id = self.operation_id,
            materials_count = all_materials.len(),
            "Phase 2 complete: Material exchange"
        );

        // Phase 3: Local aggregation
        let aggregated_result = self
            .provider
            .aggregate_materials(&self.context, &all_materials, &self.effects)
            .map_err(|e| {
                ChoreographyError::ProtocolViolation(format!("Aggregation failed: {}", e))
            })?;

        tracing::debug!(
            operation_id = self.operation_id,
            "Phase 3 complete: Local aggregation"
        );

        // Phase 4: Result verification and consistency check
        let is_valid = self
            .provider
            .verify_result(
                &self.context,
                &aggregated_result,
                &confirmed_participants,
                &self.effects,
            )
            .map_err(|e| {
                ChoreographyError::ProtocolViolation(format!("Result verification failed: {}", e))
            })?;

        let result_hash = self.hash_result(&aggregated_result)?;

        let verification_msg: ThresholdCollectMessage<TContext, TMaterial> = ThresholdCollectMessage::ResultVerification {
            participant: my_role,
            result_hash,
            success: is_valid,
            epoch: self.config.epoch,
            operation_id: self.operation_id.clone(),
        };

        // Broadcast verification result
        for participant in &confirmed_participants {
            if *participant != my_role {
                handler
                    .send(endpoint, *participant, &verification_msg)
                    .await?;
            }
        }

        // Collect verification results from all participants
        let mut verification_results = BTreeMap::new();
        verification_results.insert(my_role, (result_hash, is_valid));

        for participant in &confirmed_participants {
            if *participant != my_role {
                self.check_timeout(start_time, phase_timeout)?;

                let received: ThresholdCollectMessage<TContext, TMaterial> =
                    handler.recv(endpoint, *participant).await?;

                if let ThresholdCollectMessage::ResultVerification {
                    participant: recv_participant,
                    result_hash: recv_hash,
                    success,
                    epoch,
                    operation_id,
                } = received
                {
                    // Verify epoch and operation ID
                    if epoch != self.config.epoch {
                        return Err(ChoreographyError::ProtocolViolation(
                            "Epoch mismatch".to_string(),
                        ));
                    }

                    if operation_id != self.operation_id {
                        return Err(ChoreographyError::ProtocolViolation(
                            "Operation ID mismatch".to_string(),
                        ));
                    }

                    verification_results.insert(recv_participant, (recv_hash, success));
                }
            }
        }

        // Check for consistency across all participants
        let mut consistent_hash = None;
        let mut success_count = 0;

        for (participant, (hash, success)) in &verification_results {
            if *success {
                success_count += 1;

                if let Some(expected_hash) = consistent_hash {
                    if *hash != expected_hash {
                        if self.config.enable_byzantine_detection {
                            tracing::warn!(
                                operation_id = self.operation_id,
                                participant = ?participant,
                                "Result hash inconsistency - potential Byzantine behavior"
                            );
                        }
                        return Err(ChoreographyError::ProtocolViolation(
                            "Result consistency check failed".to_string(),
                        ));
                    }
                } else {
                    consistent_hash = Some(*hash);
                }
            }
        }

        // Check if we have sufficient successful results
        if success_count < self.config.threshold as usize {
            return Err(ChoreographyError::ProtocolViolation(format!(
                "Insufficient successful results: {} < {}",
                success_count, self.config.threshold
            )));
        }

        let duration = start_time.elapsed();
        let overall_success = is_valid && success_count >= self.config.threshold as usize;

        tracing::info!(
            operation = self.provider.operation_name(),
            operation_id = self.operation_id,
            success = overall_success,
            duration_ms = duration.as_millis(),
            participants = confirmed_participants.len(),
            "Threshold collect choreography completed"
        );

        Ok(ThresholdCollectResult {
            result: aggregated_result,
            participants: confirmed_participants,
            epoch: self.config.epoch,
            success: overall_success,
            duration_ms: duration.as_millis() as u64,
        })
    }

    fn check_timeout(
        &self,
        start_time: tokio::time::Instant,
        timeout: Duration,
    ) -> Result<(), ChoreographyError> {
        if start_time.elapsed() > timeout {
            Err(ChoreographyError::ProtocolViolation(format!(
                "Operation {} timed out after {}s",
                self.operation_id,
                timeout.as_secs()
            )))
        } else {
            Ok(())
        }
    }

    fn hash_context(&self, context: &TContext) -> Result<[u8; 32], ChoreographyError> {
        let serialized = bincode::serialize(context).map_err(|e| {
            ChoreographyError::ProtocolViolation(format!("Context serialization failed: {}", e))
        })?;
        Ok(self.effects.blake3_hash(&serialized))
    }

    fn hash_material(&self, material: &TMaterial) -> Result<[u8; 32], ChoreographyError> {
        let serialized = bincode::serialize(material).map_err(|e| {
            ChoreographyError::ProtocolViolation(format!("Material serialization failed: {}", e))
        })?;
        Ok(self.effects.blake3_hash(&serialized))
    }

    fn hash_result(&self, result: &TResult) -> Result<[u8; 32], ChoreographyError> {
        let serialized = bincode::serialize(result).map_err(|e| {
            ChoreographyError::ProtocolViolation(format!("Result serialization failed: {}", e))
        })?;
        Ok(self.effects.blake3_hash(&serialized))
    }
}

/// Default configuration for threshold collect operations
impl Default for ThresholdCollectConfig {
    fn default() -> Self {
        Self {
            threshold: 2,
            phase_timeout_seconds: 30,
            max_participants: 100,
            enable_byzantine_detection: true,
            epoch: 0,
        }
    }
}
