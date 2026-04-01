//! Reconfiguration integration coverage for device migration and guardian handoff flows.

#![allow(clippy::expect_used)]

use aura_agent::{
    AgentConfig, AuraEffectSystem, CoherenceStatus, ReconfigurationManager,
    ReconfigurationManagerError, SessionDelegationTransfer, SessionOwnerCapabilityScope,
};
use aura_core::{AuthorityId, ComposedBundle, SessionFootprint, SessionId};
use aura_effects::RuntimeCapabilityHandler;
use aura_journal::fact::{FactContent, ProtocolRelationalFact, RelationalFact};
use std::collections::BTreeSet;
use telltale_machine::{
    CanonicalPublicationContinuity, PendingEffectTreatment, ReconfigurationPlan,
    ReconfigurationPlanStep, RuntimeUpgradeCompatibility, RuntimeUpgradeExecutionConstraint,
    RuntimeUpgradeRequest, TransitionArtifactPhase,
};
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

    let outcome = manager
        .delegate_session(
            &effects,
            SessionDelegationTransfer::new(
                session_id,
                from_authority,
                to_authority,
                "device_migration",
            ),
        )
        .await
        .expect("delegate session");
    let receipt = outcome.receipt;
    let witness = outcome.witness;

    assert_eq!(receipt.session_id, session_id);
    assert_eq!(receipt.from_authority, from_authority);
    assert_eq!(receipt.to_authority, to_authority);
    assert_eq!(
        witness.link_boundary.bundle_id.as_deref(),
        Some("device_migration")
    );
    assert_eq!(
        witness.link_boundary.capability_scope,
        witness.capability_scope
    );
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
            SessionDelegationTransfer::new(
                session_id,
                from_authority,
                to_authority,
                "unregistered_bundle",
            ),
        )
        .await
        .expect_err("delegation without pre-registered bundle must fail closed");

    assert!(matches!(
        err,
        ReconfigurationManagerError::BundleNotRegistered { .. }
    ));
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
            SessionDelegationTransfer::new(
                session_id,
                from_authority,
                to_authority,
                "device_migration",
            ),
        )
        .await
        .expect_err("delegation without reconfiguration capability must fail closed");

    assert!(matches!(
        err,
        ReconfigurationManagerError::MissingCapability { .. }
    ));
}

#[tokio::test]
async fn delegation_without_recorded_source_ownership_fails_closed() {
    let from_authority = authority(14);
    let to_authority = authority(15);
    let session_id = session(16);
    let effects = AuraEffectSystem::simulation_for_test_for_authority(
        &AgentConfig::default(),
        from_authority,
    )
    .expect("simulation effect system");
    let manager = ReconfigurationManager::new();

    let err = manager
        .delegate_session(
            &effects,
            SessionDelegationTransfer::new(
                session_id,
                from_authority,
                to_authority,
                "device_migration",
            ),
        )
        .await
        .expect_err("delegation without a recorded source footprint must fail closed");

    assert!(matches!(
        err,
        ReconfigurationManagerError::DelegateSession { .. }
    ));
}

#[tokio::test]
async fn delegation_rejects_boundary_scope_mismatch() {
    let from_authority = authority(31);
    let to_authority = authority(32);
    let session_id = session(33);
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
            SessionDelegationTransfer::new(
                session_id,
                from_authority,
                to_authority,
                "device_migration",
            )
            .with_capability_scope(SessionOwnerCapabilityScope::Fragments(
                BTreeSet::from(["bundle:wrong-bundle".to_string()]),
            )),
        )
        .await
        .expect_err("boundary/scope mismatch must fail closed");

    assert!(matches!(
        err,
        ReconfigurationManagerError::InvalidLinkBoundary { .. }
    ));
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

    assert!(matches!(
        err,
        ReconfigurationManagerError::MissingCapability { .. }
    ));
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
            SessionDelegationTransfer::new(
                session_id,
                previous_guardian,
                replacement_guardian,
                "guardian_handoff",
            ),
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

#[tokio::test]
async fn delegation_carries_runtime_upgrade_artifacts_explicitly() {
    let from_authority = authority(41);
    let to_authority = authority(42);
    let session_id = session(43);

    let effects = AuraEffectSystem::simulation_for_test_for_authority(
        &AgentConfig::default(),
        from_authority,
    )
    .expect("simulation effect system");
    let manager = ReconfigurationManager::new();
    manager
        .record_native_session(from_authority, session_id)
        .await;
    manager
        .seed_runtime_upgrade_membership("device_migration", ["member-a", "member-b"])
        .await
        .expect("seed membership");

    let runtime_upgrade_request = RuntimeUpgradeRequest {
        upgrade_id: "upgrade/device-migration-delegation".to_string(),
        plan: ReconfigurationPlan {
            plan_id: "plan/device-migration-delegation".to_string(),
            steps: vec![ReconfigurationPlanStep {
                step_id: "cutover-1".to_string(),
                next_members: vec!["member-b".to_string(), "member-c".to_string()],
                placements: Vec::new(),
            }],
        },
        compatibility: RuntimeUpgradeCompatibility {
            execution_constraint: RuntimeUpgradeExecutionConstraint::PreserveBundleProfile,
            ownership_continuity_required: true,
            pending_effect_treatment: PendingEffectTreatment::PreservePending,
            canonical_publication_continuity:
                CanonicalPublicationContinuity::PreserveCanonicalTruth,
        },
        carried_publication_ids: vec!["pub:device".to_string()],
        invalidated_publication_ids: Vec::new(),
        carried_obligation_ids: vec!["obl:pending-send".to_string()],
        invalidated_obligation_ids: Vec::new(),
    };

    let outcome = manager
        .delegate_session(
            &effects,
            SessionDelegationTransfer::new(
                session_id,
                from_authority,
                to_authority,
                "device_migration",
            )
            .with_runtime_upgrade_request(runtime_upgrade_request.clone()),
        )
        .await
        .expect("delegate session with explicit runtime upgrade");

    assert_eq!(
        outcome
            .witness
            .runtime_upgrade_request
            .as_ref()
            .expect("request on witness")
            .upgrade_id,
        runtime_upgrade_request.upgrade_id
    );
    assert_eq!(
        outcome
            .witness
            .runtime_upgrade_snapshot
            .as_ref()
            .expect("snapshot on witness")
            .active_members,
        vec!["member-b".to_string(), "member-c".to_string()]
    );
    let execution = outcome
        .witness
        .runtime_upgrade_execution
        .as_ref()
        .expect("execution on witness");
    assert_eq!(
        execution
            .artifacts
            .iter()
            .map(|artifact| artifact.phase)
            .collect::<Vec<_>>(),
        vec![
            TransitionArtifactPhase::Staged,
            TransitionArtifactPhase::Admitted,
            TransitionArtifactPhase::CommittedCutover,
        ]
    );

    let snapshot = manager
        .runtime_upgrade_snapshot("device_migration")
        .await
        .expect("runtime-upgrade snapshot");
    assert_eq!(
        snapshot.active_members,
        vec!["member-b".to_string(), "member-c".to_string()]
    );
}

#[tokio::test]
async fn runtime_upgrade_records_public_execution_artifacts() {
    let manager = ReconfigurationManager::new();
    manager
        .seed_runtime_upgrade_membership("device_migration", ["member-a", "member-b"])
        .await
        .expect("seed membership");

    let request = RuntimeUpgradeRequest {
        upgrade_id: "upgrade/device-migration".to_string(),
        plan: ReconfigurationPlan {
            plan_id: "plan/device-migration".to_string(),
            steps: vec![ReconfigurationPlanStep {
                step_id: "cutover-1".to_string(),
                next_members: vec!["member-b".to_string(), "member-c".to_string()],
                placements: Vec::new(),
            }],
        },
        compatibility: RuntimeUpgradeCompatibility {
            execution_constraint: RuntimeUpgradeExecutionConstraint::PreserveBundleProfile,
            ownership_continuity_required: true,
            pending_effect_treatment: PendingEffectTreatment::PreservePending,
            canonical_publication_continuity:
                CanonicalPublicationContinuity::PreserveCanonicalTruth,
        },
        carried_publication_ids: vec!["pub:device".to_string()],
        invalidated_publication_ids: Vec::new(),
        carried_obligation_ids: vec!["obl:pending-send".to_string()],
        invalidated_obligation_ids: Vec::new(),
    };

    let execution = manager
        .execute_runtime_upgrade("device_migration", &request)
        .await
        .expect("execute runtime upgrade");

    assert_eq!(execution.upgrade_id, "upgrade/device-migration");
    assert_eq!(
        execution
            .artifacts
            .iter()
            .map(|artifact| artifact.phase)
            .collect::<Vec<_>>(),
        vec![
            TransitionArtifactPhase::Staged,
            TransitionArtifactPhase::Admitted,
            TransitionArtifactPhase::CommittedCutover,
        ]
    );
    assert_eq!(
        execution.final_members,
        vec!["member-b".to_string(), "member-c".to_string()]
    );

    let snapshot = manager
        .runtime_upgrade_snapshot("device_migration")
        .await
        .expect("runtime-upgrade snapshot");
    assert_eq!(
        snapshot.active_members,
        vec!["member-b".to_string(), "member-c".to_string()]
    );
    assert_eq!(snapshot.runtime_upgrades, vec![execution]);
}

#[tokio::test]
async fn runtime_upgrade_requires_reconfiguration_capability() {
    let manager =
        ReconfigurationManager::with_runtime_capabilities(RuntimeCapabilityHandler::from_pairs([
            ("reconfiguration", false),
        ]));
    manager
        .seed_runtime_upgrade_membership("device_migration", ["member-a", "member-b"])
        .await
        .expect("seed membership");

    let request = RuntimeUpgradeRequest {
        upgrade_id: "upgrade/device-migration".to_string(),
        plan: ReconfigurationPlan {
            plan_id: "plan/device-migration".to_string(),
            steps: vec![ReconfigurationPlanStep {
                step_id: "cutover-1".to_string(),
                next_members: vec!["member-b".to_string(), "member-c".to_string()],
                placements: Vec::new(),
            }],
        },
        compatibility: RuntimeUpgradeCompatibility {
            execution_constraint: RuntimeUpgradeExecutionConstraint::PreserveBundleProfile,
            ownership_continuity_required: true,
            pending_effect_treatment: PendingEffectTreatment::PreservePending,
            canonical_publication_continuity:
                CanonicalPublicationContinuity::PreserveCanonicalTruth,
        },
        carried_publication_ids: vec!["pub:device".to_string()],
        invalidated_publication_ids: Vec::new(),
        carried_obligation_ids: vec!["obl:pending-send".to_string()],
        invalidated_obligation_ids: Vec::new(),
    };

    let err = manager
        .execute_runtime_upgrade("device_migration", &request)
        .await
        .expect_err("runtime upgrade without reconfiguration capability must fail closed");

    assert!(matches!(
        err,
        ReconfigurationManagerError::MissingCapability { .. }
    ));
}
