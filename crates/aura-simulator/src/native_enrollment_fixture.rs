//! Shared simulation-only native broker setup and selected physical-clock drive.
//! The signed issuer and runtime remain the production owners; this module only
//! arranges their test environment and observes authenticated broker progress.
use std::{future::Future, sync::Arc, time::Duration};

use aura_agent::{
    BootstrapBrokerConfig, BootstrapBrokerLanBindPolicy, EnrollmentParticipantProfile,
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
    if let Some(next) = clock.next_registered_deadline().await {
        let now = clock.now_ms();
        if next.value() > now
            && next.value() <= now + 1_000
            && next.value() < signed_expiry
            && next.value() < enrollment.original_startup_window().deadline_at_ms()
        {
            clock.set_time(next.value());
        }
    }
}

/// An original signed issuance with its selected native provider and broker owner.
pub struct NativeEnrollmentFixture {
    pub enrollment: SimulatedEnrollment,
    pub supervisor: TaskSupervisor,
    pub clock: Arc<ManualPhysicalClock>,
    broker_url: String,
    auth_token: String,
}

impl NativeEnrollmentFixture {
    /// Bind a loopback broker and publish the original signed issuer registration.
    pub async fn prepare(
        profiles: [EnrollmentParticipantProfile; 2],
        clock: Arc<ManualPhysicalClock>,
        auth_token: &str,
    ) -> Result<Self, String> {
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
    ) -> Result<Self, String> {
        Self::prepare_inner(profiles, clock, auth_token, Some(retirements)).await
    }

    async fn prepare_inner(
        mut profiles: [EnrollmentParticipantProfile; 2],
        clock: Arc<ManualPhysicalClock>,
        auth_token: &str,
        retirements: Option<[Arc<dyn aura_core::crypto::tree_signing::FrostNonceRetirement>; 2]>,
    ) -> Result<Self, String> {
        let broker = LocalBootstrapBrokerService::bind(
            "127.0.0.1:0",
            Duration::from_secs(120),
            auth_token,
            BootstrapBrokerLanBindPolicy::LoopbackOnly,
        )
        .await
        .map_err(|error| format!("broker bind: {error}"))?
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
        .map_err(|error| format!("startup window: {error}"))?;
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
        .map_err(|error| format!("signed issuance: {error}"))?;
        enrollment
            .issuer()
            .drain_issuer_return_vm_mailbox()
            .await
            .map_err(|error| format!("issuer registration: {error}"))?;
        Ok(Self {
            enrollment,
            supervisor,
            clock,
            broker_url,
            auth_token: auth_token.to_owned(),
        })
    }

    pub fn broker_url(&self) -> &str {
        &self.broker_url
    }

    /// Drive the selected clock only while no original broker frame is queued.
    pub async fn drive_until<T, M, F>(
        &self,
        mut maintenance_pass: M,
        goal: impl Future<Output = T>,
    ) -> T
    where
        M: FnMut() -> F,
        F: Future<Output = ()>,
    {
        let (subject, device, digest) = self
            .enrollment
            .original_mailbox_selector()
            .expect("retained original mailbox selector");
        let mut goal = Box::pin(goal);
        let mut maintenance = Box::pin(maintenance_pass());
        let issuer = self.enrollment.issuer().clone();
        let mut issuer_drain = Some(Box::pin(issuer.drain_issuer_return_vm_mailbox()));
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        let mut first_frame_queued = false;
        aura_testkit::infrastructure::time::assertions::assert_completes_within(
            Duration::from_secs(30),
            async {
            loop {
                tokio::select! {
                    outcome = &mut goal => break Ok(outcome),
                    _ = &mut maintenance => maintenance = Box::pin(maintenance_pass()),
                    _ = async { match issuer_drain.as_mut() {
                        Some(drain) => drain.await,
                        None => std::future::pending().await,
                    }} => issuer_drain = None,
                    _ = tick.tick() => {
                        if issuer_drain.is_none() {
                            issuer_drain = Some(Box::pin(issuer.drain_issuer_return_vm_mailbox()));
                        }
                        if !first_frame_queued {
                            first_frame_queued = self.first_frame_queued(subject, device, digest).await;
                        }
                        advance_selected_clock_before_frame(&self.enrollment, &self.clock, first_frame_queued).await;
                    }
                }
            }
        }).await.expect("bounded original signed import completion")
    }

    async fn first_frame_queued(
        &self,
        subject: AuthorityId,
        device: DeviceId,
        digest: [u8; 32],
    ) -> bool {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let authority = self
            .broker_url
            .strip_prefix("http://")
            .expect("loopback HTTP")
            .trim_end_matches('/');
        let mut stream = tokio::net::TcpStream::connect(authority)
            .await
            .expect("broker progress connection");
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
        progress["messages"]
            .as_array()
            .is_some_and(|messages| !messages.is_empty())
    }
}
