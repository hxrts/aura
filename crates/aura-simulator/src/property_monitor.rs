//! Online property monitor for simulation ticks.

use aura_core::types::identifiers::{ContextId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::properties::{
    AuraProperty, GuardStage, PropertyContext, PropertyEvent, PropertyStateSnapshot,
};

const GUARD_STAGE_ORDER: [GuardStage; 5] = [
    GuardStage::CapGuard,
    GuardStage::FlowGuard,
    GuardStage::LeakageTracker,
    GuardStage::JournalCoupler,
    GuardStage::TransportSend,
];

/// Property violation captured during online monitoring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropertyViolation {
    /// Property name.
    pub property: String,
    /// Tick at which the violation was observed.
    pub tick: u64,
    /// Violation detail.
    pub details: String,
}

/// Serializable report for one property-monitored run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropertyRunReport {
    /// Number of registered properties.
    pub properties_checked: u64,
    /// Violations captured during this run.
    pub violations: Vec<PropertyViolation>,
}

impl PropertyRunReport {
    /// Compare against a baseline report and classify regressions.
    #[must_use]
    pub fn compare_against(&self, baseline: &Self) -> PropertyRegression {
        let key = |v: &PropertyViolation| format!("{}|{}|{}", v.property, v.tick, v.details);
        let baseline_map = baseline
            .violations
            .iter()
            .map(|violation| (key(violation), violation.clone()))
            .collect::<HashMap<_, _>>();
        let current_map = self
            .violations
            .iter()
            .map(|violation| (key(violation), violation.clone()))
            .collect::<HashMap<_, _>>();

        let mut new_violations = Vec::new();
        let mut resolved_violations = Vec::new();

        for (id, violation) in &current_map {
            if !baseline_map.contains_key(id) {
                new_violations.push(violation.clone());
            }
        }
        for (id, violation) in &baseline_map {
            if !current_map.contains_key(id) {
                resolved_violations.push(violation.clone());
            }
        }

        PropertyRegression {
            new_violations,
            resolved_violations,
        }
    }
}

/// Regression comparison output for property reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropertyRegression {
    /// Violations present in current run but absent in baseline.
    pub new_violations: Vec<PropertyViolation>,
    /// Violations present in baseline but absent in current run.
    pub resolved_violations: Vec<PropertyViolation>,
}

impl PropertyRegression {
    /// Whether this comparison introduces regressions.
    #[must_use]
    pub fn has_new_violations(&self) -> bool {
        !self.new_violations.is_empty()
    }
}

/// Time-series tracker for violation trends across runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropertyTrendTracker {
    /// Total violation counts per run (in chronological order).
    pub total_violations_by_run: Vec<u64>,
}

impl PropertyTrendTracker {
    /// Record one run report.
    pub fn record_run(&mut self, report: &PropertyRunReport) {
        self.total_violations_by_run
            .push(report.violations.len() as u64);
    }

    /// Return true when the latest run is strictly worse than the previous run.
    #[must_use]
    pub fn is_regressing(&self) -> bool {
        if self.total_violations_by_run.len() < 2 {
            return false;
        }
        let len = self.total_violations_by_run.len();
        self.total_violations_by_run[len - 1] > self.total_violations_by_run[len - 2]
    }
}

impl PropertyViolation {
    fn new(property: impl Into<String>, tick: u64, details: impl Into<String>) -> Self {
        Self {
            property: property.into(),
            tick,
            details: details.into(),
        }
    }
}

/// Per-tick property monitor.
pub struct AuraPropertyMonitor {
    properties: Vec<AuraProperty>,
    violations: Vec<PropertyViolation>,
    context: PropertyContext,
    pending_sends: HashMap<(SessionId, String), u64>,
    journal_diverged_at: HashMap<ContextId, u64>,
    consensus_started_at: HashMap<SessionId, u64>,
    liveness_started_at: HashMap<String, u64>,
    last_session_depth: HashMap<SessionId, u64>,
    guard_stage_progress: HashMap<(SessionId, String), usize>,
}

impl AuraPropertyMonitor {
    /// Create an empty monitor.
    #[must_use]
    pub fn new() -> Self {
        Self {
            properties: Vec::new(),
            violations: Vec::new(),
            context: PropertyContext::default(),
            pending_sends: HashMap::new(),
            journal_diverged_at: HashMap::new(),
            consensus_started_at: HashMap::new(),
            liveness_started_at: HashMap::new(),
            last_session_depth: HashMap::new(),
            guard_stage_progress: HashMap::new(),
        }
    }

    /// Create monitor with pre-registered properties.
    #[must_use]
    pub fn with_properties(properties: Vec<AuraProperty>) -> Self {
        let mut monitor = Self::new();
        monitor.properties = properties;
        monitor
    }

    /// Register one property.
    pub fn add_property(&mut self, property: AuraProperty) {
        self.properties.push(property);
    }

    /// Number of registered properties.
    #[must_use]
    pub fn property_count(&self) -> usize {
        self.properties.len()
    }

    /// Run checks for one simulation tick.
    pub fn check_tick(&mut self, tick: u64, snapshot: &PropertyStateSnapshot) {
        self.context.apply_snapshot(tick, snapshot);
        self.process_events(tick, &snapshot.events);

        for property in self.properties.clone() {
            match property {
                AuraProperty::NoFaults => {
                    if let Some((session, reason)) =
                        snapshot.events.iter().find_map(|event| match event {
                            PropertyEvent::Faulted { session, reason } => Some((session, reason)),
                            _ => None,
                        })
                    {
                        self.violations.push(PropertyViolation::new(
                            "NoFaults",
                            tick,
                            format!("faulted session {session}: {reason}"),
                        ));
                    }
                }
                AuraProperty::SendRecvLiveness { session, bound } => {
                    for ((pending_session, message_id), sent_at) in &self.pending_sends {
                        if pending_session == &session && tick.saturating_sub(*sent_at) > bound {
                            self.violations.push(PropertyViolation::new(
                                "SendRecvLiveness",
                                tick,
                                format!(
                                    "message {message_id} in session {session} exceeded send/recv bound {bound}"
                                ),
                            ));
                        }
                    }
                }
                AuraProperty::TypeMonotonicity { session } => {
                    if let Some(current_depth) = self.context.session_depths.get(&session).copied()
                    {
                        if let Some(previous_depth) = self.last_session_depth.get(&session).copied()
                        {
                            if current_depth > previous_depth {
                                self.violations.push(PropertyViolation::new(
                                    "TypeMonotonicity",
                                    tick,
                                    format!(
                                        "session {session} depth increased from {previous_depth} to {current_depth}"
                                    ),
                                ));
                            }
                        }
                        self.last_session_depth.insert(session, current_depth);
                    }
                }
                AuraProperty::BufferBound { session, max_size } => {
                    if let Some(size) = self.context.buffer_sizes.get(&session).copied() {
                        if size > max_size {
                            self.violations.push(PropertyViolation::new(
                                "BufferBound",
                                tick,
                                format!("session {session} buffer size {size} exceeded {max_size}"),
                            ));
                        }
                    }
                }
                AuraProperty::Liveness {
                    name,
                    precondition,
                    goal,
                    bound,
                } => {
                    let key = format!("Liveness:{name}");
                    let precondition_holds = precondition(&self.context);
                    let goal_holds = goal(&self.context);

                    if precondition_holds && !self.liveness_started_at.contains_key(&key) {
                        self.liveness_started_at.insert(key.clone(), tick);
                    }

                    if let Some(start_tick) = self.liveness_started_at.get(&key).copied() {
                        if goal_holds {
                            self.liveness_started_at.remove(&key);
                        } else if tick.saturating_sub(start_tick) > bound {
                            self.violations.push(PropertyViolation::new(
                                "Liveness",
                                tick,
                                format!(
                                    "{name} exceeded bound {bound} after precondition activation at tick {start_tick}"
                                ),
                            ));
                        }
                    }
                }
                AuraProperty::JournalConvergence { context, bound } => {
                    if let Some(start_tick) = self.journal_diverged_at.get(&context).copied() {
                        if tick.saturating_sub(start_tick) > bound {
                            self.violations.push(PropertyViolation::new(
                                "JournalConvergence",
                                tick,
                                format!(
                                    "context {context} did not converge within {bound} ticks after divergence at tick {start_tick}"
                                ),
                            ));
                        }
                    }
                }
                AuraProperty::ConsensusLiveness { session, bound } => {
                    if let Some(start_tick) = self.consensus_started_at.get(&session).copied() {
                        if tick.saturating_sub(start_tick) > bound {
                            self.violations.push(PropertyViolation::new(
                                "ConsensusLiveness",
                                tick,
                                format!(
                                    "session {session} did not commit within {bound} ticks after start at tick {start_tick}"
                                ),
                            ));
                        }
                    }
                }
                AuraProperty::FlowBudgetInvariant { context } => {
                    if let Some(balance) = self.context.flow_budget_balances.get(&context).copied()
                    {
                        if balance < 0 {
                            self.violations.push(PropertyViolation::new(
                                "FlowBudgetInvariant",
                                tick,
                                format!(
                                    "context {context} has negative flow budget balance {balance}"
                                ),
                            ));
                        }
                    }
                }
                AuraProperty::GuardChainOrdering => {}
            }
        }
    }

    /// Borrow captured violations.
    #[must_use]
    pub fn violations(&self) -> &[PropertyViolation] {
        &self.violations
    }

    /// Borrow latest property context.
    #[must_use]
    pub fn context(&self) -> &PropertyContext {
        &self.context
    }

    /// Ensure no violations were recorded.
    pub fn assert_no_violations(&self) -> Result<(), Vec<PropertyViolation>> {
        if self.violations.is_empty() {
            Ok(())
        } else {
            Err(self.violations.clone())
        }
    }

    /// Build a serializable run report for CI artifacts.
    #[must_use]
    pub fn run_report(&self) -> PropertyRunReport {
        PropertyRunReport {
            properties_checked: self.property_count() as u64,
            violations: self.violations.clone(),
        }
    }

    fn process_events(&mut self, tick: u64, events: &[PropertyEvent]) {
        for event in events {
            match event {
                PropertyEvent::Faulted { .. } => {}
                PropertyEvent::Sent {
                    session,
                    message_id,
                } => {
                    self.pending_sends
                        .insert((*session, message_id.clone()), tick);
                }
                PropertyEvent::Received {
                    session,
                    message_id,
                } => {
                    self.pending_sends.remove(&(*session, message_id.clone()));
                }
                PropertyEvent::JournalDiverged { context } => {
                    self.journal_diverged_at.entry(*context).or_insert(tick);
                }
                PropertyEvent::JournalConverged { context } => {
                    self.journal_diverged_at.remove(context);
                }
                PropertyEvent::ConsensusStarted { session } => {
                    self.consensus_started_at.entry(*session).or_insert(tick);
                }
                PropertyEvent::ConsensusCommitted { session } => {
                    self.consensus_started_at.remove(session);
                }
                PropertyEvent::GuardStage {
                    session,
                    message_id,
                    stage,
                } => self.check_guard_stage_order(tick, *session, message_id, *stage),
            }
        }
    }

    fn check_guard_stage_order(
        &mut self,
        tick: u64,
        session: SessionId,
        message_id: &str,
        stage: GuardStage,
    ) {
        let key = (session, message_id.to_string());
        let expected_index = self.guard_stage_progress.get(&key).copied().unwrap_or(0);
        let expected_stage = GUARD_STAGE_ORDER.get(expected_index).copied();

        if expected_stage == Some(stage) {
            self.guard_stage_progress
                .insert(key, expected_index.saturating_add(1));
            return;
        }

        self.violations.push(PropertyViolation::new(
            "GuardChainOrdering",
            tick,
            format!(
                "session {session} message {message_id} expected guard stage {expected_stage:?}, got {stage:?}",
            ),
        ));
    }
}

impl Default for AuraPropertyMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use aura_core::types::identifiers::{ContextId, SessionId};
    use uuid::Uuid;

    use super::*;
    use crate::properties::{AuraProperty, PropertyStateSnapshot};

    fn sid() -> SessionId {
        SessionId::from_uuid(Uuid::from_u128(1))
    }

    fn ctx() -> ContextId {
        ContextId::from_uuid(Uuid::from_u128(2))
    }

    #[test]
    fn send_recv_liveness_violation_is_detected() {
        let session = sid();
        let mut monitor =
            AuraPropertyMonitor::with_properties(vec![AuraProperty::SendRecvLiveness {
                session,
                bound: 1,
            }]);

        monitor.check_tick(
            0,
            &PropertyStateSnapshot {
                events: vec![PropertyEvent::Sent {
                    session,
                    message_id: "m1".to_string(),
                }],
                ..PropertyStateSnapshot::default()
            },
        );
        monitor.check_tick(2, &PropertyStateSnapshot::default());

        assert!(!monitor.violations().is_empty());
        assert_eq!(monitor.violations()[0].property, "SendRecvLiveness");
    }

    #[test]
    fn guard_chain_order_violation_is_detected() {
        let session = sid();
        let mut monitor =
            AuraPropertyMonitor::with_properties(vec![AuraProperty::GuardChainOrdering]);
        monitor.check_tick(
            1,
            &PropertyStateSnapshot {
                events: vec![PropertyEvent::GuardStage {
                    session,
                    message_id: "m1".to_string(),
                    stage: GuardStage::FlowGuard,
                }],
                ..PropertyStateSnapshot::default()
            },
        );

        assert!(!monitor.violations().is_empty());
        assert_eq!(monitor.violations()[0].property, "GuardChainOrdering");
    }

    #[test]
    fn flow_budget_invariant_violation_is_detected() {
        let context = ctx();
        let mut monitor =
            AuraPropertyMonitor::with_properties(vec![AuraProperty::FlowBudgetInvariant {
                context,
            }]);
        monitor.check_tick(
            5,
            &PropertyStateSnapshot {
                flow_budget_balances: HashMap::from([(context, -1)]),
                ..PropertyStateSnapshot::default()
            },
        );

        assert!(!monitor.violations().is_empty());
        assert_eq!(monitor.violations()[0].property, "FlowBudgetInvariant");
    }

    #[test]
    fn report_comparison_detects_new_violations() {
        let baseline = PropertyRunReport {
            properties_checked: 2,
            violations: vec![PropertyViolation::new("NoFaults", 1, "faulted session A")],
        };
        let current = PropertyRunReport {
            properties_checked: 2,
            violations: vec![
                PropertyViolation::new("NoFaults", 1, "faulted session A"),
                PropertyViolation::new("BufferBound", 3, "buffer exceeded"),
            ],
        };

        let regression = current.compare_against(&baseline);
        assert_eq!(regression.new_violations.len(), 1);
        assert_eq!(regression.new_violations[0].property, "BufferBound");
        assert!(regression.resolved_violations.is_empty());
        assert!(regression.has_new_violations());
    }

    #[test]
    fn trend_tracker_flags_worsening_runs() {
        let mut tracker = PropertyTrendTracker::default();
        tracker.record_run(&PropertyRunReport {
            properties_checked: 2,
            violations: vec![PropertyViolation::new("A", 1, "v1")],
        });
        tracker.record_run(&PropertyRunReport {
            properties_checked: 2,
            violations: vec![
                PropertyViolation::new("A", 1, "v1"),
                PropertyViolation::new("B", 2, "v2"),
            ],
        });
        assert!(tracker.is_regressing());
    }
}
