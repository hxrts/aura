//! Domain-Specific Action Handlers for Capability Properties
//!
//! This module implements action handlers for the `protocol_capability_properties.qnt`
//! reference specification, enabling generative simulation of Aura's core
//! security properties.
//!
//! # Actions Implemented
//!
//! - `initContext`: Initialize a relational context with flow budget
//! - `initAuthority`: Initialize an authority with capability token
//! - `completeTransportOp`: Complete a transport operation through guard chain
//! - `attenuateToken`: Attenuate a capability to narrower permissions

use super::action_registry::ActionHandler;
use async_trait::async_trait;
use aura_core::effects::{ActionEffect, ActionResult};
use aura_core::Result;
use serde_json::{json, Value};
use std::collections::HashMap;

// =============================================================================
// State Keys
// =============================================================================

/// State variable names matching Quint spec
mod state_keys {
    pub const BUDGETS: &str = "budgets";
    pub const COMPLETED_OPS: &str = "completed_ops";
    pub const CURRENT_EPOCH: &str = "current_epoch";
    pub const TOKENS: &str = "tokens";
}

/// Capability levels matching Quint spec
mod cap_levels {
    pub const NONE: i64 = 0;
    pub const EXECUTE: i64 = 1;
    pub const WRITE: i64 = 2;
    pub const READ: i64 = 3;
    pub const FULL: i64 = 4;
}

// =============================================================================
// InitContextAction
// =============================================================================

/// Handler for `initContext(ctx, peer, limit)` action
///
/// Initializes a new relational context with:
/// - A context ID
/// - A peer authority
/// - An initial flow budget limit
///
/// # Preconditions
/// - Context must not already exist in `current_epoch`
///
/// # Effects
/// - Adds context to `current_epoch` with epoch 0
/// - Creates a `FlowBudget` entry in `budgets`
pub struct InitContextHandler;

#[async_trait]
impl ActionHandler for InitContextHandler {
    fn action_name(&self) -> &str {
        "initContext"
    }

    fn parameter_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ctx": {"type": "string", "description": "Context ID"},
                "peer": {"type": "string", "description": "Peer authority ID"},
                "limit": {"type": "integer", "minimum": 0, "description": "Flow budget limit"}
            },
            "required": ["ctx", "peer", "limit"]
        })
    }

    fn description(&self) -> &str {
        "Initialize a relational context with flow budget"
    }

    fn is_enabled(&self, state: &Value) -> bool {
        // Could check preconditions here, but we'll let execute handle it
        state.is_object()
    }

    async fn execute(
        &self,
        params: &Value,
        _nondet_picks: &HashMap<String, Value>,
        state: &Value,
    ) -> Result<ActionResult> {
        let ctx = params
            .get("ctx")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing 'ctx' parameter"))?;

        let peer = params
            .get("peer")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing 'peer' parameter"))?;

        let limit = params
            .get("limit")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| aura_core::AuraError::invalid("missing 'limit' parameter"))?;

        // Clone state as mutable
        let mut new_state = state.clone();
        let state_obj = new_state
            .as_object_mut()
            .ok_or_else(|| aura_core::AuraError::invalid("state must be an object"))?;

        // Check precondition: context must not exist
        let current_epoch = state_obj
            .entry(state_keys::CURRENT_EPOCH)
            .or_insert_with(|| json!({}));

        if current_epoch.get(ctx).is_some() {
            return Ok(ActionResult {
                success: false,
                resulting_state: state.clone(),
                effects_produced: vec![],
                error: Some(format!("context '{ctx}' already exists")),
            });
        }

        // Update current_epoch
        current_epoch
            .as_object_mut()
            .unwrap()
            .insert(ctx.to_string(), json!(0));

        // Create flow budget
        let budget = json!({
            "context_id": ctx,
            "peer": peer,
            "epoch": 0,
            "spent": 0,
            "limit": limit
        });

        // Update budgets
        let budgets = state_obj
            .entry(state_keys::BUDGETS)
            .or_insert_with(|| json!({}));
        budgets
            .as_object_mut()
            .unwrap()
            .insert(ctx.to_string(), budget);

        Ok(ActionResult {
            success: true,
            resulting_state: new_state,
            effects_produced: vec![ActionEffect {
                effect_type: "context_init".to_string(),
                parameters: json!({
                    "context_id": ctx,
                    "peer": peer,
                    "limit": limit
                }),
            }],
            error: None,
        })
    }
}

// =============================================================================
// InitAuthorityAction
// =============================================================================

/// Handler for `initAuthority(auth, cap)` action
///
/// Initializes a new authority with:
/// - An authority ID
/// - An initial capability level
///
/// # Preconditions
/// - Authority must not already exist in `tokens`
///
/// # Effects
/// - Creates a `BiscuitToken` entry in `tokens`
pub struct InitAuthorityHandler;

#[async_trait]
impl ActionHandler for InitAuthorityHandler {
    fn action_name(&self) -> &str {
        "initAuthority"
    }

    fn parameter_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "auth": {"type": "string", "description": "Authority ID"},
                "cap": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 4,
                    "description": "Capability level (0=None, 1=Execute, 2=Write, 3=Read, 4=Full)"
                }
            },
            "required": ["auth", "cap"]
        })
    }

    fn description(&self) -> &str {
        "Initialize an authority with capability token"
    }

    fn is_enabled(&self, state: &Value) -> bool {
        state.is_object()
    }

    async fn execute(
        &self,
        params: &Value,
        _nondet_picks: &HashMap<String, Value>,
        state: &Value,
    ) -> Result<ActionResult> {
        let auth = params
            .get("auth")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing 'auth' parameter"))?;

        let cap = params
            .get("cap")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| aura_core::AuraError::invalid("missing 'cap' parameter"))?;

        // Validate capability level
        if !(cap_levels::NONE..=cap_levels::FULL).contains(&cap) {
            return Ok(ActionResult {
                success: false,
                resulting_state: state.clone(),
                effects_produced: vec![],
                error: Some(format!("invalid capability level: {cap}")),
            });
        }

        // Clone state as mutable
        let mut new_state = state.clone();
        let state_obj = new_state
            .as_object_mut()
            .ok_or_else(|| aura_core::AuraError::invalid("state must be an object"))?;

        // Check precondition: authority must not exist
        let tokens = state_obj
            .entry(state_keys::TOKENS)
            .or_insert_with(|| json!({}));

        if tokens.get(auth).is_some() {
            return Ok(ActionResult {
                success: false,
                resulting_state: state.clone(),
                effects_produced: vec![],
                error: Some(format!("authority '{auth}' already exists")),
            });
        }

        // Create token
        let token = json!({
            "cap_level": cap,
            "attenuation_count": 0
        });

        tokens
            .as_object_mut()
            .unwrap()
            .insert(auth.to_string(), token);

        Ok(ActionResult {
            success: true,
            resulting_state: new_state,
            effects_produced: vec![ActionEffect {
                effect_type: "authority_init".to_string(),
                parameters: json!({
                    "authority_id": auth,
                    "capability_level": cap
                }),
            }],
            error: None,
        })
    }
}

// =============================================================================
// CompleteTransportOpAction
// =============================================================================

/// Handler for `completeTransportOp(ctx, src, dst, cost)` action
///
/// Completes a transport operation through the full guard chain:
/// 1. CapGuard - capability check
/// 2. FlowGuard - budget check and charge
/// 3. JournalCoupler - journal commit
/// 4. TransportSend - actual send
///
/// # Preconditions
/// - Context must exist
/// - Source authority must have non-None capability
/// - Budget must have sufficient remaining capacity
///
/// # Effects
/// - Increments spent counter in budget
/// - Adds operation to completed_ops list
pub struct CompleteTransportOpHandler;

#[async_trait]
impl ActionHandler for CompleteTransportOpHandler {
    fn action_name(&self) -> &str {
        "completeTransportOp"
    }

    fn parameter_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ctx": {"type": "string", "description": "Context ID"},
                "src": {"type": "string", "description": "Source authority ID"},
                "dst": {"type": "string", "description": "Destination authority ID"},
                "cost": {"type": "integer", "minimum": 0, "description": "Operation cost"}
            },
            "required": ["ctx", "src", "dst", "cost"]
        })
    }

    fn description(&self) -> &str {
        "Complete a transport operation through guard chain"
    }

    fn is_enabled(&self, state: &Value) -> bool {
        // Basic check - could be more sophisticated
        state.is_object()
    }

    async fn execute(
        &self,
        params: &Value,
        _nondet_picks: &HashMap<String, Value>,
        state: &Value,
    ) -> Result<ActionResult> {
        let ctx = params
            .get("ctx")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing 'ctx' parameter"))?;

        let src = params
            .get("src")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing 'src' parameter"))?;

        let dst = params
            .get("dst")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing 'dst' parameter"))?;

        let cost = params
            .get("cost")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| aura_core::AuraError::invalid("missing 'cost' parameter"))?;

        // Clone state as mutable
        let mut new_state = state.clone();
        let state_obj = new_state
            .as_object_mut()
            .ok_or_else(|| aura_core::AuraError::invalid("state must be an object"))?;

        // Check context exists
        let current_epoch = state_obj.get(state_keys::CURRENT_EPOCH);
        if current_epoch.is_none() || current_epoch.unwrap().get(ctx).is_none() {
            return Ok(ActionResult {
                success: false,
                resulting_state: state.clone(),
                effects_produced: vec![],
                error: Some(format!("context '{ctx}' not found")),
            });
        }

        // Check source authority has capability
        let tokens = state_obj.get(state_keys::TOKENS);
        let src_token = tokens.and_then(|t| t.get(src));
        if src_token.is_none() {
            return Ok(ActionResult {
                success: false,
                resulting_state: state.clone(),
                effects_produced: vec![],
                error: Some(format!("source authority '{src}' not found")),
            });
        }

        let cap_level = src_token
            .unwrap()
            .get("cap_level")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        if cap_level <= cap_levels::NONE {
            return Ok(ActionResult {
                success: false,
                resulting_state: state.clone(),
                effects_produced: vec![],
                error: Some(format!("source authority '{src}' has no capability")),
            });
        }

        // Check budget
        let budget = state_obj.get(state_keys::BUDGETS).and_then(|b| b.get(ctx));

        if budget.is_none() {
            return Ok(ActionResult {
                success: false,
                resulting_state: state.clone(),
                effects_produced: vec![],
                error: Some(format!("budget for context '{ctx}' not found")),
            });
        }

        let budget = budget.unwrap();
        let spent = budget.get("spent").and_then(|v| v.as_i64()).unwrap_or(0);
        let limit = budget.get("limit").and_then(|v| v.as_i64()).unwrap_or(0);
        let epoch = budget.get("epoch").and_then(|v| v.as_i64()).unwrap_or(0);

        if spent + cost > limit {
            return Ok(ActionResult {
                success: false,
                resulting_state: state.clone(),
                effects_produced: vec![],
                error: Some(format!(
                    "budget exceeded: spent={spent} + cost={cost} > limit={limit}"
                )),
            });
        }

        // All checks passed - update state

        // Update budget
        let budgets = state_obj.get_mut(state_keys::BUDGETS).unwrap();
        let budget_obj = budgets.get_mut(ctx).unwrap().as_object_mut().unwrap();
        budget_obj.insert("spent".to_string(), json!(spent + cost));

        // Create operation record
        let operation = json!({
            "context_id": ctx,
            "source": src,
            "dest": dst,
            "cost": cost,
            "epoch": epoch,
            "guard_steps_completed": ["CapGuard", "FlowGuard", "JournalCoupler", "TransportSend"],
            "charged": true
        });

        // Append to completed_ops
        let completed_ops = state_obj
            .entry(state_keys::COMPLETED_OPS)
            .or_insert_with(|| json!([]));
        completed_ops.as_array_mut().unwrap().push(operation);

        Ok(ActionResult {
            success: true,
            resulting_state: new_state,
            effects_produced: vec![
                ActionEffect {
                    effect_type: "cap_guard".to_string(),
                    parameters: json!({"authority": src, "cap_level": cap_level}),
                },
                ActionEffect {
                    effect_type: "flow_guard".to_string(),
                    parameters: json!({"context": ctx, "cost": cost, "new_spent": spent + cost}),
                },
                ActionEffect {
                    effect_type: "journal_commit".to_string(),
                    parameters: json!({"context": ctx, "epoch": epoch}),
                },
                ActionEffect {
                    effect_type: "transport_send".to_string(),
                    parameters: json!({"src": src, "dst": dst, "context": ctx}),
                },
            ],
            error: None,
        })
    }
}

// =============================================================================
// AttenuateTokenAction
// =============================================================================

/// Handler for `attenuateToken(auth, new_cap)` action
///
/// Attenuates a capability token to a narrower permission level.
///
/// # Preconditions
/// - Authority must exist
/// - New capability must be <= current capability
/// - New capability must be >= CAP_NONE
///
/// # Effects
/// - Updates capability level in token
/// - Increments attenuation count
pub struct AttenuateTokenHandler;

#[async_trait]
impl ActionHandler for AttenuateTokenHandler {
    fn action_name(&self) -> &str {
        "attenuateToken"
    }

    fn parameter_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "auth": {"type": "string", "description": "Authority ID"},
                "new_cap": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 4,
                    "description": "New capability level (must be <= current)"
                }
            },
            "required": ["auth", "new_cap"]
        })
    }

    fn description(&self) -> &str {
        "Attenuate a capability token to narrower permissions"
    }

    fn is_enabled(&self, state: &Value) -> bool {
        state.is_object()
    }

    async fn execute(
        &self,
        params: &Value,
        _nondet_picks: &HashMap<String, Value>,
        state: &Value,
    ) -> Result<ActionResult> {
        let auth = params
            .get("auth")
            .and_then(|v| v.as_str())
            .ok_or_else(|| aura_core::AuraError::invalid("missing 'auth' parameter"))?;

        let new_cap = params
            .get("new_cap")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| aura_core::AuraError::invalid("missing 'new_cap' parameter"))?;

        // Clone state as mutable
        let mut new_state = state.clone();
        let state_obj = new_state
            .as_object_mut()
            .ok_or_else(|| aura_core::AuraError::invalid("state must be an object"))?;

        // Check authority exists
        let tokens = state_obj.get_mut(state_keys::TOKENS);
        if tokens.is_none() {
            return Ok(ActionResult {
                success: false,
                resulting_state: state.clone(),
                effects_produced: vec![],
                error: Some(format!("authority '{auth}' not found")),
            });
        }

        let tokens = tokens.unwrap();
        let token = tokens.get(auth);
        if token.is_none() {
            return Ok(ActionResult {
                success: false,
                resulting_state: state.clone(),
                effects_produced: vec![],
                error: Some(format!("authority '{auth}' not found")),
            });
        }

        let token = token.unwrap();
        let current_cap = token.get("cap_level").and_then(|v| v.as_i64()).unwrap_or(0);
        let attenuation_count = token
            .get("attenuation_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        // Check new_cap <= current_cap (can only narrow)
        if new_cap > current_cap {
            return Ok(ActionResult {
                success: false,
                resulting_state: state.clone(),
                effects_produced: vec![],
                error: Some(format!(
                    "cannot widen capability: {new_cap} > {current_cap}"
                )),
            });
        }

        // Check new_cap >= CAP_NONE
        if new_cap < cap_levels::NONE {
            return Ok(ActionResult {
                success: false,
                resulting_state: state.clone(),
                effects_produced: vec![],
                error: Some(format!("invalid capability level: {new_cap}")),
            });
        }

        // Update token
        let token_obj = tokens.get_mut(auth).unwrap().as_object_mut().unwrap();
        token_obj.insert("cap_level".to_string(), json!(new_cap));
        token_obj.insert(
            "attenuation_count".to_string(),
            json!(attenuation_count + 1),
        );

        Ok(ActionResult {
            success: true,
            resulting_state: new_state,
            effects_produced: vec![ActionEffect {
                effect_type: "token_attenuate".to_string(),
                parameters: json!({
                    "authority": auth,
                    "old_cap": current_cap,
                    "new_cap": new_cap,
                    "attenuation_count": attenuation_count + 1
                }),
            }],
            error: None,
        })
    }
}

// =============================================================================
// Registry Builder
// =============================================================================

use super::action_registry::ActionRegistry;

/// Create an action registry pre-populated with capability property handlers
pub fn capability_properties_registry() -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    registry.register(InitContextHandler);
    registry.register(InitAuthorityHandler);
    registry.register(CompleteTransportOpHandler);
    registry.register(AttenuateTokenHandler);
    registry
}

/// Create initial state for capability properties simulation
pub fn capability_properties_initial_state() -> Value {
    json!({
        state_keys::BUDGETS: {},
        state_keys::COMPLETED_OPS: [],
        state_keys::CURRENT_EPOCH: {},
        state_keys::TOKENS: {}
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_context() {
        let handler = InitContextHandler;
        let params = json!({
            "ctx": "ctx1",
            "peer": "auth1",
            "limit": 100
        });
        let state = capability_properties_initial_state();

        let result = handler
            .execute(&params, &HashMap::new(), &state)
            .await
            .unwrap();

        assert!(result.success);
        let new_state = result.resulting_state;

        // Check context was added
        assert_eq!(new_state["current_epoch"]["ctx1"], json!(0));

        // Check budget was created
        assert_eq!(new_state["budgets"]["ctx1"]["limit"], json!(100));
        assert_eq!(new_state["budgets"]["ctx1"]["spent"], json!(0));
        assert_eq!(new_state["budgets"]["ctx1"]["peer"], json!("auth1"));
    }

    #[tokio::test]
    async fn test_init_context_duplicate() {
        let handler = InitContextHandler;
        let params = json!({
            "ctx": "ctx1",
            "peer": "auth1",
            "limit": 100
        });

        // State where ctx1 already exists
        let state = json!({
            "current_epoch": {"ctx1": 0},
            "budgets": {},
            "completed_ops": [],
            "tokens": {}
        });

        let result = handler
            .execute(&params, &HashMap::new(), &state)
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("already exists"));
    }

    #[tokio::test]
    async fn test_init_authority() {
        let handler = InitAuthorityHandler;
        let params = json!({
            "auth": "auth1",
            "cap": cap_levels::FULL
        });
        let state = capability_properties_initial_state();

        let result = handler
            .execute(&params, &HashMap::new(), &state)
            .await
            .unwrap();

        assert!(result.success);
        let new_state = result.resulting_state;

        assert_eq!(
            new_state["tokens"]["auth1"]["cap_level"],
            json!(cap_levels::FULL)
        );
        assert_eq!(new_state["tokens"]["auth1"]["attenuation_count"], json!(0));
    }

    #[tokio::test]
    async fn test_complete_transport_op() {
        let handler = CompleteTransportOpHandler;

        // Set up state with context and authority
        let state = json!({
            "current_epoch": {"ctx1": 0},
            "budgets": {
                "ctx1": {
                    "context_id": "ctx1",
                    "peer": "auth1",
                    "epoch": 0,
                    "spent": 0,
                    "limit": 100
                }
            },
            "completed_ops": [],
            "tokens": {
                "auth1": {
                    "cap_level": cap_levels::FULL,
                    "attenuation_count": 0
                }
            }
        });

        let params = json!({
            "ctx": "ctx1",
            "src": "auth1",
            "dst": "auth2",
            "cost": 10
        });

        let result = handler
            .execute(&params, &HashMap::new(), &state)
            .await
            .unwrap();

        assert!(result.success);
        let new_state = result.resulting_state;

        // Check budget was updated
        assert_eq!(new_state["budgets"]["ctx1"]["spent"], json!(10));

        // Check operation was recorded
        assert_eq!(new_state["completed_ops"].as_array().unwrap().len(), 1);
        let op = &new_state["completed_ops"][0];
        assert_eq!(op["source"], json!("auth1"));
        assert_eq!(op["dest"], json!("auth2"));
        assert_eq!(op["cost"], json!(10));
        assert_eq!(op["charged"], json!(true));

        // Check guard chain was recorded
        assert_eq!(
            op["guard_steps_completed"],
            json!(["CapGuard", "FlowGuard", "JournalCoupler", "TransportSend"])
        );
    }

    #[tokio::test]
    async fn test_complete_transport_op_budget_exceeded() {
        let handler = CompleteTransportOpHandler;

        let state = json!({
            "current_epoch": {"ctx1": 0},
            "budgets": {
                "ctx1": {
                    "context_id": "ctx1",
                    "peer": "auth1",
                    "epoch": 0,
                    "spent": 95,
                    "limit": 100
                }
            },
            "completed_ops": [],
            "tokens": {
                "auth1": {"cap_level": cap_levels::FULL, "attenuation_count": 0}
            }
        });

        let params = json!({
            "ctx": "ctx1",
            "src": "auth1",
            "dst": "auth2",
            "cost": 10  // Would exceed limit
        });

        let result = handler
            .execute(&params, &HashMap::new(), &state)
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("budget exceeded"));
    }

    #[tokio::test]
    async fn test_attenuate_token() {
        let handler = AttenuateTokenHandler;

        let state = json!({
            "current_epoch": {},
            "budgets": {},
            "completed_ops": [],
            "tokens": {
                "auth1": {"cap_level": cap_levels::FULL, "attenuation_count": 0}
            }
        });

        let params = json!({
            "auth": "auth1",
            "new_cap": cap_levels::READ
        });

        let result = handler
            .execute(&params, &HashMap::new(), &state)
            .await
            .unwrap();

        assert!(result.success);
        let new_state = result.resulting_state;

        assert_eq!(
            new_state["tokens"]["auth1"]["cap_level"],
            json!(cap_levels::READ)
        );
        assert_eq!(new_state["tokens"]["auth1"]["attenuation_count"], json!(1));
    }

    #[tokio::test]
    async fn test_attenuate_token_cannot_widen() {
        let handler = AttenuateTokenHandler;

        let state = json!({
            "current_epoch": {},
            "budgets": {},
            "completed_ops": [],
            "tokens": {
                "auth1": {"cap_level": cap_levels::READ, "attenuation_count": 0}
            }
        });

        let params = json!({
            "auth": "auth1",
            "new_cap": cap_levels::FULL  // Wider than READ
        });

        let result = handler
            .execute(&params, &HashMap::new(), &state)
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("cannot widen"));
    }

    #[tokio::test]
    async fn test_capability_registry() {
        let registry = capability_properties_registry();

        assert!(registry.has_action("initContext"));
        assert!(registry.has_action("initAuthority"));
        assert!(registry.has_action("completeTransportOp"));
        assert!(registry.has_action("attenuateToken"));
        assert_eq!(registry.len(), 4);
    }

    #[tokio::test]
    async fn test_full_scenario() {
        let registry = capability_properties_registry();
        let mut state = capability_properties_initial_state();
        let nondet = HashMap::new();

        // 1. Initialize context
        let result = registry
            .execute(
                "initContext",
                &json!({"ctx": "ctx1", "peer": "auth1", "limit": 100}),
                &nondet,
                &state,
            )
            .await
            .unwrap();
        assert!(result.success);
        state = result.resulting_state;

        // 2. Initialize authority
        let result = registry
            .execute(
                "initAuthority",
                &json!({"auth": "auth1", "cap": cap_levels::FULL}),
                &nondet,
                &state,
            )
            .await
            .unwrap();
        assert!(result.success);
        state = result.resulting_state;

        // 3. Complete transport operation
        let result = registry
            .execute(
                "completeTransportOp",
                &json!({"ctx": "ctx1", "src": "auth1", "dst": "auth2", "cost": 10}),
                &nondet,
                &state,
            )
            .await
            .unwrap();
        assert!(result.success);
        state = result.resulting_state;

        // 4. Attenuate token
        let result = registry
            .execute(
                "attenuateToken",
                &json!({"auth": "auth1", "new_cap": cap_levels::READ}),
                &nondet,
                &state,
            )
            .await
            .unwrap();
        assert!(result.success);
        state = result.resulting_state;

        // Verify final state
        assert_eq!(state["budgets"]["ctx1"]["spent"], json!(10));
        assert_eq!(
            state["tokens"]["auth1"]["cap_level"],
            json!(cap_levels::READ)
        );
        assert_eq!(state["tokens"]["auth1"]["attenuation_count"], json!(1));
        assert_eq!(state["completed_ops"].as_array().unwrap().len(), 1);
    }
}
