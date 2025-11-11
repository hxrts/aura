//! Integration tests for capability soundness verification
//!
//! This test suite verifies that the capability soundness verification harness
//! properly validates the security properties of the Aura protocol.

use aura_core::{
    identifiers::DeviceId,
    ledger::{Cap, Fact, Journal},
    MessageContext, FactValue,
};
use aura_protocol::verification::{
    CapabilitySoundnessVerifier, CapabilityState, SoundnessProperty, VerificationConfig,
};
use std::{collections::BTreeMap, time::Duration};

/// Create a test capability state for verification
fn create_test_capability_state() -> CapabilityState {
    let device = DeviceId::new();
    let caps = Cap::with_permissions(vec![
        "journal:read".to_string(),
        "journal:write".to_string(),
        "network:send".to_string(),
    ]);

    CapabilityState {
        capabilities: caps,
        journal_facts: Fact::with_value("test_key", FactValue::String("test_value".to_string())),
        timestamp: 1000,
        active_contexts: ["test_context", "protocol_context"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        auth_levels: [(device, 2)].iter().cloned().collect(),
    }
}

/// Create a restricted capability state for testing
fn create_restricted_capability_state() -> CapabilityState {
    let device = DeviceId::new();
    let caps = Cap::with_permissions(vec!["journal:read".to_string()]);

    CapabilityState {
        capabilities: caps,
        journal_facts: Fact::with_value("restricted", FactValue::String("value".to_string())),
        timestamp: 1000,
        active_contexts: ["restricted_context"].iter().map(|s| s.to_string()).collect(),
        auth_levels: [(device, 1)].iter().cloned().collect(),
    }
}

#[tokio::test]
async fn test_non_interference_property_verification() {
    let mut verifier = CapabilitySoundnessVerifier::with_defaults();
    let initial_state = create_test_capability_state();

    let result = verifier
        .verify_property(SoundnessProperty::NonInterference, initial_state)
        .await
        .expect("Verification should succeed");

    assert_eq!(result.property, SoundnessProperty::NonInterference);
    assert!(result.confidence >= 0.0);
    assert!(result.confidence <= 1.0);
    
    // Should have either evidence or counterexamples
    assert!(
        !result.evidence.is_empty() || !result.counterexamples.is_empty(),
        "Verification should provide evidence or counterexamples"
    );

    // Statistics should be meaningful
    assert!(result.statistics.operations_tested > 0);
    assert!(result.statistics.verification_duration > Duration::from_millis(0));
}

#[tokio::test]
async fn test_monotonicity_property_verification() {
    let mut verifier = CapabilitySoundnessVerifier::with_defaults();
    let initial_state = create_test_capability_state();

    let result = verifier
        .verify_property(SoundnessProperty::Monotonicity, initial_state)
        .await
        .expect("Verification should succeed");

    assert_eq!(result.property, SoundnessProperty::Monotonicity);
    assert!(result.holds, "Monotonicity property should hold for well-formed capability system");
    assert!(result.confidence > 0.0);
}

#[tokio::test]
async fn test_temporal_consistency_property_verification() {
    let mut verifier = CapabilitySoundnessVerifier::with_defaults();
    let initial_state = create_test_capability_state();

    let result = verifier
        .verify_property(SoundnessProperty::TemporalConsistency, initial_state)
        .await
        .expect("Verification should succeed");

    assert_eq!(result.property, SoundnessProperty::TemporalConsistency);
    assert!(result.confidence >= 0.0);
    
    // Should track temporal scenarios
    assert!(
        result.evidence.iter().any(|e| e.contains("time")) ||
        result.counterexamples.iter().any(|ce| ce.description.contains("expired")),
        "Temporal verification should check time-based scenarios"
    );
}

#[tokio::test]
async fn test_context_isolation_property_verification() {
    let mut verifier = CapabilitySoundnessVerifier::with_defaults();
    let initial_state = create_test_capability_state();

    let result = verifier
        .verify_property(SoundnessProperty::ContextIsolation, initial_state)
        .await
        .expect("Verification should succeed");

    assert_eq!(result.property, SoundnessProperty::ContextIsolation);
    assert!(result.confidence >= 0.0);
    
    // Should verify context isolation
    assert!(
        result.evidence.iter().any(|e| e.contains("context")) ||
        result.counterexamples.iter().any(|ce| ce.description.contains("isolation")),
        "Context isolation verification should check context boundaries"
    );
}

#[tokio::test]
async fn test_authorization_soundness_property_verification() {
    let mut verifier = CapabilitySoundnessVerifier::with_defaults();
    let initial_state = create_test_capability_state();

    let result = verifier
        .verify_property(SoundnessProperty::AuthorizationSoundness, initial_state)
        .await
        .expect("Verification should succeed");

    assert_eq!(result.property, SoundnessProperty::AuthorizationSoundness);
    assert!(result.confidence >= 0.0);
    
    // Should verify authorization scenarios
    assert!(
        result.evidence.iter().any(|e| e.contains("authorization") || e.contains("authorized")) ||
        result.counterexamples.iter().any(|ce| ce.description.contains("authorization")),
        "Authorization soundness should check authorization scenarios"
    );
}

#[tokio::test]
async fn test_verify_all_properties_comprehensive() {
    let mut verifier = CapabilitySoundnessVerifier::with_defaults();
    let initial_state = create_test_capability_state();

    let results = verifier
        .verify_all_properties(initial_state)
        .await
        .expect("Verification should succeed");

    // Should verify all 5 properties
    assert_eq!(results.len(), 5);

    // Check that all properties are covered
    let properties: std::collections::HashSet<_> = results.iter().map(|r| r.property.clone()).collect();
    assert!(properties.contains(&SoundnessProperty::NonInterference));
    assert!(properties.contains(&SoundnessProperty::Monotonicity));
    assert!(properties.contains(&SoundnessProperty::TemporalConsistency));
    assert!(properties.contains(&SoundnessProperty::ContextIsolation));
    assert!(properties.contains(&SoundnessProperty::AuthorizationSoundness));

    // All results should have meaningful statistics
    for result in &results {
        assert!(result.statistics.operations_tested > 0);
        assert!(result.statistics.verification_duration >= Duration::from_millis(0));
        assert!(result.confidence >= 0.0 && result.confidence <= 1.0);
    }
}

#[tokio::test]
async fn test_verification_with_restricted_capabilities() {
    let mut verifier = CapabilitySoundnessVerifier::with_defaults();
    let restricted_state = create_restricted_capability_state();

    let result = verifier
        .verify_property(SoundnessProperty::NonInterference, restricted_state)
        .await
        .expect("Verification should succeed");

    assert_eq!(result.property, SoundnessProperty::NonInterference);
    // Verification should still work with restricted capabilities
    assert!(result.confidence >= 0.0);
}

#[tokio::test]
async fn test_verification_with_custom_config() {
    let config = VerificationConfig {
        max_states: 100,
        max_duration: Duration::from_millis(500),
        min_confidence: 0.8,
        collect_counterexamples: true,
        random_seed: 123,
    };

    let mut verifier = CapabilitySoundnessVerifier::new(config);
    let initial_state = create_test_capability_state();

    let result = verifier
        .verify_property(SoundnessProperty::NonInterference, initial_state)
        .await
        .expect("Verification should succeed");

    // Should respect the custom configuration
    assert!(result.statistics.verification_duration <= Duration::from_millis(500));
    assert!(result.statistics.states_explored <= 100);
}

#[tokio::test]
async fn test_soundness_report_generation() {
    let mut verifier = CapabilitySoundnessVerifier::with_defaults();
    let initial_state = create_test_capability_state();

    // Perform multiple verifications
    let _ = verifier
        .verify_property(SoundnessProperty::NonInterference, initial_state.clone())
        .await
        .expect("Verification should succeed");

    let _ = verifier
        .verify_property(SoundnessProperty::Monotonicity, initial_state)
        .await
        .expect("Verification should succeed");

    // Generate comprehensive report
    let report = verifier.generate_soundness_report();

    assert_eq!(report.total_verifications, 2);
    assert!(report.overall_confidence >= 0.0);
    assert!(report.overall_confidence <= 1.0);
    assert_eq!(report.property_results.len(), 2);
    assert!(!report.recommendations.is_empty());
}

#[tokio::test]
async fn test_verification_history_tracking() {
    let mut verifier = CapabilitySoundnessVerifier::with_defaults();
    let initial_state = create_test_capability_state();

    // Initially no history
    assert_eq!(verifier.verification_history().len(), 0);

    // Perform verification
    let _ = verifier
        .verify_property(SoundnessProperty::NonInterference, initial_state)
        .await
        .expect("Verification should succeed");

    // History should be tracked
    let history = verifier.verification_history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].property, SoundnessProperty::NonInterference);
}

#[tokio::test]
async fn test_verification_with_multiple_devices() {
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();
    
    let caps = Cap::with_permissions(vec!["multi:device".to_string()]);
    let multi_device_state = CapabilityState {
        capabilities: caps,
        journal_facts: Fact::with_value("multi", FactValue::String("devices".to_string())),
        timestamp: 1000,
        active_contexts: ["multi_device_context"].iter().map(|s| s.to_string()).collect(),
        auth_levels: [
            (device_a, 2),
            (device_b, 1),
        ].iter().cloned().collect(),
    };

    let mut verifier = CapabilitySoundnessVerifier::with_defaults();

    let result = verifier
        .verify_property(SoundnessProperty::AuthorizationSoundness, multi_device_state)
        .await
        .expect("Verification should succeed");

    assert_eq!(result.property, SoundnessProperty::AuthorizationSoundness);
    assert!(result.confidence >= 0.0);
}

#[tokio::test]
async fn test_verification_performance_metrics() {
    let mut verifier = CapabilitySoundnessVerifier::with_defaults();
    let initial_state = create_test_capability_state();

    let start_time = std::time::Instant::now();
    
    let result = verifier
        .verify_all_properties(initial_state)
        .await
        .expect("Verification should succeed");

    let total_duration = start_time.elapsed();

    // Performance characteristics
    assert!(total_duration < Duration::from_secs(5), "Verification should complete reasonably quickly");
    
    // Each verification should have performance data
    for result in &result {
        assert!(result.statistics.verification_duration >= Duration::from_millis(0));
        assert!(result.statistics.states_explored > 0);
        assert!(result.statistics.operations_tested > 0);
    }
}

#[tokio::test]
async fn test_verification_coverage_metrics() {
    let mut verifier = CapabilitySoundnessVerifier::with_defaults();
    let initial_state = create_test_capability_state();

    let result = verifier
        .verify_property(SoundnessProperty::NonInterference, initial_state)
        .await
        .expect("Verification should succeed");

    let coverage = &result.statistics.coverage_metrics;
    
    // Coverage metrics should be in valid ranges
    assert!(coverage.capability_coverage >= 0.0 && coverage.capability_coverage <= 1.0);
    assert!(coverage.operation_coverage >= 0.0 && coverage.operation_coverage <= 1.0);
    assert!(coverage.context_coverage >= 0.0 && coverage.context_coverage <= 1.0);
    assert!(coverage.temporal_coverage >= 0.0 && coverage.temporal_coverage <= 1.0);
}