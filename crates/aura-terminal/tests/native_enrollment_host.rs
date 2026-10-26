//! Genuine signed import through the retained native terminal profile owner.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::disallowed_types)]

use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use aura_agent::{AgentConfig, EnrollmentParticipantProfile, SharedTransport};
use aura_app::ui_contract::{
    OperationId, OperationInstanceId, SemanticOperationKind, SemanticOperationPhase,
};
use aura_app::{
    frontend_primitives::{
        SubmittedOperationPublisher, SubmittedTerminalAcknowledgment,
        SubmittedTerminalAcknowledgmentScope, SubmittedTerminalIntent,
        SubmittedTerminalObservation, WorkflowHandoffSubmission,
    },
    ui::workflows::invitation::import_device_enrollment_with_terminal_status,
};
use aura_core::{
    effects::{ReactiveEffects, StorageCoreEffects},
    AuthorityId, ContextId, DeviceId,
};
use aura_simulator::native_enrollment_fixture::NativeEnrollmentFixture;
use aura_terminal::{handlers::tui::ProfileStore, tui::context::AccountFilesHelper};
use aura_testkit::time::ManualPhysicalClock;

#[derive(Clone)]
struct Prior;
#[async_trait::async_trait]
impl SubmittedOperationPublisher for Prior {
    async fn publish_dispatched(
        &self,
        _: &OperationId,
        _: &OperationInstanceId,
        _: SemanticOperationKind,
    ) {
        panic!("original handoff is unpublished");
    }
    fn publish_terminal(&self, intent: SubmittedTerminalIntent) -> SubmittedTerminalObservation {
        let ack = SubmittedTerminalAcknowledgment {
            operation_id: intent.operation_id,
            instance_id: intent.instance_id,
            scope: SubmittedTerminalAcknowledgmentScope::ComponentObserved,
        };
        drop(intent.retained);
        Box::pin(async move { Ok(ack) })
    }
    fn relinquish(&self, _: &OperationId, _: &OperationInstanceId, _: SemanticOperationKind) {}
}

#[tokio::test]
async fn native_enrollment_host_ack_readback_and_same_profile_reopen() {
    let roots: Vec<_> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
    let recipient_root = roots[1].path().to_path_buf();
    let clock = Arc::new(ManualPhysicalClock::new(5_000));
    let profiles = std::array::from_fn(|index| {
        let seed = 191 + index as u8 * 3;
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
    });
    let issuer_device = profiles[0].config.device_id;
    let owner =
        NativeEnrollmentFixture::prepare(profiles, clock.clone(), "native-host-adapter-auth")
            .await
            .expect("genuine signed native issuance");
    let fixture = &owner.enrollment;
    let recipient = fixture.recipient().clone();
    let selected = ProfileStore::from_selected_runtime_for_testing(recipient.runtime().effects())
        .expect("actual selected profile remains owned");
    selected
        .stage_provisional_runtime_account_for_testing(&recipient)
        .await
        .expect("actual native provisional account staging");
    {
        let refusal = match ProfileStore::production(&recipient_root) {
            Err(refusal) => refusal,
            Ok(_) => panic!("second native owner must be refused"),
        };
        let mut source: &dyn std::error::Error = &refusal;
        let ownership = loop {
            if let Some(ownership) =
                source.downcast_ref::<aura_core::effects::profile_storage::ProfileStorageError>()
            {
                break ownership;
            }
            source = source
                .source()
                .expect("original profile ownership source retained");
        };
        assert!(matches!(
            ownership,
            aura_core::effects::profile_storage::ProfileStorageError::Busy
        ));
    }
    let account_files = AccountFilesHelper::new(
        recipient_root.clone(),
        recipient.runtime().effects().config().device_id.to_string(),
        Arc::new(AtomicBool::new(false)),
    )
    .with_profile(Some(selected.clone()), false);
    let host_policy = account_files.enrollment_import_host_policy();
    let app = fixture.recipient_app();
    aura_app::AppCore::init_signals_with_hooks(&app)
        .await
        .unwrap();
    let admission = app
        .read()
        .await
        .enrollment_import_admission()
        .cloned()
        .unwrap();
    let transfer = fixture.start().manifest_transfer.as_ref().unwrap();
    let instance = OperationInstanceId("native-host-original-import".into());
    let prior = WorkflowHandoffSubmission::submit_with_instance_unpublished(
        Prior,
        OperationId::device_enrollment(),
        SemanticOperationKind::ImportDeviceEnrollmentCode,
        instance.clone(),
    );
    let observation = import_device_enrollment_with_terminal_status(
        &app,
        &admission,
        fixture.start().enrollment_code.clone(),
        Some(aura_app::ui_contract::EnrollmentManifestTransferInput {
            manifest_code: transfer.manifest_code.clone(),
            initiator_verifier_code: transfer.initiator_verifier_code.clone(),
        }),
        Some(instance.clone()),
        host_policy,
    );
    let mut observation = match observation {
        Ok(observation) => observation,
        Err(refusal) => panic!(
            "original app import admission refused: {:?}",
            refusal.source_error()
        ),
    };
    if let Err((_prior, error)) = observation.accept_handoff(prior) {
        panic!("original handoff refused: {:?}", error.source_error());
    }
    let bridge = recipient.clone().as_runtime_bridge();
    let completion = owner
        .drive_until(
            || async {
                let _ = aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(
                    &app, &bridge,
                )
                .await;
            },
            &mut observation,
        )
        .await;
    let accepted = completion.result.unwrap_or_else(|error| {
        panic!(
            "native host import failed: {error:?}; terminal={:?}",
            completion.terminal
        )
    });
    assert_eq!(
        completion.terminal.as_ref().unwrap().status.phase,
        SemanticOperationPhase::Succeeded
    );
    let selected_bytes = selected
        .storage()
        .retrieve("selected-runtime-identity.json")
        .await
        .unwrap()
        .unwrap();
    let selected_identity: aura_app::ui::types::BootstrapRuntimeIdentity =
        serde_json::from_slice(&selected_bytes).unwrap();
    assert_eq!(selected_identity.authority_id, accepted.subject_authority());
    assert_eq!(selected_identity.device_id, accepted.device_id());
    let account_bytes = selected
        .storage()
        .retrieve("account.json")
        .await
        .unwrap()
        .unwrap();
    let account: aura_app::ui::types::AccountConfig =
        serde_json::from_slice(&account_bytes).unwrap();
    assert_eq!(account.authority_id, accepted.subject_authority());
    assert!(
        account_files.has_account(),
        "host acknowledgment marks native account ready"
    );
    let exact_terminals: Vec<_> = app
        .read()
        .await
        .authoritative_semantic_facts()
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
    assert_eq!(exact_terminals, vec![SemanticOperationPhase::Succeeded]);
    // Read the actual post-terminal view without a test-side refresh that could
    // hide a missing workflow publication. Device counts do not authorize success.
    let original_settings = app
        .read()
        .await
        .read(&*aura_app::ui::signals::SETTINGS_SIGNAL)
        .await;
    drop(observation);
    drop(bridge);
    drop(admission);
    drop(app);
    drop(account_files);
    drop(selected);
    drop(recipient);
    let stopped = owner
        .enrollment
        .finish_accepted()
        .await
        .expect("strict original owner disposal");
    owner
        .supervisor
        .shutdown_with_timeout(Duration::from_secs(5))
        .await
        .unwrap();
    let reopened_runtime = stopped
        .reopen_accepted_for_host_recovery(SharedTransport::default())
        .await
        .expect("same selected native provider reopens");
    let reopened_app = reopened_runtime.recipient_app();
    let reopened_signal_initialization =
        aura_app::AppCore::init_signals_with_hooks(&reopened_app).await;
    let reopened_settings_refresh =
        aura_app::ui::workflows::settings::refresh_settings_from_runtime(&reopened_app).await;
    let reopened_settings = reopened_app
        .read()
        .await
        .read(&*aura_app::ui::signals::SETTINGS_SIGNAL)
        .await;
    drop(reopened_app);
    let reopened = ProfileStore::from_selected_runtime_for_testing(
        reopened_runtime.recipient().runtime().effects(),
    )
    .expect("reopened runtime retains its original profile owner");
    let reopened_identity = reopened
        .storage()
        .retrieve("selected-runtime-identity.json")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened_identity, selected_bytes);
    let reopened_account = reopened
        .storage()
        .retrieve("account.json")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened_account, account_bytes);
    drop(reopened);
    reopened_runtime
        .shutdown()
        .await
        .expect("same confirmed native profiles reopen");
    // Preserve original disposal/reopen even when a new observation assertion
    // fails. These snapshots are read-only evidence, never completion authority.
    reopened_signal_initialization.expect("same-profile app signals initialize");
    reopened_settings_refresh.expect("same-profile startup refresh uses the actual runtime");
    let expected_devices = [issuer_device, accepted.device_id()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let original_settings =
        original_settings.expect("original terminal has an actual settings snapshot");
    assert_eq!(
        original_settings.authority_id,
        accepted.original_provisional().to_string(),
        "the provisional runtime retains its authority until native reload"
    );
    {
        let settings = reopened_settings.expect("reopened profile has an actual settings snapshot");
        assert_eq!(
            settings.authority_id,
            accepted.subject_authority().to_string()
        );
        assert_eq!(
            settings.devices.len(),
            2,
            "exact original enrolled device inventory"
        );
        assert_eq!(
            settings
                .devices
                .iter()
                .map(|device| device.id)
                .collect::<std::collections::BTreeSet<_>>(),
            expected_devices,
            "the app view observes both actual signed enrollment participants"
        );
        let current: Vec<_> = settings
            .devices
            .iter()
            .filter(|device| device.is_current)
            .collect();
        assert_eq!(current.len(), 1, "one actual current device is observed");
        assert_eq!(current[0].id, accepted.device_id());
    }
}
