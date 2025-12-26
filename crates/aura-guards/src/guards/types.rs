//! Feature-crate guard primitives
//!
//! Layer 5 feature crates often need protocol-specific `EffectCommand` enums, but they
//! should not duplicate the *generic* guard result vocabulary (allow/deny + commands).
//!
//! This module provides shared guard decision/outcome types and basic capability/budget
//! checks that are generic over a feature crate's command enum.

use serde::{Deserialize, Serialize};

/// Decision from guard evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardDecision {
    /// Operation is allowed.
    Allow,
    /// Operation is denied with a reason.
    Deny { reason: String },
}

impl GuardDecision {
    /// Create an allow decision.
    pub fn allow() -> Self {
        Self::Allow
    }

    /// Create a deny decision with a reason.
    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
        }
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
    pub fn denial_reason(&self) -> Option<&str> {
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
    pub fn denied(reason: impl Into<String>) -> Self {
        Self {
            decision: GuardDecision::Deny {
                reason: reason.into(),
            },
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
    fn has_capability(&self, cap: &str) -> bool;
}

/// Minimal budget query contract required by `check_flow_budget`.
pub trait FlowBudgetSnapshot {
    /// Remaining flow budget in the current context.
    fn flow_budget_remaining(&self) -> u32;
}

/// Check capability and return denied outcome if missing.
pub fn check_capability<S, C>(snapshot: &S, required_cap: &str) -> Option<GuardOutcome<C>>
where
    S: CapabilitySnapshot,
{
    if snapshot.has_capability(required_cap) {
        None
    } else {
        Some(GuardOutcome::denied(format!(
            "Missing capability: {required_cap}"
        )))
    }
}

/// Check flow budget and return denied outcome if insufficient.
pub fn check_flow_budget<S, C>(snapshot: &S, required_cost: u32) -> Option<GuardOutcome<C>>
where
    S: FlowBudgetSnapshot,
{
    let remaining = snapshot.flow_budget_remaining();
    if remaining >= required_cost {
        None
    } else {
        Some(GuardOutcome::denied(format!(
            "Insufficient flow budget: need {required_cost}, have {remaining}"
        )))
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
) -> Result<(), String> {
    let mut saw_send = false;
    let mut saw_charge = false;

    for cmd in cmds {
        if is_send(cmd) {
            saw_send = true;
        }
        if is_charge(cmd) {
            saw_charge = true;
            if saw_send {
                return Err("charge command appears after a send command".to_string());
            }
        }
    }

    if saw_send && !saw_charge {
        return Err("send command emitted without any preceding charge command".to_string());
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
    SyncRequestDigest { peer: uuid::Uuid },
    SyncRequestOps { peer: uuid::Uuid, count: usize },
    SyncAnnounceOp { peer: uuid::Uuid, cid: aura_core::Hash32 },
    SyncPushOp { peer: uuid::Uuid, cid: aura_core::Hash32 },
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
        validate_charge_before_send(
            &cmds,
            |c| matches!(c, Cmd::Charge),
            |c| matches!(c, Cmd::Send),
        )
        .unwrap();
    }

    #[test]
    fn validate_charge_before_send_rejects_missing_charge() {
        let cmds = [Cmd::Other, Cmd::Send];
        let err = validate_charge_before_send(
            &cmds,
            |c| matches!(c, Cmd::Charge),
            |c| matches!(c, Cmd::Send),
        )
        .unwrap_err();
        assert!(err.contains("without any preceding charge"));
    }

    #[test]
    fn validate_charge_before_send_rejects_misordered_charge() {
        let cmds = [Cmd::Send, Cmd::Charge];
        let err = validate_charge_before_send(
            &cmds,
            |c| matches!(c, Cmd::Charge),
            |c| matches!(c, Cmd::Send),
        )
        .unwrap_err();
        assert!(err.contains("after a send"));
    }
}
