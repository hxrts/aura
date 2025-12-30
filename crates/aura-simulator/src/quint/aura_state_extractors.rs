//! Aura State Extractors for Quint Integration
//!
//! This module provides extractors that convert Aura runtime state into
//! Quint-compatible JSON format for property evaluation.
//!
//! # State Types Supported
//!
//! - `TreeState` → Quint tree representation (epoch, leaves, branches)
//! - `AuthorityState` → Quint authority representation
//! - `FlowBudget` → Quint budget representation
//! - Simulation world state → Combined Quint state

use aura_core::effects::QuintMappable;
use aura_core::types::{AuthorityId, ContextId, Epoch, FlowBudget};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

// =============================================================================
// Tree State Extraction
// =============================================================================

/// Extract TreeState into Quint-compatible JSON
///
/// Maps to Quint structure:
/// ```quint
/// type TreeState = {
///     epoch: Epoch,
///     root_commitment: str,
///     leaf_count: int,
///     branch_count: int,
///     leaves: Set[LeafId]
/// }
/// ```
pub fn extract_tree_state(
    epoch: u64,
    root_commitment: &[u8; 32],
    leaf_count: usize,
    branch_count: usize,
    leaf_ids: &[String],
) -> Value {
    json!({
        "epoch": epoch,
        "root_commitment": hex::encode(root_commitment),
        "leaf_count": leaf_count,
        "branch_count": branch_count,
        "leaves": leaf_ids
    })
}

/// Simplified tree state for basic property checking
pub fn extract_tree_state_simple(epoch: u64, root_commitment: &[u8; 32]) -> Value {
    json!({
        "epoch": epoch,
        "root_commitment": hex::encode(root_commitment)
    })
}

// =============================================================================
// Authority State Extraction
// =============================================================================

/// Extract authority state into Quint-compatible JSON
///
/// Maps to Quint structure:
/// ```quint
/// type AuthorityState = {
///     authority_id: AuthorityId,
///     tree_epoch: Epoch,
///     has_signing_key: bool
/// }
/// ```
pub fn extract_authority_state(
    authority_id: Option<&AuthorityId>,
    tree_epoch: u64,
    has_signing_key: bool,
) -> Value {
    json!({
        "authority_id": authority_id.map(|a| a.to_string()),
        "tree_epoch": tree_epoch,
        "has_signing_key": has_signing_key
    })
}

// =============================================================================
// Flow Budget State Extraction
// =============================================================================

/// Extract flow budgets map into Quint-compatible JSON
///
/// Maps to Quint structure:
/// ```quint
/// type BudgetsState = ContextId -> FlowBudget
/// ```
pub fn extract_budgets_state(budgets: &HashMap<ContextId, FlowBudget>) -> Value {
    let mut map = Map::new();
    for (ctx, budget) in budgets {
        map.insert(ctx.to_string(), budget.to_quint());
    }
    Value::Object(map)
}

/// Extract a single flow budget
pub fn extract_flow_budget(budget: &FlowBudget) -> Value {
    budget.to_quint()
}

// =============================================================================
// Token State Extraction
// =============================================================================

/// Capability token for Quint
#[derive(Debug, Clone)]
pub struct CapabilityToken {
    pub cap_level: i64,
    pub attenuation_count: i64,
}

impl CapabilityToken {
    pub fn new(cap_level: i64) -> Self {
        Self {
            cap_level,
            attenuation_count: 0,
        }
    }

    pub fn to_quint(&self) -> Value {
        json!({
            "cap_level": self.cap_level,
            "attenuation_count": self.attenuation_count
        })
    }

    pub fn from_quint(value: &Value) -> Option<Self> {
        let cap_level = value.get("cap_level")?.as_i64()?;
        let attenuation_count = value.get("attenuation_count")?.as_i64().unwrap_or(0);
        Some(Self {
            cap_level,
            attenuation_count,
        })
    }
}

/// Extract tokens map into Quint-compatible JSON
pub fn extract_tokens_state(tokens: &HashMap<AuthorityId, CapabilityToken>) -> Value {
    let mut map = Map::new();
    for (auth, token) in tokens {
        map.insert(auth.to_string(), token.to_quint());
    }
    Value::Object(map)
}

// =============================================================================
// Transport Operation Extraction
// =============================================================================

/// Transport operation record for Quint
#[derive(Debug, Clone)]
pub struct TransportOpRecord {
    pub context_id: ContextId,
    pub source: AuthorityId,
    pub dest: AuthorityId,
    pub cost: i64,
    pub epoch: u64,
    pub guard_steps_completed: Vec<String>,
    pub charged: bool,
}

impl TransportOpRecord {
    pub fn to_quint(&self) -> Value {
        json!({
            "context_id": self.context_id.to_string(),
            "source": self.source.to_string(),
            "dest": self.dest.to_string(),
            "cost": self.cost,
            "epoch": self.epoch,
            "guard_steps_completed": self.guard_steps_completed,
            "charged": self.charged
        })
    }
}

/// Extract completed operations list into Quint-compatible JSON
pub fn extract_completed_ops(ops: &[TransportOpRecord]) -> Value {
    Value::Array(ops.iter().map(|op| op.to_quint()).collect())
}

// =============================================================================
// Combined Simulation State
// =============================================================================

/// Combined simulation state for Quint property evaluation
///
/// This struct aggregates all state needed for evaluating capability properties.
#[derive(Debug, Clone, Default)]
pub struct QuintSimulationState {
    /// Flow budgets by context
    pub budgets: HashMap<ContextId, FlowBudget>,
    /// Capability tokens by authority
    pub tokens: HashMap<AuthorityId, CapabilityToken>,
    /// Current epoch by context
    pub current_epoch: HashMap<ContextId, u64>,
    /// Completed transport operations
    pub completed_ops: Vec<TransportOpRecord>,
}

impl QuintSimulationState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert entire simulation state to Quint JSON
    pub fn to_quint(&self) -> Value {
        json!({
            "budgets": extract_budgets_state(&self.budgets),
            "tokens": extract_tokens_state(&self.tokens),
            "current_epoch": self.extract_epochs(),
            "completed_ops": extract_completed_ops(&self.completed_ops)
        })
    }

    fn extract_epochs(&self) -> Value {
        let mut map = Map::new();
        for (ctx, epoch) in &self.current_epoch {
            map.insert(ctx.to_string(), json!(epoch));
        }
        Value::Object(map)
    }

    /// Update state from Quint JSON (for non-deterministic picks)
    pub fn update_from_quint(&mut self, quint_state: &Value) -> Result<(), String> {
        // Update budgets
        if let Some(budgets_obj) = quint_state.get("budgets").and_then(|v| v.as_object()) {
            for (ctx_str, budget_val) in budgets_obj {
                let ctx: ContextId = ctx_str
                    .parse()
                    .map_err(|e| format!("invalid context id: {e}"))?;
                let budget = FlowBudget::from_quint(budget_val)
                    .map_err(|e| format!("invalid budget: {e}"))?;
                self.budgets.insert(ctx, budget);
            }
        }

        // Update tokens
        if let Some(tokens_obj) = quint_state.get("tokens").and_then(|v| v.as_object()) {
            for (auth_str, token_val) in tokens_obj {
                let auth: AuthorityId = auth_str
                    .parse()
                    .map_err(|e| format!("invalid authority id: {e}"))?;
                let token = CapabilityToken::from_quint(token_val)
                    .ok_or_else(|| "invalid token".to_string())?;
                self.tokens.insert(auth, token);
            }
        }

        // Update epochs
        if let Some(epochs_obj) = quint_state.get("current_epoch").and_then(|v| v.as_object()) {
            for (ctx_str, epoch_val) in epochs_obj {
                let ctx: ContextId = ctx_str
                    .parse()
                    .map_err(|e| format!("invalid context id: {e}"))?;
                let epoch = epoch_val
                    .as_u64()
                    .ok_or_else(|| "invalid epoch".to_string())?;
                self.current_epoch.insert(ctx, epoch);
            }
        }

        Ok(())
    }

    /// Initialize a new context
    pub fn init_context(&mut self, ctx: ContextId, _peer: AuthorityId, limit: u64) {
        self.current_epoch.insert(ctx, 0);
        self.budgets.insert(
            ctx,
            FlowBudget {
                limit,
                spent: 0,
                epoch: Epoch::new(0),
            },
        );
    }

    /// Initialize a new authority
    pub fn init_authority(&mut self, auth: AuthorityId, cap_level: i64) {
        self.tokens.insert(auth, CapabilityToken::new(cap_level));
    }

    /// Record a completed transport operation
    pub fn complete_transport_op(
        &mut self,
        ctx: &ContextId,
        src: &AuthorityId,
        dst: &AuthorityId,
        cost: i64,
    ) -> Result<(), String> {
        // Check budget
        let budget = self
            .budgets
            .get_mut(ctx)
            .ok_or_else(|| format!("context {ctx} not found"))?;

        if budget.spent + cost as u64 > budget.limit {
            return Err(format!(
                "budget exceeded: {} + {} > {}",
                budget.spent, cost, budget.limit
            ));
        }

        let epoch = budget.epoch.value();
        budget.spent += cost as u64;

        // Record operation
        self.completed_ops.push(TransportOpRecord {
            context_id: *ctx,
            source: *src,
            dest: *dst,
            cost,
            epoch,
            guard_steps_completed: vec![
                "CapGuard".to_string(),
                "FlowGuard".to_string(),
                "JournalCoupler".to_string(),
                "TransportSend".to_string(),
            ],
            charged: true,
        });

        Ok(())
    }

    /// Attenuate a token
    pub fn attenuate_token(&mut self, auth: &AuthorityId, new_cap: i64) -> Result<(), String> {
        let token = self
            .tokens
            .get_mut(auth)
            .ok_or_else(|| format!("authority {auth} not found"))?;

        if new_cap > token.cap_level {
            return Err(format!(
                "cannot widen capability: {} > {}",
                new_cap, token.cap_level
            ));
        }

        token.cap_level = new_cap;
        token.attenuation_count += 1;
        Ok(())
    }
}

// =============================================================================
// State Extractor Trait
// =============================================================================

/// Trait for types that can be extracted as Quint state
pub trait QuintStateExtractor {
    /// Extract state as Quint-compatible JSON
    fn extract_quint_state(&self) -> Value;

    /// Get the variable name for this state in Quint
    fn quint_variable_name(&self) -> &'static str;
}

impl QuintStateExtractor for QuintSimulationState {
    fn extract_quint_state(&self) -> Value {
        self.to_quint()
    }

    fn quint_variable_name(&self) -> &'static str {
        "simulation_state"
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context() -> ContextId {
        ContextId::new_from_entropy([1u8; 32])
    }

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    #[test]
    fn test_extract_tree_state() {
        let state = extract_tree_state(5, &[42u8; 32], 10, 3, &["leaf1".to_string()]);

        assert_eq!(state["epoch"], 5);
        assert_eq!(state["leaf_count"], 10);
        assert_eq!(state["branch_count"], 3);
    }

    #[test]
    fn test_extract_authority_state() {
        let auth = test_authority();
        let state = extract_authority_state(Some(&auth), 10, true);

        assert_eq!(state["tree_epoch"], 10);
        assert_eq!(state["has_signing_key"], true);
        assert!(state["authority_id"].is_string());
    }

    #[test]
    fn test_capability_token_roundtrip() {
        let token = CapabilityToken {
            cap_level: 3,
            attenuation_count: 2,
        };

        let quint = token.to_quint();
        let restored = CapabilityToken::from_quint(&quint).unwrap();

        assert_eq!(restored.cap_level, 3);
        assert_eq!(restored.attenuation_count, 2);
    }

    #[test]
    fn test_simulation_state_to_quint() {
        let mut state = QuintSimulationState::new();
        let ctx = test_context();
        let auth = test_authority();

        state.init_context(ctx, auth, 100);
        state.init_authority(auth, 4);

        let quint = state.to_quint();

        assert!(quint["budgets"].is_object());
        assert!(quint["tokens"].is_object());
        assert!(quint["current_epoch"].is_object());
        assert!(quint["completed_ops"].is_array());
    }

    #[test]
    fn test_simulation_state_operations() {
        let mut state = QuintSimulationState::new();
        let ctx = test_context();
        let auth1 = AuthorityId::new_from_entropy([1u8; 32]);
        let auth2 = AuthorityId::new_from_entropy([2u8; 32]);

        state.init_context(ctx, auth1, 100);
        state.init_authority(auth1, 4);

        // Complete a transport op
        let result = state.complete_transport_op(&ctx, &auth1, &auth2, 10);
        assert!(result.is_ok());

        // Check budget was updated
        assert_eq!(state.budgets.get(&ctx).unwrap().spent, 10);

        // Check op was recorded
        assert_eq!(state.completed_ops.len(), 1);
    }

    #[test]
    fn test_simulation_state_budget_exceeded() {
        let mut state = QuintSimulationState::new();
        let ctx = test_context();
        let auth1 = AuthorityId::new_from_entropy([1u8; 32]);
        let auth2 = AuthorityId::new_from_entropy([2u8; 32]);

        state.init_context(ctx, auth1, 10);
        state.init_authority(auth1, 4);

        // Try to exceed budget
        let result = state.complete_transport_op(&ctx, &auth1, &auth2, 20);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("budget exceeded"));
    }

    #[test]
    fn test_attenuate_token() {
        let mut state = QuintSimulationState::new();
        let auth = test_authority();

        state.init_authority(auth, 4); // CAP_FULL

        // Attenuate to CAP_READ
        let result = state.attenuate_token(&auth, 3);
        assert!(result.is_ok());

        let token = state.tokens.get(&auth).unwrap();
        assert_eq!(token.cap_level, 3);
        assert_eq!(token.attenuation_count, 1);

        // Cannot widen
        let result = state.attenuate_token(&auth, 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_simulation_state_update_from_quint() {
        let mut state = QuintSimulationState::new();
        let ctx = test_context();

        state.init_context(ctx, test_authority(), 100);

        // Simulate Quint updating the state
        let quint_update = json!({
            "budgets": {
                ctx.to_string(): {
                    "limit": 100,
                    "spent": 50,
                    "epoch": 1
                }
            }
        });

        let result = state.update_from_quint(&quint_update);
        assert!(result.is_ok());

        // Check budget was updated
        assert_eq!(state.budgets.get(&ctx).unwrap().spent, 50);
    }
}
