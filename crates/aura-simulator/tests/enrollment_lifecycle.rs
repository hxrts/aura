//! Genuine signed runtime lifecycle scenarios: no protected records or success
//! outcomes are fabricated by this L8 driver.
// Test setup and assertions intentionally fail immediately when a genuine runtime
// operation fails. The synchronous mutexes only capture callback observations;
// no guard is held across an await or used by production code.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::disallowed_types)]

use std::{error::Error, sync::Arc, time::Duration};

use aura_agent::{
    AgentConfig, EnrollmentLifecycleFailure, EnrollmentParticipantProfile, SharedTransport,
    SimulatedEnrollment,
};
use aura_core::{AuthorityId, ContextId, DeviceId, PhysicalTime, TimeoutBudget};
use aura_testkit::time::ManualPhysicalClock;

async fn issued() -> (SimulatedEnrollment, Vec<tempfile::TempDir>) {
    issued_with_broker(None).await
}

async fn issued_with_broker(
    broker: Option<aura_agent::BootstrapBrokerConfig>,
) -> (SimulatedEnrollment, Vec<tempfile::TempDir>) {
    let (fixture, roots, _clock) = issued_with_broker_and_clock(broker).await;
    (fixture, roots)
}

async fn issued_with_broker_and_clock(
    broker: Option<aura_agent::BootstrapBrokerConfig>,
) -> (
    SimulatedEnrollment,
    Vec<tempfile::TempDir>,
    Arc<ManualPhysicalClock>,
) {
    let roots: Vec<_> = (0..2)
        .map(|_| tempfile::tempdir().expect("isolated native profile"))
        .collect();
    let profiles = std::array::from_fn(|index| {
        let seed = 151 + index as u8 * 3;
        let config = AgentConfig {
            device_id: DeviceId::new_from_entropy([seed + 1; 32]),
            storage: aura_agent::core::config::StorageConfig {
                base_path: roots[index].path().to_path_buf(),
                ..Default::default()
            },
            bootstrap_broker: if index == 1 {
                broker.clone().unwrap_or_default()
            } else {
                aura_agent::BootstrapBrokerConfig::default()
            },
            ..Default::default()
        };
        EnrollmentParticipantProfile {
            config,
            authority: AuthorityId::new_from_entropy([seed; 32]),
            context: ContextId::new_from_entropy([seed + 2; 32]),
            seed: u64::from(seed),
        }
    });
    let clock = Arc::new(ManualPhysicalClock::new(5_000));
    let startup = TimeoutBudget::from_start_and_timeout(
        &PhysicalTime {
            ts_ms: 5_000,
            uncertainty: None,
        },
        Duration::from_secs(120),
    )
    .expect("original selected startup window");
    let fixture =
        SimulatedEnrollment::prepare(profiles, clock.clone(), SharedTransport::default(), startup)
            .await
            .expect("genuine signed issuance");
    (fixture, roots, clock)
}

async fn issued_through_broker() -> (
    SimulatedEnrollment,
    Vec<tempfile::TempDir>,
    aura_agent::TaskSupervisor,
    aura_core::invitation::BootstrapBrokerInvitation,
) {
    use aura_agent::{
        BootstrapBrokerConfig, BootstrapBrokerLanBindPolicy, BootstrapBrokerRegistration,
        LocalBootstrapBrokerService, TaskSupervisor,
    };
    use aura_core::invitation::{BootstrapBrokerInvitation, InvitationPurpose};
    let broker = LocalBootstrapBrokerService::bind(
        "127.0.0.1:0",
        Duration::from_secs(120),
        "genuine-batch-auth",
        BootstrapBrokerLanBindPolicy::LoopbackOnly,
    )
    .await
    .expect("actual production broker bind");
    let supervisor = TaskSupervisor::new();
    let group = supervisor.group("genuine_enrollment_broker");
    broker.start(&group);
    let config = BootstrapBrokerConfig::default()
        .enabled(true)
        .with_base_url(broker.public_url())
        .with_auth_token("genuine-batch-auth")
        .with_invitation_retrieval_token("genuine-recipient-once");
    let (fixture, roots) = issued_with_broker(Some(config)).await;
    let recipient = fixture.recipient().clone();
    let authority = recipient.authority_id();
    let device = recipient.runtime().effects().config().device_id;
    broker
        .register(BootstrapBrokerRegistration {
            authority_id: authority.to_string(),
            device_id: device,
            address: "127.0.0.1:0".into(),
            invitation_retrieval_token: "genuine-recipient-once".into(),
            nickname_suggestion: None,
        })
        .await
        .expect("actual recipient credential registration");
    let original = BootstrapBrokerInvitation {
        recipient_authority_id: authority.to_string(),
        recipient_device_id: device,
        purpose: InvitationPurpose::DeviceEnrollment,
        invitation_code: fixture.start().enrollment_code.clone(),
    };
    broker
        .queue_invitation(original.clone())
        .await
        .expect("actual signed delivery queued");
    (fixture, roots, supervisor, original)
}

#[tokio::test]
async fn genuine_signed_bootstrap_enrollment_host_refusal_retains_original_and_disposes() {
    use aura_app::ui::workflows::invitation::{
        receive_bootstrap_invitation_batch, BootstrapBatchFailureReason,
    };
    use aura_core::{
        effects::task::{RequiredTaskExit, RequiredTaskTerminalFailure},
        AuraError,
    };
    let (fixture, _roots, supervisor, original) = issued_through_broker().await;
    let issuer = fixture.issuer().clone();
    let recipient = fixture.recipient().clone();
    let native = Arc::new(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    let host_native = native.clone();
    let host_error = AuraError::Storage {
        message: "genuine prefill host refused".into(),
        source: Some(native.clone()),
    };
    let outcome = receive_bootstrap_invitation_batch(
        recipient.clone().as_runtime_bridge(),
        None,
        Arc::new(move |_| {
            Err(AuraError::Storage {
                message: "genuine prefill host refused".into(),
                source: Some(host_native.clone()),
            })
        }),
    )
    .expect("actual native required receiver admission")
    .await
    .expect("original receiver observation");
    let failure = match outcome {
        Err(failure) => failure,
        Ok(_) => panic!("host must refuse"),
    };
    eprintln!("[genuine-bootstrap-host] receiver disposition: {failure:?}");
    let returned = fixture.fail(host_error.into()).await;
    supervisor
        .shutdown_with_timeout(Duration::from_secs(5))
        .await
        .expect("original broker supervised disposal");
    assert!(matches!(
        failure.reason(),
        BootstrapBatchFailureReason::Failed(AuraError::Storage { .. })
    ));
    assert_eq!(failure.refused_deliveries(), &[original]);
    assert!(failure.completed().imported.is_empty());
    assert!(failure.completed().enrollment_prefill.is_none());
    assert!(std::ptr::eq(
        failure
            .source()
            .expect("primary host cause")
            .source()
            .expect("original native cause")
            .downcast_ref::<std::io::Error>()
            .expect("native IO"),
        native.as_ref()
    ));
    let mut current: Option<&(dyn Error + 'static)> = Some(&returned);
    let lifecycle = loop {
        let error = current.expect("original lifecycle failure");
        if let Some(actual) = error.downcast_ref::<EnrollmentLifecycleFailure>() {
            break actual;
        }
        current = error.source();
    };
    // The failed receiver's domain report survives resource disposal. It is
    // expected evidence, never rewritten into a clean semantic outcome.
    assert_eq!(lifecycle.cleanup_failures().len(), 1);
    let mut cause: Option<&(dyn Error + 'static)> = Some(&lifecycle.cleanup_failures()[0]);
    let mut terminal = None;
    while let Some(error) = cause {
        if let Some(actual) = error.downcast_ref::<RequiredTaskTerminalFailure>() {
            terminal = Some(actual);
            break;
        }
        cause = error.source();
    }
    let terminal = terminal.expect("actual acknowledged required domain failure");
    let RequiredTaskExit::Failed(execution) = terminal.execution() else {
        panic!("original receiver execution failed")
    };
    assert!(std::ptr::eq(
        execution
            .source()
            .expect("original execution host cause")
            .downcast_ref::<std::io::Error>()
            .expect("native host IO"),
        native.as_ref()
    ));
    assert!(issuer.runtime().tasks().active_tasks().is_empty());
    assert!(recipient.runtime().tasks().active_tasks().is_empty());
    assert!(supervisor.active_tasks().is_empty());
}

#[tokio::test]
async fn genuine_signed_bootstrap_enrollment_prefills_accepts_and_reopens() {
    use aura_app::ui::workflows::invitation::receive_bootstrap_invitation_batch;
    let (fixture, _roots, supervisor, original) = issued_through_broker().await;
    let host_prefill = Arc::new(std::sync::Mutex::new(None));
    let retained_prefill = host_prefill.clone();
    let received = receive_bootstrap_invitation_batch(
        fixture.recipient().clone().as_runtime_bridge(),
        None,
        Arc::new(move |code| {
            *retained_prefill.lock().expect("observed host prefill") = Some(code.to_owned());
            Ok(())
        }),
    )
    .expect("actual native receiver admission")
    .await
    .expect("original receiver observation");
    // Settle original issuance and task custody before asserting host results.
    let stopped = fixture.accept().await;
    supervisor
        .shutdown_with_timeout(Duration::from_secs(5))
        .await
        .expect("original broker disposal");
    let received = received.expect("host acknowledged actual signed prefill");
    assert!(received.imported.is_empty());
    assert_eq!(
        received.enrollment_prefill.as_deref(),
        Some(original.invitation_code.as_str())
    );
    assert_eq!(
        host_prefill
            .lock()
            .expect("retained host observation")
            .as_deref(),
        Some(original.invitation_code.as_str())
    );
    assert!(supervisor.active_tasks().is_empty());
    stopped
        .expect("canonical acceptance and strict resource disposal")
        .reopen_and_shutdown(SharedTransport::default())
        .await
        .expect("same profiles retain original native confirmation and active package");
}

#[tokio::test]
async fn genuine_signed_enrollment_accepts_and_disposes() {
    let (fixture, _roots) = issued().await;
    fixture
        .accept()
        .await
        .expect("native confirmation and strict disposal");
}

#[tokio::test]
async fn genuine_signed_enrollment_rejects_and_disposes() {
    let (fixture, _roots) = issued().await;
    let stopped = fixture
        .reject()
        .await
        .expect("native rejection and resource-only disposal");
    assert!(stopped.rejected_source().is_some());
    stopped
        .reopen_and_shutdown(SharedTransport::default())
        .await
        .expect("same rejected profiles retain the original terminal decision");
}

#[tokio::test]
async fn genuine_signed_enrollment_reopens_original_profiles() {
    let (fixture, _roots) = issued().await;
    let stopped = fixture
        .accept()
        .await
        .expect("original confirmation and disposal");
    stopped
        .reopen_and_shutdown(SharedTransport::default())
        .await
        .expect("same native profiles revalidate confirmation and active package");
}

#[tokio::test]
async fn genuine_signed_enrollment_failure_after_issuance_settles_all_owners() {
    let (fixture, _roots) = issued().await;
    let issuer = fixture.issuer().clone();
    let recipient = fixture.recipient().clone();
    let actual = match fixture.finish_accepted().await {
        Err(source) => source,
        Ok(_) => panic!("unimported original cannot have a native confirmation"),
    };
    let mut source: Option<&(dyn Error + 'static)> = Some(&actual);
    let mut retained = None;
    while let Some(current) = source {
        if let Some(failure) = current.downcast_ref::<EnrollmentLifecycleFailure>() {
            retained = Some(failure);
            break;
        }
        source = current.source();
    }
    let retained = retained.expect("primary and cleanup outcomes retained separately");
    assert!(
        matches!(
            retained.primary(),
            aura_agent::AgentError::EnrollmentManifest(_)
        ),
        "actual native missing-confirmation failure retained"
    );
    assert!(
        retained.settled_execution_failure().is_some(),
        "actual original disposal failure retained"
    );
    assert!(
        retained.cleanup_failures().is_empty(),
        "independent cleanup failed: {:?}",
        retained.cleanup_failures()
    );
    assert!(issuer.runtime().tasks().active_tasks().is_empty());
    assert!(recipient.runtime().tasks().active_tasks().is_empty());
}

#[derive(Clone)]
struct PublicAppSubmission(
    Arc<std::sync::Mutex<Vec<aura_app::ui_contract::SemanticOperationStatus>>>,
);
#[async_trait::async_trait]
impl aura_app::frontend_primitives::SubmittedOperationPublisher for PublicAppSubmission {
    async fn publish_dispatched(
        &self,
        _: &aura_app::ui_contract::OperationId,
        _: &aura_app::ui_contract::OperationInstanceId,
        _: aura_app::ui_contract::SemanticOperationKind,
    ) {
        panic!("test allocates an unpublished prior submission");
    }
    fn publish_terminal(
        &self,
        intent: aura_app::frontend_primitives::SubmittedTerminalIntent,
    ) -> aura_app::frontend_primitives::SubmittedTerminalObservation {
        let acknowledgment = aura_app::frontend_primitives::SubmittedTerminalAcknowledgment {
            operation_id: intent.operation_id,
            instance_id: intent.instance_id,
            scope: aura_app::frontend_primitives::SubmittedTerminalAcknowledgmentScope::ComponentObserved,
        };
        self.0.lock().unwrap().push(intent.status);
        drop(intent.retained);
        Box::pin(async move { Ok(acknowledgment) })
    }
    fn relinquish(
        &self,
        _: &aura_app::ui_contract::OperationId,
        _: &aura_app::ui_contract::OperationInstanceId,
        _: aura_app::ui_contract::SemanticOperationKind,
    ) {
    }
}

#[tokio::test]
async fn genuine_public_app_enrollment_import_acknowledges_host_storage_and_reopens() {
    use aura_app::frontend_primitives::WorkflowHandoffSubmission;
    use aura_app::ui::workflows::invitation::{
        import_device_enrollment_with_terminal_status, EnrollmentImportHostPolicy,
    };
    use aura_app::ui_contract::{
        OperationId, OperationInstanceId, SemanticOperationKind, SemanticOperationPhase,
    };
    use aura_core::{effects::StorageCoreEffects, AuraError};

    eprintln!("[genuine-app-import] issued enter");
    let (fixture, _roots) = issued().await;
    eprintln!("[genuine-app-import] issued ready");
    let app = fixture.recipient_app();
    let host_root = tempfile::tempdir().expect("isolated real host storage");
    let host = Arc::new(aura_effects::FilesystemStorageHandler::new(
        host_root.path().to_path_buf(),
    ));
    let instance = OperationInstanceId("genuine-public-app-enrollment".into());
    let mut refused_publication = None;
    let result = async {
        eprintln!("[genuine-app-import] init_signals enter");
        aura_core::time::timeout::execute_with_timeout_budget(
            fixture.selected_clock(),
            fixture.original_startup_window(),
            || async {
                aura_app::AppCore::init_signals_with_hooks(&app)
                    .await
                    .map_err(|source| AuraError::Internal {
                        message: "actual recipient app initialization failed".into(),
                        source: Some(Arc::new(source)),
                    })
            },
        )
        .await
        .map_err(|source| AuraError::Internal {
            message: "original recipient startup window failed".into(),
            source: Some(Arc::new(source)),
        })?;
        eprintln!("[genuine-app-import] init_signals ready");
        let admission = app
            .read()
            .await
            .enrollment_import_admission()
            .cloned()
            .ok_or_else(|| AuraError::invalid("actual recipient app admission missing"))?;
        let transfer = fixture
            .start()
            .manifest_transfer
            .as_ref()
            .ok_or_else(|| AuraError::invalid("actual signed transfer missing"))?;
        let host_write = host.clone();
        let prior_events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let prior = WorkflowHandoffSubmission::submit_with_instance_unpublished(
            PublicAppSubmission(prior_events.clone()),
            OperationId::device_enrollment(),
            SemanticOperationKind::ImportDeviceEnrollmentCode,
            instance.clone(),
        );
        let admitted = import_device_enrollment_with_terminal_status(
            &app,
            &admission,
            fixture.start().enrollment_code.clone(),
            Some(aura_app::ui_contract::EnrollmentManifestTransferInput {
                manifest_code: transfer.manifest_code.clone(),
                initiator_verifier_code: transfer.initiator_verifier_code.clone(),
            }),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::Finalize(Arc::new(move |completion| {
                let host_write = host_write.clone();
                Box::pin(async move {
                    // This test record observes public completion data only. It
                    // neither stores keys nor grants enrollment authority.
                    let bytes = serde_json::to_vec(&(
                        completion.subject_authority(),
                        completion.device_id(),
                        completion.invitation_id(),
                        completion.manifest_digest(),
                    ))
                    .map_err(|source| AuraError::Storage {
                        message: "host completion encoding failed".into(),
                        source: Some(Arc::new(source)),
                    })?;
                    host_write
                        .store("enrollment-completion", bytes)
                        .await
                        .map_err(|source| AuraError::Storage {
                            message: "actual host storage acknowledgment failed".into(),
                            source: Some(Arc::new(source)),
                        })
                })
            })),
        );
        let mut observation = match admitted {
            Ok(observation) => observation,
            Err(refusal) => {
                let primary = refusal.source_error().clone();
                refused_publication = Some(prior.fail(refusal.semantic_error()).await);
                drop(refusal);
                return Err(primary);
            }
        };
        if let Err((prior, error)) = observation.accept_handoff(prior) {
            refused_publication = Some(prior.fail(error.semantic_error()).await);
            return Err(error.source_error().clone());
        }

        eprintln!("[genuine-app-import] original observation enter");
        if !prior_events.lock().unwrap().is_empty() {
            return Err(AuraError::invalid(
                "handed-off prior owner published a terminal",
            ));
        }
        let outcome = observation.await;
        eprintln!(
            "[genuine-app-import] original observation ready: {:?}",
            outcome.terminal
        );
        let completion = outcome.result?;
        let terminal = outcome
            .terminal
            .ok_or_else(|| AuraError::invalid("actual app terminal missing"))?;
        if terminal.status.phase != SemanticOperationPhase::Succeeded {
            return Err(AuraError::invalid("actual app import did not succeed"));
        }
        let expected = serde_json::to_vec(&(
            completion.subject_authority(),
            completion.device_id(),
            completion.invitation_id(),
            completion.manifest_digest(),
        ))
        .map_err(|source| AuraError::Storage {
            message: "completion observation encoding failed".into(),
            source: Some(Arc::new(source)),
        })?;
        let actual = host
            .retrieve("enrollment-completion")
            .await
            .map_err(|source| AuraError::Storage {
                message: "host completion readback failed".into(),
                source: Some(Arc::new(source)),
            })?;
        if actual.as_ref() != Some(&expected) {
            return Err(AuraError::invalid(
                "actual persisted host completion differs",
            ));
        }
        let facts = app.read().await.authoritative_semantic_facts();
        let terminals: Vec<_> = facts
            .iter()
            .filter_map(|fact| match fact {
                aura_app::ui_contract::AuthoritativeSemanticFact::OperationStatus {
                    instance_id,
                    status,
                    ..
                } if instance_id.as_ref() == Some(&instance)
                    && matches!(
                        status.phase,
                        SemanticOperationPhase::Succeeded
                            | SemanticOperationPhase::Failed
                            | SemanticOperationPhase::Cancelled
                    ) =>
                {
                    Some(status.phase)
                }
                _ => None,
            })
            .collect();
        if terminals != vec![SemanticOperationPhase::Succeeded] {
            return Err(AuraError::invalid(
                "original app instance must have one success terminal",
            ));
        }
        Ok::<_, AuraError>(expected)
    }
    .await;
    // Release the external AppCore/runtime projection before requesting a true
    // profile reopen. Strict task shutdown alone cannot release this Arc.
    drop(app);
    let expected = match result {
        Ok(expected) => expected,
        Err(primary) => panic!(
            "genuine public app import failed with retained cleanup: {:?}; prior publication: {:?}",
            fixture.fail(primary.into()).await,
            refused_publication
        ),
    };
    eprintln!("[genuine-app-import] finish_accepted enter");
    let stopped = fixture
        .finish_accepted()
        .await
        .expect("actual native confirmation and strict original disposal");
    eprintln!("[genuine-app-import] finish_accepted ready");
    drop(host);
    let reopened_host = aura_effects::FilesystemStorageHandler::new(host_root.path().to_path_buf());
    assert_eq!(
        reopened_host
            .retrieve("enrollment-completion")
            .await
            .unwrap(),
        Some(expected)
    );
    eprintln!("[genuine-app-import] reopen enter");
    stopped
        .reopen_and_shutdown(SharedTransport::default())
        .await
        .expect("same confirmed profiles reopen and strictly dispose");
    eprintln!("[genuine-app-import] reopen ready");
}

#[tokio::test]
async fn genuine_public_app_enrollment_cancelled_before_host_ack_has_one_terminal() {
    use aura_app::frontend_primitives::WorkflowHandoffSubmission;
    use aura_app::ui::workflows::invitation::{
        import_device_enrollment_with_terminal_status, EnrollmentImportHostPolicy,
    };
    use aura_app::ui_contract::{
        AuthoritativeSemanticFact, OperationId, OperationInstanceId, SemanticOperationKind,
        SemanticOperationPhase,
    };
    use aura_core::{effects::StorageCoreEffects, AuraError};

    let (fixture, _roots) = issued().await;
    let issuer = fixture.issuer().clone();
    let recipient = fixture.recipient().clone();
    let app = fixture.recipient_app();
    aura_core::time::timeout::execute_with_timeout_budget(
        fixture.selected_clock(),
        fixture.original_startup_window(),
        || async {
            aura_app::AppCore::init_signals_with_hooks(&app)
                .await
                .map_err(|source| AuraError::Internal {
                    message: "actual recipient app initialization failed".into(),
                    source: Some(Arc::new(source)),
                })
        },
    )
    .await
    .expect("actual app signal initialization under original selected startup window");
    let admission = app
        .read()
        .await
        .enrollment_import_admission()
        .cloned()
        .expect("actual recipient app admission");
    let transfer = fixture
        .start()
        .manifest_transfer
        .as_ref()
        .expect("original signed transfer");
    let host_root = tempfile::tempdir().expect("isolated real host storage");
    let host = Arc::new(aura_effects::FilesystemStorageHandler::new(
        host_root.path().to_path_buf(),
    ));
    let (entered, observed) = tokio::sync::oneshot::channel();
    let entered = Arc::new(std::sync::Mutex::new(Some(entered)));
    let instance = OperationInstanceId("genuine-public-app-host-cancelled".into());
    let prior_events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let prior = WorkflowHandoffSubmission::submit_with_instance_unpublished(
        PublicAppSubmission(prior_events.clone()),
        OperationId::device_enrollment(),
        SemanticOperationKind::ImportDeviceEnrollmentCode,
        instance.clone(),
    );
    let mut observation = import_device_enrollment_with_terminal_status(
        &app,
        &admission,
        fixture.start().enrollment_code.clone(),
        Some(aura_app::ui_contract::EnrollmentManifestTransferInput {
            manifest_code: transfer.manifest_code.clone(),
            initiator_verifier_code: transfer.initiator_verifier_code.clone(),
        }),
        Some(instance.clone()),
        EnrollmentImportHostPolicy::Finalize(Arc::new(move |_completion| {
            let entered = entered
                .lock()
                .expect("original host callback gate")
                .take()
                .expect("one original host invocation");
            Box::pin(async move {
                entered.send(()).expect("original host callback observed");
                futures::future::pending::<Result<(), AuraError>>().await
            })
        })),
    )
    .unwrap_or_else(|refusal| panic!("actual app import admission: {}", refusal.source_error()));
    observation
        .accept_handoff(prior)
        .unwrap_or_else(|_| panic!("original workflow handoff"));
    let mut observation = Box::pin(observation);
    let reached_host = tokio::select! {
        result = observed => result.is_ok(),
        _ = &mut observation => false,
    };
    drop(observation);
    let reported = fixture
        .fail(aura_agent::AgentError::config(
            "original app import observer cancelled before host acknowledgment",
        ))
        .await;
    assert!(
        reached_host,
        "actual host callback was not reached: {reported}"
    );
    let terminals: Vec<_> = app
        .read()
        .await
        .authoritative_semantic_facts()
        .iter()
        .filter_map(|fact| match fact {
            AuthoritativeSemanticFact::OperationStatus {
                instance_id,
                status,
                ..
            } if instance_id.as_ref() == Some(&instance)
                && matches!(
                    status.phase,
                    SemanticOperationPhase::Succeeded
                        | SemanticOperationPhase::Failed
                        | SemanticOperationPhase::Cancelled
                ) =>
            {
                Some(status.phase)
            }
            _ => None,
        })
        .collect();
    assert_eq!(terminals, vec![SemanticOperationPhase::Cancelled]);
    assert!(prior_events.lock().unwrap().is_empty());
    assert_eq!(host.retrieve("enrollment-completion").await.unwrap(), None);
    assert!(issuer.runtime().tasks().active_tasks().is_empty());
    assert!(recipient.runtime().tasks().active_tasks().is_empty());
}

#[tokio::test]
async fn genuine_public_app_enrollment_selected_provider_fault_refuses_host_ack() {
    use aura_app::frontend_primitives::WorkflowHandoffSubmission;
    use aura_app::ui::workflows::invitation::{
        import_device_enrollment_with_terminal_status, EnrollmentImportHostPolicy,
        PreparedEnrollmentHostCommit,
    };
    use aura_app::ui_contract::{
        OperationId, OperationInstanceId, SemanticOperationKind, SemanticOperationPhase,
    };
    use aura_core::effects::time::{TimeError, TimeProviderOperation};
    use aura_core::AuraError;
    use std::sync::atomic::{AtomicBool, Ordering};

    let (fixture, _roots, clock) = issued_with_broker_and_clock(None).await;
    let app = fixture.recipient_app();
    aura_app::AppCore::init_signals_with_hooks(&app)
        .await
        .expect("actual selected recipient app");
    let admission = app
        .read()
        .await
        .enrollment_import_admission()
        .cloned()
        .expect("actual app admission");
    let transfer = fixture
        .start()
        .manifest_transfer
        .as_ref()
        .expect("original signed transfer");
    let native = Arc::new(std::io::Error::new(
        std::io::ErrorKind::Interrupted,
        "selected host checkpoint provider failed",
    ));
    let selected_native = native.clone();
    let committed = Arc::new(AtomicBool::new(false));
    let selected_commit = committed.clone();
    let instance = OperationInstanceId("genuine-public-app-provider-refusal".into());
    let prior_events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let prior = WorkflowHandoffSubmission::submit_with_instance_unpublished(
        PublicAppSubmission(prior_events.clone()),
        OperationId::device_enrollment(),
        SemanticOperationKind::ImportDeviceEnrollmentCode,
        instance.clone(),
    );
    let admitted = import_device_enrollment_with_terminal_status(
        &app,
        &admission,
        fixture.start().enrollment_code.clone(),
        Some(aura_app::ui_contract::EnrollmentManifestTransferInput {
            manifest_code: transfer.manifest_code.clone(),
            initiator_verifier_code: transfer.initiator_verifier_code.clone(),
        }),
        Some(instance),
        EnrollmentImportHostPolicy::Prepare(Arc::new(move |_completion| {
            let clock = clock.clone();
            let native = selected_native.clone();
            let committed = selected_commit.clone();
            Box::pin(async move {
                clock
                    .fail_next_current_task_observation(TimeError::ProviderFailure {
                        operation: TimeProviderOperation::ReadPhysicalClock,
                        source: Some(native),
                    })
                    .await
                    .map_err(|source| AuraError::Internal {
                        message: "install selected provider fault".into(),
                        source: Some(Arc::new(source)),
                    })?;
                Ok(PreparedEnrollmentHostCommit::new(move || {
                    committed.store(true, Ordering::SeqCst);
                    Ok(())
                }))
            })
        })),
    );
    let mut observation = match admitted {
        Ok(observation) => observation,
        Err(refusal) => {
            let source = refusal.source_error().clone();
            let publication = prior.fail(refusal.semantic_error()).await;
            let cleanup = fixture.fail(source.clone().into()).await;
            assert!(
                publication.is_ok(),
                "original refusal publication: {cleanup}"
            );
            panic!("actual app import admission refused: {source}; cleanup: {cleanup}");
        }
    };
    observation
        .accept_handoff(prior)
        .unwrap_or_else(|_| panic!("original workflow handoff"));
    let outcome = observation.await;
    let failure = outcome
        .result
        .expect_err("selected provider refused final host ACK");
    assert_eq!(
        outcome
            .terminal
            .expect("original app terminal")
            .status
            .phase,
        SemanticOperationPhase::Failed
    );
    assert!(!committed.load(Ordering::SeqCst));
    assert!(prior_events.lock().unwrap().is_empty());
    let mut cause: Option<&(dyn Error + 'static)> = Some(&failure);
    let mut original_provider = false;
    while let Some(error) = cause {
        if let Some(actual) = error.downcast_ref::<std::io::Error>() {
            original_provider = std::ptr::eq(actual, native.as_ref());
            break;
        }
        cause = error.source();
    }
    assert!(
        original_provider,
        "original selected provider source retained"
    );
    drop(app);
    let (_reported, stopped) = fixture
        .settle_accepted_host_failure(failure.into())
        .await
        .expect("provider refusal and original native completion settle separately");
    drop(admission);
    stopped
        .reopen_and_shutdown(SharedTransport::default())
        .await
        .expect("original selected startup endpoint permits same-profile reopen");
}

#[tokio::test]
async fn genuine_public_app_enrollment_host_storage_refusal_keeps_original_failure() {
    use aura_app::frontend_primitives::WorkflowHandoffSubmission;
    use aura_app::ui::workflows::invitation::{
        import_device_enrollment_with_terminal_status, EnrollmentImportHostPolicy,
    };
    use aura_app::ui_contract::{
        AuthoritativeSemanticFact, OperationId, OperationInstanceId, SemanticOperationKind,
        SemanticOperationPhase,
    };
    use aura_core::{effects::StorageCoreEffects, AuraError};

    let (fixture, _roots) = issued().await;
    let issuer = fixture.issuer().clone();
    let recipient = fixture.recipient().clone();
    let app = fixture.recipient_app();
    aura_core::time::timeout::execute_with_timeout_budget(
        fixture.selected_clock(),
        fixture.original_startup_window(),
        || async {
            aura_app::AppCore::init_signals_with_hooks(&app)
                .await
                .map_err(|source| AuraError::Internal {
                    message: "actual recipient app initialization failed".into(),
                    source: Some(Arc::new(source)),
                })
        },
    )
    .await
    .expect("actual app signal initialization under original selected startup window");
    let admission = app
        .read()
        .await
        .enrollment_import_admission()
        .cloned()
        .expect("actual recipient app admission");
    let transfer = fixture
        .start()
        .manifest_transfer
        .as_ref()
        .expect("original signed transfer");
    // A real file in place of the host directory makes the production storage
    // effect refuse the write. The callback cannot manufacture an ACK.
    let blocked_directory = tempfile::NamedTempFile::new().expect("real blocked host directory");
    let host = Arc::new(aura_effects::FilesystemStorageHandler::new(
        blocked_directory.path().to_path_buf(),
    ));
    let original_storage_source = Arc::new(std::sync::Mutex::new(None));
    let observed_storage_source = original_storage_source.clone();
    let instance = OperationInstanceId("genuine-public-app-host-refusal".into());
    let prior_events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let prior = WorkflowHandoffSubmission::submit_with_instance_unpublished(
        PublicAppSubmission(prior_events.clone()),
        OperationId::device_enrollment(),
        SemanticOperationKind::ImportDeviceEnrollmentCode,
        instance.clone(),
    );
    let admitted = import_device_enrollment_with_terminal_status(
        &app,
        &admission,
        fixture.start().enrollment_code.clone(),
        Some(aura_app::ui_contract::EnrollmentManifestTransferInput {
            manifest_code: transfer.manifest_code.clone(),
            initiator_verifier_code: transfer.initiator_verifier_code.clone(),
        }),
        Some(instance.clone()),
        EnrollmentImportHostPolicy::Finalize(Arc::new(move |completion| {
            let host = host.clone();
            let observed_storage_source = observed_storage_source.clone();
            Box::pin(async move {
                let bytes = serde_json::to_vec(&(
                    completion.subject_authority(),
                    completion.device_id(),
                    completion.invitation_id(),
                    completion.manifest_digest(),
                ))
                .expect("public completion encoding");
                host.store("enrollment-completion", bytes)
                    .await
                    .map_err(|source| {
                        let original = Arc::new(source);
                        *observed_storage_source.lock().unwrap() = Some(original.clone());
                        AuraError::Storage {
                            message: "actual host storage refused enrollment".into(),
                            source: Some(original),
                        }
                    })
            })
        })),
    );
    let mut observation = match admitted {
        Ok(observation) => observation,
        Err(refusal) => {
            let source = refusal.source_error().clone();
            let publication = prior.fail(refusal.semantic_error()).await;
            let reported = fixture.fail(source.clone().into()).await;
            assert!(
                publication.is_ok(),
                "original admission refusal publication failed: {reported}"
            );
            panic!("original app import admission refused: {source}; cleanup: {reported}");
        }
    };
    observation
        .accept_handoff(prior)
        .unwrap_or_else(|_| panic!("original workflow handoff"));
    let (outcome, recovery) = observation.observe_with_host_recovery().await;
    let original = original_storage_source
        .lock()
        .unwrap()
        .clone()
        .expect("actual filesystem refusal");
    let failure = outcome.result.expect_err("host refused actual completion");
    let AuraError::Storage {
        source: Some(source),
        ..
    } = &failure
    else {
        panic!("original host storage failure retained: {failure:?}")
    };
    let original: Arc<dyn Error + Send + Sync> = original;
    assert!(Arc::ptr_eq(source, &original));
    assert_eq!(
        outcome
            .terminal
            .expect("original app terminal")
            .status
            .phase,
        SemanticOperationPhase::Failed
    );
    assert!(prior_events.lock().unwrap().is_empty());
    let terminals: Vec<_> = app
        .read()
        .await
        .authoritative_semantic_facts()
        .iter()
        .filter_map(|fact| match fact {
            AuthoritativeSemanticFact::OperationStatus {
                instance_id,
                status,
                ..
            } if instance_id.as_ref() == Some(&instance)
                && matches!(
                    status.phase,
                    SemanticOperationPhase::Succeeded
                        | SemanticOperationPhase::Failed
                        | SemanticOperationPhase::Cancelled
                ) =>
            {
                Some(status.phase)
            }
            _ => None,
        })
        .collect();
    assert_eq!(terminals, vec![SemanticOperationPhase::Failed]);
    assert!(blocked_directory.path().is_file());
    drop(app);
    let (reported, stopped) = fixture
        .settle_accepted_host_failure(failure.into())
        .await
        .expect("actual accepted resources settle while host failure remains primary");
    let original_storage = original
        .downcast_ref::<aura_core::effects::StorageError>()
        .expect("actual original storage error type");
    let mut cause: Option<&(dyn Error + 'static)> = Some(&reported);
    let mut retained_source = false;
    while let Some(current) = cause {
        if let Some(storage) = current.downcast_ref::<aura_core::effects::StorageError>() {
            retained_source = std::ptr::eq(storage, original_storage);
            break;
        }
        cause = current.source();
    }
    assert!(
        retained_source,
        "original native host storage source survived disposal"
    );
    assert!(issuer.runtime().tasks().active_tasks().is_empty());
    assert!(recipient.runtime().tasks().active_tasks().is_empty());
    let expected_subject = issuer.authority_id();
    drop(admission);
    drop(issuer);
    drop(recipient);
    let reopened = stopped
        .reopen_accepted_for_host_recovery(SharedTransport::default())
        .await
        .expect("same confirmed profiles reopen without turning host failure into success");
    let recovered_app = reopened.recipient_app();
    aura_app::AppCore::init_signals_with_hooks(&recovered_app)
        .await
        .expect("reopened recipient app signals");
    let recovered_admission = recovered_app
        .read()
        .await
        .enrollment_import_admission()
        .cloned()
        .expect("reopened native admission");
    let recovered_root = tempfile::tempdir().expect("new host storage root");
    let recovered_host = Arc::new(aura_effects::FilesystemStorageHandler::new(
        recovered_root.path().to_path_buf(),
    ));
    let committing_host = recovered_host.clone();
    let new_instance = OperationInstanceId("genuine-public-app-host-recovery".into());
    assert_ne!(new_instance, instance);
    let recovered =
        aura_app::ui::workflows::invitation::retry_enrollment_host_with_terminal_status(
            &recovered_app,
            &recovered_admission,
            recovery.expect("original failed host operation issued one recovery candidate"),
            new_instance.clone(),
            EnrollmentImportHostPolicy::Finalize(Arc::new(move |completion| {
                let host = committing_host.clone();
                Box::pin(async move {
                    let bytes = serde_json::to_vec(&(
                        completion.subject_authority(),
                        completion.device_id(),
                        completion.invitation_id(),
                        completion.manifest_digest(),
                    ))
                    .expect("original public completion encoding");
                    host.store("enrollment-completion", bytes)
                        .await
                        .map_err(|source| AuraError::Storage {
                            message: "reopened host storage failed".into(),
                            source: Some(Arc::new(source)),
                        })
                })
            })),
        )
        .await;
    assert_eq!(
        recovered
            .result
            .expect("authenticated new-instance host acknowledgment")
            .subject_authority(),
        expected_subject,
    );
    assert_eq!(
        recovered.terminal.expect("new app terminal").status.phase,
        SemanticOperationPhase::Succeeded,
    );
    assert!(recovered_host
        .retrieve("enrollment-completion")
        .await
        .expect("actual new host storage")
        .is_some());
    drop(recovered_admission);
    drop(recovered_app);
    reopened
        .shutdown()
        .await
        .expect("reopened native owner shutdown");
}

#[tokio::test]
async fn genuine_signed_enrollment_failure_after_acceptance_settles_all_owners() {
    let (fixture, _roots) = issued().await;
    let issuer = fixture.issuer().clone();
    let recipient = fixture.recipient().clone();
    fixture
        .accept_recipient()
        .await
        .expect("actual native acceptance and profile adoption");
    let primary = aura_agent::AgentError::config(
        "selected host finalization refused after native acceptance",
    );
    let expected = primary.to_string();
    let actual = fixture.fail(primary).await;
    let mut current: Option<&(dyn Error + 'static)> = Some(&actual);
    let retained = loop {
        let source = current.expect("retained original primary and cleanup outcomes");
        if let Some(retained) = source.downcast_ref::<EnrollmentLifecycleFailure>() {
            break retained;
        }
        current = source.source();
    };
    assert_eq!(retained.primary().to_string(), expected);
    assert!(
        retained.settled_execution_failure().is_none(),
        "accepted original disposal is not negative failure"
    );
    assert!(
        retained.cleanup_failures().is_empty(),
        "independent cleanup: {:?}",
        retained.cleanup_failures()
    );
    assert!(issuer.runtime().tasks().active_tasks().is_empty());
    assert!(recipient.runtime().tasks().active_tasks().is_empty());
}

#[tokio::test]
async fn genuine_signed_enrollment_pending_cleanup_race_preserves_committed_resources() {
    let (fixture, _roots) = issued().await;
    fixture
        .import_recipient()
        .await
        .expect("actual original import");
    let issuer = fixture.issuer().clone();
    let recipient = fixture.recipient().clone();
    let invitations = recipient
        .invitations()
        .expect("actual service")
        .list_pending()
        .await;
    assert_eq!(invitations.len(), 1, "one actual original invitation");
    let invitation = invitations[0].invitation_id.to_string();
    let (entered, observed) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let cleanup = fixture.fail_after_pending_observation(
        aura_agent::AgentError::config("host failure races original acceptance"),
        entered,
        released,
    );
    tokio::pin!(cleanup);
    tokio::select! {
        result = &mut cleanup => panic!("cleanup finished before pending gate: {result}"),
        result = observed => result.expect("actual Pending observation entered"),
    }
    recipient
        .clone()
        .as_runtime_bridge()
        .accept_invitation(&invitation)
        .await
        .expect("actual commitment wins before cancellation");
    release
        .send(())
        .expect("release SAME original Pending cleanup");
    let actual = cleanup.await;
    let mut current: Option<&(dyn Error + 'static)> = Some(&actual);
    let retained = loop {
        let source = current.expect("retained original cleanup outcome");
        if let Some(retained) = source.downcast_ref::<EnrollmentLifecycleFailure>() {
            break retained;
        }
        current = source.source();
    };
    assert!(
        retained.positive_disposal_acknowledged(),
        "actual original positive disposal validated independently"
    );
    assert!(
        matches!(
            retained.cancellation_refusal(),
            Some(aura_core::AuraError::Invalid { .. })
        ),
        "actual retained canonical cancellation refusal: {:?}",
        retained.cancellation_refusal()
    );
    assert_eq!(
        retained.primary().to_string(),
        aura_agent::AgentError::config("host failure races original acceptance").to_string()
    );
    assert!(
        retained.cleanup_failures().is_empty(),
        "original resources unresolved: {:?}",
        retained.cleanup_failures()
    );
    assert!(issuer.runtime().tasks().active_tasks().is_empty());
    assert!(recipient.runtime().tasks().active_tasks().is_empty());
}
