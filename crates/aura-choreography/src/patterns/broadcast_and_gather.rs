//! All-to-All Broadcast and Gather choreographic pattern
//!
//! This is the most fundamental pattern in peer-to-peer protocols where every
//! participant sends a message of the same type to every other participant,
//! and then waits until they have received a message from all other N-1 participants.
//!
//! Used extensively in:
//! - FROST signing for exchanging nonce commitments and signature shares
//! - DKD protocols for exchanging key derivation shares
//! - Any protocol requiring synchronized information exchange

use aura_protocol::effects::choreographic::ChoreographicRole;
use aura_protocol::effects::Effects;
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::time::Duration;

/// Configuration for broadcast and gather operations
#[derive(Debug, Clone)]
pub struct BroadcastGatherConfig {
    /// Timeout for the gather phase
    pub gather_timeout_seconds: u64,
    /// Enable message ordering verification
    pub verify_message_ordering: bool,
    /// Enable duplicate message detection
    pub detect_duplicates: bool,
    /// Maximum message size in bytes
    pub max_message_size: usize,
    /// Epoch for anti-replay protection
    pub epoch: u64,
}

impl Default for BroadcastGatherConfig {
    fn default() -> Self {
        Self {
            gather_timeout_seconds: 30,
            verify_message_ordering: true,
            detect_duplicates: true,
            max_message_size: 1024 * 1024, // 1MB
            epoch: 0,
        }
    }
}

/// Result of broadcast and gather operation
#[derive(Debug, Clone)]
#[derive(Serialize, Deserialize)]
#[serde(bound = "T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync")]
pub struct BroadcastGatherResult<T>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
{
    /// Messages gathered from all participants
    pub messages: BTreeMap<ChoreographicRole, T>,
    /// Number of participants that contributed
    pub participant_count: usize,
    /// Total time taken for the operation
    pub duration_ms: u64,
    /// Success status
    pub success: bool,
}

/// Message wrapper for broadcast and gather
#[derive(Debug, Clone)]
#[derive(Serialize, Deserialize)]
#[serde(bound = "T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync")]
pub struct BroadcastMessage<T>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
{
    /// The actual message payload
    pub payload: T,
    /// Sender role
    pub sender: ChoreographicRole,
    /// Message sequence number
    pub sequence: u64,
    /// Epoch for anti-replay protection
    pub epoch: u64,
    /// Message hash for integrity
    pub message_hash: [u8; 32],
}

/// Trait for customizing message validation in broadcast and gather
pub trait MessageValidator<T>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
{
    /// Validate a message before sending
    fn validate_outgoing(&self, message: &T, sender: ChoreographicRole) -> Result<(), String>;
    
    /// Validate a received message
    fn validate_incoming(&self, message: &T, sender: ChoreographicRole, receiver: ChoreographicRole) -> Result<(), String>;
    
    /// Check if messages can be processed out of order
    fn allows_out_of_order(&self) -> bool {
        false
    }
}

/// Default validator that accepts all messages
pub struct DefaultMessageValidator;

impl<T> MessageValidator<T> for DefaultMessageValidator
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
{
    fn validate_outgoing(&self, _message: &T, _sender: ChoreographicRole) -> Result<(), String> {
        Ok(())
    }
    
    fn validate_incoming(&self, _message: &T, _sender: ChoreographicRole, _receiver: ChoreographicRole) -> Result<(), String> {
        Ok(())
    }
}

/// All-to-All Broadcast and Gather choreography
pub struct BroadcastAndGatherChoreography<T, V>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    V: MessageValidator<T>,
{
    config: BroadcastGatherConfig,
    participants: Vec<ChoreographicRole>,
    validator: V,
    effects: Effects,
    operation_id: String,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, V> BroadcastAndGatherChoreography<T, V>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    V: MessageValidator<T>,
{
    /// Create new broadcast and gather choreography
    pub fn new(
        config: BroadcastGatherConfig,
        participants: Vec<ChoreographicRole>,
        validator: V,
        effects: Effects,
    ) -> Result<Self, ChoreographyError> {
        if participants.is_empty() {
            return Err(ChoreographyError::ProtocolViolation(
                "At least one participant required".to_string()
            ));
        }

        let operation_id = uuid::Uuid::new_v4().to_string();

        Ok(Self {
            config,
            participants,
            validator,
            effects,
            operation_id,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Execute broadcast and gather with a message generator
    pub async fn execute<H: ChoreoHandler<Role = ChoreographicRole>, F>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        my_role: ChoreographicRole,
        message_generator: F,
    ) -> Result<BroadcastGatherResult<T>, ChoreographyError>
    where
        F: FnOnce(ChoreographicRole, &Effects) -> Result<T, String>,
    {
        let start_time = tokio::time::Instant::now();
        let timeout = Duration::from_secs(self.config.gather_timeout_seconds);

        tracing::debug!(
            operation_id = self.operation_id,
            participant = ?my_role,
            participant_count = self.participants.len(),
            "Starting broadcast and gather"
        );

        // Generate my message
        let my_message = message_generator(my_role, &self.effects)
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Message generation failed: {}", e)))?;

        // Validate outgoing message
        self.validator.validate_outgoing(&my_message, my_role)
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Outgoing message validation failed: {}", e)))?;

        // Check message size
        let message_size = bincode::serialize(&my_message)
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Message serialization failed: {}", e)))?
            .len();
        
        if message_size > self.config.max_message_size {
            return Err(ChoreographyError::ProtocolViolation(
                format!("Message too large: {} > {}", message_size, self.config.max_message_size)
            ));
        }

        // Create broadcast message wrapper
        let message_hash = self.compute_message_hash(&my_message)?;
        let broadcast_msg = BroadcastMessage {
            payload: my_message.clone(),
            sender: my_role,
            sequence: 0, // Could be used for ordering in future
            epoch: self.config.epoch,
            message_hash,
        };

        // Phase 1: Broadcast my message to all other participants
        let mut sent_count = 0;
        for participant in &self.participants {
            if *participant != my_role {
                tracing::trace!(
                    operation_id = self.operation_id,
                    from = ?my_role,
                    to = ?participant,
                    "Broadcasting message"
                );
                
                handler.send(endpoint, *participant, &broadcast_msg).await?;
                sent_count += 1;
            }
        }

        tracing::debug!(
            operation_id = self.operation_id,
            sent_count = sent_count,
            "Broadcast phase complete"
        );

        // Phase 2: Gather messages from all other participants
        let mut gathered_messages = BTreeMap::new();
        gathered_messages.insert(my_role, my_message); // Include my own message

        let mut received_count = 0;
        let expected_count = self.participants.len() - 1; // Everyone except myself

        for participant in &self.participants {
            if *participant != my_role {
                // Check timeout
                if start_time.elapsed() > timeout {
                    return Err(ChoreographyError::ProtocolViolation(format!(
                        "Gather timeout after {}s, received {}/{} messages",
                        timeout.as_secs(), received_count, expected_count
                    )));
                }

                tracing::trace!(
                    operation_id = self.operation_id,
                    from = ?participant,
                    to = ?my_role,
                    "Waiting for message"
                );

                let received: BroadcastMessage<T> = handler.recv(endpoint, *participant).await?;

                // Verify epoch
                if received.epoch != self.config.epoch {
                    tracing::warn!(
                        operation_id = self.operation_id,
                        expected_epoch = self.config.epoch,
                        received_epoch = received.epoch,
                        sender = ?participant,
                        "Epoch mismatch"
                    );
                    return Err(ChoreographyError::ProtocolViolation("Epoch mismatch".to_string()));
                }

                // Verify sender matches
                if received.sender != *participant {
                    tracing::warn!(
                        operation_id = self.operation_id,
                        expected_sender = ?participant,
                        claimed_sender = ?received.sender,
                        "Sender mismatch"
                    );
                    return Err(ChoreographyError::ProtocolViolation("Sender identity mismatch".to_string()));
                }

                // Verify message integrity
                let expected_hash = self.compute_message_hash(&received.payload)?;
                if received.message_hash != expected_hash {
                    tracing::warn!(
                        operation_id = self.operation_id,
                        sender = ?participant,
                        "Message integrity check failed"
                    );
                    return Err(ChoreographyError::ProtocolViolation("Message integrity check failed".to_string()));
                }

                // Check for duplicates if enabled
                if self.config.detect_duplicates && gathered_messages.contains_key(participant) {
                    tracing::warn!(
                        operation_id = self.operation_id,
                        sender = ?participant,
                        "Duplicate message detected"
                    );
                    return Err(ChoreographyError::ProtocolViolation("Duplicate message detected".to_string()));
                }

                // Validate incoming message
                self.validator.validate_incoming(&received.payload, *participant, my_role)
                    .map_err(|e| ChoreographyError::ProtocolViolation(format!("Incoming message validation failed: {}", e)))?;

                gathered_messages.insert(*participant, received.payload);
                received_count += 1;

                tracing::trace!(
                    operation_id = self.operation_id,
                    sender = ?participant,
                    progress = format!("{}/{}", received_count, expected_count),
                    "Message received"
                );
            }
        }

        let duration = start_time.elapsed();
        let success = gathered_messages.len() == self.participants.len();

        tracing::info!(
            operation_id = self.operation_id,
            participant = ?my_role,
            gathered_count = gathered_messages.len(),
            expected_count = self.participants.len(),
            duration_ms = duration.as_millis(),
            success = success,
            "Broadcast and gather completed"
        );

        Ok(BroadcastGatherResult {
            messages: gathered_messages.clone(),
            participant_count: gathered_messages.len(),
            duration_ms: duration.as_millis() as u64,
            success,
        })
    }

    /// Execute with a pre-computed message
    pub async fn execute_with_message<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        my_role: ChoreographicRole,
        message: T,
    ) -> Result<BroadcastGatherResult<T>, ChoreographyError> {
        self.execute(handler, endpoint, my_role, |_role, _effects| Ok(message)).await
    }

    fn compute_message_hash(&self, message: &T) -> Result<[u8; 32], ChoreographyError> {
        let serialized = bincode::serialize(message)
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Message serialization failed: {}", e)))?;
        Ok(self.effects.blake3_hash(&serialized))
    }
}

/// Convenience function for simple broadcast and gather
pub async fn broadcast_and_gather<T, H, F>(
    handler: &mut H,
    endpoint: &mut H::Endpoint,
    participants: Vec<ChoreographicRole>,
    my_role: ChoreographicRole,
    message_generator: F,
    effects: Effects,
) -> Result<BTreeMap<ChoreographicRole, T>, ChoreographyError>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    H: ChoreoHandler<Role = ChoreographicRole>,
    F: FnOnce(ChoreographicRole, &Effects) -> Result<T, String>,
{
    let config = BroadcastGatherConfig::default();
    let validator = DefaultMessageValidator;
    
    let choreography = BroadcastAndGatherChoreography::new(
        config,
        participants,
        validator,
        effects,
    )?;
    
    let result = choreography.execute(handler, endpoint, my_role, message_generator).await?;
    Ok(result.messages)
}

/// Convenience function for simple broadcast and gather with pre-computed message
pub async fn broadcast_and_gather_message<T, H>(
    handler: &mut H,
    endpoint: &mut H::Endpoint,
    participants: Vec<ChoreographicRole>,
    my_role: ChoreographicRole,
    message: T,
    effects: Effects,
) -> Result<BTreeMap<ChoreographicRole, T>, ChoreographyError>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    H: ChoreoHandler<Role = ChoreographicRole>,
{
    let config = BroadcastGatherConfig::default();
    let validator = DefaultMessageValidator;
    
    let choreography = BroadcastAndGatherChoreography::new(
        config,
        participants,
        validator,
        effects,
    )?;
    
    let result = choreography.execute_with_message(handler, endpoint, my_role, message).await?;
    Ok(result.messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestMessage {
        content: String,
        value: u32,
    }

    #[tokio::test]
    async fn test_broadcast_gather_creation() {
        let effects = Effects::test(42);
        let participants = vec![
            ChoreographicRole { device_id: Uuid::new_v4(), role_index: 0 },
            ChoreographicRole { device_id: Uuid::new_v4(), role_index: 1 },
        ];
        
        let config = BroadcastGatherConfig::default();
        let validator = DefaultMessageValidator;
        
        let choreography = BroadcastAndGatherChoreography::<TestMessage, _>::new(
            config,
            participants,
            validator,
            effects,
        );
        
        assert!(choreography.is_ok());
    }

    #[test]
    fn test_message_validator() {
        let validator = DefaultMessageValidator;
        let role = ChoreographicRole { device_id: Uuid::new_v4(), role_index: 0 };
        let message = TestMessage {
            content: "test".to_string(),
            value: 42,
        };
        
        assert!(validator.validate_outgoing(&message, role).is_ok());
        assert!(validator.validate_incoming(&message, role, role).is_ok());
        assert!(!validator.allows_out_of_order());
    }
}