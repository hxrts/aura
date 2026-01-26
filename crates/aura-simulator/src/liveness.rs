//! Bounded Liveness Checking Infrastructure
//!
//! This module provides infrastructure for checking bounded liveness properties
//! during simulation. Liveness properties assert that "something good eventually
//! happens" - this module allows specifying step bounds within which the property
//! must become true.
//!
//! ## Quint Correspondence
//!
//! This module integrates with the Quint liveness specifications:
//! - `verification/quint/liveness/properties.qnt` - Termination, progress, timing bounds
//! - `verification/quint/liveness/timing.qnt` - Synchrony model (GST, delta)
//! - `verification/quint/liveness/connectivity.qnt` - Gossip graph connectivity
//!
//! ## Rust Correspondence
//!
//! - `crates/aura-simulator/src/quint/properties.rs` - Property extraction
//! - `crates/aura-simulator/src/quint/generative_simulator.rs` - Step-based simulation
//!
//! See: docs/004_distributed_systems_contract.md §4, docs/106_consensus.md §15

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// =============================================================================
// Core Types
// =============================================================================

/// Bounded liveness property that must become true within a step bound.
///
/// Unlike safety properties (which must always hold), liveness properties
/// specify progress requirements. A bounded liveness property adds a finite
/// step limit within which progress must occur.
///
/// ## Example
///
/// ```ignore
/// BoundedLivenessProperty {
///     name: "consensus_terminates".to_string(),
///     description: "All consensus instances complete within bound".to_string(),
///     precondition: "gstReached".to_string(),
///     goal: "allInstancesTerminated(instances)".to_string(),
///     step_bound: 20,
///     ..Default::default()
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundedLivenessProperty {
    /// Property name (e.g., "consensus_terminates")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Precondition that must hold for liveness to apply
    /// (e.g., "gstReached" for partial synchrony)
    pub precondition: String,
    /// Goal condition that must eventually become true
    /// (e.g., "allInstancesTerminated(instances)")
    pub goal: String,
    /// Maximum steps within which goal must be reached
    pub step_bound: u32,
    /// Source location in Quint spec
    pub source_location: String,
    /// Tags for categorization
    pub tags: Vec<String>,
}

impl Default for BoundedLivenessProperty {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            precondition: "true".to_string(),
            goal: "true".to_string(),
            step_bound: 100,
            source_location: String::new(),
            tags: Vec::new(),
        }
    }
}

/// Result of checking a bounded liveness property.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LivenessCheckResult {
    /// Property that was checked
    pub property_name: String,
    /// Whether the property was satisfied
    pub satisfied: bool,
    /// Step at which precondition became true (if ever)
    pub precondition_step: Option<u64>,
    /// Step at which goal was achieved (if ever)
    pub goal_step: Option<u64>,
    /// Total steps executed
    pub total_steps: u64,
    /// Steps elapsed between precondition and goal (or bound violation)
    pub steps_to_goal: Option<u64>,
    /// Detailed explanation
    pub details: String,
    /// Witness state (counterexample if violated)
    pub witness: Option<Value>,
}

/// Synchrony assumption for liveness checking.
///
/// Corresponds to Quint `protocol_liveness_timing` module.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SynchronyAssumption {
    /// Asynchronous: no timing guarantees
    Asynchronous,
    /// Partial synchrony: GST eventually reached, then bounded delay
    PartialSynchrony {
        /// Global Stabilization Time (step index)
        gst: u64,
        /// Message delay bound after GST
        delta: u64,
    },
    /// Full synchrony: bounded delays from the start
    Synchronous {
        /// Message delay bound
        delta: u64,
    },
}

impl Default for SynchronyAssumption {
    fn default() -> Self {
        // Default to partial synchrony matching Quint timing.qnt
        Self::PartialSynchrony {
            gst: 0,
            delta: 3, // DELTA from timing.qnt
        }
    }
}

// =============================================================================
// Bounded Liveness Checker
// =============================================================================

/// Checker for bounded liveness properties during simulation.
///
/// This checker monitors simulation execution and evaluates when bounded
/// liveness properties are satisfied or violated. It integrates with the
/// Quint temporal property specifications.
///
/// ## Usage
///
/// ```ignore
/// let mut checker = BoundedLivenessChecker::new();
/// checker.add_property(BoundedLivenessProperty {
///     name: "fast_path_bound".to_string(),
///     precondition: "fastPathActive and gstReached".to_string(),
///     goal: "committed or fallbackActive".to_string(),
///     step_bound: 6, // 2 * DELTA
///     ..Default::default()
/// });
///
/// for step in simulation {
///     checker.check_step(step.index, &step.state)?;
/// }
///
/// let results = checker.finalize();
/// ```
#[derive(Debug)]
pub struct BoundedLivenessChecker {
    /// Properties being checked
    properties: Vec<BoundedLivenessProperty>,
    /// Tracking state for each property
    tracking: HashMap<String, PropertyTracking>,
    /// Synchrony assumption
    synchrony: SynchronyAssumption,
    /// Whether verbose logging is enabled
    verbose: bool,
    /// Current step index
    current_step: u64,
}

/// Internal tracking state for a property.
#[derive(Debug, Clone, Default)]
struct PropertyTracking {
    /// Step when precondition first became true
    precondition_step: Option<u64>,
    /// Step when goal was achieved
    goal_step: Option<u64>,
    /// Whether precondition is currently true
    precondition_active: bool,
    /// Steps elapsed since precondition
    steps_since_precondition: u64,
}

impl BoundedLivenessChecker {
    /// Create a new bounded liveness checker.
    pub fn new() -> Self {
        Self {
            properties: Vec::new(),
            tracking: HashMap::new(),
            synchrony: SynchronyAssumption::default(),
            verbose: false,
            current_step: 0,
        }
    }

    /// Create with specific synchrony assumption.
    pub fn with_synchrony(synchrony: SynchronyAssumption) -> Self {
        Self {
            synchrony,
            ..Self::new()
        }
    }

    /// Enable verbose logging.
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Add a bounded liveness property to check.
    pub fn add_property(&mut self, property: BoundedLivenessProperty) {
        let name = property.name.clone();
        self.properties.push(property);
        self.tracking.insert(name, PropertyTracking::default());
    }

    /// Add standard consensus liveness properties from Quint specs.
    ///
    /// These correspond to properties in `verification/quint/liveness/properties.qnt`.
    pub fn add_consensus_properties(&mut self) {
        // Fast path timing bound (2δ)
        // Quint: temporalFastPathBound
        self.add_property(BoundedLivenessProperty {
            name: "fast_path_bound".to_string(),
            description: "Fast path completes within 2δ when responsive conditions hold"
                .to_string(),
            precondition: "fastPathActive and gstReached and allWitnessesOnlineHonest".to_string(),
            goal: "committed or fallbackActive".to_string(),
            step_bound: match &self.synchrony {
                SynchronyAssumption::PartialSynchrony { delta, .. } => (2 * delta) as u32,
                SynchronyAssumption::Synchronous { delta } => (2 * delta) as u32,
                SynchronyAssumption::Asynchronous => 100, // Unbounded fallback
            },
            source_location: "verification/quint/liveness/properties.qnt:127".to_string(),
            tags: vec!["fast_path".to_string(), "timing".to_string()],
        });

        // Consensus termination
        // Quint: allInstancesTerminated
        self.add_property(BoundedLivenessProperty {
            name: "consensus_terminates".to_string(),
            description: "All consensus instances terminate under synchrony with honest quorum"
                .to_string(),
            precondition: "gstReached and hasQuorumOnline".to_string(),
            goal: "allInstancesTerminated".to_string(),
            step_bound: match &self.synchrony {
                SynchronyAssumption::PartialSynchrony { delta, .. } => (10 * delta) as u32,
                SynchronyAssumption::Synchronous { delta } => (5 * delta) as u32,
                SynchronyAssumption::Asynchronous => 200,
            },
            source_location: "verification/quint/liveness/properties.qnt:25".to_string(),
            tags: vec!["termination".to_string(), "consensus".to_string()],
        });

        // Progress under synchrony
        // Quint: invariantProgressUnderSynchrony
        self.add_property(BoundedLivenessProperty {
            name: "progress_under_synchrony".to_string(),
            description: "Active instances make progress when GST reached and quorum online"
                .to_string(),
            precondition: "gstReached and isActiveWithQuorum".to_string(),
            goal: "proposals.size() > 0 or terminated".to_string(),
            step_bound: match &self.synchrony {
                SynchronyAssumption::PartialSynchrony { delta, .. } => (4 * delta) as u32,
                SynchronyAssumption::Synchronous { delta } => (2 * delta) as u32,
                SynchronyAssumption::Asynchronous => 50,
            },
            source_location: "verification/quint/liveness/properties.qnt:77".to_string(),
            tags: vec!["progress".to_string(), "synchrony".to_string()],
        });

        // No deadlock
        // Quint: noDeadlock
        self.add_property(BoundedLivenessProperty {
            name: "no_deadlock".to_string(),
            description: "Active instances always have an enabled action".to_string(),
            precondition: "isActive".to_string(),
            goal: "hasEnabledAction or terminated".to_string(),
            step_bound: 1, // Must be true immediately
            source_location: "verification/quint/liveness/properties.qnt:191".to_string(),
            tags: vec!["deadlock".to_string(), "safety".to_string()],
        });
    }

    /// Check properties at a simulation step.
    ///
    /// # Arguments
    /// * `step_index` - Current step index
    /// * `state` - Current simulation state as JSON
    ///
    /// # Returns
    /// List of property violations detected at this step
    pub fn check_step(&mut self, step_index: u64, state: &Value) -> Vec<LivenessCheckResult> {
        self.current_step = step_index;
        let mut violations = Vec::new();

        // Check GST for partial synchrony
        let gst_reached = self.is_gst_reached(step_index);

        for property in &self.properties {
            let name = &property.name;

            // Evaluate precondition and goal first (immutable borrow of self)
            let precondition_holds =
                self.evaluate_expression(&property.precondition, state, gst_reached);
            let goal_holds = self.evaluate_expression(&property.goal, state, gst_reached);

            // Now get mutable borrow for tracking update
            let tracking = self
                .tracking
                .get_mut(name)
                .expect("property tracking exists");

            // Update tracking
            if precondition_holds && tracking.precondition_step.is_none() {
                tracking.precondition_step = Some(step_index);
                tracking.precondition_active = true;
                if self.verbose {
                    println!("[Liveness] {name} precondition triggered at step {step_index}");
                }
            }

            if tracking.precondition_active {
                tracking.steps_since_precondition += 1;

                if goal_holds && tracking.goal_step.is_none() {
                    tracking.goal_step = Some(step_index);
                    if self.verbose {
                        println!(
                            "[Liveness] {} goal achieved at step {} ({} steps)",
                            name, step_index, tracking.steps_since_precondition
                        );
                    }
                }

                // Check for bound violation
                if tracking.goal_step.is_none()
                    && tracking.steps_since_precondition > property.step_bound as u64
                {
                    violations.push(LivenessCheckResult {
                        property_name: name.clone(),
                        satisfied: false,
                        precondition_step: tracking.precondition_step,
                        goal_step: None,
                        total_steps: step_index,
                        steps_to_goal: None,
                        details: format!(
                            "Bound exceeded: {} steps > {} bound",
                            tracking.steps_since_precondition, property.step_bound
                        ),
                        witness: Some(state.clone()),
                    });
                }
            }
        }

        violations
    }

    /// Finalize checking and return results for all properties.
    pub fn finalize(&self) -> Vec<LivenessCheckResult> {
        let mut results = Vec::new();

        for property in &self.properties {
            let tracking = self
                .tracking
                .get(&property.name)
                .expect("property tracking exists");

            let satisfied = if tracking.precondition_step.is_none() {
                // Precondition never triggered - property vacuously true
                true
            } else if let Some(goal_step) = tracking.goal_step {
                // Goal was achieved
                let steps = goal_step - tracking.precondition_step.unwrap_or(0);
                steps <= property.step_bound as u64
            } else {
                // Precondition triggered but goal never achieved
                false
            };

            let steps_to_goal = match (tracking.precondition_step, tracking.goal_step) {
                (Some(pre), Some(goal)) => Some(goal - pre),
                _ => None,
            };

            results.push(LivenessCheckResult {
                property_name: property.name.clone(),
                satisfied,
                precondition_step: tracking.precondition_step,
                goal_step: tracking.goal_step,
                total_steps: self.current_step,
                steps_to_goal,
                details: if satisfied {
                    if tracking.precondition_step.is_none() {
                        "Vacuously satisfied (precondition never triggered)".to_string()
                    } else {
                        format!("Goal achieved within {} steps", steps_to_goal.unwrap_or(0))
                    }
                } else {
                    format!(
                        "Goal not achieved within {} step bound",
                        property.step_bound
                    )
                },
                witness: None,
            });
        }

        results
    }

    /// Get property by name.
    pub fn get_property(&self, name: &str) -> Option<&BoundedLivenessProperty> {
        self.properties.iter().find(|p| p.name == name)
    }

    /// Get all registered properties.
    pub fn properties(&self) -> &[BoundedLivenessProperty] {
        &self.properties
    }

    /// Reset tracking state for all properties.
    pub fn reset(&mut self) {
        for (_, tracking) in self.tracking.iter_mut() {
            *tracking = PropertyTracking::default();
        }
        self.current_step = 0;
    }

    // =========================================================================
    // Internal Helpers
    // =========================================================================

    /// Check if GST has been reached based on synchrony assumption.
    fn is_gst_reached(&self, step: u64) -> bool {
        match &self.synchrony {
            SynchronyAssumption::Asynchronous => false,
            SynchronyAssumption::PartialSynchrony { gst, .. } => step >= *gst,
            SynchronyAssumption::Synchronous { .. } => true,
        }
    }

    /// Evaluate a property expression against state.
    ///
    /// This is a simplified evaluator for common patterns. Full evaluation
    /// would require the Quint interpreter.
    #[allow(clippy::only_used_in_recursion)] // &self needed for method call in recursive closures
    fn evaluate_expression(&self, expr: &str, state: &Value, gst_reached: bool) -> bool {
        let expr = expr.trim();

        // Handle "true" and "false" literals
        if expr == "true" {
            return true;
        }
        if expr == "false" {
            return false;
        }

        // Handle "and" expressions
        if expr.contains(" and ") {
            let parts: Vec<&str> = expr.split(" and ").collect();
            return parts
                .iter()
                .all(|p| self.evaluate_expression(p, state, gst_reached));
        }

        // Handle "or" expressions
        if expr.contains(" or ") {
            let parts: Vec<&str> = expr.split(" or ").collect();
            return parts
                .iter()
                .any(|p| self.evaluate_expression(p, state, gst_reached));
        }

        // Handle gstReached
        if expr == "gstReached" {
            return gst_reached;
        }

        // Handle state variable lookups
        if let Some(val) = state.get(expr) {
            return match val {
                Value::Bool(b) => *b,
                Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
                Value::String(s) => !s.is_empty(),
                Value::Array(a) => !a.is_empty(),
                Value::Object(o) => !o.is_empty(),
                Value::Null => false,
            };
        }

        // Handle function-like expressions and known keywords
        // Check both expressions with parens and simple keywords
        let func_name = if expr.contains('(') {
            expr.split('(').next().unwrap_or("")
        } else {
            expr // Treat the whole expression as a potential function name
        };

        // Check if this is a known function/keyword
        let known_funcs = [
            "allInstancesTerminated",
            "terminated",
            "hasQuorumOnline",
            "isActiveWithQuorum",
            "allWitnessesOnlineHonest",
            "isActive",
            "fastPathActive",
            "fallbackActive",
            "hasEnabledAction",
            "committed",
        ];

        if known_funcs.contains(&func_name) {
            match func_name {
                "allInstancesTerminated" | "terminated" => {
                    // Check if all instances have phase == Committed or Failed
                    if let Some(instances) = state.get("instances") {
                        if let Some(map) = instances.as_object() {
                            return map.values().all(|inst| {
                                if let Some(phase) = inst.get("phase") {
                                    let phase_str = phase.as_str().unwrap_or("");
                                    phase_str == "Committed"
                                        || phase_str == "ConsensusCommitted"
                                        || phase_str == "Failed"
                                        || phase_str == "ConsensusFailed"
                                } else {
                                    false
                                }
                            });
                        }
                    }
                    true // No instances = vacuously terminated
                }
                "hasQuorumOnline" | "isActiveWithQuorum" => {
                    // Simplified: check if quorum count in state
                    state
                        .get("hasQuorum")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true)
                }
                "allWitnessesOnlineHonest" => state
                    .get("allOnline")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                "isActive" | "fastPathActive" | "fallbackActive" => {
                    if let Some(phase) = state.get("phase") {
                        let phase_str = phase.as_str().unwrap_or("");
                        if func_name == "fastPathActive" {
                            phase_str == "FastPathActive"
                        } else if func_name == "fallbackActive" {
                            phase_str == "FallbackActive"
                        } else {
                            phase_str == "FastPathActive" || phase_str == "FallbackActive"
                        }
                    } else {
                        false
                    }
                }
                "hasEnabledAction" => {
                    // Assume there's always an enabled action unless explicitly marked
                    state
                        .get("deadlocked")
                        .and_then(|v| v.as_bool())
                        .map(|d| !d)
                        .unwrap_or(true)
                }
                "committed" => {
                    if let Some(phase) = state.get("phase") {
                        let phase_str = phase.as_str().unwrap_or("");
                        phase_str == "Committed" || phase_str == "ConsensusCommitted"
                    } else {
                        false
                    }
                }
                _ => {
                    // Unknown function - default to true to avoid false negatives
                    true
                }
            }
        } else {
            // Unknown expression - default to true
            true
        }
    }
}

impl Default for BoundedLivenessChecker {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Convenience Functions
// =============================================================================

/// Create a checker with standard consensus liveness properties.
pub fn consensus_liveness_checker(synchrony: SynchronyAssumption) -> BoundedLivenessChecker {
    let mut checker = BoundedLivenessChecker::with_synchrony(synchrony);
    checker.add_consensus_properties();
    checker
}

/// Check if consensus completes within N steps (convenience function).
///
/// This is the primary example for Task 8: "consensus completes within N steps
/// under partial synchrony".
///
/// # Arguments
/// * `steps` - Sequence of simulation states
/// * `bound` - Maximum steps allowed for completion
/// * `gst` - Global Stabilization Time step
///
/// # Returns
/// Whether all consensus instances terminated within the bound after GST
pub fn check_consensus_terminates_within(
    steps: &[Value],
    bound: u32,
    gst: u64,
) -> LivenessCheckResult {
    let synchrony = SynchronyAssumption::PartialSynchrony { gst, delta: 3 };
    let mut checker = BoundedLivenessChecker::with_synchrony(synchrony);

    checker.add_property(BoundedLivenessProperty {
        name: "consensus_terminates_within".to_string(),
        description: format!("Consensus terminates within {bound} steps after GST"),
        precondition: "gstReached".to_string(),
        goal: "allInstancesTerminated".to_string(),
        step_bound: bound,
        source_location: "bounded liveness check".to_string(),
        tags: vec!["termination".to_string()],
    });

    for (i, state) in steps.iter().enumerate() {
        checker.check_step(i as u64, state);
    }

    checker.finalize().pop().expect("single property result")
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_state(phase: &str, has_quorum: bool) -> Value {
        serde_json::json!({
            "phase": phase,
            "hasQuorum": has_quorum,
            "allOnline": true,
            "instances": {}
        })
    }

    #[test]
    fn test_bounded_liveness_property_default() {
        let prop = BoundedLivenessProperty::default();
        assert_eq!(prop.precondition, "true");
        assert_eq!(prop.goal, "true");
        assert_eq!(prop.step_bound, 100);
    }

    #[test]
    fn test_checker_creation() {
        let checker = BoundedLivenessChecker::new();
        assert!(checker.properties().is_empty());

        let checker =
            BoundedLivenessChecker::with_synchrony(SynchronyAssumption::Synchronous { delta: 5 });
        assert!(checker.properties().is_empty());
    }

    #[test]
    fn test_add_property() {
        let mut checker = BoundedLivenessChecker::new();
        checker.add_property(BoundedLivenessProperty {
            name: "test_prop".to_string(),
            precondition: "gstReached".to_string(),
            goal: "committed".to_string(),
            step_bound: 10,
            ..Default::default()
        });

        assert_eq!(checker.properties().len(), 1);
        assert!(checker.get_property("test_prop").is_some());
    }

    #[test]
    fn test_add_consensus_properties() {
        let mut checker =
            BoundedLivenessChecker::with_synchrony(SynchronyAssumption::PartialSynchrony {
                gst: 5,
                delta: 3,
            });
        checker.add_consensus_properties();

        assert!(checker.get_property("fast_path_bound").is_some());
        assert!(checker.get_property("consensus_terminates").is_some());
        assert!(checker.get_property("progress_under_synchrony").is_some());
        assert!(checker.get_property("no_deadlock").is_some());
    }

    #[test]
    fn test_check_step_goal_achieved() {
        let mut checker =
            BoundedLivenessChecker::with_synchrony(SynchronyAssumption::Synchronous { delta: 3 });
        checker.add_property(BoundedLivenessProperty {
            name: "test".to_string(),
            precondition: "true".to_string(),
            goal: "committed".to_string(),
            step_bound: 5,
            ..Default::default()
        });

        // Step 0: not committed
        let state0 = create_test_state("FastPathActive", true);
        let violations = checker.check_step(0, &state0);
        assert!(violations.is_empty());

        // Step 2: committed
        let state2 = create_test_state("Committed", true);
        let violations = checker.check_step(2, &state2);
        assert!(violations.is_empty());

        // Finalize
        let results = checker.finalize();
        assert_eq!(results.len(), 1);
        assert!(results[0].satisfied);
        assert_eq!(results[0].goal_step, Some(2));
    }

    #[test]
    fn test_check_step_bound_exceeded() {
        let mut checker =
            BoundedLivenessChecker::with_synchrony(SynchronyAssumption::Synchronous { delta: 3 });
        checker.add_property(BoundedLivenessProperty {
            name: "test".to_string(),
            precondition: "true".to_string(),
            goal: "committed".to_string(),
            step_bound: 3,
            ..Default::default()
        });

        let state = create_test_state("FastPathActive", true);

        // Steps 0-3: not committed
        for i in 0..=3 {
            checker.check_step(i, &state);
        }

        // Step 4: bound exceeded
        let violations = checker.check_step(4, &state);
        assert_eq!(violations.len(), 1);
        assert!(!violations[0].satisfied);
        assert!(violations[0].details.contains("Bound exceeded"));
    }

    #[test]
    fn test_gst_based_precondition() {
        let mut checker =
            BoundedLivenessChecker::with_synchrony(SynchronyAssumption::PartialSynchrony {
                gst: 5,
                delta: 3,
            });
        checker.add_property(BoundedLivenessProperty {
            name: "test".to_string(),
            precondition: "gstReached".to_string(),
            goal: "committed".to_string(),
            step_bound: 10,
            ..Default::default()
        });

        let state = create_test_state("FastPathActive", true);

        // Before GST: precondition not triggered
        for i in 0..5 {
            checker.check_step(i, &state);
        }

        // At and after GST: precondition triggered
        checker.check_step(5, &state);

        let results = checker.finalize();
        assert_eq!(results[0].precondition_step, Some(5));
    }

    #[test]
    fn test_vacuous_satisfaction() {
        // Test with partial synchrony where GST is never reached
        let mut checker =
            BoundedLivenessChecker::with_synchrony(SynchronyAssumption::PartialSynchrony {
                gst: 100,
                delta: 3,
            });
        checker.add_property(BoundedLivenessProperty {
            name: "test".to_string(),
            precondition: "gstReached".to_string(), // GST at step 100, so never triggered
            goal: "committed".to_string(),
            step_bound: 5,
            ..Default::default()
        });

        let state = create_test_state("FastPathActive", true);
        checker.check_step(0, &state);
        checker.check_step(1, &state);

        let results = checker.finalize();
        eprintln!("Result: {:?}", results[0]);
        assert!(
            results[0].satisfied,
            "expected satisfied, got: {:?}",
            results[0]
        );
        assert!(
            results[0].details.contains("Vacuously") || results[0].details.contains("vacuously"),
            "expected vacuously in details, got: {:?}",
            results[0].details
        );
    }

    #[test]
    fn test_check_consensus_terminates_within() {
        let steps: Vec<Value> = vec![
            serde_json::json!({"instances": {"cns1": {"phase": "FastPathActive"}}}),
            serde_json::json!({"instances": {"cns1": {"phase": "FastPathActive"}}}),
            serde_json::json!({"instances": {"cns1": {"phase": "Committed"}}}),
        ];

        let result = check_consensus_terminates_within(&steps, 10, 0);
        assert!(result.satisfied);
        assert_eq!(result.goal_step, Some(2));
    }

    #[test]
    fn test_expression_evaluation() {
        let checker = BoundedLivenessChecker::new();
        let state = serde_json::json!({
            "phase": "FastPathActive",
            "hasQuorum": true,
            "flag": true
        });

        // Test literals
        assert!(checker.evaluate_expression("true", &state, true));
        assert!(!checker.evaluate_expression("false", &state, true));

        // Test GST
        assert!(checker.evaluate_expression("gstReached", &state, true));
        assert!(!checker.evaluate_expression("gstReached", &state, false));

        // Test state variables
        assert!(checker.evaluate_expression("flag", &state, true));
        assert!(checker.evaluate_expression("hasQuorum", &state, true));

        // Test and/or
        assert!(checker.evaluate_expression("flag and hasQuorum", &state, true));
        assert!(checker.evaluate_expression("flag or nope", &state, true));

        // Test function-like
        assert!(checker.evaluate_expression("fastPathActive", &state, true));
    }

    #[test]
    fn test_reset() {
        let mut checker = BoundedLivenessChecker::new();
        checker.add_property(BoundedLivenessProperty {
            name: "test".to_string(),
            precondition: "true".to_string(),
            goal: "committed".to_string(),
            step_bound: 10,
            ..Default::default()
        });

        let state = create_test_state("FastPathActive", true);
        checker.check_step(0, &state);
        checker.check_step(1, &state);

        // Reset
        checker.reset();

        let results = checker.finalize();
        assert!(results[0].precondition_step.is_none());
    }
}
