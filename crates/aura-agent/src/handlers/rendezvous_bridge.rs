//! Rendezvous Bridge - Effect Command Execution
//!
//! Bridges between `aura_rendezvous::RendezvousService` guard outcomes
//! and the agent's effect system. Executes `EffectCommand` items after
//! guard approval.

use super::shared::{context_commitment_from_journal, HandlerUtilities};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::consensus::build_consensus_params;
use crate::runtime::AuraEffectSystem;
use aura_consensus::protocol::run_consensus;
use aura_core::effects::{FlowBudgetEffects, TransportEffects, TransportEnvelope, TransportReceipt};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::{Hash32, Prestate, Receipt};
use aura_journal::DomainFact;
use aura_protocol::amp::AmpJournalEffects;
use aura_protocol::effects::TreeEffects;
use aura_rendezvous::{EffectCommand, GuardOutcome, RendezvousFact, RENDEZVOUS_FACT_TYPE_ID};
use std::collections::HashMap;

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

    let charge_peer = resolve_charge_peer(&outcome.effects, authority.authority_id());
    let mut pending_receipt: Option<Receipt> = None;

    // Execute each effect command
    for command in outcome.effects {
        execute_effect_command(
            command,
            authority,
            context_id,
            effects,
            charge_peer,
            &mut pending_receipt,
        )
        .await?;
    }

    Ok(())
}

fn resolve_charge_peer(commands: &[EffectCommand], fallback: AuthorityId) -> AuthorityId {
    commands
        .iter()
        .find_map(|command| match command {
            EffectCommand::SendHandshake { peer, .. }
            | EffectCommand::SendHandshakeResponse { peer, .. }
            | EffectCommand::RecordReceipt { peer, .. } => Some(*peer),
            _ => None,
        })
        .unwrap_or(fallback)
}

/// Execute a single effect command
async fn execute_effect_command(
    command: EffectCommand,
    authority: &AuthorityContext,
    context_id: ContextId,
    effects: &AuraEffectSystem,
    charge_peer: AuthorityId,
    pending_receipt: &mut Option<Receipt>,
) -> AgentResult<()> {
    match command {
        EffectCommand::JournalAppend { fact } => {
            execute_journal_append(fact, authority, context_id, effects).await
        }
        EffectCommand::ChargeFlowBudget { cost } => {
            *pending_receipt =
                execute_charge_flow_budget(cost, context_id, charge_peer, effects).await?;
            Ok(())
        }
        EffectCommand::SendHandshake { peer, message } => {
            let receipt = pending_receipt.take();
            execute_send_handshake(peer, message, authority, context_id, receipt, effects).await
        }
        EffectCommand::SendHandshakeResponse { peer, message } => {
            let receipt = pending_receipt.take();
            execute_send_handshake_response(peer, message, authority, context_id, receipt, effects)
                .await
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
        && !effects.is_testing()
    {
        let tree_state = effects.get_current_state().await.map_err(|e| {
            AgentError::effects(format!("Failed to read tree state for rendezvous: {e}"))
        })?;
        let journal = effects
            .fetch_context_journal(context_id)
            .await
            .map_err(|e| {
                AgentError::effects(format!("Failed to load rendezvous context journal: {e}"))
            })?;
        let context_commitment = context_commitment_from_journal(context_id, &journal)?;
        let prestate = Prestate::new(
            vec![(authority.authority_id(), Hash32(tree_state.root_commitment))],
            context_commitment,
        );
        let params = build_consensus_params(effects, authority.authority_id(), effects)
            .await
            .map_err(|e| {
                AgentError::effects(format!("Failed to build rendezvous consensus params: {e}"))
            })?;
        let commit = run_consensus(&prestate, &fact, params, effects, effects)
            .await
            .map_err(|e| {
                AgentError::effects(format!("Rendezvous consensus finalization failed: {e}"))
            })?;

        effects
            .commit_relational_facts(vec![commit.to_relational_fact()])
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

/// Execute a flow budget charge command
async fn execute_charge_flow_budget(
    cost: u32,
    context_id: ContextId,
    peer: AuthorityId,
    effects: &AuraEffectSystem,
) -> AgentResult<Option<Receipt>> {
    // In testing mode, skip actual charging
    if effects.is_testing() {
        return Ok(None);
    }

    let receipt = effects
        .charge_flow(&context_id, &peer, cost)
        .await
        .map_err(|e| {
            AgentError::effects(format!(
                "Failed to charge rendezvous flow budget: {e}"
            ))
        })?;

    Ok(Some(receipt))
}

fn transport_receipt_from_flow(receipt: Receipt) -> TransportReceipt {
    TransportReceipt {
        context: receipt.ctx,
        src: receipt.src,
        dst: receipt.dst,
        epoch: receipt.epoch.value(),
        cost: receipt.cost,
        nonce: receipt.nonce,
        prev: receipt.prev.0,
        sig: receipt.sig,
    }
}

/// Execute a send handshake command
async fn execute_send_handshake(
    peer: AuthorityId,
    message: aura_rendezvous::protocol::HandshakeInit,
    authority: &AuthorityContext,
    context_id: ContextId,
    receipt: Option<Receipt>,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // In testing mode, skip actual network send
    if effects.is_testing() {
        return Ok(());
    }

    let payload = serde_json::to_vec(&message).map_err(|e| {
        AgentError::internal(format!("Failed to serialize rendezvous handshake init: {e}"))
    })?;

    let mut metadata = HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        "application/aura-rendezvous-handshake-init".to_string(),
    );
    metadata.insert("protocol-version".to_string(), "1".to_string());
    metadata.insert(
        "rendezvous-epoch".to_string(),
        message.handshake.epoch.to_string(),
    );

    let envelope = TransportEnvelope {
        destination: peer,
        source: authority.authority_id(),
        context: context_id,
        payload,
        metadata,
        receipt: receipt.map(transport_receipt_from_flow),
    };

    effects
        .send_envelope(envelope)
        .await
        .map_err(|e| AgentError::effects(format!("Rendezvous handshake send failed: {e}")))?;
    Ok(())
}

/// Execute a send handshake response command
async fn execute_send_handshake_response(
    peer: AuthorityId,
    message: aura_rendezvous::protocol::HandshakeComplete,
    authority: &AuthorityContext,
    context_id: ContextId,
    receipt: Option<Receipt>,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // In testing mode, skip actual network send
    if effects.is_testing() {
        return Ok(());
    }

    let payload = serde_json::to_vec(&message).map_err(|e| {
        AgentError::internal(format!(
            "Failed to serialize rendezvous handshake completion: {e}"
        ))
    })?;

    let mut metadata = HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        "application/aura-rendezvous-handshake-complete".to_string(),
    );
    metadata.insert("protocol-version".to_string(), "1".to_string());
    metadata.insert(
        "rendezvous-epoch".to_string(),
        message.handshake.epoch.to_string(),
    );
    metadata.insert(
        "rendezvous-channel-id".to_string(),
        hex::encode(message.channel_id),
    );

    let envelope = TransportEnvelope {
        destination: peer,
        source: authority.authority_id(),
        context: context_id,
        payload,
        metadata,
        receipt: receipt.map(transport_receipt_from_flow),
    };

    effects
        .send_envelope(envelope)
        .await
        .map_err(|e| AgentError::effects(format!("Rendezvous handshake response failed: {e}")))?;
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
    use crate::core::AgentConfig;
    use aura_rendezvous::{GuardDecision, RendezvousDescriptor, TransportHint};
    use std::sync::Arc;

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        let authority_context = AuthorityContext::new(authority_id);
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
            authority_id: authority.authority_id(),
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
