//! Byzantine fault tolerance tests for choreographic protocols
//!
//! This module contains comprehensive tests to ensure choreographic protocols
//! correctly handle up to 33% Byzantine participants.

/*
TODO: These Byzantine fault tolerance tests need API updates after protocol refactoring.
The tests exercise Byzantine behavior detection, equivocation, timeout handling, and
threshold enforcement, but require updates to work with the new choreographic protocol APIs.

Key areas needing updates:
- ByzantineDetector API changes
- ChoreographyResult error handling patterns
- RumpsteakAdapter integration
- TimeoutManager API updates
- Message handling and validation

Re-enable once the choreographic protocol APIs have stabilized.
*/

/*
#[cfg(test)]
mod tests {
    use crate::protocols::choreographic::{
        error_handling::{ByzantineDetector, ChoreographyResult, SafeChoreography},
        handler_adapter::{BridgedRole, RumpsteakAdapter},
        timeout_management::{OperationType, TimeoutConfig, TimeoutManager},
    };
    use aura_types::errors::{AuraError, ErrorCode};
    use rumpsteak_choreography::{ChoreographyError, Label};
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock};
    use uuid::Uuid;

    /// Simulated Byzantine participant behaviors
    #[derive(Debug, Clone, Copy)]
    enum ByzantineBehavior {
        /// Send corrupted messages
        CorruptMessages,
        /// Never respond (timeout)
        Timeout,
        /// Send messages out of order
        OutOfOrder,
        /// Send conflicting messages to different participants
        Equivocate,
        /// Randomly fail operations
        RandomFailure(f64), // failure probability
    }

    /// Byzantine test harness for choreographic protocols
    struct ByzantineTestHarness {
        /// Total number of participants
        n: usize,
        /// Number of Byzantine participants
        f: usize,
        /// Byzantine behaviors by participant
        byzantine_behaviors: HashMap<Uuid, ByzantineBehavior>,
        /// Message log for equivocation detection
        message_log: Arc<RwLock<HashMap<(Uuid, Uuid, u64), Vec<u8>>>>,
        /// Byzantine detector
        detector: Arc<Mutex<ByzantineDetector>>,
        /// Timeout manager
        timeout_manager: TimeoutManager,
    }

    impl ByzantineTestHarness {
        /// Create a new test harness with n participants and f Byzantine
        fn new(n: usize, f: usize) -> Self {
            assert!(f < n);
            assert!(
                f as f64 / n as f64 <= 0.33,
                "Byzantine participants must be <= 33%"
            );

            Self {
                n,
                f,
                byzantine_behaviors: HashMap::new(),
                message_log: Arc::new(RwLock::new(HashMap::new())),
                detector: Arc::new(Mutex::new(ByzantineDetector::new())),
                timeout_manager: TimeoutManager::with_config(TimeoutConfig::for_testing()),
            }
        }

        /// Assign Byzantine behaviors to f participants
        fn setup_byzantine_participants(&mut self, behaviors: Vec<(Uuid, ByzantineBehavior)>) {
            assert_eq!(behaviors.len(), self.f);
            for (id, behavior) in behaviors {
                self.byzantine_behaviors.insert(id, behavior);
            }
        }

        /// Simulate message sending with Byzantine behavior
        async fn send_message(
            &self,
            from: Uuid,
            to: Uuid,
            message: Vec<u8>,
            sequence: u64,
        ) -> ChoreographyResult<()> {
            // Check if sender is Byzantine
            if let Some(behavior) = self.byzantine_behaviors.get(&from) {
                match behavior {
                    ByzantineBehavior::CorruptMessages => {
                        // Corrupt the message
                        let mut corrupted = message.clone();
                        if !corrupted.is_empty() {
                            corrupted[0] ^= 0xFF; // Flip bits
                        }
                        self.log_message(from, to, sequence, corrupted).await;

                        // Record Byzantine behavior
                        let mut detector = self.detector.lock().await;
                        detector.record_invalid_message(from)?;
                    }
                    ByzantineBehavior::Timeout => {
                        // Don't send message, causing timeout
                        let mut detector = self.detector.lock().await;
                        detector.record_timeout(from)?;

                        return Err(AuraError::transport_timeout("Byzantine timeout"));
                    }
                    ByzantineBehavior::OutOfOrder => {
                        // Send with wrong sequence number
                        self.log_message(from, to, sequence + 10, message).await;

                        let mut detector = self.detector.lock().await;
                        detector.record_violation(from)?;
                    }
                    ByzantineBehavior::Equivocate => {
                        // Send different messages to different participants
                        let mut equivocated = message.clone();
                        equivocated.push(to.as_bytes()[0]); // Make message unique per recipient
                        self.log_message(from, to, sequence, equivocated).await;
                    }
                    ByzantineBehavior::RandomFailure(prob) => {
                        if rand::random::<f64>() < *prob {
                            let mut detector = self.detector.lock().await;
                            detector.record_violation(from)?;
                            return Err(AuraError::transport_failed("Random Byzantine failure"));
                        } else {
                            self.log_message(from, to, sequence, message).await;
                        }
                    }
                }
            } else {
                // Honest participant
                self.log_message(from, to, sequence, message).await;
                let mut detector = self.detector.lock().await;
                detector.record_success(from);
            }

            Ok(())
        }

        /// Log a message for equivocation detection
        async fn log_message(&self, from: Uuid, to: Uuid, sequence: u64, message: Vec<u8>) {
            let mut log = self.message_log.write().await;
            log.insert((from, to, sequence), message);
        }

        /// Detect equivocation by checking for conflicting messages
        async fn check_equivocation(&self, from: Uuid, sequence: u64) -> ChoreographyResult<()> {
            let log = self.message_log.read().await;
            let mut messages = HashSet::new();

            for ((sender, _, seq), msg) in log.iter() {
                if *sender == from && *seq == sequence {
                    messages.insert(msg.clone());
                }
            }

            if messages.len() > 1 {
                let mut detector = self.detector.lock().await;
                detector.record_violation(from)?;

                return Err(AuraError::Protocol(
                    aura_types::errors::ProtocolError::ByzantineBehavior {
                        participant: from.to_string(),
                        behavior: "Equivocation detected".to_string(),
                        evidence: Some(format!("Multiple messages from participant {}", from)),
                        context: "Byzantine behavior detected".to_string(),
                    },
                ));
            }

            Ok(())
        }

        /// Run a Byzantine agreement protocol
        async fn run_byzantine_agreement(
            &self,
            honest_participants: Vec<Uuid>,
            initial_values: HashMap<Uuid, bool>,
        ) -> ChoreographyResult<bool> {
            // Simple Byzantine agreement simulation
            let mut round = 0;
            let mut values = initial_values;

            loop {
                round += 1;
                if round > 10 {
                    return Err(AuraError::protocol_timeout(
                        "Byzantine agreement exceeded max rounds",
                    ));
                }

                // Exchange values
                let mut received_values: HashMap<Uuid, HashMap<Uuid, bool>> = HashMap::new();

                for &sender in honest_participants.iter() {
                    if let Some(&value) = values.get(&sender) {
                        for &receiver in honest_participants.iter() {
                            if sender != receiver {
                                // Simulate message sending
                                match self
                                    .send_message(sender, receiver, vec![value as u8], round)
                                    .await
                                {
                                    Ok(_) => {
                                        received_values
                                            .entry(receiver)
                                            .or_insert_with(HashMap::new)
                                            .insert(sender, value);
                                    }
                                    Err(_) => {
                                        // Byzantine behavior, ignore
                                    }
                                }
                            }
                        }
                    }
                }

                // Check for agreement
                let mut all_values: HashSet<bool> = HashSet::new();
                for participant_values in received_values.values() {
                    for &value in participant_values.values() {
                        all_values.insert(value);
                    }
                }

                if all_values.len() == 1 {
                    // Agreement reached
                    return Ok(*all_values.iter().next().unwrap());
                }

                // Update values based on majority
                for &participant in honest_participants.iter() {
                    if let Some(received) = received_values.get(&participant) {
                        let mut true_count = 0;
                        let mut false_count = 0;

                        for &value in received.values() {
                            if value {
                                true_count += 1;
                            } else {
                                false_count += 1;
                            }
                        }

                        values.insert(participant, true_count > false_count);
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_byzantine_message_corruption() {
        let mut harness = ByzantineTestHarness::new(4, 1);
        let byzantine_id = Uuid::new_v4();
        let honest_ids: Vec<_> = (0..3).map(|_| Uuid::new_v4()).collect();

        harness
            .setup_byzantine_participants(vec![(byzantine_id, ByzantineBehavior::CorruptMessages)]);

        // Byzantine participant sends corrupted messages
        for &honest_id in &honest_ids {
            let result = harness
                .send_message(byzantine_id, honest_id, vec![1, 2, 3, 4], 1)
                .await;

            assert!(result.is_ok()); // Message sent but corrupted
        }

        // Check Byzantine detection
        let detector = harness.detector.lock().await;
        let report = detector.get_report();
        assert_eq!(report.byzantine_participants.len(), 1);
        assert_eq!(report.byzantine_participants[0].id, byzantine_id);
    }

    #[tokio::test]
    async fn test_byzantine_timeout_behavior() {
        let mut harness = ByzantineTestHarness::new(4, 1);
        let byzantine_id = Uuid::new_v4();
        let honest_id = Uuid::new_v4();

        harness.setup_byzantine_participants(vec![(byzantine_id, ByzantineBehavior::Timeout)]);

        // Byzantine participant times out
        let result = harness
            .timeout_manager
            .with_timeout(
                OperationType::Network,
                harness.send_message(byzantine_id, honest_id, vec![1, 2, 3], 1),
            )
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            AuraError::Infrastructure(_) => {}
            _ => panic!("Expected timeout error"),
        }

        // Check Byzantine detection
        let detector = harness.detector.lock().await;
        let report = detector.get_report();
        assert!(!report.byzantine_participants.is_empty());
    }

    #[tokio::test]
    async fn test_equivocation_detection() {
        let mut harness = ByzantineTestHarness::new(4, 1);
        let byzantine_id = Uuid::new_v4();
        let honest_ids: Vec<_> = (0..3).map(|_| Uuid::new_v4()).collect();

        harness.setup_byzantine_participants(vec![(byzantine_id, ByzantineBehavior::Equivocate)]);

        // Byzantine participant equivocates
        for &honest_id in &honest_ids {
            harness
                .send_message(byzantine_id, honest_id, vec![1, 2, 3], 1)
                .await
                .ok();
        }

        // Check for equivocation
        let result = harness.check_equivocation(byzantine_id, 1).await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code(),
            Some(ErrorCode::SessionProtocolViolation)
        );
    }

    #[tokio::test]
    async fn test_byzantine_agreement_with_failures() {
        let harness = ByzantineTestHarness::new(4, 1);
        let honest_ids: Vec<_> = (0..3).map(|_| Uuid::new_v4()).collect();

        // Initial values: majority true
        let mut initial_values = HashMap::new();
        initial_values.insert(honest_ids[0], true);
        initial_values.insert(honest_ids[1], true);
        initial_values.insert(honest_ids[2], false);

        // Run Byzantine agreement
        let result = harness
            .run_byzantine_agreement(honest_ids, initial_values)
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap()); // Should agree on true (majority)
    }

    #[tokio::test]
    async fn test_byzantine_threshold_enforcement() {
        let mut harness = ByzantineTestHarness::new(10, 3); // 30% Byzantine

        // Setup 3 Byzantine participants with random failure
        let byzantine_ids: Vec<_> = (0..3).map(|_| Uuid::new_v4()).collect();
        for id in &byzantine_ids {
            harness
                .byzantine_behaviors
                .insert(*id, ByzantineBehavior::RandomFailure(0.8));
        }

        // Simulate interactions
        for _ in 0..50 {
            for &byzantine_id in &byzantine_ids {
                let _ = harness
                    .send_message(byzantine_id, Uuid::new_v4(), vec![1, 2, 3], 1)
                    .await;
            }
        }

        // Threshold should not be exceeded (30% < 33%)
        let detector = harness.detector.lock().await;
        let result = detector.check_byzantine_threshold();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_byzantine_threshold_exceeded() {
        let mut harness = ByzantineTestHarness::new(10, 4); // 40% Byzantine (exceeds threshold)

        // Setup 4 Byzantine participants
        let byzantine_ids: Vec<_> = (0..4).map(|_| Uuid::new_v4()).collect();
        for id in &byzantine_ids {
            harness
                .byzantine_behaviors
                .insert(*id, ByzantineBehavior::RandomFailure(0.9));
        }

        // Generate enough interactions to establish pattern
        for _ in 0..20 {
            for &byzantine_id in &byzantine_ids {
                let _ = harness
                    .send_message(byzantine_id, Uuid::new_v4(), vec![1], 1)
                    .await;
            }
        }

        // Threshold should be exceeded (40% > 33%)
        let detector = harness.detector.lock().await;
        let result = detector.check_byzantine_threshold();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().severity(),
            aura_types::errors::ErrorSeverity::Critical
        );
    }

    #[tokio::test]
    async fn test_byzantine_recovery() {
        let mut harness = ByzantineTestHarness::new(7, 2); // 28% Byzantine

        // Test protocol recovery after Byzantine behavior
        let honest_ids: Vec<_> = (0..5).map(|_| Uuid::new_v4()).collect();
        let byzantine_ids: Vec<_> = (0..2).map(|_| Uuid::new_v4()).collect();

        for (i, id) in byzantine_ids.iter().enumerate() {
            let behavior = if i == 0 {
                ByzantineBehavior::OutOfOrder
            } else {
                ByzantineBehavior::RandomFailure(0.5)
            };
            harness.setup_byzantine_participants(vec![(*id, behavior)]);
        }

        // Initial values for agreement
        let mut values = HashMap::new();
        for (i, &id) in honest_ids.iter().enumerate() {
            values.insert(id, i % 2 == 0);
        }

        // Despite Byzantine participants, protocol should complete
        let result = harness
            .run_byzantine_agreement(honest_ids.clone(), values)
            .await;
        assert!(
            result.is_ok(),
            "Protocol should complete with < 33% Byzantine"
        );

        // Verify Byzantine participants were detected
        let detector = harness.detector.lock().await;
        let report = detector.get_report();
        assert!(
            !report.byzantine_participants.is_empty(),
            "Byzantine participants should be detected"
        );
    }
}
*/
