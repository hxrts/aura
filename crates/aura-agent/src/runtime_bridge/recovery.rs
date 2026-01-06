use super::AgentRuntimeBridge;
use aura_app::IntentError;
use aura_core::identifiers::CeremonyId;
use aura_core::threshold::ParticipantIdentity;

pub(super) async fn respond_to_guardian_ceremony(
    bridge: &AgentRuntimeBridge,
    ceremony_id: &str,
    accept: bool,
    _reason: Option<String>,
) -> Result<(), IntentError> {
    // Verify the ceremony exists and get tracker
    let runner = bridge.agent.ceremony_runner().await;
    let ceremony_id = CeremonyId::new(ceremony_id.to_string());
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
        Ok(())
    } else {
        // Mark ceremony as failed due to decline
        runner
            .abort(&ceremony_id, Some("Guardian declined invitation".to_string()))
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to record guardian decline: {}", e))
            })?;
        Ok(())
    }
}
