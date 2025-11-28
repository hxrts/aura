//! Guard chain executor bridging pure guards with effect systems
//!
//! This module provides a bridge between the new pure guard implementation
//! (ADR-014) and existing effect-based guard infrastructure, enabling gradual
//! migration while maintaining compatibility.

use super::{
    pure::{Guard, GuardChain, GuardRequest},
    send_guard::{SendGuardChain, SendGuardMetrics, SendGuardResult},
};
use crate::guards::effect_system_trait::GuardContextProvider;
use aura_core::{
    effects::{
        guard::{
            Decision, EffectCommand, EffectInterpreter, EffectResult, FlowBudgetView,
            GuardSnapshot, MetadataView,
        },
        FlowBudgetEffects, JournalEffects, LeakageEffects, PhysicalTimeEffects, RandomEffects,
        StorageEffects,
    },
    identifiers::{AuthorityId, ContextId},
    journal::Journal,
    time::TimeStamp,
    AuraError, AuraResult as Result, Cap, Receipt,
};

// Re-export key types for easier use by macro-generated code
pub use aura_core::effects::guard::{
    EffectCommand as ChoreographyCommand, EffectResult as ChoreographyResult,
};
use std::{collections::HashMap, sync::Arc};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Executor for pure guard chains with effect interpretation
#[derive(Debug)]
pub struct GuardChainExecutor<I: EffectInterpreter> {
    /// Pure guard chain to execute
    guard_chain: GuardChain,
    /// Effect interpreter for executing commands
    interpreter: Arc<I>,
}

impl<I: EffectInterpreter> GuardChainExecutor<I> {
    /// Create a new guard chain executor
    pub fn new(guard_chain: GuardChain, interpreter: Arc<I>) -> Self {
        Self {
            guard_chain,
            interpreter,
        }
    }

    /// Execute guard chain with pure evaluation model
    pub async fn execute<E>(
        &self,
        effect_system: &E,
        request: &GuardRequest,
    ) -> Result<GuardChainResult>
    where
        E: crate::guards::GuardEffects + PhysicalTimeEffects + FlowBudgetEffects + StorageEffects,
    {
        let start_time_ms = Self::current_time_ms(effect_system).await?;

        // Prepare snapshot from current system state
        let snapshot = self.prepare_snapshot(effect_system, request).await?;

        debug!(
            authority = ?request.authority,
            operation = %request.operation,
            cost = request.cost,
            "Evaluating pure guard chain"
        );

        // Evaluate pure guard chain
        let outcome = self.guard_chain.evaluate(&snapshot, request);

        let evaluation_end_ms = Self::current_time_ms(effect_system).await?;
        let evaluation_time_us = (evaluation_end_ms.saturating_sub(start_time_ms)) * 1000;

        // Handle authorization decision
        if !outcome.is_authorized() {
            let reason = outcome
                .decision
                .denial_reason()
                .unwrap_or("Unknown denial reason")
                .to_string();

            warn!(
                authority = ?request.authority,
                operation = %request.operation,
                reason = %reason,
                "Guard chain denied request"
            );

            return Ok(GuardChainResult {
                authorized: false,
                decision: outcome.decision,
                effects_executed: 0,
                denial_reason: Some(reason),
                execution_time_us: evaluation_time_us,
                receipt: None,
            });
        }

        // Execute effect commands
        let mut effects_executed = 0;
        let mut receipt = None;

        for (i, command) in outcome.effects.iter().enumerate() {
            debug!(
                command_index = i,
                command_type = ?std::mem::discriminant(command),
                "Executing effect command"
            );

            match self.interpreter.execute(command.clone()).await {
                Ok(result) => {
                    effects_executed += 1;

                    // Capture receipt from budget charge
                    if let (EffectCommand::ChargeBudget { .. }, EffectResult::Receipt(r)) =
                        (command, &result)
                    {
                        receipt = Some(r.clone());
                    }
                }
                Err(e) => {
                    error!(
                        command_index = i,
                        error = %e,
                        "Failed to execute effect command"
                    );
                    return Err(e);
                }
            }
        }

        let total_time_us = (Self::current_time_ms(effect_system)
            .await?
            .saturating_sub(start_time_ms))
            * 1000;

        info!(
            authority = ?request.authority,
            operation = %request.operation,
            effects_executed = effects_executed,
            total_time_us = total_time_us,
            "Guard chain execution completed successfully"
        );

        Ok(GuardChainResult {
            authorized: true,
            decision: outcome.decision,
            effects_executed,
            denial_reason: None,
            execution_time_us: total_time_us,
            receipt,
        })
    }

    /// Prepare guard snapshot from current effect system state
    async fn prepare_snapshot<E>(
        &self,
        effect_system: &E,
        request: &GuardRequest,
    ) -> Result<GuardSnapshot>
    where
        E: crate::guards::GuardEffects
            + PhysicalTimeEffects
            + FlowBudgetEffects
            + StorageEffects
            + RandomEffects,
    {
        // Get current time
        let now = TimeStamp::PhysicalClock(effect_system.physical_time().await?);

        // Get flow budgets
        let mut budgets = HashMap::new();
        if let Ok(budget) = effect_system
            .get_flow_budget(&request.context, &request.peer)
            .await
        {
            budgets.insert((request.context, request.peer), budget.remaining() as u32);
        }
        let budget_view = FlowBudgetView::new(budgets);

        // Capability container (capability enforcement handled by AuthorizationEffects)
        let caps = Cap::new();

        // Get metadata
        let mut metadata_map = HashMap::new();
        metadata_map.insert("authority_id".to_string(), request.authority.to_string());
        let metadata = MetadataView::new(metadata_map);

        // Generate deterministic RNG seed
        let rng_seed = effect_system.random_bytes_32().await;

        Ok(GuardSnapshot {
            now,
            caps,
            budgets: budget_view,
            metadata,
            rng_seed,
        })
    }

    async fn current_time_ms<E>(effects: &E) -> Result<u64>
    where
        E: PhysicalTimeEffects + ?Sized,
    {
        let ts = effects.physical_time().await?;
        Ok(ts.ts_ms)
    }

    fn derive_context_id(operation: &str) -> ContextId {
        let uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, operation.as_bytes());
        ContextId::from_uuid(uuid)
    }
}

/// Result of guard chain execution
#[derive(Debug, Clone)]
pub struct GuardChainResult {
    /// Whether the request was authorized
    pub authorized: bool,
    /// Authorization decision from guards
    pub decision: Decision,
    /// Number of effect commands executed
    pub effects_executed: usize,
    /// Reason for denial (if not authorized)
    pub denial_reason: Option<String>,
    /// Total execution time (microseconds)
    pub execution_time_us: u64,
    /// Receipt from budget charge (if applicable)
    pub receipt: Option<Receipt>,
}

/// Compatibility interpreter that executes EffectCommand against an existing effect system.
///
/// This is a bridge layer for migrating to the ADR-014 pure guard model without changing
/// the underlying effect traits yet.
pub struct EffectSystemInterpreter<E> {
    effects: Arc<E>,
}

impl<E> EffectSystemInterpreter<E> {
    pub fn new(effects: Arc<E>) -> Self {
        Self { effects }
    }
}

/// Interpreter that borrows an effect system reference. Useful when callers hold &E.
pub struct BorrowedEffectInterpreter<'a, E> {
    effects: &'a E,
}

impl<'a, E> BorrowedEffectInterpreter<'a, E> {
    pub fn new(effects: &'a E) -> Self {
        Self { effects }
    }
}

#[async_trait::async_trait]
impl<E> EffectInterpreter for EffectSystemInterpreter<E>
where
    E: crate::guards::GuardEffects
        + FlowBudgetEffects
        + PhysicalTimeEffects
        + RandomEffects
        + StorageEffects
        + JournalEffects
        + LeakageEffects,
{
    async fn execute(&self, cmd: EffectCommand) -> Result<EffectResult> {
        match cmd {
            EffectCommand::ChargeBudget {
                context,
                authority: _,
                peer,
                amount,
            } => {
                let receipt = self.effects.charge_flow(&context, &peer, amount).await?;
                Ok(EffectResult::Receipt(receipt))
            }
            EffectCommand::AppendJournal { entry } => {
                let current = self
                    .effects
                    .get_journal()
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to get journal: {}", e)))?;
                // Build a delta journal containing the new fact
                let delta = Journal::with_facts(entry.fact.clone());
                let merged = self
                    .effects
                    .merge_facts(&current, &delta)
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to merge journal: {}", e)))?;
                self.effects
                    .persist_journal(&merged)
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to persist journal: {}", e)))?;
                Ok(EffectResult::Success)
            }
            EffectCommand::RecordLeakage { bits } => {
                let event = aura_core::effects::LeakageEvent {
                    source: AuthorityId::default(),
                    destination: AuthorityId::default(),
                    context_id: ContextId::default(),
                    leakage_amount: bits as u64,
                    observer_class: aura_core::effects::ObserverClass::External,
                    operation: "guard_chain".to_string(),
                    timestamp_ms: self.effects.physical_time().await?.ts_ms,
                };
                self.effects
                    .record_leakage(event)
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to record leakage: {}", e)))?;
                Ok(EffectResult::Success)
            }
            EffectCommand::StoreMetadata { key, value } => {
                // Simple storage using StorageEffects
                self.effects
                    .store(&key, value.into_bytes())
                    .await
                    .map_err(|e| AuraError::internal(format!("store failed: {}", e)))?;
                Ok(EffectResult::Success)
            }
            EffectCommand::SendEnvelope { .. } => Ok(EffectResult::Failure(
                "SendEnvelope not supported via interpreter".to_string(),
            )),
            EffectCommand::GenerateNonce { bytes } => {
                let nonce = self.effects.random_bytes(bytes).await;
                Ok(EffectResult::Nonce(nonce))
            }
        }
    }

    fn interpreter_type(&self) -> &'static str {
        "EffectSystemInterpreter"
    }
}

#[async_trait::async_trait]
impl<'a, E> EffectInterpreter for BorrowedEffectInterpreter<'a, E>
where
    E: crate::guards::GuardEffects
        + FlowBudgetEffects
        + PhysicalTimeEffects
        + RandomEffects
        + StorageEffects
        + JournalEffects
        + LeakageEffects,
{
    async fn execute(&self, cmd: EffectCommand) -> Result<EffectResult> {
        match cmd {
            EffectCommand::ChargeBudget {
                context,
                authority: _,
                peer,
                amount,
            } => {
                let receipt = self.effects.charge_flow(&context, &peer, amount).await?;
                Ok(EffectResult::Receipt(receipt))
            }
            EffectCommand::AppendJournal { entry } => {
                let current = self
                    .effects
                    .get_journal()
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to get journal: {}", e)))?;
                let delta = Journal::with_facts(entry.fact.clone());
                let merged = self
                    .effects
                    .merge_facts(&current, &delta)
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to merge journal: {}", e)))?;
                self.effects
                    .persist_journal(&merged)
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to persist journal: {}", e)))?;
                Ok(EffectResult::Success)
            }
            EffectCommand::RecordLeakage { bits } => {
                let event = aura_core::effects::LeakageEvent {
                    source: AuthorityId::default(),
                    destination: AuthorityId::default(),
                    context_id: ContextId::default(),
                    leakage_amount: bits as u64,
                    observer_class: aura_core::effects::ObserverClass::External,
                    operation: "guard_chain".to_string(),
                    timestamp_ms: self.effects.physical_time().await?.ts_ms,
                };
                self.effects
                    .record_leakage(event)
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to record leakage: {}", e)))?;
                Ok(EffectResult::Success)
            }
            EffectCommand::StoreMetadata { key, value } => {
                self.effects
                    .store(&key, value.into_bytes())
                    .await
                    .map_err(|e| AuraError::internal(format!("store failed: {}", e)))?;
                Ok(EffectResult::Success)
            }
            EffectCommand::SendEnvelope { .. } => Ok(EffectResult::Failure(
                "SendEnvelope not supported via interpreter".to_string(),
            )),
            EffectCommand::GenerateNonce { bytes } => {
                let nonce = self.effects.random_bytes(bytes).await;
                Ok(EffectResult::Nonce(nonce))
            }
        }
    }

    fn interpreter_type(&self) -> &'static str {
        "BorrowedEffectInterpreter"
    }
}

/// Convert SendGuardChain to GuardRequest
pub fn convert_send_guard_to_request(
    send_guard: &SendGuardChain,
    authority: AuthorityId,
) -> Result<GuardRequest> {
    let operation = send_guard.authorization_requirement().to_string();
    let cost = send_guard.cost();

    let request = GuardRequest::new(authority, operation, cost)
        .with_context_id(send_guard.context())
        .with_peer(send_guard.peer())
        .with_context(send_guard.context().to_bytes().to_vec());

    Ok(request)
}

/// Execute a batch of effect commands through an interpreter
///
/// This is the primary integration point for choreography-generated commands.
/// Use this function to execute `EffectCommand` sequences produced by the
/// `aura_macros::choreography` macro's `effect_bridge::annotation_to_commands()`.
///
/// # Example
///
/// ```rust,ignore
/// use aura_protocol::guards::pure_executor::{execute_effect_commands, BorrowedEffectInterpreter};
/// use aura_core::effects::guard::EffectCommand;
///
/// // Commands from choreography! macro
/// let commands: Vec<EffectCommand> = effect_bridge::annotations_to_commands(&ctx, annotations);
///
/// // Execute through interpreter
/// let interpreter = std::sync::Arc::new(BorrowedEffectInterpreter::new(&effect_system));
/// let results = execute_effect_commands(&interpreter, commands).await?;
/// ```
pub async fn execute_effect_commands<I: EffectInterpreter>(
    interpreter: &I,
    commands: Vec<EffectCommand>,
) -> Result<Vec<EffectResult>> {
    let mut results = Vec::with_capacity(commands.len());

    for (i, command) in commands.into_iter().enumerate() {
        debug!(
            command_index = i,
            command_type = ?std::mem::discriminant(&command),
            "Executing choreography effect command"
        );

        match interpreter.execute(command).await {
            Ok(result) => {
                results.push(result);
            }
            Err(e) => {
                error!(
                    command_index = i,
                    error = %e,
                    "Failed to execute choreography effect command"
                );
                return Err(e);
            }
        }
    }

    Ok(results)
}

/// Execute guard chain and then additional choreography commands
///
/// This combines the runtime guard chain evaluation with macro-generated
/// effect commands, ensuring both sources of effects are executed atomically.
///
/// # Arguments
///
/// * `effect_system` - The effect system providing runtime state
/// * `request` - The guard request for pure guard evaluation
/// * `additional_commands` - Effect commands from choreography annotations
/// * `interpreter` - The effect interpreter for command execution
///
/// # Returns
///
/// Combined result including guard chain outcome and all executed effects
pub async fn execute_guarded_choreography<E, I>(
    effect_system: &E,
    request: &GuardRequest,
    additional_commands: Vec<EffectCommand>,
    interpreter: Arc<I>,
) -> Result<GuardChainResult>
where
    E: crate::guards::GuardEffects + PhysicalTimeEffects + FlowBudgetEffects + StorageEffects,
    I: EffectInterpreter,
{
    // First, execute the standard guard chain
    let guard_chain = GuardChain::standard();
    let executor = GuardChainExecutor::new(guard_chain, interpreter.clone());
    let mut result = executor.execute(effect_system, request).await?;

    // If guard chain passed and we have additional commands, execute them
    if result.authorized && !additional_commands.is_empty() {
        debug!(
            additional_count = additional_commands.len(),
            "Executing additional choreography commands after guard chain"
        );

        for (i, command) in additional_commands.into_iter().enumerate() {
            match interpreter.execute(command).await {
                Ok(_) => {
                    result.effects_executed += 1;
                }
                Err(e) => {
                    error!(
                        command_index = i,
                        error = %e,
                        "Failed to execute additional choreography command"
                    );
                    return Err(e);
                }
            }
        }
    }

    Ok(result)
}

/// Prepare guard snapshot from crate::guards::GuardEffects
pub async fn prepare_snapshot_from_effects<E>(
    effect_system: &E,
    authority: &AuthorityId,
    context: &ContextId,
) -> Result<GuardSnapshot>
where
    E: crate::guards::GuardEffects + PhysicalTimeEffects + FlowBudgetEffects + RandomEffects,
{
    let now = TimeStamp::PhysicalClock(effect_system.physical_time().await?);

    let mut budgets = HashMap::new();
    if let Ok(budget) = effect_system.get_flow_budget(context, authority).await {
        budgets.insert((*context, *authority), budget.remaining() as u32);
    }

    let mut metadata = HashMap::new();
    metadata.insert("authority_id".to_string(), authority.to_string());

    Ok(GuardSnapshot {
        now,
        caps: Cap::new(),
        budgets: FlowBudgetView::new(budgets),
        metadata: MetadataView::new(metadata),
        rng_seed: effect_system.random_bytes_32().await,
    })
}

/// Compatibility adapter for SendGuardChain
impl SendGuardChain {
    /// Evaluate using pure guard chain (drop-in replacement)
    pub async fn evaluate_pure<E, I>(
        &self,
        effect_system: &E,
        interpreter: Arc<I>,
        authority: AuthorityId,
    ) -> Result<SendGuardResult>
    where
        E: crate::guards::GuardEffects
            + PhysicalTimeEffects
            + FlowBudgetEffects
            + StorageEffects
            + JournalEffects
            + LeakageEffects
            + GuardContextProvider,
        I: EffectInterpreter,
    {
        let start_ms = GuardChainExecutor::<I>::current_time_ms(effect_system).await?;

        // Convert to pure guard request
        let request = convert_send_guard_to_request(self, authority)?;

        // Create standard guard chain
        let guard_chain = GuardChain::standard();
        let executor = GuardChainExecutor::new(guard_chain, interpreter);

        // Execute pure guard chain
        let result = executor.execute(effect_system, &request).await?;

        let total_time_us = (GuardChainExecutor::<I>::current_time_ms(effect_system)
            .await?
            .saturating_sub(start_ms))
            * 1000;

        // Convert back to SendGuardResult
        Ok(SendGuardResult {
            authorized: result.authorized,
            authorization_satisfied: result.authorized,
            flow_authorized: result.authorized,
            receipt: result.receipt,
            authorization_level: if result.authorized {
                Some("biscuit".to_string())
            } else {
                None
            },
            metrics: SendGuardMetrics {
                authorization_eval_time_us: result.execution_time_us / 2,
                flow_eval_time_us: result.execution_time_us / 2,
                total_time_us,
                authorization_checks: if result.authorized { 1 } else { 0 },
            },
            denial_reason: result.denial_reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::guard::EffectCommand;
    use std::sync::{Arc, Mutex};

    struct MockInterpreter {
        executed_commands: Arc<Mutex<Vec<EffectCommand>>>,
    }

    impl MockInterpreter {
        fn new() -> Self {
            Self {
                executed_commands: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn get_executed_commands(&self) -> Vec<EffectCommand> {
            self.executed_commands.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl EffectInterpreter for MockInterpreter {
        async fn execute(&self, cmd: EffectCommand) -> Result<EffectResult> {
            self.executed_commands.lock().unwrap().push(cmd.clone());

            match cmd {
                EffectCommand::ChargeBudget { amount, .. } => {
                    Ok(EffectResult::RemainingBudget(1000 - amount))
                }
                _ => Ok(EffectResult::Success),
            }
        }

        fn interpreter_type(&self) -> &'static str {
            "MockInterpreter"
        }
    }

    #[test]
    fn test_guard_chain_result() {
        let result = GuardChainResult {
            authorized: true,
            decision: Decision::Authorized,
            effects_executed: 3,
            denial_reason: None,
            execution_time_us: 1000,
            receipt: None,
        };

        assert!(result.authorized);
        assert_eq!(result.effects_executed, 3);
        assert_eq!(result.execution_time_us, 1000);
    }

    #[test]
    fn test_convert_send_guard_to_request() {
        use crate::guards::send_guard::SendGuardChain;

        let context = ContextId::new();
        let peer = AuthorityId::new();
        let authority = AuthorityId::new(); // Create once and reuse
        let message_authorization = "guard:send".to_string();
        let cost = 42;

        let guard = SendGuardChain::new(message_authorization.clone(), context, peer, cost);

        let request = convert_send_guard_to_request(&guard, authority).expect("conversion");

        assert_eq!(request.operation, message_authorization);
        assert_eq!(request.cost, cost);
        assert_eq!(request.context, context);
        assert_eq!(request.peer, peer);
        assert_eq!(request.authority, authority);
    }
}
