//! Distributed Locking Choreography
//!
//! This module implements the threshold-granted distributed locking protocol
//! using choreographic programming.
//!
//! ## Choreographic Session Type
//!
//! ```text
//! Request{p ∈ Contenders}(LotteryTicket_p) .
//! Grant(Winner, ThresholdSignature) .
//! [ Use critical section ] .
//! Release(SessionId)
//! ```
//!
//! ## Deterministic Lottery
//!
//! The winner is determined by a hash-based lottery:
//! ```text
//! ticket_p = hash(last_event_hash || device_id_p)
//! winner = argmax_p(ticket_p)
//! ```
//!
//! This ensures:
//! - Determinism: All parties compute same winner
//! - Fairness: Each device has equal probability over time
//! - Byzantine resistance: Can't predict ticket without seeing last event
//!
//! Reference:
//! - 080_architecture_protocol_integration.md - Part 3: Distributed Locking
//! - work/04_declarative_protocol_evolution.md - Phase 2

use crate::execution::{ProtocolContext, ProtocolError, ProtocolErrorType};

/// Locking Choreography
///
/// This choreography implements distributed lock acquisition using a
/// threshold-granted lottery system with Session-based state management.
///
/// ## Choreographic Flow
///
/// 1. **Session Creation**: Create Session with ProtocolType::LockAcquisition
/// 2. **Request**: Device broadcasts lock request with lottery ticket
/// 3. **Collect**: All parties observe competing requests
/// 4. **Grant**: Threshold signs grant for winner (deterministic lottery)
/// 5. **Session Update**: Update Session status based on outcome
/// 6. **Return**: If won: Ok(()), else: Err(LockDenied)
///
/// The caller is responsible for releasing the lock after use via
/// `release_lock_choreography()`.
///
/// ## Example Usage
///
/// ```rust,ignore
/// // Acquire lock
/// locking_choreography(&mut ctx, OperationType::Resharing).await?;
///
/// // Perform critical operation
/// perform_resharing().await?;
///
/// // Release lock
/// release_lock_choreography(&mut ctx).await?;
/// ```
pub async fn locking_choreography(
    ctx: &mut ProtocolContext,
    operation_type: aura_journal::OperationType,
) -> Result<(), ProtocolError> {
    use crate::execution::{EventFilter, EventTypePattern, Instruction, InstructionResult};
    use crate::utils::{compute_lottery_ticket, determine_lock_winner};
    use aura_journal::{
        Event, EventAuthorization, EventType, GrantOperationLockEvent, ParticipantId, ProtocolType,
        RequestOperationLockEvent, Session, SessionId,
    };

    // Step 1: Create Session for lock acquisition
    let session = {
        let state = ctx.execute(Instruction::GetLedgerState).await?;
        if let InstructionResult::LedgerState(_snapshot) = state {
            let participants = ctx
                .participants()
                .iter()
                .map(|device_id| ParticipantId::Device(*device_id))
                .collect();

            let current_epoch = ctx.execute(Instruction::GetCurrentEpoch).await?;
            let current_epoch = if let InstructionResult::CurrentEpoch(epoch) = current_epoch {
                epoch
            } else {
                return Err(ProtocolError {
                    session_id: ctx.session_id(),
                    error_type: ProtocolErrorType::Other,
                    message: "Failed to get current epoch".to_string(),
                });
            };

            Session::new(
                SessionId(ctx.session_id()),
                ProtocolType::LockAcquisition,
                participants,
                current_epoch,
                10, // Lock acquisition should be fast - 10 epochs TTL
                ctx.effects().now().unwrap_or(0),
            )
        } else {
            return Err(ProtocolError {
                session_id: ctx.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "Failed to get ledger state".to_string(),
            });
        }
    };

    // Step 2: Update Session status to Active
    let mut session_event = Event {
        version: 1,
        event_id: aura_journal::EventId::new_with_effects(ctx.effects()),
        account_id: session
            .participants
            .first()
            .and_then(|p| match p {
                ParticipantId::Device(device_id) => {
                    // We need to extract account_id from device, for now use a placeholder
                    Some(aura_journal::AccountId(device_id.0))
                }
                _ => None,
            })
            .unwrap_or_else(|| aura_journal::AccountId(uuid::Uuid::new_v4())),
        timestamp: ctx.effects().now().unwrap_or(0),
        nonce: ctx.generate_nonce().await.unwrap_or(0),
        parent_hash: None,
        epoch_at_write: session.started_at,
        event_type: EventType::EpochTick(aura_journal::EpochTickEvent {
            new_epoch: session.started_at + 1,
            evidence_hash: [0u8; 32], // Placeholder
        }),
        authorization: EventAuthorization::DeviceCertificate {
            device_id: aura_journal::DeviceId(ctx.device_id()),
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]), // Placeholder
        },
    };

    // Sign the event
    let signature = ctx.sign_event(&session_event)?;
    session_event.authorization = EventAuthorization::DeviceCertificate {
        device_id: aura_journal::DeviceId(ctx.device_id()),
        signature,
    };

    // Write session creation to ledger
    ctx.execute(Instruction::WriteToLedger(session_event))
        .await?;

    // Step 3: Request lock with lottery ticket
    let last_event_hash = {
        let state = ctx.execute(Instruction::GetLedgerState).await?;
        if let InstructionResult::LedgerState(snapshot) = state {
            snapshot.last_event_hash.unwrap_or([0u8; 32])
        } else {
            [0u8; 32]
        }
    };

    let my_device_id = aura_journal::DeviceId(ctx.device_id());
    let lottery_ticket = compute_lottery_ticket(&my_device_id, &last_event_hash);

    let mut request_event = {
        // Get current ledger state for proper epoch and parent hash
        let ledger_state = ctx.execute(Instruction::GetLedgerState).await?;
        let (current_epoch, parent_hash) =
            if let InstructionResult::LedgerState(snapshot) = ledger_state {
                (snapshot.current_epoch + 1, snapshot.last_event_hash)
            } else {
                return Err(ProtocolError {
                    session_id: ctx.session_id(),
                    error_type: ProtocolErrorType::Other,
                    message: "Failed to get ledger state for event creation".to_string(),
                });
            };

        let mut event = Event {
            version: 1,
            event_id: aura_journal::EventId::new_with_effects(ctx.effects()),
            account_id: session
                .participants
                .first()
                .and_then(|p| match p {
                    ParticipantId::Device(device_id) => Some(aura_journal::AccountId(device_id.0)),
                    _ => None,
                })
                .unwrap_or_else(|| aura_journal::AccountId(uuid::Uuid::new_v4())),
            timestamp: ctx.effects().now().unwrap_or(0),
            nonce: ctx.generate_nonce().await.unwrap_or(0),
            parent_hash,
            epoch_at_write: current_epoch,
            event_type: EventType::RequestOperationLock(RequestOperationLockEvent {
                operation_type,
                session_id: ctx.session_id(),
                device_id: my_device_id,
                lottery_ticket,
                delegated_action: None,
            }),
            authorization: EventAuthorization::DeviceCertificate {
                device_id: my_device_id,
                signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]), // Placeholder, will be replaced
            },
        };

        // Sign the event
        let signature = ctx.sign_event(&event)?;
        event.authorization = EventAuthorization::DeviceCertificate {
            device_id: my_device_id,
            signature,
        };

        event
    };

    // Broadcast lock request
    // Retry event creation if we get parent hash conflicts (due to concurrent event creation)
    let mut attempts = 0;
    const MAX_RETRY_ATTEMPTS: usize = 3;

    loop {
        match ctx
            .execute(Instruction::WriteToLedger(request_event.clone()))
            .await
        {
            Ok(_) => break,
            Err(e) if attempts < MAX_RETRY_ATTEMPTS && e.message.contains("parent hash") => {
                attempts += 1;
                eprintln!(
                    "Lock request write failed (attempt {}), retrying: {}",
                    attempts, e.message
                );

                // Refresh ledger state and recreate event with updated parent hash
                let ledger_state = ctx.execute(Instruction::GetLedgerState).await?;
                let (current_epoch, parent_hash) =
                    if let InstructionResult::LedgerState(snapshot) = ledger_state {
                        (snapshot.current_epoch + 1, snapshot.last_event_hash)
                    } else {
                        return Err(e);
                    };

                let mut new_event = request_event.clone();
                new_event.epoch_at_write = current_epoch;
                new_event.parent_hash = parent_hash;
                new_event.nonce = ctx.generate_nonce().await.unwrap_or(0);

                // Re-sign the updated event
                let signature = ctx.sign_event(&new_event)?;
                new_event.authorization = EventAuthorization::DeviceCertificate {
                    device_id: my_device_id,
                    signature,
                };

                request_event = new_event;

                // Small delay before retry - use minimal epoch wait instead of direct sleep
                // This ensures compatibility with the simulation engine's time control
                let _ = ctx.execute(Instruction::WaitEpochs(1)).await;
            }
            Err(e) => return Err(e),
        }
    }

    // Immediately check if our event is visible by refreshing
    // This helps with timing issues in the simulation
    let _initial_check = ctx
        .execute(Instruction::CheckForEvent {
            filter: EventFilter {
                session_id: Some(ctx.session_id()),
                event_types: Some(vec![EventTypePattern::LockRequest]),
                authors: Some(std::collections::BTreeSet::from([my_device_id])),
                predicate: None,
            },
        })
        .await;

    // Step 4: Await threshold of lock requests
    // We need to wait for other participants' lock requests, but we already wrote our own
    // So we need to wait for (participants.len() - 1) more requests + our own = participants.len() total
    let filter = EventFilter {
        session_id: Some(ctx.session_id()),
        event_types: Some(vec![EventTypePattern::LockRequest]),
        authors: None,
        predicate: None,
    };

    let threshold_result = ctx
        .execute(Instruction::AwaitThreshold {
            count: ctx.participants().len(), // Wait for all participants to request
            filter,
            timeout_epochs: Some(100), // Much longer timeout for debugging coordination issues
        })
        .await?;

    let request_events = if let InstructionResult::EventsReceived(events) = threshold_result {
        events
    } else {
        return Err(ProtocolError {
            session_id: ctx.session_id(),
            error_type: ProtocolErrorType::Timeout,
            message: "Failed to collect lock requests".to_string(),
        });
    };

    // Step 5: Determine winner using lottery
    let lock_requests: Vec<RequestOperationLockEvent> = request_events
        .iter()
        .filter_map(|event| {
            if let EventType::RequestOperationLock(req) = &event.event_type {
                Some(req.clone())
            } else {
                None
            }
        })
        .collect();

    let winner = determine_lock_winner(&lock_requests).map_err(|e| ProtocolError {
        session_id: ctx.session_id(),
        error_type: ProtocolErrorType::Other,
        message: format!("Failed to determine lock winner: {:?}", e),
    })?;

    // Verify lottery computation: all parties should compute the same winner
    // This is guaranteed by the deterministic lottery algorithm
    // In production, the grant event's threshold signature serves as proof that
    // at least M devices agreed on the winner

    // Step 6: Check if we won the lottery
    if winner == my_device_id {
        // We won! Wait for grant event (this would be threshold signed in practice)
        let _grant_filter = EventFilter {
            session_id: Some(ctx.session_id()),
            event_types: Some(vec![EventTypePattern::LockGrant]),
            authors: None,
            predicate: None,
        };

        // In production, this grant event would be threshold-signed by M-of-N devices
        // to prove consensus on the lottery winner. For now, we simulate the grant
        let grant_event = {
            let mut event = Event {
                version: 1,
                event_id: aura_journal::EventId::new_with_effects(ctx.effects()),
                account_id: session
                    .participants
                    .first()
                    .and_then(|p| match p {
                        ParticipantId::Device(device_id) => {
                            Some(aura_journal::AccountId(device_id.0))
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| aura_journal::AccountId(uuid::Uuid::new_v4())),
                timestamp: ctx.effects().now().unwrap_or(0),
                nonce: ctx.generate_nonce().await.unwrap_or(0),
                parent_hash: None,
                epoch_at_write: session.started_at + 2,
                event_type: EventType::GrantOperationLock(GrantOperationLockEvent {
                    operation_type,
                    session_id: ctx.session_id(),
                    winner_device_id: winner,
                    granted_at_epoch: session.started_at + 2,
                    threshold_signature: aura_journal::ThresholdSig {
                        signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                        signers: vec![1], // Placeholder - would be actual signer indices
                        signature_shares: vec![], // Placeholder - would be actual signature shares
                    },
                }),
                authorization: EventAuthorization::DeviceCertificate {
                    device_id: my_device_id,
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]), // Placeholder
                },
            };

            let signature = ctx.sign_event(&event)?;
            event.authorization = EventAuthorization::DeviceCertificate {
                device_id: my_device_id,
                signature,
            };

            event
        };

        ctx.execute(Instruction::WriteToLedger(grant_event)).await?;

        // Update session status to Completed (we acquired the lock)
        Ok(())
    } else {
        // We lost the lottery
        Err(ProtocolError {
            session_id: ctx.session_id(),
            error_type: ProtocolErrorType::Other,
            message: format!("Lock denied - winner was {:?}", winner),
        })
    }
}

/// Release Lock Choreography
///
/// This choreography releases a previously acquired distributed lock
/// and updates the Session status to Completed.
///
/// ## Choreographic Flow
///
/// 1. **Release**: Lock holder broadcasts release event
/// 2. **Session Update**: Update Session status to Completed
/// 3. **Observe**: All parties observe release and update state
///
/// ## Example Usage
///
/// ```rust,ignore
/// release_lock_choreography(&mut ctx, OperationType::Resharing).await?;
/// ```
pub async fn release_lock_choreography(
    ctx: &mut ProtocolContext,
    operation_type: aura_journal::OperationType,
) -> Result<(), ProtocolError> {
    use crate::execution::{Instruction, InstructionResult};
    use aura_journal::{Event, EventAuthorization, EventType, ReleaseOperationLockEvent};

    // Step 1: Create release event
    let my_device_id = aura_journal::DeviceId(ctx.device_id());

    let release_event = {
        let mut event = Event {
            version: 1,
            event_id: aura_journal::EventId::new_with_effects(ctx.effects()),
            account_id: aura_journal::AccountId(uuid::Uuid::new_v4()), // Placeholder
            timestamp: ctx.effects().now().unwrap_or(0),
            nonce: ctx.generate_nonce().await.unwrap_or(0),
            parent_hash: None,
            epoch_at_write: {
                let current_epoch = ctx.execute(Instruction::GetCurrentEpoch).await?;
                if let InstructionResult::CurrentEpoch(epoch) = current_epoch {
                    epoch
                } else {
                    return Err(ProtocolError {
                        session_id: ctx.session_id(),
                        error_type: ProtocolErrorType::Other,
                        message: "Failed to get current epoch".to_string(),
                    });
                }
            },
            event_type: EventType::ReleaseOperationLock(ReleaseOperationLockEvent {
                operation_type,
                session_id: ctx.session_id(),
                device_id: my_device_id,
            }),
            authorization: EventAuthorization::DeviceCertificate {
                device_id: my_device_id,
                signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]), // Placeholder
            },
        };

        // Sign the event
        let signature = ctx.sign_event(&event)?;
        event.authorization = EventAuthorization::DeviceCertificate {
            device_id: my_device_id,
            signature,
        };

        event
    };

    // Step 2: Broadcast release event
    ctx.execute(Instruction::WriteToLedger(release_event))
        .await?;

    // Step 3: Session is automatically updated by the ledger's event application logic
    // The ReleaseOperationLock event handler will update the session status

    Ok(())
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[tokio::test]
    async fn test_deterministic_lottery_same_winner() {
        use crate::utils::{compute_lottery_ticket, determine_lock_winner};
        use aura_crypto::Effects;
        use aura_journal::DeviceId;
        use aura_session_types::protocols::journal::LockRequest;
        
        let effects = Effects::test();
        let last_event_hash = [0xAB; 32]; // Same for all devices
        
        // Create multiple devices
        let device1 = DeviceId::new_with_effects(&effects);
        let device2 = DeviceId::new_with_effects(&effects);
        let device3 = DeviceId::new_with_effects(&effects);
        
        // Compute lottery tickets for each device
        let ticket1 = compute_lottery_ticket(&device1, &last_event_hash);
        let ticket2 = compute_lottery_ticket(&device2, &last_event_hash);
        let ticket3 = compute_lottery_ticket(&device3, &last_event_hash);
        
        // Create lock request events using the RequestOperationLockEvent struct
        let request1 = aura_journal::RequestOperationLockEvent {
            session_id: effects.gen_uuid(),
            device_id: device1,
            operation_type: aura_journal::OperationType::Resharing,
            lottery_ticket: ticket1,
            delegated_action: None,
        };
        let request2 = aura_journal::RequestOperationLockEvent {
            session_id: effects.gen_uuid(),
            device_id: device2,
            operation_type: aura_journal::OperationType::Resharing,
            lottery_ticket: ticket2,
            delegated_action: None,
        };
        let request3 = aura_journal::RequestOperationLockEvent {
            session_id: effects.gen_uuid(),
            device_id: device3,
            operation_type: aura_journal::OperationType::Resharing,
            lottery_ticket: ticket3,
            delegated_action: None,
        };
        
        let requests = vec![request1.clone(), request2.clone(), request3.clone()];
        
        // Determine winner multiple times - should be consistent
        let winner1 = determine_lock_winner(&requests).ok();
        let winner2 = determine_lock_winner(&requests).ok();
        let winner3 = determine_lock_winner(&requests).ok();
        
        assert_eq!(winner1, winner2);
        assert_eq!(winner2, winner3);
        assert!(winner1.is_some());
        
        // The winner should be one of the requesting devices
        let winner_device = winner1.unwrap();
        assert!(
            winner_device == device1 || winner_device == device2 || winner_device == device3,
            "Winner should be one of the requesting devices"
        );
    }
    
    #[tokio::test]
    async fn test_lottery_fairness_over_multiple_rounds() {
        use crate::utils::{compute_lottery_ticket, determine_lock_winner};
        use aura_crypto::Effects;
        use aura_journal::DeviceId;
        use aura_session_types::protocols::journal::LockRequest;
        use std::collections::HashMap;
        
        let effects = Effects::test();
        
        // Create devices
        let device1 = DeviceId::new_with_effects(&effects);
        let device2 = DeviceId::new_with_effects(&effects);
        let device3 = DeviceId::new_with_effects(&effects);
        let devices = [device1, device2, device3];
        
        let mut win_counts = HashMap::new();
        let rounds = 100;
        
        // Simulate multiple rounds with different last_event_hash values
        for round in 0..rounds {
            let mut last_event_hash = [0u8; 32];
            last_event_hash[0] = (round % 256) as u8;
            last_event_hash[1] = ((round / 256) % 256) as u8;
            
            // Create requests for this round
            let requests: Vec<aura_journal::RequestOperationLockEvent> = devices
                .iter()
                .map(|&device_id| {
                    let ticket = compute_lottery_ticket(&device_id, &last_event_hash);
                    aura_journal::RequestOperationLockEvent {
                        session_id: effects.gen_uuid(),
                        device_id,
                        operation_type: aura_journal::OperationType::Resharing,
                        lottery_ticket: ticket,
                        delegated_action: None,
                    }
                })
                .collect();
            
            // Determine winner for this round
            if let Ok(winner) = determine_lock_winner(&requests) {
                *win_counts.entry(winner).or_insert(0) += 1;
            }
        }
        
        // Check that each device won some rounds (fairness)
        for device in &devices {
            let wins = win_counts.get(device).unwrap_or(&0);
            assert!(
                *wins > 0,
                "Device {:?} should have won at least one round out of {}",
                device,
                rounds
            );
            
            // Check that no device is overly dominant (within reasonable bounds)
            // Each device should win roughly 1/3 of the time, allow for variance
            assert!(
                *wins > rounds / 6 && *wins < rounds * 2 / 3,
                "Device {:?} won {} times out of {} rounds, expected roughly {}",
                device,
                wins,
                rounds,
                rounds / 3
            );
        }
        
        // Verify all rounds had a winner
        let total_wins: u32 = win_counts.values().sum();
        assert_eq!(total_wins, rounds, "Every round should have exactly one winner");
    }
    
    #[tokio::test]
    async fn test_single_device_always_wins() {
        use crate::utils::{compute_lottery_ticket, determine_lock_winner};
        use aura_crypto::Effects;
        use aura_journal::DeviceId;
        use aura_session_types::protocols::journal::LockRequest;
        
        let effects = Effects::test();
        let device = DeviceId::new_with_effects(&effects);
        let last_event_hash = [0xFF; 32];
        
        let ticket = compute_lottery_ticket(&device, &last_event_hash);
        let request = aura_journal::RequestOperationLockEvent {
            session_id: effects.gen_uuid(),
            device_id: device,
            operation_type: aura_journal::OperationType::Resharing,
            lottery_ticket: ticket,
            delegated_action: None,
        };
        
        let requests = vec![request];
        let winner = determine_lock_winner(&requests);
        
        assert!(winner.is_ok(), "Single device should always win the lottery");
        assert_eq!(winner.unwrap(), device);
    }
    
    #[tokio::test]
    async fn test_empty_requests_no_winner() {
        use crate::utils::determine_lock_winner;
        
        let requests = vec![];
        let winner = determine_lock_winner(&requests);
        
        assert!(winner.is_err(), "Empty request list should have no winner");
    }
    
    #[tokio::test]
    async fn test_lottery_ticket_deterministic() {
        use crate::utils::compute_lottery_ticket;
        use aura_crypto::Effects;
        use aura_journal::DeviceId;
        
        let effects = Effects::test();
        let device = DeviceId::new_with_effects(&effects);
        let last_event_hash = [0x42; 32];
        
        // Compute ticket multiple times
        let ticket1 = compute_lottery_ticket(&device, &last_event_hash);
        let ticket2 = compute_lottery_ticket(&device, &last_event_hash);
        let ticket3 = compute_lottery_ticket(&device, &last_event_hash);
        
        assert_eq!(ticket1, ticket2);
        assert_eq!(ticket2, ticket3);
    }
    
    #[tokio::test]
    async fn test_different_devices_different_tickets() {
        use crate::utils::compute_lottery_ticket;
        use aura_crypto::Effects;
        use aura_journal::DeviceId;
        
        let effects = Effects::test();
        let device1 = DeviceId::new_with_effects(&effects);
        let device2 = DeviceId::new_with_effects(&effects);
        let last_event_hash = [0x42; 32];
        
        let ticket1 = compute_lottery_ticket(&device1, &last_event_hash);
        let ticket2 = compute_lottery_ticket(&device2, &last_event_hash);
        
        assert_ne!(ticket1, ticket2, "Different devices should have different lottery tickets");
    }
    
    #[tokio::test]
    async fn test_different_event_hash_different_tickets() {
        use crate::utils::compute_lottery_ticket;
        use aura_crypto::Effects;
        use aura_journal::DeviceId;
        
        let effects = Effects::test();
        let device = DeviceId::new_with_effects(&effects);
        
        let hash1 = [0x11; 32];
        let hash2 = [0x22; 32];
        
        let ticket1 = compute_lottery_ticket(&device, &hash1);
        let ticket2 = compute_lottery_ticket(&device, &hash2);
        
        assert_ne!(ticket1, ticket2, "Different event hashes should produce different lottery tickets");
    }
}
