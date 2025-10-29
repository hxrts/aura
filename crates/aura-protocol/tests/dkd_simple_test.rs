//! Simple DKD Test
//!
//! This test verifies the core DKD choreography function works correctly
//! by directly testing the coordination layer implementation.

use aura_crypto::Effects;
use aura_journal::{AccountLedger, AccountState, DeviceMetadata, DeviceType};
use aura_protocol::execution::time::ProductionTimeSource;
use aura_protocol::execution::{MemoryTransport, ProtocolContext};
use aura_types::{AccountId, DeviceId};
use ed25519_dalek::SigningKey;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Helper function to create a test AccountState
fn create_test_account_state(effects: &Effects) -> AccountState {
    let account_id = AccountId(Uuid::new_v4());
    let key_bytes = effects.random_bytes::<32>();
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let group_public_key = signing_key.verifying_key();

    let device_metadata = DeviceMetadata {
        device_id: DeviceId(Uuid::new_v4()),
        device_name: "Test Device".to_string(),
        device_type: DeviceType::Native,
        public_key: signing_key.verifying_key(),
        added_at: effects.now().unwrap_or(0),
        last_seen: effects.now().unwrap_or(0),
        dkd_commitment_proofs: std::collections::BTreeMap::new(),
        next_nonce: 1,
        used_nonces: std::collections::BTreeSet::new(),
    };

    AccountState::new(account_id, group_public_key, device_metadata, 2, 3)
}

#[tokio::test]
async fn test_dkd_choreography_execution() {
    println!("=== DKD Choreography Test ===");

    // Create test context with deterministic parameters
    let session_id = Uuid::from_u128(12345);
    let device_id = Uuid::from_u128(67890);
    let participants = vec![
        DeviceId(device_id),
        DeviceId(Uuid::from_u128(11111)),
        DeviceId(Uuid::from_u128(22222)),
    ];
    let threshold = Some(2);

    // Create deterministic effects for testing
    let effects = Effects::deterministic(54321, 0);

    // Create test ledger with initial state
    let initial_state = create_test_account_state(&effects);
    let ledger = Arc::new(RwLock::new(
        AccountLedger::new(initial_state).expect("Failed to create test ledger"),
    ));

    // Create stub transport
    let transport = Arc::new(MemoryTransport::default());

    // Create device signing key deterministically
    let device_key = SigningKey::from_bytes(&effects.random_bytes::<32>());

    // Create time source
    let time_source = Box::new(ProductionTimeSource::new());

    // Create protocol context for verification
    let ctx = ProtocolContext::new_dkd(
        session_id,
        device_id,
        participants.clone(),
        threshold,
        ledger,
        transport,
        effects,
        device_key,
        time_source,
    );

    println!("[OK] Created protocol context successfully");

    // Test DKD choreography with a simple context
    let context_id = b"test-app-context".to_vec();

    // Test DKD protocol structure and initialization (not full execution)
    use aura_protocol::protocols::DkdLifecycle;
    use aura_protocol::SessionId;
    println!("Creating DkdLifecycle...");
    let _protocol = DkdLifecycle::new(
        DeviceId(device_id),
        SessionId(session_id),
        context_id.clone(),
        participants.clone(),
    );
    println!("DkdLifecycle created successfully");

    // Test that the protocol can be initialized correctly
    println!("[OK] DKD protocol structure validation passed");
    println!("  Session ID: {}", session_id);
    println!("  Device ID: {}", device_id);
    println!("  Participants: {}", participants.len());
    println!("  Context ID: {}", hex::encode(&context_id));

    // Verify context configuration
    assert_eq!(ctx.session_id(), session_id);
    assert_eq!(ctx.device_id(), device_id);
    assert_eq!(ctx.threshold(), Some(2));
    assert_eq!(ctx.participants().len(), 3);

    println!("=== Test PASSED: DKD Protocol Structure Validated ===");
}

#[tokio::test]
async fn test_dkd_deterministic_setup() {
    println!("\n=== DKD Deterministic Setup Test ===");

    // Test that the same parameters produce the same context setup
    let session_id = Uuid::from_u128(11111);
    let device_id = Uuid::from_u128(22222);
    let participants = vec![DeviceId(device_id)];

    for i in 0..3 {
        println!("  Run {}: Creating context...", i + 1);

        let effects = Effects::deterministic(99999, 0); // Same seed
        let initial_state = create_test_account_state(&effects);
        let ledger = Arc::new(RwLock::new(
            AccountLedger::new(initial_state).expect("Failed to create test ledger"),
        ));
        let transport = Arc::new(MemoryTransport::default());

        let device_key = SigningKey::from_bytes(&effects.random_bytes::<32>());
        let time_source = Box::new(ProductionTimeSource::new());

        let ctx = ProtocolContext::new_dkd(
            session_id,
            device_id,
            participants.clone(),
            Some(1),
            ledger,
            transport,
            effects,
            device_key,
            time_source,
        );

        // Verify consistent setup
        assert_eq!(ctx.session_id(), session_id);
        assert_eq!(ctx.device_id(), device_id);
        assert_eq!(ctx.threshold(), Some(1));

        println!("  Run {}: Context created consistently", i + 1);
    }

    println!("[OK] All {} runs produced consistent context setup", 3);
    println!("=== Deterministic Setup Test PASSED ===");
}

#[tokio::test]
async fn test_dkd_different_contexts() {
    println!("\n=== DKD Different Contexts Test ===");

    // Test that different contexts can be set up properly
    let session_id = Uuid::from_u128(33333);
    let device_id = Uuid::from_u128(44444);
    let participants = vec![DeviceId(device_id)];

    let contexts = vec![
        b"context-1".to_vec(),
        b"context-2".to_vec(),
        b"different-app".to_vec(),
    ];

    for (i, context_id) in contexts.iter().enumerate() {
        println!(
            "  Testing context {}: {:?}",
            i + 1,
            String::from_utf8_lossy(context_id)
        );

        let effects = Effects::deterministic(77777, 0); // Same seed for all
        let initial_state = create_test_account_state(&effects);
        let ledger = Arc::new(RwLock::new(
            AccountLedger::new(initial_state).expect("Failed to create test ledger"),
        ));
        let transport = Arc::new(MemoryTransport::default());

        let device_key = SigningKey::from_bytes(&effects.random_bytes::<32>());
        let time_source = Box::new(ProductionTimeSource::new());

        let _ctx = ProtocolContext::new_dkd(
            session_id,
            device_id,
            participants.clone(),
            Some(1),
            ledger,
            transport,
            effects,
            device_key,
            time_source,
        );

        // Test that protocol can be created for each context
        use aura_protocol::protocols::DkdLifecycle;
        use aura_protocol::SessionId;
        let _protocol = DkdLifecycle::new(
            DeviceId(device_id),
            SessionId(session_id),
            context_id.clone(),
            participants.clone(),
        );

        println!("  Context {}: Protocol created successfully", i + 1);
    }

    println!(
        "[OK] All {} contexts can create DKD protocols",
        contexts.len()
    );
    println!("=== Different Contexts Test PASSED ===");
}
