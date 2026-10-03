// The signed acceptance crosses two independent runtime effect systems. The
// initiator's tracker cannot become terminal from code import or from a
// mismatched acceptance; it settles only after transcript verification and
// commit. Task 8 still must pin the invitee key independently.
#[tokio::test]
async fn device_enrollment_two_runtime_signed_acceptance_requires_commit() {
    use aura_app::runtime_bridge::CeremonyKind;
    use aura_core::threshold::ParticipantIdentity;
    use aura_core::Hash32;
    use crate::runtime::services::ceremony_runner::{CeremonyCommitMetadata, CeremonyInitRequest};

    let subject = AuthorityId::new_from_entropy([201; 32]);
    let invited = AuthorityId::new_from_entropy([202; 32]);
    let initiator_device = DeviceId::new_from_entropy([203; 32]);
    let invitee_device = DeviceId::new_from_entropy([204; 32]);
    let initiator_context = AuthorityContext::new_with_device(subject, initiator_device);
    let invitee_context = AuthorityContext::new_with_device(invited, invitee_device);
    let initiator_effects = effects_for(&initiator_context);
    let invitee_effects = effects_for(&invitee_context);
    let invitee_handler = handler_for(invitee_context);

    let mut invitation = device_enrollment_test_invitation(
        "inv-two-runtime-enrollment-boundary",
        subject,
        invited,
        invitee_device,
    );
    invitation.expires_at = None;
    let InvitationType::DeviceEnrollment {
        initiator_device_id,
        ceremony_id,
        ..
    } = &mut invitation.invitation_type else { unreachable!() };
    *initiator_device_id = initiator_device;
    let ceremony_id = ceremony_id.clone();

    bootstrap_test_signing_authority(&initiator_effects, subject).await;
    let code = InvitationServiceApi::export_signed_invitation_with_transport(
        initiator_effects.as_ref(),
        &invitation,
        &ShareableInvitationTransportMetadata {
            sender_device_id: Some(initiator_device),
            ..ShareableInvitationTransportMetadata::default()
        },
        false,
    )
    .await
    .expect("initiator exports signed enrollment code");
    let imported = invitee_handler
        .import_invitation_code(invitee_effects.as_ref(), &code)
        .await
        .expect("separate invitee runtime verifies and imports the code");
    assert_eq!(imported.invitation_id, invitation.invitation_id);
    assert_eq!(imported.receiver_id, invited);

    let time: Arc<dyn aura_core::effects::time::PhysicalTimeEffects> =
        Arc::new(initiator_effects.time_effects().clone());
    let runner = CeremonyRunner::new(CeremonyTracker::new(time));
    runner.start(CeremonyInitRequest {
        ceremony_id: ceremony_id.clone(),
        kind: CeremonyKind::DeviceEnrollment,
        initiator_id: subject,
        threshold_k: 1,
        total_n: 1,
        participants: vec![ParticipantIdentity::device(invitee_device)],
        new_epoch: 1,
        enrollment_device_id: Some(invitee_device),
        enrollment_nickname_suggestion: None,
        prestate_hash: Hash32([201; 32]),
    }).await.unwrap();
    assert_eq!(runner.terminal_outcome(&ceremony_id).await.unwrap(), None);

    let acceptance = signed_device_enrollment_accept(
        &invitee_effects,
        &invitation,
        invited,
        invitee_device,
    ).await;
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

    super::device_enrollment::verify_device_enrollment_acceptance(
        initiator_effects.as_ref(),
        &invitation,
        subject,
        &ceremony_id,
        invitee_device,
        &acceptance,
    ).await.expect("initiator verifies the acceptance transcript");
    runner.record_local_response(
        &ceremony_id,
        ParticipantIdentity::device(invitee_device),
    ).await.unwrap();
    assert_eq!(runner.terminal_outcome(&ceremony_id).await.unwrap(), None);
    runner.commit(&ceremony_id, CeremonyCommitMetadata::default()).await.unwrap();
    assert_eq!(
        runner.terminal_outcome(&ceremony_id).await.unwrap(),
        Some(crate::runtime::services::ceremony_tracker::CeremonyTerminalOutcome::Committed),
    );
}
