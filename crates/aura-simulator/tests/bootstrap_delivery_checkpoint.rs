//! Exact in-process bootstrap delivery checkpoint using canonical signed codes.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::disallowed_types)]

use std::{sync::Arc, time::Duration};

use aura_agent::{
    AgentBuilder, AgentConfig, AgentError, AuraAgent, BootstrapBrokerConfig,
    BootstrapBrokerLanBindPolicy, BootstrapBrokerRegistration, EnrollmentParticipantProfile,
    LocalBootstrapBrokerService, SharedTransport, SimulatedEnrollment, TaskSupervisor,
};
use aura_core::{
    context::EffectContext,
    effects::ExecutionMode,
    invitation::{BootstrapBrokerInvitation, InvitationPurpose},
    AuthorityId, ContextId, DeviceId, PhysicalTime, TimeoutBudget,
};
use aura_testkit::time::ManualPhysicalClock;

struct Checkpoint {
    broker: LocalBootstrapBrokerService,
    supervisor: TaskSupervisor,
    enrollment: SimulatedEnrollment,
    second: Arc<AuraAgent>,
    _roots: Vec<tempfile::TempDir>,
    first: DeviceId,
    other: DeviceId,
    authority: AuthorityId,
}

impl Checkpoint {
    async fn new() -> Self {
        let clock = Arc::new(ManualPhysicalClock::new(5_000));
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(120),
            "checkpoint-bearer",
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .expect("actual loopback broker")
        .with_mailbox_time_provider(clock.clone());
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("bootstrap_checkpoint"));
        let roots: Vec<_> = (0..3)
            .map(|_| tempfile::tempdir().expect("isolated actual profile"))
            .collect();
        let broker_config = BootstrapBrokerConfig::default()
            .enabled(true)
            .with_base_url(broker.public_url())
            .with_auth_token("checkpoint-bearer")
            .with_invitation_retrieval_token("checkpoint-first");
        let profiles = std::array::from_fn(|index| {
            let seed = 211 + index as u8 * 3;
            EnrollmentParticipantProfile {
                config: AgentConfig {
                    device_id: DeviceId::new_from_entropy([seed + 1; 32]),
                    storage: aura_agent::core::config::StorageConfig {
                        base_path: roots[index].path().to_path_buf(),
                        ..Default::default()
                    },
                    bootstrap_broker: if index == 1 {
                        broker_config.clone()
                    } else {
                        BootstrapBrokerConfig::default()
                    },
                    ..Default::default()
                },
                authority: AuthorityId::new_from_entropy([seed; 32]),
                context: ContextId::new_from_entropy([seed + 2; 32]),
                seed: u64::from(seed),
            }
        });
        let startup = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: 5_000,
                uncertainty: None,
            },
            Duration::from_secs(120),
        )
        .expect("selected original startup window");
        let enrollment = SimulatedEnrollment::prepare(
            profiles,
            clock.clone(),
            SharedTransport::default(),
            startup,
        )
        .await
        .expect("genuine signed enrollment issuance");
        let authority = enrollment.recipient().authority_id();
        let first = enrollment
            .recipient()
            .runtime()
            .effects()
            .config()
            .device_id;
        let other = DeviceId::new_from_entropy([229; 32]);
        let second_config = AgentConfig {
            device_id: other,
            storage: aura_agent::core::config::StorageConfig {
                base_path: roots[2].path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let second_context = EffectContext::new(
            authority,
            ContextId::new_from_entropy([230; 32]),
            ExecutionMode::Simulation { seed: 230 },
        );
        let second = Arc::new(
            AgentBuilder::new()
                .with_authority(authority)
                .with_config(second_config)
                .with_physical_time_provider(clock)
                .build_simulation_async_with_shared_transport(
                    230,
                    &second_context,
                    SharedTransport::default(),
                )
                .await
                .expect("second actual runtime of recipient authority"),
        );
        Self {
            broker,
            supervisor,
            enrollment,
            second,
            _roots: roots,
            first,
            other,
            authority,
        }
    }

    async fn register(&self, device: DeviceId, credential: &str) {
        self.broker
            .register(BootstrapBrokerRegistration {
                authority_id: self.authority.to_string(),
                device_id: device,
                address: format!("observed-{device}"),
                invitation_retrieval_token: credential.into(),
                nickname_suggestion: None,
            })
            .await
            .expect("actual broker registration");
    }

    async fn contact_code(&self, receiver: AuthorityId) -> String {
        let bridge = self.enrollment.issuer().clone().as_runtime_bridge();
        let invitation = bridge
            .create_contact_invitation(receiver, None, None, None, None)
            .await
            .expect("canonical contact invitation producer");
        bridge
            .export_invitation(&invitation.invitation_id.to_string())
            .await
            .expect("original signed contact code")
    }

    fn delivery(
        &self,
        device: DeviceId,
        purpose: InvitationPurpose,
        code: String,
    ) -> BootstrapBrokerInvitation {
        BootstrapBrokerInvitation {
            recipient_authority_id: self.authority.to_string(),
            recipient_device_id: device,
            purpose,
            invitation_code: code,
        }
    }

    async fn finish(self) {
        self.enrollment
            .accept()
            .await
            .expect("original enrollment disposal");
        self.second
            .runtime()
            .tasks()
            .shutdown_gracefully(Duration::from_secs(5))
            .await
            .expect("second actual device disposal");
        self.supervisor
            .shutdown_with_timeout(Duration::from_secs(5))
            .await
            .expect("broker disposal");
    }

    async fn finish_refused(self) {
        let issuer = self.enrollment.issuer().clone();
        let recipient = self.enrollment.recipient().clone();
        let _original_failure = self
            .enrollment
            .fail(AgentError::config(
                "bootstrap checkpoint retains the original refusal",
            ))
            .await;
        assert!(issuer.runtime().tasks().active_tasks().is_empty());
        assert!(recipient.runtime().tasks().active_tasks().is_empty());
        self.second
            .runtime()
            .tasks()
            .shutdown_gracefully(Duration::from_secs(5))
            .await
            .expect("second actual device disposal");
        self.supervisor
            .shutdown_with_timeout(Duration::from_secs(5))
            .await
            .expect("broker disposal after original refusal");
    }
}

#[tokio::test]
async fn bootstrap_checkpoint_two_device_credentials_refresh_and_one_use() {
    let checkpoint = Checkpoint::new().await;
    assert_eq!(checkpoint.second.authority_id(), checkpoint.authority);
    assert_eq!(
        checkpoint.second.runtime().effects().config().device_id,
        checkpoint.other
    );
    checkpoint
        .register(checkpoint.first, "first-credential")
        .await;
    checkpoint
        .register(checkpoint.other, "other-credential")
        .await;
    assert!(checkpoint
        .broker
        .take_invitations(
            &checkpoint.authority.to_string(),
            checkpoint.first,
            "first-credential"
        )
        .await
        .expect("empty retrieval does not consume credential")
        .is_empty());
    let contact = checkpoint.contact_code(checkpoint.authority).await;
    let first = checkpoint.delivery(
        checkpoint.first,
        InvitationPurpose::Contact,
        contact.clone(),
    );
    let other = checkpoint.delivery(checkpoint.other, InvitationPurpose::Contact, contact);
    checkpoint
        .broker
        .queue_invitation(first.clone())
        .await
        .expect("first queue");
    checkpoint
        .broker
        .queue_invitation(other.clone())
        .await
        .expect("other queue");
    checkpoint
        .register(checkpoint.other, "other-refreshed")
        .await;
    for (device, credential) in [
        (checkpoint.first, "other-refreshed"),
        (checkpoint.other, "first-credential"),
        (checkpoint.other, "other-credential"),
    ] {
        assert!(
            checkpoint
                .broker
                .take_invitations(&checkpoint.authority.to_string(), device, credential)
                .await
                .is_err(),
            "foreign or replaced credential refused"
        );
    }
    assert_eq!(
        checkpoint
            .broker
            .take_invitations(
                &checkpoint.authority.to_string(),
                checkpoint.first,
                "first-credential"
            )
            .await
            .expect("first exact delivery"),
        vec![first]
    );
    assert_eq!(
        checkpoint
            .broker
            .take_invitations(
                &checkpoint.authority.to_string(),
                checkpoint.other,
                "other-refreshed"
            )
            .await
            .expect("other exact delivery"),
        vec![other]
    );
    assert!(
        checkpoint
            .broker
            .take_invitations(
                &checkpoint.authority.to_string(),
                checkpoint.first,
                "first-credential"
            )
            .await
            .is_err(),
        "nonempty retrieval consumed first credential"
    );
    checkpoint.finish().await;
}

#[tokio::test]
async fn bootstrap_checkpoint_canonical_contact_and_enrollment_dispatch() {
    use aura_app::ui::workflows::invitation::receive_bootstrap_invitation_batch;
    let checkpoint = Checkpoint::new().await;
    checkpoint
        .register(checkpoint.first, "checkpoint-first")
        .await;
    checkpoint
        .register(checkpoint.other, "checkpoint-other")
        .await;
    let contact_code = checkpoint.contact_code(checkpoint.authority).await;
    let contact = checkpoint.delivery(checkpoint.first, InvitationPurpose::Contact, contact_code);
    let enrollment = checkpoint.delivery(
        checkpoint.first,
        InvitationPurpose::DeviceEnrollment,
        checkpoint.enrollment.start().enrollment_code.clone(),
    );
    checkpoint
        .broker
        .queue_invitation(contact)
        .await
        .expect("canonical contact queue");
    checkpoint
        .broker
        .queue_invitation(enrollment.clone())
        .await
        .expect("signed enrollment queue");
    let host_prefill = Arc::new(std::sync::Mutex::new(None));
    let captured = host_prefill.clone();
    let observed = receive_bootstrap_invitation_batch(
        checkpoint
            .enrollment
            .recipient()
            .clone()
            .as_runtime_bridge(),
        None,
        Arc::new(move |code| {
            *captured.lock().expect("host prefill observation") = Some(code.to_owned());
            Ok(())
        }),
    )
    .expect("actual bootstrap batch admitted")
    .await
    .expect("required batch observation")
    .expect("canonical contact import and enrollment prefill");
    assert_eq!(observed.imported.len(), 1);
    assert!(matches!(
        observed.imported[0].info().invitation_type,
        aura_app::runtime_bridge::InvitationBridgeType::Contact { .. }
    ));
    assert_eq!(
        observed.enrollment_prefill.as_deref(),
        Some(enrollment.invitation_code.as_str())
    );
    assert_eq!(
        host_prefill.lock().unwrap().as_deref(),
        Some(enrollment.invitation_code.as_str())
    );
    assert!(checkpoint
        .broker
        .take_invitations(
            &checkpoint.authority.to_string(),
            checkpoint.other,
            "checkpoint-other"
        )
        .await
        .expect("other actual device remains empty")
        .is_empty());
    checkpoint.finish().await;
}

#[tokio::test]
async fn bootstrap_checkpoint_wrong_purpose_foreign_recipient_and_verification_refuse() {
    use aura_app::ui::workflows::invitation::{
        receive_bootstrap_invitation_batch, BootstrapBatchFailureReason,
    };
    for case in [0, 2] {
        let checkpoint = Checkpoint::new().await;
        checkpoint
            .register(checkpoint.first, "checkpoint-first")
            .await;
        checkpoint
            .register(checkpoint.other, "checkpoint-other")
            .await;
        let unrelated = checkpoint.delivery(
            checkpoint.other,
            InvitationPurpose::Contact,
            checkpoint.contact_code(checkpoint.authority).await,
        );
        checkpoint
            .broker
            .queue_invitation(unrelated.clone())
            .await
            .expect("unrelated signed delivery");
        let (purpose, code) = if case == 0 {
            (
                InvitationPurpose::DeviceEnrollment,
                checkpoint.contact_code(checkpoint.authority).await,
            )
        } else {
            let code = checkpoint.contact_code(checkpoint.authority).await;
            let (invitation, proof, transport) =
                aura_invitation::shareable::ShareableInvitation::from_code_with_proof_and_transport(&code)
                    .expect("original signed contact code");
            let mut proof = proof.expect("canonical sender proof");
            proof.signature[0] ^= 1;
            (
                InvitationPurpose::Contact,
                invitation
                    .to_signed_code_with_transport(proof, transport)
                    .expect("same-shaped foreign signature"),
            )
        };
        let invalid = checkpoint.delivery(checkpoint.first, purpose, code);
        checkpoint
            .broker
            .queue_invitation(invalid.clone())
            .await
            .expect("original invalid delivery retained");
        let outcome = receive_bootstrap_invitation_batch(
            checkpoint
                .enrollment
                .recipient()
                .clone()
                .as_runtime_bridge(),
            None,
            Arc::new(|_| panic!("invalid purpose or recipient cannot reach host prefill")),
        )
        .expect("required receiver admitted")
        .await
        .expect("original receiver observation");
        let refusal = match outcome {
            Err(refusal) => refusal,
            Ok(_) => panic!("invalid canonical delivery must be refused in case {case}"),
        };
        assert!(matches!(
            refusal.reason(),
            BootstrapBatchFailureReason::Failed(_)
        ));
        assert_eq!(refusal.refused_deliveries(), &[invalid]);
        assert!(refusal.completed().imported.is_empty());
        assert_eq!(
            checkpoint
                .broker
                .take_invitations(
                    &checkpoint.authority.to_string(),
                    checkpoint.other,
                    "checkpoint-other"
                )
                .await
                .expect("unrelated original delivery survives"),
            vec![unrelated]
        );
        checkpoint.finish_refused().await;
    }
    let checkpoint = Checkpoint::new().await;
    checkpoint
        .register(checkpoint.first, "checkpoint-first")
        .await;
    checkpoint
        .register(checkpoint.other, "checkpoint-other")
        .await;
    let foreign = AuthorityId::new_from_entropy([239; 32]);
    let contact = checkpoint.contact_code(checkpoint.authority).await;
    checkpoint
        .broker
        .queue_invitation(BootstrapBrokerInvitation {
            recipient_authority_id: foreign.to_string(),
            recipient_device_id: checkpoint.first,
            purpose: InvitationPurpose::Contact,
            invitation_code: contact.clone(),
        })
        .await
        .expect("foreign selector queued only for its owner");
    assert!(
        checkpoint
            .broker
            .take_invitations(&foreign.to_string(), checkpoint.first, "checkpoint-first")
            .await
            .is_err(),
        "foreign recipient cannot use this device credential"
    );
    assert!(checkpoint
        .broker
        .take_invitations(
            &checkpoint.authority.to_string(),
            checkpoint.first,
            "checkpoint-first"
        )
        .await
        .expect("own empty retrieval retains credential")
        .is_empty());
    let unrelated = checkpoint.delivery(checkpoint.other, InvitationPurpose::Contact, contact);
    checkpoint
        .broker
        .queue_invitation(unrelated.clone())
        .await
        .expect("other exact queue");
    assert_eq!(
        checkpoint
            .broker
            .take_invitations(
                &checkpoint.authority.to_string(),
                checkpoint.other,
                "checkpoint-other"
            )
            .await
            .expect("other delivery survives foreign selector"),
        vec![unrelated]
    );
    checkpoint.finish().await;
}

#[tokio::test]
async fn bootstrap_checkpoint_cancellation_preserves_other_device_delivery() {
    use aura_app::ui::workflows::invitation::receive_bootstrap_invitation_batch;
    let checkpoint = Checkpoint::new().await;
    checkpoint
        .register(checkpoint.first, "checkpoint-first")
        .await;
    checkpoint
        .register(checkpoint.other, "checkpoint-other")
        .await;
    let unrelated = checkpoint.delivery(
        checkpoint.other,
        InvitationPurpose::Contact,
        checkpoint.contact_code(checkpoint.authority).await,
    );
    checkpoint
        .broker
        .queue_invitation(unrelated.clone())
        .await
        .expect("unrelated original queue");
    checkpoint
        .broker
        .queue_invitation(checkpoint.delivery(
            checkpoint.first,
            InvitationPurpose::DeviceEnrollment,
            checkpoint.enrollment.start().enrollment_code.clone(),
        ))
        .await
        .expect("selected original queue");
    let observer = receive_bootstrap_invitation_batch(
        checkpoint
            .enrollment
            .recipient()
            .clone()
            .as_runtime_bridge(),
        None,
        Arc::new(|_| Ok(())),
    )
    .expect("required receiver admitted");
    drop(observer);
    assert_eq!(
        checkpoint
            .broker
            .take_invitations(
                &checkpoint.authority.to_string(),
                checkpoint.other,
                "checkpoint-other"
            )
            .await
            .expect("unrelated delivery survives cancellation"),
        vec![unrelated]
    );
    checkpoint.finish_refused().await;
}
