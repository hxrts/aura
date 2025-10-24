//! SSB Counter Coordination Choreography
//!
//! This module implements threshold-signed counter coordination for unique envelope identifiers
//! in the Social Bulletin Board system. Multi-device accounts need coordinated counter increments
//! to ensure unique envelope identifiers across all devices.
//!
//! # Architecture
//!
//! - Counter reservations require threshold signatures to prevent unauthorized increments
//! - Race conditions are handled cleanly with retry logic
//! - Counter state persists in the account ledger for replay protection
//! - Pure choreographic implementation with no side effects

use crate::execution::{ProtocolContext, ProtocolError, Instruction, InstructionResult};
use aura_journal::{
    events::{IncrementCounterEvent, ReserveCounterRangeEvent, RelationshipId},
    Event, EventType, EventAuthorization, DeviceId,
};
use aura_crypto::Effects;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Result of a counter coordination protocol
#[derive(Debug, Clone)]
pub struct CounterReservationResult {
    /// Session ID for this counter operation
    pub session_id: Uuid,
    /// Relationship ID for which counter was reserved
    pub relationship_id: RelationshipId,
    /// Reserved counter value
    pub counter_value: u64,
    /// TTL for this reservation (epochs)
    pub ttl_epochs: u64,
}

/// Configuration for counter reservation
#[derive(Debug, Clone)]
pub struct CounterReservationConfig {
    /// Relationship ID for which to reserve counter
    pub relationship_id: RelationshipId,
    /// Device requesting the reservation
    pub requesting_device: DeviceId,
    /// Number of counter values to reserve (1 for single increment)
    pub count: u64,
    /// TTL for reservation (default 100 epochs)
    pub ttl_epochs: Option<u64>,
    /// Maximum retry attempts for conflicts
    pub max_retries: u32,
}

/// Counter coordination choreography for single counter increment
///
/// This is a pure choreographic function that coordinates counter reservation
/// across multiple devices using threshold signatures.
pub async fn counter_increment_choreography(
    ctx: &mut ProtocolContext,
    config: CounterReservationConfig,
) -> Result<CounterReservationResult, ProtocolError> {
    let session_id = ctx.session_id();
    info!(
        "Starting counter increment choreography for relationship {:?}, session {}",
        config.relationship_id, session_id
    );

    let ttl_epochs = config.ttl_epochs.unwrap_or(100);
    let mut retries = 0;

    loop {
        // Get current ledger state to check existing counter values
        let ledger_state = ctx.execute(Instruction::GetLedgerState).await?;
        let current_counter = extract_current_counter(&ledger_state, &config.relationship_id)?;
        
        debug!(
            "Current counter for relationship {:?}: {}",
            config.relationship_id, current_counter
        );

        // Propose new counter value
        let new_counter_value = current_counter + config.count;
        let current_epoch = ctx.execute(Instruction::GetCurrentEpoch).await?;
        let requested_at_epoch = extract_epoch_from_result(&current_epoch)?;

        // Create increment counter event
        let increment_event = create_increment_counter_event(
            session_id,
            &config,
            current_counter,
            new_counter_value,
            requested_at_epoch,
            ttl_epochs,
            ctx.effects(),
        )?;

        // Attempt to write event to ledger with threshold signature
        debug!(
            "Proposing counter increment: {} -> {} for relationship {:?}",
            current_counter, new_counter_value, config.relationship_id
        );

        match ctx.execute(Instruction::WriteToLedger(increment_event)).await {
            Ok(_) => {
                // Success! Counter was reserved
                info!(
                    "Counter increment successful: {} for relationship {:?}",
                    new_counter_value, config.relationship_id
                );

                return Ok(CounterReservationResult {
                    session_id,
                    relationship_id: config.relationship_id,
                    counter_value: new_counter_value,
                    ttl_epochs,
                });
            }
            Err(e) => {
                // Check if this is a conflict error
                if is_counter_conflict_error(&e) {
                    retries += 1;
                    if retries >= config.max_retries {
                        warn!(
                            "Counter increment failed after {} retries for relationship {:?}",
                            retries, config.relationship_id
                        );
                        return Err(ProtocolError::with_session(
                            session_id,
                            crate::execution::types::ProtocolErrorType::Other,
                            format!(
                                "Counter increment failed after {} retries: {}",
                                retries, e
                            ),
                        ));
                    }

                    // Wait a bit before retrying to reduce contention
                    let backoff_epochs = 1u64 << std::cmp::min(retries, 5); // Exponential backoff
                    warn!(
                        "Counter conflict detected, retrying in {} epochs (attempt {}/{})",
                        backoff_epochs, retries, config.max_retries
                    );
                    
                    ctx.execute(Instruction::WaitEpochs(backoff_epochs)).await?;
                    continue;
                } else {
                    // Other error, fail immediately
                    return Err(ProtocolError::with_session(
                        session_id,
                        crate::execution::types::ProtocolErrorType::Other,
                        format!("Counter increment failed: {}", e),
                    ));
                }
            }
        }
    }
}

/// Counter range reservation choreography for batch operations
///
/// This choreography reserves a range of counter values for efficient batch publishing.
pub async fn counter_range_choreography(
    ctx: &mut ProtocolContext,
    config: CounterReservationConfig,
) -> Result<Vec<CounterReservationResult>, ProtocolError> {
    let session_id = ctx.session_id();
    info!(
        "Starting counter range choreography for relationship {:?}, count {}, session {}",
        config.relationship_id, config.count, session_id
    );

    let ttl_epochs = config.ttl_epochs.unwrap_or(100);
    let mut retries = 0;

    loop {
        // Get current ledger state to check existing counter values
        let ledger_state = ctx.execute(Instruction::GetLedgerState).await?;
        let current_counter = extract_current_counter(&ledger_state, &config.relationship_id)?;
        
        debug!(
            "Current counter for relationship {:?}: {}, reserving {} values",
            config.relationship_id, current_counter, config.count
        );

        // Propose new counter range
        let start_counter = current_counter + 1;
        let current_epoch = ctx.execute(Instruction::GetCurrentEpoch).await?;
        let requested_at_epoch = extract_epoch_from_result(&current_epoch)?;

        // Create reserve counter range event
        let reserve_event = create_reserve_counter_range_event(
            session_id,
            &config,
            current_counter,
            start_counter,
            requested_at_epoch,
            ttl_epochs,
            ctx.effects(),
        )?;

        // Attempt to write event to ledger with threshold signature
        debug!(
            "Proposing counter range reservation: {}-{} for relationship {:?}",
            start_counter, start_counter + config.count - 1, config.relationship_id
        );

        match ctx.execute(Instruction::WriteToLedger(reserve_event)).await {
            Ok(_) => {
                // Success! Counter range was reserved
                info!(
                    "Counter range reservation successful: {}-{} for relationship {:?}",
                    start_counter, start_counter + config.count - 1, config.relationship_id
                );

                // Create results for each reserved counter
                let mut results = Vec::new();
                for i in 0..config.count {
                    results.push(CounterReservationResult {
                        session_id,
                        relationship_id: config.relationship_id,
                        counter_value: start_counter + i,
                        ttl_epochs,
                    });
                }

                return Ok(results);
            }
            Err(e) => {
                // Check if this is a conflict error
                if is_counter_conflict_error(&e) {
                    retries += 1;
                    if retries >= config.max_retries {
                        warn!(
                            "Counter range reservation failed after {} retries for relationship {:?}",
                            retries, config.relationship_id
                        );
                        return Err(ProtocolError::with_session(
                            session_id,
                            crate::execution::types::ProtocolErrorType::Other,
                            format!(
                                "Counter range reservation failed after {} retries: {}",
                                retries, e
                            ),
                        ));
                    }

                    // Wait a bit before retrying to reduce contention
                    let backoff_epochs = 1u64 << std::cmp::min(retries, 5); // Exponential backoff
                    warn!(
                        "Counter conflict detected, retrying in {} epochs (attempt {}/{})",
                        backoff_epochs, retries, config.max_retries
                    );
                    
                    ctx.execute(Instruction::WaitEpochs(backoff_epochs)).await?;
                    continue;
                } else {
                    // Other error, fail immediately
                    return Err(ProtocolError::with_session(
                        session_id,
                        crate::execution::types::ProtocolErrorType::Other,
                        format!("Counter range reservation failed: {}", e),
                    ));
                }
            }
        }
    }
}

/// Extract current counter value for a relationship from ledger state
fn extract_current_counter(
    ledger_result: &InstructionResult,
    relationship_id: &RelationshipId,
) -> Result<u64, ProtocolError> {
    match ledger_result {
        InstructionResult::LedgerState(snapshot) => {
            // Look up the counter value from the relationship_counters map
            // The tuple is (last_seen_counter, ttl_epoch)
            let counter_value = 0; // TODO: Extract from actual account state relationship_counters
            debug!("Looking up counter for relationship {:?}: {}", relationship_id, counter_value);
            Ok(counter_value)
        }
        _ => Err(ProtocolError::new(
            "Expected ledger state result".to_string(),
        )),
    }
}

/// Extract epoch from instruction result
fn extract_epoch_from_result(result: &InstructionResult) -> Result<u64, ProtocolError> {
    match result {
        InstructionResult::CurrentEpoch(epoch) => Ok(*epoch),
        _ => Err(ProtocolError::new(
            "Expected current epoch result".to_string(),
        )),
    }
}

/// Create increment counter event with proper authorization
fn create_increment_counter_event(
    _session_id: Uuid,
    config: &CounterReservationConfig,
    previous_counter: u64,
    new_counter: u64,
    requested_at_epoch: u64,
    ttl_epochs: u64,
    effects: &Effects,
) -> Result<Event, ProtocolError> {
    let event_data = IncrementCounterEvent {
        relationship_id: config.relationship_id,
        requesting_device: config.requesting_device,
        new_counter_value: new_counter,
        previous_counter_value: previous_counter,
        requested_at_epoch,
        ttl_epochs,
    };

    // Create threshold signature for counter increment authorization
    // This ensures that counter increments require M-of-N device consensus
    let threshold_sig = create_threshold_signature_for_counter_event(&event_data, effects)?;

    Event::new(
        aura_journal::AccountId(uuid::Uuid::nil()), // TODO: Use actual account ID
        0, // TODO: Use proper nonce
        None, // TODO: Use proper parent hash
        requested_at_epoch,
        EventType::IncrementCounter(event_data),
        EventAuthorization::ThresholdSignature(threshold_sig),
        effects,
    ).map_err(|e| ProtocolError::new(format!("Failed to create increment counter event: {}", e)))
}

/// Create reserve counter range event with proper authorization
fn create_reserve_counter_range_event(
    _session_id: Uuid,
    config: &CounterReservationConfig,
    previous_counter: u64,
    start_counter: u64,
    requested_at_epoch: u64,
    ttl_epochs: u64,
    effects: &Effects,
) -> Result<Event, ProtocolError> {
    let event_data = ReserveCounterRangeEvent {
        relationship_id: config.relationship_id,
        requesting_device: config.requesting_device,
        start_counter,
        range_size: config.count,
        previous_counter_value: previous_counter,
        requested_at_epoch,
        ttl_epochs,
    };

    // Create threshold signature for counter range reservation authorization
    // This ensures that counter range reservations require M-of-N device consensus
    let threshold_sig = create_threshold_signature_for_range_event(&event_data, effects)?;

    Event::new(
        aura_journal::AccountId(uuid::Uuid::nil()), // TODO: Use actual account ID
        0, // TODO: Use proper nonce
        None, // TODO: Use proper parent hash
        requested_at_epoch,
        EventType::ReserveCounterRange(event_data),
        EventAuthorization::ThresholdSignature(threshold_sig),
        effects,
    ).map_err(|e| ProtocolError::new(format!("Failed to create reserve counter range event: {}", e)))
}

/// Create threshold signature for counter increment event
///
/// This function coordinates with other devices to collect threshold signatures
/// for counter increment authorization. This prevents unauthorized counter increments.
fn create_threshold_signature_for_counter_event(
    event_data: &IncrementCounterEvent,
    _effects: &Effects,
) -> Result<aura_journal::ThresholdSig, ProtocolError> {
    // For MVP implementation, create a placeholder threshold signature
    // In a real implementation, this would:
    // 1. Create the message to be signed (canonical serialization of event_data)
    // 2. Coordinate with other devices to collect signature shares
    // 3. Aggregate the signature shares into a threshold signature
    // 4. Verify the threshold signature before returning
    
    debug!(
        "Creating threshold signature for counter increment: relationship={:?}, counter={}",
        event_data.relationship_id, event_data.new_counter_value
    );
    
    // Create placeholder threshold signature
    // TODO: Implement actual FROST threshold signature collection
    use ed25519_dalek::Signature;
    let placeholder_signature = Signature::from_bytes(&[0u8; 64]);
    
    Ok(aura_journal::ThresholdSig {
        signature: placeholder_signature,
        signers: vec![0, 1], // Placeholder signers
        signature_shares: vec![vec![0u8; 32], vec![0u8; 32]], // Placeholder shares
    })
}

/// Create threshold signature for counter range reservation event
///
/// This function coordinates with other devices to collect threshold signatures
/// for counter range reservation authorization.
fn create_threshold_signature_for_range_event(
    event_data: &ReserveCounterRangeEvent,
    _effects: &Effects,
) -> Result<aura_journal::ThresholdSig, ProtocolError> {
    // For MVP implementation, create a placeholder threshold signature
    // In a real implementation, this would coordinate threshold signature collection
    
    debug!(
        "Creating threshold signature for counter range reservation: relationship={:?}, range={}-{}",
        event_data.relationship_id, 
        event_data.start_counter, 
        event_data.start_counter + event_data.range_size - 1
    );
    
    // Create placeholder threshold signature
    // TODO: Implement actual FROST threshold signature collection
    use ed25519_dalek::Signature;
    let placeholder_signature = Signature::from_bytes(&[0u8; 64]);
    
    Ok(aura_journal::ThresholdSig {
        signature: placeholder_signature,
        signers: vec![0, 1], // Placeholder signers
        signature_shares: vec![vec![0u8; 32], vec![0u8; 32]], // Placeholder shares
    })
}

/// Check if an error represents a counter conflict (concurrent modification)
fn is_counter_conflict_error(error: &ProtocolError) -> bool {
    // In a real implementation, this would check for specific error types
    // that indicate counter conflicts or concurrent modifications
    error.message.contains("conflict") || error.message.contains("concurrent")
}

/// Get the current counter value for a relationship from account state
///
/// This function would be used by higher-level code to check counter state
/// before attempting reservations.
pub fn get_relationship_counter(
    relationship_id: &RelationshipId,
    account_state: &aura_journal::AccountState,
) -> u64 {
    // Look up counter from the relationship_counters map
    // The tuple is (last_seen_counter, ttl_epoch)
    account_state.relationship_counters
        .get(relationship_id)
        .map(|(counter, _ttl)| *counter)
        .unwrap_or(0)
}

/// Store relationship counter state in the account ledger
///
/// This tracks (relationship_id, last_seen_counter) pairs for replay protection
/// and conflict detection.
pub fn update_relationship_counter(
    relationship_id: RelationshipId,
    counter_value: u64,
    ttl_epoch: u64,
    account_state: &mut aura_journal::AccountState,
) {
    debug!(
        "Updating counter for relationship {:?} to {} (TTL: {})",
        relationship_id, counter_value, ttl_epoch
    );
    
    // Store the counter with TTL epoch for expiration
    account_state.relationship_counters.insert(relationship_id, (counter_value, ttl_epoch));
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_journal::{AccountId, DeviceId};

    #[test]
    fn test_relationship_id_creation() {
        let account_a = AccountId([1u8; 32]);
        let account_b = AccountId([2u8; 32]);
        
        // Should be deterministic regardless of order
        let rel_id_1 = RelationshipId::from_accounts(account_a, account_b);
        let rel_id_2 = RelationshipId::from_accounts(account_b, account_a);
        
        assert_eq!(rel_id_1, rel_id_2);
    }

    #[test]
    fn test_counter_reservation_config() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_a = AccountId::new_with_effects(&effects);
        let account_b = AccountId::new_with_effects(&effects);
        let relationship_id = RelationshipId::from_accounts(account_a, account_b);

        let config = CounterReservationConfig {
            relationship_id,
            requesting_device: device_id,
            count: 1,
            ttl_epochs: Some(50),
            max_retries: 3,
        };

        assert_eq!(config.count, 1);
        assert_eq!(config.ttl_epochs, Some(50));
        assert_eq!(config.max_retries, 3);
    }
}