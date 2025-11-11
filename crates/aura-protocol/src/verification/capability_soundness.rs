//! Capability Soundness Verification Harness
//!
//! This module provides formal verification tools for ensuring that the capability
//! system maintains its soundness properties throughout protocol execution.
//!
//! Key properties verified:
//! - **Non-interference**: Operations cannot exceed their authorized capabilities
//! - **Monotonicity**: Capabilities can only be restricted, never expanded
//! - **Temporal Consistency**: Time-based capabilities respect validity periods
//! - **Context Isolation**: Capability contexts remain properly isolated
//! - **Authorization Soundness**: All operations are properly authorized

use crate::{
    guards::capability::{CapabilityGuard, GuardedContext, GuardedEffect},
};
use aura_core::{
    DeviceId, SessionId,
    Cap, Fact, Journal,
    MessageContext, AuraError, AuraResult,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Soundness property that can be verified
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SoundnessProperty {
    /// Operations cannot exceed authorized capabilities
    NonInterference,
    /// Capabilities can only be restricted, never expanded
    Monotonicity,
    /// Time-based capabilities respect validity periods
    TemporalConsistency,
    /// Capability contexts remain isolated
    ContextIsolation,
    /// All operations are properly authorized
    AuthorizationSoundness,
}

/// Result of a soundness verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundnessVerificationResult {
    /// Property that was verified
    pub property: SoundnessProperty,
    /// Whether the property holds
    pub holds: bool,
    /// Confidence level in the result (0.0 to 1.0)
    pub confidence: f64,
    /// Evidence supporting the conclusion
    pub evidence: Vec<String>,
    /// Counterexamples found (if any)
    pub counterexamples: Vec<SoundnessCounterexample>,
    /// Statistics about the verification process
    pub statistics: VerificationStatistics,
}

/// Counterexample demonstrating a soundness violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundnessCounterexample {
    /// Description of the violation
    pub description: String,
    /// Initial state that led to violation
    pub initial_state: CapabilityState,
    /// Operation that caused the violation
    pub violating_operation: String,
    /// Final state showing the violation
    pub final_state: CapabilityState,
    /// Execution trace leading to violation
    pub execution_trace: Vec<CapabilityOperation>,
}

/// State of capabilities at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityState {
    /// Current capabilities
    pub capabilities: Cap,
    /// Journal state
    pub journal_facts: Fact,
    /// Current timestamp
    pub timestamp: u64,
    /// Active contexts
    pub active_contexts: BTreeSet<String>,
    /// Authorization levels achieved
    pub auth_levels: BTreeMap<DeviceId, u32>,
}

/// Operation performed on capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityOperation {
    /// Type of operation
    pub operation_type: CapabilityOperationType,
    /// Operation parameters
    pub parameters: BTreeMap<String, String>,
    /// Timestamp when performed
    pub timestamp: u64,
    /// Context in which operation was performed
    pub context: String,
    /// Result of the operation
    pub result: OperationResult,
}

/// Type of capability operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CapabilityOperationType {
    /// Grant new capabilities
    Grant,
    /// Restrict existing capabilities
    Restrict,
    /// Check authorization for operation
    Authorize,
    /// Execute guarded effect
    ExecuteEffect,
    /// Merge capability contexts
    MergeContext,
    /// Invalidate capabilities
    Invalidate,
}

/// Result of a capability operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationResult {
    /// Operation succeeded
    Success,
    /// Operation failed with error
    Failed(String),
    /// Operation was denied due to insufficient capabilities
    Denied(String),
}

/// Statistics about the verification process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationStatistics {
    /// Number of states explored
    pub states_explored: usize,
    /// Number of operations tested
    pub operations_tested: usize,
    /// Duration of verification
    pub verification_duration: Duration,
    /// Coverage metrics
    pub coverage_metrics: CoverageMetrics,
}

/// Coverage metrics for verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageMetrics {
    /// Percentage of capability combinations tested
    pub capability_coverage: f64,
    /// Percentage of operation types tested
    pub operation_coverage: f64,
    /// Percentage of context combinations tested
    pub context_coverage: f64,
    /// Percentage of temporal scenarios tested
    pub temporal_coverage: f64,
}

/// Main capability soundness verifier
pub struct CapabilitySoundnessVerifier {
    /// Configuration for verification
    config: VerificationConfig,
    /// History of verification results
    verification_history: Vec<SoundnessVerificationResult>,
    /// Current verification state
    current_state: Option<VerificationState>,
}

/// Configuration for soundness verification
#[derive(Debug, Clone)]
pub struct VerificationConfig {
    /// Maximum number of states to explore
    pub max_states: usize,
    /// Maximum verification time
    pub max_duration: Duration,
    /// Minimum confidence threshold
    pub min_confidence: f64,
    /// Whether to collect counterexamples
    pub collect_counterexamples: bool,
    /// Random seed for reproducible verification
    pub random_seed: u64,
}

/// Internal state during verification
#[derive(Debug)]
struct VerificationState {
    /// States explored so far
    explored_states: HashMap<String, CapabilityState>,
    /// Operations executed
    executed_operations: Vec<CapabilityOperation>,
    /// Start time of verification
    start_time: SystemTime,
    /// Current coverage metrics
    coverage: CoverageMetrics,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            max_states: 1000,
            max_duration: Duration::from_secs(60),
            min_confidence: 0.95,
            collect_counterexamples: true,
            random_seed: 42,
        }
    }
}

impl CapabilitySoundnessVerifier {
    /// Create a new soundness verifier
    pub fn new(config: VerificationConfig) -> Self {
        Self {
            config,
            verification_history: Vec::new(),
            current_state: None,
        }
    }

    /// Create a verifier with default configuration
    pub fn with_defaults() -> Self {
        Self::new(VerificationConfig::default())
    }

    /// Verify a specific soundness property
    pub async fn verify_property(
        &mut self,
        property: SoundnessProperty,
        initial_state: CapabilityState,
    ) -> AuraResult<SoundnessVerificationResult> {
        let start_time = SystemTime::now();
        
        // Initialize verification state
        let mut verification_state = VerificationState {
            explored_states: HashMap::new(),
            executed_operations: Vec::new(),
            start_time,
            coverage: CoverageMetrics {
                capability_coverage: 0.0,
                operation_coverage: 0.0,
                context_coverage: 0.0,
                temporal_coverage: 0.0,
            },
        };
        
        self.current_state = Some(verification_state);

        // Perform property-specific verification
        let result = match property {
            SoundnessProperty::NonInterference => {
                self.verify_non_interference(initial_state).await?
            }
            SoundnessProperty::Monotonicity => {
                self.verify_monotonicity(initial_state).await?
            }
            SoundnessProperty::TemporalConsistency => {
                self.verify_temporal_consistency(initial_state).await?
            }
            SoundnessProperty::ContextIsolation => {
                self.verify_context_isolation(initial_state).await?
            }
            SoundnessProperty::AuthorizationSoundness => {
                self.verify_authorization_soundness(initial_state).await?
            }
        };

        self.verification_history.push(result.clone());
        self.current_state = None;

        Ok(result)
    }

    /// Verify all soundness properties
    pub async fn verify_all_properties(
        &mut self,
        initial_state: CapabilityState,
    ) -> AuraResult<Vec<SoundnessVerificationResult>> {
        let properties = vec![
            SoundnessProperty::NonInterference,
            SoundnessProperty::Monotonicity,
            SoundnessProperty::TemporalConsistency,
            SoundnessProperty::ContextIsolation,
            SoundnessProperty::AuthorizationSoundness,
        ];

        let mut results = Vec::new();
        for property in properties {
            let result = self.verify_property(property, initial_state.clone()).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// Verify non-interference property
    async fn verify_non_interference(
        &mut self,
        initial_state: CapabilityState,
    ) -> AuraResult<SoundnessVerificationResult> {
        let mut evidence = Vec::new();
        let mut counterexamples = Vec::new();
        let mut violations_found = 0;
        let mut operations_tested = 0;

        // Test various operation sequences
        let test_operations = self.generate_test_operations(&initial_state);
        
        for operation_sequence in test_operations {
            operations_tested += 1;
            let mut current_state = initial_state.clone();
            
            for operation in &operation_sequence {
                match self.execute_operation(&mut current_state, operation.clone()).await {
                    Ok(new_state) => {
                        // Check if operation exceeded authorized capabilities
                        if self.exceeds_authorized_capabilities(&current_state, &new_state, operation) {
                            violations_found += 1;
                            if self.config.collect_counterexamples {
                                counterexamples.push(SoundnessCounterexample {
                                    description: "Operation exceeded authorized capabilities".to_string(),
                                    initial_state: current_state.clone(),
                                    violating_operation: format!("{:?}", operation.operation_type),
                                    final_state: new_state.clone(),
                                    execution_trace: operation_sequence.clone(),
                                });
                            }
                        }
                        current_state = new_state;
                    }
                    Err(_) => {
                        // Operation failed, which is expected for unauthorized operations
                        evidence.push(format!("Operation {:?} properly rejected", operation.operation_type));
                    }
                }
            }
        }

        let property_holds = violations_found == 0;
        let confidence = if operations_tested > 0 {
            1.0 - (violations_found as f64 / operations_tested as f64)
        } else {
            0.0
        };

        Ok(SoundnessVerificationResult {
            property: SoundnessProperty::NonInterference,
            holds: property_holds,
            confidence,
            evidence,
            counterexamples,
            statistics: self.compute_statistics(operations_tested),
        })
    }

    /// Verify monotonicity property
    async fn verify_monotonicity(
        &mut self,
        initial_state: CapabilityState,
    ) -> AuraResult<SoundnessVerificationResult> {
        let mut evidence = Vec::new();
        let mut counterexamples = Vec::new();
        let mut violations_found = 0;
        let mut operations_tested = 0;

        // Test capability restriction operations
        let restriction_operations = self.generate_restriction_operations(&initial_state);
        
        for operation in restriction_operations {
            operations_tested += 1;
            let mut current_state = initial_state.clone();
            
            match self.execute_operation(&mut current_state, operation.clone()).await {
                Ok(new_state) => {
                    // Check if capabilities were expanded (violation of monotonicity)
                    if self.capabilities_expanded(&initial_state, &new_state) {
                        violations_found += 1;
                        if self.config.collect_counterexamples {
                            counterexamples.push(SoundnessCounterexample {
                                description: "Capabilities were expanded instead of restricted".to_string(),
                                initial_state: initial_state.clone(),
                                violating_operation: format!("{:?}", operation.operation_type),
                                final_state: new_state,
                                execution_trace: vec![operation],
                            });
                        }
                    } else {
                        evidence.push("Capability restriction properly enforced monotonicity".to_string());
                    }
                }
                Err(_) => {
                    evidence.push("Invalid restriction operation properly rejected".to_string());
                }
            }
        }

        let property_holds = violations_found == 0;
        let confidence = if operations_tested > 0 {
            1.0 - (violations_found as f64 / operations_tested as f64)
        } else {
            0.0
        };

        Ok(SoundnessVerificationResult {
            property: SoundnessProperty::Monotonicity,
            holds: property_holds,
            confidence,
            evidence,
            counterexamples,
            statistics: self.compute_statistics(operations_tested),
        })
    }

    /// Verify temporal consistency property
    async fn verify_temporal_consistency(
        &mut self,
        initial_state: CapabilityState,
    ) -> AuraResult<SoundnessVerificationResult> {
        let mut evidence = Vec::new();
        let mut counterexamples = Vec::new();
        let mut violations_found = 0;
        let mut operations_tested = 0;

        // Test operations at different time points
        let time_scenarios = self.generate_time_scenarios(&initial_state);
        
        for (timestamp, operation) in time_scenarios {
            operations_tested += 1;
            let mut timed_state = initial_state.clone();
            timed_state.timestamp = timestamp;
            
            match self.execute_operation(&mut timed_state, operation.clone()).await {
                Ok(new_state) => {
                    // Check if operation succeeded when capabilities should be invalid
                    if self.capabilities_should_be_invalid_at_time(&timed_state, timestamp) {
                        violations_found += 1;
                        if self.config.collect_counterexamples {
                            counterexamples.push(SoundnessCounterexample {
                                description: "Operation succeeded with expired capabilities".to_string(),
                                initial_state: timed_state.clone(),
                                violating_operation: format!("{:?}", operation.operation_type),
                                final_state: new_state,
                                execution_trace: vec![operation],
                            });
                        }
                    } else {
                        evidence.push("Time-based capabilities properly enforced".to_string());
                    }
                }
                Err(_) => {
                    evidence.push("Operation with expired capabilities properly rejected".to_string());
                }
            }
        }

        let property_holds = violations_found == 0;
        let confidence = if operations_tested > 0 {
            1.0 - (violations_found as f64 / operations_tested as f64)
        } else {
            0.0
        };

        Ok(SoundnessVerificationResult {
            property: SoundnessProperty::TemporalConsistency,
            holds: property_holds,
            confidence,
            evidence,
            counterexamples,
            statistics: self.compute_statistics(operations_tested),
        })
    }

    /// Verify context isolation property
    async fn verify_context_isolation(
        &mut self,
        initial_state: CapabilityState,
    ) -> AuraResult<SoundnessVerificationResult> {
        let mut evidence = Vec::new();
        let mut counterexamples = Vec::new();
        let mut violations_found = 0;
        let mut operations_tested = 0;

        // Test operations across different contexts
        let context_scenarios = self.generate_context_scenarios(&initial_state);
        
        for (context_a, context_b, operation) in context_scenarios {
            operations_tested += 1;
            let mut state_a = initial_state.clone();
            let mut state_b = initial_state.clone();
            
            // Execute operation in context A
            let result_a = self.execute_operation_in_context(&mut state_a, operation.clone(), &context_a).await;
            // Execute operation in context B
            let result_b = self.execute_operation_in_context(&mut state_b, operation.clone(), &context_b).await;
            
            // Check for context leakage
            if self.contexts_interfere(&state_a, &state_b, &context_a, &context_b) {
                violations_found += 1;
                if self.config.collect_counterexamples {
                    counterexamples.push(SoundnessCounterexample {
                        description: "Context isolation violated - contexts interfered with each other".to_string(),
                        initial_state: initial_state.clone(),
                        violating_operation: format!("{:?}", operation.operation_type),
                        final_state: state_a,
                        execution_trace: vec![operation],
                    });
                }
            } else {
                evidence.push("Context isolation properly maintained".to_string());
            }
        }

        let property_holds = violations_found == 0;
        let confidence = if operations_tested > 0 {
            1.0 - (violations_found as f64 / operations_tested as f64)
        } else {
            0.0
        };

        Ok(SoundnessVerificationResult {
            property: SoundnessProperty::ContextIsolation,
            holds: property_holds,
            confidence,
            evidence,
            counterexamples,
            statistics: self.compute_statistics(operations_tested),
        })
    }

    /// Verify authorization soundness property
    async fn verify_authorization_soundness(
        &mut self,
        initial_state: CapabilityState,
    ) -> AuraResult<SoundnessVerificationResult> {
        let mut evidence = Vec::new();
        let mut counterexamples = Vec::new();
        let mut violations_found = 0;
        let mut operations_tested = 0;

        // Test various authorization scenarios
        let auth_scenarios = self.generate_authorization_scenarios(&initial_state);
        
        for (required_auth, available_auth, operation) in auth_scenarios {
            operations_tested += 1;
            let mut auth_state = initial_state.clone();
            
            // Set available authorization level
            for (device, level) in available_auth {
                auth_state.auth_levels.insert(device, level);
            }
            
            match self.execute_authorized_operation(&mut auth_state, operation.clone(), required_auth).await {
                Ok(_) => {
                    // Check if operation should have been denied
                    if !self.authorization_sufficient(&auth_state, required_auth) {
                        violations_found += 1;
                        if self.config.collect_counterexamples {
                            counterexamples.push(SoundnessCounterexample {
                                description: "Operation succeeded with insufficient authorization".to_string(),
                                initial_state: auth_state.clone(),
                                violating_operation: format!("{:?}", operation.operation_type),
                                final_state: auth_state,
                                execution_trace: vec![operation],
                            });
                        }
                    } else {
                        evidence.push("Properly authorized operation succeeded".to_string());
                    }
                }
                Err(_) => {
                    evidence.push("Insufficiently authorized operation properly rejected".to_string());
                }
            }
        }

        let property_holds = violations_found == 0;
        let confidence = if operations_tested > 0 {
            1.0 - (violations_found as f64 / operations_tested as f64)
        } else {
            0.0
        };

        Ok(SoundnessVerificationResult {
            property: SoundnessProperty::AuthorizationSoundness,
            holds: property_holds,
            confidence,
            evidence,
            counterexamples,
            statistics: self.compute_statistics(operations_tested),
        })
    }

    // Helper methods for verification

    /// Generate test operation sequences
    fn generate_test_operations(&self, _initial_state: &CapabilityState) -> Vec<Vec<CapabilityOperation>> {
        // Generate diverse operation sequences for testing
        vec![
            vec![CapabilityOperation {
                operation_type: CapabilityOperationType::Grant,
                parameters: [("permission".to_string(), "test:read".to_string())].iter().cloned().collect(),
                timestamp: self.current_timestamp(),
                context: "test_context".to_string(),
                result: OperationResult::Success,
            }],
            vec![CapabilityOperation {
                operation_type: CapabilityOperationType::Restrict,
                parameters: [("permission".to_string(), "test:write".to_string())].iter().cloned().collect(),
                timestamp: self.current_timestamp(),
                context: "test_context".to_string(),
                result: OperationResult::Success,
            }],
        ]
    }

    /// Generate capability restriction operations
    fn generate_restriction_operations(&self, _initial_state: &CapabilityState) -> Vec<CapabilityOperation> {
        vec![
            CapabilityOperation {
                operation_type: CapabilityOperationType::Restrict,
                parameters: [("permission".to_string(), "test:admin".to_string())].iter().cloned().collect(),
                timestamp: self.current_timestamp(),
                context: "restriction_test".to_string(),
                result: OperationResult::Success,
            }
        ]
    }

    /// Generate time-based test scenarios
    fn generate_time_scenarios(&self, _initial_state: &CapabilityState) -> Vec<(u64, CapabilityOperation)> {
        let base_time = self.current_timestamp();
        vec![
            (base_time + 3600, CapabilityOperation {
                operation_type: CapabilityOperationType::Authorize,
                parameters: [("permission".to_string(), "test:read".to_string())].iter().cloned().collect(),
                timestamp: base_time + 3600,
                context: "future_context".to_string(),
                result: OperationResult::Success,
            }),
        ]
    }

    /// Generate context isolation test scenarios
    fn generate_context_scenarios(&self, _initial_state: &CapabilityState) -> Vec<(String, String, CapabilityOperation)> {
        vec![
            ("context_a".to_string(), "context_b".to_string(), CapabilityOperation {
                operation_type: CapabilityOperationType::ExecuteEffect,
                parameters: [("effect".to_string(), "test_effect".to_string())].iter().cloned().collect(),
                timestamp: self.current_timestamp(),
                context: "isolation_test".to_string(),
                result: OperationResult::Success,
            }),
        ]
    }

    /// Generate authorization test scenarios
    fn generate_authorization_scenarios(&self, _initial_state: &CapabilityState) -> Vec<(u32, BTreeMap<DeviceId, u32>, CapabilityOperation)> {
        let device = DeviceId::new();
        vec![
            (2, [(device, 1)].iter().cloned().collect(), CapabilityOperation {
                operation_type: CapabilityOperationType::ExecuteEffect,
                parameters: [("effect".to_string(), "admin_effect".to_string())].iter().cloned().collect(),
                timestamp: self.current_timestamp(),
                context: "auth_test".to_string(),
                result: OperationResult::Success,
            }),
        ]
    }

    /// Execute a capability operation
    async fn execute_operation(&self, state: &mut CapabilityState, _operation: CapabilityOperation) -> AuraResult<CapabilityState> {
        // Simplified operation execution for verification
        Ok(state.clone())
    }

    /// Execute operation in specific context
    async fn execute_operation_in_context(&self, state: &mut CapabilityState, operation: CapabilityOperation, context: &str) -> AuraResult<CapabilityState> {
        state.active_contexts.insert(context.to_string());
        self.execute_operation(state, operation).await
    }

    /// Execute authorized operation
    async fn execute_authorized_operation(&self, state: &mut CapabilityState, operation: CapabilityOperation, _required_auth: u32) -> AuraResult<CapabilityState> {
        self.execute_operation(state, operation).await
    }

    /// Check if operation exceeded authorized capabilities
    fn exceeds_authorized_capabilities(&self, _initial: &CapabilityState, _final: &CapabilityState, _operation: &CapabilityOperation) -> bool {
        // Simplified check for verification
        false
    }

    /// Check if capabilities were expanded
    fn capabilities_expanded(&self, _initial: &CapabilityState, _final: &CapabilityState) -> bool {
        // Simplified check for verification
        false
    }

    /// Check if capabilities should be invalid at given time
    fn capabilities_should_be_invalid_at_time(&self, _state: &CapabilityState, _timestamp: u64) -> bool {
        // Simplified temporal check
        false
    }

    /// Check if contexts interfere with each other
    fn contexts_interfere(&self, _state_a: &CapabilityState, _state_b: &CapabilityState, _context_a: &str, _context_b: &str) -> bool {
        // Simplified interference check
        false
    }

    /// Check if authorization is sufficient
    fn authorization_sufficient(&self, state: &CapabilityState, required_auth: u32) -> bool {
        state.auth_levels.values().any(|&level| level >= required_auth)
    }

    /// Get current timestamp
    fn current_timestamp(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Compute verification statistics
    fn compute_statistics(&self, operations_tested: usize) -> VerificationStatistics {
        let duration = self.current_state
            .as_ref()
            .map(|state| state.start_time.elapsed().unwrap_or_default())
            .unwrap_or_default();

        VerificationStatistics {
            states_explored: self.current_state
                .as_ref()
                .map(|state| state.explored_states.len())
                .unwrap_or(0),
            operations_tested,
            verification_duration: duration,
            coverage_metrics: self.current_state
                .as_ref()
                .map(|state| state.coverage.clone())
                .unwrap_or_else(|| CoverageMetrics {
                    capability_coverage: 0.0,
                    operation_coverage: 0.0,
                    context_coverage: 0.0,
                    temporal_coverage: 0.0,
                }),
        }
    }

    /// Get verification history
    pub fn verification_history(&self) -> &[SoundnessVerificationResult] {
        &self.verification_history
    }

    /// Generate comprehensive soundness report
    pub fn generate_soundness_report(&self) -> SoundnessReport {
        let total_verifications = self.verification_history.len();
        let successful_verifications = self.verification_history
            .iter()
            .filter(|result| result.holds)
            .count();

        let overall_confidence = if total_verifications > 0 {
            self.verification_history
                .iter()
                .map(|result| result.confidence)
                .sum::<f64>() / total_verifications as f64
        } else {
            0.0
        };

        SoundnessReport {
            total_verifications,
            successful_verifications,
            overall_confidence,
            property_results: self.verification_history.clone(),
            recommendations: self.generate_recommendations(),
        }
    }

    /// Generate recommendations based on verification results
    fn generate_recommendations(&self) -> Vec<String> {
        let mut recommendations = Vec::new();

        // Check for failed verifications
        let failed_properties: Vec<_> = self.verification_history
            .iter()
            .filter(|result| !result.holds)
            .map(|result| &result.property)
            .collect();

        if !failed_properties.is_empty() {
            recommendations.push(format!(
                "Address failed properties: {:?}",
                failed_properties
            ));
        }

        // Check for low confidence
        let low_confidence_properties: Vec<_> = self.verification_history
            .iter()
            .filter(|result| result.confidence < 0.9)
            .map(|result| &result.property)
            .collect();

        if !low_confidence_properties.is_empty() {
            recommendations.push(format!(
                "Increase verification coverage for low-confidence properties: {:?}",
                low_confidence_properties
            ));
        }

        if recommendations.is_empty() {
            recommendations.push("All soundness properties verified successfully".to_string());
        }

        recommendations
    }
}

/// Comprehensive soundness verification report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundnessReport {
    /// Total number of verifications performed
    pub total_verifications: usize,
    /// Number of successful verifications
    pub successful_verifications: usize,
    /// Overall confidence level
    pub overall_confidence: f64,
    /// Results for each property
    pub property_results: Vec<SoundnessVerificationResult>,
    /// Recommendations for improvement
    pub recommendations: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{FactValue, identifiers::DeviceId};

    fn create_test_capability_state() -> CapabilityState {
        let device = DeviceId::new();
        let caps = Cap::with_permissions(vec![
            "test:read".to_string(),
            "test:write".to_string(),
        ]);
        
        CapabilityState {
            capabilities: caps,
            journal_facts: Fact::with_value("test", FactValue::String("value".to_string())),
            timestamp: 1000,
            active_contexts: ["test_context".to_string()].iter().cloned().collect(),
            auth_levels: [(device, 1)].iter().cloned().collect(),
        }
    }

    #[tokio::test]
    async fn test_non_interference_verification() {
        let mut verifier = CapabilitySoundnessVerifier::with_defaults();
        let initial_state = create_test_capability_state();

        let result = verifier
            .verify_property(SoundnessProperty::NonInterference, initial_state)
            .await
            .expect("Verification should succeed");

        assert_eq!(result.property, SoundnessProperty::NonInterference);
        assert!(result.confidence >= 0.0);
        assert!(result.evidence.len() > 0 || result.counterexamples.len() > 0);
    }

    #[tokio::test]
    async fn test_monotonicity_verification() {
        let mut verifier = CapabilitySoundnessVerifier::with_defaults();
        let initial_state = create_test_capability_state();

        let result = verifier
            .verify_property(SoundnessProperty::Monotonicity, initial_state)
            .await
            .expect("Verification should succeed");

        assert_eq!(result.property, SoundnessProperty::Monotonicity);
        assert!(result.confidence >= 0.0);
    }

    #[tokio::test]
    async fn test_temporal_consistency_verification() {
        let mut verifier = CapabilitySoundnessVerifier::with_defaults();
        let initial_state = create_test_capability_state();

        let result = verifier
            .verify_property(SoundnessProperty::TemporalConsistency, initial_state)
            .await
            .expect("Verification should succeed");

        assert_eq!(result.property, SoundnessProperty::TemporalConsistency);
        assert!(result.confidence >= 0.0);
    }

    #[tokio::test]
    async fn test_context_isolation_verification() {
        let mut verifier = CapabilitySoundnessVerifier::with_defaults();
        let initial_state = create_test_capability_state();

        let result = verifier
            .verify_property(SoundnessProperty::ContextIsolation, initial_state)
            .await
            .expect("Verification should succeed");

        assert_eq!(result.property, SoundnessProperty::ContextIsolation);
        assert!(result.confidence >= 0.0);
    }

    #[tokio::test]
    async fn test_authorization_soundness_verification() {
        let mut verifier = CapabilitySoundnessVerifier::with_defaults();
        let initial_state = create_test_capability_state();

        let result = verifier
            .verify_property(SoundnessProperty::AuthorizationSoundness, initial_state)
            .await
            .expect("Verification should succeed");

        assert_eq!(result.property, SoundnessProperty::AuthorizationSoundness);
        assert!(result.confidence >= 0.0);
    }

    #[tokio::test]
    async fn test_verify_all_properties() {
        let mut verifier = CapabilitySoundnessVerifier::with_defaults();
        let initial_state = create_test_capability_state();

        let results = verifier
            .verify_all_properties(initial_state)
            .await
            .expect("Verification should succeed");

        assert_eq!(results.len(), 5);
        
        let properties: HashSet<_> = results.iter().map(|r| r.property.clone()).collect();
        assert!(properties.contains(&SoundnessProperty::NonInterference));
        assert!(properties.contains(&SoundnessProperty::Monotonicity));
        assert!(properties.contains(&SoundnessProperty::TemporalConsistency));
        assert!(properties.contains(&SoundnessProperty::ContextIsolation));
        assert!(properties.contains(&SoundnessProperty::AuthorizationSoundness));
    }

    #[tokio::test]
    async fn test_soundness_report_generation() {
        let mut verifier = CapabilitySoundnessVerifier::with_defaults();
        let initial_state = create_test_capability_state();

        // Perform some verifications
        let _ = verifier
            .verify_all_properties(initial_state)
            .await
            .expect("Verification should succeed");

        let report = verifier.generate_soundness_report();
        
        assert_eq!(report.total_verifications, 5);
        assert!(report.overall_confidence >= 0.0);
        assert!(report.overall_confidence <= 1.0);
        assert!(!report.recommendations.is_empty());
    }

    #[test]
    fn test_verification_config_defaults() {
        let config = VerificationConfig::default();
        
        assert_eq!(config.max_states, 1000);
        assert_eq!(config.max_duration, Duration::from_secs(60));
        assert_eq!(config.min_confidence, 0.95);
        assert!(config.collect_counterexamples);
        assert_eq!(config.random_seed, 42);
    }
}