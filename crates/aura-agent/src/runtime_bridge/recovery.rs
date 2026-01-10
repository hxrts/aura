use super::AgentRuntimeBridge;
use aura_app::IntentError;
use aura_core::identifiers::CeremonyId;
use aura_core::threshold::ParticipantIdentity;
use aura_core::Hash32;
use aura_recovery::guardian_ceremony::CeremonyResponse;

pub(super) async fn respond_to_guardian_ceremony(
    bridge: &AgentRuntimeBridge,
    ceremony_id: &str,
    accept: bool,
    _reason: Option<String>,
) -> Result<(), IntentError> {
    // Verify the ceremony exists and get tracker
    let runner = bridge.agent.ceremony_runner().await;
    let ceremony_id = CeremonyId::new(ceremony_id.to_string());
    let tracker = bridge.agent.ceremony_tracker().await;
    let ceremony_state = tracker
        .get(&ceremony_id)
        .await
        .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;
    let _status = runner
        .status(&ceremony_id)
        .await
        .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;

    if accept {
        // Record acceptance in ceremony tracker
        runner
            .record_response(
                &ceremony_id,
                ParticipantIdentity::guardian(bridge.agent.authority_id()),
            )
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to record guardian acceptance: {}", e))
            })?;
    } else {
        // Mark ceremony as failed due to decline
        runner
            .abort(
                &ceremony_id,
                Some("Guardian declined invitation".to_string()),
            )
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to record guardian decline: {}", e))
            })?;
    }

    let protocol_ceremony_id = {
        let hex_str = ceremony_state.ceremony_id.as_str();
        let decoded = hex::decode(hex_str).map_err(|e| {
            IntentError::validation_failed(format!("Invalid ceremony id format: {e}"))
        })?;
        if decoded.len() != 32 {
            return Err(IntentError::validation_failed(format!(
                "Invalid ceremony id length: {}",
                decoded.len()
            )));
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&decoded[..32]);
        aura_recovery::CeremonyId(Hash32(bytes))
    };

    // Determine role index by sorting guardians and finding our position.
    // The initiator assigns Guardian1 to guardians[0] and Guardian2 to guardians[1],
    // so we must use the same deterministic ordering on both sides.
    let my_authority_id = bridge.agent.authority_id();
    let mut guardian_ids: Vec<_> = ceremony_state
        .participants
        .iter()
        .filter_map(|p| {
            if let aura_core::threshold::ParticipantIdentity::Guardian(id) = p {
                Some(*id)
            } else {
                None
            }
        })
        .collect();
    guardian_ids.sort();
    let role_index = guardian_ids
        .iter()
        .position(|id| *id == my_authority_id)
        .ok_or_else(|| {
            IntentError::validation_failed(format!(
                "Current authority {} not found in ceremony guardians",
                my_authority_id
            ))
        })?;

    let recovery_service = bridge
        .agent
        .recovery()
        .map_err(|e| IntentError::internal_error(format!("recovery_service unavailable: {e}")))?;
    let response = if accept {
        CeremonyResponse::Accept
    } else {
        CeremonyResponse::Decline
    };
    recovery_service
        .execute_guardian_ceremony_guardian(
            ceremony_state.initiator_id,
            protocol_ceremony_id,
            response,
            role_index,
        )
        .await
        .map_err(|e| {
            IntentError::internal_error(format!("Guardian ceremony choreography failed: {e}"))
        })?;

    Ok(())
}
