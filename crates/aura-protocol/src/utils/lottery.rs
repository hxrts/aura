// Deterministic lottery for distributed locking
//
// Reference: 080_architecture_protocol_integration.md - Part 3: Distributed Locking
//
// This module implements the deterministic lottery protocol for distributed lock acquisition.
// Uses hash-based lottery: hash(device_id || last_event_hash) to ensure fairness while
// maintaining determinism.

use aura_journal::{LedgerError, RequestOperationLockEvent, Result};
use aura_types::DeviceId;

/// Determine lock winner using deterministic lottery
///
/// Reference: 080 spec Part 3: Distributed Locking - Deterministic Lottery
///
/// Each device computes a lottery ticket: hash(device_id || last_event_hash)
/// The device with the lowest ticket wins. This ensures:
/// - Deterministic: Same inputs always produce same winner
/// - Fair: Probabilistic fairness over time as last_event_hash rotates
/// - Single-round: No multi-round coordination needed
pub fn determine_lock_winner(requests: &[RequestOperationLockEvent]) -> Result<DeviceId> {
    if requests.is_empty() {
        return Err(LedgerError::protocol_invalid_instruction(
            "Cannot determine winner from empty request list".to_string(),
        ));
    }

    // Find request with lowest lottery ticket
    let winner = requests
        .iter()
        .min_by_key(|req| req.lottery_ticket)
        .ok_or_else(|| {
            LedgerError::protocol_invalid_instruction("Failed to determine lock winner".to_string())
        })?;

    Ok(winner.device_id)
}

/// Compute lottery ticket for a device
///
/// Reference: 080 spec Part 3: Distributed Locking
///
/// Ticket = hash(device_id || last_event_hash)
pub fn compute_lottery_ticket(device_id: &DeviceId, last_event_hash: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(device_id.0.as_bytes());
    hasher.update(last_event_hash);
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;
    use aura_journal::OperationType;
    use uuid::Uuid;

    fn create_request(device_id: DeviceId, ticket: [u8; 32]) -> RequestOperationLockEvent {
        RequestOperationLockEvent {
            operation_type: OperationType::Dkd,
            session_id: Uuid::from_bytes([1u8; 16]), // Use deterministic session ID
            device_id,
            lottery_ticket: ticket,
            delegated_action: None, // No delegated action for basic lock request
        }
    }

    #[test]
    fn test_determine_winner_lowest_ticket() {
        let dev1 = DeviceId(Uuid::from_bytes([2u8; 16]));
        let dev2 = DeviceId(Uuid::from_bytes([3u8; 16]));
        let dev3 = DeviceId(Uuid::from_bytes([4u8; 16]));

        let ticket1 = [0xFF; 32]; // High ticket
        let ticket2 = [0x00; 32]; // Lowest ticket
        let ticket3 = [0x80; 32]; // Medium ticket

        let requests = vec![
            create_request(dev1, ticket1),
            create_request(dev2, ticket2),
            create_request(dev3, ticket3),
        ];

        let winner = determine_lock_winner(&requests).unwrap();
        assert_eq!(winner, dev2, "Device with lowest ticket should win");
    }

    #[test]
    fn test_determine_winner_deterministic() {
        let dev1 = DeviceId(Uuid::from_bytes([5u8; 16]));
        let dev2 = DeviceId(Uuid::from_bytes([6u8; 16]));

        let ticket1 = [0x50; 32];
        let ticket2 = [0x40; 32];

        let requests = vec![create_request(dev1, ticket1), create_request(dev2, ticket2)];

        // Multiple calls should return same winner
        let winner1 = determine_lock_winner(&requests).unwrap();
        let winner2 = determine_lock_winner(&requests).unwrap();
        let winner3 = determine_lock_winner(&requests).unwrap();

        assert_eq!(winner1, winner2);
        assert_eq!(winner2, winner3);
        assert_eq!(winner1, dev2);
    }

    #[test]
    fn test_determine_winner_single_request() {
        let dev1 = DeviceId(Uuid::from_bytes([7u8; 16]));
        let ticket1 = [0x42; 32];

        let requests = vec![create_request(dev1, ticket1)];

        let winner = determine_lock_winner(&requests).unwrap();
        assert_eq!(winner, dev1);
    }

    #[test]
    fn test_determine_winner_empty_requests() {
        let requests = vec![];
        let result = determine_lock_winner(&requests);
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_lottery_ticket_deterministic() {
        let device_id = DeviceId(Uuid::from_bytes([8u8; 16]));
        let last_event_hash = [0x42; 32];

        let ticket1 = compute_lottery_ticket(&device_id, &last_event_hash);
        let ticket2 = compute_lottery_ticket(&device_id, &last_event_hash);

        assert_eq!(
            ticket1, ticket2,
            "Ticket computation should be deterministic"
        );
    }

    #[test]
    fn test_compute_lottery_ticket_different_devices() {
        let dev1 = DeviceId(Uuid::from_bytes([9u8; 16]));
        let dev2 = DeviceId(Uuid::from_bytes([10u8; 16]));
        let last_event_hash = [0x42; 32];

        let ticket1 = compute_lottery_ticket(&dev1, &last_event_hash);
        let ticket2 = compute_lottery_ticket(&dev2, &last_event_hash);

        assert_ne!(
            ticket1, ticket2,
            "Different devices should have different tickets"
        );
    }

    #[test]
    fn test_compute_lottery_ticket_different_hashes() {
        let device_id = DeviceId(Uuid::from_bytes([11u8; 16]));
        let hash1 = [0x42; 32];
        let hash2 = [0x43; 32];

        let ticket1 = compute_lottery_ticket(&device_id, &hash1);
        let ticket2 = compute_lottery_ticket(&device_id, &hash2);

        assert_ne!(
            ticket1, ticket2,
            "Different event hashes should produce different tickets"
        );
    }

    #[test]
    fn test_lottery_fairness_simulation() {
        // Simulate multiple lottery rounds with rotating state
        // Verify that no device consistently loses

        let dev1 = DeviceId(Uuid::from_bytes([12u8; 16]));
        let dev2 = DeviceId(Uuid::from_bytes([13u8; 16]));
        let dev3 = DeviceId(Uuid::new_v4());

        let mut dev1_wins = 0;
        let mut dev2_wins = 0;
        let mut dev3_wins = 0;

        // Run 100 lotteries with different event hashes
        for i in 0u32..100 {
            let last_event_hash = blake3::hash(&i.to_le_bytes()).into();

            let ticket1 = compute_lottery_ticket(&dev1, &last_event_hash);
            let ticket2 = compute_lottery_ticket(&dev2, &last_event_hash);
            let ticket3 = compute_lottery_ticket(&dev3, &last_event_hash);

            let requests = vec![
                create_request(dev1, ticket1),
                create_request(dev2, ticket2),
                create_request(dev3, ticket3),
            ];

            let winner = determine_lock_winner(&requests).unwrap();

            if winner == dev1 {
                dev1_wins += 1;
            } else if winner == dev2 {
                dev2_wins += 1;
            } else if winner == dev3 {
                dev3_wins += 1;
            }
        }

        // Each device should win roughly 1/3 of the time (within tolerance)
        // This demonstrates probabilistic fairness
        assert!(
            dev1_wins > 10 && dev1_wins < 60,
            "Device 1 should win a reasonable number of times"
        );
        assert!(
            dev2_wins > 10 && dev2_wins < 60,
            "Device 2 should win a reasonable number of times"
        );
        assert!(
            dev3_wins > 10 && dev3_wins < 60,
            "Device 3 should win a reasonable number of times"
        );

        // All wins should sum to 100
        assert_eq!(dev1_wins + dev2_wins + dev3_wins, 100);
    }
}
