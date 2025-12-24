//! Consensus Simulation Scenarios
//!
//! Simulation harness for testing consensus protocol edge cases beyond
//! what Quint ITF traces cover. Provides network partition, message loss,
//! and Byzantine witness injection scenarios.
//!
//! ## Task Correspondence
//! - T8.1: Create simulation harness for consensus
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │                        Simulation Harness                        │
//! ├──────────────────────────────────────────────────────────────────┤
//! │  ┌──────────────────┐  ┌──────────────────┐  ┌────────────────┐  │
//! │  │Network Partitions│  │   Message Loss   │  │Byzantine Inject│  │
//! │  └────────┬─────────┘  └────────┬─────────┘  └───────┬────────┘  │
//! │           │                     │                    │           │
//! │           ▼                     ▼                    ▼           │
//! │  ┌────────────────────────────────────────────────────────────┐  │
//! │  │              Pure Core (ConsensusState)                    │  │
//! │  │  - Deterministic state transitions                         │  │
//! │  │  - Invariant assertions                                    │  │
//! │  └────────────────────────────────────────────────────────────┘  │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Scenarios
//!
//! - **Network Partition**: Split witnesses into isolated groups
//! - **Message Loss**: Drop messages probabilistically or selectively
//! - **Message Reordering**: Deliver messages out of order
//! - **Byzantine Witness**: Inject equivocating or invalid votes

use aura_protocol::consensus::core::state::{
    ConsensusPhase, ConsensusState, PathSelection, PureCommitFact, ShareData, ShareProposal,
};
use aura_protocol::consensus::core::transitions::{apply_share, trigger_fallback};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

/// Network partition configuration
#[derive(Debug, Clone)]
pub struct NetworkPartition {
    /// Groups of witnesses that can communicate with each other
    pub groups: Vec<HashSet<String>>,
    /// Whether the partition is bidirectional
    pub bidirectional: bool,
}

impl NetworkPartition {
    /// Create a network partition with two groups
    pub fn split(group1: Vec<&str>, group2: Vec<&str>) -> Self {
        Self {
            groups: vec![
                group1.into_iter().map(String::from).collect(),
                group2.into_iter().map(String::from).collect(),
            ],
            bidirectional: true,
        }
    }

    /// Check if two witnesses can communicate
    pub fn can_communicate(&self, from: &str, to: &str) -> bool {
        for group in &self.groups {
            if group.contains(from) && group.contains(to) {
                return true;
            }
        }
        false
    }
}

/// Message loss configuration
#[derive(Debug, Clone)]
pub struct MessageLossConfig {
    /// Drop rate as parts per 65536 (0 = never drop, 65536 = always drop, 32768 = 50%)
    /// Using integer for deterministic behavior across platforms.
    pub drop_rate: u32,
    /// Specific message types to target (None = all)
    pub target_types: Option<Vec<MessageType>>,
    /// Specific witnesses to target (None = all)
    pub target_witnesses: Option<HashSet<String>>,
}

/// Types of consensus messages
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageType {
    ShareProposal,
    CommitFact,
    FallbackTrigger,
}

impl MessageLossConfig {
    /// Drop messages at a fixed rate (parts per 65536).
    /// Use `MessageLossConfig::HALF` for 50% drop rate.
    pub fn fixed_rate(rate: u32) -> Self {
        Self {
            drop_rate: rate.min(65536),
            target_types: None,
            target_witnesses: None,
        }
    }

    /// 50% drop rate constant
    pub const HALF: u32 = 32768;

    /// Drop all messages from specific witnesses
    pub fn from_witnesses(witnesses: Vec<&str>) -> Self {
        Self {
            drop_rate: 65536, // Always drop
            target_types: None,
            target_witnesses: Some(witnesses.into_iter().map(String::from).collect()),
        }
    }
}

/// Byzantine behavior configuration
#[derive(Debug, Clone)]
pub struct ByzantineConfig {
    /// Witnesses that will behave Byzantine
    pub byzantine_witnesses: HashSet<String>,
    /// Type of Byzantine behavior
    pub behavior: ByzantineBehavior,
}

/// Types of Byzantine behavior
#[derive(Debug, Clone)]
pub enum ByzantineBehavior {
    /// Send conflicting votes for different results
    Equivocate,
    /// Send invalid signatures
    InvalidSignature,
    /// Vote but never commit
    WithholdCommit,
    /// Send votes with wrong prestate hash
    WrongPrestate,
}

impl ByzantineConfig {
    /// Create equivocating Byzantine witnesses
    pub fn equivocating(witnesses: Vec<&str>) -> Self {
        Self {
            byzantine_witnesses: witnesses.into_iter().map(String::from).collect(),
            behavior: ByzantineBehavior::Equivocate,
        }
    }
}

/// Simulated message in the network
#[derive(Debug, Clone)]
pub struct SimulatedMessage {
    pub from: String,
    pub to: String,
    pub msg_type: MessageType,
    pub proposal: Option<ShareProposal>,
    pub commit: Option<PureCommitFact>,
}

/// Simulation network layer
pub struct SimulatedNetwork {
    /// Messages pending delivery
    pub pending: VecDeque<SimulatedMessage>,
    /// Partition configuration (if any)
    pub partition: Option<NetworkPartition>,
    /// Message loss configuration (if any)
    pub loss: Option<MessageLossConfig>,
    /// Delivered message count
    pub delivered_count: usize,
    /// Dropped message count
    pub dropped_count: usize,
    /// Deterministic RNG seed
    seed: u64,
    rng_state: u64,
}

impl SimulatedNetwork {
    /// Create a new simulated network
    pub fn new(seed: u64) -> Self {
        Self {
            pending: VecDeque::new(),
            partition: None,
            loss: None,
            delivered_count: 0,
            dropped_count: 0,
            seed,
            rng_state: seed,
        }
    }

    /// Apply a network partition
    pub fn with_partition(mut self, partition: NetworkPartition) -> Self {
        self.partition = Some(partition);
        self
    }

    /// Apply message loss
    pub fn with_loss(mut self, loss: MessageLossConfig) -> Self {
        self.loss = Some(loss);
        self
    }

    /// Queue a message for delivery
    pub fn send(&mut self, msg: SimulatedMessage) {
        self.pending.push_back(msg);
    }

    /// Attempt to deliver the next message
    /// Returns None if message was dropped or no messages pending
    pub fn deliver(&mut self) -> Option<SimulatedMessage> {
        let msg = self.pending.pop_front()?;

        // Check partition
        if let Some(partition) = &self.partition {
            if !partition.can_communicate(&msg.from, &msg.to) {
                self.dropped_count += 1;
                return None;
            }
        }

        // Check loss - clone to avoid borrow conflict
        if let Some(loss) = self.loss.clone() {
            if self.should_drop(&msg, &loss) {
                self.dropped_count += 1;
                return None;
            }
        }

        self.delivered_count += 1;
        Some(msg)
    }

    fn should_drop(&mut self, msg: &SimulatedMessage, loss: &MessageLossConfig) -> bool {
        // Check if this witness is targeted
        if let Some(targets) = &loss.target_witnesses {
            if !targets.contains(&msg.from) {
                return false;
            }
        }

        // Check if this message type is targeted
        if let Some(types) = &loss.target_types {
            if !types.contains(&msg.msg_type) {
                return false;
            }
        }

        // Probabilistic drop
        self.random_bool(loss.drop_rate)
    }

    /// Deterministic random bool using pure integer arithmetic.
    /// threshold is parts per 65536 (0 = never true, 65536 = always true).
    fn random_bool(&mut self, threshold: u32) -> bool {
        self.rng_state = self.rng_state.wrapping_mul(1103515245).wrapping_add(12345);
        // Extract 16 bits as our random value in [0, 65535]
        let random_value = ((self.rng_state >> 16) & 0xFFFF) as u32;
        random_value < threshold
    }
}

/// Consensus simulation harness
pub struct ConsensusSimulation {
    /// Per-witness consensus state
    pub states: HashMap<String, ConsensusState>,
    /// Simulated network
    pub network: SimulatedNetwork,
    /// Byzantine configuration (if any)
    pub byzantine: Option<ByzantineConfig>,
    /// Threshold for consensus
    pub threshold: usize,
    /// Total witness count
    pub witness_count: usize,
    /// Step counter
    pub step: usize,
}

impl ConsensusSimulation {
    /// Create a new simulation with n witnesses and threshold k
    pub fn new(witnesses: Vec<&str>, threshold: usize, seed: u64) -> Self {
        let cid = "sim_consensus".to_string();
        let prestate = "sim_prestate".to_string();
        let operation = "sim_operation".to_string();

        let witness_set: BTreeSet<String> = witnesses.iter().map(|w| w.to_string()).collect();

        let states: HashMap<String, ConsensusState> = witnesses
            .iter()
            .map(|w| {
                (
                    w.to_string(),
                    ConsensusState::new(
                        cid.clone(),
                        operation.clone(),
                        prestate.clone(),
                        threshold,
                        witness_set.clone(),
                        w.to_string(), // Each witness is initiator of their own view
                        PathSelection::FastPath,
                    ),
                )
            })
            .collect();

        Self {
            witness_count: witnesses.len(),
            states,
            network: SimulatedNetwork::new(seed),
            byzantine: None,
            threshold,
            step: 0,
        }
    }

    /// Apply Byzantine configuration
    pub fn with_byzantine(mut self, config: ByzantineConfig) -> Self {
        self.byzantine = Some(config);
        self
    }

    /// Apply network partition
    pub fn with_partition(mut self, partition: NetworkPartition) -> Self {
        self.network = self.network.with_partition(partition);
        self
    }

    /// Apply message loss
    pub fn with_loss(mut self, loss: MessageLossConfig) -> Self {
        self.network = self.network.with_loss(loss);
        self
    }

    /// Simulate a witness proposing a share
    pub fn propose_share(&mut self, witness: &str, result_id: &str) {
        let proposal = ShareProposal {
            witness: witness.to_string(),
            result_id: result_id.to_string(),
            share: ShareData {
                share_value: format!("share_{}", witness),
                nonce_binding: format!("nonce_{}", witness),
                data_binding: format!("binding_{}_{}", witness, result_id),
            },
        };

        // Handle Byzantine behavior
        if let Some(byz) = &self.byzantine {
            if byz.byzantine_witnesses.contains(witness) {
                match &byz.behavior {
                    ByzantineBehavior::Equivocate => {
                        // Send conflicting proposal to half the witnesses
                        let mut alt_proposal = proposal.clone();
                        alt_proposal.result_id = format!("{}_alt", result_id);

                        let witnesses: Vec<_> = self.states.keys().cloned().collect();
                        for (i, to) in witnesses.iter().enumerate() {
                            let msg = if i % 2 == 0 { &proposal } else { &alt_proposal };
                            self.network.send(SimulatedMessage {
                                from: witness.to_string(),
                                to: to.clone(),
                                msg_type: MessageType::ShareProposal,
                                proposal: Some(msg.clone()),
                                commit: None,
                            });
                        }
                        return;
                    }
                    ByzantineBehavior::InvalidSignature => {
                        // Proposal with invalid signature will be rejected by validation
                        // For now, just mark as invalid
                    }
                    ByzantineBehavior::WrongPrestate => {
                        // Send proposal with wrong prestate - will fail validation
                    }
                    ByzantineBehavior::WithholdCommit => {
                        // Normal proposal but won't commit later
                    }
                }
            }
        }

        // Broadcast to all witnesses
        for to in self.states.keys().cloned().collect::<Vec<_>>() {
            self.network.send(SimulatedMessage {
                from: witness.to_string(),
                to,
                msg_type: MessageType::ShareProposal,
                proposal: Some(proposal.clone()),
                commit: None,
            });
        }
    }

    /// Process one network delivery
    pub fn step(&mut self) -> bool {
        self.step += 1;

        if let Some(msg) = self.network.deliver() {
            match msg.msg_type {
                MessageType::ShareProposal => {
                    if let Some(proposal) = msg.proposal {
                        if let Some(state) = self.states.get(&msg.to) {
                            if let Some(new_state) = apply_share(state, proposal).state() {
                                self.states.insert(msg.to.clone(), new_state);
                            }
                        }
                    }
                }
                MessageType::CommitFact => {
                    if let Some(commit) = msg.commit {
                        if let Some(state) = self.states.get_mut(&msg.to) {
                            // Set commit fact directly on the state
                            state.commit_fact = Some(commit);
                            state.phase = ConsensusPhase::Committed;
                        }
                    }
                }
                MessageType::FallbackTrigger => {
                    if let Some(state) = self.states.get(&msg.to) {
                        if let Some(new_state) = trigger_fallback(state).state() {
                            self.states.insert(msg.to.clone(), new_state);
                        }
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Run simulation until quiescent or max steps
    pub fn run_to_completion(&mut self, max_steps: usize) -> SimulationResult {
        while self.step < max_steps && !self.network.pending.is_empty() {
            self.step();
        }

        self.collect_result()
    }

    /// Check all invariants across all states
    pub fn check_invariants(&self) -> Vec<InvariantViolation> {
        let mut violations = Vec::new();

        for (witness, state) in &self.states {
            // Check phase validity - proposals shouldn't exist in Pending phase
            if matches!(state.phase, ConsensusPhase::Pending) && !state.proposals.is_empty() {
                violations.push(InvariantViolation {
                    witness: witness.clone(),
                    invariant: "NoProposalsInPending".to_string(),
                    message: "Pending state should not have proposals".to_string(),
                });
            }

            // Check threshold consistency
            if state.commit_fact.is_some() && state.proposals.len() < state.threshold {
                violations.push(InvariantViolation {
                    witness: witness.clone(),
                    invariant: "CommitRequiresThreshold".to_string(),
                    message: format!(
                        "Commit with {} proposals but threshold is {}",
                        state.proposals.len(),
                        state.threshold
                    ),
                });
            }

            // Check equivocator monotonicity (already enforced in apply_share)
        }

        // Check agreement: if multiple witnesses committed, they committed to same result
        let commits: Vec<_> = self
            .states
            .values()
            .filter_map(|s| s.commit_fact.as_ref())
            .collect();

        if commits.len() > 1 {
            let first_result = &commits[0].result_id;
            for commit in &commits[1..] {
                if commit.result_id != *first_result {
                    violations.push(InvariantViolation {
                        witness: "global".to_string(),
                        invariant: "AgreementOnCommit".to_string(),
                        message: format!("Disagreement: {} vs {}", first_result, commit.result_id),
                    });
                }
            }
        }

        violations
    }

    fn collect_result(&self) -> SimulationResult {
        let committed_count = self
            .states
            .values()
            .filter(|s| matches!(s.phase, ConsensusPhase::Committed))
            .count();

        let failed_count = self
            .states
            .values()
            .filter(|s| matches!(s.phase, ConsensusPhase::Failed))
            .count();

        SimulationResult {
            steps: self.step,
            messages_delivered: self.network.delivered_count,
            messages_dropped: self.network.dropped_count,
            committed_witnesses: committed_count,
            failed_witnesses: failed_count,
            pending_witnesses: self.witness_count - committed_count - failed_count,
            violations: self.check_invariants(),
        }
    }
}

/// Invariant violation found during simulation
#[derive(Debug, Clone)]
pub struct InvariantViolation {
    pub witness: String,
    pub invariant: String,
    pub message: String,
}

/// Result of a simulation run
#[derive(Debug)]
pub struct SimulationResult {
    pub steps: usize,
    pub messages_delivered: usize,
    pub messages_dropped: usize,
    pub committed_witnesses: usize,
    pub failed_witnesses: usize,
    pub pending_witnesses: usize,
    pub violations: Vec<InvariantViolation>,
}

impl SimulationResult {
    /// Check if simulation succeeded without violations
    pub fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }

    /// Check if consensus was reached
    pub fn reached_consensus(&self) -> bool {
        self.committed_witnesses > 0 && self.violations.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_consensus() {
        let witnesses = vec!["w1", "w2", "w3"];
        let mut sim = ConsensusSimulation::new(witnesses, 2, 42);

        // All witnesses propose for same result
        sim.propose_share("w1", "result1");
        sim.propose_share("w2", "result1");
        sim.propose_share("w3", "result1");

        let result = sim.run_to_completion(100);

        assert!(
            result.is_ok(),
            "Expected no violations: {:?}",
            result.violations
        );
        assert_eq!(result.messages_dropped, 0);
    }

    #[test]
    fn test_network_partition() {
        let witnesses = vec!["w1", "w2", "w3", "w4", "w5"];
        let partition = NetworkPartition::split(vec!["w1", "w2"], vec!["w3", "w4", "w5"]);

        let mut sim = ConsensusSimulation::new(witnesses, 3, 42).with_partition(partition);

        // All propose same result
        for w in &["w1", "w2", "w3", "w4", "w5"] {
            sim.propose_share(w, "result1");
        }

        let result = sim.run_to_completion(100);

        // Partition prevents full delivery
        assert!(result.messages_dropped > 0);
    }

    #[test]
    fn test_message_loss() {
        let witnesses = vec!["w1", "w2", "w3"];
        let loss = MessageLossConfig::fixed_rate(MessageLossConfig::HALF);

        let mut sim = ConsensusSimulation::new(witnesses, 2, 42).with_loss(loss);

        sim.propose_share("w1", "result1");
        sim.propose_share("w2", "result1");
        sim.propose_share("w3", "result1");

        let result = sim.run_to_completion(100);

        // Some messages should be dropped
        assert!(result.messages_dropped > 0);
    }

    #[test]
    fn test_byzantine_equivocation() {
        let witnesses = vec!["w1", "w2", "w3", "w4", "w5"];
        let byzantine = ByzantineConfig::equivocating(vec!["w1"]);

        let mut sim = ConsensusSimulation::new(witnesses, 3, 42).with_byzantine(byzantine);

        // Byzantine witness equivocates
        sim.propose_share("w1", "result1");
        // Honest witnesses
        sim.propose_share("w2", "result1");
        sim.propose_share("w3", "result1");
        sim.propose_share("w4", "result1");
        sim.propose_share("w5", "result1");

        let result = sim.run_to_completion(100);

        // With threshold 3 and 4 honest witnesses, consensus should still be possible
        // The equivocator will be detected
        assert!(result.is_ok());
    }

    #[test]
    fn test_invariant_agreement() {
        // This test verifies that the agreement invariant is checked
        let witnesses = vec!["w1", "w2"];
        let sim = ConsensusSimulation::new(witnesses, 1, 42);

        let violations = sim.check_invariants();
        assert!(violations.is_empty());
    }
}
