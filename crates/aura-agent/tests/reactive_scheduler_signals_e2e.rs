//! End-to-end tests for reactive scheduler signals.
#![allow(missing_docs)]

use std::sync::Arc;
use std::time::Duration;

use aura_agent::fact_registry::build_fact_registry;
use aura_agent::reactive::{ReactivePipeline, SchedulerConfig, ViewUpdate};
use aura_agent::{core::AgentConfig, AuraEffectSystem};
use aura_app::signal_defs::{register_app_signals, CONTACTS_SIGNAL, ERROR_SIGNAL};
use aura_app::ReactiveHandler;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
use aura_core::Hash32;
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::DomainFact;
use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};
use aura_testkit::time::ControllableTimeSource;

fn order(n: u8) -> OrderTime {
    OrderTime([n; 32])
}

fn fact(order_n: u8, content: FactContent) -> Fact {
    let o = order(order_n);
    Fact::new(o.clone(), TimeStamp::OrderClock(o), content)
}

fn t(ms: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms: ms,
        uncertainty: None,
    }
}

#[tokio::test]
async fn contacts_signal_updates_from_contact_facts_as_snapshots() {
    let reactive = ReactiveHandler::new();
    register_app_signals(&reactive).await.unwrap();

    let time_effects = Arc::new(ControllableTimeSource::new(0));
    let own_authority = AuthorityId::new_from_entropy([42u8; 32]);
    let config = AgentConfig::default();
    // Use unique deterministic seed to avoid master key caching issues
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_authority(&config, 10007, own_authority).unwrap(),
    );

    let pipeline = ReactivePipeline::start(
        SchedulerConfig::default(),
        Arc::new(build_fact_registry()),
        time_effects,
        effects,
        own_authority,
        reactive.clone(),
    );

    let ctx = ContextId::new_from_entropy([10u8; 32]);
    let alice = AuthorityId::new_from_entropy([11u8; 32]);

    let added = ContactFact::Added {
        context_id: ctx,
        owner_id: own_authority,
        contact_id: alice,
        nickname: "Alice".to_string(),
        added_at: t(1),
    };
    let renamed = ContactFact::Renamed {
        context_id: ctx,
        owner_id: own_authority,
        contact_id: alice,
        new_nickname: "Alice Cooper".to_string(),
        renamed_at: t(2),
    };

    let mut updates = pipeline.subscribe();
    pipeline
        .publish_journal_facts(vec![
            fact(1, FactContent::Relational(added.to_generic())),
            fact(2, FactContent::Relational(renamed.to_generic())),
        ])
        .await;

    let update = match tokio::time::timeout(Duration::from_secs(1), updates.recv()).await {
        Ok(Ok(update)) => update,
        Ok(Err(err)) => panic!("expected scheduler batch, got recv error: {err}"),
        Err(_) => panic!("expected scheduler batch"),
    };
    assert!(matches!(update, ViewUpdate::Batch { count } if count > 0));

    let contacts_state = reactive.read(&*CONTACTS_SIGNAL).await.unwrap();

    let contact = match contacts_state.contact(&alice) {
        Some(contact) => contact,
        None => panic!("alice exists"),
    };
    assert_eq!(contact.nickname, "Alice Cooper");
}

#[tokio::test]
async fn contacts_signal_reflects_guardian_binding_protocol_fact() {
    let reactive = ReactiveHandler::new();
    register_app_signals(&reactive).await.unwrap();

    let time_effects = Arc::new(ControllableTimeSource::new(0));
    let own_authority = AuthorityId::new_from_entropy([43u8; 32]);
    let config = AgentConfig::default();
    // Use unique deterministic seed to avoid master key caching issues
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_authority(&config, 10008, own_authority).unwrap(),
    );

    let pipeline = ReactivePipeline::start(
        SchedulerConfig::default(),
        Arc::new(build_fact_registry()),
        time_effects,
        effects,
        own_authority,
        reactive.clone(),
    );

    let ctx = ContextId::new_from_entropy([12u8; 32]);
    let guardian = AuthorityId::new_from_entropy([13u8; 32]);

    let added = ContactFact::Added {
        context_id: ctx,
        owner_id: own_authority,
        contact_id: guardian,
        nickname: "Guardian".to_string(),
        added_at: t(1),
    };

    let binding = RelationalFact::Protocol(aura_journal::ProtocolRelationalFact::GuardianBinding {
        account_id: own_authority,
        guardian_id: guardian,
        binding_hash: Hash32::default(),
    });

    let mut updates = pipeline.subscribe();
    pipeline
        .publish_journal_facts(vec![
            fact(1, FactContent::Relational(added.to_generic())),
            fact(2, FactContent::Relational(binding)),
        ])
        .await;

    let update = match tokio::time::timeout(Duration::from_secs(1), updates.recv()).await {
        Ok(Ok(update)) => update,
        Ok(Err(err)) => panic!("expected scheduler batch, got recv error: {err}"),
        Err(_) => panic!("expected scheduler batch"),
    };
    assert!(matches!(update, ViewUpdate::Batch { count } if count > 0));

    let contacts_state = reactive.read(&*CONTACTS_SIGNAL).await.unwrap();

    let contact = match contacts_state.contact(&guardian) {
        Some(contact) => contact,
        None => panic!("guardian exists"),
    };
    assert!(
        contact.is_guardian,
        "guardian binding should reflect into contacts"
    );
}

#[tokio::test]
async fn malformed_domain_fact_bytes_emit_error_signal() {
    let reactive = ReactiveHandler::new();
    register_app_signals(&reactive).await.unwrap();

    let time_effects = Arc::new(ControllableTimeSource::new(0));
    let own_authority = AuthorityId::new_from_entropy([44u8; 32]);
    let config = AgentConfig::default();
    // Use unique deterministic seed to avoid master key caching issues
    let effects = Arc::new(
        AuraEffectSystem::simulation_for_authority(&config, 10009, own_authority).unwrap(),
    );

    let pipeline = ReactivePipeline::start(
        SchedulerConfig::default(),
        Arc::new(build_fact_registry()),
        time_effects,
        effects,
        own_authority,
        reactive.clone(),
    );

    let ctx = ContextId::new_from_entropy([14u8; 32]);
    let bad = RelationalFact::Generic {
        context_id: ctx,
        envelope: aura_core::types::facts::FactEnvelope {
            type_id: aura_core::types::facts::FactTypeId::from(CONTACT_FACT_TYPE_ID),
            schema_version: 1,
            encoding: aura_core::types::facts::FactEncoding::DagCbor,
            payload: vec![0xff, 0x00, 0x01],
        },
    };

    let mut updates = pipeline.subscribe();
    pipeline
        .publish_journal_facts(vec![fact(1, FactContent::Relational(bad))])
        .await;

    let update = match tokio::time::timeout(Duration::from_secs(1), updates.recv()).await {
        Ok(Ok(update)) => update,
        Ok(Err(err)) => panic!("expected scheduler batch, got recv error: {err}"),
        Err(_) => panic!("expected scheduler batch"),
    };
    assert!(matches!(update, ViewUpdate::Batch { count } if count > 0));

    let err = reactive.read(&*ERROR_SIGNAL).await.unwrap();

    let msg = match err {
        Some(msg) => msg,
        None => panic!("expected Some(AppError)"),
    };
    assert!(
        msg.to_string().contains("decode ContactFact"),
        "error should mention ContactFact decode failure, got: {msg}"
    );
}
