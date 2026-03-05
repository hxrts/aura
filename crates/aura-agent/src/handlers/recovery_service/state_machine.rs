use super::*;

pub(super) async fn execute_recovery_protocol_account(
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    guardian_id: AuthorityId,
    request: ProtocolRecoveryRequest,
) -> AgentResult<()> {
    use crate::core::AgentError;

    let mut role_map = HashMap::new();
    role_map.insert(RecoveryProtocolRole::Account, authority_id);
    role_map.insert(RecoveryProtocolRole::Coordinator, authority_id);
    role_map.insert(RecoveryProtocolRole::Guardian, guardian_id);

    let request_type = std::any::type_name::<ProtocolRecoveryRequest>();

    let session_id = recovery_session_id(&request.recovery_id, &guardian_id);
    let request_clone = request.clone();
    let mut adapter = AuraProtocolAdapter::new(
        effects.clone(),
        authority_id,
        RecoveryProtocolRole::Account,
        role_map,
    )
    .with_message_provider(move |request_ctx, _received| {
        if request_ctx.type_name == request_type {
            return Some(Box::new(request_clone.clone()));
        }
        None
    });
    adapter
        .start_session(session_id)
        .await
        .map_err(|e| AgentError::internal(format!("recovery account start failed: {e}")))?;

    let result = recovery_execute_as(RecoveryProtocolRole::Account, &mut adapter)
        .await
        .map_err(|e| AgentError::internal(format!("recovery account failed: {e}")));

    let _ = adapter.end_session().await;
    result
}

pub(super) async fn execute_recovery_protocol_coordinator(
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    guardian_id: AuthorityId,
    request: ProtocolRecoveryRequest,
) -> AgentResult<()> {
    use crate::core::AgentError;

    let mut role_map = HashMap::new();
    role_map.insert(RecoveryProtocolRole::Account, authority_id);
    role_map.insert(RecoveryProtocolRole::Coordinator, authority_id);
    role_map.insert(RecoveryProtocolRole::Guardian, guardian_id);

    let request_type = std::any::type_name::<ProtocolRecoveryRequest>();
    let approval_type = std::any::type_name::<ProtocolGuardianApproval>();
    let outcome_type = std::any::type_name::<RecoveryOutcome>();

    let session_id = recovery_session_id(&request.recovery_id, &guardian_id);
    let request_clone = request.clone();
    let mut adapter = AuraProtocolAdapter::new(
        effects.clone(),
        authority_id,
        RecoveryProtocolRole::Coordinator,
        role_map,
    )
    .with_message_provider(move |request_ctx, received| {
        if request_ctx.type_name == request_type {
            return Some(Box::new(request_clone.clone()));
        }

        if request_ctx.type_name == outcome_type {
            let mut approvals = Vec::new();
            for msg in received {
                if msg.type_name == approval_type {
                    if let Ok(approval) = from_slice::<ProtocolGuardianApproval>(&msg.bytes) {
                        approvals.push(approval);
                    }
                }
            }
            let success = !approvals.is_empty();
            let outcome = RecoveryOutcome {
                success,
                recovery_grant: None,
                error: if success {
                    None
                } else {
                    Some("no approvals".to_string())
                },
                approvals,
            };
            return Some(Box::new(outcome));
        }

        None
    });

    adapter
        .start_session(session_id)
        .await
        .map_err(|e| AgentError::internal(format!("recovery coordinator start failed: {e}")))?;

    let result = recovery_execute_as(RecoveryProtocolRole::Coordinator, &mut adapter)
        .await
        .map_err(|e| AgentError::internal(format!("recovery coordinator failed: {e}")));

    let _ = adapter.end_session().await;
    result
}

pub(super) fn recovery_session_id(recovery_id: &RecoveryId, guardian_id: &AuthorityId) -> Uuid {
    let mut material = Vec::new();
    material.extend_from_slice(recovery_id.as_str().as_bytes());
    material.extend_from_slice(&guardian_id.to_bytes());
    let digest = hash(&material);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

pub(super) fn guardian_setup_session_id(setup_id: &str) -> Uuid {
    let digest = hash(setup_id.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

pub(super) fn membership_session_id(change_id: &str) -> Uuid {
    let digest = hash(change_id.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}
