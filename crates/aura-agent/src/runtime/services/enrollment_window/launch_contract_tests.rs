use super::*;
use crate::runtime_bridge::enrollment_quorum::OriginalPreparedIssuerReady;
use aura_core::effects::time::TimeProviderOperation;
use aura_core::effects::TimeError;
use aura_testkit::time::ManualPhysicalClock;

fn selected_effects(clock: &ManualPhysicalClock, salt: u64) -> Arc<AuraEffectSystem> {
    let config = crate::AgentConfig {
        device_id: aura_core::DeviceId::new_from_entropy([177; 32]),
        ..Default::default()
    };
    Arc::new(
        AuraEffectSystem::simulation_for_test_for_authority_with_salt(
            &config,
            aura_core::AuthorityId::new_from_entropy([178; 32]),
            salt,
        )
        .expect("deterministic simulator effects")
        .with_physical_time_provider(Arc::new(clock.clone())),
    )
}

#[tokio::test]
async fn original_launch_transfers_same_window_once_and_refuses_foreign_runtime() {
    let clock = ManualPhysicalClock::new(5_000);
    let effects = selected_effects(&clock, 0x350_001);
    let foreign = selected_effects(&clock, 0x350_002);
    assert_eq!(effects.device_id(), foreign.device_id());
    let (mut owner, observation) = OriginalIssuerLaunchWindow::select(effects.clone())
        .await
        .expect("original provider selection");
    let deadline = observation.active.deadline_at_ms();
    let cleanup_deadline = owner
        .birth
        .as_ref()
        .unwrap()
        .negative_cleanup
        .deadline_at_ms();
    assert!(owner.take_original_birth(&foreign).is_err());
    assert!(
        owner.birth.is_some(),
        "foreign input cannot consume selection"
    );
    clock.advance(7);
    let selected = owner
        .take_original_birth(&effects)
        .expect("original transfer");
    assert_eq!(selected.started_at, PhysicalTime::exact(5_000));
    assert_eq!(selected.birth.execution.started_at_ms(), 5_000);
    assert_eq!(selected.birth.execution.deadline_at_ms(), deadline);
    assert_eq!(
        selected.birth.negative_cleanup.deadline_at_ms(),
        cleanup_deadline
    );
    assert!(selected
        .birth
        .execution
        .shares_observation_owner_with(&observation.active));
    assert!(owner.take_original_birth(&effects).is_err());
}

#[tokio::test]
async fn original_launch_ready_slot_survives_observer_drop_and_original_expiry() {
    let clock = ManualPhysicalClock::new(5_000);
    let effects = selected_effects(&clock, 0x350_003);
    let foreign = selected_effects(&clock, 0x350_004);
    let (_owner, observation) = OriginalIssuerLaunchWindow::select(effects.clone())
        .await
        .expect("original launch");
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut receiver = Some(receiver);
    let mut retained = None;
    {
        let mut waiting =
            Box::pin(observation.receive_initial_ready(&effects, &mut receiver, &mut retained));
        assert!(futures::poll!(waiting.as_mut()).is_pending());
    }
    assert!(receiver.is_some());
    assert!(retained.is_none());
    assert!(observation
        .receive_initial_ready(&foreign, &mut receiver, &mut retained)
        .await
        .is_err());
    assert!(receiver.is_some());
    assert!(sender.send(Ok(OriginalPreparedIssuerReady::Issued)).is_ok());
    observation
        .receive_initial_ready(&effects, &mut receiver, &mut retained)
        .await
        .expect("same retained readiness receiver");
    assert!(receiver.is_none());
    assert!(matches!(
        retained,
        Some(Ok(OriginalPreparedIssuerReady::Issued))
    ));
    clock.set_time(observation.active.deadline_at_ms());
    assert!(observation
        .receive_initial_ready(&effects, &mut receiver, &mut retained)
        .await
        .is_err());
    assert!(
        matches!(retained, Some(Ok(OriginalPreparedIssuerReady::Issued))),
        "expiry must not discard the retained readiness projection"
    );
}

#[tokio::test]
async fn original_launch_negative_work_uses_original_cleanup_endpoint() {
    let clock = ManualPhysicalClock::new(5_000);
    let effects = selected_effects(&clock, 0x350_005);
    let foreign = selected_effects(&clock, 0x350_006);
    let (owner, observation) = OriginalIssuerLaunchWindow::select(effects.clone())
        .await
        .expect("original selection");
    let primary = AuraError::invalid("original allocation operation failed");
    let called = std::cell::Cell::new(false);
    let foreign_result = owner
        .execute_original_allocation_cleanup(&foreign, &primary, || async {
            called.set(true);
            Ok(())
        })
        .await;
    assert!(foreign_result.is_err());
    assert!(!called.get());
    clock.set_time(observation.active.deadline_at_ms());
    let outcome = owner
        .execute_original_allocation_cleanup(&effects, &primary, || async {
            called.set(true);
            Ok(())
        })
        .await
        .expect("same original cleanup owner");
    assert!(outcome.validation.is_ok());
    assert!(matches!(outcome.operation, Some(Ok(()))));
    assert!(std::ptr::eq(outcome.primary, &primary));
    assert!(
        called.get(),
        "positive expiry does not replace the allocated cleanup endpoint"
    );
    called.set(false);
    clock.set_time(
        owner
            .birth
            .as_ref()
            .unwrap()
            .negative_cleanup
            .deadline_at_ms(),
    );
    let expired = owner
        .execute_original_allocation_cleanup(&effects, &primary, || async {
            called.set(true);
            Ok(())
        })
        .await
        .expect("original owner remains selected");
    assert!(expired.validation.is_err());
    assert!(expired.operation.is_none());
    assert!(std::ptr::eq(expired.primary, &primary));
    assert!(!called.get());
}

#[tokio::test]
async fn original_launch_selection_retains_concrete_provider_failure() {
    let clock = ManualPhysicalClock::new(5_000);
    let effects = selected_effects(&clock, 0x350_007);
    let native = Arc::new(std::io::Error::other("original selected provider failed"));
    clock
        .fail_next_observation(TimeError::ProviderFailure {
            operation: TimeProviderOperation::ReadPhysicalClock,
            source: Some(native.clone()),
        })
        .await;
    let failed = OriginalIssuerLaunchWindow::select(effects)
        .await
        .err()
        .expect("selected provider failure must refuse allocation");
    let actual = std::error::Error::source(&failed)
        .and_then(std::error::Error::source)
        .and_then(|source| source.downcast_ref::<std::io::Error>())
        .expect("actual concrete native provider cause");
    assert!(std::ptr::eq(actual, native.as_ref()));
}

#[tokio::test]
async fn original_launch_cleanup_checkpoint_failure_retains_original_source() {
    let clock = ManualPhysicalClock::new(5_000);
    let effects = selected_effects(&clock, 0x350_008);
    let (owner, _) = OriginalIssuerLaunchWindow::select(effects.clone())
        .await
        .unwrap();
    let primary_native = Arc::new(std::io::Error::other("original allocation failure"));
    let primary = AuraError::Internal {
        message: "original allocation failure".into(),
        source: Some(primary_native.clone()),
    };
    let checkpoint_native = Arc::new(std::io::Error::other("original checkpoint failure"));
    let checkpoint_error = TimeoutBudgetError::CheckpointFailure {
        detail: "actual failed checkpoint".into(),
        source: Some(AuraError::Internal {
            message: "actual failed checkpoint".into(),
            source: Some(checkpoint_native.clone()),
        }),
    };
    let mut retained = OriginalAllocationCheckpointCustody::default();
    let result = owner
        .acknowledge_original_allocation_cleanup(
            &effects,
            &primary,
            || async { Err(checkpoint_error) },
            &mut retained,
        )
        .await;
    assert!(
        result.is_err(),
        "a failed checkpoint cannot mint a release acknowledgment"
    );
    let observed = retained
        .latest
        .as_ref()
        .expect("actual failed checkpoint outcome retained");
    let aura_core::time::timeout::CheckpointAttempt::Failed {
        source:
            TimeoutBudgetError::CheckpointFailure {
                source: Some(actual),
                ..
            },
    } = &observed.checkpoint
    else {
        panic!("actual checkpoint failure must remain distinct from interrupted observation");
    };
    let actual_native = std::error::Error::source(actual)
        .and_then(|source| source.downcast_ref::<std::io::Error>())
        .unwrap();
    assert!(std::ptr::eq(actual_native, checkpoint_native.as_ref()));
    let actual_primary = std::error::Error::source(&observed.primary)
        .and_then(|source| source.downcast_ref::<std::io::Error>())
        .unwrap();
    assert!(std::ptr::eq(actual_primary, primary_native.as_ref()));
    assert!(observed.validation.is_err());

    let retry_native = Arc::new(std::io::Error::other("later checkpoint failure"));
    let retry = owner
        .acknowledge_original_allocation_cleanup(
            &effects,
            &primary,
            || async {
                Err(TimeoutBudgetError::CheckpointFailure {
                    detail: "later failed checkpoint".into(),
                    source: Some(AuraError::Internal {
                        message: "later failed checkpoint".into(),
                        source: Some(retry_native.clone()),
                    }),
                })
            },
            &mut retained,
        )
        .await;
    assert!(retry.is_err());
    let aura_core::time::timeout::CheckpointAttempt::Failed { source } =
        &retained.latest.as_ref().unwrap().checkpoint
    else {
        panic!("latest retry outcome must remain observable");
    };
    assert!(std::ptr::eq(
        native_checkpoint_cause(source),
        retry_native.as_ref()
    ));
    assert!(std::ptr::eq(
        native_checkpoint_cause(retained.first_checkpoint_failure.as_ref().unwrap()),
        checkpoint_native.as_ref(),
    ));
    assert!(std::ptr::eq(
        native_checkpoint_cause(retained.first_validation_failure.as_ref().unwrap()),
        checkpoint_native.as_ref(),
    ));
}

fn native_checkpoint_cause(source: &TimeoutBudgetError) -> &std::io::Error {
    let TimeoutBudgetError::CheckpointFailure {
        source: Some(actual),
        ..
    } = source
    else {
        panic!("actual native checkpoint source required");
    };
    std::error::Error::source(actual)
        .and_then(|source| source.downcast_ref::<std::io::Error>())
        .expect("original native checkpoint allocation retained")
}

#[tokio::test]
async fn original_launch_cleanup_checkpoint_expiry_preserves_unattempted_state() {
    let clock = ManualPhysicalClock::new(5_000);
    let effects = selected_effects(&clock, 0x350_009);
    let (owner, _) = OriginalIssuerLaunchWindow::select(effects.clone())
        .await
        .unwrap();
    let primary = AuraError::invalid("original allocation failed");
    let called = std::cell::Cell::new(false);
    let mut retained = OriginalAllocationCheckpointCustody::default();
    clock.set_time(
        owner
            .birth
            .as_ref()
            .unwrap()
            .negative_cleanup
            .deadline_at_ms(),
    );
    let result = owner
        .acknowledge_original_allocation_cleanup(
            &effects,
            &primary,
            || async {
                called.set(true);
                Err(TimeoutBudgetError::CheckpointFailure {
                    detail: "expired checkpoint must never execute".into(),
                    source: None,
                })
            },
            &mut retained,
        )
        .await;
    assert!(
        result.is_err(),
        "expiry cannot mint a release acknowledgment"
    );
    assert!(!called.get());
    let observed = retained
        .latest
        .as_ref()
        .expect("original expired observation retained");
    assert!(matches!(
        observed.checkpoint,
        aura_core::time::timeout::CheckpointAttempt::NotAttempted
    ));
    assert!(observed.validation.is_err());
}
