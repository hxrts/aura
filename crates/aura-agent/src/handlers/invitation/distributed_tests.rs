// The signed acceptance crosses two independent runtime effect systems. The
// initiator's tracker cannot become terminal from code import or from a
// mismatched acceptance; it settles only after transcript verification and
// commit. Task 8 still must pin the invitee key independently.

large_stack_async_test!(device_enrollment_invitee_rejects_wrong_request_and_negative_confirmation, {
    use aura_invitation::protocol::DeviceEnrollmentMessageError;
    let reject = |case| {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Box::pin(super::enrollment_vm_admission::actual_invalid_control_rejection_for_test(case)),
        )
    };
    let error = reject("wrong-request")
        .await
        .expect("permanent authenticated failure stays bounded");
    assert!(
        matches!(error, AgentError::DeviceEnrollmentMessage(DeviceEnrollmentMessageError::DeviceMismatch)),
        "unexpected error: {error:?}"
    );
    // The issuer refuses to sign a negative Committed decision for an
    // uncommitted epoch, so no authenticated negative confirmation exists.
    let error = reject("negative-confirmation")
        .await
        .expect("permanent authenticated failure stays bounded");
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&error);
    let mut refused = false;
    while let Some(current) = cause {
        refused |= matches!(
            current.downcast_ref::<super::enrollment_vm_admission::EnrollmentVmAdmissionError>(),
            Some(super::enrollment_vm_admission::EnrollmentVmAdmissionError::CurrentMembership)
        );
        cause = current.source();
    }
    assert!(refused, "unexpected error: {error:?}");
});

#[tokio::test]
async fn device_enrollment_owned_sessions_exchange_request_accept_confirm() {
    use aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts;
    use crate::runtime::{open_owned_manifest_vm_session_admitted, AuraVmSchedulerSignals, SharedTransport};
    use std::collections::BTreeMap;

    let authority = AuthorityId::new_from_entropy([211; 32]);
    let devices = [DeviceId::new_from_entropy([212; 32]), DeviceId::new_from_entropy([213; 32])];
    let transport = SharedTransport::new();
    let mut effects = Vec::new();
    for (index, device_id) in devices.iter().enumerate() {
        effects.push(Arc::new(
            AuraEffectSystem::simulation_for_named_test_with_shared_transport_for_authority(
                &AgentConfig { device_id: *device_id, ..Default::default() },
                &format!("enrollment-owned-exchange:{index}"),
                authority,
                transport.clone(),
            ).expect("device runtime effects"),
        ));
    }
    let roles = vec![
        ChoreographicRole::new(devices[0], authority, RoleIndex::new(0).unwrap()),
        ChoreographicRole::new(devices[1], authority, RoleIndex::new(1).unwrap()),
    ];
    let manifest = vm_artifacts::composition_manifest();
    let global = vm_artifacts::global_type();
    let locals = vm_artifacts::local_types();
    let session_id = uuid::Uuid::from_u128(0xd6e211);
    let run_owner = |index: usize| {
        let effects = effects[index].clone();
        let roles = roles.clone();
        let manifest = manifest.clone();
        let global = global.clone();
        let locals = locals.clone();
        async move {
            let role = if index == 0 { "Initiator" } else { "Invitee" };
            let peer = if index == 0 { "Invitee" } else { "Initiator" };
            let peer_roles = BTreeMap::from([(peer.to_owned(), roles[1 - index])]);
            let mut session = open_owned_manifest_vm_session_admitted(
                effects.clone(),
                session_id,
                roles,
                &manifest,
                role,
                &global,
                &locals,
                AuraVmSchedulerSignals::default(),
            ).await.expect("owned enrollment session opens");
            if index == 0 {
                session.queue_send_bytes(b"request".to_vec());
                session.queue_send_bytes(b"confirmation".to_vec());
            } else {
                session.queue_send_bytes(b"acceptance".to_vec());
            }
            let mut received = Vec::new();
            for _ in 0..64 {
                let round = session.advance_round(role, &peer_roles).await.expect("host round advances");
                if let Some(blocked) = round.blocked_receive {
                    received.push(blocked.payload.clone());
                    session.inject_blocked_receive(blocked).expect("owner injects receive");
                } else {
                    assert!(matches!(round.host_wait_status, AuraVmHostWaitStatus::Idle | AuraVmHostWaitStatus::Delivered), "unexpected host wait: {:?}", round.host_wait_status);
                    if matches!(round.step, StepResult::AllDone) {
                        session.close().await.expect("owner closes session");
                        assert!(effects.current_runtime_choreography_session_id().is_none(), "completed VM must release its runtime owner binding");
                        return received;
                    }
                    assert!(!matches!(round.step, StepResult::Stuck), "enrollment owner became stuck");
                }
                tokio::task::yield_now().await;
            }
            panic!("{role} did not finish within the bounded VM round budget; received={received:?}");
        }
    };
    // Reopen the same session after completion: a leaked runtime binding or
    // fragment owner would reject the second admission.
    for _ in 0..2 {
        let (initiator, invitee) = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            async { tokio::join!(run_owner(0), run_owner(1)) },
        ).await.expect("two runtime owners finish before the test deadline");
        assert_eq!(initiator, vec![b"acceptance".to_vec()]);
        assert_eq!(invitee, vec![b"request".to_vec(), b"confirmation".to_vec()]);
    }
}

large_stack_async_test!(device_enrollment_two_runtime_signed_acceptance_requires_commit, {
    let (initiator, invitee, invitation, start, acceptance, _verified) =
        actual_pinned_device_enrollment_fixture("two-runtime-proof-commit").await;
    let subject = initiator.authority_id();
    let invited = invitee.authority_id();
    let initiator_effects = initiator.runtime().effects();
    let invitee_effects = invitee.runtime().effects();
    let invitee_handler = handler_for(AuthorityContext::new_with_device(invited, invitee.context().device_id()));
    let ceremony_id = start.ceremony_id;
    let invitee_device = start.device_id;
    let code = start.enrollment_code;
    let imported = invitee_handler
        .import_invitation_code(invitee_effects.as_ref(), &code)
        .await
        .expect("separate invitee runtime verifies and imports the code");
    assert_eq!(imported.invitation_id, invitation.invitation_id);
    assert_eq!(imported.receiver_id, invited);

    let runner = initiator.runtime().ceremony_runner().clone();
    assert_eq!(runner.terminal_outcome(&ceremony_id).await.unwrap(), None,
        "unexpected initiator owner failure: {:?}",
        initiator.runtime().ceremony_tracker().get(&ceremony_id).await.unwrap().error_message);

    let mut wrong_device = acceptance.clone();
    wrong_device.device_id = DeviceId::new_from_entropy([205; 32]);
    assert!(super::device_enrollment::verify_device_enrollment_acceptance(
        initiator_effects.as_ref(),
        &invitation,
        subject,
        &ceremony_id,
        invitee_device,
        &wrong_device,
    ).await.is_err());
    assert_eq!(runner.terminal_outcome(&ceremony_id).await.unwrap(), None);

    let verified = super::device_enrollment::verify_device_enrollment_acceptance(
        initiator_effects.as_ref(),
        &invitation,
        subject,
        &ceremony_id,
        invitee_device,
        &acceptance,
    ).await.expect("initiator verifies the acceptance transcript");
    runner.record_verified_enrollment_response(verified).await.unwrap();
    let service = crate::handlers::device_epoch_rotation::DeviceEpochRotationService::new(
        initiator.authority_id(), initiator.runtime().effects(),
        initiator.runtime().ceremony_tracker().clone(), runner.clone(),
        initiator.runtime().threshold_signing(), initiator.runtime().reconfiguration().clone(),
    );
    tokio::time::timeout(std::time::Duration::from_secs(10),
        service.finalize_sole_device_enrollment(&ceremony_id))
        .await.expect("bounded finalizer").expect("actual finalizer commits verified acceptance");
    assert_eq!(
        runner.terminal_outcome(&ceremony_id).await.unwrap(),
        Some(crate::runtime::services::ceremony_tracker::CeremonyTerminalOutcome::Committed),
    );
});
