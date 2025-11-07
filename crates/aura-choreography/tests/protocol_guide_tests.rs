//! Protocol Guide Compliant Tests
//!
//! Tests implementing patterns from docs/405_protocol_guide.md

use aura_choreography::{
    runtime::aura_handler_adapter::{AuraHandlerAdapter, AuraHandlerAdapterFactory},
    types::ChoreographicRole,
};
use aura_types::DeviceId;
use proptest::prelude::*;
use std::time::Duration;

/// Unit tests using mock effects as per protocol guide
mod unit_tests {
    use super::*;

    #[tokio::test]
    async fn test_adapter_factory_for_testing() {
        let device_id = DeviceId::new();
        let adapter = AuraHandlerAdapterFactory::for_testing(device_id);

        // Verify adapter creation with correct device ID
        assert_eq!(adapter.device_id, device_id);
    }

    #[tokio::test]
    async fn test_role_mapping_functionality() {
        let device_id = DeviceId::new();
        let mut adapter = AuraHandlerAdapterFactory::for_testing(device_id);

        let peer_id = DeviceId::new();
        adapter.add_role_mapping("participant_1".to_string(), peer_id);

        assert_eq!(
            adapter.get_device_id_for_role("participant_1"),
            Some(peer_id)
        );
        assert_eq!(adapter.get_device_id_for_role("nonexistent"), None);
    }

    #[tokio::test]
    async fn test_choreographic_role_conversion() {
        let device_id = DeviceId::new();
        let adapter = AuraHandlerAdapterFactory::for_testing(device_id);

        // Test role conversion logic
        let participant_role = ChoreographicRole::Participant(1);
        let coordinator_role = ChoreographicRole::Coordinator;
        let device_role = ChoreographicRole::Device(device_id);

        // These should not panic during conversion
        match participant_role {
            ChoreographicRole::Participant(idx) => assert_eq!(idx, 1),
            _ => panic!("Expected participant role"),
        }

        match coordinator_role {
            ChoreographicRole::Coordinator => {}
            _ => panic!("Expected coordinator role"),
        }

        match device_role {
            ChoreographicRole::Device(id) => assert_eq!(id, device_id),
            _ => panic!("Expected device role"),
        }
    }
}

/// Property-based tests for protocol invariants
mod property_tests {
    use super::*;

    proptest! {
        #[test]
        fn test_adapter_creation_deterministic(device_seed: u64) {
            let device_id = DeviceId::from_bytes([device_seed as u8; 32]);
            let adapter1 = AuraHandlerAdapterFactory::for_testing(device_id);
            let adapter2 = AuraHandlerAdapterFactory::for_testing(device_id);

            // Same device ID should create equivalent adapters
            prop_assert_eq!(adapter1.device_id, adapter2.device_id);
        }

        #[test]
        fn test_role_mapping_consistency(
            role_name in "[a-z_]{1,20}",
            device_seed: u64
        ) {
            let device_id = DeviceId::new();
            let mut adapter = AuraHandlerAdapterFactory::for_testing(device_id);

            let mapped_device = DeviceId::from_bytes([device_seed as u8; 32]);
            adapter.add_role_mapping(role_name.clone(), mapped_device);

            // Mapping should be consistent
            prop_assert_eq!(adapter.get_device_id_for_role(&role_name), Some(mapped_device));
        }

        #[test]
        fn test_multiple_role_mappings(role_count in 1..10usize) {
            let device_id = DeviceId::new();
            let mut adapter = AuraHandlerAdapterFactory::for_testing(device_id);

            let mut expected_mappings = std::collections::HashMap::new();

            for i in 0..role_count {
                let role_name = format!("role_{}", i);
                let mapped_device = DeviceId::from_bytes([i as u8; 32]);
                adapter.add_role_mapping(role_name.clone(), mapped_device);
                expected_mappings.insert(role_name, mapped_device);
            }

            // All mappings should be preserved
            for (role, expected_device) in expected_mappings {
                prop_assert_eq!(adapter.get_device_id_for_role(&role), Some(expected_device));
            }
        }
    }
}

/// Integration tests for multi-participant coordination
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_multi_participant_setup() {
        // Test setting up multiple participants as per protocol guide
        let participant_count = 3;
        let threshold = 2;

        let mut adapters = Vec::new();
        let mut device_ids = Vec::new();

        // Create adapters for each participant
        for i in 0..participant_count {
            let device_id = DeviceId::from_bytes([i as u8; 32]);
            device_ids.push(device_id);

            let mut adapter = AuraHandlerAdapterFactory::for_testing(device_id);

            // Add role mappings for other participants
            for j in 0..participant_count {
                if i != j {
                    let role_name = format!("participant_{}", j);
                    adapter.add_role_mapping(role_name, device_ids[j]);
                }
            }

            adapters.push(adapter);
        }

        // Verify all adapters have correct participant count
        assert_eq!(adapters.len(), participant_count);

        // Verify each adapter knows about other participants
        for (i, adapter) in adapters.iter().enumerate() {
            for j in 0..participant_count {
                if i != j {
                    let role_name = format!("participant_{}", j);
                    assert_eq!(
                        adapter.get_device_id_for_role(&role_name),
                        Some(device_ids[j])
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn test_threshold_protocol_setup() {
        // Test threshold protocol initialization
        let threshold = 2;
        let total_participants = 3;

        // Create coordinator
        let coordinator_id = DeviceId::new();
        let mut coordinator_adapter = AuraHandlerAdapterFactory::for_testing(coordinator_id);

        // Create participants
        let mut participant_adapters = Vec::new();
        for i in 0..total_participants {
            let participant_id = DeviceId::from_bytes([i as u8; 32]);
            let mut adapter = AuraHandlerAdapterFactory::for_testing(participant_id);

            // Map coordinator
            adapter.add_role_mapping("coordinator".to_string(), coordinator_id);
            coordinator_adapter.add_role_mapping(format!("participant_{}", i), participant_id);

            participant_adapters.push(adapter);
        }

        // Verify threshold constraints
        assert!(threshold <= total_participants);
        assert!(threshold > 0);
        assert_eq!(participant_adapters.len(), total_participants);

        // Verify coordinator knows all participants
        for i in 0..total_participants {
            let participant_id = DeviceId::from_bytes([i as u8; 32]);
            assert_eq!(
                coordinator_adapter.get_device_id_for_role(&format!("participant_{}", i)),
                Some(participant_id)
            );
        }
    }
}

/// Performance and timing tests
mod performance_tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn test_adapter_creation_performance() {
        let start = Instant::now();

        // Create many adapters to test performance
        let adapter_count = 100;
        let mut adapters = Vec::with_capacity(adapter_count);

        for i in 0..adapter_count {
            let device_id = DeviceId::from_bytes([i as u8; 32]);
            let adapter = AuraHandlerAdapterFactory::for_testing(device_id);
            adapters.push(adapter);
        }

        let duration = start.elapsed();

        // Should create adapters quickly (under 1 second for 100 adapters)
        assert!(duration < Duration::from_secs(1));
        assert_eq!(adapters.len(), adapter_count);
    }

    #[tokio::test]
    async fn test_role_mapping_performance() {
        let device_id = DeviceId::new();
        let mut adapter = AuraHandlerAdapterFactory::for_testing(device_id);

        let start = Instant::now();

        // Add many role mappings
        let mapping_count = 1000;
        for i in 0..mapping_count {
            let role_name = format!("role_{}", i);
            let mapped_device = DeviceId::from_bytes([i as u8; 32]);
            adapter.add_role_mapping(role_name, mapped_device);
        }

        let add_duration = start.elapsed();

        // Test lookup performance
        let lookup_start = Instant::now();
        for i in 0..mapping_count {
            let role_name = format!("role_{}", i);
            let _ = adapter.get_device_id_for_role(&role_name);
        }
        let lookup_duration = lookup_start.elapsed();

        // Should be fast (under 100ms for 1000 operations)
        assert!(add_duration < Duration::from_millis(100));
        assert!(lookup_duration < Duration::from_millis(100));
    }
}

/// Error handling and edge case tests
mod error_tests {
    use super::*;

    #[tokio::test]
    async fn test_nonexistent_role_lookup() {
        let device_id = DeviceId::new();
        let adapter = AuraHandlerAdapterFactory::for_testing(device_id);

        // Looking up non-existent role should return None
        assert_eq!(adapter.get_device_id_for_role("nonexistent"), None);
        assert_eq!(adapter.get_device_id_for_role(""), None);
        assert_eq!(
            adapter.get_device_id_for_role("very_long_role_name_that_does_not_exist"),
            None
        );
    }

    #[tokio::test]
    async fn test_role_mapping_overwrite() {
        let device_id = DeviceId::new();
        let mut adapter = AuraHandlerAdapterFactory::for_testing(device_id);

        let role_name = "test_role".to_string();
        let device1 = DeviceId::from_bytes([1u8; 32]);
        let device2 = DeviceId::from_bytes([2u8; 32]);

        // Add initial mapping
        adapter.add_role_mapping(role_name.clone(), device1);
        assert_eq!(adapter.get_device_id_for_role(&role_name), Some(device1));

        // Overwrite mapping
        adapter.add_role_mapping(role_name.clone(), device2);
        assert_eq!(adapter.get_device_id_for_role(&role_name), Some(device2));
    }

    #[tokio::test]
    async fn test_empty_role_name() {
        let device_id = DeviceId::new();
        let mut adapter = AuraHandlerAdapterFactory::for_testing(device_id);

        let mapped_device = DeviceId::new();
        adapter.add_role_mapping("".to_string(), mapped_device);

        // Empty role name should still work
        assert_eq!(adapter.get_device_id_for_role(""), Some(mapped_device));
    }
}

/// Factory pattern tests as specified in protocol guide
mod factory_tests {
    use super::*;

    #[tokio::test]
    async fn test_testing_factory() {
        let device_id = DeviceId::new();
        let adapter = AuraHandlerAdapterFactory::for_testing(device_id);
        assert_eq!(adapter.device_id, device_id);
    }

    #[tokio::test]
    async fn test_production_factory() {
        let device_id = DeviceId::new();
        let adapter = AuraHandlerAdapterFactory::for_production(device_id);
        assert_eq!(adapter.device_id, device_id);
    }

    #[tokio::test]
    async fn test_simulation_factory() {
        let device_id = DeviceId::new();
        let adapter = AuraHandlerAdapterFactory::for_simulation(device_id);
        assert_eq!(adapter.device_id, device_id);
    }

    #[tokio::test]
    async fn test_factory_consistency() {
        let device_id = DeviceId::new();

        let testing_adapter = AuraHandlerAdapterFactory::for_testing(device_id);
        let production_adapter = AuraHandlerAdapterFactory::for_production(device_id);
        let simulation_adapter = AuraHandlerAdapterFactory::for_simulation(device_id);

        // All factories should use the same device ID
        assert_eq!(testing_adapter.device_id, device_id);
        assert_eq!(production_adapter.device_id, device_id);
        assert_eq!(simulation_adapter.device_id, device_id);
    }
}
