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
use aura_app::ui_contract::SemanticOperationPhase;
use aura_core::{AuthorityId, ContextId, DeviceId, PhysicalTime, TimeoutBudget};
use aura_testkit::time::ManualPhysicalClock;

type BrokerStageTrace = Arc<std::sync::Mutex<Vec<(u64, String, String)>>>;

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
    issued_with_broker_clock_and_sender(broker, false, Arc::new(ManualPhysicalClock::new(5_000)))
        .await
}

async fn issued_with_broker_clock_and_sender(
    broker: Option<aura_agent::BootstrapBrokerConfig>,
    sender_uses_broker: bool,
    clock: Arc<ManualPhysicalClock>,
) -> (
    SimulatedEnrollment,
    Vec<tempfile::TempDir>,
    Arc<ManualPhysicalClock>,
) {
    issued_with_broker_clock_and_sender_seed(broker, sender_uses_broker, clock, 151).await
}

async fn issued_with_broker_clock_and_sender_seed(
    broker: Option<aura_agent::BootstrapBrokerConfig>,
    sender_uses_broker: bool,
    clock: Arc<ManualPhysicalClock>,
    seed_base: u8,
) -> (
    SimulatedEnrollment,
    Vec<tempfile::TempDir>,
    Arc<ManualPhysicalClock>,
) {
    let roots: Vec<_> = (0..2)
        .map(|_| tempfile::tempdir().expect("isolated native profile"))
        .collect();
    let profiles = std::array::from_fn(|index| {
        let seed = seed_base + index as u8 * 3;
        let config = AgentConfig {
            device_id: DeviceId::new_from_entropy([seed + 1; 32]),
            storage: aura_agent::core::config::StorageConfig {
                base_path: roots[index].path().to_path_buf(),
                ..Default::default()
            },
            bootstrap_broker: if index == 1 || sender_uses_broker {
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
    let startup = TimeoutBudget::from_start_and_timeout(
        &PhysicalTime {
            ts_ms: clock.now_ms(),
            uncertainty: None,
        },
        Duration::from_secs(120),
    )
    .expect("original selected startup window");
    let fixture = if sender_uses_broker {
        SimulatedEnrollment::prepare_native_broker(
            profiles,
            clock.clone(),
            SharedTransport::default(),
            startup,
        )
        .await
    } else {
        SimulatedEnrollment::prepare(profiles, clock.clone(), SharedTransport::default(), startup)
            .await
    }
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

async fn issued_with_original_vm_broker() -> (
    SimulatedEnrollment,
    Vec<tempfile::TempDir>,
    aura_agent::TaskSupervisor,
    String,
    Arc<ManualPhysicalClock>,
) {
    issued_with_original_vm_broker_from(151, 5_000).await
}

async fn issued_with_original_vm_broker_from(
    seed_base: u8,
    selected_time_ms: u64,
) -> (
    SimulatedEnrollment,
    Vec<tempfile::TempDir>,
    aura_agent::TaskSupervisor,
    String,
    Arc<ManualPhysicalClock>,
) {
    use aura_simulator::native_enrollment_fixture::NativeEnrollmentFixture;
    let clock = Arc::new(ManualPhysicalClock::new(selected_time_ms));
    let roots: Vec<_> = (0..2)
        .map(|_| tempfile::tempdir().expect("isolated native profile"))
        .collect();
    let profiles = original_vm_profiles(seed_base, &roots);
    let owner = NativeEnrollmentFixture::prepare(profiles, clock, "genuine-complete-vm-auth")
        .await
        .expect("original signed native broker issuance");
    let url = owner.broker_url().to_owned();
    (owner.enrollment, roots, owner.supervisor, url, owner.clock)
}

fn original_vm_profiles(
    seed_base: u8,
    roots: &[tempfile::TempDir],
) -> [EnrollmentParticipantProfile; 2] {
    std::array::from_fn(|index| {
        let seed = seed_base + index as u8 * 3;
        EnrollmentParticipantProfile {
            config: AgentConfig {
                device_id: DeviceId::new_from_entropy([seed + 1; 32]),
                storage: aura_agent::core::config::StorageConfig {
                    base_path: roots[index].path().to_path_buf(),
                    ..Default::default()
                },
                ..Default::default()
            },
            authority: AuthorityId::new_from_entropy([seed; 32]),
            context: ContextId::new_from_entropy([seed + 2; 32]),
            seed: u64::from(seed),
        }
    })
}

async fn original_vm_broker_progress(
    broker_url: &str,
    subject: AuthorityId,
    device: DeviceId,
    digest: [u8; 32],
) -> serde_json::Value {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let authority = broker_url
        .strip_prefix("http://")
        .expect("test broker uses explicit loopback HTTP")
        .trim_end_matches('/');
    let mut stream = tokio::net::TcpStream::connect(authority)
        .await
        .expect("actual broker progress connection");
    let path = format!(
        "/v2/bootstrap/enrollment-mailbox/progress-witness/{subject}/{device}/{}",
        hex::encode(digest)
    );
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer genuine-complete-vm-auth\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("private authenticated broker query");
    let mut response = Vec::new();
    (&mut stream)
        .take(16 * 1024 + 1)
        .read_to_end(&mut response)
        .await
        .expect("bounded broker progress response");
    assert!(
        response.len() < 16 * 1024,
        "broker progress response is bounded"
    );
    let divider = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("broker progress HTTP divider");
    assert!(
        response.starts_with(b"HTTP/1.1 200 "),
        "broker progress is authorized"
    );
    serde_json::from_slice(&response[divider + 4..]).expect("typed broker progress JSON")
}

#[cfg(test)]
async fn drive_brokered_import_until<T, M, F>(
    fixture: &SimulatedEnrollment,
    clock: &Arc<ManualPhysicalClock>,
    broker_url: &str,
    maintenance_pass: M,
    goal: impl std::future::Future<Output = T>,
) -> T
where
    M: FnMut() -> F,
    F: std::future::Future<Output = Result<(), aura_core::AuraError>>,
{
    drive_brokered_import_until_traced(
        fixture,
        clock,
        broker_url,
        maintenance_pass,
        goal,
        None,
        false,
    )
    .await
}

async fn drive_brokered_import_until_traced<T, M, F>(
    fixture: &SimulatedEnrollment,
    clock: &Arc<ManualPhysicalClock>,
    broker_url: &str,
    mut maintenance_pass: M,
    goal: impl std::future::Future<Output = T>,
    trace: Option<&BrokerStageTrace>,
    finish_inflight_maintenance: bool,
) -> T
where
    M: FnMut() -> F,
    F: std::future::Future<Output = Result<(), aura_core::AuraError>>,
{
    let mut goal = Box::pin(goal);
    let mut maintenance = Box::pin(maintenance_pass());
    let (subject, device, digest) = fixture
        .original_mailbox_selector()
        .expect("retained exact original mailbox selector");
    let mut first_frame_queued = false;
    let issuer = fixture.issuer().clone();
    let mut issuer_drain = Some(Box::pin(issuer.drain_issuer_return_vm_mailbox()));
    let mut drain_tick = tokio::time::interval(Duration::from_millis(100));
    let mut tick = tokio::time::interval(Duration::from_millis(10));
    let result = aura_testkit::infrastructure::time::assertions::assert_completes_within(
        Duration::from_secs(30),
        async {
        loop {
            tokio::select! {
                outcome = &mut goal => {
                    if finish_inflight_maintenance {
                        fixture
                            .original_startup_window()
                            .remaining_at(&PhysicalTime {
                                ts_ms: clock.now_ms(),
                                uncertainty: None,
                            })
                            .expect("in-flight maintenance remains in original selected window");
                        let upkeep = aura_testkit::infrastructure::time::assertions::assert_completes_within(
                            Duration::from_secs(5),
                            async { Ok::<_, aura_core::AuraError>((&mut maintenance).await) },
                        )
                        .await
                        .expect("original in-flight mailbox maintenance must settle");
                        if let Err(error) = upkeep {
                            panic!(
                                "original in-flight mailbox maintenance failed with typed kind {:?}",
                                std::mem::discriminant(&error)
                            );
                        }
                    }
                    break Ok(outcome);
                },
                upkeep = &mut maintenance => {
                    assert!(upkeep.is_ok() || matches!(&upkeep, Err(aura_core::AuraError::Internal { .. })),
                        "independent mailbox upkeep preserves typed sync failure: {upkeep:?}");
                    maintenance = Box::pin(maintenance_pass());
                }
                _outcome = async {
                    match issuer_drain.as_mut() {
                        Some(drain) => drain.await,
                        None => std::future::pending().await,
                    }
                } => {
                    issuer_drain = None;
                }
                _ = drain_tick.tick() => {
                    if issuer_drain.is_none() {
                        issuer_drain = Some(Box::pin(issuer.drain_issuer_return_vm_mailbox()));
                    }
                }
                _ = tick.tick() => {
                    if !first_frame_queued || trace.is_some() {
                        let progress = original_vm_broker_progress(broker_url, subject, device, digest).await;
                        if let Some(trace) = trace {
                            let stage = progress["messages"][0]["stage"]
                                .as_str()
                                .unwrap_or("no_message")
                                .to_string();
                            let message_id = progress["messages"][0]["message_id"]
                                .as_str()
                                .unwrap_or("")
                                .to_owned();
                            let observation = (clock.now_ms(), message_id, stage);
                            let mut trace = trace.lock().unwrap();
                            if trace.last() != Some(&observation) {
                                trace.push(observation);
                            }
                        }
                        first_frame_queued |= progress["messages"].as_array().is_some_and(|messages| !messages.is_empty());
                    }
                    aura_simulator::native_enrollment_fixture::advance_selected_clock_before_frame(
                        fixture, clock, first_frame_queued,
                    ).await;
                }
            }
        }
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            let (subject, device, digest) = fixture
                .original_mailbox_selector()
                .expect("retained original signed mailbox selector");
            let progress = original_vm_broker_progress(broker_url, subject, device, digest).await;
            panic!("original signed broker VM/host timeout: registration={}, count={}, stage={}, clock={}, next={:?}",
                progress["registration_present"] == true,
                progress["messages"].as_array().map_or(0, Vec::len),
                progress["messages"][0]["stage"].as_str().unwrap_or("absent"),
                clock.now_ms(),
                clock.next_registered_deadline().await.map(|d| d.value()));
        }
    }
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
    let (fixture, _roots, broker_supervisor, broker_url, clock) =
        issued_with_original_vm_broker().await;
    eprintln!("[genuine-app-import] issued ready");
    let app = fixture.recipient_app();
    let host_root = tempfile::tempdir().expect("isolated real host storage");
    let host = Arc::new(aura_effects::FilesystemStorageHandler::new(
        host_root.path().to_path_buf(),
    ));
    let instance = OperationInstanceId("genuine-public-app-enrollment".into());
    let mut refused_publication = None;
    let result =
        async {
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
            let bridge = fixture.recipient().clone().as_runtime_bridge();
            let outcome = drive_brokered_import_until(
                &fixture,
                &clock,
                &broker_url,
                || {
                    aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(
                        &app, &bridge,
                    )
                },
                observation,
            )
            .await;
            eprintln!(
                "[genuine-app-import] original observation ready: {:?}",
                outcome.terminal
            );
            let completion = outcome.result?;
            let progress = original_vm_broker_progress(
                &broker_url,
                completion.subject_authority(),
                completion.device_id(),
                completion.manifest_digest(),
            )
            .await;
            if progress["subject"] != completion.subject_authority().to_string()
                || progress["device"] != completion.device_id().to_string()
                || progress["manifest_digest"] != hex::encode(completion.manifest_digest())
                || progress["registration_present"] != true
                || progress["messages"]
                    .as_array()
                    .is_none_or(|messages| messages.is_empty() || messages.len() > 4)
                || progress["messages"][0]["stage"] != "receiver_acknowledged_retained_frame"
            {
                return Err(AuraError::invalid(format!(
                "exact first broker receiver ACK is absent: registration={}, count={}, stage={}",
                progress["registration_present"] == true,
                progress["messages"].as_array().map_or(0, Vec::len),
                progress["messages"][0]["stage"].as_str().unwrap_or("absent")
            )));
            }
            let message_id = progress["messages"][0]["message_id"]
                .as_str()
                .ok_or_else(|| AuraError::invalid("original broker ACK message ID absent"))?;
            uuid::Uuid::parse_str(message_id)
                .map_err(|_| AuraError::invalid("original broker ACK message ID malformed"))?;
            let digest = progress["messages"][0]["envelope_digest"]
                .as_str()
                .ok_or_else(|| AuraError::invalid("original broker ACK envelope digest absent"))?;
            if hex::decode(digest).map_or(true, |bytes| bytes.len() != 32) {
                return Err(AuraError::invalid(
                    "original broker ACK envelope digest malformed",
                ));
            }
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
    broker_supervisor
        .shutdown_with_timeout(Duration::from_secs(5))
        .await
        .expect("original VM broker disposes after exact ACK");
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

    let (fixture, _roots, broker_supervisor, broker_url, clock) =
        issued_with_original_vm_broker().await;
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
        EnrollmentImportHostPolicy::Finalize(Arc::new(move |completion| {
            let entered = entered
                .lock()
                .expect("original host callback gate")
                .take()
                .expect("one original host invocation");
            let selector = (
                completion.subject_authority(),
                completion.device_id(),
                completion.manifest_digest(),
            );
            Box::pin(async move {
                entered
                    .send(selector)
                    .expect("original host callback observed");
                futures::future::pending::<Result<(), AuraError>>().await
            })
        })),
    )
    .unwrap_or_else(|refusal| panic!("actual app import admission: {}", refusal.source_error()));
    observation
        .accept_handoff(prior)
        .unwrap_or_else(|_| panic!("original workflow handoff"));
    let mut observation = Box::pin(observation);
    let bridge = fixture.recipient().clone().as_runtime_bridge();
    let host_selector = drive_brokered_import_until(
        &fixture,
        &clock,
        &broker_url,
        || aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(&app, &bridge),
        async {
            tokio::select! {
                result = observed => result.ok(),
                _ = &mut observation => None,
            }
        },
    )
    .await;
    if let Some((subject, device, digest)) = host_selector {
        let progress = original_vm_broker_progress(&broker_url, subject, device, digest).await;
        assert_eq!(progress["registration_present"], true);
        assert_eq!(
            progress["messages"][0]["stage"],
            "receiver_acknowledged_retained_frame"
        );
    }
    drop(observation);
    drop(bridge);
    let reported = fixture
        .fail(aura_agent::AgentError::config(
            "original app import observer cancelled before host acknowledgment",
        ))
        .await;
    assert!(
        host_selector.is_some(),
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
    broker_supervisor
        .shutdown_with_timeout(Duration::from_secs(5))
        .await
        .expect("original VM broker disposes after cancelled host observer");
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

    let (fixture, _roots, broker_supervisor, broker_url, clock) =
        issued_with_original_vm_broker().await;
    let driver_clock = clock.clone();
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
    let host_selector = Arc::new(std::sync::Mutex::new(None));
    let observed_selector = host_selector.clone();
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
        EnrollmentImportHostPolicy::Prepare(Arc::new(move |completion| {
            *observed_selector.lock().unwrap() = Some((
                completion.subject_authority(),
                completion.device_id(),
                completion.manifest_digest(),
            ));
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
    let bridge = fixture.recipient().clone().as_runtime_bridge();
    let outcome = drive_brokered_import_until(
        &fixture,
        &driver_clock,
        &broker_url,
        || aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(&app, &bridge),
        observation,
    )
    .await;
    let (subject, device, digest) = host_selector
        .lock()
        .unwrap()
        .expect("selected provider refusal follows original completion");
    let progress = original_vm_broker_progress(&broker_url, subject, device, digest).await;
    assert_eq!(progress["registration_present"], true);
    assert_eq!(
        progress["messages"][0]["stage"],
        "receiver_acknowledged_retained_frame"
    );
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
    drop(bridge);
    drop(app);
    let (_reported, stopped) = fixture
        .settle_accepted_host_failure(failure.into())
        .await
        .expect("provider refusal and original native completion settle separately");
    broker_supervisor
        .shutdown_with_timeout(Duration::from_secs(5))
        .await
        .expect("original VM broker disposes after selected provider refusal");
    drop(admission);
    stopped
        .reopen_and_shutdown(SharedTransport::default())
        .await
        .expect("original selected startup endpoint permits same-profile reopen");
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HostRefusalReplayInputs {
    seed_base: u8,
    selected_physical_ms: u64,
    operation_instance: &'static str,
    owner: &'static str,
    injected_fault: &'static str,
}

impl Default for HostRefusalReplayInputs {
    fn default() -> Self {
        Self {
            seed_base: 151,
            selected_physical_ms: 5_000,
            operation_instance: "genuine-public-app-host-refusal",
            owner: "WorkflowHandoffSubmission/ImportDeviceEnrollmentCode",
            injected_fault: "HostPathIsFile",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct HostRefusalReplayReceipt {
    inputs: HostRefusalReplayInputs,
    /// Ordered authoritative milestones; polling may skip transient stages.
    broker_milestones: Vec<&'static str>,
    broker_registration: bool,
    broker_first_envelope: bool,
    broker_receiver_ack: bool,
    typed_terminal: SemanticOperationPhase,
    typed_host_failure: &'static str,
    same_profile_reopened: bool,
}

async fn run_host_storage_refusal_from_fresh_profile(
    inputs: HostRefusalReplayInputs,
) -> HostRefusalReplayReceipt {
    run_host_storage_refusal_with_profiles(inputs, None, None, |_| {}).await
}

async fn run_host_storage_refusal_with_profiles<F>(
    inputs: HostRefusalReplayInputs,
    original_roots: Option<&[tempfile::TempDir]>,
    nonce_retirements: Option<[Arc<dyn aura_core::crypto::tree_signing::FrostNonceRetirement>; 2]>,
    on_selected_owners: F,
) -> HostRefusalReplayReceipt
where
    F: FnOnce(&SimulatedEnrollment),
{
    use aura_app::frontend_primitives::WorkflowHandoffSubmission;
    use aura_app::ui::workflows::invitation::{
        import_device_enrollment_with_terminal_status, EnrollmentImportHostPolicy,
    };
    use aura_app::ui_contract::{
        AuthoritativeSemanticFact, OperationId, OperationInstanceId, SemanticOperationKind,
        SemanticOperationPhase,
    };
    use aura_core::{effects::StorageCoreEffects, AuraError};

    assert!(
        original_roots.is_some() || nonce_retirements.is_none(),
        "retirement scope requires the exact original profile roots"
    );

    let (fixture, _roots, broker_supervisor, broker_url, clock) = if let Some(roots) =
        original_roots
    {
        use aura_simulator::native_enrollment_fixture::NativeEnrollmentFixture;
        let clock = Arc::new(ManualPhysicalClock::new(inputs.selected_physical_ms));
        let profiles = original_vm_profiles(inputs.seed_base, roots);
        let owner = match nonce_retirements {
            #[cfg(feature = "simulator-checkpoint")]
            Some(retirements) => {
                NativeEnrollmentFixture::prepare_with_nonce_retirement(
                    profiles,
                    clock,
                    "genuine-complete-vm-auth",
                    retirements,
                )
                .await
            }
            #[cfg(not(feature = "simulator-checkpoint"))]
            Some(_) => panic!("simulator retirement feature is absent"),
            None => {
                NativeEnrollmentFixture::prepare(profiles, clock, "genuine-complete-vm-auth").await
            }
        }
        .expect("original signed native broker issuance from selected profiles");
        let url = owner.broker_url().to_owned();
        (
            owner.enrollment,
            Vec::new(),
            owner.supervisor,
            url,
            owner.clock,
        )
    } else {
        issued_with_original_vm_broker_from(inputs.seed_base, inputs.selected_physical_ms).await
    };
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
    let host_selector = Arc::new(std::sync::Mutex::new(None));
    let observed_selector = host_selector.clone();
    let instance = OperationInstanceId(inputs.operation_instance.into());
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
            *observed_selector.lock().unwrap() = Some((
                completion.subject_authority(),
                completion.device_id(),
                completion.manifest_digest(),
            ));
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
    let bridge = fixture.recipient().clone().as_runtime_bridge();
    let stage_trace = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (outcome, recovery) = drive_brokered_import_until_traced(
        &fixture,
        &clock,
        &broker_url,
        || aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(&app, &bridge),
        observation.observe_with_host_recovery(),
        Some(&stage_trace),
        true,
    )
    .await;
    let (subject, device, digest) = host_selector
        .lock()
        .unwrap()
        .expect("actual host refusal follows original verified completion");
    let first_message_id = stage_trace
        .lock()
        .unwrap()
        .iter()
        .find_map(|(_, message_id, _)| (!message_id.is_empty()).then_some(message_id.clone()))
        .expect("original first broker message ID was observed");
    let post_terminal_progress =
        aura_testkit::infrastructure::time::assertions::assert_completes_within(
            Duration::from_secs(5),
            async {
                let mut tick = tokio::time::interval(Duration::from_millis(100));
                loop {
                    fixture
                        .original_startup_window()
                        .remaining_at(&PhysicalTime {
                            ts_ms: clock.now_ms(),
                            uncertainty: None,
                        })
                        .expect(
                            "post-terminal ACK stays within original selected startup deadline",
                        );
                    let progress =
                        original_vm_broker_progress(&broker_url, subject, device, digest).await;
                    if progress["messages"].as_array().is_some_and(|messages| {
                        messages.iter().any(|message| {
                            message["message_id"] == first_message_id
                                && message["stage"] == "receiver_acknowledged_retained_frame"
                        })
                    }) {
                        break Ok(progress);
                    }
                    tick.tick().await;
                    let upkeep =
                        aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(
                            &app, &bridge,
                        )
                        .await;
                    if let Err(error) = upkeep {
                        panic!(
                            "post-terminal original mailbox upkeep failed with typed kind {:?}",
                            std::mem::discriminant(&error)
                        );
                    }
                }
            },
        )
        .await;
    let progress = match post_terminal_progress {
        Ok(progress) => progress,
        Err(_) => {
            let progress = original_vm_broker_progress(&broker_url, subject, device, digest).await;
            let stages: Vec<_> = progress["messages"]
                .as_array()
                .expect("typed broker message inventory")
                .iter()
                .map(|message| {
                    (
                        message["message_id"].as_str().unwrap_or("missing-id"),
                        message["stage"].as_str().unwrap_or("missing-stage"),
                    )
                })
                .collect();
            panic!("original receiver ACK missing after bounded upkeep: {stages:?}");
        }
    };
    assert_eq!(progress["registration_present"], true);
    assert!(progress["messages"].as_array().is_some_and(|messages| {
        messages.iter().any(|message| {
            message["message_id"] == first_message_id
                && message["stage"] == "receiver_acknowledged_retained_frame"
        })
    }));
    let broker_milestones = {
        let stages = stage_trace.lock().unwrap();
        assert!(
            stages.iter().any(|(_, _, stage)| stage != "no_message"),
            "actual broker first-envelope stage was observed"
        );
        assert!(stages.iter().any(|(_, id, _)| id == &first_message_id));
        [
            "registration_present",
            "original_message_id_observed",
            "receiver_acknowledged_retained_frame",
        ]
        .to_vec()
    };
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
    on_selected_owners(&fixture);
    drop(bridge);
    drop(app);
    let (reported, stopped) = fixture
        .settle_accepted_host_failure(failure.into())
        .await
        .expect("actual accepted resources settle while host failure remains primary");
    broker_supervisor
        .shutdown_with_timeout(Duration::from_secs(5))
        .await
        .expect("original VM broker disposes after typed host refusal");
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
    HostRefusalReplayReceipt {
        inputs,
        broker_milestones,
        broker_registration: progress["registration_present"] == true,
        broker_first_envelope: progress["messages"]
            .as_array()
            .is_some_and(|messages| !messages.is_empty()),
        broker_receiver_ack: progress["messages"][0]["stage"]
            == "receiver_acknowledged_retained_frame",
        typed_terminal: SemanticOperationPhase::Failed,
        typed_host_failure: "Storage",
        same_profile_reopened: true,
    }
}

#[cfg(all(feature = "simulator-checkpoint", unix))]
struct ProtectedNativeEnrollmentReplay {
    roots: Vec<tempfile::TempDir>,
    _artifacts: tempfile::TempDir,
    checkpoints: [aura_effects::profile_checkpoint::QuiescentFilesystemProfileCheckpoint; 2],
    revision: [u8; 32],
    participants: [[u8; 32]; 2],
    configurations: [[u8; 32]; 2],
    checkpoint_bindings: [[u8; 32]; 2],
}

#[cfg(all(feature = "simulator-checkpoint", unix))]
impl ProtectedNativeEnrollmentReplay {
    fn binding(profile: &EnrollmentParticipantProfile) -> ([u8; 32], [u8; 32]) {
        let participant =
            aura_core::Hash32::from_value(&(profile.authority, profile.config.device_id))
                .expect("canonical participant binding")
                .0;
        // Broker registration/queue is empty at this pre-issuance boundary.
        // Each branch binds the same loopback policy and receives a separately
        // recorded ephemeral port when its fresh broker owner starts.
        let configuration = aura_core::Hash32::from_value(&(
            profile.authority,
            profile.config.device_id,
            profile.context,
            profile.seed,
            "LoopbackOnly/OriginalIssuerAndRecipient",
            // Configuration requests PlatformCredentialStore, but the
            // nonproduction runtime selects FilesystemFallback.
            "FilesystemFallback/runtime-selected",
        ))
        .expect("canonical stable fixture configuration")
        .0;
        (participant, configuration)
    }

    async fn capture(inputs: &HostRefusalReplayInputs) -> Self {
        use aura_effects::profile_checkpoint::QuiescentFilesystemProfileCheckpoint;
        let roots: Vec<_> = (0..2)
            .map(|_| tempfile::tempdir().expect("isolated original profile"))
            .collect();
        use std::os::unix::fs::PermissionsExt;
        for root in &roots {
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
                .expect("private original profile root");
        }
        let profiles = original_vm_profiles(inputs.seed_base, &roots);
        let clock = Arc::new(ManualPhysicalClock::new(inputs.selected_physical_ms));
        let startup = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: clock.now_ms(),
                uncertainty: None,
            },
            Duration::from_secs(120),
        )
        .expect("original selected startup deadline");
        let selected = SimulatedEnrollment::prepare_quiescent_native_signing_profiles(
            &profiles, clock, &startup,
        )
        .await
        .expect("original selected owners stop before issuance");
        let artifacts = tempfile::tempdir().expect("private protected artifacts");
        std::fs::set_permissions(artifacts.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private checkpoint artifact directory");
        // The source-bound test artifact identifies this compiled fixture.
        // A fixed-commit manifest independently binds the complete repository.
        let revision = aura_core::Hash32::from_bytes(include_bytes!("enrollment_lifecycle.rs")).0;
        let bindings = profiles.each_ref().map(Self::binding);
        let participants = bindings.map(|(participant, _)| participant);
        let configurations = profiles.each_ref().map(|profile| Self::binding(profile).1);
        let checkpoint_bindings = [0, 1].map(|index| {
            aura_core::Hash32::from_value(&(participants[index], 0_u64, configurations[index]))
                .expect("canonical original checkpoint binding")
                .0
        });
        let [issuer_selected, recipient_selected] = selected;
        #[cfg(unix)]
        for root in &roots {
            use std::os::unix::fs::PermissionsExt;
            for entry in [root.path().to_path_buf(), root.path().join("secure_store")] {
                let mode = std::fs::metadata(&entry)
                    .expect("selected profile directory exists")
                    .permissions()
                    .mode()
                    & 0o777;
                assert_eq!(
                    mode & 0o077,
                    0,
                    "selected profile directory must be private: {entry:?} mode {mode:o}"
                );
            }
        }
        let checkpoints = [issuer_selected, recipient_selected]
            .into_iter()
            .enumerate()
            .map(|(index, source)| {
                QuiescentFilesystemProfileCheckpoint::capture(
                    source,
                    roots[index].path(),
                    &artifacts.path().join(format!("profile-{index}.bin")),
                    revision,
                    checkpoint_bindings[index],
                )
                .expect("effect-owned selected profile capture")
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap_or_else(|_| panic!("exact original checkpoint inventory"));
        Self {
            roots,
            _artifacts: artifacts,
            checkpoints,
            revision,
            participants,
            configurations,
            checkpoint_bindings,
        }
    }

    async fn restore(&self) {
        // The two original profiles have disjoint roots and independent
        // exclusive leases. Restore both under fresh effect owners while
        // preserving each owner's full pre-mutation inventory validation.
        let issuer = self.checkpoints[0].restore(
            self.roots[0].path(),
            self.revision,
            self.checkpoint_bindings[0],
        );
        let recipient = self.checkpoints[1].restore(
            self.roots[1].path(),
            self.revision,
            self.checkpoint_bindings[1],
        );
        let (issuer, recipient) = tokio::join!(issuer, recipient);
        issuer.expect("fresh issuer effect owner restores original selected filesystem profile");
        recipient
            .expect("fresh recipient effect owner restores original selected filesystem profile");
    }
}

#[cfg(all(feature = "simulator-checkpoint", unix))]
#[tokio::test]
async fn original_signed_host_refusal_replays_from_protected_selected_profiles() {
    use aura_simulator::simulator_nonce_retirement::SimulatorNonceRetirementLedger;
    let inputs = HostRefusalReplayInputs::default();
    let protected = ProtectedNativeEnrollmentReplay::capture(&inputs).await;
    let retirement = Arc::new(SimulatorNonceRetirementLedger::default());
    let branch_retirements =
        |branch| [0, 1].map(|index| retirement.scoped(protected.participants[index], 0, branch));
    let captured = run_host_storage_refusal_with_profiles(
        inputs.clone(),
        Some(&protected.roots),
        Some(branch_retirements(1)),
        |_| {},
    )
    .await;
    assert!(captured.broker_registration && captured.broker_receiver_ack);
    protected.restore().await;
    let replayed = run_host_storage_refusal_with_profiles(
        inputs,
        Some(&protected.roots),
        Some(branch_retirements(2)),
        |_| {},
    )
    .await;
    assert_eq!(
        captured, replayed,
        "same typed app/host/ACK outcome after restore"
    );
}

#[cfg(all(feature = "simulator-checkpoint", unix))]
#[tokio::test]
async fn protected_original_broker_delivery_fault_retains_cause_and_cancels() {
    use aura_app::frontend_primitives::WorkflowHandoffSubmission;
    use aura_app::ui::workflows::invitation::{
        import_device_enrollment_with_terminal_status, EnrollmentImportHostPolicy,
    };
    use aura_app::ui_contract::{
        AuthoritativeSemanticFact, OperationId, OperationInstanceId, SemanticOperationKind,
        SemanticOperationPhase,
    };
    use aura_core::AuraError;
    use aura_simulator::native_enrollment_fixture::NativeEnrollmentFixture;
    use aura_simulator::simulator_nonce_retirement::SimulatorNonceRetirementLedger;

    let inputs = HostRefusalReplayInputs {
        injected_fault: "BrokerOwnerShutdownAfterFirstQueue",
        operation_instance: "genuine-public-app-broker-delivery-fault",
        ..Default::default()
    };
    let protected = ProtectedNativeEnrollmentReplay::capture(&inputs).await;
    let retirement = Arc::new(SimulatorNonceRetirementLedger::default());
    let mut receipts = Vec::new();
    for branch in 0..2 {
        protected.restore().await;
        let clock = Arc::new(ManualPhysicalClock::new(inputs.selected_physical_ms));
        let mut owner = NativeEnrollmentFixture::prepare_with_nonce_retirement(
            original_vm_profiles(inputs.seed_base, &protected.roots),
            clock.clone(),
            "genuine-complete-vm-auth",
            [0, 1].map(|index| retirement.scoped(protected.participants[index], 0, branch)),
        )
        .await
        .expect("original signed VM and broker registration");
        let broker_url = owner.broker_url().to_owned();
        let fixture = &owner.enrollment;
        let issuer = fixture.issuer().clone();
        let recipient = fixture.recipient().clone();
        let app = fixture.recipient_app();
        aura_app::AppCore::init_signals_with_hooks(&app)
            .await
            .expect("original recipient app signals");
        let admission = app
            .read()
            .await
            .enrollment_import_admission()
            .cloned()
            .expect("original app admission");
        let transfer = fixture
            .start()
            .manifest_transfer
            .as_ref()
            .expect("original signed transfer");
        let host_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed_host = host_called.clone();
        let instance = OperationInstanceId(inputs.operation_instance.into());
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
            EnrollmentImportHostPolicy::Finalize(Arc::new(move |_| {
                observed_host.store(true, std::sync::atomic::Ordering::SeqCst);
                Box::pin(async { Err(AuraError::invalid("withheld delivery reached host")) })
            })),
        )
        .unwrap_or_else(|refusal| panic!("original import admission: {}", refusal.source_error()));
        observation
            .accept_handoff(prior)
            .unwrap_or_else(|_| panic!("original workflow handoff"));
        let (subject, device, digest) = fixture
            .original_mailbox_selector()
            .expect("exact signed mailbox selector");
        let mut issuer_drain = Box::pin(issuer.drain_issuer_return_vm_mailbox());
        let (message_id, stage) = aura_testkit::infrastructure::time::assertions::assert_completes_within(Duration::from_secs(10), async {
            let mut tick = tokio::time::interval(Duration::from_millis(10));
            loop {
                tokio::select! {
                    drain = &mut issuer_drain => {
                        drain.expect("original issuer registration and send drain");
                        issuer_drain = Box::pin(issuer.drain_issuer_return_vm_mailbox());
                    }
                    _ = tick.tick() => {
                        let progress = original_vm_broker_progress(&broker_url, subject, device, digest).await;
                        if let Some(message) = progress["messages"].as_array().and_then(|messages| messages.first()) {
                            assert_eq!(progress["registration_present"], true);
                            let id = message["message_id"].as_str().expect("exact first ID").to_owned();
                            let stage = message["stage"].as_str().expect("typed first stage").to_owned();
                            break Ok((id, stage));
                        }
                        aura_simulator::native_enrollment_fixture::advance_selected_clock_before_frame(&fixture, &clock, false).await;
                    }
                }
            }
        })
        .await
        .expect("original first envelope queues before delivery fault");
        assert_eq!(
            stage, "queued",
            "fault follows exact first queue before retrieval"
        );
        assert!(!host_called.load(std::sync::atomic::Ordering::SeqCst));
        owner
            .stop_original_broker_for_delivery_fault()
            .await
            .expect("named original broker-owner delivery fault closes listener");
        let bridge = recipient.clone().as_runtime_bridge();
        let delivery_error =
            aura_testkit::infrastructure::time::assertions::assert_completes_within(
                Duration::from_secs(5),
                async {
                    Ok::<_, AuraError>(
                        aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(
                            &app, &bridge,
                        )
                        .await,
                    )
                },
            )
            .await
            .expect("original receiver maintenance reports broker fault")
            .expect_err("original receiver must refuse unavailable broker delivery");
        let mut cause: Option<&(dyn Error + 'static)> = Some(&delivery_error);
        let mut broker_connect = false;
        let mut refused_connection = false;
        while let Some(current) = cause {
            if let Some(AuraError::Network { message, .. }) = current.downcast_ref::<AuraError>() {
                broker_connect |= message == "bootstrap broker connect failed";
            }
            if let Some(io) = current.downcast_ref::<std::io::Error>() {
                refused_connection |= io.kind() == std::io::ErrorKind::ConnectionRefused;
            }
            cause = current.source();
        }
        assert!(broker_connect && refused_connection, "original typed broker connection refusal survives receiver maintenance: broker_connect={broker_connect} refused_connection={refused_connection} top={:?}", std::mem::discriminant(&delivery_error));
        let delivery_kind = match delivery_error {
            AuraError::Network { .. } => "Network",
            AuraError::Internal { .. } => "Internal",
            other => panic!(
                "original receiver reported unexpected typed broker failure {:?}",
                std::mem::discriminant(&other)
            ),
        };
        drop(bridge);
        // Dropping the accepted observer requests cancellation through its
        // real app owner; the host callback must never acknowledge this fault.
        drop(observation);
        aura_testkit::infrastructure::time::assertions::assert_completes_within(
            Duration::from_secs(5),
            async {
                loop {
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
                    if !terminals.is_empty() {
                        assert_eq!(terminals, vec![SemanticOperationPhase::Cancelled]);
                        break Ok::<(), AuraError>(());
                    }
                    tokio::task::yield_now().await;
                }
            },
        )
        .await
        .expect("original app cancellation reaches typed terminal");
        assert!(!message_id.is_empty());
        assert!(!host_called.load(std::sync::atomic::Ordering::SeqCst));
        assert!(prior_events.lock().unwrap().is_empty());
        drop(admission);
        drop(app);
        let _reported = owner
            .enrollment
            .fail(aura_agent::AgentError::config(
                "original broker delivery failed and app cancelled",
            ))
            .await;
        assert!(issuer.runtime().tasks().active_tasks().is_empty());
        assert!(recipient.runtime().tasks().active_tasks().is_empty());
        drop(issuer_drain);
        drop(issuer);
        drop(recipient);
        receipts.push((
            inputs.injected_fault,
            vec![
                "registration_present",
                "first_envelope_queued",
                "broker_owner_stopped_before_receiver_ack",
            ],
            SemanticOperationPhase::Cancelled,
            delivery_kind,
        ));
    }
    assert_eq!(
        receipts[0], receipts[1],
        "same typed cancelled replay milestones"
    );
    protected.restore().await;
    let reopened = NativeEnrollmentFixture::prepare(
        original_vm_profiles(inputs.seed_base, &protected.roots),
        Arc::new(ManualPhysicalClock::new(inputs.selected_physical_ms)),
        "genuine-complete-vm-auth",
    )
    .await
    .expect("same original profiles reopen under fresh signed runtime owners");
    reopened
        .enrollment
        .fail(aura_agent::AgentError::config("reopened owner disposition"))
        .await;
    reopened
        .supervisor
        .shutdown_with_timeout(Duration::from_secs(5))
        .await
        .expect("reopened broker owner acknowledges shutdown");

    // The retained receipt is stage-only. Neither the signed transfer nor
    // broker credentials, envelope bytes, and native error text are exported.
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let artifact_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts");
    if !artifact_root.exists() {
        std::fs::create_dir(&artifact_root).expect("private receipt artifact root");
    }
    let metadata = std::fs::symlink_metadata(&artifact_root).expect("receipt artifact root");
    assert!(metadata.file_type().is_dir() && !metadata.file_type().is_symlink());
    let receipt_dir = tempfile::Builder::new()
        .prefix("aura-task449-delivery-")
        .tempdir_in(&artifact_root)
        .expect("new protected delivery receipt directory");
    std::fs::set_permissions(receipt_dir.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private delivery receipt directory");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(receipt_dir.path().join("replay.json"))
        .expect("new owner-only delivery replay receipt");
    let redacted = serde_json::json!({
        "fixture_source_digest": protected.revision.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "boundary": "quiescent-pre-issuance-empty-broker",
        "selected_provider": "runtime-owned-filesystem-fallback",
        "seed_base": inputs.seed_base,
        "selected_physical_ms": inputs.selected_physical_ms,
        "operation_instance": inputs.operation_instance,
        "owner": inputs.owner,
        "injected_fault": inputs.injected_fault,
        "broker_milestones": receipts[0].1,
        "typed_native_cause": "BrokerNetwork/ConnectionRefused",
        "typed_app_terminal": "Cancelled",
        "same_profile_reopened": true,
        "compared_branches": receipts.len(),
        "network_delivery_asserted": false,
    });
    serde_json::to_writer_pretty(&mut file, &redacted).expect("redacted delivery receipt");
    file.flush().expect("flush delivery receipt");
    file.sync_all().expect("durable delivery receipt");
    let retained = receipt_dir.keep();
    println!(
        "task449 protected delivery receipt: {}",
        retained.join("replay.json").display()
    );
}

#[cfg(all(feature = "simulator-checkpoint", unix))]
#[tokio::test]
async fn restored_original_agent_issues_new_normal_signed_enrollment() {
    use aura_simulator::simulator_nonce_retirement::SimulatorNonceRetirementLedger;
    let inputs = HostRefusalReplayInputs::default();
    let protected = ProtectedNativeEnrollmentReplay::capture(&inputs).await;
    protected.restore().await;
    let retirement = Arc::new(SimulatorNonceRetirementLedger::default());
    let receipt = run_host_storage_refusal_with_profiles(
        inputs,
        Some(&protected.roots),
        Some([0, 1].map(|index| retirement.scoped(protected.participants[index], 0, 1))),
        |_| {},
    )
    .await;
    assert!(receipt.broker_registration && receipt.broker_receiver_ack);
    assert!(receipt.same_profile_reopened);
    assert_eq!(receipt.typed_terminal, SemanticOperationPhase::Failed);
    assert!(retirement
        .records_for(protected.participants[0], 0)
        .is_empty());
}

#[cfg(all(feature = "simulator-checkpoint", unix))]
#[tokio::test]
async fn selected_replay_refuses_foreign_owner_and_stale_epoch_before_mutation() {
    use aura_effects::profile_checkpoint::ProfileCheckpointError;
    let inputs = HostRefusalReplayInputs::default();
    let protected = ProtectedNativeEnrollmentReplay::capture(&inputs).await;
    let stale_epoch_binding = aura_core::Hash32::from_value(&(
        protected.participants[0],
        1_u64,
        protected.configurations[0],
    ))
    .expect("canonical stale epoch binding")
    .0;
    assert!(matches!(
        protected.checkpoints[0]
            .restore(
                protected.roots[0].path(),
                protected.revision,
                stale_epoch_binding,
            )
            .await,
        Err(ProfileCheckpointError::ForeignSource)
    ));
    assert!(matches!(
        protected.checkpoints[0]
            .restore(
                protected.roots[1].path(),
                protected.revision,
                protected.checkpoint_bindings[0],
            )
            .await,
        Err(ProfileCheckpointError::ForeignSource)
    ));
    // Failed authentication cannot leave an incomplete marker or replace the
    // original selected profile: both fresh owners still reacquire normally.
    protected.restore().await;
}

#[cfg(all(feature = "simulator-checkpoint", unix))]
#[tokio::test]
async fn protected_selected_profile_replay_benchmarks_three_branches_against_full_setups() {
    use aura_simulator::simulator_nonce_retirement::SimulatorNonceRetirementLedger;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let inputs = HostRefusalReplayInputs::default();
    let started = aura_effects::time::monotonic_now();
    let protected = ProtectedNativeEnrollmentReplay::capture(&inputs).await;
    let capture_millis = started.elapsed().as_millis();
    let retirement = Arc::new(SimulatorNonceRetirementLedger::default());
    let mut branches = Vec::new();
    let mut restore_millis = Vec::new();
    let mut branch_millis = Vec::new();
    for branch in 0..3 {
        let restore_started = aura_effects::time::monotonic_now();
        protected.restore().await;
        restore_millis.push(restore_started.elapsed().as_millis());
        let branch_started = aura_effects::time::monotonic_now();
        let receipt = run_host_storage_refusal_with_profiles(
            inputs.clone(),
            Some(&protected.roots),
            Some([0, 1].map(|index| retirement.scoped(protected.participants[index], 0, branch))),
            |_| {},
        )
        .await;
        assert!(receipt.broker_receiver_ack && receipt.same_profile_reopened);
        branches.push(receipt);
        branch_millis.push(branch_started.elapsed().as_millis());
    }
    let checkpoint_millis = started.elapsed().as_millis();
    assert!(branches.windows(2).all(|pair| pair[0] == pair[1]));

    let full_started = aura_effects::time::monotonic_now();
    let mut full = Vec::new();
    let mut full_setup_millis = Vec::new();
    for _ in 0..3 {
        let run_started = aura_effects::time::monotonic_now();
        full.push(run_host_storage_refusal_from_fresh_profile(inputs.clone()).await);
        full_setup_millis.push(run_started.elapsed().as_millis());
    }
    let full_millis = full_started.elapsed().as_millis();
    assert!(full.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(branches[0].typed_terminal, full[0].typed_terminal);
    assert_eq!(branches[0].typed_host_failure, full[0].typed_host_failure);

    // This artifact contains only fixed identity inputs, enum stage names,
    // typed status, and elapsed benchmark values. It never receives the
    // signed code, verifier, broker credential, profile bytes, or nonce state.
    let artifact_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts");
    let artifact_metadata =
        std::fs::symlink_metadata(&artifact_root).expect("worktree artifacts root");
    assert!(artifact_metadata.file_type().is_dir() && !artifact_metadata.file_type().is_symlink());
    let receipt_dir = tempfile::Builder::new()
        .prefix("aura-task449-receipt-")
        .tempdir_in(&artifact_root)
        .expect("private benchmark receipt directory");
    std::fs::set_permissions(receipt_dir.path(), std::fs::Permissions::from_mode(0o700))
        .expect("owner-only receipt directory");
    let receipt_path = receipt_dir.path().join("replay.json");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&receipt_path)
        .expect("new owner-only replay receipt");
    let redacted = serde_json::json!({
        "fixture_source_digest": protected.revision.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "selected_provider": "runtime-owned-filesystem-fallback",
        "boundary": "quiescent-pre-issuance-empty-broker",
        "seed_base": inputs.seed_base,
        "selected_physical_ms": inputs.selected_physical_ms,
        "operation_instance": inputs.operation_instance,
        "owner": inputs.owner,
        "injected_fault": inputs.injected_fault,
        "broker_milestones": branches[0].broker_milestones,
        "first_message_ack": branches[0].broker_receiver_ack,
        "typed_terminal": format!("{:?}", branches[0].typed_terminal),
        "typed_host_failure": branches[0].typed_host_failure,
        "same_profile_reopened": branches[0].same_profile_reopened,
        "checkpoint_plus_three_restores_and_branches_ms": checkpoint_millis,
        "checkpoint_capture_ms": capture_millis,
        "restore_ms": restore_millis,
        "signed_branch_ms": branch_millis,
        "three_full_setups_ms": full_millis,
        "full_setup_ms": full_setup_millis,
        "network_delivery_asserted": false,
    });
    serde_json::to_writer_pretty(&mut file, &redacted).expect("redacted benchmark receipt");
    use std::io::Write;
    file.flush().expect("flush receipt");
    file.sync_all().expect("durable receipt");
    drop(file);
    let retained = receipt_dir.keep();
    println!(
        "task449 protected replay receipt: {}",
        retained.join("replay.json").display()
    );
}

#[tokio::test]
async fn genuine_public_app_enrollment_host_storage_refusal_keeps_original_failure() {
    let receipt =
        run_host_storage_refusal_from_fresh_profile(HostRefusalReplayInputs::default()).await;
    assert!(receipt.broker_receiver_ack);
}

#[tokio::test]
async fn original_host_storage_refusal_replays_from_captured_inputs() {
    let captured =
        run_host_storage_refusal_from_fresh_profile(HostRefusalReplayInputs::default()).await;
    assert_eq!(
        captured.inputs.owner,
        "WorkflowHandoffSubmission/ImportDeviceEnrollmentCode"
    );
    assert_eq!(captured.inputs.injected_fault, "HostPathIsFile");
    let replayed = run_host_storage_refusal_from_fresh_profile(captured.inputs.clone()).await;
    assert_eq!(captured, replayed);
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
