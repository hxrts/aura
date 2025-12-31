use super::AgentRuntimeBridge;
use aura_app::IntentError;

pub(super) async fn respond_to_guardian_ceremony(
    bridge: &AgentRuntimeBridge,
    ceremony_id: &str,
    accept: bool,
    _reason: Option<String>,
) -> Result<(), IntentError> {
    // Verify the ceremony exists and get tracker
    let tracker = bridge.agent.ceremony_tracker().await;
    let _state = tracker
        .get(ceremony_id)
        .await
        .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;

    if accept {
        // Record acceptance in ceremony tracker
        tracker
            .mark_accepted(
                ceremony_id,
                aura_core::threshold::ParticipantIdentity::guardian(bridge.agent.authority_id()),
            )
            .await
            .map_err(|e| {
                IntentError::internal_error(format!(
                    "Failed to record guardian acceptance: {}",
                    e
                ))
            })?;
        Ok(())
    } else {
        // Mark ceremony as failed due to decline
        tracker
            .mark_failed(
                ceremony_id,
                Some("Guardian declined invitation".to_string()),
            )
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to record guardian decline: {}", e))
            })?;
        Ok(())
    }
}
