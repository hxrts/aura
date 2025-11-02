//! Test Utilities for Coordination Protocols
//!
//! This module provides common test infrastructure that reduces
//! duplication across test suites.

#![cfg(test)]

use aura_crypto::Effects;
use aura_journal::{AccountLedger, AccountState, DeviceMetadata, DeviceType};
use aura_protocol::execution::{BaseContext, MemoryTransport, ProtocolContext, Transport};
use aura_types::{AccountId, AccountIdExt, DeviceId};
use ed25519_dalek::{SigningKey, VerifyingKey};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Test fixture for protocol testing
pub struct TestFixture {
    pub device_id: DeviceId,
    pub account_id: AccountId,
    pub participants: Vec<DeviceId>,
    pub threshold: usize,
    pub effects: Effects,
    pub transport: Arc<MemoryTransport>,
}

impl TestFixture {
    /// Create a new test fixture with default values
    pub fn new() -> Self {
        Self::with_participants(3, 2)
    }

    /// Create a test fixture with specific participant count and threshold
    pub fn with_participants(participant_count: usize, threshold: usize) -> Self {
        let effects = Effects::test();
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId::new_with_effects(&effects);

        let participants: Vec<DeviceId> = (0..participant_count)
            .map(|_| DeviceId(Uuid::new_v4()))
            .collect();

        let transport = Arc::new(MemoryTransport::default());

        Self {
            device_id,
            account_id,
            participants,
            threshold,
            effects,
            transport,
        }
    }

    /// Create a protocol context for testing
    pub async fn create_context(&self) -> ProtocolContext {
        let base_context = self.create_base_context().await;
        // DkdContext is just a type alias for BaseContext
        ProtocolContext::Dkd(base_context)
    }

    /// Create a base context for testing
    pub async fn create_base_context(&self) -> BaseContext {
        let session_id = Uuid::new_v4();
        let device_key = SigningKey::from_bytes(&self.effects.random_bytes::<32>());
        let group_public_key = VerifyingKey::from_bytes(&[1u8; 32]).unwrap();

        // Create device metadata
        let device_metadata = DeviceMetadata {
            device_id: self.device_id,
            device_name: "test-device".to_string(),
            device_type: DeviceType::Native,
            public_key: group_public_key,
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: Default::default(),
            next_nonce: 0,
            used_nonces: Default::default(),
        };

        // Create initial account state
        let initial_state = AccountState::new(
            self.account_id,
            group_public_key,
            device_metadata,
            self.threshold as u16,
            self.participants.len() as u16,
        );

        let ledger = Arc::new(RwLock::new(
            AccountLedger::new(initial_state).expect("Failed to create ledger"),
        ));

        let time_source = Box::new(aura_protocol::execution::time::ProductionTimeSource::new());

        BaseContext::new(
            session_id,
            self.device_id.0,
            self.participants.clone(),
            Some(self.threshold),
            ledger,
            self.transport.clone() as Arc<dyn Transport>,
            self.effects.clone(),
            device_key,
            time_source,
        )
    }
}

/// Common test assertions for protocol states
pub mod assertions {

    /// Assert that a protocol is in the expected state
    pub fn assert_state<T: std::fmt::Debug>(actual: &T, expected_state_name: &str) {
        let debug_str = format!("{:?}", actual);
        assert!(
            debug_str.contains(expected_state_name),
            "Expected state '{}' but found: {:?}",
            expected_state_name,
            actual
        );
    }

    /// Assert that a result contains an expected error
    pub fn assert_error_contains<T, E: std::fmt::Display>(
        result: Result<T, E>,
        expected_msg: &str,
    ) {
        match result {
            Ok(_) => panic!("Expected error containing '{}', but got Ok", expected_msg),
            Err(e) => {
                let error_msg = e.to_string();
                assert!(
                    error_msg.contains(expected_msg),
                    "Error message '{}' does not contain expected '{}'",
                    error_msg,
                    expected_msg
                );
            }
        }
    }
}

/// Mock implementations for testing
pub mod mocks {
    // Use necessary execution imports based on actual usage in tests
    use aura_journal::{Event, EventType};

    /// Mock event builder for testing
    pub struct MockEventBuilder {
        events: Vec<Event>,
    }

    impl MockEventBuilder {
        pub fn new() -> Self {
            Self { events: Vec::new() }
        }

        pub fn add_event(mut self, event: Event) -> Self {
            self.events.push(event);
            self
        }

        pub fn build(self) -> Vec<Event> {
            self.events
        }
    }

    /// Create a mock event with minimal fields
    pub fn mock_event(event_type: EventType) -> Event {
        use aura_crypto::{ed25519_sign, generate_ed25519_key, Effects};
        use aura_types::{AccountId, DeviceId, DeviceIdExt, EventId, EventIdExt};

        let effects = Effects::for_test("mock_event");

        // Create a mock device signature for the event
        let device_id = DeviceId::new_with_effects(&effects);
        let signing_key = generate_ed25519_key();
        let message = b"mock event signature";
        let signature = ed25519_sign(&signing_key, message);

        Event {
            version: 1,
            event_id: EventId::new_with_effects(&effects),
            account_id: AccountId::new(),
            timestamp: effects.now().unwrap_or(0),
            nonce: 0,
            parent_hash: None,
            epoch_at_write: 0,
            event_type,
            authorization: aura_authentication::EventAuthorization::DeviceCertificate {
                device_id,
                signature,
            },
        }
    }
}

/// Test helpers for async operations
pub mod async_helpers {
    use tokio::time::{timeout, Duration};

    /// Run an async operation with a timeout
    pub async fn with_timeout<F, T>(duration_secs: u64, future: F) -> Result<T, String>
    where
        F: std::future::Future<Output = T>,
    {
        timeout(Duration::from_secs(duration_secs), future)
            .await
            .map_err(|_| format!("Operation timed out after {} seconds", duration_secs))
    }
}

/// Protocol-specific test helpers
pub mod protocol_helpers {
    use aura_crypto::Effects;
    use aura_protocol::types::ThresholdConfig;
    use aura_types::{DeviceId, GuardianId, GuardianIdExt};
    use uuid::Uuid;

    /// Create a test DKD context
    pub fn create_dkd_test_context(
        app_id: String,
        context: String,
        participants: Vec<DeviceId>,
    ) -> (DeviceId, String, String, Vec<DeviceId>) {
        let device_id = DeviceId(Uuid::new_v4());
        (device_id, app_id, context, participants)
    }

    /// Create test recovery guardians
    pub fn create_test_guardians(count: usize) -> Vec<GuardianId> {
        let effects = Effects::test();
        (0..count)
            .map(|_| GuardianId::new_with_effects(&effects))
            .collect()
    }

    /// Create test threshold configuration
    pub fn create_test_threshold_config(
        threshold: u16,
        total_participants: u16,
    ) -> ThresholdConfig {
        ThresholdConfig {
            threshold,
            total_participants,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_protocol::execution::ProtocolContext;

    #[tokio::test]
    async fn test_fixture_creation() {
        let fixture = TestFixture::new();
        assert_eq!(fixture.participants.len(), 3);
        assert_eq!(fixture.threshold, 2);
    }

    #[tokio::test]
    async fn test_context_creation() {
        let fixture = TestFixture::new();
        let context = fixture.create_context().await;

        match context {
            ProtocolContext::Dkd(dkd_ctx) => {
                assert_eq!(dkd_ctx.participants.len(), 3);
                assert_eq!(dkd_ctx.threshold, Some(2));
            }
            _ => panic!("Expected Dkd context"),
        }
    }

    #[test]
    fn test_mock_event_creation() {
        use super::mocks::mock_event;
        use aura_journal::{EpochTickEvent, EventType};

        let epoch_tick = EpochTickEvent {
            new_epoch: 1,
            evidence_hash: [0u8; 32],
        };
        let event = mock_event(EventType::EpochTick(epoch_tick));
        assert_eq!(event.version, 1);
    }
}
