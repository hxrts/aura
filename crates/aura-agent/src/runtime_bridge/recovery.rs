use super::error_boundary::{bridge_internal, bridge_validation, bridge_validation_message};
use super::AgentRuntimeBridge;
use crate::handlers::recovery_service::GuardianCeremonyGuardianOutcome;
use crate::runtime::services::ceremony_runner::CeremonyCommitMetadata;
use aura_app::IntentError;
use aura_core::threshold::ParticipantIdentity;
use aura_core::types::identifiers::CeremonyId;
use aura_core::Hash32;
use aura_recovery::guardian_ceremony::CeremonyResponse;

pub(super) async fn respond_to_guardian_ceremony(
    bridge: &AgentRuntimeBridge,
    ceremony_id: &CeremonyId,
    accept: bool,
    _reason: Option<String>,
) -> Result<(), IntentError> {
    // Verify the ceremony exists and get tracker
    let runner = bridge.agent.ceremony_runner().await;
    let tracker = bridge.agent.ceremony_tracker().await;
    let ceremony_state = tracker
        .get(ceremony_id)
        .await
        .map_err(|e| bridge_validation("Ceremony not found", e))?;
    let _status = runner
        .status(ceremony_id)
        .await
        .map_err(|e| bridge_validation("Ceremony not found", e))?;

    if accept {
        // Record acceptance in ceremony tracker
        runner
            .record_local_response(
                ceremony_id,
                ParticipantIdentity::guardian(bridge.agent.authority_id()),
            )
            .await
            .map_err(|e| bridge_internal("Record guardian acceptance failed", e))?;
    } else {
        // Mark ceremony as failed due to decline
        runner
            .abort(
                ceremony_id,
                Some("Guardian declined invitation".to_string()),
            )
            .await
            .map_err(|e| bridge_internal("Record guardian decline failed", e))?;
    }

    let protocol_ceremony_id = {
        let hex_str = ceremony_state.ceremony_id.as_str();
        let decoded =
            hex::decode(hex_str).map_err(|e| bridge_validation("Invalid ceremony id format", e))?;
        if decoded.len() != 32 {
            return Err(bridge_validation_message(format!(
                "Invalid ceremony id length: {}",
                decoded.len()
            )));
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&decoded[..32]);
        aura_recovery::CeremonyId(Hash32(bytes))
    };

    // The initiator assigns Guardian1/Guardian2 by sorted guardian id; the
    // service derives our role from the full guardian set the same way.
    let my_authority_id = bridge.agent.authority_id();
    let guardian_ids: Vec<_> = ceremony_state
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
    if !guardian_ids.contains(&my_authority_id) {
        return Err(bridge_validation_message(format!(
            "Current authority {} not found in ceremony guardians",
            my_authority_id
        )));
    }

    let recovery_service = bridge
        .agent
        .recovery()
        .map_err(|e| bridge_internal("Recovery service unavailable", e))?;
    let response = if accept {
        CeremonyResponse::Accept
    } else {
        CeremonyResponse::Decline
    };

    // The guardian session finishes only once the initiator commits or aborts,
    // which also waits on the other guardian. Run it as an owned background
    // task so the UI operation returns once the response is underway.
    let initiator_id = ceremony_state.initiator_id;
    let tracker_ceremony_id = ceremony_id.clone();
    let task_name = format!("guardian_ceremony_guardian.{ceremony_id}");
    let fut = async move {
        let result = recovery_service
            .execute_guardian_ceremony_guardian(
                initiator_id,
                protocol_ceremony_id,
                response,
                &guardian_ids,
            )
            .await;
        match result {
            Ok(GuardianCeremonyGuardianOutcome::Committed { .. }) => {
                if accept {
                    if let Err(error) = runner
                        .commit(&tracker_ceremony_id, CeremonyCommitMetadata::default())
                        .await
                    {
                        tracing::warn!(
                            ceremony_id = %tracker_ceremony_id,
                            error = %error,
                            "failed to mark guardian ceremony committed"
                        );
                    }
                }
            }
            Ok(GuardianCeremonyGuardianOutcome::Aborted { reason }) => {
                let _ = runner.abort(&tracker_ceremony_id, Some(reason)).await;
            }
            Err(error) => {
                tracing::warn!(
                    ceremony_id = %tracker_ceremony_id,
                    error = %error,
                    "guardian ceremony choreography failed"
                );
                let _ = runner
                    .abort(&tracker_ceremony_id, Some(error.to_string()))
                    .await;
            }
        }
    };
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            let _task_handle = bridge.agent.runtime().tasks().spawn_local_named(task_name, fut);
        } else {
            let _task_handle = bridge.agent.runtime().tasks().spawn_named(task_name, fut);
        }
    }

    Ok(())
}
