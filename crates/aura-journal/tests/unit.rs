//! Extracted unit tests for `aura-journal`.

#[path = "common/mod.rs"]
mod common;

#[path = "unit/extensibility.rs"]
mod extensibility;

#[path = "unit/pure_merge.rs"]
mod pure_merge;

#[path = "unit/pure_reduce.rs"]
mod pure_reduce;

#[path = "unit/authority_state.rs"]
mod authority_state;

#[path = "unit/commitment_integration.rs"]
mod commitment_integration;

#[path = "unit/crdt_handler_trait.rs"]
mod crdt_handler_trait;

#[path = "unit/algebra_op_log.rs"]
mod algebra_op_log;

#[path = "unit/algebra_account_state.rs"]
mod algebra_account_state;

#[path = "unit/effect_api_capability.rs"]
mod effect_api_capability;

#[path = "unit/effect_api_intent.rs"]
mod effect_api_intent;

#[path = "unit/effects.rs"]
mod effects;

#[path = "unit/fact.rs"]
mod fact;

#[path = "unit/reduction.rs"]
mod reduction;
