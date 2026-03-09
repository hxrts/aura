//! Reconfiguration integration coverage for device migration and guardian handoff flows.

#![allow(clippy::expect_used)]

use aura_agent::{AgentConfig, AuraEffectSystem, CoherenceStatus, ReconfigurationManager};
use aura_core::{AuthorityId, ComposedBundle, SessionFootprint, SessionId};
use aura_effects::RuntimeCapabilityHandler;
use aura_journal::fact::{FactContent, ProtocolRelationalFact, RelationalFact};
use std::collections::BTreeSet;
use uuid::Uuid;

fn authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

fn session(seed: u8) -> SessionId {
    SessionId::from_uuid(Uuid::from_bytes([seed; 16]))
}

#[tokio::test]
async fn device_migration_delegation_persists_audit_fact_and_coherence() {
    let from_authority = authority(1);
    let to_authority = authority(2);
    let session_id = session(9);

    let effects = AuraEffectSystem::simulation_for_test_for_authority(
        &AgentConfig::default(),
        from_authority,
    )
    .expect("simulation effect system");
    let manager = ReconfigurationManager::new();
    let preloaded = manager
        .bundle("device_migration")
        .await
        .expect("device migration bundle should preload from manifests");
    assert!(preloaded.exports.contains("device_migration.request"));
    assert!(preloaded.imports.contains("device_migration.accept"));

    manager
        .record_native_session(from_authority, session_id)
        .await;

    let receipt = manager
        .delegate_session(
            &effects,
            None,
            session_id,
            from_authority,
            to_authority,
            Some("device_migration".to_string()),
        )
        .await
        .expect("delegate session");

    assert_eq!(receipt.session_id, session_id);
    assert_eq!(receipt.from_authority, from_authority);
    assert_eq!(receipt.to_authority, to_authority);
    assert_eq!(manager.verify_coherence().await, CoherenceStatus::Coherent);

    let committed = effects
        .load_committed_facts(from_authority)
        .await
        .expect("load committed facts");
    assert!(
        committed.iter().any(|fact| {
            matches!(
                &fact.content,
                FactContent::Relational(RelationalFact::Protocol(
                    ProtocolRelationalFact::SessionDelegation(delegation),
                )) if delegation.session_id == session_id
                    && delegation.from_authority == from_authority
                    && delegation.to_authority == to_authority
                    && delegation.bundle_id.as_deref() == Some("device_migration")
            )
        }),
        "delegation audit fact must be committed"
    );
}

#[tokio::test]
async fn delegation_requires_pre_registered_bundle_evidence() {
    let from_authority = authority(4);
    let to_authority = authority(5);
    let session_id = session(10);
    let effects = AuraEffectSystem::simulation_for_test_for_authority(
        &AgentConfig::default(),
        from_authority,
    )
    .expect("simulation effect system");
    let manager = ReconfigurationManager::new();
    manager
        .record_native_session(from_authority, session_id)
        .await;

    let err = manager
        .delegate_session(
            &effects,
            None,
            session_id,
            from_authority,
            to_authority,
            Some("unregistered_bundle".to_string()),
        )
        .await
        .expect_err("delegation without pre-registered bundle must fail closed");

    assert!(err.contains("pre-registered bundle"));
}

#[tokio::test]
async fn delegation_requires_reconfiguration_capability() {
    let from_authority = authority(11);
    let to_authority = authority(12);
    let session_id = session(13);
    let effects = AuraEffectSystem::simulation_for_test_for_authority(
        &AgentConfig::default(),
        from_authority,
    )
    .expect("simulation effect system");
    let manager =
        ReconfigurationManager::with_runtime_capabilities(RuntimeCapabilityHandler::from_pairs([
            ("reconfiguration", false),
        ]));
    manager
        .record_native_session(from_authority, session_id)
        .await;

    let err = manager
        .delegate_session(
            &effects,
            None,
            session_id,
            from_authority,
            to_authority,
            Some("device_migration".to_string()),
        )
        .await
        .expect_err("delegation without reconfiguration capability must fail closed");

    assert!(err.contains("requires runtime capability `reconfiguration`"));
}

#[tokio::test]
async fn bundle_lifecycle_linking_tracks_composed_sessions() {
    let manager = ReconfigurationManager::new();

    let mut left_fp = SessionFootprint::new();
    left_fp.add_native(session(1));
    let left = ComposedBundle::new(
        "left",
        vec!["sync.epoch_rotation".to_string()],
        BTreeSet::from(["sync.delta".to_string()]),
        BTreeSet::from(["chat.send".to_string()]),
        left_fp,
    );
    manager.register_bundle(left).await.expect("register left");

    let mut right_fp = SessionFootprint::new();
    right_fp.add_native(session(2));
    let right = ComposedBundle::new(
        "right",
        vec!["chat.channel".to_string()],
        BTreeSet::from(["chat.send".to_string()]),
        BTreeSet::from(["sync.delta".to_string()]),
        right_fp,
    );
    manager
        .register_bundle(right)
        .await
        .expect("register right");

    let linked = manager
        .link_bundles("left", "right", "linked")
        .await
        .expect("link bundles");
    let sessions = linked.session_footprint.all_sessions();
    assert!(sessions.contains(&session(1)));
    assert!(sessions.contains(&session(2)));
}

#[tokio::test]
async fn bundle_linking_requires_reconfiguration_capability() {
    let manager =
        ReconfigurationManager::with_runtime_capabilities(RuntimeCapabilityHandler::from_pairs([
            ("reconfiguration", false),
        ]));

    let mut left_fp = SessionFootprint::new();
    left_fp.add_native(session(21));
    let left = ComposedBundle::new(
        "left",
        vec!["sync.epoch_rotation".to_string()],
        BTreeSet::from(["sync.delta".to_string()]),
        BTreeSet::from(["chat.send".to_string()]),
        left_fp,
    );
    manager.register_bundle(left).await.expect("register left");

    let mut right_fp = SessionFootprint::new();
    right_fp.add_native(session(22));
    let right = ComposedBundle::new(
        "right",
        vec!["chat.channel".to_string()],
        BTreeSet::from(["chat.send".to_string()]),
        BTreeSet::from(["sync.delta".to_string()]),
        right_fp,
    );
    manager
        .register_bundle(right)
        .await
        .expect("register right");

    let err = manager
        .link_bundles("left", "right", "linked")
        .await
        .expect_err("bundle linking without reconfiguration capability must fail closed");

    assert!(err.contains("requires runtime capability `reconfiguration`"));
}

#[tokio::test]
async fn guardian_handoff_delegation_records_guardian_bundle() {
    let previous_guardian = authority(7);
    let replacement_guardian = authority(8);
    let session_id = session(33);

    let effects =
        AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), authority(9))
            .expect("simulation effect system");
    let manager = ReconfigurationManager::new();
    manager
        .record_native_session(previous_guardian, session_id)
        .await;

    manager
        .delegate_session(
            &effects,
            None,
            session_id,
            previous_guardian,
            replacement_guardian,
            Some("guardian_handoff".to_string()),
        )
        .await
        .expect("guardian handoff delegation");

    let committed = effects
        .load_committed_facts(authority(9))
        .await
        .expect("load committed facts");
    assert!(
        committed.iter().any(|fact| {
            matches!(
                &fact.content,
                FactContent::Relational(RelationalFact::Protocol(
                    ProtocolRelationalFact::SessionDelegation(delegation),
                )) if delegation.session_id == session_id
                    && delegation.bundle_id.as_deref() == Some("guardian_handoff")
            )
        }),
        "guardian handoff delegation fact must be committed"
    );
}
