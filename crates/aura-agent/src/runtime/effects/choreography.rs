use super::{AuraEffectSystem, CHOREO_FLOW_COST_PER_KB, DEFAULT_CHOREO_FLOW_COST};
use async_trait::async_trait;
use aura_core::effects::transport::{TransportEnvelope, TransportReceipt};
use aura_core::effects::{PhysicalTimeEffects, TransportEffects};
use aura_core::hash::hash;
use aura_core::{AuthorityId, ContextId, DeviceId, FlowCost};
use aura_guards::prelude::create_send_guard_op;
use aura_guards::{GuardOperation, JournalCoupler};
use aura_protocol::effects::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError, ChoreographyEvent,
    ChoreographyMetrics, RoleIndex,
};
use std::collections::HashMap;

// Implementation of ChoreographicEffects
#[async_trait]
impl ChoreographicEffects for AuraEffectSystem {
    async fn send_to_role_bytes(
        &self,
        role: ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), ChoreographyError> {
        let (context_id, current_role) = {
            let state = self.choreography_state.read();
            (
                state
                    .context_id
                    .ok_or(ChoreographyError::SessionNotStarted)?,
                state
                    .current_role
                    .ok_or(ChoreographyError::SessionNotStarted)?,
            )
        };

        let peer = AuthorityId::from_uuid(role.device_id.0);
        eprintln!(
            "[DEBUG] Choreography send: from {:?} to {:?} (authority {}), context {:?}, {} bytes",
            current_role.device_id,
            role.device_id,
            peer,
            context_id,
            message.len()
        );
        let kb_units = ((message.len() as u32).saturating_add(1023)) / 1024;
        let flow_cost = DEFAULT_CHOREO_FLOW_COST
            .saturating_add(kb_units.saturating_mul(CHOREO_FLOW_COST_PER_KB));

        let guard_chain = create_send_guard_op(
            GuardOperation::Custom("choreography:send".to_string()),
            context_id,
            peer,
            FlowCost::new(flow_cost),
        )
        .with_operation_id(format!(
            "choreography_send_{:?}_{:?}",
            context_id, role.device_id
        ));

        let guard_result =
            guard_chain
                .evaluate(self)
                .await
                .map_err(|e| ChoreographyError::InternalError {
                    message: format!("Choreography send guard failed: {e}"),
                })?;

        if !guard_result.authorized {
            return Err(ChoreographyError::InternalError {
                message: guard_result
                    .denial_reason
                    .unwrap_or_else(|| "Choreography send denied by guard chain".to_string()),
            });
        }

        JournalCoupler::new()
            .couple_with_send(self, &guard_result.receipt)
            .await
            .map_err(|e| ChoreographyError::InternalError {
                message: format!("Choreography journal coupling failed: {e}"),
            })?;

        let transport_receipt = guard_result
            .receipt
            .as_ref()
            .map(|receipt| TransportReceipt {
                context: receipt.ctx,
                src: receipt.src,
                dst: receipt.dst,
                epoch: receipt.epoch.value(),
                cost: receipt.cost.value(),
                nonce: receipt.nonce.value(),
                prev: receipt.prev.0,
                sig: receipt.sig.clone().into_bytes(),
            });

        // Include choreography metadata so receivers can identify and route these messages
        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            "application/aura-choreography".to_string(),
        );

        // Include session_id so guardians can join the correct session
        {
            let state = self.choreography_state.read();
            if let Some(session_id) = state.session_id {
                metadata.insert("session-id".to_string(), session_id.to_string());
            }
        }

        let envelope = TransportEnvelope {
            destination: peer,
            source: AuthorityId::from_uuid(current_role.device_id.0),
            context: context_id,
            payload: message,
            metadata,
            receipt: transport_receipt,
        };

        TransportEffects::send_envelope(self, envelope)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })?;

        {
            let mut state = self.choreography_state.write();
            state.metrics.messages_sent = state.metrics.messages_sent.saturating_add(1);
        }
        Ok(())
    }

    #[allow(clippy::disallowed_methods)] // Instant::now() legitimate for network receive timeout
    async fn receive_from_role_bytes(
        &self,
        role: ChoreographicRole,
    ) -> Result<Vec<u8>, ChoreographyError> {
        let context_id = {
            let state = self.choreography_state.read();
            state
                .context_id
                .ok_or(ChoreographyError::SessionNotStarted)?
        };

        // Poll for messages with timeout to allow async guardians time to respond.
        // Default timeout of 5 seconds with 50ms polling interval.
        let timeout_ms = {
            let state = self.choreography_state.read();
            state.timeout_ms.unwrap_or(5000)
        };
        let start = std::time::Instant::now();
        let poll_interval = std::time::Duration::from_millis(50);

        let source_authority = AuthorityId::from_uuid(role.device_id.0);
        tracing::debug!(
            "Choreography receive: waiting for message from {:?} (authority {:?}) in context {:?}, timeout={}ms",
            role.device_id,
            source_authority,
            context_id,
            timeout_ms
        );

        let envelope = loop {
            match TransportEffects::receive_envelope_from(
                self,
                AuthorityId::from_uuid(role.device_id.0),
                context_id,
            )
            .await
            {
                Ok(env) => break env,
                Err(aura_core::effects::TransportError::NoMessage) => {
                    // Check timeout
                    if start.elapsed().as_millis() as u64 > timeout_ms {
                        return Err(ChoreographyError::Transport {
                            source: Box::new(aura_core::effects::TransportError::NoMessage),
                        });
                    }
                    // Yield to allow other tasks (like DemoSimulator) to process
                    tokio::time::sleep(poll_interval).await;
                }
                Err(e) => {
                    return Err(ChoreographyError::Transport {
                        source: Box::new(e),
                    });
                }
            }
        };

        {
            let mut state = self.choreography_state.write();
            state.metrics.messages_received = state.metrics.messages_received.saturating_add(1);
        }

        Ok(envelope.payload)
    }

    async fn broadcast_bytes(&self, message: Vec<u8>) -> Result<(), ChoreographyError> {
        let (roles, current_role) = {
            let state = self.choreography_state.read();
            (
                state.roles.clone(),
                state
                    .current_role
                    .ok_or(ChoreographyError::SessionNotStarted)?,
            )
        };

        for role in roles {
            if role.device_id == current_role.device_id {
                continue;
            }
            self.send_to_role_bytes(role, message.clone()).await?;
        }

        Ok(())
    }

    #[allow(clippy::disallowed_methods)]
    fn current_role(&self) -> ChoreographicRole {
        let state = self.choreography_state.read();
        state.current_role.unwrap_or_else(|| {
            let role_index = RoleIndex::new(0).expect("role index");
            ChoreographicRole::new(DeviceId::from_uuid(self.authority_id.0), role_index)
        })
    }

    fn all_roles(&self) -> Vec<ChoreographicRole> {
        let state = self.choreography_state.read();
        if state.roles.is_empty() {
            vec![self.current_role()]
        } else {
            state.roles.clone()
        }
    }

    async fn is_role_active(&self, role: ChoreographicRole) -> bool {
        let context_id = {
            let state = self.choreography_state.read();
            match state.context_id {
                Some(context_id) => context_id,
                None => return false,
            }
        };

        TransportEffects::is_channel_established(
            self,
            context_id,
            AuthorityId::from_uuid(role.device_id.0),
        )
        .await
    }

    async fn start_session(
        &self,
        session_id: uuid::Uuid,
        roles: Vec<ChoreographicRole>,
    ) -> Result<(), ChoreographyError> {
        let current_device = DeviceId::from_uuid(self.authority_id.0);
        let current_role = roles
            .iter()
            .find(|role| role.device_id == current_device)
            .copied()
            .ok_or_else(|| {
                let role_index = RoleIndex::new(0).expect("role index");
                ChoreographyError::RoleNotFound {
                    role: ChoreographicRole::new(current_device, role_index),
                }
            })?;

        let context_id = ContextId::new_from_entropy(hash(session_id.as_bytes()));
        tracing::debug!(
            "Choreography start_session: session_id={}, context_id={:?}, authority={:?}, roles={:?}",
            session_id,
            context_id,
            self.authority_id,
            roles.iter().map(|r| r.device_id).collect::<Vec<_>>()
        );
        let started_at_ms = self
            .physical_time()
            .await
            .map(|time| time.ts_ms)
            .unwrap_or_default();

        let mut state = self.choreography_state.write();
        if let Some(active) = state.session_id {
            return Err(ChoreographyError::SessionAlreadyExists { session_id: active });
        }

        state.session_id = Some(session_id);
        state.context_id = Some(context_id);
        state.roles = roles;
        state.current_role = Some(current_role);
        state.started_at_ms = Some(started_at_ms);
        Ok(())
    }

    async fn end_session(&self) -> Result<(), ChoreographyError> {
        let ended_at_ms = self
            .physical_time()
            .await
            .map(|time| time.ts_ms)
            .unwrap_or_default();

        let mut state = self.choreography_state.write();
        if state.session_id.is_none() {
            return Err(ChoreographyError::SessionNotStarted);
        }

        if let Some(started_at_ms) = state.started_at_ms {
            state.metrics.total_duration_ms = ended_at_ms.saturating_sub(started_at_ms);
        }

        state.session_id = None;
        state.context_id = None;
        state.roles.clear();
        state.current_role = None;
        state.timeout_ms = None;
        state.started_at_ms = None;
        Ok(())
    }

    async fn emit_choreo_event(&self, event: ChoreographyEvent) -> Result<(), ChoreographyError> {
        tracing::debug!(?event, "choreography event");
        Ok(())
    }

    async fn set_timeout(&self, timeout_ms: u64) {
        let mut state = self.choreography_state.write();
        state.timeout_ms = Some(timeout_ms);
    }

    async fn get_metrics(&self) -> ChoreographyMetrics {
        let state = self.choreography_state.read();
        state.metrics.clone()
    }
}
