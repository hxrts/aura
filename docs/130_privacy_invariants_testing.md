# RFC 130: Privacy Invariant Testing in the Simulation Engine

**Related**: [006_simulation_engine_using_injected_effects.md](006_simulation_engine_using_injected_effects.md), [080_quint_driven_chaos_testing.md](080_quint_driven_chaos_testing.md), [131_privacy_model_specification.md](131_privacy_model_specification.md)

**Status**: Revised - Aligned with RFC 131 privacy model and consent-based boundaries

## 1. Executive Summary

This document proposes an extension to the Aura simulation and chaos testing framework to enable robust, automated testing of privacy invariants. By introducing a new "Observer" participant role with probabilistic inference capabilities and leveraging the Quint formal specification engine, we can model adversarial views of the system, define privacy properties as bounded information leakage, and automatically detect privacy violations that would be missed by traditional correctness testing.

**Key insight**: Privacy is not a binary property but a spectrum of information leakage. We measure privacy in terms of adversarial confidence and bound it below acceptable thresholds.

## 2. Motivation

Aura is designed as a privacy-first system. However, verifying privacy is notoriously difficult. It is not enough to test that the system does what it's supposed to do; we must also verify that it *doesn't* leak information beyond acceptable bounds to unauthorized parties.

The existing simulation framework is excellent for testing protocol correctness and fault tolerance. The proposed Quint-driven chaos testing engine takes this a step further for automated property verification. This RFC builds on that foundation to address the unique challenge of privacy by answering the question: "How much can an adversary learn by observing the system?"

### 2.1 What Privacy Means in Aura

As detailed in RFC 131, we define privacy across five layers, each with specific testable properties:

1. **Identity Privacy**: Context-specific identities via DKD cannot be linked across contexts
   - Test metric: `identity_linkability_score` < 0.05

2. **Relationship Privacy**: Social graph structure hidden from external observers and neighbors
   - Test metric: External observer confidence < 0.01, Neighborhood confidence < 0.10
   - Note: Within-relationship visibility is consensual, not a privacy violation

3. **Group Privacy**: Membership inference bounded by k-anonymity, within-group anonymity for threshold operations
   - Test metric: Membership inference P(member | observation) ≤ 1/k

4. **Content Privacy**: E2E encryption ensures only authorized parties can decrypt
   - Test metric: Key exposure rate monitoring (binary property)

5. **Metadata Privacy**: Activity patterns obscured via cover traffic, batching, and onion routing
   - Test metric: Timing entropy > 4 bits, participation inference within 10% of base rate

This RFC focuses on **Relationship Privacy** (layer 2), **Group Privacy** (layer 3), **Identity Privacy** (layer 1), and **Metadata Privacy** (layer 5) as these have measurable adversarial inference properties. Content Privacy (layer 4) is tested separately through key management audits.

## 3. Proposed Architecture

We propose enhancing the simulation framework with three core components: a probabilistic observer model to represent realistic adversaries, a methodology for defining privacy as bounded information leakage, and enhancements to the simulation environment to support rigorous testing.

### 3.1. The Probabilistic `Observer` Model

The central idea is to introduce a new type of simulated participant: the **`Observer`**. Unlike a malicious participant who injects faults, the `Observer`'s role is to passively collect information and attempt to infer secrets using realistic inference techniques.

#### 3.1.1 Observer Types by Privacy Boundary

Following RFC 131's privacy boundary model, we define observers positioned at each boundary:

```rust
/// Observer positioned at specific privacy boundary
pub enum ObserverPosition {
    /// Within a relationship - sees consensual disclosure
    /// Used to test that within-relationship visibility works correctly
    RelationshipPartner { 
        relationship_id: RelationshipId,
        can_see_content: bool,
    },
    
    /// Gossip neighbor - sees envelope metadata only
    /// Tests neighborhood boundary privacy
    GossipNeighbor {
        peer_id: PeerId,
        /// Which hop positions in routes this peer controls
        hop_positions: Vec<usize>,
    },
    
    /// Hub node - highly connected gossip participant
    /// Tests hub-specific privacy threats
    HubNode {
        peer_id: PeerId,
        /// Number of peers this hub serves
        connectivity: usize,
    },
    
    /// External network observer - sees only Tor traffic or IP-level data
    /// Tests external boundary
    ExternalObserver {
        /// false if Tor is active
        can_see_ips: bool,
        /// e.g., "ISP-level", "nation-state"
        geographical_position: Option<String>,
    },
    
    /// Colluding gossip neighbors
    /// Tests multi-hop correlation attacks
    ColludingGossipSet {
        peer_ids: Vec<PeerId>,
        information_sharing: SharingStrategy,
    },
    
    /// Storage provider observing access patterns
    StorageProvider {
        peer_id: PeerId,
        can_see_access_patterns: bool,
    },
}

pub enum SharingStrategy {
    /// All observers pool complete information instantly
    Complete,
    /// Observers share partial information with delay
    Partial { delay: Duration, sample_rate: f64 },
    /// No coordination (independent observations)
    None,
}
```

**Key Insight**: Observers are now explicitly mapped to RFC 131's privacy boundaries. A `RelationshipPartner` observer tests consensual disclosure (should work), while `GossipNeighbor` and `ExternalObserver` test privacy boundaries (should be protected).

#### 3.1.2 Inference Model

Observers produce **probabilistic inference results** rather than binary determinations:

```rust
/// Result of an observer's inference attempt
pub struct InferenceResult {
    /// Confidence level (0.0 = no confidence, 1.0 = certain)
    pub confidence: f64,
    
    /// Evidence trail that led to this inference
    pub evidence: Vec<Evidence>,
    
    /// Type of attack used
    pub attack_type: AttackType,
    
    /// Timestamp of inference
    pub inferred_at: SimulationTime,
}

pub enum AttackType {
    /// Statistical correlation of message timing
    TimingCorrelation { window_size: Duration },
    
    /// Frequency analysis of communication patterns
    FrequencyAnalysis { threshold: u32 },
    
    /// Size correlation across multiple messages
    SizeCorrelation { sample_size: usize },
    
    /// Multi-hop relationship inference
    TransitiveInference { max_hops: u32 },
    
    /// Hub node observes large fraction of network routes
    HubTrafficAnalysis { hub_connectivity: usize },
    
    /// Machine learning-based pattern detection
    MLInference { model: String, accuracy: f64 },
}

pub enum Evidence {
    MessageObserved { from: PeerId, to: PeerId, timestamp: u64, size: usize },
    CorrelatedTiming { events: Vec<Timestamp>, correlation: f64 },
    PatternMatch { pattern: String, confidence: f64 },
    HubObservation {
        coverage_fraction: f64,
        coappearance_count: usize,
        temporal_correlation: f64,
    },
}
```

### 3.2. Defining Privacy Invariants as Bounded Leakage

Privacy invariants are defined as **upper bounds on adversarial confidence**. Rather than asserting that the adversary cannot infer something (which is impossible for systems that actually communicate), we assert that their confidence must remain below an acceptable threshold.

#### 3.2.1 Relationship Privacy Invariant

Following RFC 131's consent-based model, relationship privacy tests that external and neighborhood observers cannot infer relationships that exist, even with cover traffic:

```quint
/// Privacy thresholds by observer position
const NEIGHBORHOOD_THRESHOLD: Real = 0.10
const EXTERNAL_THRESHOLD: Real = 0.01

/// Privacy invariant: Real relationships are obscured from unauthorized observers
invariant relationshipPrivacy = {
  forall alice, bob in HonestParticipants:
    // For all observers NOT in the relationship
    forall obs in NeighborhoodObservers + ExternalObservers:
      let inference = obs.infer_relationship(alice, bob) in
        // If Alice and Bob DO have a relationship
        GroundTruth.has_relationship(alice, bob) =>
          // Neighborhood observers should have bounded confidence
          (obs.position == GossipNeighbor or obs.position == HubNode =>
            inference.confidence < NEIGHBORHOOD_THRESHOLD) and
          // External observers should have near-zero confidence
          (obs.position == ExternalObserver =>
            inference.confidence < EXTERNAL_THRESHOLD)
}
```

This invariant tests the core privacy property: **unauthorized observers cannot infer relationships with high confidence despite observing encrypted traffic**. Cover traffic, batching, and onion routing should keep inference confidence below these thresholds.

#### 3.2.2 Group-Based Cover Traffic Effectiveness

RFC 131 describes adaptive, group-based cover traffic. This invariant tests that group participation provides k-anonymity:

```quint
/// Invariant: Group-based cover traffic provides k-anonymity
invariant groupCoverTrafficEffectiveness = {
  forall group in Groups:
    forall member in group.members:
      forall obs in NeighborhoodObservers:
        // Observer sees group aggregate traffic
        let group_traffic = obs.observe_group_traffic(group) in
        // Cannot distinguish individual member's real traffic from group cover
        let member_inference = obs.infer_member_activity(member, group_traffic) in
          // Confidence bounded by k-anonymity
          member_inference.confidence <= 1.0 / group.size or
          // Unless group is too small for anonymity
          group.size < MIN_ANONYMITY_SET_SIZE
}
```

#### 3.2.3 Group Membership Privacy Invariant

Groups as micro-anonymity sets require membership inference to be bounded:

```quint
/// Invariant: Group membership inference bounded by k-anonymity
const MIN_ANONYMITY_SET_SIZE: Int = 3

invariant groupMembershipPrivacy = {
  forall group in Groups:
    forall member in group.members:
      forall obs in NeighborhoodObservers + ExternalObservers:
        // Observer's confidence that member is in group
        let inference = obs.infer_group_membership(member, group) in
          // Should not exceed k-anonymity bound
          inference.confidence <= 1.0 / group.size or
          // Unless group is too small to provide anonymity
          group.size < MIN_ANONYMITY_SET_SIZE
}

/// Invariant: Within-group attribution for anonymous operations
invariant withinGroupAnonymity = {
  forall group in Groups:
    forall operation in group.anonymous_operations:
      forall member in group.members:
        // Even group insiders cannot attribute operation to specific member
        forall insider in group.members where insider != member:
          let inference = insider.infer_author(operation) in
            // Confidence bounded by threshold k
            inference.confidence <= 1.0 / operation.threshold_k
}
```

#### 3.2.4 Identity Unlinkability Invariant

Context-specific identities via DKD must be unlinkable across contexts:

```quint
/// Invariant: Context-specific identities cannot be linked
const IDENTITY_LINKING_THRESHOLD: Real = 0.05

invariant identityUnlinkability = {
  forall account in Accounts:
    forall context_a, context_b in account.contexts where context_a != context_b:
      forall obs in AllObservers:
        // Observer sees identities in both contexts
        let id_a = account.identity_in(context_a) in
        let id_b = account.identity_in(context_b) in
          // Cannot link them with high confidence
          let inference = obs.infer_same_account(id_a, id_b) in
            inference.confidence < IDENTITY_LINKING_THRESHOLD
}
```

#### 3.2.5 Consensual Disclosure Invariant

Within-relationship visibility should work correctly (positive test):

```quint
/// Invariant: Within-relationship visibility works correctly
/// This tests that consensual disclosure provides expected visibility
invariant consensualDisclosureWorks = {
  forall alice, bob in HonestParticipants:
    let shared_ctx = shared_context(alice, bob) in
    GroundTruth.has_relationship(alice, bob) =>
      // Alice CAN see Bob's activity in their shared context
      alice.can_see_activity(bob, shared_ctx) and
      bob.can_see_activity(alice, shared_ctx) and
      // But Alice CANNOT see Bob's activity in other contexts
      forall other_ctx in bob.contexts where not shared_ctx.includes(other_ctx):
        not alice.can_see_activity(bob, other_ctx)
}
```

#### 3.2.6 Differential Privacy Invariant (Future Work)

```quint
/// Invariant: Adding/removing one participant changes observer's 
/// view by at most epsilon
invariant differentialPrivacy(epsilon: Real) = {
  forall adjacent_worlds W1, W2:
    // W1 and W2 differ by exactly one participant's data
    differs_by_one_participant(W1, W2) =>
      forall obs in Observers:
        // Observer's inferences in both worlds must be similar
        distance(obs.infer_in(W1), obs.infer_in(W2)) <= epsilon
}
```

### 3.3. Simulation Environment Enhancements

#### 3.3.1 Test-Only Ground Truth Oracle

The `GroundTruthOracle` maintains the definitive source of truth about the system's secrets. This is **strictly test infrastructure** and isolated from production code.

```rust
/// Test-only oracle that knows all system secrets
/// 
/// IMPORTANT: This type is only available in #[cfg(test)] and must
/// never be imported or referenced in production code.
#[cfg(test)]
pub struct GroundTruthOracle {
    /// Complete social graph (who actually communicates with whom)
    relationship_graph: BTreeMap<(UserId, UserId), RelationshipMetadata>,
    
    /// Plaintext of all messages (for verification only)
    message_plaintext: BTreeMap<MessageId, Vec<u8>>,
    
    /// Actual user identities (for unlinkability testing)
    identity_map: BTreeMap<PseudonymId, UserId>,
}

#[cfg(test)]
impl GroundTruthOracle {
    /// Check if two participants have a relationship
    pub fn has_relationship(&self, a: UserId, b: UserId) -> bool {
        self.relationship_graph.contains_key(&(a, b)) ||
        self.relationship_graph.contains_key(&(b, a))
    }
    
    /// Get relationship metadata (frequency, duration, etc.)
    pub fn relationship_metadata(&self, a: UserId, b: UserId) 
        -> Option<&RelationshipMetadata> {
        self.relationship_graph.get(&(a, b))
            .or_else(|| self.relationship_graph.get(&(b, a)))
    }
}
```

**Isolation Guarantees**:
- Located in `crates/simulator/src/testing/` (not in production crates)
- Only compiled when `#[cfg(test)]` is active
- Never exposed through public APIs
- Static analysis enforced in CI to prevent production imports

#### 3.3.2 Detailed Event Logs for Observers

Updated to reflect Aura's P2P gossip architecture (not relay-based):

```rust
/// Events that observers can witness
#[derive(Debug, Clone)]
pub enum ObservableEvent {
    /// Gossip envelope forwarded (neighborhood view)
    EnvelopeForwarded {
        from_peer: PeerId,      // previous hop
        to_peer: PeerId,        // next hop
        rtag: RtagId,           // rotating routing tag
        size: usize,            // fixed size (always 16KB)
        timestamp: SimulationTime,
        hop_count: Option<u8>,  // if visible in onion layer
    },
    
    /// Envelope received at destination
    EnvelopeReceived {
        recipient: PeerId,
        rtag: RtagId,
        timestamp: SimulationTime,
    },
    
    /// Cover traffic generated
    CoverTrafficSent {
        from: PeerId,
        rtag: RtagId,
        timestamp: SimulationTime,
    },
    
    /// Relationship established (only visible to partners)
    RelationshipEstablished {
        party_a: PeerId,
        party_b: PeerId,
        context: ContextId,
        timestamp: SimulationTime,
    },
    
    /// Data was stored
    StorageWrite {
        peer: PeerId,
        chunk_id: ChunkId,
        size: usize,
        timestamp: SimulationTime,
    },
    
    /// Data was retrieved
    StorageRead {
        peer: PeerId,
        chunk_id: ChunkId,
        timestamp: SimulationTime,
    },
    
    /// Participant came online
    ParticipantOnline {
        peer: PeerId,
        timestamp: SimulationTime,
    },
    
    /// Participant went offline
    ParticipantOffline {
        peer: PeerId,
        timestamp: SimulationTime,
    },
}

/// Log of all observable events in the simulation
pub struct ObservationLog {
    events: Vec<ObservableEvent>,
    /// Efficient index for querying by time window
    time_index: BTreeMap<SimulationTime, Vec<usize>>,
    /// Index for querying by participant
    participant_index: BTreeMap<PeerId, Vec<usize>>,
    /// Index for querying by rtag (routing tag)
    rtag_index: BTreeMap<RtagId, Vec<usize>>,
}
```

**Key Changes**: Events now model gossip envelope forwarding with rtags, onion routing hop counts, and cover traffic. This matches Aura's P2P architecture rather than assuming relay nodes.

#### 3.3.3 Declarative Adversary Models

Extended TOML scenario format for privacy testing:

```toml
[scenario.privacy]
# Enable privacy invariant testing
enabled = true

# Privacy thresholds by boundary (0.0 - 1.0)
neighborhood_threshold = 0.10  # Gossip neighbors
external_threshold = 0.01      # External observers
identity_linking_threshold = 0.05  # Cross-context identity linking

# Observer configurations
[[scenario.privacy.observers]]
type = "ExternalObserver"
name = "isp_level_adversary"
can_see_ips = false  # Tor is active
geographical_position = "ISP-level"

# Inference techniques this observer uses
[[scenario.privacy.observers.techniques]]
type = "TimingCorrelation"
window_size_secs = 60
min_correlation = 0.7

[[scenario.privacy.observers.techniques]]
type = "FrequencyAnalysis"
message_threshold = 5
time_window_secs = 300

# Gossip neighbor observer
[[scenario.privacy.observers]]
type = "GossipNeighbor"
name = "compromised_peer_42"
peer_id = "peer_42"
hop_positions = [1, 2]  # Controls hops 1 and 2 in some routes

[[scenario.privacy.observers.techniques]]
type = "TimingCorrelation"
window_size_secs = 120
min_correlation = 0.6

# Hub node observer
[[scenario.privacy.observers]]
type = "HubNode"
name = "high_connectivity_hub"
peer_id = "peer_999"
connectivity = 150  # Serves 150 peers

[[scenario.privacy.observers.techniques]]
type = "HubTrafficAnalysis"
coverage_threshold = 0.3  # Hub sees 30% of network traffic

# Colluding gossip neighbor set
[[scenario.privacy.observers]]
type = "ColludingGossipSet"
name = "three_compromised_neighbors"
members = ["peer_42", "peer_117", "peer_203"]
information_sharing = "complete"  # or "partial", "delayed"

[[scenario.privacy.observers.techniques]]
type = "TransitiveInference"
max_hops = 3

# Sampling strategy to avoid O(N²) complexity
[scenario.privacy.sampling]
strategy = "random_pairs"
sample_size = 100  # Check 100 random pairs per step
# Or: strategy = "incremental" - only check when relevant events occur

# Check frequency
check_interval_steps = 10  # Only check every 10 simulation steps
```

## 4. Realistic Inference Techniques

To ensure our testing actually catches privacy leaks, we implement realistic adversary capabilities.

### 4.1 Timing Correlation Attack

```rust
impl NetworkObserver {
    /// Infer relationships based on timing correlation
    pub fn timing_correlation_attack(
        &self,
        user_a: UserId,
        user_b: UserId,
        window: Duration,
    ) -> InferenceResult {
        // Collect all messages involving either user
        let events_a = self.observations.messages_from(user_a, window);
        let events_b = self.observations.messages_to(user_b, window);
        
        // Compute statistical correlation between timing patterns
        let correlation = compute_pearson_correlation(
            &events_a.iter().map(|e| e.timestamp).collect::<Vec<_>>(),
            &events_b.iter().map(|e| e.timestamp).collect::<Vec<_>>(),
        );
        
        // High correlation suggests communication pattern
        let confidence = if correlation > 0.7 {
            correlation
        } else {
            0.0
        };
        
        InferenceResult {
            confidence,
            evidence: vec![
                Evidence::CorrelatedTiming {
                    events: events_a.iter().chain(&events_b)
                        .map(|e| e.timestamp)
                        .collect(),
                    correlation,
                }
            ],
            attack_type: AttackType::TimingCorrelation { window_size: window },
            inferred_at: self.current_time(),
        }
    }
}
```

### 4.2 Frequency Analysis Attack

```rust
impl NetworkObserver {
    /// Infer relationships based on message frequency
    pub fn frequency_analysis_attack(
        &self,
        user_a: UserId,
        user_b: UserId,
        threshold: u32,
    ) -> InferenceResult {
        // Count messages between these users in recent window
        let message_count = self.observations
            .count_messages_between(user_a, user_b, Duration::from_secs(3600));
        
        // Simple threshold-based inference
        let confidence = if message_count > threshold {
            (message_count as f64 / (threshold as f64 * 2.0)).min(1.0)
        } else {
            0.0
        };
        
        let evidence: Vec<Evidence> = self.observations
            .messages_between(user_a, user_b, Duration::from_secs(3600))
            .iter()
            .map(|msg| Evidence::MessageObserved {
                from: msg.from,
                to: msg.to,
                timestamp: msg.timestamp,
                size: msg.size,
            })
            .collect();
        
        InferenceResult {
            confidence,
            evidence,
            attack_type: AttackType::FrequencyAnalysis { threshold },
            inferred_at: self.current_time(),
        }
    }
}
```

### 4.3 Transitive Inference Attack

```rust
impl ColludingGossipSet {
    /// Infer relationships through multi-hop analysis
    /// If A talks to B frequently, and B talks to C frequently,
    /// infer that A and C might be related
    pub fn transitive_inference_attack(
        &self,
        user_a: UserId,
        user_c: UserId,
        max_hops: u32,
    ) -> InferenceResult {
        // Build communication graph from pooled observations
        let graph = self.build_communication_graph();
        
        // Find paths between A and C
        let paths = graph.find_paths(user_a, user_c, max_hops);
        
        // Confidence based on path strength and length
        let confidence = paths.iter().map(|path| {
            let strength = path.edges.iter()
                .map(|e| e.weight)
                .product::<f64>();
            // Decay confidence with path length
            strength / (path.length() as f64)
        }).max().unwrap_or(0.0);
        
        InferenceResult {
            confidence,
            evidence: paths.into_iter().flat_map(|p| {
                p.edges.into_iter().map(|e| Evidence::MessageObserved {
                    from: e.from,
                    to: e.to,
                    timestamp: e.timestamp,
                    size: e.size,
                })
            }).collect(),
            attack_type: AttackType::TransitiveInference { max_hops },
            inferred_at: self.current_time(),
        }
    }
}
```

### 4.4 Hub Node Traffic Analysis Attack

Hub nodes see a large fraction of network traffic and can perform sophisticated correlation:

```rust
impl HubNodeObserver {
    /// Hub-specific attack: observe large fraction of network traffic
    pub fn hub_traffic_analysis(
        &self,
        target_a: PeerId,
        target_b: PeerId,
    ) -> InferenceResult {
        // Hub sees X% of all network routes
        let coverage = self.calculate_route_coverage();
        
        // Count how often A and B appear in routes the hub sees
        let coappearance = self.count_coappearance_in_routes(target_a, target_b);
        
        // Analyze temporal correlation of envelopes
        let temporal_correlation = self.analyze_temporal_patterns(target_a, target_b);
        
        // Higher coverage + higher coappearance + temporal correlation = higher confidence
        let base_confidence = (coappearance as f64 / coverage.total_routes as f64)
            .min(1.0);
        
        let confidence = (base_confidence * 0.6 + temporal_correlation * 0.4)
            .min(1.0);
        
        InferenceResult {
            confidence,
            evidence: vec![
                Evidence::HubObservation {
                    coverage_fraction: coverage.fraction,
                    coappearance_count: coappearance,
                    temporal_correlation,
                }
            ],
            attack_type: AttackType::HubTrafficAnalysis {
                hub_connectivity: self.connectivity,
            },
            inferred_at: self.current_time(),
        }
    }
    
    /// Calculate what fraction of network routes pass through this hub
    fn calculate_route_coverage(&self) -> RouteCoverage {
        let total_routes = self.observations.count_unique_routes();
        let routes_through_hub = self.observations
            .count_routes_containing_peer(self.peer_id);
        
        RouteCoverage {
            total_routes,
            routes_through_hub,
            fraction: routes_through_hub as f64 / total_routes as f64,
        }
    }
    
    /// Count how often two peers appear in the same observed routes
    fn count_coappearance_in_routes(
        &self,
        peer_a: PeerId,
        peer_b: PeerId,
    ) -> usize {
        self.observations
            .routes_containing_both(peer_a, peer_b)
            .len()
    }
    
    /// Analyze temporal correlation between envelope observations
    fn analyze_temporal_patterns(
        &self,
        peer_a: PeerId,
        peer_b: PeerId,
    ) -> f64 {
        let events_a = self.observations.envelope_events_for(peer_a);
        let events_b = self.observations.envelope_events_for(peer_b);
        
        compute_pearson_correlation(
            &events_a.iter().map(|e| e.timestamp).collect(),
            &events_b.iter().map(|e| e.timestamp).collect(),
        ).abs()
    }
}

pub struct RouteCoverage {
    total_routes: usize,
    routes_through_hub: usize,
    fraction: f64,
}
```

## 5. Computational Efficiency

To handle large simulations (1000+ participants), we employ several strategies:

### 5.1 Sampling Strategies

```rust
pub enum SamplingStrategy {
    /// Check all pairs (only for small simulations)
    Exhaustive,
    
    /// Random sample of N pairs per check
    RandomPairs { sample_size: usize },
    
    /// Only check pairs involved in recent events
    Incremental { event_window: Duration },
    
    /// Adaptive sampling - increase checks when leakage detected
    Adaptive {
        base_sample_size: usize,
        boost_factor: f64,
        boost_duration: Duration,
    },
}

impl PrivacyMonitor {
    pub fn check_invariants_with_sampling(
        &mut self,
        strategy: &SamplingStrategy,
    ) -> Vec<PrivacyViolation> {
        match strategy {
            SamplingStrategy::RandomPairs { sample_size } => {
                // O(sample_size) instead of O(N²)
                let pairs = self.sample_random_pairs(*sample_size);
                self.check_pairs(pairs)
            }
            SamplingStrategy::Incremental { event_window } => {
                // Only check pairs involved in recent events
                let active_participants = self.get_active_participants(*event_window);
                let pairs = active_participants.iter()
                    .flat_map(|a| active_participants.iter().map(move |b| (*a, *b)))
                    .collect();
                self.check_pairs(pairs)
            }
            // ... other strategies
        }
    }
}
```

### 5.2 Incremental Checking

```rust
/// Only re-check invariants when relevant events occur
pub struct IncrementalPrivacyMonitor {
    /// Cache of previous inference results
    inference_cache: BTreeMap<(UserId, UserId), CachedInference>,
    
    /// Dirty set of pairs that need rechecking
    dirty_pairs: HashSet<(UserId, UserId)>,
}

impl IncrementalPrivacyMonitor {
    /// Mark pairs as dirty when events occur
    pub fn on_event(&mut self, event: &ObservableEvent) {
        match event {
            ObservableEvent::MessageTransmit { from, to, .. } => {
                // Mark this pair and all pairs involving these users as dirty
                for user in self.all_users() {
                    self.dirty_pairs.insert((*from, user));
                    self.dirty_pairs.insert((user, *from));
                    self.dirty_pairs.insert((*to, user));
                    self.dirty_pairs.insert((user, *to));
                }
            }
            // ... handle other events
        }
    }
    
    /// Only check dirty pairs
    pub fn check_dirty_pairs(&mut self) -> Vec<PrivacyViolation> {
        let pairs_to_check: Vec<_> = self.dirty_pairs.drain().collect();
        self.check_pairs(pairs_to_check)
    }
}
```

### 5.3. Adversary Model Calibration (Post-MVP)

The effectiveness of this framework hinges on the quality of the inference models. An uncalibrated model could lead to a false sense of security or a flood of false positives. To ensure our observers are realistic and their confidence scores are meaningful, we will introduce a calibration phase.

**Process:**
Before running a scenario, the simulation will execute a series of baseline tests to calibrate the observers for that specific network configuration and workload.

1.  **100% Signal Baseline**: The simulation runs with two participants communicating with **no cover traffic**. The resulting confidence score from an observer becomes the benchmark for a "perfectly detectable" event. This helps validate the upper bound of the model.
2.  **100% Noise Baseline**: The simulation runs with **only cover traffic** and no real communication between target participants. The confidence scores should remain close to zero. This helps identify flaws or biases in the inference models themselves.

These calibration steps provide a dynamic, scenario-specific baseline, allowing us to set more accurate and meaningful `PRIVACY_THRESHOLD` values.

## 6. Example Workflow

Here is a step-by-step example of how relationship privacy would be tested:

1. **Define Invariant**: The `relationshipPrivacy` invariant is defined in a Quint file with neighborhood threshold of 0.10 and external threshold of 0.01.

2. **Create Scenario**: A declarative TOML scenario specifies:
   - `GossipNeighbor` observer at peer_42 with `TimingCorrelation` attack
   - `HubNode` observer at peer_999 (connectivity: 150) with `HubTrafficAnalysis`
   - `ExternalObserver` (ISP-level) with Tor active
   - Neighborhood threshold: 0.10
   - External threshold: 0.01
   - Sampling strategy: 100 random pairs per check
   - Check interval: every 10 steps

3. **(Post-MVP) Calibration Phase**: The `ScenarioEngine` first runs the baseline signal/noise tests to calibrate the observers' confidence models for this specific scenario.

4. **Run Simulation**: The `ScenarioEngine` runs the main simulation:
   - Alice and Bob establish a relationship and exchange 20 envelopes
   - Alice and Bob's group generates adaptive cover traffic
   - Charlie and Dave have no relationship but are in other groups with cover traffic
   - All envelopes are fixed-size (16KB) with rotating rtags
   - Envelopes are onion-routed through multiple hops

5. **Observer Collects Data**: 
   - The `GossipNeighbor` at peer_42 observes envelopes forwarded through it
   - The `HubNode` at peer_999 sees ~30% of all network routes
   - The `ExternalObserver` sees only encrypted Tor traffic (no IP correlation possible)

6. **Property Monitoring**: Every 10 steps, the `PropertyMonitor`:
   - Samples 100 random participant pairs
   - For each pair, runs all configured inference attacks per observer type
   - Computes confidence scores

7. **Inference and Verification**:
   - `GossipNeighbor.timing_correlation_attack(Alice, Bob)` returns confidence 0.08 (below 0.10 - pass)
   - `HubNode.hub_traffic_analysis(Alice, Bob)` returns confidence 0.15 (above 0.10 - violation!)
   - `ExternalObserver.timing_correlation_attack(Alice, Bob)` returns confidence 0.005 (below 0.01 - pass)

8. **Violation Detected**:
   - The hub node observer exceeded neighborhood threshold for Alice-Bob
   - Check ground truth: Alice and Bob DO have relationship
   - **Privacy violation**: Hub's high route coverage defeated cover traffic
   - Confidence: 0.15, Threshold: 0.10, Delta: +0.05
   - Attack: HubTrafficAnalysis with 30% route coverage

9. **Debugging with Visualization**:
   - System creates checkpoint at violation point
   - `TimeTravelDebugger` is activated
   - The debugger generates hub-specific visualizations:
     - Route coverage map showing which routes pass through the hub
     - Temporal correlation plot for Alice and Bob's envelope patterns as seen by the hub
     - Coappearance frequency chart
   - The developer can inspect:
     - The generated visualization showing hub advantage
     - The raw `Evidence` trail with HubObservation details
     - Routing decisions (were routes insufficiently diverse?)
     - Cover traffic strategy (did groups avoid the hub?)
   - The debugger attempts to generate a minimal reproduction scenario
   - Recommendation: Implement hub avoidance in routing algorithm or increase cover traffic rate for high-visibility routes

## 7. Testing Cover Traffic Effectiveness

A critical component is verifying that privacy defenses actually work:

```rust
#[cfg(test)]
mod cover_traffic_tests {
    use super::*;
    
    #[test]
    fn test_group_cover_traffic_defeats_frequency_analysis() {
        // Setup: Alice and Bob in a 10-member group exchange 15 real envelopes
        let scenario = Scenario::new()
            .add_group("group_g", 10)
            .add_relationship("alice", "bob", EnvelopeCount(15))
            .with_group_membership("alice", "group_g")
            .with_group_membership("bob", "group_g")
            .with_cover_traffic(CoverTrafficStrategy::AdaptiveGroupBased {
                group_rate_multiplier: 2.0,  // Group generates 2x member activity
            })
            .with_observer(GossipNeighbor::new("peer_42", vec![
                AttackType::FrequencyAnalysis { threshold: 10 }
            ]));
        
        let result = scenario.run();
        
        // Verify: Observer confidence should be bounded by k-anonymity
        let inference = result.observers[0]
            .infer_relationship("alice", "bob");
        
        assert!(
            inference.confidence < 0.10,
            "Group cover traffic failed: neighborhood confidence {} exceeds threshold",
            inference.confidence
        );
    }
    
    #[test]
    fn test_onion_routing_defeats_hub_correlation() {
        // Hub node sees 40% of routes but onion routing + cover traffic prevent inference
        let scenario = Scenario::new()
            .add_relationship("alice", "bob", EnvelopeCount(25))
            .with_network_topology(TopologyConfig {
                hub_nodes: vec![("peer_999", 150)],  // 150 connections
                routing_strategy: RoutingStrategy::HubAware {
                    max_hub_reuse: 0.2,  // Use hub in max 20% of routes
                },
            })
            .with_cover_traffic(CoverTrafficStrategy::AdaptiveGroupBased {
                group_rate_multiplier: 2.0,
            })
            .with_observer(HubNode::new("peer_999", 150, vec![
                AttackType::HubTrafficAnalysis { hub_connectivity: 150 },
                AttackType::TimingCorrelation { window_size: Duration::from_secs(120) }
            ]));
        
        let result = scenario.run();
        
        let inference = result.observers[0]
            .infer_relationship("alice", "bob");
        
        assert!(
            inference.confidence < 0.10,
            "Hub mitigation failed: hub confidence {} exceeds threshold. \
             Consider: more aggressive hub avoidance or increased cover traffic",
            inference.confidence
        );
    }
    
    #[test]
    fn test_tor_defeats_external_observer() {
        // External observer with Tor active should have near-zero confidence
        let scenario = Scenario::new()
            .add_relationship("alice", "bob", EnvelopeCount(30))
            .with_tor_enabled(true)
            .with_observer(ExternalObserver::new(false, vec![  // can_see_ips = false (Tor)
                AttackType::TimingCorrelation { window_size: Duration::from_secs(60) },
                AttackType::FrequencyAnalysis { threshold: 15 }
            ]));
        
        let result = scenario.run();
        
        let inference = result.observers[0]
            .infer_relationship("alice", "bob");
        
        assert!(
            inference.confidence < 0.01,
            "Tor protection failed: external confidence {} exceeds threshold",
            inference.confidence
        );
    }
    
    #[test]
    fn test_consensual_disclosure_works() {
        // Within-relationship visibility should work (positive test)
        let scenario = Scenario::new()
            .add_relationship("alice", "bob", EnvelopeCount(20))
            .with_observer(RelationshipPartner::new("alice", "alice-bob-relationship"));
        
        let result = scenario.run();
        
        // Alice SHOULD be able to see Bob's activity in their shared context
        assert!(
            result.observers[0].can_see_activity("bob", "alice-bob-relationship"),
            "Consensual disclosure broken: Alice cannot see Bob's activity in shared context"
        );
        
        // But Alice should NOT see Bob in other contexts
        assert!(
            !result.observers[0].can_see_activity("bob", "bob-other-context"),
            "Context isolation broken: Alice can see Bob in unshared context"
        );
    }
}
```

## 8. Future Enhancements

### 8.1 Machine Learning-Based Inference

Integrate actual ML models as adversaries:

```rust
pub struct MLInferenceAttack {
    /// Pre-trained model for relationship inference
    model: Box<dyn RelationshipInferenceModel>,
    
    /// Feature extractor
    features: Box<dyn FeatureExtractor>,
}

impl MLInferenceAttack {
    pub fn infer(&self, observations: &ObservationLog) -> InferenceResult {
        // Extract features from observations
        let features = self.features.extract(observations);
        
        // Run model inference
        let prediction = self.model.predict(&features);
        
        InferenceResult {
            confidence: prediction.probability,
            evidence: vec![Evidence::MLPrediction {
                features: features.summary(),
                model_name: self.model.name(),
            }],
            attack_type: AttackType::MLInference {
                model: self.model.name(),
                accuracy: self.model.accuracy(),
            },
            inferred_at: observations.latest_timestamp(),
        }
    }
}
```

### 8.2 Differential Privacy Verification

Implement formal DP guarantees:

```rust
pub struct DifferentialPrivacyVerifier {
    epsilon: f64,
    delta: f64,
}

impl DifferentialPrivacyVerifier {
    /// Verify epsilon-delta DP by running simulation on adjacent datasets
    pub fn verify(&self, scenario: &Scenario) -> DPVerificationResult {
        // Run scenario with participant N present
        let world_with_n = scenario.run_with_participant("participant_n");
        
        // Run scenario with participant N absent
        let world_without_n = scenario.run_without_participant("participant_n");
        
        // Compare observer inferences in both worlds
        let distance = self.compute_inference_distance(
            &world_with_n.observer_inferences,
            &world_without_n.observer_inferences,
        );
        
        DPVerificationResult {
            satisfies_dp: distance <= self.epsilon,
            measured_epsilon: distance,
            target_epsilon: self.epsilon,
            delta: self.delta,
        }
    }
}
```

### 8.3 Information-Theoretic Metrics

Measure privacy using information theory:

```rust
pub struct InformationLeakageAnalyzer {
    /// Measure mutual information between observations and secrets
    pub fn mutual_information(
        &self,
        observations: &ObservationLog,
        ground_truth: &GroundTruthOracle,
    ) -> f64 {
        // I(Observations; Secrets) = H(Secrets) - H(Secrets | Observations)
        let h_secrets = self.entropy(ground_truth);
        let h_conditional = self.conditional_entropy(ground_truth, observations);
        h_secrets - h_conditional
    }
    
    /// Maximum leakage bound
    const MAX_LEAKAGE_BITS: f64 = 2.0;  // At most 2 bits of information leaked
}
```

### 8.4. Adversarial Evolution of Attacks (Post-MVP)

To mitigate the risk of "teaching to the test"—where defenses are narrowly tuned to pass a fixed set of attacks—we can use the chaos engine to create a co-evolutionary arms race.

Instead of using fixed attack parameters, the `ChaosTestGenerator` can be tasked with finding the *most effective* parameters for a given scenario. It will vary parameters like `window_size` or `message_threshold` to maximize the observer's confidence score. If it finds a parameter set that causes a violation, it flags a more robust and dangerous privacy leak. This forces developers to build more general and resilient privacy defenses.

## 9. Conclusion

By extending the simulation framework with boundary-aware probabilistic observer models and bounded information leakage properties, we move from hoping the system is private to rigorously testing specific, measurable privacy guarantees aligned with RFC 131's consent-based privacy model.

Key improvements over naive privacy testing:

1. **Boundary-Aware Testing**: Observers positioned at specific privacy boundaries (relationship, neighborhood, external) with appropriate thresholds for each
2. **Consent-Based Model**: Tests distinguish between consensual disclosure (within relationships) and privacy violations (unauthorized inference)
3. **Realistic Adversaries**: Models actual adversary positions (gossip neighbors, hub nodes, external observers) with realistic inference techniques
4. **Architecture-Aligned**: Tests reflect Aura's P2P gossip architecture with envelopes, rtags, onion routing, not generic "network observers"
5. **Multi-Layer Privacy**: Tests all five privacy layers from RFC 131 (identity, relationship, group, content, metadata)
6. **Group Privacy**: K-anonymity testing for groups as micro-anonymity sets
7. **Hub Node Awareness**: Specific tests for hub node threat model and mitigation strategies
8. **Testable Defenses**: Verify group-based cover traffic, onion routing diversity, and Tor integration actually work
9. **Computational Feasibility**: Sampling strategies for large-scale simulation
10. **Future-Proof**: Foundation for DP verification and information-theoretic analysis

**Integration with Privacy Model**: This testing framework implements the testable metrics defined in RFC 131:
- Identity linkability score < 0.05 across contexts
- Relationship inference confidence < 0.10 for neighborhood, < 0.01 for external
- Group membership inference bounded by k-anonymity (≤ 1/k)
- Metadata leakage bounded by timing entropy and participation inference thresholds

This approach integrates seamlessly with the Quint-driven chaos testing engine, allowing us to leverage formal methods for both correctness and privacy verification. By grounding privacy tests in Aura's specific architecture and consent-based model, we ensure that tests catch real privacy violations rather than false positives from consensual disclosure.
