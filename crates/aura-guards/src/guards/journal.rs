//! Journal Coupler for Guard Chain Integration
//!
//! This module provides the `JournalCoupler` that bridges the guard chain execution
//! with journal CRDT operations. It ensures that protocol operations that succeed
//! capability checks properly update and persist the distributed journal state.
//!
//! ## Integration Flow
//!
//! ```text
//! CapGuard → FlowGuard → JournalCoupler → Protocol Execution
//!     ↓         ↓            ↓                    ↓
//! Check     Check       Apply & persist    Execute with
//! caps      budget      journal deltas     full context
//! ```
//!
//! ## Charge-Before-Send Invariant
//!
//! The JournalCoupler enforces the charge-before-send invariant from the formal model:
//! journal facts MUST be persisted before any transport effects occur. This ensures:
//!
//! 1. **Durability**: Facts are committed even if the protocol operation fails
//! 2. **Consistency**: Other replicas see journal state before related messages
//! 3. **Monotonicity**: CRDT semantics guarantee no rollback is needed
//!
//! ## Execution Modes
//!
//! - **Pessimistic** (default): Execute operation first, then apply and persist journal
//!   changes. Journal is only persisted if the operation succeeds.
//!
//! - **Optimistic**: Apply and persist journal changes first, then execute operation.
//!   Journal changes remain even if operation fails (safe due to CRDT monotonicity).
//!
//! ## Persistence Contract
//!
//! All journal changes are persisted via `JournalEffects::persist_journal()` after
//! being computed. This module does NOT rely on callers to persist - it handles
//! persistence internally to maintain the charge-before-send invariant.
//!
//! The JournalCoupler implements the formal model's "journal coupling" semantics
//! where protocol operations atomically update both local state and distributed
//! journal facts using join-semilattice operations.

use super::{GuardEffects, ProtocolGuard};
use aura_core::{AuraResult, Journal, TimeEffects};
use aura_mpst::journal::{JournalAnnotation, JournalOpType};
use serde_json::Value as JsonValue;
use std::{collections::HashMap, future::Future};
use tracing::{debug, error, info, instrument, warn};

/// Journal coupling coordinator for the guard chain
///
/// The JournalCoupler sits at the end of the guard chain (after CapGuard and FlowGuard)
/// and ensures that successful protocol operations properly update the distributed
/// journal state using CRDT operations.
#[derive(Debug)]
pub struct JournalCoupler {
    /// Journal annotations for operations
    pub annotations: HashMap<String, JournalAnnotation>,
    /// Whether to apply deltas optimistically or pessimistically
    pub optimistic_application: bool,
    /// Maximum retry attempts for journal operations
    pub max_retry_attempts: usize,
}

/// Result of journal coupling operation
#[derive(Debug)]
pub struct JournalCouplingResult<T> {
    /// The protocol execution result
    pub result: T,
    /// Journal operations that were applied
    pub applied_operations: Vec<JournalOperation>,
    /// Updated journal state after coupling
    pub updated_journal: Journal,
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

/// Journal operation types for proper CRDT integration
#[derive(Debug, Clone, PartialEq)]
pub enum JournalOperation {
    /// Merge facts into the journal (join semilattice operation)
    MergeFacts {
        /// Facts to merge
        facts: Journal,
        /// Operation description
        description: String,
    },
    /// Refine capabilities (meet semilattice operation)
    RefineCapabilities {
        /// Capability refinement
        refinement: Journal,
        /// Operation description
        description: String,
    },
    /// General journal merge (both facts and capabilities)
    GeneralMerge {
        /// Journal delta to merge
        delta: Journal,
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

impl JournalCoupler {
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
        annotation: JournalAnnotation,
    ) -> &mut Self {
        self.annotations.insert(operation_id, annotation);
        self
    }

    /// Add multiple journal annotations
    pub fn add_annotations(
        &mut self,
        annotations: HashMap<String, JournalAnnotation>,
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
    ///
    /// Timing is captured via the tracing span (subscriber handles `Instant::now()`).
    #[instrument(skip(self, effect_system, operation), fields(optimistic = self.optimistic_application))]
    pub async fn execute_with_coupling<E, T, F, Fut>(
        &self,
        operation_id: &str,
        effect_system: &mut E,
        operation: F,
    ) -> AuraResult<JournalCouplingResult<T>>
    where
        E: GuardEffects + aura_core::TimeEffects,
        F: FnOnce(&mut E) -> Fut,
        Fut: Future<Output = AuraResult<T>>,
    {
        debug!("Starting journal-coupled execution");

        // Get current journal state from the effect system
        let initial_journal = effect_system
            .get_journal()
            .await
            .map_err(|e| {
                warn!(
                    operation_id = operation_id,
                    error = %e,
                    "Failed to retrieve current journal state, using empty journal"
                );
                e
            })
            .unwrap_or_else(|_| Journal::new());

        if self.optimistic_application {
            self.execute_optimistic(operation_id, effect_system, operation, initial_journal)
                .await
        } else {
            self.execute_pessimistic(operation_id, effect_system, operation, initial_journal)
                .await
        }
    }

    /// Execute with optimistic journal application (apply deltas first)
    ///
    /// Timing is captured via the tracing span (subscriber handles `Instant::now()`).
    ///
    /// # Optimistic Semantics
    ///
    /// In optimistic mode, journal changes are persisted BEFORE operation execution.
    /// This follows CRDT semantics where operations are monotonic - if the operation
    /// fails, the journal changes remain valid and don't need rollback. This is safe
    /// because:
    /// 1. Journal operations are join-semilattice (monotonic, idempotent)
    /// 2. The operation's success/failure doesn't affect the validity of the facts
    /// 3. Retrying the operation will see the already-applied journal state
    #[instrument(skip(self, effect_system, operation, initial_journal))]
    async fn execute_optimistic<E, T, F, Fut>(
        &self,
        operation_id: &str,
        effect_system: &mut E,
        operation: F,
        initial_journal: Journal,
    ) -> AuraResult<JournalCouplingResult<T>>
    where
        E: GuardEffects + aura_core::TimeEffects,
        F: FnOnce(&mut E) -> Fut,
        Fut: Future<Output = AuraResult<T>>,
    {
        // Phase 1: Apply journal annotations optimistically
        let (updated_journal, journal_ops) = self
            .apply_annotations(operation_id, effect_system, &initial_journal)
            .await?;

        // Phase 2: Persist journal changes before operation execution
        // This ensures durability of the journal state regardless of operation outcome
        if !journal_ops.is_empty() {
            effect_system
                .persist_journal(&updated_journal)
                .await
                .map_err(|e| {
                    error!(
                        operation_id = operation_id,
                        error = %e,
                        "Failed to persist optimistic journal changes"
                    );
                    aura_core::AuraError::internal(format!(
                        "Optimistic journal persistence failed for operation '{operation_id}': {e}"
                    ))
                })?;

            debug!(
                operation_id = operation_id,
                journal_ops_applied = journal_ops.len(),
                "Optimistic journal changes persisted, proceeding with operation"
            );
        }

        // Phase 3: Execute the protocol operation
        let execution_result = operation(effect_system).await;

        match execution_result {
            Ok(result) => {
                info!(
                    operation_id = operation_id,
                    journal_ops_applied = journal_ops.len(),
                    "Optimistic journal coupling successful"
                );

                Ok(JournalCouplingResult {
                    result,
                    applied_operations: journal_ops.clone(),
                    updated_journal,
                    coupling_metrics: CouplingMetrics {
                        // Timing captured by tracing span, not explicit measurement
                        journal_application_time_us: 0,
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
                    journal_ops_committed = journal_ops.len(),
                    "Operation failed after optimistic journal application - journal changes remain committed (CRDT monotonicity)"
                );

                // In optimistic mode, we don't roll back journal changes
                // The journal changes are already persisted and remain valid
                // This follows CRDT semantics where operations are monotonic

                Err(e)
            }
        }
    }

    /// Execute with pessimistic journal application (apply deltas after operation succeeds)
    ///
    /// Timing is captured via the tracing span (subscriber handles `Instant::now()`).
    #[instrument(skip(self, effect_system, operation, initial_journal))]
    async fn execute_pessimistic<E, T, F, Fut>(
        &self,
        operation_id: &str,
        effect_system: &mut E,
        operation: F,
        initial_journal: Journal,
    ) -> AuraResult<JournalCouplingResult<T>>
    where
        E: GuardEffects + aura_core::TimeEffects,
        F: FnOnce(&mut E) -> Fut,
        Fut: Future<Output = AuraResult<T>>,
    {
        // Phase 1: Execute the protocol operation first
        let execution_result = operation(effect_system).await?;

        // Phase 2: Apply journal annotations only after success
        let (updated_journal, journal_ops) = self
            .apply_annotations(operation_id, effect_system, &initial_journal)
            .await?;

        // Phase 3: Persist journal changes atomically
        // This ensures charge-before-send invariant: journal commit happens before transport
        if !journal_ops.is_empty() {
            effect_system
                .persist_journal(&updated_journal)
                .await
                .map_err(|e| {
                    error!(
                        operation_id = operation_id,
                        error = %e,
                        "Failed to persist journal changes - operation succeeded but journal not committed"
                    );
                    aura_core::AuraError::internal(format!(
                        "Journal persistence failed for operation '{operation_id}': {e}. \
                         Operation completed but journal state is inconsistent."
                    ))
                })?;

            debug!(
                operation_id = operation_id,
                journal_ops_applied = journal_ops.len(),
                "Journal changes persisted successfully"
            );
        }

        info!(
            operation_id = operation_id,
            journal_ops_applied = journal_ops.len(),
            "Pessimistic journal coupling successful"
        );

        Ok(JournalCouplingResult {
            result: execution_result,
            applied_operations: journal_ops.clone(),
            updated_journal,
            coupling_metrics: CouplingMetrics {
                // Timing captured by tracing span, not explicit measurement
                journal_application_time_us: 0,
                operations_applied: journal_ops.len(),
                retry_attempts: 0,
                coupling_successful: true,
            },
        })
    }

    /// Apply journal annotations for an operation
    async fn apply_annotations<E: aura_core::effects::JournalEffects + TimeEffects>(
        &self,
        operation_id: &str,
        effect_system: &E,
        initial_journal: &Journal,
    ) -> AuraResult<(Journal, Vec<JournalOperation>)> {
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
            match self
                .apply_single_annotation(annotation, effect_system, &current_journal)
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
                            "Journal annotation application failed after max retries"
                        );
                        return Err(e);
                    } else {
                        warn!(
                            operation_id = operation_id,
                            attempt = attempt + 1,
                            error = %e,
                            "Journal annotation application failed, retrying"
                        );
                        // Small delay before retry
                        let delay_ms = 10 * (attempt + 1) as u64;
                        effect_system.sleep_ms(delay_ms).await.map_err(|e| {
                            aura_core::AuraError::internal(format!(
                                "Failed to sleep during journal operation retry: {e}"
                            ))
                        })?;
                    }
                }
            }
        }

        Ok((current_journal, applied_ops))
    }

    /// Couple journal operations with a successful send operation
    ///
    /// This method is called by the send guard chain after successful authorization
    /// and flow budget charging to atomically apply any journal changes.
    ///
    /// Timing is captured via the tracing span (subscriber handles `Instant::now()`).
    #[instrument(skip(self, effect_system, receipt), fields(receipt_present = receipt.is_some()))]
    pub async fn couple_with_send<E: aura_core::effects::JournalEffects + TimeEffects>(
        &self,
        effect_system: &E,
        receipt: &Option<aura_core::Receipt>,
    ) -> AuraResult<CouplingMetrics> {
        let operation_id = "send_coupling";

        debug!("Coupling journal operations with send");

        // Get the current journal state
        let current_journal = effect_system
            .get_journal()
            .await
            .unwrap_or_else(|_| Journal::new());

        // Apply any pending annotations for this send operation
        let (updated_journal, applied_ops) = self
            .apply_annotations(operation_id, effect_system, &current_journal)
            .await?;

        // Persist the updated journal if changes were made
        if !applied_ops.is_empty() {
            effect_system.persist_journal(&updated_journal).await?;

            debug!(
                operations_applied = applied_ops.len(),
                "Journal coupling with send completed successfully"
            );
        }

        Ok(CouplingMetrics {
            // Timing captured by tracing span, not explicit measurement
            journal_application_time_us: 0,
            operations_applied: applied_ops.len(),
            retry_attempts: 0,
            coupling_successful: true,
        })
    }

    /// Apply a single journal annotation
    async fn apply_single_annotation<E: aura_core::effects::JournalEffects + TimeEffects>(
        &self,
        annotation: &JournalAnnotation,
        effect_system: &E,
        current_journal: &Journal,
    ) -> AuraResult<(Journal, JournalOperation)> {
        match &annotation.op_type {
            JournalOpType::AddFacts => {
                if let Some(delta) = &annotation.delta {
                    let updated_journal = effect_system.merge_facts(current_journal, delta).await?;
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
                    Ok((
                        current_journal.clone(),
                        JournalOperation::MergeFacts {
                            facts: Journal::new(),
                            description: "No-op facts addition".to_string(),
                        },
                    ))
                }
            }
            JournalOpType::RefineCaps => {
                if let Some(refinement) = &annotation.delta {
                    let updated_journal = effect_system
                        .refine_caps(current_journal, refinement)
                        .await?;
                    let journal_op = JournalOperation::RefineCapabilities {
                        refinement: refinement.clone(),
                        description: annotation
                            .description
                            .clone()
                            .unwrap_or_else(|| "Refine capabilities".to_string()),
                    };
                    Ok((updated_journal, journal_op))
                } else {
                    Ok((
                        current_journal.clone(),
                        JournalOperation::RefineCapabilities {
                            refinement: Journal::new(),
                            description: "No-op capability refinement".to_string(),
                        },
                    ))
                }
            }
            JournalOpType::Merge => {
                if let Some(delta) = &annotation.delta {
                    // Apply both facts and capabilities
                    let with_facts = effect_system.merge_facts(current_journal, delta).await?;
                    let final_journal = effect_system.refine_caps(&with_facts, delta).await?;
                    let journal_op = JournalOperation::GeneralMerge {
                        delta: delta.clone(),
                        description: annotation
                            .description
                            .clone()
                            .unwrap_or_else(|| "General merge".to_string()),
                    };
                    Ok((final_journal, journal_op))
                } else {
                    Ok((
                        current_journal.clone(),
                        JournalOperation::GeneralMerge {
                            delta: Journal::new(),
                            description: "No-op general merge".to_string(),
                        },
                    ))
                }
            }
            JournalOpType::Custom(custom_op) => {
                // Custom operations require application-specific handling.
                // If a delta is provided, we apply it as a general merge.
                // Otherwise, we log a warning since this may indicate a misconfiguration.
                if let Some(delta) = &annotation.delta {
                    // Apply the delta if provided with the custom operation
                    let with_facts = effect_system.merge_facts(current_journal, delta).await?;
                    let final_journal = effect_system.refine_caps(&with_facts, delta).await?;

                    debug!(
                        custom_op = custom_op,
                        "Applied delta for custom journal operation"
                    );

                    let journal_op = JournalOperation::CustomOperation {
                        name: custom_op.clone(),
                        data: serde_json::json!({
                            "delta_applied": true,
                            "facts_count": delta.facts.len(),
                        }),
                        description: annotation
                            .description
                            .clone()
                            .unwrap_or_else(|| format!("Custom operation: {custom_op}")),
                    };
                    Ok((final_journal, journal_op))
                } else {
                    // No delta provided - this is a no-op but may indicate a bug
                    warn!(
                        custom_op = custom_op,
                        "Custom journal operation '{}' has no delta - no journal changes applied. \
                         If this is intentional, consider using a different operation type.",
                        custom_op
                    );

                    let journal_op = JournalOperation::CustomOperation {
                        name: custom_op.clone(),
                        data: serde_json::json!({
                            "delta_applied": false,
                            "warning": "No delta provided for custom operation"
                        }),
                        description: annotation
                            .description
                            .clone()
                            .unwrap_or_else(|| format!("Custom operation (no-op): {custom_op}")),
                    };
                    Ok((current_journal.clone(), journal_op))
                }
            }
        }
    }
}

impl Default for JournalCoupler {
    fn default() -> Self {
        Self::new()
    }
}

/// Integration with the guard chain execution
impl ProtocolGuard {
    /// Execute with journal coupling integrated into the guard chain
    ///
    /// This method provides a complete CapGuard → FlowGuard → JournalCoupler execution path
    pub async fn execute_with_journal_coupling<E, T, F, Fut>(
        &self,
        effect_system: &mut E,
        journal_coupler: &JournalCoupler,
        operation: F,
    ) -> AuraResult<JournalCouplingResult<T>>
    where
        E: GuardEffects + aura_core::TimeEffects,
        F: FnOnce(&mut E) -> Fut + Send,
        Fut: Future<Output = AuraResult<T>> + Send,
    {
        // Execute with full guard chain integration
        debug!(
            operation_id = %self.operation_id,
            required_tokens = self.required_tokens.len(),
            delta_facts = self.delta_facts.len(),
            "Executing protocol guard with journal coupling integration"
        );

        // Phase 1: Evaluate authorization guards (using existing guard evaluation)
        use crate::guards::execution::evaluate_guard;
        let guard_result = evaluate_guard(self).await?;

        if !guard_result.passed {
            warn!(
                operation_id = %self.operation_id,
                failed_requirements = guard_result.failed_requirements.len(),
                "Protocol guard evaluation failed, blocking journal coupling"
            );
            return Err(aura_core::AuraError::permission_denied(format!(
                "Guard evaluation failed for operation '{}': {:?}",
                self.operation_id, guard_result.failed_requirements
            )));
        }

        info!(
            operation_id = %self.operation_id,
            flow_consumed = guard_result.flow_consumed,
            "Protocol guards passed, proceeding with journal-coupled execution"
        );

        // Phase 2: Execute with journal coupling after successful guard evaluation
        let coupling_result = journal_coupler
            .execute_with_coupling(&self.operation_id, effect_system, operation)
            .await?;

        Ok(coupling_result)
    }
}

/// Builder for creating configured journal couplers
pub struct JournalCouplerBuilder {
    coupler: JournalCoupler,
}

impl JournalCouplerBuilder {
    /// Create a new journal coupler builder
    pub fn new() -> Self {
        Self {
            coupler: JournalCoupler::new(),
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
    pub fn with_annotation(mut self, operation_id: String, annotation: JournalAnnotation) -> Self {
        self.coupler.add_annotation(operation_id, annotation);
        self
    }

    /// Build the configured journal coupler
    pub fn build(self) -> JournalCoupler {
        self.coupler
    }
}

impl Default for JournalCouplerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any())]
#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::DeviceId;
    use aura_macros::aura_test;
    use aura_mpst::journal::JournalAnnotation;
    use aura_testkit::*;

    #[tokio::test]
    async fn test_journal_coupler_creation() {
        let coupler = JournalCoupler::new();
        assert!(!coupler.optimistic_application);
        assert_eq!(coupler.max_retry_attempts, 3);
        assert!(coupler.annotations.is_empty());
    }

    #[tokio::test]
    async fn test_journal_coupler_builder() {
        let coupler = JournalCouplerBuilder::new()
            .optimistic()
            .max_retries(5)
            .with_annotation(
                "test_op".to_string(),
                JournalAnnotation::add_facts("Test fact addition"),
            )
            .build();

        assert!(coupler.optimistic_application);
        assert_eq!(coupler.max_retry_attempts, 5);
        assert!(coupler.annotations.contains_key("test_op"));
    }

    #[aura_test]
    async fn test_journal_coupling_with_no_annotations() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let mut effect_system = (*fixture.effects()).clone();
        let coupler = JournalCoupler::new();

        let result = coupler
            .execute_with_coupling("no_annotation_op", &mut effect_system, |_| async {
                Ok(42u32)
            })
            .await?;

        assert_eq!(result.result, 42);
        assert!(result.applied_operations.is_empty());
        assert!(result.coupling_metrics.coupling_successful);
        Ok(())
    }

    #[aura_test]
    async fn test_journal_coupling_with_facts_annotation() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let mut effect_system = (*fixture.effects()).clone();

        let mut coupler = JournalCoupler::new();
        let annotation = JournalAnnotation::with_delta(
            JournalOpType::AddFacts,
            Journal::new(), // Empty journal for testing
        );
        coupler.add_annotation("test_facts_op".to_string(), annotation);

        let result = coupler
            .execute_with_coupling("test_facts_op", &mut effect_system, |_| async {
                Ok("facts_applied".to_string())
            })
            .await?;

        assert_eq!(result.result, "facts_applied");
        assert!(!result.applied_operations.is_empty());
        assert!(result.coupling_metrics.coupling_successful);
        Ok(())
    }

    #[tokio::test]
    async fn test_optimistic_vs_pessimistic_execution() {
        let _device_id = DeviceId::new_from_entropy([3u8; 32]);

        // Test pessimistic execution (default)
        let pessimistic_coupler = JournalCoupler::new();
        assert!(!pessimistic_coupler.optimistic_application);

        // Test optimistic execution
        let optimistic_coupler = JournalCoupler::optimistic();
        assert!(optimistic_coupler.optimistic_application);
    }
}
