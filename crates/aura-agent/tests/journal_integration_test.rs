//! Journal Integration Tests
//!
//! These tests verify the integration between the journal fact system
//! and the reactive infrastructure:
//!
//! - Facts flow from journal source to scheduler to views
//! - Fact ordering is preserved through the pipeline
//! - Multiple fact sources are handled correctly
//! - Views see consistent fact snapshots
//!
//! ## Architecture
//!
//! Journal → FactSource → ReactiveScheduler → ReactiveView.update() → View State

use aura_agent::fact_registry::build_fact_registry;
use aura_agent::reactive::{FactSource, ReactiveScheduler, ReactiveView, SchedulerConfig};
use aura_core::{
    identifiers::{AuthorityId, ContextId},
    time::{OrderTime, PhysicalTime, TimeStamp},
    Hash32,
};
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};

// =============================================================================
// Test View Implementation
// =============================================================================

/// Test view that records facts it receives
struct FactRecordingView {
    id: String,
    received_facts: Arc<RwLock<Vec<Fact>>>,
    update_count: AtomicUsize,
}

impl FactRecordingView {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            received_facts: Arc::new(RwLock::new(Vec::new())),
            update_count: AtomicUsize::new(0),
        }
    }

    async fn get_facts(&self) -> Vec<Fact> {
        self.received_facts.read().await.clone()
    }

    fn get_update_count(&self) -> usize {
        self.update_count.load(Ordering::SeqCst)
    }
}

impl ReactiveView for FactRecordingView {
    async fn update(&self, facts: &[Fact]) {
        self.update_count.fetch_add(1, Ordering::SeqCst);
        let mut stored = self.received_facts.write().await;
        stored.extend_from_slice(facts);
    }

    fn view_id(&self) -> &str {
        &self.id
    }

    fn dependencies(&self) -> Vec<String> {
        Vec::new()
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

fn make_order_time(index: u64) -> OrderTime {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&index.to_be_bytes());
    OrderTime(bytes)
}

fn make_timestamp(ms: u64) -> TimeStamp {
    TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: ms,
        uncertainty: None,
    })
}

fn make_guardian_fact(account_seed: u8, guardian_seed: u8, index: u64) -> Fact {
    Fact::new(
        make_order_time(index),
        make_timestamp(1000 + index),
        FactContent::Relational(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::GuardianBinding {
                account_id: AuthorityId::new_from_entropy([account_seed; 32]),
                guardian_id: AuthorityId::new_from_entropy([guardian_seed; 32]),
                binding_hash: Hash32([0u8; 32]),
            },
        )),
    )
}

fn make_generic_fact(binding_type: &str, index: u64) -> Fact {
    Fact::new(
        make_order_time(index),
        make_timestamp(1000 + index),
        FactContent::Relational(RelationalFact::Generic {
            context_id: ContextId::new_from_entropy([0u8; 32]),
            binding_type: binding_type.to_string(),
            binding_data: vec![index as u8],
        }),
    )
}

fn scheduler_with_registry(
    config: SchedulerConfig,
) -> (
    ReactiveScheduler,
    mpsc::Sender<FactSource>,
    mpsc::Sender<()>,
) {
    use aura_effects::time::PhysicalTimeHandler;
    use std::sync::Arc;
    let time_effects = Arc::new(PhysicalTimeHandler);
    ReactiveScheduler::new(config, Arc::new(build_fact_registry()), time_effects)
}

// =============================================================================
// Basic Integration Tests
// =============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_scheduler_receives_journal_facts() {
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view = Arc::new(FactRecordingView::new("test_view"));
    scheduler.register_view(view.clone());

    // Spawn scheduler
    tokio::spawn(scheduler.run());

    // Send facts from journal source
    let facts = vec![make_guardian_fact(1, 2, 1), make_guardian_fact(1, 3, 2)];

    fact_tx.send(FactSource::Journal(facts)).await.unwrap();

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify facts were received
    let received = view.get_facts().await;
    assert_eq!(received.len(), 2, "View should receive 2 facts");

    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_scheduler_receives_network_facts() {
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view = Arc::new(FactRecordingView::new("test_view"));
    scheduler.register_view(view.clone());

    tokio::spawn(scheduler.run());

    // Send facts from network source
    let facts = vec![make_guardian_fact(1, 2, 1)];
    fact_tx.send(FactSource::Network(facts)).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let received = view.get_facts().await;
    assert_eq!(received.len(), 1, "View should receive 1 network fact");

    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_fact_ordering_preserved() {
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view = Arc::new(FactRecordingView::new("test_view"));
    scheduler.register_view(view.clone());

    tokio::spawn(scheduler.run());

    // Send ordered facts
    let facts = vec![
        make_generic_fact("first", 1),
        make_generic_fact("second", 2),
        make_generic_fact("third", 3),
    ];

    fact_tx.send(FactSource::Journal(facts)).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let received = view.get_facts().await;
    assert_eq!(received.len(), 3);

    // Verify ordering by order time
    for i in 0..received.len() - 1 {
        assert!(
            received[i].order < received[i + 1].order,
            "Facts should be in order"
        );
    }

    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_multiple_fact_batches() {
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view = Arc::new(FactRecordingView::new("test_view"));
    scheduler.register_view(view.clone());

    tokio::spawn(scheduler.run());

    // Send multiple batches
    for i in 0..5 {
        let facts = vec![make_guardian_fact(1, i as u8 + 2, i + 1)];
        fact_tx.send(FactSource::Journal(facts)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    let received = view.get_facts().await;
    assert_eq!(received.len(), 5, "Should receive all 5 facts");
    assert!(
        view.get_update_count() >= 1,
        "Should have at least 1 update"
    );

    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_empty_fact_batch() {
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view = Arc::new(FactRecordingView::new("test_view"));
    scheduler.register_view(view.clone());

    tokio::spawn(scheduler.run());

    // Send empty batch
    fact_tx.send(FactSource::Journal(vec![])).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let received = view.get_facts().await;
    assert!(received.is_empty(), "No facts should be received");

    shutdown_tx.send(()).await.unwrap();
}

// =============================================================================
// Multi-View Tests
// =============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_multiple_views_receive_same_facts() {
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view1 = Arc::new(FactRecordingView::new("view1"));
    let view2 = Arc::new(FactRecordingView::new("view2"));
    let view3 = Arc::new(FactRecordingView::new("view3"));

    scheduler.register_view(view1.clone());
    scheduler.register_view(view2.clone());
    scheduler.register_view(view3.clone());

    tokio::spawn(scheduler.run());

    let facts = vec![make_guardian_fact(1, 2, 1), make_guardian_fact(1, 3, 2)];
    fact_tx.send(FactSource::Journal(facts)).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    // All views should receive the same facts
    let received1 = view1.get_facts().await;
    let received2 = view2.get_facts().await;
    let received3 = view3.get_facts().await;

    assert_eq!(received1.len(), 2);
    assert_eq!(received2.len(), 2);
    assert_eq!(received3.len(), 2);

    shutdown_tx.send(()).await.unwrap();
}

// =============================================================================
// Mixed Source Tests
// =============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_mixed_fact_sources() {
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view = Arc::new(FactRecordingView::new("test_view"));
    scheduler.register_view(view.clone());

    tokio::spawn(scheduler.run());

    // Send from journal
    fact_tx
        .send(FactSource::Journal(vec![make_guardian_fact(1, 2, 1)]))
        .await
        .unwrap();

    // Send from network
    fact_tx
        .send(FactSource::Network(vec![make_guardian_fact(1, 3, 2)]))
        .await
        .unwrap();

    // Send from timer
    fact_tx
        .send(FactSource::Timer(vec![make_generic_fact("timer_event", 3)]))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    let received = view.get_facts().await;
    assert_eq!(received.len(), 3, "Should receive facts from all sources");

    shutdown_tx.send(()).await.unwrap();
}

// =============================================================================
// Fact Content Tests
// =============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_guardian_binding_facts_preserved() {
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view = Arc::new(FactRecordingView::new("test_view"));
    scheduler.register_view(view.clone());

    tokio::spawn(scheduler.run());

    let original = make_guardian_fact(42, 99, 1);
    fact_tx
        .send(FactSource::Journal(vec![original.clone()]))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let received = view.get_facts().await;
    assert_eq!(received.len(), 1);

    // Verify fact content is preserved
    if let FactContent::Relational(RelationalFact::Protocol(
        aura_journal::ProtocolRelationalFact::GuardianBinding {
            account_id,
            guardian_id,
            ..
        },
    )) = &received[0].content
    {
        assert_eq!(*account_id, AuthorityId::new_from_entropy([42u8; 32]));
        assert_eq!(*guardian_id, AuthorityId::new_from_entropy([99u8; 32]));
    } else {
        panic!("Expected GuardianBinding fact");
    }

    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_generic_facts_preserved() {
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view = Arc::new(FactRecordingView::new("test_view"));
    scheduler.register_view(view.clone());

    tokio::spawn(scheduler.run());

    let original = make_generic_fact("test_binding_type", 1);
    fact_tx
        .send(FactSource::Journal(vec![original.clone()]))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let received = view.get_facts().await;
    assert_eq!(received.len(), 1);

    // Verify fact content is preserved
    if let FactContent::Relational(RelationalFact::Generic { binding_type, .. }) =
        &received[0].content
    {
        assert_eq!(binding_type, "test_binding_type");
    } else {
        panic!("Expected Generic fact");
    }

    shutdown_tx.send(()).await.unwrap();
}

// =============================================================================
// Stress Tests
// =============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_high_volume_facts() {
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view = Arc::new(FactRecordingView::new("test_view"));
    scheduler.register_view(view.clone());

    tokio::spawn(scheduler.run());

    // Send 100 facts
    let facts: Vec<Fact> = (0..100)
        .map(|i| make_generic_fact("bulk_fact", i))
        .collect();
    fact_tx.send(FactSource::Journal(facts)).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let received = view.get_facts().await;
    assert_eq!(received.len(), 100, "All 100 facts should be received");

    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_rapid_fact_updates() {
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view = Arc::new(FactRecordingView::new("test_view"));
    scheduler.register_view(view.clone());

    tokio::spawn(scheduler.run());

    // Rapidly send individual facts
    for i in 0..50 {
        let facts = vec![make_generic_fact("rapid_fact", i)];
        fact_tx.send(FactSource::Journal(facts)).await.unwrap();
    }

    tokio::time::sleep(Duration::from_millis(300)).await;

    let received = view.get_facts().await;
    assert_eq!(received.len(), 50, "All 50 rapid facts should be received");

    shutdown_tx.send(()).await.unwrap();
}

// =============================================================================
// Shutdown Tests
// =============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_graceful_shutdown() {
    let config = SchedulerConfig::default();
    let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

    let view = Arc::new(FactRecordingView::new("test_view"));
    scheduler.register_view(view.clone());

    let handle = tokio::spawn(scheduler.run());

    // Send some facts
    fact_tx
        .send(FactSource::Journal(vec![make_guardian_fact(1, 2, 1)]))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Graceful shutdown
    shutdown_tx.send(()).await.unwrap();

    // Verify scheduler exits cleanly
    let result = tokio::time::timeout(Duration::from_secs(1), handle).await;
    assert!(result.is_ok(), "Scheduler should exit gracefully");
}
