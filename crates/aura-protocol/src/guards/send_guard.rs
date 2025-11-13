//! Send-site guard chain implementing the unified predicate: need(m) ≤ Caps(ctx) ∧ headroom(ctx, cost)
//!
//! This module implements the complete guard chain as specified in docs/002_system_architecture.md
//! and docs/101_auth_authz.md, providing the CapGuard → FlowGuard → JournalCoupler sequence
//! that enforces both authorization and budget constraints at every protocol send site.

#![allow(clippy::disallowed_methods)] // TODO: Replace direct time calls with effect system

use crate::effects::AuraEffectSystem;
use crate::guards::{flow::FlowGuard, ProtocolGuard};
use aura_core::{relationships::ContextId, AuraError, AuraResult, DeviceId, Receipt};
use aura_wot::{Capability, EffectiveCapabilitySet};
use std::time::Instant;
use tracing::{debug, info, warn};

/// Complete send-site guard chain implementing the formal predicate:
/// need(m) ≤ Caps(ctx) ∧ headroom(ctx, cost)
#[derive(Debug)]
pub struct SendGuardChain {
    /// Message type capability requirement
    message_capability: Capability,
    /// Target peer device
    peer: DeviceId,
    /// Flow cost for this send
    cost: u32,
    /// Context for capability and flow evaluation
    context: ContextId,
    /// Optional operation ID for logging
    operation_id: Option<String>,
}

/// Result of complete send guard evaluation
#[derive(Debug)]
pub struct SendGuardResult {
    /// Whether the complete predicate passed
    pub authorized: bool,
    /// Capability check result
    pub capability_satisfied: bool,
    /// Flow budget check result  
    pub flow_authorized: bool,
    /// Receipt from flow budget charge (if successful)
    pub receipt: Option<Receipt>,
    /// Effective capabilities evaluated
    pub effective_capabilities: Option<EffectiveCapabilitySet>,
    /// Execution metrics
    pub metrics: SendGuardMetrics,
    /// Reason for denial (if not authorized)
    pub denial_reason: Option<String>,
}

/// Metrics for send guard chain execution
#[derive(Debug, Default)]
pub struct SendGuardMetrics {
    /// Time for capability evaluation (microseconds)
    pub capability_eval_time_us: u64,
    /// Time for flow budget check (microseconds)
    pub flow_eval_time_us: u64,
    /// Total guard chain time (microseconds)
    pub total_time_us: u64,
    /// Number of capabilities checked
    pub capabilities_checked: usize,
}

impl SendGuardChain {
    /// Create new send guard chain
    ///
    /// # Parameters
    /// - `message_capability`: Required capability for sending this message type
    /// - `context`: Context ID for capability and flow evaluation  
    /// - `peer`: Target device for the send
    /// - `cost`: Flow budget cost for this operation
    pub fn new(
        message_capability: Capability,
        context: ContextId,
        peer: DeviceId,
        cost: u32,
    ) -> Self {
        Self {
            message_capability,
            peer,
            cost,
            context,
            operation_id: None,
        }
    }

    /// Set operation ID for logging and metrics
    pub fn with_operation_id(mut self, operation_id: impl Into<String>) -> Self {
        self.operation_id = Some(operation_id.into());
        self
    }

    /// Evaluate the complete send guard predicate: need(m) ≤ Caps(ctx) ∧ headroom(ctx, cost)
    ///
    /// This implements the formal guard chain:
    /// 1. CapGuard: Check need(m) ≤ Caps(ctx) using capability evaluation
    /// 2. FlowGuard: Check headroom(ctx, cost) and charge flow budget  
    /// 3. Return authorization decision with receipt for successful sends
    ///
    /// # Invariants Enforced
    /// - **Charge-Before-Send**: Flow budget must be charged before any transport send
    /// - **No-Observable-Without-Charge**: No send occurs without prior budget charge
    /// - **Capability-Gated**: All sends require appropriate message capabilities
    pub async fn evaluate(&self, effect_system: &AuraEffectSystem) -> AuraResult<SendGuardResult> {
        let start_time = Instant::now();
        let operation_id = self.operation_id.as_deref().unwrap_or("unnamed_send");

        debug!(
            operation_id = operation_id,
            peer = ?self.peer,
            cost = self.cost,
            capability = ?self.message_capability,
            context = ?self.context,
            "Starting send guard chain evaluation"
        );

        // Phase 1: CapGuard - Evaluate need(m) ≤ Caps(ctx)
        let cap_start = Instant::now();
        let (capability_satisfied, effective_capabilities) =
            self.evaluate_capability_guard(effect_system).await?;
        let cap_time = cap_start.elapsed();

        if !capability_satisfied {
            let total_time = start_time.elapsed();
            warn!(
                operation_id = operation_id,
                capability = ?self.message_capability,
                "Send denied: capability requirement not satisfied"
            );

            return Ok(SendGuardResult {
                authorized: false,
                capability_satisfied: false,
                flow_authorized: false,
                receipt: None,
                effective_capabilities: Some(effective_capabilities),
                metrics: SendGuardMetrics {
                    capability_eval_time_us: cap_time.as_micros() as u64,
                    flow_eval_time_us: 0,
                    total_time_us: total_time.as_micros() as u64,
                    capabilities_checked: 1,
                },
                denial_reason: Some(format!(
                    "Missing required capability: {:?}",
                    self.message_capability
                )),
            });
        }

        // Phase 2: FlowGuard - Evaluate headroom(ctx, cost) and charge budget
        let flow_start = Instant::now();
        let flow_result = self.evaluate_flow_guard(effect_system).await;
        let flow_time = flow_start.elapsed();

        let (flow_authorized, receipt) = match flow_result {
            Ok(receipt) => {
                debug!(
                    operation_id = operation_id,
                    "Flow budget charged successfully"
                );
                (true, Some(receipt))
            }
            Err(err) => {
                warn!(
                    operation_id = operation_id,
                    error = %err,
                    "Send denied: flow budget charge failed"
                );
                (false, None)
            }
        };

        let total_time = start_time.elapsed();
        let authorized = capability_satisfied && flow_authorized;

        if authorized {
            info!(
                operation_id = operation_id,
                peer = ?self.peer,
                cost = self.cost,
                total_time_us = total_time.as_micros(),
                "Send authorized by complete guard chain"
            );
        } else {
            warn!(
                operation_id = operation_id,
                capability_ok = capability_satisfied,
                flow_ok = flow_authorized,
                "Send denied by guard chain"
            );
        }

        Ok(SendGuardResult {
            authorized,
            capability_satisfied,
            flow_authorized,
            receipt,
            effective_capabilities: Some(effective_capabilities),
            metrics: SendGuardMetrics {
                capability_eval_time_us: cap_time.as_micros() as u64,
                flow_eval_time_us: flow_time.as_micros() as u64,
                total_time_us: total_time.as_micros() as u64,
                capabilities_checked: 1,
            },
            denial_reason: if authorized {
                None
            } else {
                Some(self.build_denial_reason(capability_satisfied, flow_authorized))
            },
        })
    }

    /// Evaluate the capability guard: need(m) ≤ Caps(ctx)
    async fn evaluate_capability_guard(
        &self,
        effect_system: &AuraEffectSystem,
    ) -> AuraResult<(bool, EffectiveCapabilitySet)> {
        let guard_evaluator =
            crate::guards::evaluation::create_guard_evaluator(effect_system).await?;

        // Create a protocol guard for capability evaluation
        let protocol_guard = ProtocolGuard::new(
            self.operation_id
                .clone()
                .unwrap_or_else(|| "send_capability_check".to_string()),
        )
        .require_capability(self.message_capability.clone());

        let eval_result = guard_evaluator
            .evaluate_guards(&protocol_guard, effect_system)
            .await?;

        Ok((eval_result.passed, eval_result.effective_capabilities))
    }

    /// Evaluate the flow guard: headroom(ctx, cost)
    async fn evaluate_flow_guard(&self, effect_system: &AuraEffectSystem) -> AuraResult<Receipt> {
        let flow_guard = FlowGuard::new(self.context.clone(), self.peer, self.cost);
        flow_guard.authorize(effect_system).await
    }

    /// Build human-readable denial reason
    fn build_denial_reason(&self, capability_ok: bool, flow_ok: bool) -> String {
        match (capability_ok, flow_ok) {
            (false, false) => format!(
                "Missing capability {:?} and insufficient flow budget (cost: {})",
                self.message_capability, self.cost
            ),
            (false, true) => format!("Missing required capability: {:?}", self.message_capability),
            (true, false) => format!("Insufficient flow budget for cost: {}", self.cost),
            (true, true) => "Send authorized".to_string(), // Should not happen
        }
    }

    /// Convenience method to evaluate and return only the authorization decision
    pub async fn is_send_authorized(&self, effect_system: &AuraEffectSystem) -> AuraResult<bool> {
        let result = self.evaluate(effect_system).await?;
        Ok(result.authorized)
    }

    /// Convenience method to evaluate and return the receipt if authorized
    pub async fn authorize_send(
        &self,
        effect_system: &AuraEffectSystem,
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
}

/// Create a send guard chain for a message send
///
/// This is the primary interface for protocol implementations to use the guard chain.
///
/// # Example
/// ```rust,ignore
/// use aura_protocol::guards::send_guard::create_send_guard;
/// use aura_wot::Capability;
///
/// let guard = create_send_guard(
///     Capability::send_message(),
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
    message_capability: Capability,
    context: ContextId,
    peer: DeviceId,
    cost: u32,
) -> SendGuardChain {
    SendGuardChain::new(message_capability, context, peer, cost)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::AccountId;
    use aura_wot::Capability;

    #[tokio::test]
    async fn test_send_guard_chain_creation() {
        let capability = Capability::Execute {
            operation: "message:send".to_string(),
        };
        let context = ContextId::new(AccountId::from_bytes([1u8; 32]).to_string());
        let peer = DeviceId::from_bytes([2u8; 32]);
        let cost = 100;

        let guard = SendGuardChain::new(capability.clone(), context.clone(), peer, cost)
            .with_operation_id("test_send");

        assert_eq!(guard.message_capability, capability);
        assert_eq!(guard.context, context);
        assert_eq!(guard.peer, peer);
        assert_eq!(guard.cost, cost);
        assert_eq!(guard.operation_id.as_deref(), Some("test_send"));
    }

    #[tokio::test]
    async fn test_create_send_guard_convenience() {
        let capability = Capability::Execute {
            operation: "message:send".to_string(),
        };
        let context = ContextId::new(AccountId::from_bytes([1u8; 32]).to_string());
        let peer = DeviceId::from_bytes([2u8; 32]);
        let cost = 50;

        let guard = create_send_guard(capability.clone(), context.clone(), peer, cost);

        assert_eq!(guard.message_capability, capability);
        assert_eq!(guard.context, context);
        assert_eq!(guard.peer, peer);
        assert_eq!(guard.cost, cost);
    }

    #[tokio::test]
    async fn test_denial_reason_formatting() {
        let capability = Capability::Execute {
            operation: "message:send".to_string(),
        };
        let context = ContextId::new(AccountId::from_bytes([1u8; 32]).to_string());
        let peer = DeviceId::from_bytes([2u8; 32]);
        let guard = SendGuardChain::new(capability.clone(), context, peer, 100);

        // Test capability failure only
        let reason = guard.build_denial_reason(false, true);
        assert!(reason.contains("Missing required capability"));

        // Test flow failure only
        let reason = guard.build_denial_reason(true, false);
        assert!(reason.contains("Insufficient flow budget"));

        // Test both failures
        let reason = guard.build_denial_reason(false, false);
        assert!(reason.contains("Missing capability"));
        assert!(reason.contains("insufficient flow budget"));
    }
}
