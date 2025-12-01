//! Integration tests for unified time system
//!
//! Tests the complete time system integration including:
//! - Domain-specific time trait implementations
//! - Cross-domain time comparisons and ordering
//! - Time-based authorization and capability constraints
//! - Journal fact ordering with different time domains
//! - Time leakage and privacy properties

use aura_core::effects::time::{
    LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects, TimeError,
};
use aura_core::time::{
    LogicalTime, OrderingPolicy, PhysicalTime, TimeDomain, TimeOrdering, TimeStamp, VectorClock,
};
use aura_core::{AuthorityId, DeviceId};
use aura_testkit::time::{ControllableTimeSource, TimeScenarioBuilder};
use std::collections::BTreeMap;

/// Mock fact content for testing
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct TestFactContent {
    data: String,
}

/// Test suite for unified time system integration
#[tokio::test]
async fn test_physical_time_effects_basic_operations() {
    let time_source = ControllableTimeSource::new(1000);

    // Test basic physical time retrieval
    let physical_time = time_source.physical_time().await.unwrap();
    assert_eq!(physical_time.ts_ms, 1000);
    assert_eq!(physical_time.uncertainty, None);

    // Test time advancement
    time_source.advance_time(500);
    let advanced_time = time_source.physical_time().await.unwrap();
    assert_eq!(advanced_time.ts_ms, 1500);

    // Test sleep functionality
    let sleep_result = time_source.sleep_ms(100).await;
    assert!(sleep_result.is_ok());
}

#[tokio::test]
async fn test_logical_clock_effects_causal_ordering() {
    let time_source = ControllableTimeSource::new(1000);
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();

    // Test logical time advancement
    let logical_time1 = time_source.logical_now().await.unwrap();
    assert!(logical_time1.vector.is_empty());
    assert_eq!(logical_time1.lamport, 0);

    // Advance logical time
    let mut observed_vector = BTreeMap::new();
    observed_vector.insert(device_a, 5);
    observed_vector.insert(device_b, 3);

    let logical_time2 = time_source
        .logical_advance(Some(&VectorClock::Multiple(observed_vector)))
        .await
        .unwrap();
    assert!(logical_time2.lamport > logical_time1.lamport);

    // Verify causal ordering
    let ts1 = TimeStamp::LogicalClock(logical_time1);
    let ts2 = TimeStamp::LogicalClock(logical_time2);

    assert_eq!(
        ts1.compare(&ts2, OrderingPolicy::Native),
        TimeOrdering::Before
    );
    assert_eq!(
        ts2.compare(&ts1, OrderingPolicy::Native),
        TimeOrdering::After
    );
}

#[tokio::test]
async fn test_order_clock_effects_deterministic_ordering() {
    let time_source = ControllableTimeSource::new(1000);

    // Test order clock generation
    let order1 = time_source.order_time().await.unwrap();
    let order2 = time_source.order_time().await.unwrap();

    // Verify ordering is deterministic and total
    let ts1 = TimeStamp::OrderClock(order1);
    let ts2 = TimeStamp::OrderClock(order2);

    let comparison = ts1.compare(&ts2, OrderingPolicy::Native);
    assert!(matches!(
        comparison,
        TimeOrdering::Before | TimeOrdering::After
    ));

    // Verify reflexivity
    assert_eq!(
        ts1.compare(&ts1, OrderingPolicy::Native),
        TimeOrdering::Concurrent
    );
}

#[tokio::test]
async fn test_cross_domain_time_comparisons() {
    let time_source = ControllableTimeSource::new(1000);

    // Create timestamps from different domains
    let physical = TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1000,
        uncertainty: None,
    });
    let logical = TimeStamp::LogicalClock(LogicalTime {
        vector: VectorClock::Multiple(BTreeMap::new()),
        lamport: 500,
    });
    let order = TimeStamp::OrderClock(time_source.order_time().await.unwrap());

    // Test cross-domain comparisons return Incomparable with native policy
    assert_eq!(
        physical.compare(&logical, OrderingPolicy::Native),
        TimeOrdering::Incomparable
    );
    assert_eq!(
        logical.compare(&order, OrderingPolicy::Native),
        TimeOrdering::Incomparable
    );
    assert_eq!(
        physical.compare(&order, OrderingPolicy::Native),
        TimeOrdering::Incomparable
    );

    // Test deterministic tie-break provides total ordering via index conversion
    let physical_index = physical.to_index_ms();
    let logical_index = logical.to_index_ms();

    assert_ne!(physical_index, logical_index); // Should differ for different domains

    // Test sort_compare provides deterministic ordering across domains
    let cmp = physical.sort_compare(&logical, OrderingPolicy::Native);
    assert!(matches!(
        cmp,
        std::cmp::Ordering::Less | std::cmp::Ordering::Greater
    ));
}

#[tokio::test]
async fn test_fact_creation_with_different_time_domains() {
    let time_source = ControllableTimeSource::new(1000);
    let authority = AuthorityId::new_from_entropy([0u8; 32]);
    let content = TestFactContent {
        data: "test".to_string(),
    };

    // Test fact creation with physical time domain
    let physical_fact = create_test_fact_with_domain(
        content.clone(),
        authority,
        TimeDomain::PhysicalClock,
        &time_source,
    )
    .await
    .unwrap();

    match &physical_fact.timestamp {
        TimeStamp::PhysicalClock(pt) => assert_eq!(pt.ts_ms, 1000),
        _ => panic!("Expected physical clock timestamp"),
    }

    // Test fact creation with logical time domain
    let logical_fact = create_test_fact_with_domain(
        content.clone(),
        authority,
        TimeDomain::LogicalClock,
        &time_source,
    )
    .await
    .unwrap();

    match &logical_fact.timestamp {
        TimeStamp::LogicalClock(_) => {} // Success
        _ => panic!("Expected logical clock timestamp"),
    }

    // Test fact creation with order time domain
    let order_fact =
        create_test_fact_with_domain(content, authority, TimeDomain::OrderClock, &time_source)
            .await
            .unwrap();

    match &order_fact.timestamp {
        TimeStamp::OrderClock(_) => {} // Success
        _ => panic!("Expected order clock timestamp"),
    }
}

#[tokio::test]
async fn test_fact_ordering_across_time_domains() {
    let time_source = ControllableTimeSource::new(1000);
    let authority = AuthorityId::new_from_entropy([1u8; 32]);

    // Create facts with different time domains
    let content1 = TestFactContent {
        data: "fact1".to_string(),
    };
    let fact1 =
        create_test_fact_with_domain(content1, authority, TimeDomain::PhysicalClock, &time_source)
            .await
            .unwrap();

    // Create another physical fact to ensure deterministic ordering
    time_source.advance_time(100);
    let content2 = TestFactContent {
        data: "fact2".to_string(),
    };
    let fact2 =
        create_test_fact_with_domain(content2, authority, TimeDomain::PhysicalClock, &time_source)
            .await
            .unwrap();

    // Test ordering with sort_compare
    let mut facts = [fact2.clone(), fact1.clone()].to_vec(); // Reverse chronological order
    facts.sort_by(|a, b| {
        a.timestamp
            .sort_compare(&b.timestamp, OrderingPolicy::Native)
    });

    // Verify facts are ordered by timestamp (fact1 at 1000ms should come before fact2 at 1100ms)
    assert_eq!(
        facts[0].timestamp.to_index_ms(),
        fact1.timestamp.to_index_ms()
    );
    assert_eq!(
        facts[1].timestamp.to_index_ms(),
        fact2.timestamp.to_index_ms()
    );

    // Verify the actual ordering values
    assert!(fact1.timestamp.to_index_ms() < fact2.timestamp.to_index_ms());
}

#[tokio::test]
async fn test_time_based_capability_constraints() {
    let time_source = ControllableTimeSource::new(1000);

    // Test capability token expiration with unified time system
    let current_time = time_source.physical_time().await.unwrap();
    let expires_in_future = current_time.ts_ms + 5000; // 5 seconds in future
    let expires_in_past = current_time.ts_ms.saturating_sub(1000); // 1 second in past

    // Mock capability verification result
    let valid_token = MockCapabilityResult {
        valid: true,
        expires_at: Some(expires_in_future / 1000), // Convert to seconds
    };

    let expired_token = MockCapabilityResult {
        valid: true,
        expires_at: Some(expires_in_past / 1000), // Convert to seconds
    };

    // Test capability validation at different times
    assert!(valid_token.is_valid_at(current_time.ts_ms));
    assert!(!expired_token.is_valid_at(current_time.ts_ms));

    // Test time advancement affects validation
    time_source.advance_time(10000); // Advance 10 seconds
    let future_time = time_source.physical_time().await.unwrap();
    assert!(!valid_token.is_valid_at(future_time.ts_ms)); // Should now be expired
}

#[tokio::test]
async fn test_time_leakage_properties() {
    let time_source = ControllableTimeSource::new(1000);

    // Test that order clocks leak no timing information
    let order1 = time_source.order_time().await.unwrap();
    time_source.advance_time(5000); // Significant time advancement
    let order2 = time_source.order_time().await.unwrap();

    // Order clocks should provide ordering but no timing correlation
    let ts1 = TimeStamp::OrderClock(order1);
    let ts2 = TimeStamp::OrderClock(order2);

    // Should have deterministic ordering
    let comparison = ts1.compare(&ts2, OrderingPolicy::Native);
    assert!(matches!(
        comparison,
        TimeOrdering::Before | TimeOrdering::After
    ));

    // But the raw bytes should not reveal timing information
    // (In production, these would be cryptographically generated)
    assert_ne!(ts1.to_index_ms(), 1000); // Should not leak original timestamp
    assert_ne!(ts2.to_index_ms(), 6000); // Should not leak advanced timestamp
}

#[tokio::test]
async fn test_time_scenario_builder_integration() {
    let scenario = TimeScenarioBuilder::new()
        .with_initial_time(1000)
        .with_devices(&[DeviceId::new(), DeviceId::new()])
        .with_time_skew(100) // 100ms skew
        .build();

    let time_source = scenario.time_source();

    // Test scenario-based time operations
    let physical_time = time_source.physical_time().await.unwrap();
    assert!(physical_time.ts_ms >= 1000);
    assert!(physical_time.ts_ms <= 1000 + 100); // Within skew tolerance

    let logical_time = time_source.logical_now().await.unwrap();
    assert_eq!(logical_time.lamport, 0); // Should start at 0

    // Test scenario time advancement
    scenario.advance_all_clocks(500);
    let advanced_time = time_source.physical_time().await.unwrap();
    assert!(advanced_time.ts_ms >= 1500);
}

#[tokio::test]
async fn test_timestamp_storage_serialization() {
    use aura_core::time::TimeStamp;
    use serde_json;

    let time_source = ControllableTimeSource::new(1000);

    // Test serialization of different TimeStamp variants
    let physical_time = time_source.physical_time().await.unwrap();
    let physical_timestamp = TimeStamp::PhysicalClock(physical_time);

    let logical_time = time_source.logical_now().await.unwrap();
    let logical_timestamp = TimeStamp::LogicalClock(logical_time);

    let order_time = time_source.order_time().await.unwrap();
    let order_timestamp = TimeStamp::OrderClock(order_time);

    // Test serialization round-trip for all timestamp types
    let physical_bytes = serde_json::to_vec(&physical_timestamp).unwrap();
    let physical_restored: TimeStamp = serde_json::from_slice(&physical_bytes).unwrap();
    assert_eq!(physical_timestamp, physical_restored);

    let logical_bytes = serde_json::to_vec(&logical_timestamp).unwrap();
    let logical_restored: TimeStamp = serde_json::from_slice(&logical_bytes).unwrap();
    assert_eq!(logical_timestamp, logical_restored);

    let order_bytes = serde_json::to_vec(&order_timestamp).unwrap();
    let order_restored: TimeStamp = serde_json::from_slice(&order_bytes).unwrap();
    assert_eq!(order_timestamp, order_restored);

    // Verify different domains serialize to different byte patterns
    assert_ne!(physical_bytes, logical_bytes);
    assert_ne!(logical_bytes, order_bytes);
    assert_ne!(physical_bytes, order_bytes);
}

// Helper functions and mock structures

/// Create a test fact with a specific time domain
async fn create_test_fact_with_domain<T>(
    content: TestFactContent,
    authority: AuthorityId,
    domain: TimeDomain,
    time_effects: &T,
) -> Result<TestFact, TimeError>
where
    T: PhysicalTimeEffects + LogicalClockEffects + OrderClockEffects,
{
    let timestamp = match domain {
        TimeDomain::PhysicalClock => TimeStamp::PhysicalClock(time_effects.physical_time().await?),
        TimeDomain::LogicalClock => TimeStamp::LogicalClock(time_effects.logical_now().await?),
        TimeDomain::OrderClock => TimeStamp::OrderClock(time_effects.order_time().await?),
        TimeDomain::Range => {
            return Err(TimeError::OperationFailed {
                reason: "Range domain not supported for fact creation".to_string(),
            });
        }
    };

    Ok(TestFact {
        content,
        authority,
        timestamp,
    })
}

/// Mock fact structure for testing
#[derive(Debug, Clone)]
struct TestFact {
    #[allow(dead_code)]
    content: TestFactContent,
    #[allow(dead_code)]
    authority: AuthorityId,
    timestamp: TimeStamp,
}

/// Mock capability verification result
struct MockCapabilityResult {
    valid: bool,
    expires_at: Option<u64>,
}

impl MockCapabilityResult {
    /// Check if valid at the given time (in milliseconds)
    fn is_valid_at(&self, current_time_ms: u64) -> bool {
        if !self.valid {
            return false;
        }

        if let Some(expires_at) = self.expires_at {
            let expires_at_ms = expires_at * 1000; // Convert to milliseconds
            return current_time_ms < expires_at_ms;
        }

        true
    }
}
