//! Memory-based choreographic handler for testing

use crate::effects::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError, ChoreographyEvent,
    ChoreographyMetrics,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Memory-based choreographic handler for testing
pub struct MemoryChoreographicHandler {
    device_id: Uuid,
    role_index: usize,
    active_roles: Arc<Mutex<Vec<ChoreographicRole>>>,
    message_queue: Arc<Mutex<HashMap<ChoreographicRole, Vec<Vec<u8>>>>>,
    metrics: Arc<Mutex<ChoreographyMetrics>>,
}

impl MemoryChoreographicHandler {
    /// Create a new memory-based choreographic handler
    pub fn new(device_id: Uuid) -> Self {
        Self {
            device_id,
            role_index: 0,
            active_roles: Arc::new(Mutex::new(vec![])),
            message_queue: Arc::new(Mutex::new(HashMap::new())),
            metrics: Arc::new(Mutex::new(ChoreographyMetrics {
                messages_sent: 0,
                messages_received: 0,
                avg_latency_ms: 0.0,
                timeout_count: 0,
                retry_count: 0,
                total_duration_ms: 0,
            })),
        }
    }
}

#[async_trait]
impl ChoreographicEffects for MemoryChoreographicHandler {
    async fn send_to_role_bytes(
        &self,
        role: ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), ChoreographyError> {
        let mut queue = self.message_queue.lock().unwrap();
        queue.entry(role).or_insert_with(Vec::new).push(message);

        let mut metrics = self.metrics.lock().unwrap();
        metrics.messages_sent += 1;

        Ok(())
    }

    async fn receive_from_role_bytes(
        &self,
        role: ChoreographicRole,
    ) -> Result<Vec<u8>, ChoreographyError> {
        let mut queue = self.message_queue.lock().unwrap();
        if let Some(messages) = queue.get_mut(&role) {
            if let Some(message) = messages.pop() {
                let mut metrics = self.metrics.lock().unwrap();
                metrics.messages_received += 1;
                return Ok(message);
            }
        }

        Err(ChoreographyError::CommunicationTimeout {
            role,
            timeout_ms: 1000,
        })
    }

    async fn broadcast_bytes(&self, message: Vec<u8>) -> Result<(), ChoreographyError> {
        let roles = self.active_roles.lock().unwrap().clone();
        for role in roles {
            if role.device_id != self.device_id {
                self.send_to_role_bytes(role, message.clone()).await?;
            }
        }
        Ok(())
    }

    fn current_role(&self) -> ChoreographicRole {
        ChoreographicRole {
            device_id: self.device_id,
            role_index: self.role_index,
        }
    }

    fn all_roles(&self) -> Vec<ChoreographicRole> {
        self.active_roles.lock().unwrap().clone()
    }

    async fn is_role_active(&self, role: ChoreographicRole) -> bool {
        self.active_roles.lock().unwrap().contains(&role)
    }

    async fn start_session(
        &self,
        _session_id: Uuid,
        roles: Vec<ChoreographicRole>,
    ) -> Result<(), ChoreographyError> {
        *self.active_roles.lock().unwrap() = roles;
        Ok(())
    }

    async fn end_session(&self) -> Result<(), ChoreographyError> {
        self.active_roles.lock().unwrap().clear();
        self.message_queue.lock().unwrap().clear();
        Ok(())
    }

    async fn emit_choreo_event(&self, _event: ChoreographyEvent) -> Result<(), ChoreographyError> {
        // For memory handler, just ignore events
        Ok(())
    }

    async fn set_timeout(&self, _timeout_ms: u64) {
        // For memory handler, timeouts are immediate
    }

    async fn get_metrics(&self) -> ChoreographyMetrics {
        self.metrics.lock().unwrap().clone()
    }
}
