//! Simulator-only proof that signed setup retains its original broker owner.

#![cfg(all(feature = "simulator-checkpoint", unix))]

use std::{sync::Arc, time::Duration};

use aura_agent::{
    AgentConfig, BootstrapBrokerLanBindPolicy, EnrollmentParticipantProfile,
    LocalBootstrapBrokerService, StoppedBrokerCheckpointOwner, TaskSupervisor,
};
use aura_core::{AuthorityId, ContextId, DeviceId};
use aura_simulator::native_enrollment_fixture::NativeEnrollmentFixture;
use aura_testkit::time::ManualPhysicalClock;

#[derive(Clone)]
struct CheckpointSubmission;

#[async_trait::async_trait]
impl aura_app::frontend_primitives::SubmittedOperationPublisher for CheckpointSubmission {
    async fn publish_dispatched(
        &self,
        _: &aura_app::ui_contract::OperationId,
        _: &aura_app::ui_contract::OperationInstanceId,
        _: aura_app::ui_contract::SemanticOperationKind,
    ) {
        panic!("checkpoint fixture hands off an unpublished operation");
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

async fn pending_progress(
    broker_url: &str,
    subject: AuthorityId,
    device: DeviceId,
    digest: [u8; 32],
) -> serde_json::Value {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let authority = broker_url.strip_prefix("http://").expect("loopback broker");
    let mut stream = tokio::net::TcpStream::connect(authority)
        .await
        .expect("original broker progress connection");
    let request = format!(
        "GET /v2/bootstrap/enrollment-mailbox/progress-witness/{subject}/{device}/{} HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer genuine-complete-vm-auth\r\nConnection: close\r\n\r\n",
        hex::encode(digest)
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("authenticated progress query");
    let mut response = Vec::new();
    (&mut stream)
        .take(16 * 1024 + 1)
        .read_to_end(&mut response)
        .await
        .expect("bounded progress response");
    assert!(response.len() < 16 * 1024 && response.starts_with(b"HTTP/1.1 200 "));
    let divider = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("progress divider");
    serde_json::from_slice(&response[divider + 4..]).expect("typed original broker progress")
}

fn original_vm_profiles(roots: &[tempfile::TempDir]) -> [EnrollmentParticipantProfile; 2] {
    std::array::from_fn(|index| {
        let seed = 151 + index as u8 * 3;
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

#[tokio::test]
async fn original_signed_broker_registration_stops_same_owned_broker() {
    let roots: Vec<_> = (0..2)
        .map(|_| tempfile::tempdir().expect("isolated original profile"))
        .collect();
    use std::os::unix::fs::PermissionsExt;
    for root in &roots {
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private original profile root");
    }
    let clock = Arc::new(ManualPhysicalClock::new(5_000));
    let mut owner = NativeEnrollmentFixture::prepare(
        original_vm_profiles(&roots),
        clock.clone(),
        "genuine-complete-vm-auth",
    )
    .await
    .expect("original signed issuance and broker registration");
    let (subject, _device, digest) = owner
        .enrollment
        .original_mailbox_selector()
        .expect("original signed selector");
    let issuer_device = owner
        .enrollment
        .issuer()
        .runtime()
        .effects()
        .config()
        .device_id;
    let authority = owner
        .broker_url()
        .strip_prefix("http://")
        .expect("loopback broker")
        .trim_end_matches('/')
        .to_owned();
    let mut stream = tokio::net::TcpStream::connect(&authority)
        .await
        .expect("original broker connection");
    let path = format!(
        "/v2/bootstrap/enrollment-return-mailbox/register/{subject}/{issuer_device}/{}",
        hex::encode(digest)
    );
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer genuine-complete-vm-auth\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("query original broker");
    let mut response = Vec::new();
    (&mut stream)
        .take(16 * 1024 + 1)
        .read_to_end(&mut response)
        .await
        .expect("bounded original registration readback");
    assert!(response.len() < 16 * 1024 && response.starts_with(b"HTTP/1.1 200 "));
    let divider = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("original registration divider");
    let registration: serde_json::Value =
        serde_json::from_slice(&response[divider + 4..]).expect("typed registration response");
    assert!(
        registration.is_object(),
        "original signed return registration retained"
    );
    let witness = owner
        .enrollment
        .issuer_broker_checkpoint_witness()
        .await
        .expect("original signed issuer and selected filesystem profile");
    let stopped = owner
        .stop_original_broker_for_checkpoint()
        .await
        .expect("exact original broker task group disposed");
    assert!(owner.supervisor.active_tasks().is_empty());
    assert!(owner.stop_original_broker_for_checkpoint().await.is_err());
    let _ = owner
        .enrollment
        .fail(aura_agent::AgentError::config(
            "fixture stops after original broker disposal",
        ))
        .await;
    let artifact_root = tempfile::tempdir().expect("private checkpoint artifact root");
    std::fs::set_permissions(artifact_root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private checkpoint artifact directory");
    let revision = aura_core::Hash32::from_bytes(include_bytes!("broker_checkpoint_handoff.rs")).0;
    let participant = aura_core::Hash32::from_value(&(subject, issuer_device, digest))
        .expect("original signed participant binding")
        .0;
    let owner_id = witness.owner();
    let fixture = aura_core::Hash32::from_value(&(participant, "signed-original-loopback"))
        .expect("signed fixture binding")
        .0;
    let context = StoppedBrokerCheckpointOwner::context_for_simulator(
        owner_id,
        witness.epoch(),
        fixture,
        clock.now_ms(),
    );
    let mut sealed = stopped
        .seal_with_selected_profile(
            witness,
            &artifact_root.path().join("broker-v2.bin"),
            revision,
            participant,
            context,
        )
        .expect("effect-owned V2 broker/profile co-checkpoint");
    let new_broker = || async {
        LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(120),
            "genuine-complete-vm-auth",
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("fresh broker bind")
        .with_mailbox_time_provider(clock.clone())
    };
    let mut foreign = context;
    foreign.owner[0] ^= 1;
    assert!(sealed
        .restore_into_fresh_broker(new_broker().await, foreign, roots[0].path())
        .await
        .is_err());
    let mut stale_epoch = context;
    stale_epoch.epoch += 1;
    assert!(sealed
        .restore_into_fresh_broker(new_broker().await, stale_epoch, roots[0].path())
        .await
        .is_err());
    assert!(sealed
        .restore_into_fresh_broker(new_broker().await, context, roots[1].path())
        .await
        .is_err());
    clock.set_time(context.selected_clock_ms - 1);
    assert!(sealed
        .restore_into_fresh_broker(new_broker().await, context, roots[0].path())
        .await
        .is_err());
    clock.set_time(context.selected_clock_ms);
    let restored = sealed
        .restore_into_fresh_broker(new_broker().await, context, roots[0].path())
        .await
        .expect("authenticated original broker state restored");
    let reopened_url = restored.public_url().to_owned();
    let reopened_supervisor = TaskSupervisor::new();
    restored.start(&reopened_supervisor.group("reopened_original_signed_broker"));
    let address = reopened_url.strip_prefix("http://").expect("loopback URL");
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("fresh broker connection");
    let request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer genuine-complete-vm-auth\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("fresh broker request");
    let mut reopened_response = Vec::new();
    (&mut stream)
        .take(16 * 1024 + 1)
        .read_to_end(&mut reopened_response)
        .await
        .expect("fresh broker bounded readback");
    assert!(reopened_response.starts_with(b"HTTP/1.1 200 "));
    let divider = reopened_response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .expect("fresh broker divider");
    let restored_registration: serde_json::Value =
        serde_json::from_slice(&reopened_response[divider + 4..])
            .expect("restored signed registration");
    assert_eq!(restored_registration, registration);
    assert!(sealed
        .restore_into_fresh_broker(new_broker().await, context, roots[0].path())
        .await
        .is_err());
    reopened_supervisor
        .shutdown_with_timeout(Duration::from_secs(5))
        .await
        .expect("fresh broker shutdown");
}

#[derive(Clone, Copy)]
enum OriginalDeliveryStage {
    Queued,
    Served,
    Acknowledged,
}

async fn signed_delivery_refuses_checkpoint(stage: OriginalDeliveryStage) {
    use aura_agent::BrokerCheckpointQuiescenceError;
    use aura_app::frontend_primitives::WorkflowHandoffSubmission;
    use aura_app::ui::workflows::invitation::{
        import_device_enrollment_with_terminal_status, EnrollmentImportHostPolicy,
    };
    use aura_app::ui_contract::{OperationId, OperationInstanceId, SemanticOperationKind};

    let roots: Vec<_> = (0..2)
        .map(|_| tempfile::tempdir().expect("private profile"))
        .collect();
    use std::os::unix::fs::PermissionsExt;
    for root in &roots {
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private original profile");
    }
    let clock = Arc::new(ManualPhysicalClock::new(5_000));
    let mut owner = NativeEnrollmentFixture::prepare(
        original_vm_profiles(&roots),
        clock.clone(),
        "genuine-complete-vm-auth",
    )
    .await
    .expect("original signed issuance and broker registration");
    let (subject, device, digest) = owner
        .enrollment
        .original_mailbox_selector()
        .expect("retained original signed selector");
    let app = owner.enrollment.recipient_app();
    aura_app::AppCore::init_signals_with_hooks(&app)
        .await
        .expect("recipient app signals");
    let admission = app
        .read()
        .await
        .enrollment_import_admission()
        .cloned()
        .expect("original app admission");
    let transfer = owner
        .enrollment
        .start()
        .manifest_transfer
        .as_ref()
        .expect("original signed manifest transfer");
    let prior = WorkflowHandoffSubmission::submit_with_instance_unpublished(
        CheckpointSubmission,
        OperationId::device_enrollment(),
        SemanticOperationKind::ImportDeviceEnrollmentCode,
        OperationInstanceId("signed-checkpoint-delivery".into()),
    );
    let mut observation = import_device_enrollment_with_terminal_status(
        &app,
        &admission,
        owner.enrollment.start().enrollment_code.clone(),
        Some(aura_app::ui_contract::EnrollmentManifestTransferInput {
            manifest_code: transfer.manifest_code.clone(),
            initiator_verifier_code: transfer.initiator_verifier_code.clone(),
        }),
        Some(OperationInstanceId("signed-checkpoint-delivery".into())),
        EnrollmentImportHostPolicy::Finalize(Arc::new(|_| Box::pin(async { Ok(()) }))),
    )
    .unwrap_or_else(|refusal| panic!("original app import admission: {}", refusal.source_error()));
    observation
        .accept_handoff(prior)
        .unwrap_or_else(|_| panic!("original app handoff"));
    let mut observation = Box::pin(observation);
    let issuer = owner.enrollment.issuer().clone();
    let mut issuer_drain = Some(Box::pin(issuer.drain_issuer_return_vm_mailbox()));
    let broker_url = owner.broker_url().to_owned();
    let queued_id = aura_testkit::infrastructure::time::assertions::assert_completes_within(
        Duration::from_secs(15), async {
            let mut tick = tokio::time::interval(Duration::from_millis(10));
            loop {
                tokio::select! {
                    result = &mut observation => panic!("original import completed before first queue: {result:?}"),
                    result = async {
                        match issuer_drain.as_mut() {
                            Some(drain) => drain.await,
                            None => std::future::pending().await,
                        }
                    } => {
                        result.expect("original issuer drain");
                        issuer_drain = None;
                    }
                    _ = tick.tick() => {
                        if issuer_drain.is_none() {
                            issuer_drain = Some(Box::pin(issuer.drain_issuer_return_vm_mailbox()));
                        }
                        let progress = pending_progress(&broker_url, subject, device, digest).await;
                        if let Some(message) = progress["messages"].as_array().and_then(|messages| messages.first()) {
                            assert_eq!(message["stage"], "queued");
                            let id = message["message_id"].as_str().expect("original first ID").to_owned();
                            break Ok(id);
                        }
                        aura_simulator::native_enrollment_fixture::advance_selected_clock_before_frame(
                            &owner.enrollment, &clock, false,
                        ).await;
                    }
                }
            }
        },
    ).await.expect("original signed request queued");
    drop(issuer_drain);

    if matches!(stage, OriginalDeliveryStage::Served) {
        assert!(
            owner.interrupt_original_pending_ack_for_checkpoint(queued_id.clone()),
            "named fault binds original queued message ID"
        );
    }
    if !matches!(stage, OriginalDeliveryStage::Queued) {
        let recipient = owner.enrollment.recipient().clone();
        let bridge = recipient.as_runtime_bridge();
        let app_for_maintenance = app.clone();
        let broker_for_progress = broker_url.clone();
        let maintenance_errors = Arc::new(std::sync::Mutex::new(Vec::new()));
        let errors_for_maintenance = maintenance_errors.clone();
        let expected = match stage {
            OriginalDeliveryStage::Served => "retrieval_selected_for_receiver_credential",
            OriginalDeliveryStage::Acknowledged => "receiver_acknowledged_retained_frame",
            OriginalDeliveryStage::Queued => unreachable!(),
        };
        owner.drive_until(
            || {
                let app = app_for_maintenance.clone();
                let bridge = bridge.clone();
                let errors = errors_for_maintenance.clone();
                async move {
                    if let Err(error) = aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(
                        &app, &bridge,
                    ).await {
                        errors.lock().expect("typed maintenance cause").push(error);
                    }
                }
            },
            async {
                let mut tick = tokio::time::interval(Duration::from_millis(10));
                let mut observation_completed = false;
                loop {
                    tokio::select! {
                        result = async {
                            if observation_completed {
                                std::future::pending().await
                            } else {
                                (&mut observation).await
                            }
                        } => {
                            if matches!(stage, OriginalDeliveryStage::Served) {
                                panic!("faulted import completed before served custody: {result:?}");
                            }
                            observation_completed = true;
                        }
                        _ = tick.tick() => {
                            let progress = pending_progress(&broker_for_progress, subject, device, digest).await;
                            if progress["messages"].as_array().is_some_and(|messages| messages.iter().any(|message| {
                                message["message_id"] == queued_id && message["stage"] == expected
                            })) && (!matches!(stage, OriginalDeliveryStage::Served)
                                || !maintenance_errors.lock().expect("typed maintenance cause").is_empty()) {
                                break;
                            }
                        }
                    }
                }
            },
        ).await;
        if matches!(stage, OriginalDeliveryStage::Served) {
            assert!(
                maintenance_errors
                    .lock()
                    .expect("typed maintenance cause")
                    .iter()
                    .any(|error| {
                        let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
                        while let Some(current) = cause {
                            if matches!(
                                current.downcast_ref::<aura_core::AuraError>(),
                                Some(aura_core::AuraError::Network { .. })
                            ) {
                                return true;
                            }
                            cause = current.source();
                        }
                        false
                    }),
                "named original ACK interruption must retain its typed network cause"
            );
        }
        drop(bridge);
    }
    drop(observation);
    drop(app);
    let refusal = match owner.stop_original_broker_for_checkpoint().await {
        Err(refusal) => refusal,
        Ok(_) => panic!("delivery custody must refuse checkpoint"),
    };
    assert!(
        match stage {
            OriginalDeliveryStage::Queued | OriginalDeliveryStage::Served =>
                matches!(&refusal, BrokerCheckpointQuiescenceError::UnsettledDelivery),
            OriginalDeliveryStage::Acknowledged =>
                matches!(&refusal, BrokerCheckpointQuiescenceError::UnsupportedState),
        },
        "unexpected typed original broker refusal: {refusal:?}"
    );
    assert!(owner.supervisor.active_tasks().is_empty());
    let _ = owner
        .enrollment
        .fail(aura_agent::AgentError::config(
            "checkpoint refused live signed delivery",
        ))
        .await;
}

#[tokio::test]
async fn original_signed_queued_delivery_refuses_checkpoint() {
    signed_delivery_refuses_checkpoint(OriginalDeliveryStage::Queued).await;
}

#[tokio::test]
async fn original_signed_served_delivery_refuses_checkpoint() {
    signed_delivery_refuses_checkpoint(OriginalDeliveryStage::Served).await;
}

#[tokio::test]
async fn original_signed_ack_history_refuses_checkpoint() {
    signed_delivery_refuses_checkpoint(OriginalDeliveryStage::Acknowledged).await;
}
