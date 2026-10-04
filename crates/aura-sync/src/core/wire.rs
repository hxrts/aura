//! Small shared wire-format and timestamp helpers for sync protocols.

use crate::core::{sync_session_error, SyncResult};
use aura_core::effects::NetworkEffects;
use aura_core::time::PhysicalTime;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt::Display;
use uuid::Uuid;

/// Construct a deterministic `PhysicalTime` from a Unix-millisecond value.
pub fn physical_time_from_ms(timestamp_ms: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms: timestamp_ms,
        uncertainty: None,
    }
}

/// Serialize a payload to JSON bytes while preserving the sync error surface.
pub fn json_serialize<T: Serialize + ?Sized>(
    data_type: &str,
    description: &str,
    value: &T,
) -> SyncResult<Vec<u8>> {
    serde_json::to_vec(value).map_err(|err| {
        let diagnostic = {
            crate::core::errors::SyncDiagnostic::serialization(
                data_type,
                format!("Failed to serialize {description}: {err}"),
            )
        };
        crate::core::errors::sync_error_with_cause(diagnostic, err)
    })
}

/// Deserialize a payload from JSON bytes while preserving the sync error surface.
pub fn json_deserialize<T: DeserializeOwned>(
    data_type: &str,
    description: &str,
    bytes: &[u8],
) -> SyncResult<T> {
    serde_json::from_slice(bytes).map_err(|err| {
        let diagnostic = {
            crate::core::errors::SyncDiagnostic::serialization(
                data_type,
                format!("Failed to deserialize {description}: {err}"),
            )
        };
        crate::core::errors::sync_error_with_cause(diagnostic, err)
    })
}

/// Serialize a payload with Aura's binary codec while preserving sync error shaping.
pub fn binary_serialize<T: Serialize>(
    data_type: &str,
    description: &str,
    value: &T,
) -> SyncResult<Vec<u8>> {
    aura_core::util::serialization::to_vec(value).map_err(|err| {
        let diagnostic = {
            crate::core::errors::SyncDiagnostic::serialization(
                data_type,
                format!("Failed to serialize {description}: {err}"),
            )
        };
        crate::core::errors::sync_error_with_cause(diagnostic, err)
    })
}

/// Deserialize a payload with Aura's binary codec while preserving sync error shaping.
pub fn binary_deserialize<T: DeserializeOwned>(
    data_type: &str,
    description: &str,
    bytes: &[u8],
) -> SyncResult<T> {
    aura_core::util::serialization::from_slice(bytes).map_err(|err| {
        let diagnostic = {
            crate::core::errors::SyncDiagnostic::serialization(
                data_type,
                format!("Failed to deserialize {description}: {err}"),
            )
        };
        crate::core::errors::sync_error_with_cause(diagnostic, err)
    })
}

/// Send already-serialized bytes to a peer with a consistent network-error shape.
pub async fn send_bytes_to_peer<E, P>(
    effects: &E,
    peer_id: Uuid,
    peer: &P,
    description: &str,
    bytes: Vec<u8>,
) -> SyncResult<()>
where
    E: NetworkEffects + Send + Sync,
    P: Display + ?Sized,
{
    effects.send_to_peer(peer_id, bytes).await.map_err(|err| {
        let diagnostic = {
            crate::core::errors::SyncDiagnostic::network(format!(
                "Failed to send {description} to peer {peer}: {err}"
            ))
        };
        crate::core::errors::sync_error_with_cause(diagnostic, err)
    })
}

/// Upper bound on stale or out-of-step frames skipped while waiting for one kind.
const MAX_SKIPPED_FRAMES: usize = 32;

/// Wire frame tagging each sync message with its kind, so a session never
/// mistakes a peer's concurrent request for the reply it is waiting on.
#[derive(serde::Serialize, serde::Deserialize)]
struct SyncFrame<T> {
    kind: String,
    body: T,
}

/// Serialize a JSON payload as a `kind`-tagged frame and send it to a peer.
pub async fn send_json_to_peer<E, T, P>(
    effects: &E,
    peer_id: Uuid,
    peer: &P,
    kind: &str,
    description: &str,
    value: &T,
) -> SyncResult<()>
where
    E: NetworkEffects + Send + Sync,
    T: Serialize + ?Sized,
    P: Display + ?Sized,
{
    let frame = SyncFrame {
        kind: kind.to_string(),
        body: value,
    };
    let bytes = json_serialize(kind, description, &frame)?;
    send_bytes_to_peer(effects, peer_id, peer, description, bytes).await
}

/// Receive the next `kind` frame from the expected peer and deserialize it.
///
/// Frames from other peers stay queued for their own sessions; frames of a
/// different kind from this peer are stale steps of an earlier round and are
/// dropped.
pub async fn receive_json_from_expected_peer<E, T, P>(
    effects: &E,
    expected_peer_id: Uuid,
    expected_peer: &P,
    kind: &str,
    description: &str,
) -> SyncResult<T>
where
    E: NetworkEffects + Send + Sync,
    T: DeserializeOwned,
    P: Display + ?Sized,
{
    for _ in 0..MAX_SKIPPED_FRAMES {
        let payload =
            receive_bytes_from_expected_peer(effects, expected_peer_id, expected_peer, description)
                .await?;
        let frame: SyncFrame<serde_json::Value> = json_deserialize(kind, description, &payload)?;
        if frame.kind != kind {
            tracing::debug!(
                peer = %expected_peer,
                expected_kind = kind,
                received_kind = %frame.kind,
                "Dropping out-of-step sync frame"
            );
            continue;
        }
        return serde_json::from_value(frame.body).map_err(|err| {
            let diagnostic = {
                crate::core::errors::SyncDiagnostic::serialization(
                    kind,
                    format!("Failed to deserialize {description}: {err}"),
                )
            };
            crate::core::errors::sync_error_with_cause(diagnostic, err)
        });
    }
    Err(sync_session_error(format!(
        "No {description} from peer {expected_peer} after {MAX_SKIPPED_FRAMES} out-of-step frames"
    )))
}

async fn receive_bytes_from_expected_peer<E, P>(
    effects: &E,
    expected_peer_id: Uuid,
    expected_peer: &P,
    description: &str,
) -> SyncResult<Vec<u8>>
where
    E: NetworkEffects + Send + Sync,
    P: Display + ?Sized,
{
    let receive_error = |err| {
        let diagnostic = crate::core::errors::SyncDiagnostic::network(format!(
            "Failed to receive {description} from peer {expected_peer}: {err}"
        ));
        crate::core::errors::sync_error_with_cause(diagnostic, err)
    };
    match effects.receive_from(expected_peer_id).await {
        Ok(payload) => Ok(payload),
        Err(aura_core::effects::NetworkError::NotImplemented) => {
            let (sender_id, payload) = effects.receive().await.map_err(receive_error)?;
            if sender_id != expected_peer_id {
                return Err(sync_session_error(format!(
                    "Received {description} from unexpected peer: expected {expected_peer}, got {sender_id}"
                )));
            }
            Ok(payload)
        }
        Err(err) => Err(receive_error(err)),
    }
}

/// Serialize a JSON request, send it to the expected peer, and decode the JSON response.
pub async fn exchange_json_with_peer<E, Req, Resp, P>(
    effects: &E,
    peer_id: Uuid,
    peer: &P,
    request_type: &str,
    request_description: &str,
    request: &Req,
    response_type: &str,
    response_description: &str,
) -> SyncResult<Resp>
where
    E: NetworkEffects + Send + Sync,
    Req: Serialize + ?Sized,
    Resp: DeserializeOwned,
    P: Display + ?Sized,
{
    send_json_to_peer(
        effects,
        peer_id,
        peer,
        request_type,
        request_description,
        request,
    )
    .await?;
    receive_json_from_expected_peer(effects, peer_id, peer, response_type, response_description)
        .await
}

#[cfg(test)]
mod required_source_tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn required_sync_json_codec_retains_actual_malformed_payload_source() {
        let failure =
            json_deserialize::<serde_json::Value>("sync_request", "required sync request", b"{")
                .expect_err("actual malformed JSON cannot become successful sync data");
        assert!(matches!(
            failure,
            aura_core::AuraError::Serialization { .. }
        ));
        let original = failure
            .source()
            .expect("actual codec source")
            .downcast_ref::<serde_json::Error>()
            .expect("native JSON error directly downcasts");
        assert!(original.is_eof());
        let cloned = failure.clone();
        assert!(cloned
            .source()
            .expect("clone retains original native cause")
            .is::<serde_json::Error>());
    }
}
