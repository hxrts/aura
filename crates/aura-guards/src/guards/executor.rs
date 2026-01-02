//! Guard chain executor bridging pure guards with effect systems
//!
//! This module provides a bridge between the pure guard implementation
//! and effect-based guard infrastructure, enabling gradual
//! migration while maintaining compatibility.

use super::{
    chain::{SendGuardChain, SendGuardMetrics, SendGuardResult},
    pure::{Guard, GuardChain, GuardRequest},
};
use crate::authorization::BiscuitAuthorizationBridge;
use crate::guards::biscuit_evaluator::BiscuitGuardEvaluator;
use crate::guards::traits::{require_biscuit_metadata, GuardContextProvider};
use crate::guards::types::{CapabilityId, GuardOperationId};
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
    AuraError, AuraResult as Result, Cap, FlowCost, Receipt,
};

// Re-export key types for easier use by macro-generated code
use aura_authorization::ResourceScope;
pub use aura_core::effects::guard::{
    EffectCommand as ChoreographyCommand, EffectResult as ChoreographyResult,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use biscuit_auth::{Biscuit, PublicKey};
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
        E: crate::guards::GuardEffects
            + GuardContextProvider
            + PhysicalTimeEffects
            + FlowBudgetEffects
            + StorageEffects,
    {
        let start_time_ms = Self::current_time_ms(effect_system).await?;

        // Prepare snapshot from current system state
        let snapshot = self.prepare_snapshot(effect_system, request).await?;

        debug!(
            authority = ?request.authority,
            operation = %request.operation,
            cost = ?request.cost,
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
                .map(ToString::to_string)
                .unwrap_or_else(|| "Unknown denial reason".to_string());

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
            + GuardContextProvider
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
            let remaining = aura_core::FlowCost::try_from(budget.remaining())
                .map_err(|e| AuraError::invalid(e.to_string()))?;
            budgets.insert((request.context, request.peer), remaining);
        }
        let budget_view = FlowBudgetView::new(budgets);

        // Capability container (capability enforcement handled by AuthorizationEffects)
        let caps = Cap::new();

        // Get metadata
        let mut metadata_map = HashMap::new();
        metadata_map.insert("authority_id".to_string(), request.authority.to_string());

        let authz_key = format!("authz:{}", request.operation);
        let authz_ok = self
            .evaluate_biscuit_authorization(effect_system, request)
            .await;
        metadata_map.insert(
            authz_key,
            if authz_ok {
                "allow".to_string()
            } else {
                "deny".to_string()
            },
        );
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

    fn derive_context_id(operation: &GuardOperationId) -> ContextId {
        let op_bytes = operation.to_string();
        let uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, op_bytes.as_bytes());
        ContextId::from_uuid(uuid)
    }

    async fn evaluate_biscuit_authorization<E>(
        &self,
        effect_system: &E,
        request: &GuardRequest,
    ) -> bool
    where
        E: GuardContextProvider + PhysicalTimeEffects,
    {
        if request.operation.is_empty() {
            return true;
        }

        let (token_b64, root_pk_b64) = match require_biscuit_metadata(effect_system) {
            Ok(values) => values,
            Err(err) => {
                if !effect_system.execution_mode().is_production() {
                    debug!(
                        operation = %request.operation,
                        error = %err,
                        "Missing Biscuit metadata in non-production mode; allowing"
                    );
                    return true;
                }
                warn!(
                    operation = %request.operation,
                    error = %err,
                    "Missing Biscuit metadata in production; denying"
                );
                return false;
            }
        };

        let root_bytes = match BASE64.decode(&root_pk_b64) {
            Ok(bytes) => bytes,
            Err(err) => {
                warn!(
                    operation = %request.operation,
                    error = %err,
                    "Failed to decode Biscuit root key"
                );
                return false;
            }
        };

        let root_pk = match PublicKey::from_bytes(&root_bytes) {
            Ok(key) => key,
            Err(err) => {
                warn!(
                    operation = %request.operation,
                    error = %err,
                    "Invalid Biscuit root key"
                );
                return false;
            }
        };

        let token = match Biscuit::from_base64(&token_b64, |_| Ok(root_pk)) {
            Ok(token) => token,
            Err(err) => {
                warn!(
                    operation = %request.operation,
                    error = %err,
                    "Invalid Biscuit token"
                );
                return false;
            }
        };

        let bridge = BiscuitAuthorizationBridge::new(root_pk, effect_system.authority_id());
        let evaluator = BiscuitGuardEvaluator::new(bridge);

        let resource = ResourceScope::Context {
            context_id: request.context,
            operation: aura_authorization::ContextOp::UpdateParams,
        };

        let now_secs = match effect_system.physical_time().await {
            Ok(time) => time.ts_ms / 1000,
            Err(err) => {
                warn!(
                    operation = %request.operation,
                    error = %err,
                    "Failed to fetch time for Biscuit evaluation"
                );
                return false;
            }
        };

        let capability = CapabilityId::from(request.operation.to_string());
        match evaluator.check_guard(&token, &capability, &resource, now_secs) {
            Ok(authorized) => authorized,
            Err(err) => {
                warn!(
                    operation = %request.operation,
                    error = %err,
                    "Biscuit authorization evaluation failed"
                );
                false
            }
        }
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
                    .map_err(|e| AuraError::invalid(format!("Failed to get journal: {e}")))?;
                // Build a delta journal containing the new fact
                let delta = Journal::with_facts(entry.fact.clone());
                let merged = self
                    .effects
                    .merge_facts(&current, &delta)
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to merge journal: {e}")))?;
                self.effects
                    .persist_journal(&merged)
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to persist journal: {e}")))?;
                Ok(EffectResult::Success)
            }
            EffectCommand::RecordLeakage { bits } => {
                let timestamp = self.effects.physical_time().await?;
                let event = aura_core::effects::LeakageEvent {
                    source: AuthorityId::new_from_entropy([1u8; 32]),
                    destination: AuthorityId::new_from_entropy([1u8; 32]),
                    context_id: ContextId::new_from_entropy([2u8; 32]),
                    leakage_amount: bits as u64,
                    observer_class: aura_core::effects::ObserverClass::External,
                    operation: "guard_chain".to_string(),
                    timestamp,
                };
                self.effects
                    .record_leakage(event)
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to record leakage: {e}")))?;
                Ok(EffectResult::Success)
            }
            EffectCommand::StoreMetadata { key, value } => {
                // Simple storage using StorageEffects
                self.effects
                    .store(&key, value.into_bytes())
                    .await
                    .map_err(|e| AuraError::internal(format!("store failed: {e}")))?;
                Ok(EffectResult::Success)
            }
            EffectCommand::SendEnvelope { .. } => Ok(EffectResult::Failure(
                "SendEnvelope not supported via interpreter".to_string(),
            )),
            EffectCommand::GenerateNonce { bytes } => {
                let nonce = self.effects.random_bytes(bytes as usize).await;
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
                    .map_err(|e| AuraError::invalid(format!("Failed to get journal: {e}")))?;
                let delta = Journal::with_facts(entry.fact.clone());
                let merged = self
                    .effects
                    .merge_facts(&current, &delta)
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to merge journal: {e}")))?;
                self.effects
                    .persist_journal(&merged)
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to persist journal: {e}")))?;
                Ok(EffectResult::Success)
            }
            EffectCommand::RecordLeakage { bits } => {
                let timestamp = self.effects.physical_time().await?;
                let event = aura_core::effects::LeakageEvent {
                    source: AuthorityId::new_from_entropy([1u8; 32]),
                    destination: AuthorityId::new_from_entropy([1u8; 32]),
                    context_id: ContextId::new_from_entropy([2u8; 32]),
                    leakage_amount: bits as u64,
                    observer_class: aura_core::effects::ObserverClass::External,
                    operation: "guard_chain".to_string(),
                    timestamp,
                };
                self.effects
                    .record_leakage(event)
                    .await
                    .map_err(|e| AuraError::invalid(format!("Failed to record leakage: {e}")))?;
                Ok(EffectResult::Success)
            }
            EffectCommand::StoreMetadata { key, value } => {
                self.effects
                    .store(&key, value.into_bytes())
                    .await
                    .map_err(|e| AuraError::internal(format!("store failed: {e}")))?;
                Ok(EffectResult::Success)
            }
            EffectCommand::SendEnvelope { .. } => Ok(EffectResult::Failure(
                "SendEnvelope not supported via interpreter".to_string(),
            )),
            EffectCommand::GenerateNonce { bytes } => {
                let nonce = self.effects.random_bytes(bytes as usize).await;
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
    let operation = GuardOperationId::from(send_guard.authorization_requirement());
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
/// use aura_guards::executor::{execute_effect_commands, BorrowedEffectInterpreter};
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
    E: crate::guards::GuardEffects
        + GuardContextProvider
        + PhysicalTimeEffects
        + FlowBudgetEffects
        + StorageEffects,
    I: EffectInterpreter,
{
    let plan = GuardPlan::new(request.clone(), additional_commands);
    execute_guard_plan(effect_system, &plan, interpreter).await
}

/// Shared guard plan for send-site and choreography execution.
#[derive(Debug, Clone)]
pub struct GuardPlan {
    request: GuardRequest,
    additional_commands: Vec<EffectCommand>,
}

impl GuardPlan {
    pub fn new(request: GuardRequest, additional_commands: Vec<EffectCommand>) -> Self {
        Self {
            request,
            additional_commands,
        }
    }

    pub fn from_send_guard(send_guard: &SendGuardChain, authority: AuthorityId) -> Result<Self> {
        let request = convert_send_guard_to_request(send_guard, authority)?;
        Ok(Self::new(request, Vec::new()))
    }

    pub fn request(&self) -> &GuardRequest {
        &self.request
    }

    pub fn additional_commands(&self) -> &[EffectCommand] {
        &self.additional_commands
    }
}

/// Execute a guard plan (shared for send-site + choreography paths).
pub async fn execute_guard_plan<E, I>(
    effect_system: &E,
    plan: &GuardPlan,
    interpreter: Arc<I>,
) -> Result<GuardChainResult>
where
    E: crate::guards::GuardEffects
        + GuardContextProvider
        + PhysicalTimeEffects
        + FlowBudgetEffects
        + StorageEffects,
    I: EffectInterpreter,
{
    // First, execute the standard guard chain
    let guard_chain = GuardChain::standard();
    let executor = GuardChainExecutor::new(guard_chain, interpreter.clone());
    let mut result = executor.execute(effect_system, &plan.request).await?;

    // If guard chain passed and we have additional commands, execute them
    if result.authorized && !plan.additional_commands.is_empty() {
        debug!(
            additional_count = plan.additional_commands.len(),
            "Executing additional choreography commands after guard chain"
        );

        for (i, command) in plan.additional_commands.iter().cloned().enumerate() {
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
        let remaining = FlowCost::try_from(budget.remaining())
            .map_err(|e| AuraError::invalid(e.to_string()))?;
        budgets.insert((*context, *authority), remaining);
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
    use std::sync::Arc;
    use tokio::sync::Mutex;

    struct MockInterpreter {
        executed_commands: Arc<Mutex<Vec<EffectCommand>>>,
    }

    impl MockInterpreter {
        fn new() -> Self {
            Self {
                executed_commands: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait::async_trait]
    impl EffectInterpreter for MockInterpreter {
        async fn execute(&self, cmd: EffectCommand) -> Result<EffectResult> {
            self.executed_commands.lock().await.push(cmd.clone());

            match cmd {
                EffectCommand::ChargeBudget { amount, .. } => {
                    Ok(EffectResult::RemainingBudget(
                        1000u32.saturating_sub(amount.value()),
                    ))
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
        use crate::guards::chain::SendGuardChain;

        let context = ContextId::new_from_entropy([74u8; 32]);
        let peer = AuthorityId::new_from_entropy([75u8; 32]);
        let authority = AuthorityId::new_from_entropy([76u8; 32]); // Create once and reuse
        let message_authorization = "guard:send";
        let cost = FlowCost::new(42);

        let guard = SendGuardChain::new(
            CapabilityId::from(message_authorization),
            context,
            peer,
            cost,
        );

        let request = match convert_send_guard_to_request(&guard, authority) {
            Ok(request) => request,
            Err(err) => panic!("conversion: {err}"),
        };

        assert_eq!(
            request.operation,
            GuardOperationId::from(message_authorization)
        );
        assert_eq!(request.cost, cost);
        assert_eq!(request.context, context);
        assert_eq!(request.peer, peer);
        assert_eq!(request.authority, authority);
    }
}
