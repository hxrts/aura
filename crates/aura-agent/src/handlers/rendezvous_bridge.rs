//! Rendezvous Bridge - Effect Command Execution
//!
//! Bridges between `aura_rendezvous::RendezvousService` guard outcomes
//! and the agent's effect system. Executes `EffectCommand` items after
//! guard approval.

use super::shared::HandlerUtilities;
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::consensus::build_consensus_params;
use crate::runtime::AuraEffectSystem;
use aura_consensus::protocol::run_consensus;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::{Hash32, Prestate};
use aura_journal::{DomainFact, FactJournal};
use aura_protocol::amp::AmpJournalEffects;
use aura_protocol::effects::TreeEffects;
use aura_rendezvous::{EffectCommand, GuardOutcome, RendezvousFact, RENDEZVOUS_FACT_TYPE_ID};

/// Execute a guard outcome's effect commands
///
/// Takes a `GuardOutcome` from `aura_rendezvous::RendezvousService` and
/// executes each `EffectCommand` using the agent's effect system.
///
/// # Arguments
/// * `outcome` - The guard outcome to execute
/// * `authority` - The authority context for the operation
/// * `context_id` - The context for the operation
/// * `effects` - The effect system to use for execution
///
/// # Returns
/// * `Ok(())` if all effects were executed successfully
/// * `Err(AgentError)` if any effect fails or if the outcome was denied
pub async fn execute_guard_outcome(
    outcome: GuardOutcome,
    authority: &AuthorityContext,
    context_id: ContextId,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // Check if the operation was denied
    if outcome.decision.is_denied() {
        let reason = match &outcome.decision {
            aura_rendezvous::GuardDecision::Deny { reason } => reason.as_str(),
            _ => "Operation denied",
        };
        return Err(AgentError::effects(format!(
            "Guard denied operation: {}",
            reason
        )));
    }

    // Execute each effect command
    for command in outcome.effects {
        execute_effect_command(command, authority, context_id, effects).await?;
    }

    Ok(())
}

/// Execute a single effect command
async fn execute_effect_command(
    command: EffectCommand,
    authority: &AuthorityContext,
    context_id: ContextId,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    match command {
        EffectCommand::JournalAppend { fact } => {
            execute_journal_append(fact, authority, context_id, effects).await
        }
        EffectCommand::ChargeFlowBudget { cost } => {
            execute_charge_flow_budget(cost, context_id, effects).await
        }
        EffectCommand::SendHandshake { peer, message } => {
            execute_send_handshake(peer, message, context_id, effects).await
        }
        EffectCommand::SendHandshakeResponse { peer, message } => {
            execute_send_handshake_response(peer, message, context_id, effects).await
        }
        EffectCommand::RecordReceipt { operation, peer } => {
            execute_record_receipt(operation, peer, context_id, effects).await
        }
    }
}

/// Execute a journal append command
async fn execute_journal_append(
    fact: RendezvousFact,
    authority: &AuthorityContext,
    context_id: ContextId,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    let policy = policy_for(CeremonyFlow::RendezvousSecureChannel);

    if matches!(fact, RendezvousFact::ChannelEstablished { .. })
        && policy.allows_mode(AgreementMode::ConsensusFinalized)
    {
        let tree_state = effects.get_current_state().await.map_err(|e| {
            AgentError::effects(format!("Failed to read tree state for rendezvous: {e}"))
        })?;
        let journal = effects.fetch_context_journal(context_id).await.map_err(|e| {
            AgentError::effects(format!("Failed to load rendezvous context journal: {e}"))
        })?;
        let context_commitment = context_commitment_from_journal(context_id, &journal)?;
        let prestate = Prestate::new(
            vec![(authority.authority_id, tree_state.root_commitment)],
            context_commitment,
        );
        let params = build_consensus_params(effects, authority.authority_id, effects).await.map_err(
            |e| AgentError::effects(format!("Failed to build rendezvous consensus params: {e}")),
        )?;
        let commit = run_consensus(&prestate, &fact, params, effects, effects).await.map_err(
            |e| AgentError::effects(format!("Rendezvous consensus finalization failed: {e}")),
        )?;

        effects
            .insert_relational_fact(commit.to_relational_fact())
            .await
            .map_err(|e| AgentError::effects(format!("Commit rendezvous consensus fact: {e}")))?;
    }

    // Append the fact to the journal
    HandlerUtilities::append_generic_fact(
        authority,
        effects,
        context_id,
        RENDEZVOUS_FACT_TYPE_ID,
        &fact.to_bytes(),
    )
    .await
}

fn context_commitment_from_journal(
    context: ContextId,
    journal: &FactJournal,
) -> AgentResult<Hash32> {
    let mut hasher = aura_core::hash::hasher();
    hasher.update(b"RELATIONAL_CONTEXT_FACTS");
    hasher.update(context.as_bytes());
    for fact in journal.facts.iter() {
        let bytes = aura_core::util::serialization::to_vec(fact)
            .map_err(|e| AgentError::effects(format!("Serialize context fact: {e}")))?;
        hasher.update(&bytes);
    }
    Ok(Hash32(hasher.finalize()))
}

/// Execute a flow budget charge command
async fn execute_charge_flow_budget(
    cost: u32,
    context_id: ContextId,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // In testing mode, skip actual charging
    if effects.is_testing() {
        return Ok(());
    }

    // Flow budget charging uses the FlowBudgetManager when available.
    // Currently logs the charge request for debugging.
    tracing::debug!(
        cost = cost,
        context = %context_id,
        "Rendezvous flow budget charge requested"
    );
    Ok(())
}

/// Execute a send handshake command
async fn execute_send_handshake(
    peer: AuthorityId,
    message: aura_rendezvous::protocol::HandshakeInit,
    context_id: ContextId,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // In testing mode, skip actual network send
    if effects.is_testing() {
        return Ok(());
    }

    // Handshake sending will use TransportEffects when integrated.
    // Currently logs the request for debugging.
    tracing::debug!(
        peer = %peer,
        context = %context_id,
        epoch = message.handshake.epoch,
        "Rendezvous handshake init send requested"
    );
    Ok(())
}

/// Execute a send handshake response command
async fn execute_send_handshake_response(
    peer: AuthorityId,
    message: aura_rendezvous::protocol::HandshakeComplete,
    context_id: ContextId,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // In testing mode, skip actual network send
    if effects.is_testing() {
        return Ok(());
    }

    // Handshake response sending will use TransportEffects when integrated.
    // Currently logs the request for debugging.
    tracing::debug!(
        peer = %peer,
        context = %context_id,
        channel_id = ?message.channel_id,
        "Rendezvous handshake complete send requested"
    );
    Ok(())
}

/// Execute a receipt recording command
async fn execute_record_receipt(
    operation: String,
    peer: AuthorityId,
    context_id: ContextId,
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
        peer = %peer,
        context = %context_id,
        "Rendezvous receipt recording requested"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::RelationalContext;
    use crate::core::AgentConfig;
    use aura_rendezvous::{GuardDecision, RendezvousDescriptor, TransportHint};
    use std::sync::Arc;

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
        let authority = create_test_authority(80);
        let context_id = ContextId::new_from_entropy([180u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let outcome = GuardOutcome {
            decision: GuardDecision::Allow,
            effects: vec![EffectCommand::ChargeFlowBudget { cost: 1 }],
        };

        let result = execute_guard_outcome(outcome, &authority, context_id, &effects).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_denied_outcome() {
        let authority = create_test_authority(81);
        let context_id = ContextId::new_from_entropy([181u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let outcome = GuardOutcome {
            decision: GuardDecision::Deny {
                reason: "Test denial".to_string(),
            },
            effects: vec![],
        };

        let result = execute_guard_outcome(outcome, &authority, context_id, &effects).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_journal_append() {
        let authority = create_test_authority(82);
        let context_id = ContextId::new_from_entropy([182u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let descriptor = RendezvousDescriptor {
            authority_id: authority.authority_id,
            context_id,
            transport_hints: vec![TransportHint::QuicDirect {
                addr: "127.0.0.1:8443".to_string(),
            }],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: 10000,
            nonce: [0u8; 32],
            display_name: None,
        };

        let outcome = GuardOutcome {
            decision: GuardDecision::Allow,
            effects: vec![EffectCommand::JournalAppend {
                fact: RendezvousFact::Descriptor(descriptor),
            }],
        };

        let result = execute_guard_outcome(outcome, &authority, context_id, &effects).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_record_receipt() {
        let authority = create_test_authority(83);
        let context_id = ContextId::new_from_entropy([183u8; 32]);
        let peer = AuthorityId::new_from_entropy([84u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let outcome = GuardOutcome {
            decision: GuardDecision::Allow,
            effects: vec![EffectCommand::RecordReceipt {
                operation: "test_operation".to_string(),
                peer,
            }],
        };

        let result = execute_guard_outcome(outcome, &authority, context_id, &effects).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_multiple_effects() {
        let authority = create_test_authority(85);
        let context_id = ContextId::new_from_entropy([185u8; 32]);
        let peer = AuthorityId::new_from_entropy([86u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let outcome = GuardOutcome {
            decision: GuardDecision::Allow,
            effects: vec![
                EffectCommand::ChargeFlowBudget { cost: 1 },
                EffectCommand::RecordReceipt {
                    operation: "multi_test".to_string(),
                    peer,
                },
            ],
        };

        let result = execute_guard_outcome(outcome, &authority, context_id, &effects).await;
        assert!(result.is_ok());
    }
}
