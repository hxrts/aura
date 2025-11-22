//! DKD Integration Test
//!
//! Integration test for distributed key derivation functionality.

use anyhow::Result;
use aura_authenticate::{create_test_config, execute_simple_dkd, DkdProtocol};
use aura_core::DeviceId;
use aura_macros::aura_test;
use uuid::Uuid;

/// Test DKD functionality through effects system
/// Currently ignored as it requires full network effects implementation
#[aura_test]
#[ignore]
async fn test_dkd_integration() -> Result<()> {
    // Create test participants
    let participants = vec![
        DeviceId(Uuid::from_bytes([1u8; 16])),
        DeviceId(Uuid::from_bytes([2u8; 16])),
        DeviceId(Uuid::from_bytes([3u8; 16])),
    ];

    // Create test fixture
    let fixture = aura_testkit::create_test_fixture_with_device_id(participants[0])
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let effects = fixture.effect_system_direct();

    // Test parameters
    let app_id = "test_app";
    let context = "test_context";

    // Execute DKD protocol using the real implementation
    let result = execute_simple_dkd(&effects, participants.clone(), app_id, context).await?;

    // Verify results
    assert_eq!(result.participant_count, 3);
    assert!(result.participant_count >= 2); // threshold
    assert_eq!(result.derived_key.len(), 32);
    assert!(!result.verification_proof.is_empty());

    // Verify session ID is set
    assert!(!result.session_id.0.is_empty());

    // Verify combined commitment is deterministic
    assert_eq!(result.combined_commitment.0.len(), 32);

    Ok(())
}

/// Test DKD configuration and protocol setup
#[aura_test]
async fn test_dkd_protocol_setup() -> Result<()> {
    // Test configuration creation
    let config = create_test_config(2, 3);
    assert_eq!(config.threshold, 2);
    assert_eq!(config.total_participants, 3);
    assert_eq!(config.app_id, "test_app");
    assert_eq!(config.context, "test_context");

    // Test protocol creation
    let protocol = DkdProtocol::new(config);
    assert_eq!(protocol.active_session_count(), 0);

    Ok(())
}

/// Test DKD session lifecycle
#[aura_test]
async fn test_dkd_session_lifecycle() -> Result<()> {
    let participants = vec![
        DeviceId(Uuid::from_bytes([1u8; 16])),
        DeviceId(Uuid::from_bytes([2u8; 16])),
    ];

    let fixture = aura_testkit::create_test_fixture_with_device_id(participants[0])
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let effects = fixture.effect_system_direct();

    let config = create_test_config(2, 2);
    let mut protocol = DkdProtocol::new(config);

    // Test session initiation
    let session_id = protocol
        .initiate_session(&effects, participants.clone(), None)
        .await?;
    assert!(protocol.is_session_active(&session_id));
    assert_eq!(protocol.active_session_count(), 1);

    Ok(())
}
