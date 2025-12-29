//! Mock handler for testing
//!
//! Provides a simple mock implementation of AuraHandler for unit tests.
//!
//! # Blocking Lock Usage
//!
//! Uses `std::sync::Mutex` because this is Layer 8 test infrastructure where:
//! 1. Tests run in controlled single-threaded contexts
//! 2. Lock contention is not a concern in test scenarios
//! 3. Simpler synchronous API is preferred for test clarity

#![allow(clippy::disallowed_types)]

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aura_core::effects::ExecutionMode;
use aura_core::AuraError;
use aura_mpst::LocalSessionType;

/// Minimal effect type used by mock handlers in tests
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EffectType {
    Dummy,
}

impl EffectType {
    pub fn all() -> Vec<EffectType> {
        vec![EffectType::Dummy]
    }
}

/// Minimal AuraContext used by mock handlers in tests
#[derive(Clone, Debug, Default)]
pub struct AuraContext;

pub type AuraHandlerError = AuraError;

#[async_trait]
pub trait AuraHandler: Send + Sync {
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        params: &[u8],
        context: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError>;

    async fn execute_session(
        &self,
        session: LocalSessionType,
        ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError>;

    fn supports_effect(&self, effect_type: EffectType) -> bool;

    fn execution_mode(&self) -> ExecutionMode;

    fn supported_effects(&self) -> Vec<EffectType>;
}

/// Mock handler for testing effect execution
#[derive(Clone)]
pub struct MockHandler {
    /// Recorded calls for verification
    calls: Arc<Mutex<Vec<MockCall>>>,
    /// Predefined responses
    responses: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

/// Record of a mock call
#[derive(Debug, Clone)]
pub struct MockCall {
    pub effect_type: EffectType,
    pub operation: String,
    pub params: Vec<u8>,
}

impl Default for MockHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MockHandler {
    /// Create a new mock handler
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Set a predefined response for an operation
    pub fn set_response(&self, operation: &str, response: Vec<u8>) {
        self.responses
            .lock()
            .unwrap()
            .insert(operation.to_string(), response);
    }

    /// Get recorded calls
    pub fn get_calls(&self) -> Vec<MockCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Clear recorded calls
    pub fn clear_calls(&self) {
        self.calls.lock().unwrap().clear();
    }
}

#[async_trait]
impl AuraHandler for MockHandler {
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        params: &[u8],
        _context: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        // Record the call
        self.calls.lock().unwrap().push(MockCall {
            effect_type,
            operation: operation.to_string(),
            params: params.to_vec(),
        });

        // Return predefined response or default
        let responses = self.responses.lock().unwrap();
        if let Some(response) = responses.get(operation) {
            Ok(response.clone())
        } else {
            // Default responses for common operations
            match operation {
                "current_timestamp" => Ok(1_000_000u64.to_le_bytes().to_vec()),
                "current_timestamp_millis" => Ok(1_000_000_000u64.to_le_bytes().to_vec()),
                "random_uuid" => {
                    let mut h = aura_core::hash::hasher();
                    h.update(b"mock-random-uuid");
                    let digest = h.finalize();
                    let mut bytes = [0u8; 16];
                    bytes.copy_from_slice(&digest[..16]);
                    Ok(uuid::Uuid::from_bytes(bytes).as_bytes().to_vec())
                }
                "hash" => Ok(vec![0; 32]),
                _ => Ok(Vec::new()),
            }
        }
    }

    async fn execute_session(
        &self,
        _session: LocalSessionType,
        _ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError> {
        Ok(()) // Mock implementation does nothing
    }

    fn supports_effect(&self, _effect_type: EffectType) -> bool {
        true // Mock handler supports all effects
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Testing
    }

    fn supported_effects(&self) -> Vec<EffectType> {
        EffectType::all()
    }
}
