//! Leakage tracking handlers for testing

use aura_core::ContextId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Leakage budget
#[derive(Debug, Clone)]
pub struct LeakageBudget {
    pub remaining: u64,
}

/// Leakage event
#[derive(Debug, Clone)]
pub struct LeakageEvent {
    pub context: ContextId,
    pub amount: u64,
    pub timestamp: u64,
}

/// Production leakage handler for testing
#[derive(Debug)]
pub struct ProductionLeakageHandler {
    budgets: Arc<RwLock<HashMap<ContextId, LeakageBudget>>>,
    history: Arc<RwLock<Vec<LeakageEvent>>>,
}

impl ProductionLeakageHandler {
    pub fn new() -> Self {
        Self {
            budgets: Arc::new(RwLock::new(HashMap::new())),
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

/// Test leakage handler for testing
#[derive(Debug)]
pub struct TestLeakageHandler {
    pub events: Arc<RwLock<Vec<LeakageEvent>>>,
}

impl TestLeakageHandler {
    pub fn new() -> Self {
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
        }
    }
}