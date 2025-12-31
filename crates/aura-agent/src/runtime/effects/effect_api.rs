use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::DeviceId;
use aura_protocol::effects::{EffectApiEffects, EffectApiError, EffectApiEventStream};

// Implementation of EffectApiEffects
#[async_trait]
impl EffectApiEffects for AuraEffectSystem {
    async fn append_event(&self, _event: Vec<u8>) -> Result<(), EffectApiError> {
        self.ensure_mock_effect_api("append_event")?;
        Ok(())
    }

    async fn current_epoch(&self) -> Result<u64, EffectApiError> {
        self.ensure_mock_effect_api("current_epoch")?;
        Ok(0)
    }

    async fn events_since(&self, _epoch: u64) -> Result<Vec<Vec<u8>>, EffectApiError> {
        self.ensure_mock_effect_api("events_since")?;
        Ok(vec![])
    }

    async fn is_device_authorized(
        &self,
        _device_id: DeviceId,
        _operation: &str,
    ) -> Result<bool, EffectApiError> {
        self.ensure_mock_effect_api("is_device_authorized")?;
        Ok(true)
    }

    async fn update_device_activity(&self, _device_id: DeviceId) -> Result<(), EffectApiError> {
        self.ensure_mock_effect_api("update_device_activity")?;
        Ok(())
    }

    async fn subscribe_to_events(&self) -> Result<EffectApiEventStream, EffectApiError> {
        self.ensure_mock_effect_api("subscribe_to_events")?;
        Err(EffectApiError::CryptoOperationFailed {
            message: "subscribe_to_events not implemented in mock".to_string(),
        })
    }

    async fn would_create_cycle(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _new_edge: (Vec<u8>, Vec<u8>),
    ) -> Result<bool, EffectApiError> {
        self.ensure_mock_effect_api("would_create_cycle")?;
        Ok(false)
    }

    async fn find_connected_components(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<Vec<u8>>>, EffectApiError> {
        self.ensure_mock_effect_api("find_connected_components")?;
        Ok(vec![])
    }

    async fn topological_sort(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<u8>>, EffectApiError> {
        self.ensure_mock_effect_api("topological_sort")?;
        Ok(vec![])
    }

    async fn shortest_path(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _start: Vec<u8>,
        _end: Vec<u8>,
    ) -> Result<Option<Vec<Vec<u8>>>, EffectApiError> {
        self.ensure_mock_effect_api("shortest_path")?;
        Ok(None)
    }

    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, EffectApiError> {
        Ok(self.random_bytes(length).await)
    }

    async fn hash_data(&self, data: &[u8]) -> Result<[u8; 32], EffectApiError> {
        // Mock implementation - simple hash
        use aura_core::hash::hash;
        Ok(hash(data))
    }

    async fn current_timestamp(&self) -> Result<u64, EffectApiError> {
        // Use PhysicalTimeEffects instead of direct SystemTime
        let physical_time =
            self.time_handler
                .physical_time()
                .await
                .map_err(|e| EffectApiError::Backend {
                    error: format!("time unavailable: {e}"),
                })?;
        Ok(physical_time.ts_ms / 1000)
    }

    async fn effect_api_device_id(&self) -> Result<DeviceId, EffectApiError> {
        Ok(self.device_id())
    }

    #[allow(clippy::disallowed_methods)]
    async fn new_uuid(&self) -> Result<uuid::Uuid, EffectApiError> {
        // Mock implementation
        Ok(uuid::Uuid::new_v4())
    }
}

