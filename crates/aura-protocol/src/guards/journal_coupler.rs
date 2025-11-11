//! JournalMap Coupler for Guard Chain Integration
//!
//! This module provides the `JournalMapCoupler` that bridges the guard chain execution
//! with journal CRDT operations. It ensures that protocol operations that succeed
//! capability checks properly update the distributed journal state.
//!
//! ## Integration Flow
//!
//! ```text
//! CapGuard → FlowGuard → JournalMapCoupler → Protocol Execution
//!     ↓         ↓            ↓                    ↓
//! Check     Check       Apply journal      Execute with
//! caps      budget      deltas atomically  full context
//! ```
//!
//! The JournalMapCoupler implements the formal model's "journal coupling" semantics
//! where protocol operations atomically update both local state and distributed
//! journal facts using join-semilattice operations.

use super::{ExecutionMetrics, GuardedExecutionResult, LeakageBudget, ProtocolGuard};
use crate::effects::{system::AuraEffectSystem, JournalEffects};
use aura_core::{AuraError, AuraResult, JournalMap};
use aura_mpst::journal_coupling::{JournalAnnotation, JournalOpType};
use serde_json::Value as JsonValue;
use std::{collections::HashMap, future::Future, time::Instant};
use tracing::{debug, error, info, warn};

/// JournalMap coupling coordinator for the guard chain
///
/// The JournalMapCoupler sits at the end of the guard chain (after CapGuard and FlowGuard)
/// and ensures that successful protocol operations properly update the distributed
/// journal state using CRDT operations.
#[derive(Debug)]
pub struct JournalMapCoupler {
    /// JournalMap annotations for operations
    pub annotations: HashMap<String, JournalMapAnnotation>,
    /// Whether to apply deltas optimistically or pessimistically  
    pub optimistic_application: bool,
    /// Maximum retry attempts for journal operations
    pub max_retry_attempts: usize,
}

/// Result of journal coupling operation
#[derive(Debug)]
pub struct JournalMapCouplingResult<T> {
    /// The protocol execution result
    pub result: T,
    /// JournalMap operations that were applied
    pub journal_ops_applied: Vec<JournalMapOperation>,
    /// Updated journal state after coupling
    pub updated_journal: JournalMap,
    /// Coupling metrics
    pub coupling_metrics: CouplingMetrics,
}

/// Metrics for journal coupling operations
#[derive(Debug, Default)]
pub struct CouplingMetrics {
    /// Time spent applying journal operations (microseconds)
    pub journal_application_time_us: u64,
    /// Number of journal operations applied
    pub operations_applied: usize,
    /// Number of retry attempts
    pub retry_attempts: usize,
    /// Whether coupling was successful
    pub coupling_successful: bool,
}

/// JournalMap operation types for proper CRDT integration
#[derive(Debug, Clone, PartialEq)]
pub enum JournalOperation {
    /// Merge facts into the journal (join semilattice operation)
    MergeFacts {
        /// Facts to merge
        facts: JournalMap,
        /// Operation description
        description: String,
    },
    /// Refine capabilities (meet semilattice operation)
    RefineCapabilities {
        /// Capability refinement
        refinement: JournalMap,
        /// Operation description
        description: String,
    },
    /// General journal merge (both facts and capabilities)
    GeneralMerge {
        /// JournalMap delta to merge
        delta: JournalMap,
        /// Operation description
        description: String,
    },
    /// Custom application-specific journal operation
    CustomOperation {
        /// Operation name
        name: String,
        /// Operation data
        data: JsonValue,
        /// Operation description
        description: String,
    },
}

impl JournalMapCoupler {
    /// Create a new journal coupler with default settings
    pub fn new() -> Self {
        Self {
            annotations: HashMap::new(),
            optimistic_application: false, // Default to pessimistic for safety
            max_retry_attempts: 3,
        }
    }

    /// Create a journal coupler with optimistic application enabled
    pub fn optimistic() -> Self {
        Self {
            optimistic_application: true,
            ..Self::new()
        }
    }

    /// Add a journal annotation for an operation
    pub fn add_annotation(
        &mut self,
        operation_id: String,
        annotation: JournalMapAnnotation,
    ) -> &mut Self {
        self.annotations.insert(operation_id, annotation);
        self
    }

    /// Add multiple journal annotations
    pub fn add_annotations(
        &mut self,
        annotations: HashMap<String, JournalMapAnnotation>,
    ) -> &mut Self {
        self.annotations.extend(annotations);
        self
    }

    /// Execute a protocol operation with journal coupling
    ///
    /// This method implements the complete journal coupling semantics:
    /// 1. Execute the protocol operation
    /// 2. On success, apply journal annotations atomically
    /// 3. Return both the operation result and journal coupling result
    pub async fn execute_with_coupling<T, F, Fut>(
        &self,
        operation_id: &str,
        effect_system: &mut AuraEffectSystem,
        operation: F,
    ) -> AuraResult<JournalMapCouplingResult<T>>
    where
        F: FnOnce(&mut AuraEffectSystem) -> Fut,
        Fut: Future<Output = AuraResult<T>>,
    {
        let coupling_start = Instant::now();

        debug!(
            operation_id = operation_id,
            optimistic = self.optimistic_application,
            "Starting journal-coupled execution"
        );

        // Get current journal state - use JournalMapMap as the initial journal for now
        let initial_journal = effect_system.get_journal_state().await?;

        if self.optimistic_application {
            self.execute_optimistic(operation_id, effect_system, operation, initial_journal)
                .await
        } else {
            self.execute_pessimistic(operation_id, effect_system, operation, initial_journal)
                .await
        }
    }

    /// Execute with optimistic journal application (apply deltas first)
    async fn execute_optimistic<T, F, Fut>(
        &self,
        operation_id: &str,
        effect_system: &mut AuraEffectSystem,
        operation: F,
        initial_journal: JournalMap,
    ) -> AuraResult<JournalMapCouplingResult<T>>
    where
        F: FnOnce(&mut AuraEffectSystem) -> Fut,
        Fut: Future<Output = AuraResult<T>>,
    {
        let application_start = Instant::now();

        // Phase 1: Apply journal annotations optimistically
        let (updated_journal, journal_ops) =
            self.apply_annotations(operation_id, effect_system, &initial_journal)
                .await?;

        let journal_application_time = application_start.elapsed();

        // Phase 2: Execute the protocol operation
        let execution_result = operation(effect_system).await;

        match execution_result {
            Ok(result) => {
                info!(
                    operation_id = operation_id,
                    journal_ops_applied = journal_ops.len(),
                    "Optimistic journal coupling successful"
                );

                Ok(JournalMapCouplingResult {
                    result,
                    journal_ops_applied: journal_ops.clone(),
                    updated_journal,
                    coupling_metrics: CouplingMetrics {
                        journal_application_time_us: journal_application_time.as_micros() as u64,
                        operations_applied: journal_ops.len(),
                        retry_attempts: 0,
                        coupling_successful: true,
                    },
                })
            }
            Err(e) => {
                warn!(
                    operation_id = operation_id,
                    error = %e,
                    "Operation failed after optimistic journal application"
                );

                // In optimistic mode, we don't roll back journal changes
                // The journal changes are considered committed
                // This follows CRDT semantics where operations are monotonic

                Err(e)
            }
        }
    }

    /// Execute with pessimistic journal application (apply deltas after operation succeeds)
    async fn execute_pessimistic<T, F, Fut>(
        &self,
        operation_id: &str,
        effect_system: &mut AuraEffectSystem,
        operation: F,
        initial_journal: JournalMap,
    ) -> AuraResult<JournalMapCouplingResult<T>>
    where
        F: FnOnce(&mut AuraEffectSystem) -> Fut,
        Fut: Future<Output = AuraResult<T>>,
    {
        // Phase 1: Execute the protocol operation first
        let execution_result = operation(effect_system).await?;

        // Phase 2: Apply journal annotations only after success
        let application_start = Instant::now();
        let (updated_journal, journal_ops) =
            self.apply_annotations(operation_id, effect_system, &initial_journal)
                .await?;

        let journal_application_time = application_start.elapsed();

        info!(
            operation_id = operation_id,
            journal_ops_applied = journal_ops.len(),
            "Pessimistic journal coupling successful"
        );

        Ok(JournalMapCouplingResult {
            result: execution_result,
            journal_ops_applied: journal_ops.clone(),
            updated_journal,
            coupling_metrics: CouplingMetrics {
                journal_application_time_us: journal_application_time.as_micros() as u64,
                operations_applied: journal_ops.len(),
                retry_attempts: 0,
                coupling_successful: true,
            },
        })
    }

    /// Apply journal annotations for an operation
    async fn apply_annotations(
        &self,
        operation_id: &str,
        effect_system: &mut AuraEffectSystem,
        initial_journal: &JournalMap,
    ) -> AuraResult<(JournalMap, Vec<JournalMapOperation>)> {
        // Check if there are annotations for this operation
        let annotation = match self.annotations.get(operation_id) {
            Some(annotation) => annotation,
            None => {
                debug!(
                    operation_id = operation_id,
                    "No journal annotations for operation"
                );
                return Ok((initial_journal.clone(), Vec::new()));
            }
        };

        let mut current_journal = initial_journal.clone();
        let mut applied_ops = Vec::new();

        // Apply the annotation with retry logic
        for attempt in 0..self.max_retry_attempts {
            match self.apply_single_annotation(annotation, effect_system, &current_journal)
                .await
            {
                Ok((updated_journal, journal_op)) => {
                    current_journal = updated_journal;
                    applied_ops.push(journal_op);
                    break;
                }
                Err(e) => {
                    if attempt == self.max_retry_attempts - 1 {
                        error!(
                            operation_id = operation_id,
                            attempt = attempt + 1,
                            error = %e,
                            "JournalMap annotation application failed after max retries"
                        );
                        return Err(e);
                    } else {
                        warn!(
                            operation_id = operation_id,
                            attempt = attempt + 1,
                            error = %e,
                            "JournalMap annotation application failed, retrying"
                        );
                        // Small delay before retry
                        tokio::time::sleep(tokio::time::Duration::from_millis(10 * (attempt + 1) as u64))
                            .await;
                    }
                }
            }
        }

        Ok((current_journal, applied_ops))
    }

    /// Apply a single journal annotation
    async fn apply_single_annotation(
        &self,
        annotation: &JournalMapAnnotation,
        effect_system: &mut AuraEffectSystem,
        current_journal: &JournalMap,
    ) -> AuraResult<(JournalMap, JournalMapOperation)> {
        match &annotation.op_type {
            JournalOpType::AddFacts => {
                if let Some(delta) = &annotation.delta {
                    // TODO: Implement merge_facts in AuraEffectSystem
                    let updated_journal = current_journal.clone();
                    let journal_op = JournalOperation::MergeFacts {
                        facts: delta.clone(),
                        description: annotation
                            .description
                            .clone()
                            .unwrap_or_else(|| "Add facts".to_string()),
                    };
                    Ok((updated_journal, journal_op))
                } else {
                    // No specific delta - return unchanged journal
                    Ok((current_journal.clone(), JournalOperation::MergeFacts {
                        facts: JournalMap::new(),
                        description: "No-op facts addition".to_string(),
                    }))
                }
            }
            JournalOpType::RefineCaps => {
                if let Some(refinement) = &annotation.delta {
                    // TODO: Implement refine_caps in AuraEffectSystem
                    let updated_journal = current_journal.clone();
                    let journal_op = JournalOperation::RefineCapabilities {
                        refinement: refinement.clone(),
                        description: annotation
                            .description
                            .clone()
                            .unwrap_or_else(|| "Refine capabilities".to_string()),
                    };
                    Ok((updated_journal, journal_op))
                } else {
                    Ok((current_journal.clone(), JournalOperation::RefineCapabilities {
                        refinement: JournalMap::new(),
                        description: "No-op capability refinement".to_string(),
                    }))
                }
            }
            JournalOpType::Merge => {
                if let Some(delta) = &annotation.delta {
                    // Apply both facts and capabilities
                    // TODO: Implement merge_facts and refine_caps in AuraEffectSystem
                    let with_facts = current_journal.clone();
                    let final_journal = with_facts;
                    let journal_op = JournalOperation::GeneralMerge {
                        delta: delta.clone(),
                        description: annotation
                            .description
                            .clone()
                            .unwrap_or_else(|| "General merge".to_string()),
                    };
                    Ok((final_journal, journal_op))
                } else {
                    Ok((current_journal.clone(), JournalOperation::GeneralMerge {
                        delta: JournalMap::new(),
                        description: "No-op general merge".to_string(),
                    }))
                }
            }
            JournalOpType::Custom(custom_op) => {
                // Custom operations are application-specific
                // For now, we don't apply any journal changes
                let journal_op = JournalOperation::CustomOperation {
                    name: custom_op.clone(),
                    data: serde_json::Value::Null,
                    description: annotation
                        .description
                        .clone()
                        .unwrap_or_else(|| format!("Custom operation: {}", custom_op)),
                };
                Ok((current_journal.clone(), journal_op))
            }
        }
    }
}

impl Default for JournalMapCoupler {
    fn default() -> Self {
        Self::new()
    }
}

/// Integration with the guard chain execution
impl ProtocolGuard {
    /// Execute with journal coupling integrated into the guard chain
    ///
    /// This method provides a complete CapGuard → FlowGuard → JournalMapCoupler execution path
    pub async fn execute_with_journal_coupling<T, F, Fut>(
        &self,
        effect_system: &mut AuraEffectSystem,
        journal_coupler: &JournalMapCoupler,
        operation: F,
    ) -> AuraResult<JournalMapCouplingResult<T>>
    where
        F: FnOnce(&mut AuraEffectSystem) -> Fut + Send,
        Fut: Future<Output = AuraResult<T>> + Send,
    {
        // For now, execute the journal coupling directly
        // TODO: Integrate with proper guard execution chain
        let coupling_result = journal_coupler.execute_with_coupling(&self.operation_id, effect_system, operation).await?;

        Ok(coupling_result)
    }
}

/// Builder for creating configured journal couplers
pub struct JournalMapCouplerBuilder {
    coupler: JournalMapCoupler,
}

impl JournalMapCouplerBuilder {
    /// Create a new journal coupler builder
    pub fn new() -> Self {
        Self {
            coupler: JournalMapCoupler::new(),
        }
    }

    /// Enable optimistic journal application
    pub fn optimistic(mut self) -> Self {
        self.coupler.optimistic_application = true;
        self
    }

    /// Set maximum retry attempts
    pub fn max_retries(mut self, attempts: usize) -> Self {
        self.coupler.max_retry_attempts = attempts;
        self
    }

    /// Add a journal annotation
    pub fn with_annotation(
        mut self,
        operation_id: String,
        annotation: JournalMapAnnotation,
    ) -> Self {
        self.coupler.add_annotation(operation_id, annotation);
        self
    }

    /// Build the configured journal coupler
    pub fn build(self) -> JournalMapCoupler {
        self.coupler
    }
}

impl Default for JournalMapCouplerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{semilattice::Bottom, DeviceId};
    use aura_mpst::journal_coupling::JournalMapAnnotation;
    use aura_protocol::handlers::ExecutionMode;

    #[tokio::test]
    async fn test_journal_coupler_creation() {
        let coupler = JournalMapCoupler::new();
        assert!(!coupler.optimistic_application);
        assert_eq!(coupler.max_retry_attempts, 3);
        assert!(coupler.annotations.is_empty());
    }

    #[tokio::test]
    async fn test_journal_coupler_builder() {
        let coupler = JournalMapCouplerBuilder::new()
            .optimistic()
            .max_retries(5)
            .with_annotation(
                "test_op".to_string(),
                JournalMapAnnotation::add_facts("Test fact addition"),
            )
            .build();

        assert!(coupler.optimistic_application);
        assert_eq!(coupler.max_retry_attempts, 5);
        assert!(coupler.annotations.contains_key("test_op"));
    }

    #[tokio::test]
    async fn test_journal_coupling_with_no_annotations() {
        let device_id = DeviceId::new();
        let mut effect_system = AuraEffectSystem::new(device_id, ExecutionMode::Testing);
        let coupler = JournalMapCoupler::new();

        let result = coupler
            .execute_with_coupling("no_annotation_op", &mut effect_system, |_| async {
                Ok(42u32)
            })
            .await
            .unwrap();

        assert_eq!(result.result, 42);
        assert!(result.journal_ops_applied.is_empty());
        assert!(result.coupling_metrics.coupling_successful);
    }

    #[tokio::test]
    async fn test_journal_coupling_with_facts_annotation() {
        let device_id = DeviceId::new();
        let mut effect_system = AuraEffectSystem::new(device_id, ExecutionMode::Testing);
        
        let mut coupler = JournalMapCoupler::new();
        let annotation = JournalMapAnnotation::with_delta(
            JournalOpType::AddFacts,
            JournalMap::new(), // Empty journal for testing
        );
        coupler.add_annotation("test_facts_op".to_string(), annotation);

        let result = coupler
            .execute_with_coupling("test_facts_op", &mut effect_system, |_| async {
                Ok("facts_applied".to_string())
            })
            .await
            .unwrap();

        assert_eq!(result.result, "facts_applied");
        assert!(!result.journal_ops_applied.is_empty());
        assert!(result.coupling_metrics.coupling_successful);
    }

    #[tokio::test]
    async fn test_optimistic_vs_pessimistic_execution() {
        let device_id = DeviceId::new();
        
        // Test pessimistic execution (default)
        let pessimistic_coupler = JournalMapCoupler::new();
        assert!(!pessimistic_coupler.optimistic_application);

        // Test optimistic execution
        let optimistic_coupler = JournalMapCoupler::optimistic();
        assert!(optimistic_coupler.optimistic_application);
    }
}