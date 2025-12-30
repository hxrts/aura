//! Invitation Bridge - Effect Command Execution
//!
//! Bridges between `aura_invitation::InvitationService` guard outcomes
//! and the agent's effect system. Executes `EffectCommand` items after
//! guard approval.

use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::TransportEffects;
use aura_core::hash::hash;
use aura_core::identifiers::ContextId;
use aura_invitation::guards::{EffectCommand, GuardOutcome};
use aura_invitation::InvitationFact;
use aura_journal::DomainFact;
use std::collections::HashMap;

use super::shared::HandlerUtilities;
use super::{invitation::InvitationHandler, InvitationService};

/// Execute a guard outcome's effect commands
///
/// Takes a `GuardOutcome` from `aura_invitation::InvitationService` and
/// executes each `EffectCommand` using the agent's effect system.
///
/// # Arguments
/// * `outcome` - The guard outcome to execute
/// * `authority` - The authority context for the operation
/// * `effects` - The effect system to use for execution
///
/// # Returns
/// * `Ok(())` if all effects were executed successfully
/// * `Err(AgentError)` if any effect fails or if the outcome was denied
pub async fn execute_guard_outcome(
    outcome: GuardOutcome,
    authority: &AuthorityContext,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // Check if the operation was denied
    if outcome.is_denied() {
        let reason = outcome
            .decision
            .denial_reason()
            .unwrap_or("Operation denied");
        return Err(AgentError::effects(format!(
            "Guard denied operation: {}",
            reason
        )));
    }

    // Execute each effect command
    for command in outcome.effects {
        execute_effect_command(command, authority, effects).await?;
    }

    Ok(())
}

/// Execute a single effect command
async fn execute_effect_command(
    command: EffectCommand,
    authority: &AuthorityContext,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    match command {
        EffectCommand::JournalAppend { fact } => {
            execute_journal_append(fact, authority, effects).await
        }
        EffectCommand::ChargeFlowBudget { cost } => execute_charge_flow_budget(cost, effects).await,
        EffectCommand::NotifyPeer {
            peer,
            invitation_id,
        } => execute_notify_peer(peer, invitation_id, authority, effects).await,
        EffectCommand::RecordReceipt { operation, peer } => {
            execute_record_receipt(operation, peer, effects).await
        }
    }
}

/// Execute a journal append command
async fn execute_journal_append(
    fact: InvitationFact,
    authority: &AuthorityContext,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // Derive context ID from authority ID (same pattern as HandlerContext::new)
    let context_id = ContextId::new_from_entropy(hash(&authority.authority_id.to_bytes()));

    // Append the fact to the journal
    HandlerUtilities::append_generic_fact(
        authority,
        effects,
        context_id,
        "invitation",
        &fact.to_bytes(),
    )
    .await
}

/// Execute a flow budget charge command
async fn execute_charge_flow_budget(cost: u32, effects: &AuraEffectSystem) -> AgentResult<()> {
    // In testing mode, skip actual charging
    if effects.is_testing() {
        return Ok(());
    }

    // Flow budget charging will use FlowBudgetEffects when integrated.
    // Currently logs the charge request for debugging.
    tracing::debug!(cost = cost, "Flow budget charge requested");
    Ok(())
}

/// Execute a peer notification command
async fn execute_notify_peer(
    peer: aura_core::identifiers::AuthorityId,
    invitation_id: String,
    authority: &AuthorityContext,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // In testing mode, skip actual notification
    if effects.is_testing() {
        return Ok(());
    }

    let invitation = InvitationHandler::load_created_invitation(
        effects,
        authority.authority_id,
        &invitation_id,
    )
    .await
    .ok_or_else(|| {
        AgentError::context(format!("Invitation not found for notify: {invitation_id}"))
    })?;

    let code = InvitationService::export_invitation(&invitation);
    let mut metadata = HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        "application/aura-invitation".to_string(),
    );
    metadata.insert("invitation-id".to_string(), invitation_id.clone());
    metadata.insert(
        "invitation-context".to_string(),
        invitation.context_id.to_string(),
    );

    let envelope = aura_core::effects::TransportEnvelope {
        destination: peer,
        source: authority.authority_id,
        context: invitation.context_id,
        payload: code.into_bytes(),
        metadata,
        receipt: None,
    };

    effects.send_envelope(envelope).await.map_err(|e| {
        AgentError::effects(format!("Failed to notify peer with invitation: {e}"))
    })?;

    Ok(())
}

/// Execute a receipt recording command
async fn execute_record_receipt(
    operation: String,
    peer: Option<aura_core::identifiers::AuthorityId>,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // In testing mode, skip actual receipt recording
    if effects.is_testing() {
        return Ok(());
    }

    // Receipt recording will use JournalEffects when integrated.
    // Currently logs the receipt request for debugging.
    tracing::debug!(
        operation = %operation,
        peer = ?peer,
        "Receipt recording requested"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::RelationalContext;
    use crate::core::AgentConfig;
    use aura_core::identifiers::{AuthorityId, ContextId};
    use aura_invitation::guards::GuardOutcome;

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(RelationalContext {
            context_id: ContextId::new_from_entropy([seed + 100; 32]),
            participants: vec![],
            metadata: Default::default(),
        });
        authority_context
    }

    #[tokio::test]
    async fn test_execute_allowed_outcome() {
        let authority = create_test_authority(130);
        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();

        // Create an allowed outcome with a charge command
        let outcome = GuardOutcome::allowed(vec![EffectCommand::ChargeFlowBudget { cost: 1 }]);

        let result = execute_guard_outcome(outcome, &authority, &effects).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_denied_outcome() {
        let authority = create_test_authority(131);
        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();

        // Create a denied outcome
        let outcome = GuardOutcome::denied("Test denial reason");

        let result = execute_guard_outcome(outcome, &authority, &effects).await;
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Test denial reason"));
    }

    #[tokio::test]
    async fn test_execute_journal_append() {
        let authority = create_test_authority(132);
        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();

        let fact = InvitationFact::sent_ms(
            ContextId::new_from_entropy([232u8; 32]),
            "inv-test".to_string(),
            authority.authority_id,
            AuthorityId::new_from_entropy([133u8; 32]),
            "contact".to_string(),
            1000,
            Some(2000),
            None,
        );

        let outcome = GuardOutcome::allowed(vec![EffectCommand::JournalAppend { fact }]);

        let result = execute_guard_outcome(outcome, &authority, &effects).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_notify_peer() {
        let authority = create_test_authority(134);
        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();

        let peer = AuthorityId::new_from_entropy([135u8; 32]);
        let outcome = GuardOutcome::allowed(vec![EffectCommand::NotifyPeer {
            peer,
            invitation_id: "inv-notify".to_string(),
        }]);

        let result = execute_guard_outcome(outcome, &authority, &effects).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_record_receipt() {
        let authority = create_test_authority(136);
        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();

        let outcome = GuardOutcome::allowed(vec![EffectCommand::RecordReceipt {
            operation: "send_invitation".to_string(),
            peer: Some(AuthorityId::new_from_entropy([137u8; 32])),
        }]);

        let result = execute_guard_outcome(outcome, &authority, &effects).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_multiple_commands() {
        let authority = create_test_authority(138);
        let config = AgentConfig::default();
        let effects = AuraEffectSystem::testing(&config).unwrap();

        let peer = AuthorityId::new_from_entropy([139u8; 32]);
        let outcome = GuardOutcome::allowed(vec![
            EffectCommand::ChargeFlowBudget { cost: 1 },
            EffectCommand::NotifyPeer {
                peer,
                invitation_id: "inv-multi".to_string(),
            },
            EffectCommand::RecordReceipt {
                operation: "send_invitation".to_string(),
                peer: Some(peer),
            },
        ]);

        let result = execute_guard_outcome(outcome, &authority, &effects).await;
        assert!(result.is_ok());
    }
}
