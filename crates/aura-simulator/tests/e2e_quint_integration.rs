//! End-to-end integration test demonstrating simulator basic functionality
//!
//! This test validates the basic simulator effect system

use aura_macros::aura_test;
use aura_core::effects::TimeEffects;
use aura_testkit::{DeviceTestFixture, TestEffectsBuilder};

#[aura_test]
async fn test_simulator_effect_composition_basic() -> aura_core::AuraResult<()> {
    // Create a basic effect system for testing
    let fixture = DeviceTestFixture::new(0);
    let effects_builder = TestEffectsBuilder::for_unit_tests(fixture.device_id());
    let effects = effects_builder.build()?;

    // Test basic time effect
    let timestamp = effects.current_timestamp().await;

    assert!(timestamp >= 0);

    println!("[OK] Simulator effect composition test completed");

    Ok(())
}

#[aura_test]
async fn test_simulator_full_effect_composition() -> aura_core::AuraResult<()> {
    // Test all effect handlers together
    let fixture = DeviceTestFixture::new(1);
    let effects_builder = TestEffectsBuilder::for_unit_tests(fixture.device_id());
    let effects = effects_builder.build()?;

    // Test time effects
    let timestamp = effects.current_timestamp().await;
    assert!(timestamp >= 0);

    // Test crypto effects
    use aura_core::effects::CryptoEffects;
    let (private_key, public_key) = effects.ed25519_generate_keypair().await?;
    assert!(!private_key.is_empty());
    assert!(!public_key.is_empty());

    // Test storage effects
    use aura_core::effects::StorageEffects;
    let test_key = "test_key";
    let test_value = b"test_value".to_vec();
    effects.store(test_key, test_value.clone()).await?;

    // Test console effects  
    use aura_core::effects::ConsoleEffects;
    effects.log_info("Integration test completed successfully").await?;

    println!("[OK] Full effect composition test completed");
    Ok(())
}
