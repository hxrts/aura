//! Choreographic adapter for ChoreoHandler interface

use crate::{
    core::Transport, MessageMetadata, MessagePriority, TransportEnvelope, TransportResult,
};
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

/// Adapter that bridges ChoreoHandler to core Transport
pub struct ChoreographicAdapter {
    transport: Arc<dyn Transport>,
    device_id: DeviceId,
}

impl ChoreographicAdapter {
    /// Create a new choreographic adapter with the given transport and device ID
    pub fn new(transport: Arc<dyn Transport>, device_id: DeviceId) -> Self {
        Self {
            transport,
            device_id,
        }
    }

    /// Send choreographic message
    pub async fn send_choreo_message<M>(&self, to: DeviceId, message: &M) -> TransportResult<()>
    where
        M: Serialize,
    {
        let payload = aura_types::serialization::bincode::to_bincode_bytes(message)
            .map_err(|e| crate::error::TransportErrorBuilder::transport(e.to_string()))?;

        let envelope = TransportEnvelope {
            from: self.device_id,
            to,
            #[allow(clippy::disallowed_methods)]
            message_id: Uuid::new_v4(),
            payload,
            metadata: MessageMetadata {
                timestamp: aura_types::time_utils::current_unix_timestamp_millis(),
                message_type: "choreographic".to_string(),
                priority: MessagePriority::Normal,
            },
        };

        self.transport.send(envelope).await
    }

    /// Receive choreographic message
    pub async fn receive_choreo_message<M>(
        &self,
        timeout: Duration,
    ) -> TransportResult<Option<(DeviceId, M)>>
    where
        M: for<'de> Deserialize<'de>,
    {
        match self.transport.receive(timeout).await? {
            Some(envelope) => {
                let message: M =
                    aura_types::serialization::bincode::from_bincode_bytes(&envelope.payload)
                        .map_err(|e| {
                            crate::error::TransportErrorBuilder::transport(e.to_string())
                        })?;
                Ok(Some((envelope.from, message)))
            }
            None => Ok(None),
        }
    }
}
