#![allow(missing_docs)]

use super::*;
enum DeviceEnrollmentAcceptConvergenceError {
    Terminal(AuraError),
    Workflow(AuraError),
}

#[cfg(all(test, feature = "signals"))]
mod failure_owner_tests {
    use super::*;
    use std::error::Error;

    async fn signed_transfer() -> (
        aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
        crate::ui_contract::EnrollmentManifestTransferInput,
        aura_invitation::enrollment_manifest::VerifiedEnrollmentManifestSignature,
    ) {
        use aura_core::crypto::single_signer::SigningMode;
        use aura_core::effects::CryptoCoreEffects;
        use aura_core::threshold::{AgreementMode, ParticipantIdentity};
        use aura_core::tree::NodeIndex;
        use aura_invitation::enrollment_manifest::{
            encode_initiator_verifier_transfer, EnrollmentParentVerifier, EnrollmentTrustManifest,
            SignedEnrollmentTrustManifest,
        };
        use aura_invitation::enrollment_setup::DeviceEnrollmentSetupBinding;
        let crypto = aura_effects::RealCryptoHandler::for_simulation_seed([175; 32]);
        let (private, public) = crypto
            .ed25519_generate_keypair()
            .await
            .expect("original enrollment fixture prerequisite");
        let device = aura_core::DeviceId::new_from_entropy([91; 32]);
        // Same valid inventory shape as the domain's signed-manifest fixture.
        let manifest = EnrollmentTrustManifest {
            version: EnrollmentTrustManifest::CURRENT_VERSION,
            subject: AuthorityId::new_from_entropy([92; 32]),
            initiator_device: device,
            invitee_authority: AuthorityId::new_from_entropy([93; 32]),
            invitee_device: aura_core::DeviceId::new_from_entropy([94; 32]),
            setup: DeviceEnrollmentSetupBinding {
                nonce: [95; 32],
                digest: [96; 32],
            },
            invitation: aura_core::InvitationId::new("exact-manifest-invitation"),
            ceremony: aura_core::CeremonyId::new("exact-manifest-ceremony"),
            expires_at_ms: 200,
            baseline_digest: [97; 32],
            baseline_count: 1,
            starting_epoch: 0,
            starting_commitment: [98; 32],
            final_inventory: vec![EnrollmentParentVerifier {
                epoch: 1,
                commitment: [99; 32],
                signing_node: NodeIndex(0),
                mode: SigningMode::SingleSigner,
                threshold: 1,
                participants: vec![ParticipantIdentity::device(device)],
                public_key_package:
                    aura_core::crypto::single_signer::SingleSignerPublicKeyPackage::try_new(
                        public.clone(),
                    )
                    .expect("original enrollment fixture prerequisite")
                    .to_bytes()
                    .expect("original enrollment fixture prerequisite"),
                agreement: AgreementMode::ConsensusFinalized,
            }],
            final_epoch: 1,
            final_commitment: [99; 32],
            pending_epoch: 2,
            pending_share_digest: [106; 32],
            pending_public_key_package_digest: [100; 32],
            pending_threshold_config_digest: aura_core::Hash32::new([101; 32]),
            initiator_confirmation_verifier: public.clone(),
            parents: vec![EnrollmentParentVerifier {
                epoch: 0,
                commitment: [98; 32],
                signing_node: NodeIndex(0),
                mode: SigningMode::SingleSigner,
                threshold: 1,
                participants: vec![ParticipantIdentity::device(device)],
                public_key_package: vec![102; 32],
                agreement: AgreementMode::ConsensusFinalized,
            }],
        };
        let signature = aura_signature::sign_ed25519_transcript(&crypto, &manifest, &private)
            .await
            .expect("original enrollment fixture prerequisite");
        let signed = SignedEnrollmentTrustManifest {
            manifest: manifest.clone(),
            signature,
        };
        let transfer = crate::ui_contract::EnrollmentManifestTransferInput {
            manifest_code: signed
                .encode()
                .expect("original enrollment fixture prerequisite"),
            initiator_verifier_code: encode_initiator_verifier_transfer(
                manifest.subject,
                device,
                &public,
            )
            .expect("original enrollment fixture prerequisite"),
        };
        let decoded = SignedEnrollmentTrustManifest::decode(&transfer.manifest_code)
            .expect("original enrollment fixture prerequisite");
        let selected = aura_invitation::enrollment_manifest::decode_initiator_verifier_transfer(
            &transfer.initiator_verifier_code,
        )
        .expect("original enrollment fixture prerequisite");
        let verified = decoded
            .manifest
            .verify_signature(&crypto, &selected.verifying_key, &decoded.signature)
            .await
            .expect("original enrollment fixture prerequisite");
        (manifest, transfer, verified)
    }

    #[tokio::test]
    async fn public_import_uses_verified_transfer_and_preserves_original_terminal_and_native_failure(
    ) {
        use futures::FutureExt;
        for fail_import in [false, true] {
            let (manifest, transfer, verified) = signed_transfer().await;
            let digest = verified.digest();
            let mut runtime =
                crate::runtime_bridge::OfflineRuntimeBridge::new(manifest.invitee_authority);
            runtime.use_time_provider(Arc::new(aura_testkit::time::ManualPhysicalClock::new(100)));
            runtime
                .set_settings_snapshot(crate::runtime_bridge::OfflineSettingsSnapshot::default());
            runtime.queue_sync_answers((0..16).map(|_| async { Ok(()) }.boxed()).collect());
            runtime.set_process_ceremony_result(Ok(
                crate::runtime_bridge::CeremonyProcessingOutcome::NoProgress,
            ));
            runtime.queue_verified_manifest(
                transfer.manifest_code.clone(),
                transfer.initiator_verifier_code.clone(),
                Ok(verified),
            );
            let invitation = InvitationInfo {
                invitation_id: manifest.invitation.clone(),
                sender_id: manifest.subject,
                receiver_id: manifest.invitee_authority,
                invitation_type: InvitationBridgeType::DeviceEnrollment {
                    subject_authority: manifest.subject,
                    initiator_device_id: manifest.initiator_device,
                    device_id: manifest.invitee_device,
                    nickname_suggestion: None,
                    ceremony_id: manifest.ceremony.clone(),
                    pending_epoch: aura_core::Epoch(manifest.pending_epoch),
                },
                status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
                created_at_ms: 100,
                expires_at_ms: Some(manifest.expires_at_ms),
                message: None,
                receiver_nickname: None,
            };
            let native = std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "original enrollment import denied",
            );
            runtime.queue_enrollment_import_answers(vec![async move {
                if fail_import {
                    Err(
                        aura_invitation::enrollment_manifest::EnrollmentManifestError::Runtime(
                            Box::new(native),
                        ),
                    )
                } else {
                    Ok(invitation)
                }
            }
            .boxed()]);
            runtime.set_accept_invitation_result(Ok(
                crate::runtime_bridge::InvitationMutationOutcome {
                    invitation_id: manifest.invitation.clone(),
                    new_status: crate::runtime_bridge::InvitationBridgeStatus::Accepted,
                },
            ));
            let supervisor = aura_testkit::task_custody::SupervisedTaskTestkit::new();
            runtime.use_test_task_spawner(supervisor.owned_spawner());
            let runtime = Arc::new(runtime);
            let core = AppCore::with_runtime(crate::AppConfig::default(), runtime.clone())
                .expect("original enrollment fixture prerequisite");
            crate::signal_defs::register_app_signals(&core)
                .await
                .expect("original enrollment fixture prerequisite");
            let app = Arc::new(RwLock::new(core));
            let instance =
                crate::ui_contract::OperationInstanceId(format!("public-import-{fail_import}"));
            let outcome = super::super::enrollment_import_owner::test_support::run(
                &app,
                "original-enrollment-code".into(),
                Some(transfer),
                Some(instance.clone()),
                crate::workflows::invitation::EnrollmentImportHostPolicy::RuntimeOnly,
            )
            .await;
            let custody_report =
                super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
            assert_eq!(custody_report.domain_failure.is_some(), fail_import);
            let terminal = outcome
                .terminal
                .expect("original enrollment fixture prerequisite");
            if fail_import {
                let error = outcome.result.unwrap_err();
                let mut cause: Option<&(dyn Error + 'static)> = Some(&error);
                let mut matched = false;
                while let Some(source) = cause {
                    if let Some(retained) = source.downcast_ref::<std::io::Error>() {
                        matched = retained.kind() == std::io::ErrorKind::PermissionDenied
                            && retained.to_string() == "original enrollment import denied";
                        break;
                    }
                    cause = source.source();
                }
                assert!(
                    matched,
                    "original concrete native error must survive public import: {error:?}"
                );
                assert_eq!(terminal.status.phase, SemanticOperationPhase::Failed);
                assert_eq!(runtime.accept_invitation_call_count(), 0);
            } else {
                let completion = outcome
                    .result
                    .expect("original enrollment fixture prerequisite");
                assert_eq!(completion.subject_authority(), manifest.subject);
                assert_eq!(completion.device_id(), manifest.invitee_device);
                assert_eq!(completion.invitation_id(), &manifest.invitation);
                assert_eq!(completion.manifest_digest(), digest);
                assert_eq!(terminal.status.phase, SemanticOperationPhase::Succeeded);
                assert_eq!(runtime.accept_invitation_call_count(), 1);
            }
            assert_eq!(
                runtime.enrollment_import_requests().await,
                vec![("original-enrollment-code".into(), digest)]
            );
            let facts = crate::workflows::signals::read_signal_or_default(
                &app,
                &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
            )
            .await;
            let terminals: Vec<_> = facts
                .iter()
                .filter_map(|fact| match fact {
                    crate::ui_contract::AuthoritativeSemanticFact::OperationStatus {
                        operation_id,
                        instance_id,
                        status,
                        ..
                    } if operation_id == &OperationId::device_enrollment()
                        && matches!(
                            status.phase,
                            SemanticOperationPhase::Succeeded
                                | SemanticOperationPhase::Failed
                                | SemanticOperationPhase::Cancelled
                        ) =>
                    {
                        Some((instance_id.clone(), status.phase))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(terminals, vec![(Some(instance), terminal.status.phase)]);
        }
    }

    async fn component_public_import_host_fixture() -> (
        Arc<RwLock<AppCore>>,
        Arc<crate::runtime_bridge::OfflineRuntimeBridge>,
        crate::ui_contract::EnrollmentManifestTransferInput,
        aura_testkit::time::ManualPhysicalClock,
        aura_testkit::task_custody::SupervisedTaskTestkit,
    ) {
        use futures::FutureExt;
        let (manifest, transfer, verified) = signed_transfer().await;
        let mut runtime =
            crate::runtime_bridge::OfflineRuntimeBridge::new(manifest.invitee_authority);
        let clock = aura_testkit::time::ManualPhysicalClock::new(100);
        runtime.use_time_provider(Arc::new(clock.clone()));
        let supervisor = aura_testkit::task_custody::SupervisedTaskTestkit::new();
        runtime.use_test_task_spawner(supervisor.owned_spawner());
        runtime.set_settings_snapshot(crate::runtime_bridge::OfflineSettingsSnapshot::default());
        runtime.queue_sync_answers((0..16).map(|_| async { Ok(()) }.boxed()).collect());
        runtime.set_process_ceremony_result(Ok(
            crate::runtime_bridge::CeremonyProcessingOutcome::NoProgress,
        ));
        runtime.queue_verified_manifest(
            transfer.manifest_code.clone(),
            transfer.initiator_verifier_code.clone(),
            Ok(verified),
        );
        let invitation = InvitationInfo {
            invitation_id: manifest.invitation.clone(),
            sender_id: manifest.subject,
            receiver_id: manifest.invitee_authority,
            invitation_type: InvitationBridgeType::DeviceEnrollment {
                subject_authority: manifest.subject,
                initiator_device_id: manifest.initiator_device,
                device_id: manifest.invitee_device,
                nickname_suggestion: None,
                ceremony_id: manifest.ceremony.clone(),
                pending_epoch: aura_core::Epoch(manifest.pending_epoch),
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 100,
            expires_at_ms: Some(manifest.expires_at_ms),
            message: None,
            receiver_nickname: None,
        };
        runtime.queue_enrollment_import_answers(vec![async move { Ok(invitation) }.boxed()]);
        runtime.set_accept_invitation_result(Ok(
            crate::runtime_bridge::InvitationMutationOutcome {
                invitation_id: manifest.invitation.clone(),
                new_status: crate::runtime_bridge::InvitationBridgeStatus::Accepted,
            },
        ));
        let runtime = Arc::new(runtime);
        let core = AppCore::with_runtime(crate::AppConfig::default(), runtime.clone())
            .expect("original enrollment fixture prerequisite");
        crate::signal_defs::register_app_signals(&core)
            .await
            .expect("original enrollment fixture prerequisite");
        let app = Arc::new(RwLock::new(core));
        (app, runtime, transfer, clock, supervisor)
    }

    #[tokio::test]
    async fn public_import_host_failure_preserves_original_source_and_instance() {
        let (app, runtime, transfer, _clock, supervisor) =
            component_public_import_host_fixture().await;
        let instance = crate::ui_contract::OperationInstanceId("host-storage-failure".into());
        let native = Arc::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "original host storage denied",
        ));
        let expected = native.clone();
        let outcome = super::super::enrollment_import_owner::test_support::run(
            &app,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::Finalize(Arc::new(move |_completed| {
                let native = native.clone();
                Box::pin(async move {
                    Err(AuraError::Storage {
                        message: "host storage acknowledgment failed".into(),
                        source: Some(native),
                    })
                })
            })),
        )
        .await;
        let custody_report =
            super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert!(custody_report.domain_failure.is_some());
        let error = outcome.result.unwrap_err();
        let retained = error
            .source()
            .expect("original enrollment fixture prerequisite")
            .downcast_ref::<std::io::Error>()
            .expect("original enrollment fixture prerequisite");
        assert!(std::ptr::eq(retained, expected.as_ref()));
        assert_eq!(retained.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(runtime.accept_invitation_call_count(), 1);
        let terminal = outcome
            .terminal
            .expect("original enrollment fixture prerequisite");
        assert_eq!(terminal.status.phase, SemanticOperationPhase::Failed);
        let facts = crate::workflows::signals::read_signal_or_default(
            &app,
            &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        )
        .await;
        let terminals: Vec<_> = facts
            .iter()
            .filter_map(|fact| match fact {
                crate::ui_contract::AuthoritativeSemanticFact::OperationStatus {
                    instance_id,
                    status,
                    ..
                } if matches!(
                    status.phase,
                    SemanticOperationPhase::Failed
                        | SemanticOperationPhase::Succeeded
                        | SemanticOperationPhase::Cancelled
                ) =>
                {
                    Some((instance_id.clone(), status.phase))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            terminals,
            vec![(Some(instance), SemanticOperationPhase::Failed)]
        );
    }

    #[tokio::test]
    async fn public_import_host_cancellation_never_publishes_success() {
        let (app, runtime, transfer, _clock, supervisor) =
            component_public_import_host_fixture().await;
        let (entered, observed) = tokio::sync::oneshot::channel();
        let entered = Arc::new(async_lock::Mutex::new(Some(entered)));
        let instance = crate::ui_contract::OperationInstanceId("host-storage-cancelled".into());
        let operation = Box::pin(super::super::enrollment_import_owner::test_support::run(
            &app,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::Finalize(Arc::new(move |completion| {
                assert_eq!(
                    completion.invitation_id(),
                    &aura_core::InvitationId::new("exact-manifest-invitation")
                );
                let entered = entered
                    .try_lock()
                    .expect("fixture access has no competing borrower")
                    .take()
                    .expect("single host invocation");
                Box::pin(async move {
                    entered
                        .send(())
                        .expect("original enrollment fixture prerequisite");
                    futures::future::pending::<Result<(), AuraError>>().await
                })
            })),
        ));
        match futures::future::select(operation, observed).await {
            futures::future::Either::Right((Ok(()), original)) => drop(original),
            _ => panic!("actual host acknowledgment must be entered before cancellation"),
        }
        let custody_report =
            super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert!(matches!(
            custody_report.execution,
            aura_core::effects::task::RequiredTaskExit::Cancelled
        ));
        assert!(custody_report.domain_failure.is_none());
        assert_component_host_terminal(&app, &instance, SemanticOperationPhase::Cancelled).await;
        assert_eq!(runtime.accept_invitation_call_count(), 1);
        let facts = crate::workflows::signals::read_signal_or_default(
            &app,
            &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        )
        .await;
        assert!(!facts.iter().any(|fact| matches!(fact, crate::ui_contract::AuthoritativeSemanticFact::OperationStatus { instance_id, status, .. } if instance_id.as_ref()==Some(&instance) && status.phase==SemanticOperationPhase::Succeeded)));
    }

    async fn assert_component_host_terminal(
        app: &Arc<RwLock<AppCore>>,
        instance: &crate::ui_contract::OperationInstanceId,
        expected: SemanticOperationPhase,
    ) {
        let facts = crate::workflows::signals::read_signal_or_default(
            app,
            &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        )
        .await;
        let terminals: Vec<_> = facts
            .iter()
            .filter_map(|fact| match fact {
                crate::ui_contract::AuthoritativeSemanticFact::OperationStatus {
                    instance_id,
                    status,
                    ..
                } if instance_id.as_ref() == Some(instance)
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
        assert_eq!(terminals, vec![expected]);
    }

    #[tokio::test]
    async fn public_import_host_prepare_success_commits_once_before_original_success() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (app, runtime, transfer, clock, supervisor) =
            component_public_import_host_fixture().await;
        let commits = Arc::new(AtomicUsize::new(0));
        let observed = commits.clone();
        let instance = crate::ui_contract::OperationInstanceId("prepared-host-success".into());
        let outcome = super::super::enrollment_import_owner::test_support::run(
            &app,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::Prepare(Arc::new(move |completion| {
                assert_eq!(
                    completion.invitation_id(),
                    &aura_core::InvitationId::new("exact-manifest-invitation")
                );
                let commits = commits.clone();
                Box::pin(async move {
                    Ok(PreparedEnrollmentHostCommit::new(move || {
                        commits.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }))
                })
            })),
        )
        .await;
        let custody_report =
            super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert!(custody_report.domain_failure.is_none());
        let completion = outcome
            .result
            .expect("original enrollment fixture prerequisite");
        assert_eq!(
            completion.invitation_id(),
            &aura_core::InvitationId::new("exact-manifest-invitation")
        );
        assert_eq!(observed.load(Ordering::SeqCst), 1);
        assert_eq!(runtime.accept_invitation_call_count(), 1);
        assert_eq!(clock.now_ms(), 100);
        assert_component_host_terminal(&app, &instance, SemanticOperationPhase::Succeeded).await;
    }

    #[tokio::test]
    async fn public_import_host_prepare_failure_preserves_source_without_commit() {
        let (app, runtime, transfer, clock, supervisor) =
            component_public_import_host_fixture().await;
        let native = Arc::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "original host preparation denied",
        ));
        let expected = native.clone();
        let instance =
            crate::ui_contract::OperationInstanceId("prepared-host-native-failure".into());
        let outcome = super::super::enrollment_import_owner::test_support::run(
            &app,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::Prepare(Arc::new(move |_completion| {
                let native = native.clone();
                let clock = clock.clone();
                Box::pin(async move {
                    clock
                        .fail_next_observation(
                            aura_core::effects::TimeError::AbsoluteDeadlineUnsupported,
                        )
                        .await;
                    Err(AuraError::Storage {
                        message: "actual component host preparation failed".into(),
                        source: Some(native),
                    })
                })
            })),
        )
        .await;
        let custody_report =
            super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert!(custody_report.domain_failure.is_some());
        let error = outcome.result.unwrap_err();
        let actual = error
            .source()
            .expect("original enrollment fixture prerequisite")
            .downcast_ref::<std::io::Error>()
            .expect("original enrollment fixture prerequisite");
        assert!(std::ptr::eq(actual, expected.as_ref()));
        assert_eq!(runtime.accept_invitation_call_count(), 1);
        assert_component_host_terminal(&app, &instance, SemanticOperationPhase::Failed).await;
    }

    #[tokio::test]
    async fn public_import_host_prepare_postread_failure_never_commits() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (app, runtime, transfer, clock, supervisor) =
            component_public_import_host_fixture().await;
        let commits = Arc::new(AtomicUsize::new(0));
        let observed = commits.clone();
        let native = Arc::new(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "original host final observation failed",
        ));
        let expected = native.clone();
        let instance =
            crate::ui_contract::OperationInstanceId("prepared-host-postread-failure".into());
        let outcome = super::super::enrollment_import_owner::test_support::run(
            &app,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::Prepare(Arc::new(move |_completion| {
                let commits = commits.clone();
                let clock = clock.clone();
                let native = native.clone();
                Box::pin(async move {
                    clock
                        .fail_next_observation(aura_core::effects::TimeError::ProviderFailure {
                            operation:
                                aura_core::effects::time::TimeProviderOperation::ReadPhysicalClock,
                            source: Some(native),
                        })
                        .await;
                    Ok(PreparedEnrollmentHostCommit::new(move || {
                        commits.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }))
                })
            })),
        )
        .await;
        let custody_report =
            super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert!(custody_report.domain_failure.is_some());
        let error = outcome.result.unwrap_err();
        let mut source: Option<&(dyn Error + 'static)> = Some(&error);
        let mut found = false;
        while let Some(actual) = source {
            if let Some(io) = actual.downcast_ref::<std::io::Error>() {
                found = std::ptr::eq(io, expected.as_ref());
                break;
            }
            source = actual.source();
        }
        assert!(
            found,
            "original provider native source must survive: {error:?}"
        );
        assert_eq!(observed.load(Ordering::SeqCst), 0);
        assert_eq!(runtime.accept_invitation_call_count(), 1);
        assert_component_host_terminal(&app, &instance, SemanticOperationPhase::Failed).await;
    }

    #[tokio::test]
    async fn public_import_host_prepare_drop_never_commits() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        struct PreparedDrop(Arc<AtomicBool>);
        impl Drop for PreparedDrop {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let (app, runtime, transfer, clock, supervisor) =
            component_public_import_host_fixture().await;
        let commits = Arc::new(AtomicUsize::new(0));
        let observed = commits.clone();
        let dropped = Arc::new(AtomicBool::new(false));
        let disposal = dropped.clone();
        let (entered, ready) = tokio::sync::oneshot::channel();
        let entered = Arc::new(async_lock::Mutex::new(Some(entered)));
        let instance = crate::ui_contract::OperationInstanceId("prepared-host-drop".into());
        let original = Box::pin(super::super::enrollment_import_owner::test_support::run(
            &app,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::Prepare(Arc::new(move |_completion| {
                let commits = commits.clone();
                let witness = PreparedDrop(dropped.clone());
                let entered = entered
                    .try_lock()
                    .expect("fixture access has no competing borrower")
                    .take()
                    .expect("original enrollment fixture prerequisite");
                Box::pin(async move {
                    let prepared = PreparedEnrollmentHostCommit::new(move || {
                        let _witness = witness;
                        commits.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    });
                    entered
                        .send(())
                        .expect("original enrollment fixture prerequisite");
                    futures::future::pending::<()>().await;
                    Ok(prepared)
                })
            })),
        ));
        match futures::future::select(original, ready).await {
            futures::future::Either::Right((Ok(()), original)) => drop(original),
            _ => panic!("actual prepared host boundary must be entered"),
        }
        let custody_report =
            super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert!(matches!(
            custody_report.execution,
            aura_core::effects::task::RequiredTaskExit::Cancelled
        ));
        assert!(custody_report.domain_failure.is_none());
        assert_component_host_terminal(&app, &instance, SemanticOperationPhase::Cancelled).await;
        assert!(disposal.load(Ordering::SeqCst));
        assert_eq!(observed.load(Ordering::SeqCst), 0);
        assert_eq!(clock.now_ms(), 100);
        assert_eq!(runtime.accept_invitation_call_count(), 1);
        let facts = crate::workflows::signals::read_signal_or_default(
            &app,
            &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        )
        .await;
        assert!(!facts.iter().any(|fact| matches!(fact,crate::ui_contract::AuthoritativeSemanticFact::OperationStatus {instance_id,status,..} if instance_id.as_ref()==Some(&instance) && status.phase==SemanticOperationPhase::Succeeded)));
    }

    #[tokio::test]
    async fn public_import_cancelled_after_arm_before_execute_has_original_terminal_and_disposal() {
        let (app, runtime, transfer, clock, supervisor) =
            component_public_import_host_fixture().await;
        supervisor.pause_execution();
        let instance = OperationInstanceId("armed-before-execute-cancel".into());
        let observation = super::super::enrollment_import_owner::test_support::admit(
            &app,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::RuntimeOnly,
        )
        .await;
        drop(observation);
        let report = super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert!(matches!(
            report.execution,
            aura_core::effects::task::RequiredTaskExit::Cancelled
        ));
        assert!(report.domain_failure.is_none());
        assert_component_host_terminal(&app, &instance, SemanticOperationPhase::Cancelled).await;
        assert_eq!(runtime.accept_invitation_call_count(), 0);
        assert!(runtime.enrollment_import_requests().await.is_empty());
        assert_eq!(clock.now_ms(), 100);
    }

    #[tokio::test]
    async fn public_import_host_acknowledged_commit_wins_observer_cancellation() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (app, _runtime, transfer, clock, supervisor) =
            component_public_import_host_fixture().await;
        let commits = Arc::new(AtomicUsize::new(0));
        let count = commits.clone();
        let (committed, observed) = tokio::sync::oneshot::channel();
        let committed = Arc::new(async_lock::Mutex::new(Some(committed)));
        let instance = OperationInstanceId("host-ack-before-observer-drop".into());
        let observation = super::super::enrollment_import_owner::test_support::admit(
            &app,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::Prepare(Arc::new(move |_| {
                let count = count.clone();
                let sender = committed
                    .try_lock()
                    .expect("fixture access has no competing borrower")
                    .take()
                    .expect("original enrollment fixture prerequisite");
                Box::pin(async move {
                    Ok(PreparedEnrollmentHostCommit::new(move || {
                        count.fetch_add(1, Ordering::SeqCst);
                        sender
                            .send(())
                            .expect("original enrollment fixture prerequisite");
                        Ok(())
                    }))
                })
            })),
        )
        .await;
        observed
            .await
            .expect("actual synchronous host commit entered");
        drop(observation);
        let report = super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert!(report.domain_failure.is_none());
        assert_eq!(commits.load(Ordering::SeqCst), 1);
        assert_component_host_terminal(&app, &instance, SemanticOperationPhase::Succeeded).await;
        assert_eq!(clock.now_ms(), 100);
    }

    #[tokio::test]
    async fn public_import_live_unarmed_shutdown_refuses_late_handoff_with_original_publisher() {
        let (app, runtime, transfer, clock, supervisor) =
            component_public_import_host_fixture().await;
        supervisor.pause_execution();
        let admission = app
            .read()
            .await
            .enrollment_import_admission()
            .cloned()
            .expect("original enrollment fixture prerequisite");
        let instance = OperationInstanceId("live-unarmed-original".into());
        let (prior, prior_events) =
            super::super::enrollment_import_owner::test_support::prior_submission(instance.clone());
        let mut observation = import_device_enrollment_with_terminal_status(
            &app,
            &admission,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::RuntimeOnly,
        )
        .unwrap_or_else(|refusal| {
            panic!("unexpected admission refusal: {:?}", refusal.source_error())
        });
        supervisor.shutdown();
        // The observer and its arm sender are still alive at actual cleanup ACK.
        let report = super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert!(matches!(
            report.execution,
            aura_core::effects::task::RequiredTaskExit::Cancelled
        ));
        assert!(report.domain_failure.is_none());
        let (prior, error) = observation
            .accept_handoff(prior)
            .expect_err("disposed unarmed owner refuses late handoff");
        assert_eq!(prior.instance_id(), &instance);
        assert!(prior_events.lock().await.is_empty());
        let receipt = prior
            .fail_with_custody(error.semantic_error(), Some(Box::new(error)))
            .await
            .expect("original enrollment fixture prerequisite");
        assert_eq!(receipt.operation_id, OperationId::device_enrollment());
        assert_eq!(receipt.instance_id, instance);
        assert_eq!(
            receipt.scope,
            crate::frontend_primitives::SubmittedTerminalAcknowledgmentScope::ComponentObserved
        );
        let events = prior_events.lock().await.clone();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, instance);
        assert_eq!(events[0].1.phase, SemanticOperationPhase::Failed);
        drop(events);
        let facts = crate::workflows::signals::read_signal_or_default(
            &app,
            &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        )
        .await;
        assert!(!facts.iter().any(|fact| matches!(fact, crate::ui_contract::AuthoritativeSemanticFact::OperationStatus { instance_id, .. } if instance_id.as_ref()==Some(&instance))));
        assert_eq!(runtime.accept_invitation_call_count(), 0);
        assert_eq!(clock.now_ms(), 100);
        drop(observation);
    }

    #[tokio::test]
    async fn public_import_unarmed_shutdown_preserves_prior_submission_without_app_terminal() {
        let (app, runtime, transfer, clock, supervisor) =
            component_public_import_host_fixture().await;
        supervisor.pause_execution();
        let admission = app
            .read()
            .await
            .enrollment_import_admission()
            .cloned()
            .expect("original enrollment fixture prerequisite");
        let instance = OperationInstanceId("unarmed-shutdown".into());
        let (prior, prior_events) =
            super::super::enrollment_import_owner::test_support::prior_submission(instance.clone());
        let observation = import_device_enrollment_with_terminal_status(
            &app,
            &admission,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::RuntimeOnly,
        )
        .unwrap_or_else(|refusal| {
            panic!("unexpected admission refusal: {:?}", refusal.source_error())
        });
        supervisor.shutdown();
        let refusal = match import_device_enrollment_with_terminal_status(
            &app,
            &admission,
            "original-enrollment-code".into(),
            None,
            Some(instance.clone()),
            EnrollmentImportHostPolicy::RuntimeOnly,
        ) {
            Err(refusal) => refusal,
            Ok(_) => panic!("same shut down supervisor must refuse required admission"),
        };
        // The original submission retains its publisher through actual failure
        // publication; the rejected box is retained until that acknowledgment.
        let receipt = prior
            .fail_with_custody(refusal.semantic_error(), Some(Box::new(refusal)))
            .await
            .expect("original enrollment fixture prerequisite");
        assert_eq!(receipt.operation_id, OperationId::device_enrollment());
        assert_eq!(receipt.instance_id, instance);
        assert_eq!(
            receipt.scope,
            crate::frontend_primitives::SubmittedTerminalAcknowledgmentScope::ComponentObserved
        );
        let events = prior_events.lock().await.clone();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, instance);
        assert_eq!(events[0].1.phase, SemanticOperationPhase::Failed);
        drop(events);
        // No handoff occurred. Abandoning the observer closes its original arm
        // sender; it cannot allocate an app semantic terminal during cleanup.
        drop(observation);
        let report = super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert!(matches!(
            report.execution,
            aura_core::effects::task::RequiredTaskExit::Cancelled
        ));
        assert!(report.domain_failure.is_none());
        let facts = crate::workflows::signals::read_signal_or_default(
            &app,
            &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        )
        .await;
        assert!(!facts.iter().any(|fact|matches!(fact,crate::ui_contract::AuthoritativeSemanticFact::OperationStatus {instance_id,..} if instance_id.as_ref()==Some(&instance))));
        assert_eq!(runtime.accept_invitation_call_count(), 0);
        assert_eq!(clock.now_ms(), 100);
    }

    #[tokio::test]
    async fn public_import_foreign_and_repeated_arm_preserve_one_original_publisher() {
        let (app, runtime, transfer, clock, supervisor) =
            component_public_import_host_fixture().await;
        supervisor.pause_execution();
        let admission = app
            .read()
            .await
            .enrollment_import_admission()
            .cloned()
            .expect("original enrollment fixture prerequisite");
        let instance = OperationInstanceId("exact-original-arm".into());
        let mut observation = import_device_enrollment_with_terminal_status(
            &app,
            &admission,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::RuntimeOnly,
        )
        .unwrap_or_else(|refusal| {
            panic!("unexpected admission refusal: {:?}", refusal.source_error())
        });
        let (foreign, foreign_events) =
            super::super::enrollment_import_owner::test_support::prior_submission(
                OperationInstanceId("foreign-arm".into()),
            );
        let foreign_release = foreign.handoff_to_workflow();
        assert!(
            super::super::enrollment_import_owner::test_support::arm_for_test(
                &mut observation,
                &foreign_release
            )
            .is_err()
        );
        assert!(foreign_events
            .try_lock()
            .expect("fixture access has no competing borrower")
            .is_empty());
        let (original, prior_events) =
            super::super::enrollment_import_owner::test_support::prior_submission(instance.clone());
        let release = original.handoff_to_workflow();
        super::super::enrollment_import_owner::test_support::arm_for_test(
            &mut observation,
            &release,
        )
        .expect("actual original release still arms after foreign refusal");
        assert!(
            super::super::enrollment_import_owner::test_support::arm_for_test(
                &mut observation,
                &release
            )
            .is_err()
        );
        assert!(prior_events.lock().await.is_empty());
        drop(observation);
        let report = super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert!(matches!(
            report.execution,
            aura_core::effects::task::RequiredTaskExit::Cancelled
        ));
        assert_component_host_terminal(&app, &instance, SemanticOperationPhase::Cancelled).await;
        assert_eq!(runtime.accept_invitation_call_count(), 0);
        assert_eq!(clock.now_ms(), 100);
    }

    #[tokio::test]
    async fn public_import_foreign_app_origin_refuses_same_runtime_without_preparation() {
        let (original_app, runtime, transfer, clock, supervisor) =
            component_public_import_host_fixture().await;
        let admission = original_app
            .read()
            .await
            .enrollment_import_admission()
            .cloned()
            .expect("original enrollment fixture prerequisite");
        let foreign_core = AppCore::with_runtime(crate::AppConfig::default(), runtime.clone())
            .expect("original enrollment fixture prerequisite");
        crate::signal_defs::register_app_signals(&foreign_core)
            .await
            .expect("original enrollment fixture prerequisite");
        let foreign_app = Arc::new(RwLock::new(foreign_core));
        let instance = OperationInstanceId("same-runtime-foreign-app".into());
        let mut observation = import_device_enrollment_with_terminal_status(
            &foreign_app,
            &admission,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::RuntimeOnly,
        )
        .unwrap_or_else(|refusal| {
            panic!(
                "unexpected task admission refusal: {:?}",
                refusal.source_error()
            )
        });
        let (prior, _) =
            super::super::enrollment_import_owner::test_support::prior_submission(instance.clone());
        observation
            .accept_handoff(prior)
            .unwrap_or_else(|(_, error)| panic!("same original handoff: {error}"));
        let outcome = observation.await;
        let error = outcome.result.unwrap_err();
        assert!(error
            .to_string()
            .contains("foreign enrollment import app/runtime admission"));
        let report = super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert_eq!(
            report
                .domain_failure
                .expect("original enrollment fixture prerequisite")
                .to_string(),
            error.to_string()
        );
        assert_component_host_terminal(&foreign_app, &instance, SemanticOperationPhase::Failed)
            .await;
        assert!(runtime.enrollment_import_requests().await.is_empty());
        assert_eq!(runtime.accept_invitation_call_count(), 0);
        assert_eq!(clock.now_ms(), 100);
    }

    #[tokio::test]
    async fn public_import_detached_origin_refuses_reattachment_and_stale_admission() {
        let (app, runtime, transfer, clock, supervisor) =
            component_public_import_host_fixture().await;
        let admission = app
            .read()
            .await
            .enrollment_import_admission()
            .cloned()
            .expect("original enrollment fixture prerequisite");
        assert!(AppCore::detach_runtime(&app).await);
        assert!(app
            .write()
            .await
            .attach_bootstrap_runtime(runtime.clone())
            .is_err());
        let instance = OperationInstanceId("detached-original-admission".into());
        let mut observation = import_device_enrollment_with_terminal_status(
            &app,
            &admission,
            "original-enrollment-code".into(),
            Some(transfer),
            Some(instance.clone()),
            EnrollmentImportHostPolicy::RuntimeOnly,
        )
        .unwrap_or_else(|refusal| {
            panic!(
                "unexpected task admission refusal: {:?}",
                refusal.source_error()
            )
        });
        let (prior, _) =
            super::super::enrollment_import_owner::test_support::prior_submission(instance.clone());
        observation
            .accept_handoff(prior)
            .unwrap_or_else(|(_, error)| panic!("same original handoff: {error}"));
        let outcome = observation.await;
        let error = outcome.result.unwrap_err();
        assert!(error
            .to_string()
            .contains("foreign enrollment import app/runtime admission"));
        let report = super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert_eq!(
            report
                .domain_failure
                .expect("original enrollment fixture prerequisite")
                .to_string(),
            error.to_string()
        );
        assert_component_host_terminal(&app, &instance, SemanticOperationPhase::Failed).await;
        assert!(app.read().await.runtime().is_none());
        assert!(runtime.enrollment_import_requests().await.is_empty());
        assert_eq!(runtime.accept_invitation_call_count(), 0);
        assert_eq!(clock.now_ms(), 100);
    }

    #[tokio::test]
    async fn canonical_acceptance_returns_prepared_value_and_publishes_only_original_instance() {
        use futures::FutureExt;
        let authority = AuthorityId::new_from_entropy([171; 32]);
        let mut runtime = crate::runtime_bridge::OfflineRuntimeBridge::new(authority);
        runtime.use_time_provider(Arc::new(aura_testkit::time::ManualPhysicalClock::new(100)));
        runtime.set_settings_snapshot(crate::runtime_bridge::OfflineSettingsSnapshot::default());
        runtime.queue_sync_answers((0..16).map(|_| async { Ok(()) }.boxed()).collect());
        runtime.set_process_ceremony_result(Ok(
            crate::runtime_bridge::CeremonyProcessingOutcome::NoProgress,
        ));
        let invitation_id = aura_core::InvitationId::new("original-prepared-enrollment");
        runtime.set_accept_invitation_result(Ok(
            crate::runtime_bridge::InvitationMutationOutcome {
                invitation_id: invitation_id.clone(),
                new_status: crate::runtime_bridge::InvitationBridgeStatus::Accepted,
            },
        ));
        let runtime = Arc::new(runtime);
        let core = AppCore::with_runtime(crate::AppConfig::default(), runtime.clone())
            .expect("original enrollment fixture prerequisite");
        crate::signal_defs::register_app_signals(&core)
            .await
            .expect("original enrollment fixture prerequisite");
        let app = Arc::new(RwLock::new(core));
        let instance = crate::ui_contract::OperationInstanceId("original-prepared-instance".into());
        let owner = SemanticWorkflowOwner::new(
            &app,
            OperationId::device_enrollment(),
            Some(instance.clone()),
            SemanticOperationKind::ImportDeviceEnrollmentCode,
        );
        let invitation = InvitationInfo {
            invitation_id,
            sender_id: AuthorityId::new_from_entropy([172; 32]),
            receiver_id: authority,
            invitation_type: InvitationBridgeType::DeviceEnrollment {
                subject_authority: authority,
                initiator_device_id: aura_core::DeviceId::new_from_entropy([173; 32]),
                device_id: aura_core::DeviceId::new_from_entropy([174; 32]),
                nickname_suggestion: None,
                ceremony_id: aura_core::CeremonyId::new("original-prepared-ceremony"),
                pending_epoch: aura_core::Epoch(1),
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 100,
            expires_at_ms: None,
            message: None,
            receiver_nickname: None,
        };
        let prepared = Box::new(37_u64);
        let original_address = (&*prepared) as *const u64;
        let returned = accept_device_enrollment_invitation_owned(
            &app,
            &owner,
            async { Ok((invitation, prepared)) },
            None,
        )
        .await
        .expect("original enrollment fixture prerequisite");
        assert_eq!((&*returned) as *const u64, original_address);
        assert_eq!(runtime.accept_invitation_call_count(), 1);
        let terminal = owner
            .terminal_status()
            .await
            .expect("original enrollment fixture prerequisite");
        assert_eq!(terminal.status.phase, SemanticOperationPhase::Succeeded);
        let facts = crate::workflows::signals::read_signal_or_default(
            &app,
            &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        )
        .await;
        let terminal_instances: Vec<_> = facts
            .iter()
            .filter_map(|fact| match fact {
                crate::ui_contract::AuthoritativeSemanticFact::OperationStatus {
                    operation_id,
                    instance_id,
                    status,
                    ..
                } if operation_id == &OperationId::device_enrollment()
                    && matches!(
                        status.phase,
                        SemanticOperationPhase::Succeeded
                            | SemanticOperationPhase::Failed
                            | SemanticOperationPhase::Cancelled
                    ) =>
                {
                    Some(instance_id.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(terminal_instances, vec![Some(instance)]);
    }

    #[tokio::test]
    async fn unpinned_import_fails_before_native_import_and_publishes_terminal_failure() {
        let (app, runtime, _transfer, _clock, supervisor) =
            component_public_import_host_fixture().await;
        let outcome = super::super::enrollment_import_owner::test_support::run(
            &app,
            "untrusted payload".to_owned(),
            None,
            Some(crate::ui_contract::OperationInstanceId(
                "missing-manifest-pin".to_owned(),
            )),
            crate::workflows::invitation::EnrollmentImportHostPolicy::RuntimeOnly,
        )
        .await;
        let custody_report =
            super::super::enrollment_import_owner::test_support::drain(&supervisor).await;
        assert!(custody_report.domain_failure.is_some());
        let error = outcome.result.unwrap_err();
        assert!(matches!(
            error
                .source()
                .expect("original enrollment fixture prerequisite")
                .downcast_ref::<aura_invitation::enrollment_manifest::EnrollmentManifestError>(),
            Some(aura_invitation::enrollment_manifest::EnrollmentManifestError::MissingPin)
        ));
        assert_eq!(
            outcome
                .terminal
                .expect("original enrollment fixture prerequisite")
                .status
                .phase,
            SemanticOperationPhase::Failed
        );
        assert!(runtime.enrollment_import_requests().await.is_empty());
        assert_eq!(runtime.accept_invitation_call_count(), 0);
    }

    #[tokio::test]
    async fn device_accept_failure_settles_existing_owner_and_retains_cause() {
        let core = AppCore::new(crate::AppConfig::default())
            .expect("original enrollment fixture prerequisite");
        crate::signal_defs::register_app_signals(&core)
            .await
            .expect("original enrollment fixture prerequisite");
        let app_core = Arc::new(RwLock::new(core));
        let owner = SemanticWorkflowOwner::new(
            &app_core,
            OperationId::device_enrollment(),
            Some(crate::ui_contract::OperationInstanceId(
                "device-accept-failure-owner".into(),
            )),
            SemanticOperationKind::ImportDeviceEnrollmentCode,
        );
        owner
            .publish_phase(SemanticOperationPhase::WorkflowDispatched)
            .await
            .expect("original enrollment fixture prerequisite");
        let source = AuraError::Storage {
            message: "enrolled runtime did not settle".into(),
            source: Some(Arc::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "journal denied",
            ))),
        };
        let returned = fail_device_enrollment_accept::<()>(&owner, source)
            .await
            .unwrap_err();
        assert_eq!(returned.category(), "storage");
        assert_eq!(
            returned
                .source()
                .expect("original enrollment fixture prerequisite")
                .downcast_ref::<std::io::Error>()
                .expect("original enrollment fixture prerequisite")
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
        let terminal = owner
            .terminal_status()
            .await
            .expect("original enrollment fixture prerequisite");
        assert_eq!(terminal.status.phase, SemanticOperationPhase::Failed);
        assert_eq!(
            terminal.status.kind,
            SemanticOperationKind::ImportDeviceEnrollmentCode
        );
        let facts = crate::workflows::signals::read_signal_or_default(
            &app_core,
            &*crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        )
        .await;
        crate::workflows::semantic_facts::assert_terminal_failure_or_cancelled(
            &facts,
            &OperationId::device_enrollment(),
            &crate::ui_contract::OperationInstanceId("device-accept-failure-owner".into()),
            SemanticOperationKind::ImportDeviceEnrollmentCode,
        );
    }
}

fn log_device_enrollment_accept_progress(message: impl Into<String>) {
    let message = message.into();
    #[cfg(feature = "wasm")]
    crate::platform::wasm::console_log(&format!("[device-enrollment-accept] {message}"));
    #[cfg(all(not(feature = "wasm"), feature = "instrumented"))]
    tracing::info!(target: "device-enrollment-accept", "{message}");
    #[cfg(all(not(feature = "wasm"), not(feature = "instrumented")))]
    let _ = message;
}

pub(super) async fn fail_device_enrollment_accept<T>(
    owner: &SemanticWorkflowOwner,
    error: AuraError,
) -> Result<T, AuraError> {
    let failure = AcceptInvitationError::AcceptFailed {
        detail: error.to_string(),
        source: Some(error.clone()),
    };
    owner
        .publish_failure(failure.semantic_error(owner.kind()))
        .await?;
    Err(error)
}

async fn prime_device_enrollment_accept_connectivity(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    budget: &TimeoutBudget,
) -> Result<(), AuraError> {
    drive_invitation_accept_convergence(runtime, None, budget)
        .await
        .map_err(AuraError::from)
}

/// Observed result minted after authenticated enrollment acceptance.
///
/// Frontends cannot construct a completion from received identity fields.
/// ```compile_fail
/// use aura_app::ui::workflows::invitation::DeviceEnrollmentImportCompleted;
/// let fabricated=DeviceEnrollmentImportCompleted {
///     subject_authority:aura_core::AuthorityId::new_from_entropy([1;32]),
///     device_id:aura_core::DeviceId::new_from_entropy([2;32]),
/// };
/// ```

#[derive(Debug, Clone)]
pub struct DeviceEnrollmentImportCompleted {
    subject_authority: AuthorityId,
    device_id: aura_core::DeviceId,
    original_provisional: AuthorityId,
    invitation_id: aura_core::InvitationId,
    ceremony_id: aura_core::CeremonyId,
    pending_epoch: u64,
    manifest_digest: [u8; 32],
    setup_digest: [u8; 32],
}
impl DeviceEnrollmentImportCompleted {
    pub fn subject_authority(&self) -> AuthorityId {
        self.subject_authority
    }
    pub fn device_id(&self) -> aura_core::DeviceId {
        self.device_id
    }
    /// Diagnostic/receipt locator fields identify durable evidence. They never
    /// authorize profile migration without the actual runtime receipt owner.
    pub fn original_provisional(&self) -> AuthorityId {
        self.original_provisional
    }
    pub fn invitation_id(&self) -> &aura_core::InvitationId {
        &self.invitation_id
    }
    pub fn ceremony_id(&self) -> &aura_core::CeremonyId {
        &self.ceremony_id
    }
    pub fn pending_epoch(&self) -> u64 {
        self.pending_epoch
    }
    pub fn manifest_digest(&self) -> [u8; 32] {
        self.manifest_digest
    }
    pub fn setup_digest(&self) -> [u8; 32] {
        self.setup_digest
    }
}

/// Host storage acknowledgment required before the original import owner succeeds.
#[derive(Clone)]
pub enum EnrollmentImportHostPolicy {
    /// The host has no additional identity storage to acknowledge.
    RuntimeOnly,
    /// Borrow the original completion; a host cannot replace its sealed bindings.
    Finalize(EnrollmentImportHostCallback<DeviceEnrollmentImportCompleted>),
    /// Prepare without changing selected identity, then synchronously commit after
    /// the original selected budget acknowledges its final observation.
    Prepare(EnrollmentImportHostPrepareCallback<DeviceEnrollmentImportCompleted>),
}

pub type EnrollmentImportHostCallback<T> =
    Arc<dyn for<'a> Fn(&'a T) -> EnrollmentImportHostFuture<'a> + Send + Sync>;
#[cfg(not(target_arch = "wasm32"))]
pub type EnrollmentImportHostFuture<'a> = futures::future::BoxFuture<'a, Result<(), AuraError>>;
#[cfg(target_arch = "wasm32")]
pub type EnrollmentImportHostFuture<'a> =
    futures::future::LocalBoxFuture<'a, Result<(), AuraError>>;

pub type EnrollmentImportHostPrepareCallback<T> =
    Arc<dyn for<'a> Fn(&'a T) -> EnrollmentImportHostPrepareFuture<'a> + Send + Sync>;
#[cfg(not(target_arch = "wasm32"))]
pub type EnrollmentImportHostPrepareFuture<'a> =
    futures::future::BoxFuture<'a, Result<PreparedEnrollmentHostCommit, AuraError>>;
#[cfg(target_arch = "wasm32")]
pub type EnrollmentImportHostPrepareFuture<'a> =
    futures::future::LocalBoxFuture<'a, Result<PreparedEnrollmentHostCommit, AuraError>>;

/// Move-owned host work, not an enrollment or storage-success proof. The actual
/// commit result remains independently fallible, including partial-write errors.
pub struct PreparedEnrollmentHostCommit {
    commit: Box<dyn FnOnce() -> Result<(), AuraError> + Send>,
}
impl PreparedEnrollmentHostCommit {
    pub fn new(commit: impl FnOnce() -> Result<(), AuraError> + Send + 'static) -> Self {
        Self {
            commit: Box::new(commit),
        }
    }
    fn commit(self) -> Result<(), AuraError> {
        (self.commit)()
    }
}

pub(super) enum EnrollmentHostFinalization<T> {
    Async(EnrollmentImportHostCallback<T>),
    Prepared(EnrollmentImportHostPrepareCallback<T>),
}

pub fn import_device_enrollment_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    admission: &super::enrollment_import_owner::EnrollmentImportAdmission,
    code: String,
    transfer: Option<crate::ui_contract::EnrollmentManifestTransferInput>,
    instance_id: Option<crate::ui_contract::OperationInstanceId>,
    host: EnrollmentImportHostPolicy,
) -> Result<
    super::enrollment_import_owner::EnrollmentImportObservation,
    super::enrollment_import_owner::EnrollmentImportAdmissionRefusal,
> {
    super::enrollment_import_owner::admit_import(
        app_core,
        admission,
        code,
        transfer,
        instance_id,
        host,
    )
}

/// A new app operation may retry only host persistence after the original
/// terminal Failed. The runtime revalidates both the signed confirmation and
/// the current active generation; native enrollment is never replayed.
pub async fn retry_enrollment_host_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    admission: &super::enrollment_import_owner::EnrollmentImportAdmission,
    recovery: super::enrollment_import_owner::EnrollmentHostRecovery,
    instance: crate::ui_contract::OperationInstanceId,
    host: EnrollmentImportHostPolicy,
) -> crate::ui_contract::WorkflowTerminalOutcome<DeviceEnrollmentImportCompleted> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment(),
        Some(instance.clone()),
        SemanticOperationKind::ImportDeviceEnrollmentCode,
    );
    let result =
        retry_enrollment_host_owned(app_core, admission, recovery, instance, host, &owner).await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

#[aura_macros::semantic_owner(
    owner = "retry_enrollment_host_owned",
    wrapper = "retry_enrollment_host_with_terminal_status",
    terminal = "publish_success_with",
    postcondition = "device_enrollment_imported",
    proof = crate::workflows::semantic_facts::DeviceEnrollmentImportedProof,
    authoritative_inputs = "failed_original_recovery,authenticated_current_generation,host_acknowledgment",
    depends_on = "runtime_verified_enrollment_confirmation,active_signing_generation,host_storage_acknowledgment",
    child_ops = "",
    category = "move_owned"
)]
async fn retry_enrollment_host_owned(
    app_core: &Arc<RwLock<AppCore>>,
    admission: &super::enrollment_import_owner::EnrollmentImportAdmission,
    recovery: super::enrollment_import_owner::EnrollmentHostRecovery,
    instance: crate::ui_contract::OperationInstanceId,
    host: EnrollmentImportHostPolicy,
    owner: &SemanticWorkflowOwner<OperationContext<OperationId, OperationInstanceId, TraceContext>>,
) -> Result<DeviceEnrollmentImportCompleted, AuraError> {
    let attempted = async {
        if instance == recovery.failed_instance {
            return Err(AuraError::invalid(
                "host recovery requires a new operation instance",
            ));
        }
        let runtime = {
            let core = app_core.read().await;
            admission.require_core(&core)?;
            admission.runtime.clone()
        };
        owner
            .publish_phase(SemanticOperationPhase::WorkflowDispatched)
            .await?;
        let budget = workflow_timeout_budget(&runtime, DEVICE_ENROLLMENT_ACCEPT_TIMEOUT).await?;
        execute_with_runtime_timeout_budget(&runtime, &budget, || {
            runtime.authenticate_enrollment_host_recovery(&recovery.completion)
        })
        .await
        .map_err(|error| match error {
            TimeoutRunError::Operation(source) => {
                AuraError::from(crate::workflows::error::runtime_call(
                    "authenticate enrollment host recovery",
                    source,
                ))
            }
            TimeoutRunError::Timeout(source) => AuraError::from(source),
        })?;
        match host {
            EnrollmentImportHostPolicy::RuntimeOnly => {
                return Err(AuraError::invalid(
                    "host recovery requires a host commit owner",
                ));
            }
            EnrollmentImportHostPolicy::Finalize(callback) => {
                execute_with_runtime_timeout_budget(&runtime, &budget, || {
                    callback(&recovery.completion)
                })
                .await
                .map_err(|error| match error {
                    TimeoutRunError::Operation(source) => source,
                    TimeoutRunError::Timeout(source) => source.into(),
                })?;
            }
            EnrollmentImportHostPolicy::Prepare(callback) => {
                aura_core::time::timeout::acknowledge_with_timeout_budget(
                    &runtime.physical_time_provider(),
                    &budget,
                    || async {
                        Ok::<_, aura_core::TimeoutBudgetError>(callback(&recovery.completion).await)
                    },
                    |prepared| prepared.and_then(|commit| commit.commit()),
                )
                .await
                .map_err(AuraError::from)??;
            }
        }
        owner
            .publish_success_with(issue_device_enrollment_imported_proof(
                recovery.completion.invitation_id().clone(),
            ))
            .await?;
        Ok(recovery.completion)
    }
    .await;
    match attempted {
        Ok(completion) => Ok(completion),
        Err(error) => fail_device_enrollment_accept(owner, error).await,
    }
}

pub(super) async fn prepare_device_enrollment_import(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    budget: &aura_core::TimeoutBudget,
    code: String,
    transfer: Option<crate::ui_contract::EnrollmentManifestTransferInput>,
) -> Result<(InvitationInfo, DeviceEnrollmentImportCompleted), AuraError> {
    use aura_invitation::enrollment_manifest::EnrollmentManifestError;
    let prepare = async {
        let transfer = transfer.ok_or(EnrollmentManifestError::MissingPin)?;
        let pin =
            crate::workflows::ceremonies::pin_user_transferred_enrollment_manifest_with_budget(
                runtime,
                budget,
                transfer.manifest_code,
                transfer.initiator_verifier_code,
            )
            .await?;
        let subject_authority = pin.manifest().subject;
        let device_id = pin.manifest().invitee_device;
        let original_provisional = pin.manifest().invitee_authority;
        let invitation_id = pin.manifest().invitation.clone();
        let ceremony_id = pin.manifest().ceremony.clone();
        let pending_epoch = pin.manifest().pending_epoch;
        let manifest_digest = pin.digest();
        let setup_digest = pin.manifest().setup.digest;

        let import_runtime = runtime.clone();
        let info = crate::workflows::runtime::timeout_runtime_call_under_budget(
            runtime,
            budget,
            "import_device_enrollment_owned",
            "import_enrollment_invitation",
            move || async move {
                import_runtime
                    .import_enrollment_invitation(&code, pin)
                    .await
            },
        )
        .await
        .map_err(EnrollmentManifestError::Boundary)??;
        Ok::<_, EnrollmentManifestError>((
            info,
            DeviceEnrollmentImportCompleted {
                subject_authority,
                device_id,
                original_provisional,
                invitation_id,
                ceremony_id,
                pending_epoch,
                manifest_digest,
                setup_digest,
            },
        ))
    }
    .await;
    prepare.map_err(|error| AuraError::PermissionDenied {
        message: error.to_string(),
        source: Some(Arc::new(error)),
    })
}

pub async fn accept_device_enrollment_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: &InvitationInfo,
) -> Result<(), AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment(),
        None,
        SemanticOperationKind::ImportDeviceEnrollmentCode,
    );
    accept_device_enrollment_invitation_owned(
        app_core,
        &owner,
        async { Ok((invitation.clone(), ())) },
        None,
    )
    .await
}

#[aura_macros::semantic_owner(
    owner = "accept_device_enrollment_invitation_owned",
    wrapper = "accept_device_enrollment_invitation",
    terminal = "publish_success_with",
    postcondition = "device_enrollment_imported",
    proof = crate::workflows::semantic_facts::DeviceEnrollmentImportedProof,
    authoritative_inputs = "canonical_invitation,runtime",
    depends_on = "runtime_verified_enrollment_confirmation",
    child_ops = "",
    category = "move_owned"
)]
async fn accept_device_enrollment_invitation_owned<T>(
    app_core: &Arc<RwLock<AppCore>>,
    owner: &SemanticWorkflowOwner<OperationContext<OperationId, OperationInstanceId, TraceContext>>,
    preparation: impl std::future::Future<Output = Result<(InvitationInfo, T), AuraError>>,
    finalize: Option<EnrollmentHostFinalization<T>>,
) -> Result<T, AuraError> {
    let mut state = EnrollmentAcceptState::new(false);
    run_enrollment_accept(&mut state, app_core, owner, preparation, finalize).await?;
    let invitation = state
        .invitation
        .as_ref()
        .ok_or_else(|| AuraError::invalid("original invitation missing"))?;
    owner
        .publish_success_with(issue_device_enrollment_imported_proof(
            invitation.invitation_id.clone(),
        ))
        .await?;
    state
        .completion
        .take()
        .ok_or_else(|| AuraError::invalid("original enrollment completion missing"))
}

pub(super) struct EnrollmentAcceptState<T> {
    pub(super) invitation: Option<InvitationInfo>,
    pub(super) completion: Option<T>,
    pub(super) runtime: Option<Arc<dyn crate::runtime_bridge::RuntimeBridge>>,
    pub(super) budget: Option<TimeoutBudget>,
    pub(super) first_failure: Option<AuraError>,
    pub(super) host_acknowledged: bool,
    /// Native acceptance settled, but the selected host finalization failed.
    /// This is the only point from which a separately authenticated host
    /// recovery operation may be issued after the original fails.
    pub(super) host_finalization_failed: bool,
    defer_terminal: bool,
}
impl<T> EnrollmentAcceptState<T> {
    pub(super) fn new(defer_terminal: bool) -> Self {
        Self {
            invitation: None,
            completion: None,
            runtime: None,
            budget: None,
            first_failure: None,
            host_acknowledged: false,
            host_finalization_failed: false,
            defer_terminal,
        }
    }
    async fn fail<U>(
        &mut self,
        owner: &SemanticWorkflowOwner,
        error: AuraError,
    ) -> Result<U, AuraError> {
        let original = self.first_failure.get_or_insert(error).clone();
        if self.defer_terminal {
            Err(original)
        } else {
            fail_device_enrollment_accept(owner, original).await
        }
    }
}

pub(super) async fn run_enrollment_accept<T>(
    state: &mut EnrollmentAcceptState<T>,
    app_core: &Arc<RwLock<AppCore>>,
    owner: &SemanticWorkflowOwner,
    preparation: impl std::future::Future<Output = Result<(InvitationInfo, T), AuraError>>,
    finalize: Option<EnrollmentHostFinalization<T>>,
) -> Result<(), AuraError> {
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    if state.defer_terminal {
        let runtime = state
            .runtime
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original import runtime missing"))?
            .clone();
        let budget = state
            .budget
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original import budget missing"))?
            .clone();
        let prepared = execute_with_runtime_timeout_budget(&runtime, &budget, || async {
            match preparation.await {
                Ok((invitation, completion)) => {
                    state.invitation = Some(invitation);
                    state.completion = Some(completion);
                    Ok(())
                }
                Err(error) => {
                    state.first_failure.get_or_insert_with(|| error.clone());
                    Err(error)
                }
            }
        })
        .await;
        let error = match prepared {
            Ok(()) => None,
            Err(TimeoutRunError::Operation(error)) => Some(error),
            Err(TimeoutRunError::Timeout(error)) => Some(error.into()),
        };
        if let Some(error) = error {
            return state.fail(owner, error).await;
        }
    } else {
        match preparation.await {
            Ok((invitation, completion)) => {
                state.invitation = Some(invitation);
                state.completion = Some(completion);
            }
            Err(error) => return state.fail(owner, error).await,
        }
    }
    let invitation = state
        .invitation
        .as_ref()
        .ok_or_else(|| AuraError::invalid("original invitation missing"))?
        .clone();
    let invitation = &invitation;
    let InvitationBridgeType::DeviceEnrollment { .. } = &invitation.invitation_type else {
        return state
            .fail(
                owner,
                crate::workflows::error::WorkflowError::Precondition(
                    "accept_device_enrollment_invitation requires a device enrollment invitation",
                )
                .into(),
            )
            .await;
    };

    let runtime = if state.defer_terminal {
        state
            .runtime
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original import runtime missing"))?
            .clone()
    } else {
        match require_runtime(app_core).await {
            Ok(runtime) => runtime,
            Err(error) => return state.fail(owner, error).await,
        }
    };
    log_device_enrollment_accept_progress(format!(
        "start invitation_id={};authority={}",
        invitation.invitation_id,
        runtime.authority_id()
    ));
    let budget = match &state.budget {
        Some(original) => original.clone(),
        None => match workflow_timeout_budget(&runtime, DEVICE_ENROLLMENT_ACCEPT_TIMEOUT).await {
            Ok(budget) => budget,
            Err(error) => return state.fail(owner, error.into()).await,
        },
    };
    state.runtime = Some(runtime.clone());
    state.budget = Some(budget.clone());
    // This preflight only warms connectivity; the sealed runtime enrollment
    // choreography remains the required admission and completion boundary.
    if let Err(error) = prime_device_enrollment_accept_connectivity(&runtime, &budget).await {
        tracing::debug!(%error, "device enrollment connectivity preflight unavailable");
    }
    log_device_enrollment_accept_progress(format!(
        "connectivity preflight complete invitation_id={}",
        invitation.invitation_id
    ));
    let accept_result = execute_with_runtime_timeout_budget(&runtime, &budget, || {
        runtime.accept_invitation(invitation.invitation_id.as_str())
    })
    .await;
    if let Err(error) = accept_result {
        return state
            .fail(
                owner,
                crate::workflows::error::runtime_call("accept invitation failed", error).into(),
            )
            .await;
    }
    log_device_enrollment_accept_progress(format!(
        "accept_invitation returned invitation_id={}",
        invitation.invitation_id
    ));

    // The accept above is authoritative: the runtime returns success only after
    // the signed enrollment choreography completed and this device adopted the
    // enrolled epoch. What remains is settling local state; device counts are
    // not evidence of success (the device list includes this device even before
    // the tree does), so they no longer decide the outcome.
    let invitation_id = invitation.invitation_id.clone();
    let enrollment_result: Result<(), DeviceEnrollmentAcceptConvergenceError> = async {
        execute_with_runtime_timeout_budget(&runtime, &budget, || {
            runtime.process_ceremony_messages()
        })
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(error) => {
                DeviceEnrollmentAcceptConvergenceError::Workflow(error.into())
            }
            TimeoutRunError::Operation(error) => DeviceEnrollmentAcceptConvergenceError::Terminal(
                crate::workflows::error::runtime_call(
                    "device enrollment ceremony processing failed",
                    error,
                )
                .into(),
            ),
        })?;
        crate::workflows::runtime::converge_runtime(&runtime, &budget)
            .await
            .map_err(DeviceEnrollmentAcceptConvergenceError::Workflow)?;
        execute_with_runtime_timeout_budget(&runtime, &budget, || {
            settings::refresh_settings_from_runtime(app_core)
        })
        .await
        .map_err(|error| {
            DeviceEnrollmentAcceptConvergenceError::Workflow(match error {
                TimeoutRunError::Timeout(error) => error.into(),
                TimeoutRunError::Operation(error) => error,
            })
        })?;
        log_device_enrollment_accept_progress(format!("settled invitation_id={invitation_id}"));
        if let Err(_error) = execute_with_runtime_timeout_budget(&runtime, &budget, || {
            ensure_runtime_peer_connectivity(&runtime, "device_enrollment_accept")
        })
        .await
        {
            #[cfg(feature = "instrumented")]
            tracing::warn!(
                error = %_error,
                invitation_id = %invitation_id,
                "device enrollment acceptance completed without reachable peers"
            );
        }
        Ok(())
    }
    .await;
    match enrollment_result {
        Ok(()) => {
            if let Some(finalize) = finalize {
                // The actual owner keeps the native cause and commit ACK before
                // callback Ready, independently of the executor's final clock read.
                let Some(completion) = state.completion.as_ref() else {
                    return state
                        .fail(owner, AuraError::invalid("original completion missing"))
                        .await;
                };
                let first_failure = &mut state.first_failure;
                let host_acknowledged = &mut state.host_acknowledged;
                let host_finalization_failed = &mut state.host_finalization_failed;
                let acknowledged = match finalize {
                    EnrollmentHostFinalization::Async(finalize) => {
                        execute_with_runtime_timeout_budget(&runtime, &budget, || async {
                            let result = finalize(completion).await;
                            match &result {
                                // Async native writes may have completed, but
                                // original postvalidation must still acknowledge
                                // them before cancellation can select success.
                                Ok(()) => {}
                                Err(error) => {
                                    first_failure.get_or_insert_with(|| error.clone());
                                    *host_finalization_failed = true;
                                }
                            }
                            result
                        })
                        .await
                    }
                    EnrollmentHostFinalization::Prepared(prepare) => {
                        aura_core::time::timeout::acknowledge_with_timeout_budget(
                            &runtime.physical_time_provider(),
                            &budget,
                            || async {
                                let prepared = prepare(completion).await;
                                if let Err(error) = &prepared {
                                    first_failure.get_or_insert_with(|| error.clone());
                                    *host_finalization_failed = true;
                                }
                                Ok::<_, aura_core::TimeoutBudgetError>(prepared)
                            },
                            |prepared| {
                                prepared.and_then(|prepared| {
                                    prepared.commit()?;
                                    *host_acknowledged = true;
                                    Ok(())
                                })
                            },
                        )
                        .await
                        .map_err(TimeoutRunError::Timeout)
                        .and_then(|result| result.map_err(TimeoutRunError::Operation))
                    }
                };
                if let Some(error) = state.first_failure.clone() {
                    return state.fail(owner, error).await;
                }
                match acknowledged {
                    Ok(()) => {
                        state.host_acknowledged = true;
                    }
                    Err(TimeoutRunError::Operation(error)) => {
                        state.host_finalization_failed = true;
                        return state.fail(owner, error).await;
                    }
                    Err(TimeoutRunError::Timeout(error)) => {
                        state.host_finalization_failed = true;
                        return state.fail(owner, error.into()).await;
                    }
                }
            }
            state.host_acknowledged = true;
            Ok(())
        }
        Err(DeviceEnrollmentAcceptConvergenceError::Terminal(error)) => {
            state.fail(owner, error).await
        }
        Err(DeviceEnrollmentAcceptConvergenceError::Workflow(error)) => {
            #[cfg(feature = "instrumented")]
            tracing::warn!(
                invitation_id = %invitation.invitation_id,
                error = %error,
                "device enrollment acceptance failed while settling the enrolled runtime"
            );
            state
                .fail(
                    owner,
                    crate::workflows::error::runtime_call(
                        "device enrollment acceptance did not settle",
                        error,
                    )
                    .into(),
                )
                .await
        }
    }
}
