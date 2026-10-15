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
    let selected = owner
        .enrollment
        .issuer()
        .runtime()
        .effects()
        .selected_filesystem_checkpoint_source_for_simulation()
        .expect("original issuer selected filesystem profile");
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
    let owner_id = aura_core::Hash32::from_value(&(subject, issuer_device, digest, authority))
        .expect("original stopped broker owner binding")
        .0;
    let fixture = aura_core::Hash32::from_value(&(participant, "signed-original-loopback"))
        .expect("signed fixture binding")
        .0;
    let context =
        StoppedBrokerCheckpointOwner::context_for_simulator(owner_id, 0, fixture, clock.now_ms());
    let mut sealed = stopped
        .seal_with_selected_profile(
            selected,
            roots[0].path(),
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
