//! Journal effects tests for AMP.
//!
//! Tests the AmpJournalEffects trait adapter, context journal operations,
//! and relational fact insertion.

use aura_amp::AmpJournalEffects;
use aura_core::effects::time::OrderClockEffects;
use aura_core::effects::JournalEffects;
use aura_core::identifiers::{ChannelId, ContextId};
use aura_journal::fact::{ChannelCheckpoint, ChannelPolicy, RelationalFact};
use aura_testkit::mock_effects::MockEffects;

fn test_context() -> ContextId {
    ContextId::from_uuid(uuid::Uuid::from_bytes([1u8; 16]))
}

fn test_context_2() -> ContextId {
    ContextId::from_uuid(uuid::Uuid::from_bytes([2u8; 16]))
}

fn test_channel() -> ChannelId {
    ChannelId::from_bytes([3u8; 32])
}

fn test_channel_2() -> ChannelId {
    ChannelId::from_bytes([4u8; 32])
}

// =============================================================================
// AmpJournalEffects Trait Tests
// =============================================================================

#[tokio::test]
async fn test_fetch_context_journal_empty() {
    let effects = MockEffects::deterministic();

    // Fetch journal for a context with no facts
    let journal = effects.fetch_context_journal(test_context()).await;
    assert!(journal.is_ok());

    let j = journal.unwrap();
    assert!(j.facts.is_empty(), "empty context should have no facts");
}

#[tokio::test]
async fn test_insert_relational_fact_checkpoint() {
    let effects = MockEffects::deterministic();

    let checkpoint = ChannelCheckpoint {
        context: test_context(),
        channel: test_channel(),
        chan_epoch: 0,
        base_gen: 0,
        window: 1024,
        ck_commitment: Default::default(),
        skip_window_override: Some(1024),
    };

    let result = effects
        .insert_relational_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint),
        ))
        .await;

    assert!(result.is_ok(), "inserting checkpoint fact should succeed");
}

#[tokio::test]
async fn test_insert_relational_fact_policy() {
    let effects = MockEffects::deterministic();

    let policy = ChannelPolicy {
        context: test_context(),
        channel: test_channel(),
        skip_window: Some(2048),
    };

    let result = effects
        .insert_relational_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelPolicy(policy),
        ))
        .await;

    assert!(result.is_ok(), "inserting policy fact should succeed");
}

#[tokio::test]
async fn test_insert_multiple_facts() {
    let effects = MockEffects::deterministic();

    // Insert checkpoint
    let checkpoint = ChannelCheckpoint {
        context: test_context(),
        channel: test_channel(),
        chan_epoch: 0,
        base_gen: 0,
        window: 1024,
        ck_commitment: Default::default(),
        skip_window_override: None,
    };
    effects
        .insert_relational_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint),
        ))
        .await
        .unwrap();

    // Insert policy
    let policy = ChannelPolicy {
        context: test_context(),
        channel: test_channel(),
        skip_window: Some(512),
    };
    effects
        .insert_relational_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelPolicy(policy),
        ))
        .await
        .unwrap();

    // Both insertions should succeed without error
}

#[tokio::test]
async fn test_insert_facts_for_different_contexts() {
    let effects = MockEffects::deterministic();

    // Insert fact for context 1
    let checkpoint1 = ChannelCheckpoint {
        context: test_context(),
        channel: test_channel(),
        chan_epoch: 0,
        base_gen: 0,
        window: 1024,
        ck_commitment: Default::default(),
        skip_window_override: None,
    };
    effects
        .insert_relational_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint1),
        ))
        .await
        .unwrap();

    // Insert fact for context 2
    let checkpoint2 = ChannelCheckpoint {
        context: test_context_2(),
        channel: test_channel(),
        chan_epoch: 0,
        base_gen: 0,
        window: 2048,
        ck_commitment: Default::default(),
        skip_window_override: None,
    };
    effects
        .insert_relational_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint2),
        ))
        .await
        .unwrap();

    // Both should succeed
}

#[tokio::test]
async fn test_insert_facts_for_different_channels() {
    let effects = MockEffects::deterministic();

    // Insert fact for channel 1
    let checkpoint1 = ChannelCheckpoint {
        context: test_context(),
        channel: test_channel(),
        chan_epoch: 0,
        base_gen: 0,
        window: 1024,
        ck_commitment: Default::default(),
        skip_window_override: None,
    };
    effects
        .insert_relational_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint1),
        ))
        .await
        .unwrap();

    // Insert fact for channel 2
    let checkpoint2 = ChannelCheckpoint {
        context: test_context(),
        channel: test_channel_2(),
        chan_epoch: 0,
        base_gen: 0,
        window: 512,
        ck_commitment: Default::default(),
        skip_window_override: None,
    };
    effects
        .insert_relational_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint2),
        ))
        .await
        .unwrap();

    // Both should succeed
}

// =============================================================================
// Context Store Tests
// =============================================================================

#[tokio::test]
async fn test_context_store_fetch_journal() {
    let effects = MockEffects::deterministic();

    // Use context_store accessor
    let store = effects.context_store();

    let journal = store.fetch_context_journal(test_context()).await;
    assert!(journal.is_ok());
}

#[tokio::test]
async fn test_context_store_insert_fact() {
    let effects = MockEffects::deterministic();

    let store = effects.context_store();

    let checkpoint = ChannelCheckpoint {
        context: test_context(),
        channel: test_channel(),
        chan_epoch: 1,
        base_gen: 100,
        window: 256,
        ck_commitment: Default::default(),
        skip_window_override: Some(256),
    };

    let result = store
        .insert_relational_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint),
        ))
        .await;

    assert!(result.is_ok());
}

// =============================================================================
// Order Clock Tests
// =============================================================================

#[tokio::test]
async fn test_order_time_generates_unique_values() {
    let effects = MockEffects::deterministic();

    let order1 = effects.order_time().await.unwrap();
    let order2 = effects.order_time().await.unwrap();
    let order3 = effects.order_time().await.unwrap();

    // Each order time should be unique (since MockEffects uses RNG)
    assert_ne!(order1.0, order2.0);
    assert_ne!(order2.0, order3.0);
    assert_ne!(order1.0, order3.0);
}

#[tokio::test]
async fn test_order_time_is_32_bytes() {
    let effects = MockEffects::deterministic();

    let order = effects.order_time().await.unwrap();

    assert_eq!(order.0.len(), 32);
}

// =============================================================================
// Journal Effects Base Tests
// =============================================================================

#[tokio::test]
async fn test_get_journal() {
    let effects = MockEffects::deterministic();

    let journal = effects.get_journal().await;
    assert!(journal.is_ok());
}

#[tokio::test]
async fn test_persist_journal() {
    let effects = MockEffects::deterministic();

    let journal = effects.get_journal().await.unwrap();
    let result = effects.persist_journal(&journal).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_merge_facts() {
    let effects = MockEffects::deterministic();

    let target = effects.get_journal().await.unwrap();
    let delta = aura_core::Journal::new();

    let result = effects.merge_facts(&target, &delta).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_refine_caps() {
    let effects = MockEffects::deterministic();

    let target = effects.get_journal().await.unwrap();
    let refinement = aura_core::Journal::new();

    let result = effects.refine_caps(&target, &refinement).await;
    assert!(result.is_ok());
}

// =============================================================================
// Relational Fact Type Tests
// =============================================================================

#[test]
fn test_channel_checkpoint_serialization() {
    let checkpoint = ChannelCheckpoint {
        context: test_context(),
        channel: test_channel(),
        chan_epoch: 42,
        base_gen: 100,
        window: 1024,
        ck_commitment: Default::default(),
        skip_window_override: Some(512),
    };

    let fact = RelationalFact::Protocol(
        aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint),
    );

    // Serialize to JSON
    let json = serde_json::to_string(&fact).expect("serialization should succeed");
    assert!(!json.is_empty());

    // Deserialize back
    let recovered: RelationalFact =
        serde_json::from_str(&json).expect("deserialization should succeed");

    if let RelationalFact::Protocol(aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(
        cp,
    )) = recovered
    {
        assert_eq!(cp.chan_epoch, 42);
        assert_eq!(cp.base_gen, 100);
        assert_eq!(cp.window, 1024);
        assert_eq!(cp.skip_window_override, Some(512));
    } else {
        panic!("expected AmpChannelCheckpoint variant");
    }
}

#[test]
fn test_channel_policy_serialization() {
    let policy = ChannelPolicy {
        context: test_context(),
        channel: test_channel(),
        skip_window: Some(2048),
    };

    let fact = RelationalFact::Protocol(aura_journal::ProtocolRelationalFact::AmpChannelPolicy(
        policy,
    ));

    // Serialize to JSON
    let json = serde_json::to_string(&fact).expect("serialization should succeed");
    assert!(!json.is_empty());

    // Deserialize back
    let recovered: RelationalFact =
        serde_json::from_str(&json).expect("deserialization should succeed");

    if let RelationalFact::Protocol(aura_journal::ProtocolRelationalFact::AmpChannelPolicy(p)) =
        recovered
    {
        assert_eq!(p.skip_window, Some(2048));
    } else {
        panic!("expected AmpChannelPolicy variant");
    }
}

#[test]
fn test_channel_policy_none_skip_window() {
    let policy = ChannelPolicy {
        context: test_context(),
        channel: test_channel(),
        skip_window: None,
    };

    let fact = RelationalFact::Protocol(aura_journal::ProtocolRelationalFact::AmpChannelPolicy(
        policy,
    ));

    let json = serde_json::to_string(&fact).expect("serialization should succeed");
    let recovered: RelationalFact =
        serde_json::from_str(&json).expect("deserialization should succeed");

    if let RelationalFact::Protocol(aura_journal::ProtocolRelationalFact::AmpChannelPolicy(p)) =
        recovered
    {
        assert_eq!(p.skip_window, None);
    } else {
        panic!("expected AmpChannelPolicy variant");
    }
}
