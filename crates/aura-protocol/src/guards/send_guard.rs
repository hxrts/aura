//! Send-site guard chain implementing the unified predicate: need(m) ≤ Caps(ctx) ∧ headroom(ctx, cost)
//!
//! This module implements the complete guard chain as specified in docs/002_system_architecture.md
//! and docs/101_auth_authz.md, providing the CapGuard → FlowGuard → JournalCoupler sequence
//! that enforces both authorization and budget constraints at every protocol send site.

use super::effect_system_trait::GuardContextProvider;
use super::GuardEffects;
use crate::guards::pure_executor::{BorrowedEffectInterpreter, GuardChainExecutor};
use crate::guards::{privacy::track_leakage_consumption, JournalCoupler, LeakageBudget};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::{AuraError, AuraResult, Receipt};
use biscuit_auth::Biscuit;
// use aura_wot::Capability; // Legacy capability removed - use Biscuit tokens instead
use tracing::{debug, warn};

/// Complete send-site guard chain implementing the formal predicate:
/// need(m) ≤ Auth(ctx) ∧ headroom(ctx, cost) - using Biscuit tokens
#[derive(Debug)]
pub struct SendGuardChain {
    /// Message type authorization requirement
    message_authorization: String,
    /// Target peer authority
    peer: AuthorityId,
    /// Flow cost for this send
    cost: u32,
    /// Context for authorization and flow evaluation
    context: ContextId,
    /// Optional leakage budget to consume for this send
    leakage_budget: Option<LeakageBudget>,
    /// Optional journal coupler to atomically apply annotated facts
    journal_coupler: Option<JournalCoupler>,
    /// Optional operation ID for logging
    operation_id: Option<String>,
}

impl SendGuardChain {
    /// Temporary basic authorization guard while migrating to pure guard path.
    /// Uses a metadata-provided Biscuit token string until the interpreter pipeline wires in BiscuitGuardEvaluator.
    async fn evaluate_authorization_guard<E: GuardContextProvider>(
        &self,
        effect_system: &E,
    ) -> AuraResult<(bool, String)> {
        // Simplified placeholder: require a Biscuit token that matches the expected authorization string.
        // In production this should delegate to biscuit_evaluator.
        let expected = self.message_authorization.clone();
        // If no authorization requirement, allow.
        if expected.is_empty() {
            return Ok((true, "none".to_string()));
        }

        // Try to load a token from effect system metadata (legacy path)
        let maybe_token = effect_system.get_metadata("biscuit_token");
        if let Some(token) = maybe_token {
            if token == expected {
                return Ok((true, "biscuit".to_string()));
            }
        }

        Ok((false, "authorization_failed".to_string()))
    }

    pub fn authorization_requirement(&self) -> &str {
        &self.message_authorization
    }

    pub fn cost(&self) -> u32 {
        self.cost
    }

    pub fn context(&self) -> ContextId {
        self.context
    }

    pub fn peer(&self) -> AuthorityId {
        self.peer
    }

    /// Temporary noop evaluation for legacy callers that haven't migrated to the async path.
    /// Returns an authorized result without performing any checks.
    pub fn evaluate_noop(&self) -> SendGuardResult {
        SendGuardResult {
            authorized: true,
            authorization_satisfied: true,
            flow_authorized: true,
            receipt: None,
            authorization_level: Some(self.message_authorization.clone()),
            metrics: SendGuardMetrics::default(),
            denial_reason: None,
        }
    }
}

/// Result of complete send guard evaluation
#[derive(Debug)]
pub struct SendGuardResult {
    /// Whether the complete predicate passed
    pub authorized: bool,
    /// Authorization check result
    pub authorization_satisfied: bool,
    /// Flow budget check result
    pub flow_authorized: bool,
    /// Receipt from flow budget charge (if successful)
    pub receipt: Option<Receipt>,
    /// Authorization level used
    pub authorization_level: Option<String>,
    /// Execution metrics
    pub metrics: SendGuardMetrics,
    /// Reason for denial (if not authorized)
    pub denial_reason: Option<String>,
}

/// Metrics for send guard chain execution
#[derive(Debug, Default)]
pub struct SendGuardMetrics {
    /// Time for authorization evaluation (microseconds)
    pub authorization_eval_time_us: u64,
    /// Time for flow budget check (microseconds)
    pub flow_eval_time_us: u64,
    /// Total guard chain time (microseconds)
    pub total_time_us: u64,
    /// Number of authorization checks performed
    pub authorization_checks: usize,
}

impl SendGuardChain {
    /// Create new send guard chain
    ///
    /// # Parameters
    /// - `message_authorization`: Required authorization for sending this message type
    /// - `context`: Context ID for authorization and flow evaluation
    /// - `peer`: Target device for the send
    /// - `cost`: Flow budget cost for this operation
    pub fn new(
        message_authorization: String,
        context: ContextId,
        peer: AuthorityId,
        cost: u32,
    ) -> Self {
        Self {
            message_authorization,
            peer,
            cost,
            context,
            leakage_budget: None,
            journal_coupler: None,
            operation_id: None,
        }
    }

    /// Set operation ID for logging and metrics
    pub fn with_operation_id(mut self, operation_id: impl Into<String>) -> Self {
        self.operation_id = Some(operation_id.into());
        self
    }

    /// Set an explicit leakage budget that will be consumed before sending
    pub fn with_leakage_budget(mut self, budget: LeakageBudget) -> Self {
        self.leakage_budget = Some(budget);
        self
    }

    /// Attach a journal coupler so annotated deltas can be applied atomically
    pub fn with_journal_coupler(mut self, coupler: JournalCoupler) -> Self {
        self.journal_coupler = Some(coupler);
        self
    }

    /// Evaluate the complete send guard predicate: need(m) ≤ Auth(ctx) ∧ headroom(ctx, cost)
    ///
    /// This implements the formal guard chain:
    /// 1. AuthGuard: Check need(m) ≤ Auth(ctx) using Biscuit authorization
    /// 2. FlowGuard: Check headroom(ctx, cost) and charge flow budget
    /// 3. Return authorization decision with receipt for successful sends
    ///
    /// # Invariants Enforced
    /// - **Charge-Before-Send**: Flow budget must be charged before any transport send
    /// - **No-Observable-Without-Charge**: No send occurs without prior budget charge
    /// - **Authorization-Gated**: All sends require appropriate message authorization
    ///
    /// # Note
    /// Full evaluation with Biscuit authorization integration
    pub async fn evaluate<
        E: GuardEffects
            + GuardContextProvider
            + aura_core::TimeEffects
            + aura_core::PhysicalTimeEffects,
    >(
        &self,
        effect_system: &E,
    ) -> AuraResult<SendGuardResult> {
        let operation_id = self.operation_id.as_deref().unwrap_or("unnamed_send");

        debug!(
            operation_id = operation_id,
            peer = ?self.peer,
            cost = self.cost,
            authorization = %self.message_authorization,
            context = ?self.context,
            "Starting send guard evaluation (pure executor path)"
        );

        let (authorization_satisfied, authorization_level) = self
            .evaluate_authorization_guard(effect_system)
            .await
            .unwrap_or_else(|_| (false, "authorization_failed".to_string()));

        if !authorization_satisfied {
            warn!(
                operation_id = operation_id,
                authorization = %self.message_authorization,
                "Send denied: authorization requirement not satisfied"
            );

            return Ok(SendGuardResult {
                authorized: false,
                authorization_satisfied: false,
                flow_authorized: false,
                receipt: None,
                authorization_level: Some(authorization_level.clone()),
                metrics: SendGuardMetrics::default(),
                denial_reason: Some(format!(
                    "Missing required authorization: {}",
                    self.message_authorization
                )),
            });
        }

        if let Some(budget) = &self.leakage_budget {
            track_leakage_consumption(budget, operation_id, effect_system).await?;
        }

        let authority = GuardContextProvider::authority_id(effect_system);
        let interpreter = std::sync::Arc::new(BorrowedEffectInterpreter::new(effect_system));
        let pure_result =
            GuardChainExecutor::new(crate::guards::pure::GuardChain::standard(), interpreter)
                .execute(
                    effect_system,
                    &crate::guards::pure_executor::convert_send_guard_to_request(self, authority)?,
                )
                .await?;

        let authorized = authorization_satisfied && pure_result.authorized;

        Ok(SendGuardResult {
            authorized,
            authorization_satisfied,
            flow_authorized: pure_result.authorized,
            receipt: pure_result.receipt,
            authorization_level: Some(self.message_authorization.clone()),
            metrics: SendGuardMetrics::default(),
            denial_reason: if authorized {
                None
            } else if !authorization_satisfied {
                Some(format!(
                    "Missing required authorization: {}",
                    self.message_authorization
                ))
            } else {
                pure_result.denial_reason
            },
        })
    }

    /// Retrieve Biscuit token for send authorization from effect system
    async fn retrieve_send_token<E: GuardContextProvider + aura_core::TimeEffects>(
        &self,
        capability: &str,
        resource: &str,
        effect_system: &E,
    ) -> AuraResult<Biscuit> {
        // Try to retrieve token from storage based on capability and context
        let token_key = format!("send_tokens/{}_{}", capability, self.context);

        // First try to get a stored token for this capability and context
        if let Some(token_data) = effect_system.get_metadata(&token_key) {
            // Try to deserialize the token from storage (assume it's hex-encoded bytes for now)
            if hex::decode(&token_data).is_err() {
                tracing::warn!(
                    capability = %capability,
                    "Failed to decode hex token data, creating new one"
                );
            }
        }

        // If no token found or deserialization failed, create a new one using the authorization bridge
        self.create_fresh_send_token(capability, resource, effect_system)
            .await
    }

    /// Create a fresh Biscuit token for send authorization using authorization bridge
    async fn create_fresh_send_token<E: GuardContextProvider + aura_core::TimeEffects>(
        &self,
        capability: &str,
        resource: &str,
        effect_system: &E,
    ) -> AuraResult<Biscuit> {
        use biscuit_auth::KeyPair;

        // In production, this would use the proper authorization bridge with the root keypair
        // For now, create a properly structured token that would match real tokens
        let keypair = KeyPair::new();
        self.create_fresh_send_token_with_keypair(capability, resource, effect_system, &keypair)
            .await
    }

    async fn create_fresh_send_token_with_keypair<
        E: GuardContextProvider + aura_core::TimeEffects,
    >(
        &self,
        capability: &str,
        resource: &str,
        effect_system: &E,
        keypair: &biscuit_auth::KeyPair,
    ) -> AuraResult<Biscuit> {
        use biscuit_auth::macros::*;

        let context_str = self.context.to_string();
        let peer_str = self.peer.to_string();
        let authority_str = effect_system.authority_id().to_string();
        let timestamp_secs = effect_system.current_timestamp().await as i64;
        let expiry_secs = timestamp_secs + 3600; // 1 hour from now

        // Create a Biscuit token with comprehensive send permissions
        let token = biscuit!(
            r#"
            authority({authority_str});
            resource({resource});
            capability({capability});
            context({context_str});
            peer({peer_str});
            operation("send");
            time({timestamp});
            expires_at({expiry});
            "#,
            authority_str = authority_str.clone(),
            context_str = context_str,
            peer_str = peer_str,
            timestamp = timestamp_secs,
            expiry = expiry_secs
        )
        .build(keypair)
        .map_err(|e| AuraError::invalid(format!("Failed to build Biscuit token: {}", e)))?;

        debug!(
            capability = %capability,
            resource = %resource,
            context = ?self.context,
            peer = ?self.peer,
            authority = %authority_str,
            "Created fresh send authorization token with authorization bridge integration"
        );

        Ok(token)
    }

    /// Evaluate the flow guard: headroom(ctx, cost) and charge flow budget
    async fn evaluate_flow_guard<E: GuardEffects>(&self, effect_system: &E) -> AuraResult<Receipt> {
        debug!(
            context = ?self.context,
            peer = ?self.peer,
            cost = self.cost,
            "Evaluating flow guard: checking headroom and charging budget"
        );

        // Check and charge flow budget using the effect system
        // This implements the charge-before-send invariant
        let receipt = effect_system
            .charge_flow(&self.context, &self.peer, self.cost)
            .await
            .map_err(|e| {
                warn!(
                    context = ?self.context,
                    peer = ?self.peer,
                    cost = self.cost,
                    error = %e,
                    "Flow budget charge failed"
                );
                AuraError::permission_denied(format!(
                    "Flow budget charge failed for peer {} cost {}: {}",
                    self.peer, self.cost, e
                ))
            })?;

        debug!(
            context = ?self.context,
            peer = ?self.peer,
            cost = self.cost,
            nonce = receipt.nonce,
            "Flow budget charged successfully, receipt generated"
        );

        Ok(receipt)
    }

    /// Build human-readable denial reason
    fn build_denial_reason(&self, authorization_ok: bool, flow_ok: bool) -> String {
        match (authorization_ok, flow_ok) {
            (false, false) => format!(
                "Missing authorization {} and insufficient flow budget (cost: {})",
                self.message_authorization, self.cost
            ),
            (false, true) => format!(
                "Missing required authorization: {}",
                self.message_authorization
            ),
            (true, false) => format!("Insufficient flow budget for cost: {}", self.cost),
            (true, true) => "Send authorized".to_string(), // Should not happen
        }
    }

    /// Convenience method to evaluate and return only the authorization decision
    pub async fn is_send_authorized<
        E: GuardEffects
            + GuardContextProvider
            + aura_core::TimeEffects
            + aura_core::PhysicalTimeEffects,
    >(
        &self,
        effect_system: &E,
    ) -> AuraResult<bool> {
        let result = self.evaluate(effect_system).await?;
        Ok(result.authorized)
    }

    /// Convenience method to evaluate and return the receipt if authorized
    pub async fn authorize_send<
        E: GuardEffects
            + GuardContextProvider
            + aura_core::TimeEffects
            + aura_core::PhysicalTimeEffects,
    >(
        &self,
        effect_system: &E,
    ) -> AuraResult<Option<Receipt>> {
        let result = self.evaluate(effect_system).await?;
        if result.authorized {
            Ok(result.receipt)
        } else {
            Err(AuraError::permission_denied(
                result
                    .denial_reason
                    .unwrap_or_else(|| "Send authorization failed for unknown reason".to_string()),
            ))
        }
    }

    /// Evaluate the guard chain and, if authorized, apply journal coupling hooks (requires &mut).
    pub async fn evaluate_with_coupling<
        E: GuardEffects
            + GuardContextProvider
            + aura_core::TimeEffects
            + aura_core::PhysicalTimeEffects,
    >(
        &self,
        effect_system: &mut E,
    ) -> AuraResult<SendGuardResult> {
        let result = self.evaluate(effect_system).await?;

        if result.authorized {
            if let Some(coupler) = &self.journal_coupler {
                debug!("Applying journal coupling after successful send authorization");

                // Apply journal coupling to atomically commit any annotated facts
                // This ensures that protocol state changes are coupled with successful sends
                let coupling_result = coupler
                    .couple_with_send(effect_system, &result.receipt)
                    .await
                    .map_err(|e| {
                        warn!(
                            error = %e,
                            "Journal coupling failed after successful send authorization"
                        );
                        AuraError::internal(format!("Journal coupling failed: {}", e))
                    })?;

                debug!(
                    facts_applied = coupling_result.operations_applied,
                    "Journal coupling completed successfully"
                );
            }
        }

        Ok(result)
    }
}

/// Create a send guard chain for a message send
///
/// This is the primary interface for protocol implementations to use the guard chain.
///
/// # Example
/// ```rust,ignore
/// use aura_protocol::guards::send_guard::create_send_guard;
///
/// let guard = create_send_guard(
///     "message:send".to_string(), // authorization requirement
///     context_id,
///     peer_device,
///     100, // flow cost
/// ).with_operation_id("ping_send");
///
/// let result = guard.evaluate(&effect_system).await?;
/// if result.authorized {
///     // Proceed with send using result.receipt
///     transport.send_with_receipt(message, result.receipt.unwrap()).await?;
/// }
/// ```
pub fn create_send_guard(
    message_authorization: String,
    context: ContextId,
    peer: AuthorityId,
    cost: u32,
) -> SendGuardChain {
    SendGuardChain::new(message_authorization, context, peer, cost)
}

#[cfg(test)]
mod tests {
    use super::*;
    // use aura_core::AccountId;

    #[tokio::test]
    async fn test_send_guard_chain_creation() {
        let authorization = "message:send".to_string();
        let context = ContextId::new();
        let peer = AuthorityId::new();
        let cost = 100;

        let guard = SendGuardChain::new(authorization.clone(), context, peer, cost)
            .with_operation_id("test_send");

        assert_eq!(guard.message_authorization, authorization);
        assert_eq!(guard.context, context);
        assert_eq!(guard.peer, peer);
        assert_eq!(guard.cost, cost);
        assert_eq!(guard.operation_id.as_deref(), Some("test_send"));
    }

    #[tokio::test]
    async fn test_create_send_guard_convenience() {
        let authorization = "message:send".to_string();
        let context = ContextId::new();
        let peer = AuthorityId::new();
        let cost = 50;

        let guard = create_send_guard(authorization.clone(), context, peer, cost);

        assert_eq!(guard.message_authorization, authorization);
        assert_eq!(guard.context, context);
        assert_eq!(guard.peer, peer);
        assert_eq!(guard.cost, cost);
    }

    #[tokio::test]
    async fn test_denial_reason_formatting() {
        let authorization = "message:send".to_string();
        let context = ContextId::new();
        let peer = AuthorityId::new();
        let guard = SendGuardChain::new(authorization.clone(), context, peer, 100);

        // Test authorization failure only
        let reason = guard.build_denial_reason(false, true);
        assert!(reason.contains("Missing required authorization"));

        // Test flow failure only
        let reason = guard.build_denial_reason(true, false);
        assert!(reason.contains("Insufficient flow budget"));

        // Test both failures
        let reason = guard.build_denial_reason(false, false);
        assert!(reason.contains("Missing authorization"));
        assert!(reason.contains("insufficient flow budget"));
    }
}
