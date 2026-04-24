use super::{AuraEffectSystem, CHOREO_FLOW_COST_PER_KB, DEFAULT_CHOREO_FLOW_COST};
use async_trait::async_trait;
use aura_chat::capabilities::ChatCapability;
use aura_core::effects::transport::{TransportEnvelope, TransportReceipt};
use aura_core::effects::{PhysicalTimeEffects, TransportEffects, WakeCondition};
use aura_core::hash::hash;
use aura_core::{AuthorityId, ContextId, FlowCost};
use aura_guards::prelude::create_send_guard_op;
use aura_guards::{GuardOperation, GuardOperationId, JournalCoupler};
use aura_protocol::effects::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError, ChoreographyEvent,
    ChoreographyMetrics, RoleIndex,
};
use std::collections::HashMap;

use crate::runtime::subsystems::choreography::RuntimeChoreographySessionId;
use crate::runtime::subsystems::choreography::SessionStartError;
use crate::runtime::transport_boundary::send_guarded_transport_envelope;

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
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ChoreographicEffects for AuraEffectSystem {
    async fn send_to_role_bytes(
        &self,
        role: ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), ChoreographyError> {
        let session = current_session_snapshot(self)?;
        let context_id = session.context_id;
        let current_role = session.current_role;

        let peer = role.authority_id;
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
            GuardOperation::Custom(ChatCapability::MessageSend.as_name().to_string()),
            context_id,
            peer,
            FlowCost::new(flow_cost),
        )
        .with_operation_id(
            GuardOperationId::custom(format!(
                "choreography_send_{}_{}_{:?}",
                session.session_id, context_id, role.device_id
            ))
            .map_err(|error| ChoreographyError::InternalError {
                message: error.to_string(),
            })?,
        );

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
        metadata.insert(
            "aura-source-device-id".to_string(),
            current_role.device_id.to_string(),
        );
        metadata.insert(
            "aura-destination-device-id".to_string(),
            role.device_id.to_string(),
        );
        if let Some(protocol_id) = session.protocol_id.as_ref() {
            metadata.insert("protocol-id".to_string(), protocol_id.clone());
        }

        let envelope = TransportEnvelope {
            destination: peer,
            source: current_role.authority_id,
            context: context_id,
            payload: message,
            metadata,
            receipt: transport_receipt,
        };

        send_guarded_transport_envelope(self, envelope)
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
        let timeout_handle = self
            .time_handler
            .set_timeout(timeout_ms)
            .await
            .map_err(|error| ChoreographyError::InternalError {
                message: format!("failed to issue receive timeout witness: {error}"),
            })?;

        let source_authority = role.authority_id;
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

            let Some(session_inbox_notify) = session_inbox_notify.clone() else {
                let _ = self.time_handler.cancel_timeout(timeout_handle).await;
                return Err(ChoreographyError::InternalError {
                    message: format!(
                        "missing choreography inbox notifier for active session {session_id}"
                    ),
                });
            };

            tokio::select! {
                _ = session_inbox_notify.notified() => {}
                _ = async {
                    if let Some(shared_inbox_notify) = shared_inbox_notify.clone() {
                        shared_inbox_notify.notified().await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {}
                timeout_result = self.time_handler.yield_until(WakeCondition::TimeoutExpired {
                    timeout_id: timeout_handle,
                }) => {
                    if let Err(error) = timeout_result {
                        return Err(ChoreographyError::InternalError {
                            message: format!("receive timeout witness failed: {error}"),
                        });
                    }
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
                let _ = self.time_handler.cancel_timeout(timeout_handle).await;
                return Err(ChoreographyError::SessionNotStarted);
            }
            if self
                .choreography_state
                .read()
                .current_session_id()
                .is_some_and(|active| active != session_id)
            {
                let _ = self.time_handler.cancel_timeout(timeout_handle).await;
                return Err(ChoreographyError::InternalError {
                    message: format!(
                        "choreography session binding changed while waiting for receive: {session_id}"
                    ),
                });
            }
        };

        let _ = self.time_handler.cancel_timeout(timeout_handle).await;

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
            if role == current_role {
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
                ChoreographicRole::with_authority(
                    self.config.device_id(),
                    self.authority_id,
                    role_index,
                )
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

        TransportEffects::is_channel_established(self, context_id, role.authority_id).await
    }

    async fn start_session(
        &self,
        session_id: uuid::Uuid,
        roles: Vec<ChoreographicRole>,
    ) -> Result<(), ChoreographyError> {
        let runtime_session_id = RuntimeChoreographySessionId::from_uuid(session_id);
        let current_device = self.config.device_id();
        let current_role = roles
            .iter()
            .find(|role| role.device_id == current_device)
            .or_else(|| {
                roles
                    .iter()
                    .find(|role| role.authority_id == self.authority_id)
            })
            .copied()
            .ok_or_else(|| {
                let role_index = RoleIndex::new(0).expect("role index");
                ChoreographyError::RoleNotFound {
                    role: ChoreographicRole::with_authority(
                        current_device,
                        self.authority_id,
                        role_index,
                    ),
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
                None,
                context_id,
                roles,
                current_role,
                None,
                started_at_ms,
            )
            .map_err(|error| match error {
                SessionStartError::SessionAlreadyExists { .. } => {
                    ChoreographyError::SessionAlreadyExists { session_id }
                }
                SessionStartError::TaskAlreadyBound { .. } => ChoreographyError::InternalError {
                    message: error.to_string(),
                },
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
    use std::time::Duration;
    use tokio::sync::Barrier;
    use uuid::Uuid;

    async fn assert_settles_within<T, E: std::fmt::Debug>(
        future: impl std::future::Future<Output = Result<T, E>>,
        timeout: Duration,
        message: &str,
    ) -> Result<T, E> {
        let time = aura_effects::time::PhysicalTimeHandler::new();
        let started_at = time
            .physical_time()
            .await
            .expect("physical time should be available");
        let budget = aura_core::TimeoutBudget::from_start_and_timeout(&started_at, timeout)
            .expect("timeout budget should fit");
        match aura_core::execute_with_timeout_budget(&time, &budget, || future).await {
            Ok(value) => Ok(value),
            Err(aura_core::TimeoutRunError::Operation(error)) => Err(error),
            Err(aura_core::TimeoutRunError::Timeout(error)) => {
                panic!("{message}: timeout budget exceeded: {error:?}")
            }
        }
    }

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
        ChoreographicRole::for_authority(
            authority_id,
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
            AuthorityId::new_from_entropy([11u8; 32]),
            RoleIndex::new(1).expect("role index"),
        );
        let peer_b = ChoreographicRole::new(
            DeviceId::from_uuid(Uuid::from_u128(12)),
            AuthorityId::new_from_entropy([12u8; 32]),
            RoleIndex::new(1).expect("role index"),
        );

        let task_a_effects = Arc::clone(&effects);
        let task_a_barrier = Arc::clone(&barrier);
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
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
        tasks.spawn(async move {
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
        tasks
            .join_next()
            .await
            .expect("task a joined")
            .expect("task a");
        tasks
            .join_next()
            .await
            .expect("task b joined")
            .expect("task b");
        assert_eq!(effects.choreography_state.read().active_session_count(), 0);
    }

    #[tokio::test]
    async fn concurrent_session_sends_keep_guard_and_transport_contexts_isolated() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([13; 16]));
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &AgentConfig::default(),
                authority_id,
                crate::runtime::SharedTransport::new(),
            )
            .expect("testing effect system with shared transport"),
        );
        let barrier = Arc::new(Barrier::new(3));

        let session_a = Uuid::from_u128(41);
        let session_b = Uuid::from_u128(42);
        let self_role = authority_device_role(authority_id, 0);
        let loopback_peer = authority_device_role(authority_id, 1);

        let task_a_effects = Arc::clone(&effects);
        let task_a_barrier = Arc::clone(&barrier);
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            task_a_effects
                .start_session(session_a, vec![self_role, loopback_peer])
                .await
                .expect("session a starts");
            task_a_barrier.wait().await;
            task_a_effects
                .send_to_role_bytes(loopback_peer, b"alpha".to_vec())
                .await
                .expect("session a send succeeds");
            task_a_effects.end_session().await.expect("session a ends");
        });

        let task_b_effects = Arc::clone(&effects);
        let task_b_barrier = Arc::clone(&barrier);
        tasks.spawn(async move {
            task_b_effects
                .start_session(session_b, vec![self_role, loopback_peer])
                .await
                .expect("session b starts");
            task_b_barrier.wait().await;
            task_b_effects
                .send_to_role_bytes(loopback_peer, b"beta".to_vec())
                .await
                .expect("session b send succeeds");
            task_b_effects.end_session().await.expect("session b ends");
        });

        barrier.wait().await;
        tasks
            .join_next()
            .await
            .expect("first task joined")
            .expect("first task result");
        tasks
            .join_next()
            .await
            .expect("second task joined")
            .expect("second task result");
        let shared = effects
            .transport
            .shared_transport()
            .expect("shared transport should be attached for the test");
        let shared_inbox = shared.inbox_for(authority_id).read().clone();
        let session_a_envelopes = shared_inbox
            .iter()
            .filter(|env| {
                env.metadata
                    .get("session-id")
                    .is_some_and(|value| value == &session_a.to_string())
            })
            .cloned()
            .collect::<Vec<_>>();
        let session_b_envelopes = shared_inbox
            .iter()
            .filter(|env| {
                env.metadata
                    .get("session-id")
                    .is_some_and(|value| value == &session_b.to_string())
            })
            .cloned()
            .collect::<Vec<_>>();
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

        assert_eq!(peer_role.authority_id, peer_authority);
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
        let mut delayed_tasks = tokio::task::JoinSet::new();
        delayed_tasks.spawn(async move {
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

        let payload = assert_settles_within(
            effects.receive_from_role_bytes(peer_role),
            Duration::from_millis(40),
            "session-local notify should wake receive before polling-sized timeout",
        )
        .await;
        delayed_tasks
            .join_next()
            .await
            .expect("enqueue task joined")
            .expect("enqueue task");
        let payload = payload.expect("session-scoped receive succeeds");
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
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
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
        tasks.spawn(async move {
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

        let enqueue = async {
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
        };

        enqueue.await;
        let first = tasks
            .join_next()
            .await
            .expect("first receive task joined")
            .expect("first receive task");
        let second = tasks
            .join_next()
            .await
            .expect("second receive task joined")
            .expect("second receive task");
        assert!(matches!(
            (first.as_slice(), second.as_slice()),
            (b"alpha", b"beta") | (b"beta", b"alpha")
        ));
        assert_eq!(effects.choreography_state.read().active_session_count(), 0);
    }

    #[tokio::test]
    async fn session_sends_include_protocol_and_device_routing_metadata() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([0x71; 16]));
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport_for_authority(
                &AgentConfig::default(),
                authority_id,
                crate::runtime::SharedTransport::new(),
            )
            .expect("testing effect system with shared transport"),
        );
        let session_id = Uuid::from_u128(0x7172);
        let self_role = authority_device_role(authority_id, 0);
        let loopback_peer = authority_device_role(authority_id, 1);

        effects
            .start_session(session_id, vec![self_role, loopback_peer])
            .await
            .expect("session starts");
        effects
            .set_current_runtime_choreography_protocol_id("aura.test.protocol")
            .expect("protocol id attaches to current session");
        effects
            .send_to_role_bytes(loopback_peer, b"hello".to_vec())
            .await
            .expect("send succeeds");

        let shared = effects
            .transport
            .shared_transport()
            .expect("shared transport should be attached for the test");
        let envelope = shared
            .inbox_for(authority_id)
            .read()
            .first()
            .cloned()
            .expect("loopback send should queue one envelope");
        let session_id_string = session_id.to_string();
        let source_device_string = self_role.device_id.to_string();
        let destination_device_string = loopback_peer.device_id.to_string();

        assert_eq!(
            envelope.metadata.get("content-type").map(String::as_str),
            Some("application/aura-choreography")
        );
        assert_eq!(
            envelope.metadata.get("session-id").map(String::as_str),
            Some(session_id_string.as_str())
        );
        assert_eq!(
            envelope.metadata.get("protocol-id").map(String::as_str),
            Some("aura.test.protocol")
        );
        assert_eq!(
            envelope
                .metadata
                .get("aura-source-device-id")
                .map(String::as_str),
            Some(source_device_string.as_str())
        );
        assert_eq!(
            envelope
                .metadata
                .get("aura-destination-device-id")
                .map(String::as_str),
            Some(destination_device_string.as_str())
        );

        effects.end_session().await.expect("session ends");
    }

    #[tokio::test]
    async fn async_ingress_reordering_preserves_communication_identity() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([0x61; 16]));
        let peer_authority = AuthorityId::from_uuid(Uuid::from_bytes([0x62; 16]));
        let effects = test_effects(authority_id);
        let session_id = Uuid::from_u128(0x6162);
        let self_role = authority_device_role(authority_id, 0);
        let peer_role = authority_device_role(peer_authority, 1);

        effects
            .start_session(session_id, vec![self_role, peer_role])
            .await
            .expect("session starts");

        let context_id = ContextId::new_from_entropy(hash(session_id.as_bytes()));
        for (message_id, replay_key, payload) in [
            ("msg-2", "replay-2", b"second".to_vec()),
            ("msg-1", "replay-1", b"first".to_vec()),
        ] {
            let mut metadata = HashMap::new();
            metadata.insert(
                "content-type".to_string(),
                "application/aura-choreography".to_string(),
            );
            metadata.insert("session-id".to_string(), session_id.to_string());
            metadata.insert("message-id".to_string(), message_id.to_string());
            metadata.insert("replay-key".to_string(), replay_key.to_string());
            effects.requeue_envelope(TransportEnvelope {
                destination: authority_id,
                source: peer_authority,
                context: context_id,
                payload,
                metadata,
                receipt: None,
            });
        }

        let session_runtime_id = RuntimeChoreographySessionId::from_uuid(session_id);
        let snapshot = effects
            .choreography_state
            .read()
            .session_inbox_snapshot(session_runtime_id);
        let identities = snapshot
            .iter()
            .map(|envelope| {
                (
                    envelope
                        .metadata
                        .get("message-id")
                        .cloned()
                        .expect("message id preserved"),
                    envelope
                        .metadata
                        .get("replay-key")
                        .cloned()
                        .expect("replay key preserved"),
                    envelope.payload.clone(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            identities,
            vec![
                (
                    "msg-2".to_string(),
                    "replay-2".to_string(),
                    b"second".to_vec()
                ),
                (
                    "msg-1".to_string(),
                    "replay-1".to_string(),
                    b"first".to_vec()
                ),
            ],
            "host ingress reordering may change arrival order, but communication identity must survive unchanged"
        );

        for expected in [
            ("msg-2", "replay-2", b"second".to_vec()),
            ("msg-1", "replay-1", b"first".to_vec()),
        ] {
            let envelope = take_session_envelope(
                effects.as_ref(),
                session_runtime_id,
                peer_authority,
                context_id,
            )
            .expect("session envelope should be available");
            assert_eq!(
                envelope.metadata.get("message-id").map(String::as_str),
                Some(expected.0)
            );
            assert_eq!(
                envelope.metadata.get("replay-key").map(String::as_str),
                Some(expected.1)
            );
            assert_eq!(envelope.payload, expected.2);
        }

        effects.end_session().await.expect("session ends");
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

        let error = assert_settles_within(
            effects.receive_from_role_bytes(peer_role),
            Duration::from_millis(100),
            "receive should resolve with a timeout error",
        )
        .await
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
        let mut delayed_tasks = tokio::task::JoinSet::new();
        delayed_tasks.spawn(async move {
            delayed_effects.time_handler.sleep_ms(10).await;
            delayed_effects
                .choreography_state
                .write()
                .cancel_session(runtime_session_id);
        });

        let error = assert_settles_within(
            effects.receive_from_role_bytes(peer_role),
            Duration::from_millis(100),
            "receive should resolve when session is cancelled",
        )
        .await;
        delayed_tasks
            .join_next()
            .await
            .expect("cancel task joined")
            .expect("cancel task");
        let error = error.expect_err("receive should fail when session is cancelled");
        assert!(matches!(error, ChoreographyError::SessionNotStarted));
    }
}
