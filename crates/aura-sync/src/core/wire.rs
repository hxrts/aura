//! Small shared wire-format and timestamp helpers for sync protocols.

use crate::core::{sync_network_error, sync_serialization_error, sync_session_error, SyncResult};
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
        sync_serialization_error(
            data_type,
            format!("Failed to serialize {description}: {err}"),
        )
    })
}

/// Deserialize a payload from JSON bytes while preserving the sync error surface.
pub fn json_deserialize<T: DeserializeOwned>(
    data_type: &str,
    description: &str,
    bytes: &[u8],
) -> SyncResult<T> {
    serde_json::from_slice(bytes).map_err(|err| {
        sync_serialization_error(
            data_type,
            format!("Failed to deserialize {description}: {err}"),
        )
    })
}

/// Serialize a payload with Aura's binary codec while preserving sync error shaping.
pub fn binary_serialize<T: Serialize>(
    data_type: &str,
    description: &str,
    value: &T,
) -> SyncResult<Vec<u8>> {
    aura_core::util::serialization::to_vec(value).map_err(|err| {
        sync_serialization_error(
            data_type,
            format!("Failed to serialize {description}: {err}"),
        )
    })
}

/// Deserialize a payload with Aura's binary codec while preserving sync error shaping.
pub fn binary_deserialize<T: DeserializeOwned>(
    data_type: &str,
    description: &str,
    bytes: &[u8],
) -> SyncResult<T> {
    aura_core::util::serialization::from_slice(bytes).map_err(|err| {
        sync_serialization_error(
            data_type,
            format!("Failed to deserialize {description}: {err}"),
        )
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
        sync_network_error(format!(
            "Failed to send {description} to peer {peer}: {err}"
        ))
    })
}

/// Receive a JSON payload from the expected peer and deserialize it.
pub async fn receive_json_from_expected_peer<E, T, P>(
    effects: &E,
    expected_peer_id: Uuid,
    expected_peer: &P,
    data_type: &str,
    description: &str,
) -> SyncResult<T>
where
    E: NetworkEffects + Send + Sync,
    T: DeserializeOwned,
    P: Display + ?Sized,
{
    let (sender_id, payload) = effects.receive().await.map_err(|err| {
        sync_network_error(format!(
            "Failed to receive {description} from peer {expected_peer}: {err}"
        ))
    })?;

    if sender_id != expected_peer_id {
        return Err(sync_session_error(format!(
            "Received {description} from unexpected peer: expected {expected_peer}, got {sender_id}"
        )));
    }

    json_deserialize(data_type, description, &payload)
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
    let request_data = json_serialize(request_type, request_description, request)?;
    send_bytes_to_peer(effects, peer_id, peer, request_description, request_data).await?;
    receive_json_from_expected_peer(effects, peer_id, peer, response_type, response_description)
        .await
}
