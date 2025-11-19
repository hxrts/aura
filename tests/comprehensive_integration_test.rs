//! Comprehensive Integration Test Suite
//!
//! This test suite verifies end-to-end functionality across all major Aura components,
//! ensuring that the distributed protocol implementation works correctly across
//! realistic scenarios.

use aura_core::{
    identifiers::{DeviceId, SessionId},
    ledger::{Cap, Fact, Journal},
    MessageContext, FactValue, AuraResult,
};
use aura_agent::runtime::AuraEffectSystem;
use aura_protocol::{
    authorization_bridge::{authenticate_and_authorize, AuthorizationContext, AuthorizationRequest},
    guards::{evaluate_guard, ProtocolGuard},
    handlers::{CompositeHandler, ExecutionMode},
    verification::{CapabilitySoundnessVerifier, SoundnessProperty, VerificationConfig},
    middleware::{CircuitBreakerConfig, CircuitBreakerMiddleware},
};
use std::{collections::HashMap, time::Duration};
use tokio::time::timeout;

/// Comprehensive integration test configuration
#[derive(Debug, Clone)]
struct IntegrationTestConfig {
    /// Number of devices in the test
    device_count: usize,
    /// Test execution timeout
    timeout_duration: Duration,
    /// Whether to enable circuit breaker testing
    test_circuit_breakers: bool,
    /// Whether to run formal verification
    run_verification: bool,
}

impl Default for IntegrationTestConfig {
    fn default() -> Self {
        Self {
            device_count: 3,
            timeout_duration: Duration::from_secs(30),
            test_circuit_breakers: true,
            run_verification: true,
        }
    }
}

/// Test device with associated components
struct TestDevice {
    device_id: DeviceId,
    effect_system: CompositeHandler,
    journal: Journal,
    capabilities: Cap,
}

impl TestDevice {
    /// Create a new test device with basic capabilities
    fn new() -> Self {
        let device_id = DeviceId::new();
        let effect_system = CompositeHandler::for_testing(device_id);
        
        let capabilities = Cap::with_permissions(vec![
            "journal:read".to_string(),
            "journal:write".to_string(),
            "network:send".to_string(),
            "network:receive".to_string(),
        ]);
        
        let mut facts = Fact::new();
        facts = facts.with_value("device_id", FactValue::String(device_id.to_string()));
        facts = facts.with_value("initialized", FactValue::Boolean(true));
        
        let journal = Journal::with_caps_and_facts(capabilities.clone(), facts);
        
        Self {
            device_id,
            effect_system,
            journal,
            capabilities,
        }
    }
    
    /// Create device with enhanced capabilities for testing
    fn with_enhanced_capabilities() -> Self {
        let mut device = Self::new();
        
        let enhanced_caps = Cap::with_permissions(vec![
            "journal:read".to_string(),
            "journal:write".to_string(),
            "journal:admin".to_string(),
            "network:send".to_string(),
            "network:receive".to_string(),
            "network:broadcast".to_string(),
            "crypto:sign".to_string(),
            "crypto:verify".to_string(),
        ]);
        
        device.capabilities = enhanced_caps.clone();
        device.journal = Journal::with_caps_and_facts(enhanced_caps, device.journal.facts);
        
        device
    }
}

/// Integration test suite for distributed protocol functionality
#[tokio::test]
async fn test_end_to_end_device_communication() {
    let config = IntegrationTestConfig::default();
    
    let result = timeout(config.timeout_duration, async {
        // Setup test devices
        let device_a = TestDevice::new();
        let device_b = TestDevice::new();
        let device_c = TestDevice::new();
        
        // Test basic device creation
        assert_ne!(device_a.device_id, device_b.device_id);
        assert_ne!(device_b.device_id, device_c.device_id);
        
        // Test capability verification
        assert!(device_a.capabilities.allows("journal:read"));
        assert!(device_a.capabilities.allows("network:send"));
        
        // Test effect system functionality
        let random_data = device_a.effect_system.random_bytes(32).await?;
        assert_eq!(random_data.len(), 32);
        
        // Test cross-device communication simulation
        let message = b"Hello from device A".to_vec();
        device_a.effect_system.send_to_peer(device_b.device_id, message.clone()).await?;
        
        // Test journal operations
        assert!(device_a.journal.facts.contains_key("device_id"));
        assert!(device_a.journal.facts.contains_key("initialized"));
        
        AuraResult::Ok(())
    }).await;
    
    match result {
        Ok(inner_result) => {
            inner_result.expect("End-to-end communication should succeed");
        }
        Err(_) => {
            panic!("Integration test timed out after {:?}", config.timeout_duration);
        }
    }
}

#[tokio::test]
async fn test_authorization_workflow_integration() {
    let config = IntegrationTestConfig::default();
    
    let result = timeout(config.timeout_duration, async {
        let device = TestDevice::with_enhanced_capabilities();
        
        // Create authorization context
        let context = AuthorizationContext {
            device_id: device.device_id,
            session_id: Some(SessionId::new()),
            journal: device.journal.clone(),
            timestamp: 1000,
        };
        
        // Create authorization request for journal write
        let auth_request = AuthorizationRequest {
            operation: "journal:write".to_string(),
            resource: "facts".to_string(),
            context: MessageContext::dkd_context("test", [0u8; 32]),
            required_permissions: vec!["journal:write".to_string()],
            cost: 1,
        };
        
        // Test authorization workflow
        let auth_result = authenticate_and_authorize(auth_request, &context).await?;
        assert!(auth_result.authorized);
        assert_eq!(auth_result.permission_grants.len(), 1);
        
        // Test guard evaluation
        let guard = ProtocolGuard::new()
            .with_capability_requirement("journal:write")
            .with_cost_limit(10);
            
        let guard_result = evaluate_guard(&guard, &context.journal, &context.context, 1).await?;
        assert!(guard_result.allowed);
        
        AuraResult::Ok(())
    }).await;
    
    match result {
        Ok(inner_result) => {
            inner_result.expect("Authorization workflow should succeed");
        }
        Err(_) => {
            panic!("Authorization test timed out after {:?}", config.timeout_duration);
        }
    }
}

#[tokio::test]
async fn test_circuit_breaker_integration() {
    let config = IntegrationTestConfig {
        test_circuit_breakers: true,
        ..Default::default()
    };
    
    if !config.test_circuit_breakers {
        return;
    }
    
    let result = timeout(config.timeout_duration, async {
        let device = TestDevice::new();
        
        // Create circuit breaker with aggressive settings for testing
        let cb_config = CircuitBreakerConfig {
            failure_threshold: 2,
            timeout_duration: Duration::from_millis(100),
            half_open_max_requests: 1,
            success_threshold: 1,
            respect_flow_budget: true,
        };
        
        let circuit_breaker = CircuitBreakerMiddleware::new(device.effect_system, cb_config);
        
        // Test successful operations
        let result1 = circuit_breaker.random_bytes(16).await;
        assert!(result1.is_ok());
        
        let result2 = circuit_breaker.random_bytes(16).await;
        assert!(result2.is_ok());
        
        // Test circuit breaker statistics
        let stats = circuit_breaker.get_all_circuit_stats();
        assert!(!stats.is_empty());
        
        AuraResult::Ok(())
    }).await;
    
    match result {
        Ok(inner_result) => {
            inner_result.expect("Circuit breaker integration should succeed");
        }
        Err(_) => {
            panic!("Circuit breaker test timed out after {:?}", config.timeout_duration);
        }
    }
}

#[tokio::test]
async fn test_capability_soundness_verification_integration() {
    let config = IntegrationTestConfig {
        run_verification: true,
        ..Default::default()
    };
    
    if !config.run_verification {
        return;
    }
    
    let result = timeout(config.timeout_duration, async {
        let device = TestDevice::with_enhanced_capabilities();
        
        // Create capability state for verification
        let capability_state = aura_protocol::verification::CapabilityState {
            capabilities: device.capabilities.clone(),
            journal_facts: device.journal.facts.clone(),
            timestamp: 1000,
            active_contexts: ["test_context".to_string()].iter().cloned().collect(),
            auth_levels: [(device.device_id, 2)].iter().cloned().collect(),
        };
        
        // Create verifier with quick configuration for testing
        let verification_config = VerificationConfig {
            max_states: 50,
            max_duration: Duration::from_secs(5),
            min_confidence: 0.8,
            collect_counterexamples: true,
            random_seed: 42,
        };
        
        let mut verifier = CapabilitySoundnessVerifier::new(verification_config);
        
        // Test individual property verification
        let non_interference_result = verifier
            .verify_property(SoundnessProperty::NonInterference, capability_state.clone())
            .await?;
            
        assert_eq!(non_interference_result.property, SoundnessProperty::NonInterference);
        assert!(non_interference_result.confidence >= 0.0);
        assert!(non_interference_result.statistics.operations_tested > 0);
        
        // Test monotonicity
        let monotonicity_result = verifier
            .verify_property(SoundnessProperty::Monotonicity, capability_state.clone())
            .await?;
            
        assert_eq!(monotonicity_result.property, SoundnessProperty::Monotonicity);
        assert!(monotonicity_result.confidence >= 0.0);
        
        // Generate comprehensive report
        let report = verifier.generate_soundness_report();
        assert_eq!(report.total_verifications, 2);
        assert!(!report.recommendations.is_empty());
        
        AuraResult::Ok(())
    }).await;
    
    match result {
        Ok(inner_result) => {
            inner_result.expect("Capability verification should succeed");
        }
        Err(_) => {
            panic!("Verification test timed out after {:?}", config.timeout_duration);
        }
    }
}

#[tokio::test]
async fn test_multi_device_distributed_scenario() {
    let config = IntegrationTestConfig {
        device_count: 5,
        timeout_duration: Duration::from_secs(60),
        ..Default::default()
    };
    
    let result = timeout(config.timeout_duration, async {
        // Create multiple test devices
        let devices: Vec<TestDevice> = (0..config.device_count)
            .map(|_| TestDevice::with_enhanced_capabilities())
            .collect();
        
        // Test that all devices have unique IDs
        let device_ids: std::collections::HashSet<_> = devices.iter().map(|d| d.device_id).collect();
        assert_eq!(device_ids.len(), config.device_count);
        
        // Test distributed operations
        for (i, device) in devices.iter().enumerate() {
            // Each device generates some random data
            let data = device.effect_system.random_bytes(16).await?;
            assert_eq!(data.len(), 16);
            
            // Test capability checking
            assert!(device.capabilities.allows("network:send"));
            assert!(device.capabilities.allows("crypto:sign"));
            
            // Test journal state
            assert!(device.journal.facts.contains_key("device_id"));
            
            // Simulate sending to other devices
            for (j, other_device) in devices.iter().enumerate() {
                if i != j {
                    let message = format!("Message from device {} to device {}", i, j).into_bytes();
                    device.effect_system.send_to_peer(other_device.device_id, message).await?;
                }
            }
        }
        
        AuraResult::Ok(())
    }).await;
    
    match result {
        Ok(inner_result) => {
            inner_result.expect("Multi-device scenario should succeed");
        }
        Err(_) => {
            panic!("Multi-device test timed out after {:?}", config.timeout_duration);
        }
    }
}

#[tokio::test]
async fn test_error_handling_and_resilience() {
    let config = IntegrationTestConfig::default();
    
    let result = timeout(config.timeout_duration, async {
        let device = TestDevice::new();
        
        // Test handling of operations with insufficient capabilities
        let restricted_caps = Cap::with_permissions(vec!["journal:read".to_string()]);
        let restricted_journal = Journal::with_caps(restricted_caps);
        
        let auth_context = AuthorizationContext {
            device_id: device.device_id,
            session_id: Some(SessionId::new()),
            journal: restricted_journal,
            timestamp: 1000,
        };
        
        let write_request = AuthorizationRequest {
            operation: "journal:write".to_string(),
            resource: "facts".to_string(),
            context: MessageContext::dkd_context("test", [0u8; 32]),
            required_permissions: vec!["journal:write".to_string()],
            cost: 1,
        };
        
        // This should fail due to insufficient capabilities
        let auth_result = authenticate_and_authorize(write_request, &auth_context).await;
        match auth_result {
            Ok(result) => {
                assert!(!result.authorized, "Should not authorize without proper capabilities");
            }
            Err(_) => {
                // Expected behavior - operation should fail
            }
        }
        
        // Test system resilience with valid operations
        let valid_data = device.effect_system.random_bytes(32).await?;
        assert_eq!(valid_data.len(), 32);
        
        AuraResult::Ok(())
    }).await;
    
    match result {
        Ok(inner_result) => {
            inner_result.expect("Error handling test should succeed");
        }
        Err(_) => {
            panic!("Error handling test timed out after {:?}", config.timeout_duration);
        }
    }
}

#[tokio::test]
async fn test_performance_characteristics() {
    let config = IntegrationTestConfig {
        timeout_duration: Duration::from_secs(10),
        ..Default::default()
    };
    
    let result = timeout(config.timeout_duration, async {
        let device = TestDevice::with_enhanced_capabilities();
        let start_time = std::time::Instant::now();
        
        // Test rapid operations
        for i in 0..100 {
            let data = device.effect_system.random_bytes(16).await?;
            assert_eq!(data.len(), 16);
            
            // Verify operation doesn't slow down significantly over time
            let current_duration = start_time.elapsed();
            assert!(current_duration < Duration::from_secs(5), 
                "Operations should complete quickly, iteration {}", i);
        }
        
        let total_duration = start_time.elapsed();
        println!("Completed 100 operations in {:?}", total_duration);
        
        // Performance should be reasonable
        assert!(total_duration < Duration::from_secs(5), 
            "100 operations should complete within 5 seconds");
        
        AuraResult::Ok(())
    }).await;
    
    match result {
        Ok(inner_result) => {
            inner_result.expect("Performance test should succeed");
        }
        Err(_) => {
            panic!("Performance test timed out after {:?}", config.timeout_duration);
        }
    }
}

#[tokio::test]
async fn test_system_state_consistency() {
    let config = IntegrationTestConfig::default();
    
    let result = timeout(config.timeout_duration, async {
        let mut device = TestDevice::with_enhanced_capabilities();
        
        // Test initial state consistency
        assert!(device.journal.facts.contains_key("device_id"));
        assert!(device.journal.facts.contains_key("initialized"));
        
        // Test state modifications
        let new_fact = Fact::with_value("test_key", FactValue::String("test_value".to_string()));
        device.journal.facts = device.journal.facts.join(&new_fact);
        
        // Verify state is still consistent
        assert!(device.journal.facts.contains_key("test_key"));
        assert!(device.journal.facts.contains_key("device_id"));
        
        // Test capability consistency
        assert!(device.capabilities.allows("journal:write"));
        assert!(device.capabilities.allows("network:send"));
        
        AuraResult::Ok(())
    }).await;
    
    match result {
        Ok(inner_result) => {
            inner_result.expect("State consistency test should succeed");
        }
        Err(_) => {
            panic!("State consistency test timed out after {:?}", config.timeout_duration);
        }
    }
}