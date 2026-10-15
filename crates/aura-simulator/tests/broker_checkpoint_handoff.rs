//! Simulator-only proof that signed setup retains its original broker owner.

#![cfg(feature = "simulator-checkpoint")]

use std::sync::Arc;

use aura_agent::{AgentConfig, EnrollmentParticipantProfile};
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
    let clock = Arc::new(ManualPhysicalClock::new(5_000));
    let mut owner = NativeEnrollmentFixture::prepare(
        original_vm_profiles(&roots),
        clock,
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
        .trim_end_matches('/');
    let mut stream = tokio::net::TcpStream::connect(authority)
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
    let stopped = owner
        .stop_original_broker_for_checkpoint()
        .await
        .expect("exact original broker task group disposed");
    let bytes = stopped
        .into_unsealed_registrations()
        .expect("original signed registration codec");
    let snapshot: serde_json::Value =
        serde_json::from_slice(&bytes).expect("versioned broker codec");
    assert_eq!(snapshot["version"], 1);
    assert_eq!(snapshot["returns"].as_array().map(Vec::len), Some(1));
    assert!(owner.supervisor.active_tasks().is_empty());
    assert!(owner.stop_original_broker_for_checkpoint().await.is_err());
    let _ = owner
        .enrollment
        .fail(aura_agent::AgentError::config(
            "fixture stops after original broker disposal",
        ))
        .await;
}
