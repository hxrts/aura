//! Integration test runner for aura-sync
//!
//! This module serves as the entry point for all integration tests,
//! ensuring they can be discovered and run by the test harness.

// Import all integration test modules
mod integration;

// Re-export for test discovery
pub use integration::*;

#[cfg(test)]
mod test_discovery {
    //! Ensure integration tests are discoverable by the test runner

    use super::*;

    #[tokio::test]
    async fn test_integration_modules_compile() {
        // This test ensures all integration test modules compile correctly
        // and their dependencies are properly resolved
        println!("Integration test modules compiled successfully");
    }

    #[tokio::test]
    async fn test_utility_functions() {
        // Test basic utility functions used across integration tests
        let config = test_sync_config();
        assert!(config.network.sync_timeout > std::time::Duration::ZERO);

        let session_manager = test_session_manager();
        // Session manager should be created successfully
        drop(session_manager);

        println!("Integration test utilities working correctly");
    }
}

#[cfg(test)]
mod integration_test_examples {
    //! Example tests demonstrating the integration test framework

    use super::*;
    use aura_core::AuraResult;
    use std::time::Duration;

    #[tokio::test]
    async fn example_basic_test_setup() -> AuraResult<()> {
        // Example showing how to set up a basic integration test
        let fixture = test_utils::MultiDeviceTestFixture::trio().await?;

        // Test should be able to create devices and sessions
        assert_eq!(fixture.devices.len(), 3);

        let session = fixture.create_coordinated_session("example").await?;
        println!("Created test session");

        // Clean completion
        fixture
            .wait_for_session_completion(&session, Duration::from_secs(10))
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn example_network_simulation() -> AuraResult<()> {
        // Example showing network simulation capabilities
        let mut fixture = test_utils::MultiDeviceTestFixture::trio().await?;

        let device1 = fixture.devices[0];
        let device2 = fixture.devices[1];

        // Test network condition setting
        let poor_conditions = aura_testkit::simulation::network::NetworkCondition::poor();
        fixture
            .set_network_condition(device1, device2, poor_conditions)
            .await;

        // Test partition creation and healing
        fixture.create_partition(vec![device1], vec![device2]).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        fixture.heal_partitions().await;

        Ok(())
    }
}
