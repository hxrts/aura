//! Delta Application Tests
//!
//! These tests verify the ViewReduction → Delta → View pattern:
//! - ViewReduction correctly transforms facts into deltas
//! - ViewAdapter bridges ReactiveView and delta patterns
//! - Deltas are applied correctly to view state
//!
//! ## Architecture
//!
//! Facts → ViewReduction::reduce() → Vec<Delta> → View::apply_delta()
//!
//! This pattern ensures:
//! - Pure reduction functions (deterministic, monotone)
//! - Idempotent delta application
//! - Composable view updates

use aura_agent::reactive::ViewReduction;
use aura_core::{
    identifiers::{AuthorityId, ContextId},
    time::{OrderTime, PhysicalTime, TimeStamp},
};
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

// =============================================================================
// Test Delta Types
// =============================================================================

/// Simple delta for testing
#[derive(Debug, Clone, PartialEq)]
enum TestDelta {
    Increment,
    Decrement,
    SetValue(i32),
    AppendMessage(String),
}

/// Simple view state for testing
struct TestView {
    counter: AtomicUsize,
    messages: RwLock<Vec<String>>,
    delta_count: AtomicUsize,
}

impl TestView {
    fn new() -> Self {
        Self {
            counter: AtomicUsize::new(0),
            messages: RwLock::new(Vec::new()),
            delta_count: AtomicUsize::new(0),
        }
    }

    async fn apply_delta(&self, delta: TestDelta) {
        self.delta_count.fetch_add(1, Ordering::SeqCst);
        match delta {
            TestDelta::Increment => {
                self.counter.fetch_add(1, Ordering::SeqCst);
            }
            TestDelta::Decrement => {
                self.counter.fetch_sub(1, Ordering::SeqCst);
            }
            TestDelta::SetValue(val) => {
                self.counter.store(val as usize, Ordering::SeqCst);
            }
            TestDelta::AppendMessage(msg) => {
                self.messages.write().await.push(msg);
            }
        }
    }

    fn get_counter(&self) -> usize {
        self.counter.load(Ordering::SeqCst)
    }

    async fn get_messages(&self) -> Vec<String> {
        self.messages.read().await.clone()
    }

    fn get_delta_count(&self) -> usize {
        self.delta_count.load(Ordering::SeqCst)
    }
}

// =============================================================================
// Test Reductions
// =============================================================================

/// Counts guardian bindings as increments
struct CountingReduction;

impl ViewReduction<TestDelta> for CountingReduction {
    fn reduce(&self, facts: &[Fact], _own_authority: Option<AuthorityId>) -> Vec<TestDelta> {
        facts
            .iter()
            .filter_map(|fact| {
                if let FactContent::Relational(RelationalFact::GuardianBinding { .. }) =
                    &fact.content
                {
                    Some(TestDelta::Increment)
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Extracts message content from generic facts
struct MessageReduction;

impl ViewReduction<TestDelta> for MessageReduction {
    fn reduce(&self, facts: &[Fact], _own_authority: Option<AuthorityId>) -> Vec<TestDelta> {
        facts
            .iter()
            .filter_map(|fact| {
                if let FactContent::Relational(RelationalFact::Generic { binding_type, .. }) =
                    &fact.content
                {
                    binding_type
                        .strip_prefix("message:")
                        .map(|message| TestDelta::AppendMessage(message.to_string()))
                } else {
                    None
                }
            })
            .collect()
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

fn make_guardian_fact(index: u64) -> Fact {
    use aura_core::identifiers::AuthorityId;
    use aura_core::Hash32;

    Fact {
        order: make_order_time(index),
        timestamp: make_timestamp(1000 + index),
        content: FactContent::Relational(RelationalFact::GuardianBinding {
            account_id: AuthorityId::new_from_entropy([1u8; 32]),
            guardian_id: AuthorityId::new_from_entropy([index as u8; 32]),
            binding_hash: Hash32([0u8; 32]),
        }),
    }
}

fn make_message_fact(message: &str, index: u64) -> Fact {
    Fact {
        order: make_order_time(index),
        timestamp: make_timestamp(1000 + index),
        content: FactContent::Relational(RelationalFact::Generic {
            context_id: ContextId::new_from_entropy([0u8; 32]),
            binding_type: format!("message:{}", message),
            binding_data: vec![],
        }),
    }
}

fn make_generic_fact(binding_type: &str, index: u64) -> Fact {
    Fact {
        order: make_order_time(index),
        timestamp: make_timestamp(1000 + index),
        content: FactContent::Relational(RelationalFact::Generic {
            context_id: ContextId::new_from_entropy([0u8; 32]),
            binding_type: binding_type.to_string(),
            binding_data: vec![],
        }),
    }
}

// =============================================================================
// ViewReduction Tests
// =============================================================================

#[test]
fn test_counting_reduction_basic() {
    let reduction = CountingReduction;

    let facts = vec![
        make_guardian_fact(1),
        make_guardian_fact(2),
        make_guardian_fact(3),
    ];

    let deltas = reduction.reduce(&facts, None);

    assert_eq!(deltas.len(), 3);
    assert!(deltas.iter().all(|d| *d == TestDelta::Increment));
}

#[test]
fn test_counting_reduction_filters_non_guardian() {
    let reduction = CountingReduction;

    let facts = vec![
        make_guardian_fact(1),
        make_generic_fact("other_type", 2),
        make_guardian_fact(3),
    ];

    let deltas = reduction.reduce(&facts, None);

    assert_eq!(deltas.len(), 2);
}

#[test]
fn test_counting_reduction_empty_facts() {
    let reduction = CountingReduction;
    let deltas = reduction.reduce(&[], None);
    assert!(deltas.is_empty());
}

#[test]
fn test_message_reduction_basic() {
    let reduction = MessageReduction;

    let facts = vec![make_message_fact("hello", 1), make_message_fact("world", 2)];

    let deltas = reduction.reduce(&facts, None);

    assert_eq!(deltas.len(), 2);
    assert_eq!(deltas[0], TestDelta::AppendMessage("hello".to_string()));
    assert_eq!(deltas[1], TestDelta::AppendMessage("world".to_string()));
}

#[test]
fn test_message_reduction_filters_non_messages() {
    let reduction = MessageReduction;

    let facts = vec![
        make_message_fact("hello", 1),
        make_generic_fact("not_a_message", 2),
        make_message_fact("world", 3),
    ];

    let deltas = reduction.reduce(&facts, None);

    assert_eq!(deltas.len(), 2);
}

#[test]
fn test_reduction_determinism() {
    let reduction = CountingReduction;

    let facts = vec![make_guardian_fact(1), make_guardian_fact(2)];

    // Same input should always produce same output
    let deltas1 = reduction.reduce(&facts, None);
    let deltas2 = reduction.reduce(&facts, None);

    assert_eq!(deltas1, deltas2);
}

#[test]
fn test_reduction_monotonicity() {
    let reduction = CountingReduction;

    let facts_small = vec![make_guardian_fact(1)];
    let facts_large = vec![make_guardian_fact(1), make_guardian_fact(2)];

    let deltas_small = reduction.reduce(&facts_small, None);
    let deltas_large = reduction.reduce(&facts_large, None);

    // F₁ ⊆ F₂ ⇒ reduce(F₁) ⊆ reduce(F₂)
    assert!(deltas_small.len() <= deltas_large.len());
}

// =============================================================================
// Delta Application Tests
// =============================================================================

#[tokio::test]
async fn test_delta_application_increment() {
    let view = TestView::new();

    view.apply_delta(TestDelta::Increment).await;
    assert_eq!(view.get_counter(), 1);

    view.apply_delta(TestDelta::Increment).await;
    assert_eq!(view.get_counter(), 2);
}

#[tokio::test]
async fn test_delta_application_decrement() {
    let view = TestView::new();

    view.apply_delta(TestDelta::SetValue(10)).await;
    view.apply_delta(TestDelta::Decrement).await;
    assert_eq!(view.get_counter(), 9);
}

#[tokio::test]
async fn test_delta_application_messages() {
    let view = TestView::new();

    view.apply_delta(TestDelta::AppendMessage("first".to_string()))
        .await;
    view.apply_delta(TestDelta::AppendMessage("second".to_string()))
        .await;

    let messages = view.get_messages().await;
    assert_eq!(messages, vec!["first", "second"]);
}

#[tokio::test]
async fn test_delta_application_tracks_count() {
    let view = TestView::new();

    view.apply_delta(TestDelta::Increment).await;
    view.apply_delta(TestDelta::Increment).await;
    view.apply_delta(TestDelta::Decrement).await;

    assert_eq!(view.get_delta_count(), 3);
}

#[tokio::test]
async fn test_idempotent_delta_application() {
    let view = TestView::new();

    // SetValue should be idempotent - applying same value multiple times
    // leaves state the same (value-wise, not application count)
    view.apply_delta(TestDelta::SetValue(42)).await;
    let val1 = view.get_counter();

    view.apply_delta(TestDelta::SetValue(42)).await;
    let val2 = view.get_counter();

    assert_eq!(val1, val2);
    assert_eq!(val1, 42);
}

// =============================================================================
// Integration Tests
// =============================================================================

#[tokio::test]
async fn test_full_pipeline_facts_to_view() {
    // Simulate the full pipeline: Facts → Reduction → Deltas → View
    let view = Arc::new(TestView::new());
    let reduction = CountingReduction;

    // Step 1: Receive facts
    let facts = vec![
        make_guardian_fact(1),
        make_guardian_fact(2),
        make_guardian_fact(3),
        make_generic_fact("other", 4), // Should be filtered
    ];

    // Step 2: Reduce to deltas
    let deltas = reduction.reduce(&facts, None);
    assert_eq!(deltas.len(), 3);

    // Step 3: Apply deltas to view
    for delta in deltas {
        view.apply_delta(delta).await;
    }

    // Step 4: Verify final state
    assert_eq!(view.get_counter(), 3);
}

#[tokio::test]
async fn test_multiple_reductions_same_facts() {
    // Different reductions can process same facts differently
    let facts = vec![
        make_guardian_fact(1),
        make_message_fact("hello", 2),
        make_guardian_fact(3),
        make_message_fact("world", 4),
    ];

    let count_reduction = CountingReduction;
    let message_reduction = MessageReduction;

    let count_deltas = count_reduction.reduce(&facts, None);
    let message_deltas = message_reduction.reduce(&facts, None);

    assert_eq!(count_deltas.len(), 2); // 2 guardian facts
    assert_eq!(message_deltas.len(), 2); // 2 message facts
}

#[tokio::test]
async fn test_concurrent_delta_application() {
    let view = Arc::new(TestView::new());

    // Apply deltas concurrently
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let v = view.clone();
            tokio::spawn(async move {
                v.apply_delta(TestDelta::Increment).await;
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap();
    }

    // All increments should have been applied
    assert_eq!(view.get_counter(), 10);
    assert_eq!(view.get_delta_count(), 10);
}

#[tokio::test]
async fn test_ordered_fact_processing() {
    let view = Arc::new(TestView::new());
    let reduction = MessageReduction;

    // Facts should be processed in order
    let facts = vec![
        make_message_fact("first", 1),
        make_message_fact("second", 2),
        make_message_fact("third", 3),
    ];

    let deltas = reduction.reduce(&facts, None);
    for delta in deltas {
        view.apply_delta(delta).await;
    }

    let messages = view.get_messages().await;
    assert_eq!(messages, vec!["first", "second", "third"]);
}
