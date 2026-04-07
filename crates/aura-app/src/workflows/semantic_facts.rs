mod lifecycle;
mod owner;
mod proofs;
mod publication;
#[cfg(test)]
use crate::ui_contract::{
    AuthoritativeSemanticFact, OperationId, OperationInstanceId, SemanticOperationCausality,
    SemanticOperationKind, SemanticOperationPhase, SemanticOperationStatus, WorkflowTerminalStatus,
};
#[allow(unused_imports)]
pub(in crate::workflows) use lifecycle::{
    operation_phase_fact, publish_authoritative_operation_failure_with_instance,
    publish_authoritative_operation_phase_with_instance, publish_exact_operation_lifecycle,
    ExactOperationLifecyclePublication,
};
#[allow(unused_imports)]
pub(in crate::workflows) use owner::{
    issue_semantic_operation_context, semantic_lifecycle_publication_capability,
    semantic_postcondition_proof_capability, semantic_readiness_publication_capability,
    SemanticOperationContext, SemanticWorkflowOwner,
};
#[allow(unused_imports)]
pub(in crate::workflows) use proofs::{
    authoritative_semantic_facts_snapshot, issue_account_created_proof,
    issue_channel_invitation_created_proof, issue_channel_membership_ready_proof,
    issue_device_enrollment_imported_proof, issue_device_enrollment_started_proof,
    issue_home_created_proof, issue_invitation_accepted_or_materialized_proof,
    issue_invitation_created_proof, issue_invitation_declined_proof,
    issue_invitation_exported_proof, issue_invitation_revoked_proof, issue_message_committed_proof,
    issue_pending_invitation_consumed_proof, prove_channel_membership_ready, prove_home_created,
    AccountCreatedProof, ChannelInvitationCreatedProof, ChannelMembershipReadyProof,
    DeviceEnrollmentImportedProof, DeviceEnrollmentStartedProof, HomeCreatedProof,
    InvitationAcceptedOrMaterializedProof, InvitationCreatedProof, InvitationDeclinedProof,
    InvitationExportedProof, InvitationRevokedProof, MessageCommittedProof,
    PendingInvitationConsumedProof,
};
#[allow(unused_imports)]
pub(in crate::workflows) use publication::{
    publish_authoritative_semantic_fact, replace_authoritative_semantic_facts_of_kind,
    update_authoritative_semantic_facts,
};

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn authoritative_status_for_instance(
    facts: &[AuthoritativeSemanticFact],
    operation_id: &OperationId,
    instance_id: &OperationInstanceId,
) -> Option<SemanticOperationStatus> {
    facts.iter().find_map(|fact| match fact {
        AuthoritativeSemanticFact::OperationStatus {
            operation_id: observed_operation_id,
            instance_id: Some(observed_instance_id),
            status,
            ..
        } if observed_operation_id == operation_id && observed_instance_id == instance_id => {
            Some(status.clone())
        }
        _ => None,
    })
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn assert_succeeded_with_postcondition(
    facts: &[AuthoritativeSemanticFact],
    operation_id: &OperationId,
    instance_id: &OperationInstanceId,
    expected_kind: SemanticOperationKind,
    postcondition_holds: impl Fn(&[AuthoritativeSemanticFact]) -> bool,
) {
    let status = authoritative_status_for_instance(facts, operation_id, instance_id)
        .unwrap_or_else(|| panic!("missing status for {operation_id:?}/{instance_id:?}"));
    assert_eq!(status.kind, expected_kind);
    assert_eq!(status.phase, SemanticOperationPhase::Succeeded);
    assert!(
        postcondition_holds(facts),
        "declared postcondition must hold after success for {operation_id:?}/{instance_id:?}"
    );
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn assert_terminal_failure_or_cancelled(
    facts: &[AuthoritativeSemanticFact],
    operation_id: &OperationId,
    instance_id: &OperationInstanceId,
    expected_kind: SemanticOperationKind,
) {
    let status = authoritative_status_for_instance(facts, operation_id, instance_id)
        .unwrap_or_else(|| panic!("missing status for {operation_id:?}/{instance_id:?}"));
    assert_eq!(status.kind, expected_kind);
    assert!(
        matches!(
            status.phase,
            SemanticOperationPhase::Failed | SemanticOperationPhase::Cancelled
        ),
        "expected failed/cancelled terminal status for {operation_id:?}/{instance_id:?}, got {:?}",
        status.phase
    );
}

#[cfg(test)]
pub(crate) fn assert_terminal_failure_status(
    terminal: &WorkflowTerminalStatus,
    expected_kind: SemanticOperationKind,
) {
    assert_eq!(terminal.status.kind, expected_kind);
    assert_eq!(terminal.status.phase, SemanticOperationPhase::Failed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_bridge::OfflineRuntimeBridge;
    use crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL;
    use crate::workflows::signals::read_signal_or_default;
    use crate::{AppConfig, AppCore};
    use async_lock::RwLock;
    use aura_core::types::identifiers::AuthorityId;
    use aura_core::ChannelId;
    use aura_core::InvitationId;
    use aura_core::{
        issue_operation_context, AuraError, OperationTimeoutBudget, OwnedShutdownToken, OwnerEpoch,
        PublicationSequence, TraceContext,
    };
    use std::sync::Arc;

    fn runtime_backed_test_app_core() -> Arc<RwLock<AppCore>> {
        let authority = AuthorityId::new_from_entropy([42; 32]);
        let runtime = Arc::new(OfflineRuntimeBridge::new(authority));
        runtime.set_pending_invitations(Vec::new());
        crate::testing::test_app_core_with_runtime(AppConfig::default(), runtime)
    }

    #[tokio::test]
    async fn authoritative_semantic_facts_snapshot_reads_owned_store_without_registered_signal() {
        let app_core = crate::testing::default_test_app_core();
        {
            let mut core = app_core.write().await;
            core.set_authoritative_semantic_facts(vec![
                AuthoritativeSemanticFact::PendingHomeInvitationReady,
            ]);
        }

        let facts = authoritative_semantic_facts_snapshot(&app_core)
            .await
            .expect("authoritative semantic facts should read from the owned app-core store");
        assert_eq!(
            facts,
            vec![AuthoritativeSemanticFact::PendingHomeInvitationReady]
        );
    }

    #[tokio::test]
    async fn authoritative_semantic_fact_update_restores_owned_store_on_signal_emit_failure() {
        let app_core = crate::testing::default_test_app_core();
        {
            let mut core = app_core.write().await;
            core.set_authoritative_semantic_facts(vec![
                AuthoritativeSemanticFact::PendingHomeInvitationReady,
            ]);
        }

        let error = publish_authoritative_semantic_fact(
            &app_core,
            owner::authorize_readiness_publication(AuthoritativeSemanticFact::ContactLinkReady {
                authority_id: "owner-a".into(),
                contact_count: 1,
            }),
        )
        .await
        .expect_err("signal emission should still fail when the signal is unregistered");
        assert!(matches!(error, AuraError::Internal { .. }));

        let facts = authoritative_semantic_facts_snapshot(&app_core)
            .await
            .expect("owned semantic-facts store should remain readable after failed emit");
        assert_eq!(
            facts,
            vec![AuthoritativeSemanticFact::PendingHomeInvitationReady]
        );
    }

    #[tokio::test]
    async fn concurrent_authoritative_fact_updates_do_not_lose_entries() {
        let app_core = runtime_backed_test_app_core();
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let first = publish_authoritative_semantic_fact(
            &app_core,
            owner::authorize_readiness_publication(
                AuthoritativeSemanticFact::PendingHomeInvitationReady,
            ),
        );
        let second = publish_authoritative_semantic_fact(
            &app_core,
            owner::authorize_readiness_publication(AuthoritativeSemanticFact::ContactLinkReady {
                authority_id: "owner-a".into(),
                contact_count: 1,
            }),
        );

        let (first_result, second_result) = futures::future::join(first, second).await;
        first_result.unwrap_or_else(|error| panic!("{error}"));
        second_result.unwrap_or_else(|error| panic!("{error}"));

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;

        assert!(facts.contains(&AuthoritativeSemanticFact::PendingHomeInvitationReady));
        assert!(
            facts.contains(&AuthoritativeSemanticFact::ContactLinkReady {
                authority_id: "owner-a".into(),
                contact_count: 1,
            })
        );
        assert_eq!(facts.len(), 2);
    }

    #[tokio::test]
    async fn no_op_authoritative_fact_update_does_not_bump_revision() {
        let app_core = crate::testing::default_test_app_core();
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap_or_else(|error| panic!("{error}"));
        }

        publish_authoritative_semantic_fact(
            &app_core,
            owner::authorize_readiness_publication(
                AuthoritativeSemanticFact::PendingHomeInvitationReady,
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));

        let before = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;

        update_authoritative_semantic_facts(&app_core, |_facts| {})
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let after = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.facts, before.facts);
    }

    #[tokio::test]
    async fn prove_home_created_requires_registered_homes_signal() {
        let app_core = crate::testing::default_test_app_core();
        let home_id = ChannelId::new(aura_core::Hash32([7; 32]));

        let error = prove_home_created(&app_core, home_id)
            .await
            .expect_err("home_created proof should require the homes signal");
        assert!(matches!(error, AuraError::Internal { .. }));
    }

    #[tokio::test]
    async fn exact_operation_lifecycle_publication_retains_instance_identity() {
        let app_core = runtime_backed_test_app_core();
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        publish_exact_operation_lifecycle(
            &app_core,
            ExactOperationLifecyclePublication::phase(
                semantic_lifecycle_publication_capability(),
                OperationId::invitation_accept_channel(),
                OperationInstanceId("tui-op-invitation_accept-3".to_string()),
                SemanticOperationKind::AcceptPendingChannelInvitation,
                SemanticOperationPhase::Succeeded,
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.contains(&AuthoritativeSemanticFact::OperationStatus {
            operation_id: OperationId::invitation_accept_channel(),
            instance_id: Some(OperationInstanceId(
                "tui-op-invitation_accept-3".to_string()
            )),
            causality: None,
            status: SemanticOperationStatus::new(
                SemanticOperationKind::AcceptPendingChannelInvitation,
                SemanticOperationPhase::Succeeded,
            ),
        }));
    }

    #[tokio::test]
    async fn exact_operation_lifecycle_round_trips_identity_under_contention() {
        let app_core = runtime_backed_test_app_core();
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let mut invitation_context = issue_operation_context(
            &owner::SEMANTIC_OPERATION_CONTEXT_CAPABILITY,
            OperationId::invitation_accept_channel(),
            OperationInstanceId("tui-op-invitation_accept-7".to_string()),
            OwnerEpoch::new(2),
            PublicationSequence::new(5),
            OperationTimeoutBudget::deferred_local_policy(),
            OwnedShutdownToken::detached(),
            TraceContext::detached(),
        );
        let mut send_context = issue_operation_context(
            &owner::SEMANTIC_OPERATION_CONTEXT_CAPABILITY,
            OperationId::send_message(),
            OperationInstanceId("tui-op-send_message-9".to_string()),
            OwnerEpoch::new(4),
            PublicationSequence::new(8),
            OperationTimeoutBudget::deferred_local_policy(),
            OwnedShutdownToken::detached(),
            TraceContext::detached(),
        );

        let invitation = publish_exact_operation_lifecycle(
            &app_core,
            ExactOperationLifecyclePublication::phase_from_context(
                semantic_lifecycle_publication_capability(),
                &mut invitation_context,
                SemanticOperationKind::AcceptPendingChannelInvitation,
                SemanticOperationPhase::WorkflowDispatched,
            ),
        );
        let send = publish_exact_operation_lifecycle(
            &app_core,
            ExactOperationLifecyclePublication::phase_from_context(
                semantic_lifecycle_publication_capability(),
                &mut send_context,
                SemanticOperationKind::SendChatMessage,
                SemanticOperationPhase::WorkflowDispatched,
            ),
        );

        let (invitation_result, send_result) = futures::future::join(invitation, send).await;
        invitation_result.unwrap_or_else(|error| panic!("{error}"));
        send_result.unwrap_or_else(|error| panic!("{error}"));

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.contains(&AuthoritativeSemanticFact::OperationStatus {
            operation_id: OperationId::invitation_accept_channel(),
            instance_id: Some(OperationInstanceId(
                "tui-op-invitation_accept-7".to_string()
            )),
            causality: Some(SemanticOperationCausality {
                owner_epoch: OwnerEpoch::new(2),
                publication_sequence: PublicationSequence::new(5),
            }),
            status: SemanticOperationStatus::new(
                SemanticOperationKind::AcceptPendingChannelInvitation,
                SemanticOperationPhase::WorkflowDispatched,
            ),
        }));
        assert!(facts.contains(&AuthoritativeSemanticFact::OperationStatus {
            operation_id: OperationId::send_message(),
            instance_id: Some(OperationInstanceId("tui-op-send_message-9".to_string())),
            causality: Some(SemanticOperationCausality {
                owner_epoch: OwnerEpoch::new(4),
                publication_sequence: PublicationSequence::new(8),
            }),
            status: SemanticOperationStatus::new(
                SemanticOperationKind::SendChatMessage,
                SemanticOperationPhase::WorkflowDispatched,
            ),
        }));
    }

    #[tokio::test]
    async fn batched_terminal_success_fact_records_owner_terminal_status() {
        let app_core = runtime_backed_test_app_core();
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let owner = SemanticWorkflowOwner::new(
            &app_core,
            OperationId::invitation_accept_contact(),
            Some(OperationInstanceId(
                "tui-op-invitation_accept-batched".to_string(),
            )),
            SemanticOperationKind::AcceptContactInvitation,
        );

        let _fact = owner
            .terminal_success_fact_with(issue_invitation_accepted_or_materialized_proof(
                InvitationId::new("batched-terminal-status"),
            ))
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(
            owner.terminal_status().await,
            Some(WorkflowTerminalStatus {
                causality: Some(SemanticOperationCausality::new(
                    OwnerEpoch::new(0),
                    PublicationSequence::new(0),
                )),
                status: SemanticOperationStatus::new(
                    SemanticOperationKind::AcceptContactInvitation,
                    SemanticOperationPhase::Succeeded,
                ),
            })
        );
    }

    #[test]
    fn exact_operation_lifecycle_from_context_carries_causality() {
        let mut context = issue_operation_context(
            &owner::SEMANTIC_OPERATION_CONTEXT_CAPABILITY,
            OperationId::invitation_accept_channel(),
            OperationInstanceId("tui-op-invitation_accept-3".to_string()),
            OwnerEpoch::new(7),
            PublicationSequence::new(11),
            OperationTimeoutBudget::deferred_local_policy(),
            OwnedShutdownToken::detached(),
            TraceContext::detached(),
        );

        let fact = ExactOperationLifecyclePublication::phase_from_context(
            semantic_lifecycle_publication_capability(),
            &mut context,
            SemanticOperationKind::AcceptPendingChannelInvitation,
            SemanticOperationPhase::WorkflowDispatched,
        )
        .into_fact();

        assert_eq!(
            fact,
            AuthoritativeSemanticFact::OperationStatus {
                operation_id: OperationId::invitation_accept_channel(),
                instance_id: Some(OperationInstanceId(
                    "tui-op-invitation_accept-3".to_string()
                )),
                causality: Some(SemanticOperationCausality::new(
                    OwnerEpoch::new(7),
                    PublicationSequence::new(11),
                )),
                status: SemanticOperationStatus::new(
                    SemanticOperationKind::AcceptPendingChannelInvitation,
                    SemanticOperationPhase::WorkflowDispatched,
                ),
            }
        );
    }
}
