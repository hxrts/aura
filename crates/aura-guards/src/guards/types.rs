//! Feature-crate guard primitives
//!
//! Layer 5 feature crates often need protocol-specific `EffectCommand` enums, but they
//! should not duplicate the *generic* guard result vocabulary (allow/deny + commands).
//!
//! This module provides shared guard decision/outcome types and basic capability/budget
//! checks that are generic over a feature crate's command enum.

use aura_core::FlowCost;
use serde::{Deserialize, Serialize};

/// Typed identifier for guard capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityId(String);

impl CapabilityId {
    /// Create a new capability identifier.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Get the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for CapabilityId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for CapabilityId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl std::fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Structured guard violation reasons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardViolation {
    MissingCapability { capability: CapabilityId },
    InsufficientFlowBudget { required: FlowCost, remaining: FlowCost },
    AuthorizationDenied,
    MissingAuthorizationDecision,
    CapabilityCheckFailed,
    ChargeAfterSend,
    MissingChargeBeforeSend,
    Other(String),
}

impl GuardViolation {
    pub fn other(reason: impl Into<String>) -> Self {
        Self::Other(reason.into())
    }
}

impl std::fmt::Display for GuardViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GuardViolation::MissingCapability { capability } => {
                write!(f, "Missing capability: {capability}")
            }
            GuardViolation::InsufficientFlowBudget { required, remaining } => write!(
                f,
                "Insufficient flow budget: need {required}, have {remaining}"
            ),
            GuardViolation::AuthorizationDenied => write!(f, "Authorization denied"),
            GuardViolation::MissingAuthorizationDecision => {
                write!(f, "Missing authorization decision")
            }
            GuardViolation::CapabilityCheckFailed => write!(f, "Capability check failed"),
            GuardViolation::ChargeAfterSend => {
                write!(f, "charge command appears after a send command")
            }
            GuardViolation::MissingChargeBeforeSend => {
                write!(f, "send command emitted without any preceding charge command")
            }
            GuardViolation::Other(reason) => write!(f, "{reason}"),
        }
    }
}

/// Decision from guard evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardDecision {
    /// Operation is allowed.
    Allow,
    /// Operation is denied with a reason.
    Deny { reason: GuardViolation },
}

impl GuardDecision {
    /// Create an allow decision.
    pub fn allow() -> Self {
        Self::Allow
    }

    /// Create a deny decision with a reason.
    pub fn deny(reason: GuardViolation) -> Self {
        Self::Deny { reason }
    }

    /// Returns `true` if the decision allows the operation.
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }

    /// Returns `true` if the decision denies the operation.
    pub fn is_denied(&self) -> bool {
        !self.is_allowed()
    }

    /// Returns the denial reason, if denied.
    pub fn denial_reason(&self) -> Option<&GuardViolation> {
        match self {
            Self::Allow => None,
            Self::Deny { reason } => Some(reason),
        }
    }
}

/// Outcome of guard evaluation.
///
/// The effect command type is feature-crate specific (e.g. authentication commands,
/// invitation commands, rendezvous commands).
#[derive(Debug, Clone)]
pub struct GuardOutcome<C> {
    /// Decision (allow/deny).
    pub decision: GuardDecision,
    /// Effect commands to execute if allowed.
    pub effects: Vec<C>,
}

impl<C> GuardOutcome<C> {
    /// Create an allowed outcome.
    pub fn allowed(effects: Vec<C>) -> Self {
        Self {
            decision: GuardDecision::Allow,
            effects,
        }
    }

    /// Create a denied outcome with no effects.
    pub fn denied(reason: GuardViolation) -> Self {
        Self {
            decision: GuardDecision::Deny { reason },
            effects: Vec::new(),
        }
    }

    /// Returns `true` if allowed.
    pub fn is_allowed(&self) -> bool {
        self.decision.is_allowed()
    }

    /// Returns `true` if denied.
    pub fn is_denied(&self) -> bool {
        self.decision.is_denied()
    }
}

/// Minimal capability query contract required by `check_capability`.
pub trait CapabilitySnapshot {
    /// Returns `true` if the snapshot contains `cap`.
    fn has_capability(&self, cap: &CapabilityId) -> bool;
}

/// Minimal budget query contract required by `check_flow_budget`.
pub trait FlowBudgetSnapshot {
    /// Remaining flow budget in the current context.
    fn flow_budget_remaining(&self) -> FlowCost;
}

/// Check capability and return denied outcome if missing.
pub fn check_capability<S, C>(
    snapshot: &S,
    required_cap: &CapabilityId,
) -> Option<GuardOutcome<C>>
where
    S: CapabilitySnapshot,
{
    if snapshot.has_capability(required_cap) {
        None
    } else {
        Some(GuardOutcome::denied(GuardViolation::MissingCapability {
            capability: required_cap.clone(),
        }))
    }
}

/// Check flow budget and return denied outcome if insufficient.
pub fn check_flow_budget<S, C>(snapshot: &S, required_cost: FlowCost) -> Option<GuardOutcome<C>>
where
    S: FlowBudgetSnapshot,
{
    let remaining = snapshot.flow_budget_remaining();
    if remaining >= required_cost {
        None
    } else {
        Some(GuardOutcome::denied(GuardViolation::InsufficientFlowBudget {
            required: required_cost,
            remaining,
        }))
    }
}

/// Validate that all "charge" commands occur before any "send" commands.
///
/// This is intentionally generic: each feature crate defines its own `EffectCommand` enum,
/// so callers provide classifiers for which commands are "charge" vs "send".
pub fn validate_charge_before_send<C>(
    cmds: &[C],
    is_charge: impl Fn(&C) -> bool,
    is_send: impl Fn(&C) -> bool,
) -> Result<(), GuardViolation> {
    let mut saw_send = false;
    let mut saw_charge = false;

    for cmd in cmds {
        if is_send(cmd) {
            saw_send = true;
        }
        if is_charge(cmd) {
            saw_charge = true;
            if saw_send {
                return Err(GuardViolation::ChargeAfterSend);
            }
        }
    }

    if saw_send && !saw_charge {
        return Err(GuardViolation::MissingChargeBeforeSend);
    }

    Ok(())
}

/// Typed guard operation identifiers to avoid stringly-typed call sites.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GuardOperation {
    AmpSend,
    SyncRequestDigest,
    SyncRequestOps,
    SyncPushOps,
    SyncAnnounceOp,
    SyncPushOp,
    Custom(String),
}

impl GuardOperation {
    pub fn as_str(&self) -> &str {
        match self {
            GuardOperation::AmpSend => "amp:send",
            GuardOperation::SyncRequestDigest => "sync:request_digest",
            GuardOperation::SyncRequestOps => "sync:request_ops",
            GuardOperation::SyncPushOps => "sync:push_ops",
            GuardOperation::SyncAnnounceOp => "sync:announce_op",
            GuardOperation::SyncPushOp => "sync:push_op",
            GuardOperation::Custom(value) => value.as_str(),
        }
    }
}

/// Typed operation identifiers for logging/metrics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GuardOperationId {
    AmpSend,
    SyncRequestDigest {
        peer: uuid::Uuid,
    },
    SyncRequestOps {
        peer: uuid::Uuid,
        count: u32,
    },
    SyncAnnounceOp {
        peer: uuid::Uuid,
        cid: aura_core::Hash32,
    },
    SyncPushOp {
        peer: uuid::Uuid,
        cid: aura_core::Hash32,
    },
    Custom(String),
}

impl std::fmt::Display for GuardOperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GuardOperationId::AmpSend => write!(f, "amp_send"),
            GuardOperationId::SyncRequestDigest { peer } => write!(f, "digest_request_{peer}"),
            GuardOperationId::SyncRequestOps { peer, count } => {
                write!(f, "ops_request_{peer}_{count}")
            }
            GuardOperationId::SyncAnnounceOp { peer, cid } => {
                write!(f, "announce_{peer}_{cid:?}")
            }
            GuardOperationId::SyncPushOp { peer, cid } => {
                write!(f, "push_op_{peer}_{cid:?}")
            }
            GuardOperationId::Custom(value) => write!(f, "{value}"),
        }
    }
}

impl GuardOperationId {
    pub fn is_empty(&self) -> bool {
        matches!(self, GuardOperationId::Custom(value) if value.is_empty())
    }
}

impl From<String> for GuardOperationId {
    fn from(value: String) -> Self {
        GuardOperationId::Custom(value)
    }
}

impl From<&str> for GuardOperationId {
    fn from(value: &str) -> Self {
        GuardOperationId::Custom(value.to_string())
    }
}

impl From<GuardOperationId> for String {
    fn from(value: GuardOperationId) -> Self {
        value.to_string()
    }
}

impl From<GuardOperation> for String {
    fn from(operation: GuardOperation) -> Self {
        operation.as_str().to_string()
    }
}

impl From<&str> for GuardOperation {
    fn from(value: &str) -> Self {
        GuardOperation::Custom(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    enum Cmd {
        Charge,
        Send,
        Other,
    }

    #[test]
    fn validate_charge_before_send_accepts_ordered() {
        let cmds = [Cmd::Charge, Cmd::Other, Cmd::Send];
        match validate_charge_before_send(
            &cmds,
            |c| matches!(c, Cmd::Charge),
            |c| matches!(c, Cmd::Send),
        ) {
            Ok(()) => {}
            Err(err) => panic!("expected ok: {err}"),
        }
    }

    #[test]
    fn validate_charge_before_send_rejects_missing_charge() {
        let cmds = [Cmd::Other, Cmd::Send];
        let err = match validate_charge_before_send(
            &cmds,
            |c| matches!(c, Cmd::Charge),
            |c| matches!(c, Cmd::Send),
        ) {
            Ok(()) => panic!("expected error"),
            Err(err) => err,
        };
        assert!(matches!(err, GuardViolation::MissingChargeBeforeSend));
    }

    #[test]
    fn validate_charge_before_send_rejects_misordered_charge() {
        let cmds = [Cmd::Send, Cmd::Charge];
        let err = match validate_charge_before_send(
            &cmds,
            |c| matches!(c, Cmd::Charge),
            |c| matches!(c, Cmd::Send),
        ) {
            Ok(()) => panic!("expected error"),
            Err(err) => err,
        };
        assert!(matches!(err, GuardViolation::ChargeAfterSend));
    }
}
