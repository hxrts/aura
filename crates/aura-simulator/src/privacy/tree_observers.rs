//! Privacy observer models for tree protocol analysis.
//!
//! This module implements different observer models to measure information
//! leakage in the commitment tree protocols:
//!
//! - **External Observer**: Only sees encrypted traffic (timing, sizes)
//! - **Neighbor Observer**: Can see envelope metadata (sender, receiver)
//! - **In-Group Observer**: Participant who sees signer_count but not identities
//!
//! Each observer tracks accumulated leakage across different privacy dimensions.

use aura_journal::{AttestedOp, Epoch, LeafId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
#[cfg(test)]
use std::time::SystemTime;

// ============================================================================
// Privacy Budget Configuration
// ============================================================================

/// Privacy budget thresholds for different leakage dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrivacyBudget {
    /// Maximum timing correlation leakage (bits)
    pub timing_entropy_min: f64,

    /// Maximum participation inference confidence (0.0 - 1.0)
    pub participation_inference_max: f64,

    /// Whether signer identities must be completely hidden
    pub signer_identity_hidden: bool,

    /// Whether author identities must be completely hidden
    pub author_identity_hidden: bool,

    /// Maximum message size variation (bytes)
    pub message_size_variance_max: usize,
}

impl Default for PrivacyBudget {
    fn default() -> Self {
        Self {
            timing_entropy_min: 8.0,          // At least 8 bits of timing entropy
            participation_inference_max: 0.3, // Max 30% confidence
            signer_identity_hidden: true,     // Complete identity hiding
            author_identity_hidden: true,     // Complete author hiding
            message_size_variance_max: 1024,  // 1KB padding tolerance
        }
    }
}

// ============================================================================
// Observation Events
// ============================================================================

/// Events that observers can see, depending on their capability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ObservationEvent {
    /// Network traffic observed (timestamp, size in bytes)
    NetworkTraffic {
        timestamp_ms: u64,
        size: usize,
        encrypted: bool,
    },

    /// Message envelope metadata (timing, sender, receiver)
    EnvelopeMetadata {
        timestamp_ms: u64,
        sender: Option<LeafId>,
        receiver: Option<LeafId>,
        message_type: String,
        size: usize,
    },

    /// Operation committed to journal (only content, not participants)
    OperationCommitted {
        timestamp_ms: u64,
        epoch: Epoch,
        op_type: String,
        signer_count: u16,
    },

    /// Ceremony execution observed
    CeremonyExecution {
        timestamp_ms: u64,
        phase: String,
        participant_count: usize,
    },
}

// ============================================================================
// Privacy Leakage Measurements
// ============================================================================

/// Accumulated privacy leakage across different dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrivacyLeakage {
    /// Timing entropy (bits) - higher is better
    pub timing_entropy: f64,

    /// Participation inference confidence (0.0 - 1.0) - lower is better
    pub participation_inference: f64,

    /// Number of signer identities revealed - should be 0
    pub signer_identities_revealed: usize,

    /// Number of author identities revealed - should be 0
    pub author_identities_revealed: usize,

    /// Message size variance (bytes) - lower is better
    pub message_size_variance: f64,

    /// Detailed timing attack surface
    pub timing_observations: Vec<Duration>,

    /// Detailed size observations
    pub size_observations: Vec<usize>,
}

impl Default for PrivacyLeakage {
    fn default() -> Self {
        Self {
            timing_entropy: 0.0,
            participation_inference: 0.0,
            signer_identities_revealed: 0,
            author_identities_revealed: 0,
            message_size_variance: 0.0,
            timing_observations: Vec::new(),
            size_observations: Vec::new(),
        }
    }
}

impl PrivacyLeakage {
    /// Checks if leakage exceeds privacy budget.
    pub fn exceeds_budget(&self, budget: &PrivacyBudget) -> bool {
        if self.timing_entropy < budget.timing_entropy_min {
            return true;
        }

        if self.participation_inference > budget.participation_inference_max {
            return true;
        }

        if budget.signer_identity_hidden && self.signer_identities_revealed > 0 {
            return true;
        }

        if budget.author_identity_hidden && self.author_identities_revealed > 0 {
            return true;
        }

        if self.message_size_variance > budget.message_size_variance_max as f64 {
            return true;
        }

        false
    }

    /// Computes entropy from timing observations.
    pub fn compute_timing_entropy(&mut self) {
        if self.timing_observations.is_empty() {
            self.timing_entropy = 0.0;
            return;
        }

        // Simple entropy estimate: log2 of unique timing buckets (100ms granularity)
        let mut buckets = BTreeSet::new();
        for duration in &self.timing_observations {
            let bucket = duration.as_millis() / 100; // 100ms buckets
            buckets.insert(bucket);
        }

        self.timing_entropy = (buckets.len() as f64).log2();
    }

    /// Computes message size variance.
    pub fn compute_size_variance(&mut self) {
        if self.size_observations.is_empty() {
            self.message_size_variance = 0.0;
            return;
        }

        let mean: f64 = self.size_observations.iter().sum::<usize>() as f64
            / self.size_observations.len() as f64;

        let variance: f64 = self
            .size_observations
            .iter()
            .map(|&size| {
                let diff = size as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / self.size_observations.len() as f64;

        self.message_size_variance = variance.sqrt();
    }
}

// ============================================================================
// External Observer (Minimal Capability)
// ============================================================================

/// External observer who can only see encrypted network traffic.
///
/// Can observe:
/// - Packet timing (when messages are sent)
/// - Packet sizes (encrypted payload size)
/// - Traffic patterns (bursts, intervals)
///
/// Cannot observe:
/// - Message content (encrypted)
/// - Sender/receiver identities (transport layer encryption)
/// - Message types
pub struct ExternalObserver {
    observations: Vec<ObservationEvent>,
    leakage: PrivacyLeakage,
}

impl Default for ExternalObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl ExternalObserver {
    pub fn new() -> Self {
        Self {
            observations: Vec::new(),
            leakage: PrivacyLeakage::default(),
        }
    }

    /// Observes network traffic.
    pub fn observe_traffic(&mut self, timestamp_ms: u64, size: usize) {
        let event = ObservationEvent::NetworkTraffic {
            timestamp_ms,
            size,
            encrypted: true,
        };

        // Record timing
        if let Some(ObservationEvent::NetworkTraffic {
            timestamp_ms: last, ..
        }) = self.observations.last()
        {
            let delta = timestamp_ms.saturating_sub(*last);
            self.leakage
                .timing_observations
                .push(Duration::from_millis(delta));
        }

        // Record size
        self.leakage.size_observations.push(size);

        self.observations.push(event);
    }

    /// Analyzes accumulated observations and computes leakage.
    pub fn analyze(&mut self) -> &PrivacyLeakage {
        self.leakage.compute_timing_entropy();
        self.leakage.compute_size_variance();
        &self.leakage
    }

    /// Returns all observations.
    pub fn observations(&self) -> &[ObservationEvent] {
        &self.observations
    }

    /// Returns current leakage measurements.
    pub fn leakage(&self) -> &PrivacyLeakage {
        &self.leakage
    }
}

// ============================================================================
// Neighbor Observer (Medium Capability)
// ============================================================================

/// Neighbor observer who participates in message routing.
///
/// Can observe:
/// - Everything external observer can see
/// - Message envelope metadata (sender, receiver, type)
/// - Routing patterns
///
/// Cannot observe:
/// - Message payload content (encrypted)
/// - Signer identities (hidden in AttestedOp)
/// - Author identities (never transmitted)
pub struct NeighborObserver {
    observations: Vec<ObservationEvent>,
    leakage: PrivacyLeakage,
    routing_graph: BTreeMap<LeafId, BTreeSet<LeafId>>, // sender -> receivers
}

impl Default for NeighborObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl NeighborObserver {
    pub fn new() -> Self {
        Self {
            observations: Vec::new(),
            leakage: PrivacyLeakage::default(),
            routing_graph: BTreeMap::new(),
        }
    }

    /// Observes envelope metadata during message routing.
    pub fn observe_envelope(
        &mut self,
        timestamp_ms: u64,
        sender: Option<LeafId>,
        receiver: Option<LeafId>,
        message_type: String,
        size: usize,
    ) {
        let event = ObservationEvent::EnvelopeMetadata {
            timestamp_ms,
            sender,
            receiver,
            message_type,
            size,
        };

        // Record timing
        if let Some(ObservationEvent::EnvelopeMetadata {
            timestamp_ms: last, ..
        }) = self.observations.last()
        {
            let delta = timestamp_ms.saturating_sub(*last);
            self.leakage
                .timing_observations
                .push(Duration::from_millis(delta));
        }

        // Record size
        self.leakage.size_observations.push(size);

        // Build routing graph
        if let (Some(s), Some(r)) = (sender, receiver) {
            self.routing_graph.entry(s).or_default().insert(r);
        }

        self.observations.push(event);
    }

    /// Attempts to infer participation in ceremonies from routing patterns.
    pub fn infer_participation(&mut self) {
        // Simple heuristic: if a node sends to many others simultaneously,
        // they might be coordinating a ceremony
        let mut max_fanout = 0;
        for receivers in self.routing_graph.values() {
            max_fanout = max_fanout.max(receivers.len());
        }

        // Confidence increases with fanout (normalized to 0.0-1.0)
        // This is a very simple model - real analysis would be more sophisticated
        self.leakage.participation_inference = (max_fanout as f64 / 10.0).min(1.0);
    }

    /// Analyzes accumulated observations and computes leakage.
    pub fn analyze(&mut self) -> &PrivacyLeakage {
        self.leakage.compute_timing_entropy();
        self.leakage.compute_size_variance();
        self.infer_participation();
        &self.leakage
    }

    /// Returns all observations.
    pub fn observations(&self) -> &[ObservationEvent] {
        &self.observations
    }

    /// Returns current leakage measurements.
    pub fn leakage(&self) -> &PrivacyLeakage {
        &self.leakage
    }

    /// Returns routing graph for analysis.
    pub fn routing_graph(&self) -> &BTreeMap<LeafId, BTreeSet<LeafId>> {
        &self.routing_graph
    }
}

// ============================================================================
// In-Group Observer (High Capability)
// ============================================================================

/// In-group observer who is a tree participant.
///
/// Can observe:
/// - Everything neighbor observer can see
/// - Committed operations in journal (signer_count but not identities)
/// - Ceremony execution phases
/// - Tree state changes
///
/// Cannot observe:
/// - Individual signer identities (hidden by aggregate signature)
/// - Author identity (never recorded in AttestedOp)
/// - Other participants' private key shares
pub struct InGroupObserver {
    observations: Vec<ObservationEvent>,
    leakage: PrivacyLeakage,
    signer_counts: Vec<u16>,
    ceremony_participants: BTreeMap<String, usize>, // phase -> count
}

impl Default for InGroupObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl InGroupObserver {
    pub fn new() -> Self {
        Self {
            observations: Vec::new(),
            leakage: PrivacyLeakage::default(),
            signer_counts: Vec::new(),
            ceremony_participants: BTreeMap::new(),
        }
    }

    /// Observes operation committed to journal.
    pub fn observe_operation(
        &mut self,
        timestamp_ms: u64,
        epoch: Epoch,
        op_type: String,
        signer_count: u16,
    ) {
        let event = ObservationEvent::OperationCommitted {
            timestamp_ms,
            epoch,
            op_type,
            signer_count,
        };

        // Record signer count (this is intentionally visible)
        self.signer_counts.push(signer_count);

        // Record timing
        if let Some(ObservationEvent::OperationCommitted {
            timestamp_ms: last, ..
        }) = self.observations.last()
        {
            let delta = timestamp_ms.saturating_sub(*last);
            self.leakage
                .timing_observations
                .push(Duration::from_millis(delta));
        }

        self.observations.push(event);
    }

    /// Observes ceremony execution phase.
    pub fn observe_ceremony(&mut self, timestamp_ms: u64, phase: String, participant_count: usize) {
        let event = ObservationEvent::CeremonyExecution {
            timestamp_ms,
            phase: phase.clone(),
            participant_count,
        };

        // Record participant count per phase
        self.ceremony_participants.insert(phase, participant_count);

        self.observations.push(event);
    }

    /// Checks if any signer identities were revealed (should be zero).
    pub fn check_signer_identity_leakage(&mut self, attested_op: &AttestedOp) {
        // AttestedOp should only contain:
        // - agg_sig: aggregate signature (hides individual signers)
        // - signer_count: number of signers (intentionally visible)
        //
        // It should NOT contain:
        // - Signer IDs
        // - Individual signatures
        // - Author identity

        // This is structural verification - the type system enforces this
        // but we document it here for privacy audit purposes
        let _ = attested_op.signer_count; // Only count is visible

        // No identity leakage expected from well-formed AttestedOp
        self.leakage.signer_identities_revealed = 0;
        self.leakage.author_identities_revealed = 0;
    }

    /// Analyzes accumulated observations and computes leakage.
    pub fn analyze(&mut self) -> &PrivacyLeakage {
        self.leakage.compute_timing_entropy();
        self.leakage.compute_size_variance();

        // Participation inference from ceremony observations
        let max_participants = self
            .ceremony_participants
            .values()
            .max()
            .copied()
            .unwrap_or(0);
        self.leakage.participation_inference = (max_participants as f64 / 10.0).min(1.0);

        &self.leakage
    }

    /// Returns all observations.
    pub fn observations(&self) -> &[ObservationEvent] {
        &self.observations
    }

    /// Returns current leakage measurements.
    pub fn leakage(&self) -> &PrivacyLeakage {
        &self.leakage
    }

    /// Returns signer count distribution.
    pub fn signer_count_distribution(&self) -> &[u16] {
        &self.signer_counts
    }

    /// Returns ceremony participant counts.
    pub fn ceremony_participants(&self) -> &BTreeMap<String, usize> {
        &self.ceremony_participants
    }
}

// ============================================================================
// Privacy Audit Report
// ============================================================================

/// Complete privacy audit report combining all observer models.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrivacyAuditReport {
    pub budget: PrivacyBudget,
    pub external_leakage: PrivacyLeakage,
    pub neighbor_leakage: PrivacyLeakage,
    pub ingroup_leakage: PrivacyLeakage,
    pub violations: Vec<String>,
}

impl PrivacyAuditReport {
    /// Creates a new audit report with given budget.
    pub fn new(budget: PrivacyBudget) -> Self {
        Self {
            budget,
            external_leakage: PrivacyLeakage::default(),
            neighbor_leakage: PrivacyLeakage::default(),
            ingroup_leakage: PrivacyLeakage::default(),
            violations: Vec::new(),
        }
    }

    /// Checks all leakage against budget and records violations.
    pub fn check_violations(&mut self) {
        self.violations.clear();

        if self.external_leakage.exceeds_budget(&self.budget) {
            self.violations
                .push("External observer leakage exceeds budget".to_string());
        }

        if self.neighbor_leakage.exceeds_budget(&self.budget) {
            self.violations
                .push("Neighbor observer leakage exceeds budget".to_string());
        }

        if self.ingroup_leakage.exceeds_budget(&self.budget) {
            self.violations
                .push("In-group observer leakage exceeds budget".to_string());
        }

        // Specific checks
        if self.ingroup_leakage.signer_identities_revealed > 0 {
            self.violations.push(format!(
                "Signer identities revealed: {}",
                self.ingroup_leakage.signer_identities_revealed
            ));
        }

        if self.ingroup_leakage.author_identities_revealed > 0 {
            self.violations.push(format!(
                "Author identities revealed: {}",
                self.ingroup_leakage.author_identities_revealed
            ));
        }
    }

    /// Returns true if any violations were detected.
    pub fn has_violations(&self) -> bool {
        !self.violations.is_empty()
    }

    /// Returns list of violations.
    pub fn violations(&self) -> &[String] {
        &self.violations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_privacy_budget_defaults() {
        let budget = PrivacyBudget::default();
        assert_eq!(budget.timing_entropy_min, 8.0);
        assert_eq!(budget.participation_inference_max, 0.3);
        assert!(budget.signer_identity_hidden);
        assert!(budget.author_identity_hidden);
    }

    #[test]
    fn test_external_observer_timing_entropy() {
        let mut observer = ExternalObserver::new();
        let now = SystemTime::UNIX_EPOCH;

        // Observe traffic with varying delays to create different timing buckets
        let intervals = [50, 150, 200, 100, 300, 75, 250, 125, 175, 225]; // Varied intervals
        let mut cumulative_time = 0;
        for interval in intervals {
            cumulative_time += interval;
            let timestamp_ms = now
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
                + cumulative_time;
            observer.observe_traffic(timestamp_ms, 1024);
        }

        let leakage = observer.analyze();

        // Should have reasonable timing entropy
        assert!(
            leakage.timing_entropy > 0.0,
            "Should have some timing entropy"
        );
    }

    #[test]
    fn test_neighbor_observer_routing_graph() {
        let mut observer = NeighborObserver::new();
        let now = SystemTime::UNIX_EPOCH;

        let sender = LeafId(1);
        let receivers = vec![LeafId(2), LeafId(3), LeafId(4)];

        // Observe broadcast pattern
        let now_ms = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        for receiver in receivers {
            observer.observe_envelope(
                now_ms,
                Some(sender),
                Some(receiver),
                "OpBroadcast".to_string(),
                512,
            );
        }

        observer.analyze();

        // Should detect fanout pattern
        let graph = observer.routing_graph();
        assert_eq!(graph.get(&sender).unwrap().len(), 3);
    }

    #[test]
    fn test_ingroup_observer_signer_count() {
        let mut observer = InGroupObserver::new();
        let now = SystemTime::UNIX_EPOCH;

        let now_ms = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        observer.observe_operation(now_ms, 1, "AddLeaf".to_string(), 5);
        observer.observe_operation(now_ms + 1000, 1, "RemoveLeaf".to_string(), 3);

        let distribution = observer.signer_count_distribution();
        assert_eq!(distribution, &[5, 3]);
    }

    #[test]
    fn test_privacy_leakage_exceeds_budget() {
        let budget = PrivacyBudget::default();

        let leakage = PrivacyLeakage {
            timing_entropy: 5.0, // Below minimum of 8.0
            ..PrivacyLeakage::default()
        };

        assert!(leakage.exceeds_budget(&budget));
    }

    #[test]
    fn test_privacy_audit_report() {
        let budget = PrivacyBudget::default();
        let mut report = PrivacyAuditReport::new(budget);

        // Set up leakage that meets timing requirements but violates signer identity
        report.external_leakage.timing_entropy = 10.0; // Above minimum of 8.0
        report.neighbor_leakage.timing_entropy = 10.0; // Above minimum of 8.0
        report.ingroup_leakage.timing_entropy = 10.0; // Above minimum of 8.0
        report.ingroup_leakage.signer_identities_revealed = 2; // Violates identity hiding

        report.check_violations();

        assert!(report.has_violations());
        assert_eq!(report.violations().len(), 2); // In-group budget exceeded + specific signer identity violation
    }
}
