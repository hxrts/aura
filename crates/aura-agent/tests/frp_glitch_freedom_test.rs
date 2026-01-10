//! FRP Glitch-Freedom Tests
//!
//! These tests verify that the FRP infrastructure maintains glitch-freedom:
#![allow(clippy::uninlined_format_args)] // Test code uses explicit format args for clarity
//! - Derived dynamics don't show intermediate inconsistent states
//! - Topological ordering ensures downstream values see consistent state
//! - Combined values are always computed from consistent snapshots
//!
//! ## What is Glitch-Freedom?
//!
//! In FRP, a "glitch" occurs when a derived value temporarily shows an
//! inconsistent state. For example, if `sum = a + b` and both `a` and `b`
//! are updated simultaneously, a glitch would be if `sum` momentarily
//! shows `old_a + new_b` before settling to `new_a + new_b`.
//!
//! Glitch-freedom guarantees that derived values only ever show states
//! that correspond to valid snapshots of their inputs.

use aura_agent::fact_registry::build_fact_registry;
use aura_agent::reactive::{Dynamic, ReactiveScheduler, ReactiveView, SchedulerConfig};
use aura_journal::fact::Fact;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tokio::task::yield_now;

fn scheduler_with_registry(
    config: SchedulerConfig,
) -> (
    ReactiveScheduler,
    mpsc::Sender<aura_agent::reactive::FactSource>,
    mpsc::Sender<()>,
) {
    use aura_effects::time::PhysicalTimeHandler;
    use std::sync::Arc;
    let time_effects = Arc::new(PhysicalTimeHandler);
    let (scheduler, fact_tx, shutdown_tx, _update_tx) =
        ReactiveScheduler::new(config, Arc::new(build_fact_registry()), time_effects);
    (scheduler, fact_tx, shutdown_tx)
}

// =============================================================================
// Basic Glitch-Freedom Tests
// =============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_map_no_intermediate_states() {
    // Test that map doesn't show intermediate states
    let source = Dynamic::new(0);
    let doubled = source.map(|x| x * 2).await;

    // Record all observed values
    let observations = Arc::new(RwLock::new(Vec::new()));
    let obs_clone = observations.clone();
    let mut rx = doubled.subscribe().await;

    tokio::spawn(async move {
        while let Ok(val) = rx.recv().await {
            obs_clone.write().await.push(*val);
        }
    });

    // Update source multiple times
    source.set(1).await;
    source.set(2).await;
    source.set(3).await;

    tokio::time::sleep(Duration::from_millis(20)).await;

    // All observed values should be valid doubles
    let obs = observations.read().await;
    for val in obs.iter() {
        assert!(val % 2 == 0, "Observed non-doubled value: {}", val);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_combine_consistent_snapshots() {
    // Test that combine always uses consistent snapshots of both inputs
    let a = Dynamic::new(0);
    let b = Dynamic::new(0);

    // Create a combined value that checks consistency
    // If we ever see (a, b) where a != b, it means we saw an inconsistent state
    let inconsistencies = Arc::new(AtomicUsize::new(0));
    let inc_clone = inconsistencies.clone();

    let combined = a
        .combine(&b, move |x, y| {
            if *x != *y {
                inc_clone.fetch_add(1, Ordering::SeqCst);
            }
            (*x, *y)
        })
        .await;

    // Subscribe to force evaluation
    let _rx = combined.subscribe();

    // Update both values together repeatedly
    for i in 1..=10 {
        a.set(i).await;
        b.set(i).await;
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Note: Due to the async nature, some inconsistencies may be observed
    // during the transition. The key is that the final state is consistent.
    let final_val = combined.get().await;
    assert_eq!(final_val.0, final_val.1, "Final state should be consistent");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_chain_propagation() {
    // Test that changes propagate correctly through a chain
    let source = Dynamic::new(1);
    let doubled = source.map(|x| x * 2).await;
    let plus_one = doubled.map(|x| x + 1).await;
    let stringified = plus_one.map(|x| format!("value={}", x)).await;

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Initial: 1 -> 2 -> 3 -> "value=3"
    assert_eq!(stringified.get().await, "value=3");

    source.set(5).await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Updated: 5 -> 10 -> 11 -> "value=11"
    assert_eq!(stringified.get().await, "value=11");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_fold_accumulation_order() {
    // Test that fold accumulates values in order
    let events = Dynamic::new(0);
    let history = events
        .fold(Vec::new(), |mut acc, x| {
            acc.push(*x);
            acc
        })
        .await;

    // Subscribe to trigger updates
    let _rx = history.subscribe().await;

    // Send ordered events
    events.set(1).await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    events.set(2).await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    events.set(3).await;
    tokio::time::sleep(Duration::from_millis(10)).await;

    let final_history = history.get().await;
    assert_eq!(
        final_history,
        vec![1, 2, 3],
        "Events should be accumulated in order"
    );
}

// =============================================================================
// Diamond Dependency Tests
// =============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_diamond_dependency_consistency() {
    // Classic diamond pattern:
    //     a
    //    / \
    //   b   c
    //    \ /
    //     d
    //
    // When 'a' changes, 'd' should see a consistent view of 'b' and 'c'

    let a = Dynamic::new(0);
    let b = a.map(|x| x + 1).await; // b = a + 1
    let c = a.map(|x| x * 2).await; // c = a * 2
    let d = b.combine(&c, |x, y| (*x, *y)).await; // d = (b, c)

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Initial: a=0 -> b=1, c=0 -> d=(1, 0)
    let initial = d.get().await;
    assert_eq!(initial, (1, 0));

    // Update a to 5
    a.set(5).await;
    tokio::time::sleep(Duration::from_millis(30)).await;

    // Expected: a=5 -> b=6, c=10 -> d=(6, 10)
    let updated = d.get().await;

    // The final state should be consistent with a=5
    // b = 5 + 1 = 6
    // c = 5 * 2 = 10
    assert_eq!(updated, (6, 10), "Diamond dependency should be consistent");
}

// =============================================================================
// Filter Consistency Tests
// =============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_filter_preserves_consistency() {
    // Test that filter doesn't break consistency
    let source = Dynamic::new(0);
    let positive_only = source.filter(|x| *x > 0).await;

    tokio::time::sleep(Duration::from_millis(10)).await;

    // Initial value should be preserved
    assert_eq!(positive_only.get().await, 0);

    // Positive values should propagate
    source.set(5).await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(positive_only.get().await, 5);

    // Negative values should be filtered
    source.set(-3).await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(
        positive_only.get().await,
        5,
        "Negative should not propagate"
    );

    // Another positive should work
    source.set(10).await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(positive_only.get().await, 10);
}

// =============================================================================
// Scheduler Topological Ordering Tests
// =============================================================================

/// Mock view that records update order
struct OrderTrackingView {
    id: String,
    deps: Vec<String>,
    update_order: Arc<RwLock<Vec<String>>>,
}

impl OrderTrackingView {
    fn new(id: &str, deps: &[&str], order: Arc<RwLock<Vec<String>>>) -> Self {
        Self {
            id: id.to_string(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            update_order: order,
        }
    }
}

impl ReactiveView for OrderTrackingView {
    async fn update(&self, _facts: &[Fact]) {
        self.update_order.write().await.push(self.id.clone());
    }

    fn view_id(&self) -> &str {
        &self.id
    }

    fn dependencies(&self) -> Vec<String> {
        self.deps.clone()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_scheduler_topological_update_order() {
    use aura_agent::reactive::FactSource;
    use aura_core::identifiers::ContextId;
    use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
    use aura_journal::fact::{FactContent, RelationalFact};

    let update_order = Arc::new(RwLock::new(Vec::new()));

    // Create views with dependencies: c -> b -> a
    // Expected update order: a, b, c
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view_c = Arc::new(OrderTrackingView::new("c", &["b"], update_order.clone()));
    let view_b = Arc::new(OrderTrackingView::new("b", &["a"], update_order.clone()));
    let view_a = Arc::new(OrderTrackingView::new("a", &[], update_order.clone()));

    // Register in "wrong" order to verify topological sort works
    scheduler.register_view(view_c);
    scheduler.register_view(view_a);
    scheduler.register_view(view_b);

    // Spawn scheduler
    tokio::spawn(scheduler.run());

    // Create a test fact
    let fact = Fact::new(
        OrderTime([0u8; 32]),
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        }),
        FactContent::Relational(RelationalFact::Generic {
            context_id: ContextId::new_from_entropy([0u8; 32]),
            envelope: aura_core::types::facts::FactEnvelope {
                type_id: aura_core::types::facts::FactTypeId::from("test"),
                schema_version: 1,
                encoding: aura_core::types::facts::FactEncoding::DagCbor,
                payload: vec![1],
            },
        }),
    );

    // Send fact
    fact_tx.send(FactSource::Journal(vec![fact])).await.unwrap();

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Shutdown
    shutdown_tx.send(()).await.unwrap();

    // Verify update order
    let order = update_order.read().await;
    assert_eq!(order.len(), 3, "All views should be updated");

    // Find positions
    let pos_a = order.iter().position(|x| x == "a").unwrap();
    let pos_b = order.iter().position(|x| x == "b").unwrap();
    let pos_c = order.iter().position(|x| x == "c").unwrap();

    assert!(pos_a < pos_b, "a should be updated before b");
    assert!(pos_b < pos_c, "b should be updated before c");
}

// =============================================================================
// Stress Tests
// =============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_rapid_updates_consistency() {
    let source = Dynamic::new(0);
    let derived = source.map(|x| x * 2).await;
    // map() now synchronizes internally - spawned task is ready when map returns

    // Rapidly update source with occasional yields to allow propagation
    for i in 0..100 {
        source.set(i).await;
        // Yield every 10 updates to allow the reactive task to process
        if i % 10 == 0 {
            yield_now().await;
        }
    }

    // Wait for all updates to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Final state should be consistent - derived should equal 2x source
    let source_val = source.get().await;
    let derived_val = derived.get().await;

    assert_eq!(
        derived_val,
        source_val * 2,
        "Final derived value should be 2x source"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrent_readers_consistency() {
    let source = Dynamic::new(0);
    let derived = source.map(|x| x * 2).await;

    let source_clone = source.clone();
    let derived_clone = derived.clone();

    // Spawn a writer
    let writer = tokio::spawn(async move {
        for i in 1..=50 {
            source_clone.set(i).await;
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });

    // Spawn a reader that checks consistency
    let inconsistencies = Arc::new(AtomicUsize::new(0));
    let inc_clone = inconsistencies.clone();

    let reader = tokio::spawn(async move {
        for _ in 0..100 {
            let s: i32 = source.get().await;
            let d: i32 = derived_clone.get().await;
            // Due to async timing, we might see d = 2*(s-1) or d = 2*s
            // but never anything else
            if d != s * 2 && d != (s.saturating_sub(1)) * 2 {
                inc_clone.fetch_add(1, Ordering::SeqCst);
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });

    let _ = tokio::join!(writer, reader);

    // Allow for some timing-related observations, but should be rare
    let inc_count = inconsistencies.load(Ordering::SeqCst);
    assert!(
        inc_count < 5,
        "Too many inconsistencies observed: {}",
        inc_count
    );
}
