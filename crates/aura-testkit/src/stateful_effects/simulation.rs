//! Mock simulation effect handlers for testing

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Mock simulation storage
#[derive(Debug)]
pub struct MockSimulationStorage {
    pub data: HashMap<String, Vec<u8>>,
}

/// Mock simulation handler for testing
#[derive(Debug)]
pub struct MockSimulationHandler {
    storage: Arc<RwLock<MockSimulationStorage>>,
}

impl MockSimulationHandler {
    pub fn new() -> Self {
        Self {
            storage: Arc::new(RwLock::new(MockSimulationStorage {
                data: HashMap::new(),
            })),
        }
    }
}

/// Stateless simulation handler for testing
#[derive(Debug)]
pub struct StatelessSimulationHandler;

impl StatelessSimulationHandler {
    pub fn new() -> Self {
        Self
    }
}
