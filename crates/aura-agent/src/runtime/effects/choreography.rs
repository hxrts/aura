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

use crate::runtime::subsystems::choreography::RuntimeChoreographySessionId;

fn current_session_snapshot(
    effects: &AuraEffectSystem,
) -> Result<crate::runtime::subsystems::choreography::ChoreographySessionState, ChoreographyError> {
    effects
        .choreography_state
        .read()
        .current_session()
        .ok_or(ChoreographyError::SessionNotStarted)
}

fn take_session_envelope(
    effects: &AuraEffectSystem,
    session_id: RuntimeChoreographySessionId,
    source: AuthorityId,
    context: ContextId,
) -> Option<TransportEnvelope> {
    let self_device_id = effects.config.device_id.to_string();
    effects
        .choreography_state
        .write()
        .take_matching_session_envelope(
            session_id,
            source,
            context,
            effects.authority_id,
            &self_device_id,
        )
}

fn promote_shared_session_envelopes(
    effects: &AuraEffectSystem,
    session_id: RuntimeChoreographySessionId,
) {
    let Some(shared) = effects.transport.shared_transport() else {
        return;
    };
    let session_ref = session_id.to_string();
    let inbox = shared.inbox_for(effects.authority_id);
    let mut inbox = inbox.write();
    let mut promoted = Vec::new();

    let mut index = 0usize;
    while index < inbox.len() {
        let matches_session = inbox[index]
            .metadata
            .get("content-type")
            .is_some_and(|value| value == "application/aura-choreography")
            && inbox[index]
                .metadata
                .get("session-id")
                .is_some_and(|value| value == &session_ref);
        if matches_session {
            promoted.push(inbox.remove(index));
        } else {
            index += 1;
        }
    }
    drop(inbox);

    if promoted.is_empty() {
        return;
    }

    let mut state = effects.choreography_state.write();
    for envelope in promoted {
        state.queue_session_envelope(session_id, envelope);
    }
}

// Implementation of ChoreographicEffects
#[async_trait]
impl ChoreographicEffects for AuraEffectSystem {
    async fn send_to_role_bytes(
        &self,
        role: ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), ChoreographyError> {
        let session = current_session_snapshot(self)?;
        let context_id = session.context_id;
        let current_role = session.current_role;

        let peer = AuthorityId::from_uuid(role.device_id.0);
        tracing::debug!(
            session_id = %session.session_id,
            from = ?current_role.device_id,
            to = ?role.device_id,
            peer = %peer,
            ?context_id,
            bytes = message.len(),
            "choreography send"
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
            "choreography_send_{}_{}_{:?}",
            session.session_id, context_id, role.device_id
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
        metadata.insert("session-id".to_string(), session.session_id.to_string());

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
            state
                .with_current_session_mut(|session| {
                    session.metrics.messages_sent = session.metrics.messages_sent.saturating_add(1);
                })
                .map_err(|message| ChoreographyError::InternalError { message })?;
        }
        Ok(())
    }

    #[allow(clippy::disallowed_methods)] // Instant::now() legitimate for network receive timeout
    async fn receive_from_role_bytes(
        &self,
        role: ChoreographicRole,
    ) -> Result<Vec<u8>, ChoreographyError> {
        let session = current_session_snapshot(self)?;
        let context_id = session.context_id;
        let session_id = session.session_id;
        let session_inbox_notify = self
            .choreography_state
            .read()
            .session_inbox_notify(session_id);
        let shared_inbox_notify = self
            .transport
            .shared_transport()
            .map(|shared| shared.inbox_notify(self.authority_id));

        // Wait on the session-local inbox notifier instead of polling the global inbox.
        // Default timeout remains 5 seconds to allow async guardians time to respond.
        let timeout_ms = session.timeout_ms.unwrap_or(5000);
        let start = aura_effects::time::monotonic_now();

        let source_authority = AuthorityId::from_uuid(role.device_id.0);
        tracing::debug!(
            session_id = %session_id,
            "Choreography receive: waiting for message from {:?} (authority {:?}) in context {:?}, timeout={}ms",
            role.device_id,
            source_authority,
            context_id,
            timeout_ms
        );

        let envelope = loop {
            if let Some(env) = take_session_envelope(self, session_id, source_authority, context_id)
            {
                self.transport.record_receive();
                break env;
            }

            promote_shared_session_envelopes(self, session_id);
            if let Some(env) = take_session_envelope(self, session_id, source_authority, context_id)
            {
                self.transport.record_receive();
                break env;
            }

            let elapsed_ms = start.elapsed().as_millis() as u64;
            if elapsed_ms >= timeout_ms {
                let mut state = self.choreography_state.write();
                let _ = state.with_current_session_mut(|session| {
                    session.metrics.timeout_count = session.metrics.timeout_count.saturating_add(1);
                });
                return Err(ChoreographyError::Transport {
                    source: Box::new(aura_core::effects::TransportError::NoMessage),
                });
            }

            let Some(session_inbox_notify) = session_inbox_notify.clone() else {
                return Err(ChoreographyError::InternalError {
                    message: format!(
                        "missing choreography inbox notifier for active session {session_id}"
                    ),
                });
            };

            let wait_ms = timeout_ms.saturating_sub(elapsed_ms);
            tokio::select! {
                _ = session_inbox_notify.notified() => {}
                _ = async {
                    if let Some(shared_inbox_notify) = shared_inbox_notify.clone() {
                        shared_inbox_notify.notified().await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {}
                _ = self.time_handler.sleep_ms(wait_ms) => {
                    let mut state = self.choreography_state.write();
                    let _ = state.with_current_session_mut(|session| {
                        session.metrics.timeout_count = session.metrics.timeout_count.saturating_add(1);
                    });
                    return Err(ChoreographyError::Transport {
                        source: Box::new(aura_core::effects::TransportError::NoMessage),
                    });
                }
            }

            if !self.choreography_state.read().is_active() {
                return Err(ChoreographyError::SessionNotStarted);
            }
            if self
                .choreography_state
                .read()
                .current_session_id()
                .is_some_and(|active| active != session_id)
            {
                return Err(ChoreographyError::InternalError {
                    message: format!(
                        "choreography session binding changed while waiting for receive: {session_id}"
                    ),
                });
            }
        };

        {
            let mut state = self.choreography_state.write();
            state
                .with_current_session_mut(|session| {
                    session.metrics.messages_received =
                        session.metrics.messages_received.saturating_add(1);
                })
                .map_err(|message| ChoreographyError::InternalError { message })?;
        }

        Ok(envelope.payload)
    }

    async fn broadcast_bytes(&self, message: Vec<u8>) -> Result<(), ChoreographyError> {
        let session = current_session_snapshot(self)?;
        let roles = session.roles.clone();
        let current_role = session.current_role;

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
        current_session_snapshot(self).map_or_else(
            |_| {
                let role_index = RoleIndex::new(0).expect("role index");
                ChoreographicRole::new(DeviceId::from_uuid(self.authority_id.0), role_index)
            },
            |session| session.current_role,
        )
    }

    fn all_roles(&self) -> Vec<ChoreographicRole> {
        current_session_snapshot(self).map_or_else(
            |_| vec![self.current_role()],
            |session| {
                if session.roles.is_empty() {
                    vec![self.current_role()]
                } else {
                    session.roles
                }
            },
        )
    }

    async fn is_role_active(&self, role: ChoreographicRole) -> bool {
        let context_id = match current_session_snapshot(self) {
            Ok(session) => session.context_id,
            Err(_) => return false,
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
        let runtime_session_id = RuntimeChoreographySessionId::from_uuid(session_id);
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

        // Each runtime choreography session gets its own derived relational context so
        // guard, leakage, and journal coupling stay isolated under concurrent execution.
        let context_id = ContextId::new_from_entropy(hash(session_id.as_bytes()));
        tracing::debug!(
            "Choreography start_session: session_id={}, context_id={:?}, authority={:?}, roles={:?}",
            runtime_session_id,
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
        state
            .start_session(
                runtime_session_id,
                context_id,
                roles,
                current_role,
                None,
                started_at_ms,
            )
            .map_err(|message| {
                if message.contains("already exists") {
                    ChoreographyError::SessionAlreadyExists { session_id }
                } else {
                    ChoreographyError::InternalError { message }
                }
            })
    }

    async fn end_session(&self) -> Result<(), ChoreographyError> {
        let ended_at_ms = self
            .physical_time()
            .await
            .map(|time| time.ts_ms)
            .unwrap_or_default();

        let mut state = self.choreography_state.write();
        let ended_session_id = state
            .end_session(ended_at_ms)
            .map_err(|_| ChoreographyError::SessionNotStarted)?;
        drop(state);
        let _released_fragments = self.release_vm_fragments_for_session(ended_session_id);
        Ok(())
    }

    async fn emit_choreo_event(&self, event: ChoreographyEvent) -> Result<(), ChoreographyError> {
        tracing::debug!(?event, "choreography event");
        Ok(())
    }

    async fn set_timeout(&self, timeout_ms: u64) {
        let mut state = self.choreography_state.write();
        let _ = state.with_current_session_mut(|session| {
            session.timeout_ms = Some(timeout_ms);
        });
    }

    async fn get_metrics(&self) -> ChoreographyMetrics {
        current_session_snapshot(self).map_or_else(|_| default_metrics(), |session| session.metrics)
    }
}

fn default_metrics() -> ChoreographyMetrics {
    ChoreographyMetrics {
        messages_sent: 0,
        messages_received: 0,
        avg_latency_ms: 0.0,
        timeout_count: 0,
        retry_count: 0,
        total_duration_ms: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::DeviceId;
    use std::sync::Arc;
    use tokio::sync::Barrier;
    use uuid::Uuid;

    fn test_effects(authority_id: AuthorityId) -> Arc<AuraEffectSystem> {
        let authority_bytes = authority_id.to_bytes();
        let seed_salt = u64::from_le_bytes(authority_bytes[..8].try_into().expect("salt bytes"));
        Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority_with_salt(
                &AgentConfig::default(),
                authority_id,
                seed_salt,
            )
            .expect("testing effect system"),
        )
    }

    fn authority_device_role(authority_id: AuthorityId, role_index: u16) -> ChoreographicRole {
        ChoreographicRole::new(
            DeviceId::from_uuid(Uuid::from_bytes(authority_id.to_bytes())),
            RoleIndex::new(role_index.into()).expect("role index"),
        )
    }

    #[tokio::test]
    async fn concurrent_sessions_are_isolated_per_task() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([7; 16]));
        let effects = test_effects(authority_id);
        let barrier = Arc::new(Barrier::new(3));

        let session_a = Uuid::from_u128(1);
        let session_b = Uuid::from_u128(2);
        let peer_a = ChoreographicRole::new(
            DeviceId::from_uuid(Uuid::from_u128(11)),
            RoleIndex::new(1).expect("role index"),
        );
        let peer_b = ChoreographicRole::new(
            DeviceId::from_uuid(Uuid::from_u128(12)),
            RoleIndex::new(1).expect("role index"),
        );

        let task_a_effects = Arc::clone(&effects);
        let task_a_barrier = Arc::clone(&barrier);
        let task_a = tokio::spawn(async move {
            task_a_effects
                .start_session(
                    session_a,
                    vec![authority_device_role(authority_id, 0), peer_a],
                )
                .await
                .expect("session a starts");
            task_a_barrier.wait().await;
            assert_eq!(
                task_a_effects.current_role(),
                authority_device_role(authority_id, 0)
            );
            assert_eq!(task_a_effects.all_roles().len(), 2);
            task_a_effects.set_timeout(111).await;
            assert_eq!(task_a_effects.get_metrics().await.messages_sent, 0);
            task_a_effects.end_session().await.expect("session a ends");
        });

        let task_b_effects = Arc::clone(&effects);
        let task_b_barrier = Arc::clone(&barrier);
        let task_b = tokio::spawn(async move {
            task_b_effects
                .start_session(
                    session_b,
                    vec![authority_device_role(authority_id, 0), peer_b],
                )
                .await
                .expect("session b starts");
            task_b_barrier.wait().await;
            assert_eq!(
                task_b_effects.current_role(),
                authority_device_role(authority_id, 0)
            );
            assert_eq!(task_b_effects.all_roles().len(), 2);
            task_b_effects.set_timeout(222).await;
            assert_eq!(task_b_effects.get_metrics().await.messages_received, 0);
            task_b_effects.end_session().await.expect("session b ends");
        });

        barrier.wait().await;
        task_a.await.expect("task a");
        task_b.await.expect("task b");
        assert_eq!(effects.choreography_state.read().active_session_count(), 0);
    }

    #[tokio::test]
    async fn concurrent_session_sends_keep_guard_and_transport_contexts_isolated() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([13; 16]));
        let effects = test_effects(authority_id);
        let barrier = Arc::new(Barrier::new(3));

        let session_a = Uuid::from_u128(41);
        let session_b = Uuid::from_u128(42);
        let self_role = authority_device_role(authority_id, 0);
        let loopback_peer = authority_device_role(authority_id, 1);

        let task_a_effects = Arc::clone(&effects);
        let task_a_barrier = Arc::clone(&barrier);
        let task_a = tokio::spawn(async move {
            task_a_effects
                .start_session(session_a, vec![self_role, loopback_peer])
                .await
                .expect("session a starts");
            task_a_barrier.wait().await;
            task_a_effects
                .send_to_role_bytes(loopback_peer, b"alpha".to_vec())
                .await
                .expect("session a send succeeds");
            let envelopes = task_a_effects
                .choreography_state
                .read()
                .session_inbox_snapshot(RuntimeChoreographySessionId::from_uuid(session_a));
            task_a_effects.end_session().await.expect("session a ends");
            envelopes
        });

        let task_b_effects = Arc::clone(&effects);
        let task_b_barrier = Arc::clone(&barrier);
        let task_b = tokio::spawn(async move {
            task_b_effects
                .start_session(session_b, vec![self_role, loopback_peer])
                .await
                .expect("session b starts");
            task_b_barrier.wait().await;
            task_b_effects
                .send_to_role_bytes(loopback_peer, b"beta".to_vec())
                .await
                .expect("session b send succeeds");
            let envelopes = task_b_effects
                .choreography_state
                .read()
                .session_inbox_snapshot(RuntimeChoreographySessionId::from_uuid(session_b));
            task_b_effects.end_session().await.expect("session b ends");
            envelopes
        });

        barrier.wait().await;
        let session_a_envelopes = task_a.await.expect("task a");
        let session_b_envelopes = task_b.await.expect("task b");
        assert_eq!(
            session_a_envelopes.len(),
            1,
            "session a should queue one local send"
        );
        assert_eq!(
            session_b_envelopes.len(),
            1,
            "session b should queue one local send"
        );

        let expected = [
            (
                session_a.to_string(),
                ContextId::new_from_entropy(hash(session_a.as_bytes())),
                session_a_envelopes,
            ),
            (
                session_b.to_string(),
                ContextId::new_from_entropy(hash(session_b.as_bytes())),
                session_b_envelopes,
            ),
        ];

        for (session_id, context_id, envelopes) in expected {
            let envelope = envelopes
                .iter()
                .find(|env| {
                    env.metadata
                        .get("session-id")
                        .is_some_and(|value| value == &session_id)
                })
                .expect("session envelope should be present");
            assert_eq!(envelope.context, context_id);
            assert_eq!(
                envelope.receipt.as_ref().map(|receipt| receipt.context),
                Some(context_id),
                "guard/journal receipt context must remain session-scoped"
            );
        }
    }

    #[tokio::test]
    async fn receive_filters_by_session_id_metadata() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([9; 16]));
        let peer_authority = AuthorityId::from_uuid(Uuid::from_bytes([10; 16]));
        let effects = test_effects(authority_id);
        let session_id = Uuid::from_u128(33);
        let wrong_session_id = Uuid::from_u128(34);
        let self_role = authority_device_role(authority_id, 0);
        let peer_role = authority_device_role(peer_authority, 1);

        effects
            .start_session(session_id, vec![self_role, peer_role])
            .await
            .expect("session starts");

        let context_id = ContextId::new_from_entropy(hash(session_id.as_bytes()));
        for (sid, payload) in [
            (wrong_session_id, b"wrong".to_vec()),
            (session_id, b"correct".to_vec()),
        ] {
            let mut metadata = HashMap::new();
            metadata.insert(
                "content-type".to_string(),
                "application/aura-choreography".to_string(),
            );
            metadata.insert("session-id".to_string(), sid.to_string());
            effects.requeue_envelope(TransportEnvelope {
                destination: authority_id,
                source: peer_authority,
                context: context_id,
                payload,
                metadata,
                receipt: None,
            });
        }
        {
            let state = effects.choreography_state.read();
            assert_eq!(
                state.session_inbox_len(RuntimeChoreographySessionId::from_uuid(wrong_session_id)),
                1
            );
            assert_eq!(
                state.session_inbox_len(RuntimeChoreographySessionId::from_uuid(session_id)),
                1
            );
        }

        assert_eq!(
            AuthorityId::from_uuid(peer_role.device_id.0),
            peer_authority
        );
        let payload = take_session_envelope(
            effects.as_ref(),
            RuntimeChoreographySessionId::from_uuid(session_id),
            peer_authority,
            context_id,
        )
        .expect("session-scoped envelope should be available")
        .payload;
        assert_eq!(payload, b"correct".to_vec());
        {
            let state = effects.choreography_state.read();
            assert_eq!(
                state.session_inbox_len(RuntimeChoreographySessionId::from_uuid(wrong_session_id)),
                1
            );
            assert_eq!(
                state.session_inbox_len(RuntimeChoreographySessionId::from_uuid(session_id)),
                0
            );
        }

        effects.end_session().await.expect("session ends");
    }

    #[tokio::test]
    async fn receive_waits_on_session_local_notify() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([11; 16]));
        let peer_authority = AuthorityId::from_uuid(Uuid::from_bytes([12; 16]));
        let effects = test_effects(authority_id);
        let session_id = Uuid::from_u128(35);
        let self_role = authority_device_role(authority_id, 0);
        let peer_role = authority_device_role(peer_authority, 1);

        effects
            .start_session(session_id, vec![self_role, peer_role])
            .await
            .expect("session starts");

        let context_id = ContextId::new_from_entropy(hash(session_id.as_bytes()));
        let delayed_effects = Arc::clone(&effects);
        tokio::spawn(async move {
            delayed_effects.time_handler.sleep_ms(10).await;
            let mut metadata = HashMap::new();
            metadata.insert(
                "content-type".to_string(),
                "application/aura-choreography".to_string(),
            );
            metadata.insert("session-id".to_string(), session_id.to_string());
            delayed_effects.requeue_envelope(TransportEnvelope {
                destination: authority_id,
                source: peer_authority,
                context: context_id,
                payload: b"notified".to_vec(),
                metadata,
                receipt: None,
            });
        });

        let payload = tokio::time::timeout(
            std::time::Duration::from_millis(40),
            effects.receive_from_role_bytes(peer_role),
        )
        .await
        .expect("session-local notify should wake receive before polling-sized timeout")
        .expect("session-scoped receive succeeds");
        assert_eq!(payload, b"notified".to_vec());

        effects.end_session().await.expect("session ends");
    }

    #[tokio::test]
    async fn concurrent_inbound_delivery_remains_isolated_per_active_fragment() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([19; 16]));
        let effects = test_effects(authority_id);
        let barrier = Arc::new(Barrier::new(3));

        let session_a = Uuid::from_u128(38);
        let session_b = Uuid::from_u128(39);
        let peer_a_authority = AuthorityId::from_uuid(Uuid::from_bytes([20; 16]));
        let peer_b_authority = AuthorityId::from_uuid(Uuid::from_bytes([21; 16]));
        let self_role = authority_device_role(authority_id, 0);
        let peer_a_role = authority_device_role(peer_a_authority, 1);
        let peer_b_role = authority_device_role(peer_b_authority, 1);

        let task_a_effects = Arc::clone(&effects);
        let task_a_barrier = Arc::clone(&barrier);
        let task_a = tokio::spawn(async move {
            task_a_effects
                .start_session(session_a, vec![self_role, peer_a_role])
                .await
                .expect("session a starts");
            task_a_barrier.wait().await;
            let payload = task_a_effects
                .receive_from_role_bytes(peer_a_role)
                .await
                .expect("session a receive succeeds");
            task_a_effects.end_session().await.expect("session a ends");
            payload
        });

        let task_b_effects = Arc::clone(&effects);
        let task_b_barrier = Arc::clone(&barrier);
        let task_b = tokio::spawn(async move {
            task_b_effects
                .start_session(session_b, vec![self_role, peer_b_role])
                .await
                .expect("session b starts");
            task_b_barrier.wait().await;
            let payload = task_b_effects
                .receive_from_role_bytes(peer_b_role)
                .await
                .expect("session b receive succeeds");
            task_b_effects.end_session().await.expect("session b ends");
            payload
        });

        barrier.wait().await;

        for (session_id, source, payload) in [
            (session_a, peer_a_authority, b"alpha".to_vec()),
            (session_b, peer_b_authority, b"beta".to_vec()),
        ] {
            let mut metadata = HashMap::new();
            metadata.insert(
                "content-type".to_string(),
                "application/aura-choreography".to_string(),
            );
            metadata.insert("session-id".to_string(), session_id.to_string());
            effects.requeue_envelope(TransportEnvelope {
                destination: authority_id,
                source,
                context: ContextId::new_from_entropy(hash(session_id.as_bytes())),
                payload,
                metadata,
                receipt: None,
            });
        }

        assert_eq!(task_a.await.expect("task a"), b"alpha".to_vec());
        assert_eq!(task_b.await.expect("task b"), b"beta".to_vec());
        assert_eq!(effects.choreography_state.read().active_session_count(), 0);
    }

    #[tokio::test]
    async fn receive_reports_timeout_without_polling_loop() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([17; 16]));
        let peer_authority = AuthorityId::from_uuid(Uuid::from_bytes([18; 16]));
        let effects = test_effects(authority_id);
        let session_id = Uuid::from_u128(36);
        let self_role = authority_device_role(authority_id, 0);
        let peer_role = authority_device_role(peer_authority, 1);

        effects
            .start_session(session_id, vec![self_role, peer_role])
            .await
            .expect("session starts");
        effects.set_timeout(20).await;

        let error = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            effects.receive_from_role_bytes(peer_role),
        )
        .await
        .expect("receive should resolve with a timeout error")
        .expect_err("receive should time out");
        assert!(matches!(
            error,
            ChoreographyError::Transport { source }
                if source
                    .downcast_ref::<aura_core::effects::TransportError>()
                    .is_some_and(|inner| matches!(inner, aura_core::effects::TransportError::NoMessage))
        ));
        assert_eq!(effects.get_metrics().await.timeout_count, 1);

        effects.end_session().await.expect("session ends");
    }

    #[tokio::test]
    async fn receive_returns_session_not_started_when_session_is_cancelled() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([15; 16]));
        let peer_authority = AuthorityId::from_uuid(Uuid::from_bytes([16; 16]));
        let effects = test_effects(authority_id);
        let session_id = Uuid::from_u128(37);
        let runtime_session_id = RuntimeChoreographySessionId::from_uuid(session_id);
        let self_role = authority_device_role(authority_id, 0);
        let peer_role = authority_device_role(peer_authority, 1);

        effects
            .start_session(session_id, vec![self_role, peer_role])
            .await
            .expect("session starts");

        let delayed_effects = Arc::clone(&effects);
        tokio::spawn(async move {
            delayed_effects.time_handler.sleep_ms(10).await;
            delayed_effects
                .choreography_state
                .write()
                .cancel_session(runtime_session_id);
        });

        let error = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            effects.receive_from_role_bytes(peer_role),
        )
        .await
        .expect("receive should resolve when session is cancelled")
        .expect_err("receive should fail when session is cancelled");
        assert!(matches!(error, ChoreographyError::SessionNotStarted));
    }
}
