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
                IntentError::internal_error(format!(
                    "Failed to record guardian acceptance: {}",
                    e
                ))
            })?;
    } else {
        // Mark ceremony as failed due to decline
        runner
            .abort(&ceremony_id, Some("Guardian declined invitation".to_string()))
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
        )
        .await
        .map_err(|e| {
            IntentError::internal_error(format!(
                "Guardian ceremony choreography failed: {e}"
            ))
        })?;

    Ok(())
}
