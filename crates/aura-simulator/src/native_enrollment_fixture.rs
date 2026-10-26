//! Shared simulation-only native broker setup and selected physical-clock drive.
//! The signed issuer and runtime remain the production owners; this module only
//! arranges their test environment and observes authenticated broker progress.
use std::{future::Future, sync::Arc, time::Duration};

use aura_agent::{
    AuraAgent, BootstrapBrokerConfig, BootstrapBrokerLanBindPolicy, EnrollmentParticipantProfile,
    LocalBootstrapBrokerService, SharedTransport, SimulatedEnrollment, TaskSupervisor,
};
use aura_core::{AuthorityId, DeviceId, PhysicalTime, TimeoutBudget};
use aura_testkit::time::ManualPhysicalClock;

/// Preserve the original signed startup and manifest windows while driving
/// selected test time. A queued frame freezes advancement for VM delivery.
pub async fn advance_selected_clock_before_frame(
    enrollment: &SimulatedEnrollment,
    clock: &ManualPhysicalClock,
    first_frame_queued: bool,
) {
    if first_frame_queued {
        return;
    }
    let transfer = enrollment
        .start()
        .manifest_transfer
        .as_ref()
        .expect("original signed transfer");
    let signed_expiry =
        aura_invitation::enrollment_manifest::SignedEnrollmentTrustManifest::decode(
            &transfer.manifest_code,
        )
        .expect("signed manifest shape")
        .manifest
        .expires_at_ms;
    advance_selected_clock_with_bounds(
        clock,
        signed_expiry,
        enrollment.original_startup_window().deadline_at_ms(),
        first_frame_queued,
    )
    .await;
}

async fn advance_selected_clock_with_bounds(
    clock: &ManualPhysicalClock,
    signed_expiry: u64,
    startup_deadline: u64,
    first_frame_queued: bool,
) {
    if first_frame_queued {
        return;
    }
    if let Some(next) = clock.next_registered_deadline().await {
        let now = clock.now_ms();
        if next.value() > now
            && next.value() <= now + 1_000
            && next.value() < signed_expiry
            && next.value() < startup_deadline
        {
            clock.set_time(next.value());
        }
    }
}

/// Observed signed bounds and broker progress selector for the existing
/// selected-clock driver. This carries no signer, lease, checkpoint key, or
/// permission to resume an enrollment.
pub struct OriginalSignedDriveContext {
    selector: (AuthorityId, DeviceId, [u8; 32]),
    broker_url: String,
    auth_token: String,
    clock: Arc<ManualPhysicalClock>,
    signed_expiry: u64,
    startup_deadline: u64,
}

struct OriginalBrokerProgress {
    registration_present: bool,
    first_frame_queued: bool,
}

/// An original signed issuance with its selected native provider and broker owner.
pub struct NativeEnrollmentFixture {
    pub enrollment: SimulatedEnrollment,
    pub supervisor: TaskSupervisor,
    pub clock: Arc<ManualPhysicalClock>,
    broker_url: String,
    auth_token: String,
    #[cfg(feature = "simulator-checkpoint")]
    original_broker: Option<LocalBootstrapBrokerService>,
}

/// Typed failure from the shared genuine signed setup driver. Its public text
/// identifies the stage without exposing profile paths or protected values;
/// the original runtime error remains available as the source.
#[derive(Debug, thiserror::Error)]
pub enum NativeEnrollmentFixtureError {
    #[error("native enrollment fixture setup failed")]
    Setup(String),
    #[error("original signed enrollment issuance failed")]
    SignedIssuance(#[source] aura_agent::AgentError),
    #[error("original signed broker registration failed")]
    Registration(#[source] aura_agent::AgentError),
}

impl NativeEnrollmentFixture {
    /// Bind a loopback broker and publish the original signed issuer registration.
    pub async fn prepare(
        profiles: [EnrollmentParticipantProfile; 2],
        clock: Arc<ManualPhysicalClock>,
        auth_token: &str,
    ) -> Result<Self, NativeEnrollmentFixtureError> {
        Self::prepare_inner(profiles, clock, auth_token, None).await
    }

    /// Use the same signed issuance path with an external simulator retirement
    /// owner that survives profile restore. No signature is fixture-assembled.
    #[cfg(feature = "simulator-checkpoint")]
    pub async fn prepare_with_nonce_retirement(
        profiles: [EnrollmentParticipantProfile; 2],
        clock: Arc<ManualPhysicalClock>,
        auth_token: &str,
        retirements: [Arc<dyn aura_core::crypto::tree_signing::FrostNonceRetirement>; 2],
    ) -> Result<Self, NativeEnrollmentFixtureError> {
        Self::prepare_inner(profiles, clock, auth_token, Some(retirements)).await
    }

    async fn prepare_inner(
        mut profiles: [EnrollmentParticipantProfile; 2],
        clock: Arc<ManualPhysicalClock>,
        auth_token: &str,
        retirements: Option<[Arc<dyn aura_core::crypto::tree_signing::FrostNonceRetirement>; 2]>,
    ) -> Result<Self, NativeEnrollmentFixtureError> {
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(120),
            auth_token,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .map_err(|error| NativeEnrollmentFixtureError::Setup(format!("broker bind: {error}")))?
        .with_mailbox_time_provider(clock.clone());
        let broker_url = broker.public_url().to_owned();
        let supervisor = TaskSupervisor::new();
        broker.start(&supervisor.group("native_enrollment_fixture_broker"));
        let config = BootstrapBrokerConfig::default()
            .enabled(true)
            .with_base_url(&broker_url)
            .with_auth_token(auth_token);
        for profile in &mut profiles {
            profile.config.bootstrap_broker = config.clone();
        }
        let startup = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: clock.now_ms(),
                uncertainty: None,
            },
            Duration::from_secs(120),
        )
        .map_err(|error| NativeEnrollmentFixtureError::Setup(format!("startup window: {error}")))?;
        let enrollment = match retirements {
            #[cfg(feature = "simulator-checkpoint")]
            Some(retirements) => {
                SimulatedEnrollment::prepare_native_broker_with_nonce_retirement(
                    profiles,
                    clock.clone(),
                    SharedTransport::default(),
                    startup,
                    retirements,
                )
                .await
            }
            #[cfg(not(feature = "simulator-checkpoint"))]
            Some(_) => unreachable!("simulator retirement cannot be supplied without feature"),
            None => {
                SimulatedEnrollment::prepare_native_broker(
                    profiles,
                    clock.clone(),
                    SharedTransport::default(),
                    startup,
                )
                .await
            }
        }
        .map_err(NativeEnrollmentFixtureError::SignedIssuance)?;
        enrollment
            .issuer()
            .drain_issuer_return_vm_mailbox()
            .await
            .map_err(NativeEnrollmentFixtureError::Registration)?;
        Ok(Self {
            enrollment,
            supervisor,
            clock,
            broker_url,
            auth_token: auth_token.to_owned(),
            #[cfg(feature = "simulator-checkpoint")]
            original_broker: Some(broker),
        })
    }

    /// Transfer the same one-shot enrollment, broker and supervision owners.
    /// This lets a checkpoint fixture acknowledge normal enrollment disposal
    /// while the original broker is still available, then stop that broker.
    /// Missing broker custody is returned alongside the original enrollment;
    /// it never drops an unresolved domain owner as a refusal side effect.
    #[cfg(feature = "simulator-checkpoint")]
    pub fn into_original_checkpoint_parts(
        self,
    ) -> (
        SimulatedEnrollment,
        TaskSupervisor,
        Result<LocalBootstrapBrokerService, aura_agent::BrokerCheckpointQuiescenceError>,
    ) {
        (
            self.enrollment,
            self.supervisor,
            self.original_broker
                .ok_or(aura_agent::BrokerCheckpointQuiescenceError::NotStarted),
        )
    }

    /// Consume the exact broker that handled this original signed setup.
    /// The opaque result carries no export or restore API.
    #[cfg(feature = "simulator-checkpoint")]
    pub async fn stop_original_broker_for_checkpoint(
        &mut self,
    ) -> Result<aura_agent::StoppedBrokerCheckpointOwner, aura_agent::BrokerCheckpointQuiescenceError>
    {
        self.original_broker
            .take()
            .ok_or(aura_agent::BrokerCheckpointQuiescenceError::NotStarted)?
            .stop_for_simulator_checkpoint(Duration::from_secs(5))
            .await
    }

    /// Interrupt the ACK of an exact frame already queued by the original
    /// signed execution. Only the retained original broker owner can arm it.
    #[cfg(feature = "simulator-checkpoint")]
    pub fn interrupt_original_pending_ack_for_checkpoint(&self, message_id: String) -> bool {
        self.original_broker
            .as_ref()
            .is_some_and(|broker| broker.interrupt_exact_pending_ack_for_checkpoint(message_id))
    }

    /// Inject a named delivery fault by stopping the original HTTP tasks and
    /// dropping their exact bound listener. This grants no checkpoint owner.
    #[cfg(feature = "simulator-checkpoint")]
    pub async fn stop_original_broker_for_delivery_fault(&mut self) -> Result<(), String> {
        self.supervisor
            .shutdown_with_timeout(Duration::from_secs(5))
            .await
            .map_err(|error| error.to_string())?;
        drop(
            self.original_broker
                .take()
                .ok_or_else(|| "original signed broker owner already consumed".to_string())?,
        );
        Ok(())
    }

    pub fn broker_url(&self) -> &str {
        &self.broker_url
    }

    /// Capture observation inputs before the original broker is stopped.
    /// Fresh and recovered owners use the same authenticated progress witness
    /// and original signed time bounds; this does not carry runtime custody.
    pub fn drive_context(&self) -> OriginalSignedDriveContext {
        self.drive_context_with_selector(
            self.enrollment
                .original_mailbox_selector()
                .expect("retained original mailbox selector"),
        )
    }

    fn drive_context_with_selector(
        &self,
        selector: (AuthorityId, DeviceId, [u8; 32]),
    ) -> OriginalSignedDriveContext {
        let transfer = self
            .enrollment
            .start()
            .manifest_transfer
            .as_ref()
            .expect("original signed transfer");
        let signed_expiry =
            aura_invitation::enrollment_manifest::SignedEnrollmentTrustManifest::decode(
                &transfer.manifest_code,
            )
            .expect("signed manifest shape")
            .manifest
            .expires_at_ms;
        OriginalSignedDriveContext {
            selector,
            broker_url: self.broker_url.clone(),
            auth_token: self.auth_token.clone(),
            clock: self.clock.clone(),
            signed_expiry,
            startup_deadline: self.enrollment.original_startup_window().deadline_at_ms(),
        }
    }

    /// Drive the selected clock only while no original broker frame is queued.
    pub async fn drive_until<T, M, F>(
        &self,
        maintenance_pass: M,
        goal: impl Future<Output = T>,
    ) -> T
    where
        M: FnMut() -> F,
        F: Future<Output = ()>,
    {
        self.drive_context()
            .drive_until(self.enrollment.issuer().clone(), maintenance_pass, goal)
            .await
    }
}

impl OriginalSignedDriveContext {
    /// Authenticated broker observation for the exact original signed selector.
    /// This is never a registration credential or transport send capability.
    #[cfg(feature = "simulator-checkpoint")]
    pub async fn original_registration_present(&self) -> bool {
        self.broker_progress().await.registration_present
    }

    /// Observe the same selected-time, mailbox, and maintenance contracts
    /// with either the original issuer or a normally reentered issuer.
    pub async fn drive_until<T, M, F>(
        &self,
        issuer: Arc<AuraAgent>,
        maintenance_pass: M,
        goal: impl Future<Output = T>,
    ) -> T
    where
        M: FnMut() -> F,
        F: Future<Output = ()>,
    {
        self.drive_until_with_policy(
            Some(issuer),
            #[cfg(feature = "simulator-checkpoint")]
            None,
            maintenance_pass,
            goal,
            true,
        )
        .await
        .expect("bounded original signed import completion")
    }

    /// Recovered owners must not expire their own in-flight maintenance pass
    /// by treating an arbitrary selected-time timer as protocol quiescence.
    /// The exact original signed clock is retained and observed, but only an
    /// owned quiescence witness may authorize future time advancement.
    #[cfg(feature = "simulator-checkpoint")]
    pub async fn drive_until_recovered<T, M, F>(
        &self,
        issuer: Arc<AuraAgent>,
        recipient: Arc<AuraAgent>,
        maintenance_pass: M,
        goal: impl Future<Output = T>,
    ) -> aura_core::AuraResult<T>
    where
        M: FnMut() -> F,
        F: Future<Output = ()>,
    {
        self.drive_until_with_policy(Some(issuer), Some(recipient), maintenance_pass, goal, false)
            .await
    }

    async fn drive_until_with_policy<T, M, F>(
        &self,
        issuer: Option<Arc<AuraAgent>>,
        #[cfg(feature = "simulator-checkpoint")] recipient: Option<Arc<AuraAgent>>,
        mut maintenance_pass: M,
        goal: impl Future<Output = T>,
        advance_clock: bool,
    ) -> aura_core::AuraResult<T>
    where
        M: FnMut() -> F,
        F: Future<Output = ()>,
    {
        let mut goal = Box::pin(goal);
        let mut maintenance = Box::pin(maintenance_pass());
        let mut issuer_drain: Option<
            std::pin::Pin<Box<dyn Future<Output = aura_agent::AgentResult<usize>> + '_>>,
        > = Some(Box::pin(async {
            issuer
                .as_ref()
                .expect("fresh original issuer")
                .drain_issuer_return_vm_mailbox()
                .await
        }));
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        let mut first_frame_queued = false;
        let mut last_recovered_observation = None;
        #[cfg(feature = "simulator-checkpoint")]
        let mut registration_seen = false;
        #[cfg(feature = "simulator-checkpoint")]
        let mut last_recovered_probe = None;
        let result = aura_testkit::infrastructure::time::assertions::assert_completes_within(
            Duration::from_secs(30),
            async {
            loop {
                tokio::select! {
                    outcome = &mut goal => break Ok(outcome),
                    _ = &mut maintenance => maintenance = Box::pin(maintenance_pass()),
                    drained = async { match issuer_drain.as_mut() {
                        Some(drain) => drain.await,
                        None => std::future::pending().await,
                    }} => {
                        issuer_drain = None;
                        let _ = drained;
                    },
                    _ = tick.tick() => {
                        if issuer_drain.is_none() {
                            issuer_drain = Some(Box::pin(async {
                                issuer.as_ref().expect("fresh original issuer").drain_issuer_return_vm_mailbox().await
                            }));
                        }
                        let progress = if first_frame_queued {
                            None
                        } else {
                            Some(self.broker_progress().await)
                        };
                        first_frame_queued |= progress.as_ref().is_some_and(|progress| progress.first_frame_queued);
                        if advance_clock {
                            advance_selected_clock_with_bounds(
                                &self.clock, self.signed_expiry, self.startup_deadline, first_frame_queued,
                            ).await;
                        } else {
                            #[cfg(feature = "simulator-checkpoint")]
                            if !first_frame_queued {
                                let progress = progress.as_ref().expect("broker was observed before first frame");
                                let selected: Arc<dyn aura_core::effects::PhysicalTimeEffects> = self.clock.clone();
                                registration_seen |= progress.registration_present;
                                use aura_agent::OriginalPendingMailboxRetryObservation as Retry;
                                let (retry_probe, retry_advanced) = if progress.registration_present {
                                    let retry = issuer.as_ref().expect("fresh original issuer").runtime()
                                        .original_pending_mailbox_retry_witness(&selected, self.selector);
                                    match retry {
                                        Retry::Pending(witness) => {
                                            match witness.with_current_registration(|registration, task| {
                                                    let deadline = registration.deadline.value();
                                                    let now = self.clock.now_ms();
                                                    let bounded = deadline > now
                                                        && deadline <= now.saturating_add(100)
                                                        && deadline < self.signed_expiry
                                                        && deadline < self.startup_deadline;
                                                    let advanced = bounded && self.clock.try_advance_registered_timer(registration, task);
                                                    (format!("original-registration-retry:{deadline}-generation:{},bounded:{bounded},advanced:{advanced}", registration.generation), advanced)
                                                }) {
                                                Some(result) => result,
                                                None => ("stale-original-registration-retry".to_owned(), false),
                                            }
                                        }
                                        Retry::NoPendingRetry => ("no-original-registration-retry".to_owned(), false),
                                        Retry::ForeignSelector => ("foreign-original-registration-selector".to_owned(), false),
                                        Retry::ForeignSelectedProvider => ("foreign-original-registration-provider".to_owned(), false),
                                    }
                                } else {
                                    ("recipient-registration-absent".to_owned(), false)
                                };
                                use aura_agent::SelectedSchedulerBatchObservation as Batch;
                                let (issuer_batch, recipient_batch) = {
                                    let issuer_batch = issuer.as_ref().expect("fresh original issuer").runtime()
                                        .selected_scheduler_batch_deadline_witness(&selected).await;
                                    let recipient_batch = match recipient.as_ref() {
                                        Some(owner) => Some(owner.runtime()
                                            .selected_scheduler_batch_deadline_witness(&selected).await),
                                        None => None,
                                    };
                                    (issuer_batch, recipient_batch)
                                };
                                let status = |batch: &Batch| match batch {
                                    Batch::ForeignSelectedProvider => "foreign-provider",
                                    Batch::PipelineUnavailable => "pipeline-unavailable",
                                    Batch::NoPendingBatch => "no-pending-batch",
                                    Batch::Pending(_) => "pending",
                                };
                                let probe = if let (Batch::Pending(issuer_witness), Some(Batch::Pending(recipient_witness))) =
                                    (&issuer_batch, &recipient_batch)
                                {
                                    // Hold both exact original scheduler generations through
                                    // the one synchronized selected-provider time decision.
                                    match issuer_witness.with_current_deadline(|issuer_deadline, issuer_task| {
                                        recipient_witness.with_current_deadline(|recipient_deadline, recipient_task| {
                                            let now = self.clock.now_ms();
                                            let within_signed_bounds = issuer_deadline == recipient_deadline
                                                && issuer_deadline > now
                                                && issuer_deadline <= now.saturating_add(1_000)
                                                && issuer_deadline < self.signed_expiry
                                                && issuer_deadline < self.startup_deadline;
                                            let issuer_timer = self.clock.inspect_registered_deadline(issuer_deadline, issuer_task);
                                            let recipient_timer = self.clock.inspect_registered_deadline(recipient_deadline, recipient_task);
                                            let advanced = !retry_advanced && within_signed_bounds
                                                && self.clock.try_advance_exact_registered_deadline_set(
                                                    issuer_deadline, &[issuer_task, recipient_task],
                                                );
                                            format!("{retry_probe},issuer:{issuer_deadline}-{issuer_timer:?},recipient:{recipient_deadline}-{recipient_timer:?},bounded:{within_signed_bounds},advanced:{advanced}")
                                        })
                                    }) {
                                        Some(Some(result)) => result,
                                        _ => format!("{retry_probe},stale-original-scheduler-generation"),
                                    }
                                } else {
                                    format!("{retry_probe},issuer:{},recipient:{}", status(&issuer_batch),
                                        recipient_batch.as_ref().map_or("missing", &status))
                                };
                                if last_recovered_probe.as_ref() != Some(&probe) {
                                    eprintln!("recovered selected scheduler probe: {probe}");
                                    last_recovered_probe = Some(probe);
                                }
                            }
                            let next = self.clock.next_registered_deadline().await.map(|value| value.value());
                            let observation = (self.clock.now_ms(), next, first_frame_queued, progress.as_ref().map(|progress| progress.registration_present));
                            if last_recovered_observation != Some(observation) {
                                eprintln!("recovered selected-clock observation now={} next={:?} first_frame_queued={} registration_present={:?}", observation.0, observation.1, observation.2, observation.3);
                                last_recovered_observation = Some(observation);
                            }
                        }
                    }
                }
            }
        }).await;
        #[cfg(feature = "simulator-checkpoint")]
        if !advance_clock && !registration_seen {
            return result.map_err(|source| aura_core::AuraError::Invalid {
                message: "original recipient mailbox registration was not published".into(),
                source: Some(Arc::new(source)),
            });
        }
        result
    }

    async fn broker_progress(&self) -> OriginalBrokerProgress {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let authority = self
            .broker_url
            .strip_prefix("http://")
            .expect("loopback HTTP")
            .trim_end_matches('/');
        let mut stream = tokio::net::TcpStream::connect(authority)
            .await
            .expect("broker progress connection");
        let (subject, device, digest) = &self.selector;
        let path = format!(
            "/v2/bootstrap/enrollment-mailbox/progress-witness/{subject}/{device}/{}",
            hex::encode(digest),
        );
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
            self.auth_token,
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("authenticated progress request");
        let mut response = Vec::new();
        (&mut stream)
            .take(16 * 1024 + 1)
            .read_to_end(&mut response)
            .await
            .expect("bounded broker progress response");
        assert!(response.len() < 16 * 1024 && response.starts_with(b"HTTP/1.1 200 "));
        let divider = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .expect("broker response divider");
        let progress: serde_json::Value =
            serde_json::from_slice(&response[divider + 4..]).expect("broker progress JSON");
        assert_eq!(
            progress["subject"].as_str(),
            Some(subject.to_string().as_str())
        );
        assert_eq!(
            progress["device"].as_str(),
            Some(device.to_string().as_str())
        );
        assert_eq!(
            progress["manifest_digest"].as_str(),
            Some(hex::encode(digest).as_str())
        );
        let registration_present = progress["registration_present"]
            .as_bool()
            .expect("exact broker registration observation");
        let first_frame_queued = progress["messages"]
            .as_array()
            .is_some_and(|messages| !messages.is_empty());
        OriginalBrokerProgress {
            registration_present,
            first_frame_queued,
        }
    }
}
