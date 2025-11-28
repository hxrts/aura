# Privacy and Information Flow

Aura's privacy model is consent-based and articulated as a unified information-flow system. Privacy boundaries align with social relationships rather than technical perimeters. Violations occur when information crosses trust boundaries without consent. Acceptable flows consume explicitly budgeted headroom.

This document specifies Aura's privacy and information-flow model covering privacy leakage and spam as one regulatory system. It defines what information is visible to different observers, how properties degrade under attack, and how enforcement works via a monotone semilattice-based budget. Per-context per-peer flow headroom is charged before any transport side effect. See [Flow Budget System](003_information_flow_contract.md#flow-budget-system-reference) for the full budget specification.

## Privacy Philosophy

Traditional privacy systems force users to choose between complete isolation and complete exposure. Aura recognizes that privacy is relational. Sharing information with trusted parties is not a privacy violation. It is the foundation of meaningful collaboration.

The system implements three core principles. Consensual Disclosure means you consent to sharing certain information when you join a group or establish a relationship. This is the basis for coordination, not a privacy leak. Contextual Identity means through Deterministic Key Derivation you present different identities in different contexts. Only relationship parties can link these identities, preventing global identity correlation. Neighborhood Visibility means peer-to-peer gossip neighbors observe encrypted envelope metadata. This is acceptable because context isolation prevents linking across contexts and information-flow budgets hard-limit observable rate and volume per context.

## Unified Information-Flow Budget

Aura enforces privacy leakage limits and spam resistance with the same monotone mechanism. For each context and peer pair, the journal records charge facts that contribute to a conceptual budget:

```
FlowBudget {
    limit: u64, // derived at runtime
    spent: u64, // replicated fact (join = max)
    epoch: Epoch, // replicated fact
}
```

Only `spent` and `epoch` values appear as facts inside the journal. The `limit` field is computed at runtime by intersecting Biscuit-derived capabilities with sovereign policy; it is never stored in the journal. Before any transport effect, `FlowGuard` charges `cost` to the context and peer pair. Guard evaluation runs over a prepared snapshot and emits commands for an interpreter to execute, keeping the guard path pure. If `spent` plus `cost` exceeds the computed `limit`, the send is blocked locally with no observable behavior.

For multi-hop forwarding, relays validate a signed per-hop receipt from the previous hop, then charge their own budget before forwarding. Limits update deterministically via the shared Biscuit/policy evaluation on every device, so replicas converge even though only `spent` charges are recorded. Guard evaluation follows the sequence described in [Authorization](109_authorization.md).

### Time Domain Semantics and Leakage

- **PhysicalClock** timestamps are used for guard-chain charging, receipts, and cooldowns. They are obtained exclusively through `PhysicalTimeEffects` implementations (production or simulator) and never via direct `SystemTime::now()`.
- **LogicalClock** is used for CRDT causal delivery and journal conflict resolution; it does not leak wall-clock information.
- **OrderClock** provides a privacy-preserving total order for cases where ordering is required without causal or wall-clock meaning.
- **Range** expresses validity windows (e.g., dispute/cooldown periods) and is derived from physical time plus policy-defined uncertainty.

Cross-domain comparisons are explicit via `TimeStamp::compare(policy)`; the system never infers ordering across domains implicitly. Facts store `TimeStamp` directly (legacy FactId removed), keeping time semantics auditable within the leakage model.

### Budget Formulas

The limit for a context and peer is computed as:

```
limit(ctx, peer) = base(ctx) ⊓ policy(ctx) ⊓ role(ctx, peer)
                   ⊓ relay_factor(ctx) ⊓ peer_health(peer)
```

The `base(ctx)` is the context class default. The `policy(ctx)` is local sovereign policy set by the account. The `role(ctx, peer)` is the per-peer role within the context. The `relay_factor(ctx)` reduces limit for high-degree hubs to mitigate amplification. The `peer_health(peer)` term shrinks when an authority is marked unhealthy or overloaded.

Each term is encoded as a lattice element. Merges occur via meet, ensuring convergence and preventing widening.

### Receipts and Epochs

Per-hop receipts are required for forwarding and bound to the epoch:

```rust
Receipt {
  ctx: ContextId,
  src: AuthorityId,
  dst: AuthorityId,
  epoch: Epoch,
  cost: u32,
  nonce: u64,
  prev: Hash32,
  sig: Signature,
}
```

The acceptance window is the current epoch only. Rotation triggers when the journal fact `Epoch(ctx)` increments. On rotation, `spent(ctx, *)` resets and receipts cannot be reused across epochs.

### Fairness and Liveness

At most one outstanding reservation exists per context and device. Provide a minimum headroom floor for active relationships to prevent starvation. Optional jitter on sensitive protocols like guardian recovery reduces timing correlation without violating monotonicity.

This unifies who may talk, how often, and with how much metadata leakage under the same laws as capabilities and facts. See [Flow Budget System](003_information_flow_contract.md#flow-budget-system-reference) for implementation details.

## Privacy Boundaries

Aura defines four distinct privacy boundaries each with different visibility properties.

### Relationship Boundary

When you establish a direct relationship or join a group, you cross into the relationship boundary. Within this boundary, both parties have consented to share information necessary for coordination.

Participants can see your context-specific identity, your online status, your participation in shared activities, and message content. This visibility is by design, enabling collaboration. Within a relationship, participants cannot link your identity across different contexts unless you explicitly share that information. Deterministic Key Derivation ensures each relationship gets a unique identity derived from account key, app ID, and context label.

Each context and peer pair maintains a flow budget where only the `spent` charges are replicated. Every send charges `spent` before any transport effect. If `spent` plus `cost` exceeds the locally computed `limit`, the send is blocked with no observable behavior. This bounds metadata exposure and prevents spam by construction.

### Neighborhood Boundary

Your neighborhood consists of devices that forward gossip traffic for you. They see encrypted envelopes but cannot decrypt them.

Neighbors observe that encrypted traffic passes through them. They see envelope sizes (which are fixed), rotating rtag identifiers, and timing patterns. Sophisticated adversaries might infer relationships through sustained observation. Neighbors cannot decrypt content, determine the ultimate sender or receiver, or link rtags to real identities.

Forwarding uses per-hop receipts. A relay only forwards after validating the upstream receipt and charging its own budget. Receipt and counter state is scoped to the same context, preventing budget leakage to unrelated observers.

### Group Boundary

Group participants see group-scoped identities, content, and threshold operations. Leakage accounting uses the group dimension, constrained by `limit(ctx).group` where applicable.

### External Boundary

External observers have no relationship with you and are not part of your gossip neighborhood. External observers should learn nothing.

With Tor integration, external observers see only encrypted Tor traffic. Without Tor, an ISP-level adversary observes only that you are connecting to known Aura nodes. Information-flow budgets reduce total emission opportunities, further limiting inference.

External observers cannot determine which accounts exist, what relationships exist, what content you access, or even that you are using Aura. Fixed-size envelopes prevent size-based fingerprinting. The lack of a central directory means there is no global metadata to leak.

## Privacy Layers

Aura's privacy model operates across five distinct layers.

### Identity Privacy

Traditional systems leak identity through persistent identifiers. Aura eliminates this through context-specific identities.

Deterministic Key Derivation derives context-specific keys using `derive(account_root, app_id, context_label)`. The derivation is deterministic but irreversible. Knowing the derived key for context A reveals nothing about context B. An adversary who observes you in multiple contexts cannot prove they are the same account without side-channel analysis.

The testing metric `identity_linkability_score` measures the probability that an observer can link identities across contexts. It should remain below 0.05 (5% confidence) for any pair of contexts.

### Relationship Privacy

Your social graph reveals personal relationships and organizational structures. Aura keeps the relationship graph opaque to external observers while allowing consensual visibility within relationships.

The existence of a relationship between two identities is only visible to the parties in the relationship and to parties both parties have explicitly disclosed the relationship to. Relationships are established through out-of-band key exchange or web-of-trust introductions. There is no central directory.

External observers cannot determine that Alice and Bob have a relationship. A neighborhood adversary who controls gossip hops might correlate timing patterns but cover traffic and batching degrade this attack.

The testing metric `relationship_inference_confidence` measures the probability that an observer can infer the existence of a relationship. It should remain below 0.10 for neighborhood adversaries and 0.01 for external observers.

### Group Privacy

Groups are micro-anonymity sets. Within a group, members are visible through consensual disclosure. Outside the group, membership is hidden.

Group membership is only visible within the group. External observers cannot enumerate members or confirm a group's existence. Group communications are indistinguishable from person-to-person communications at the envelope level.

Groups have their own Deterministic Key Derivation-derived identity independent of member identities. Members hold shares of the group's threshold key. For privacy-sensitive operations, groups can operate in anonymity mode where actions are attributed to the group rather than individuals.

The testing metric `group_membership_inference` measures the probability of inferring membership through traffic analysis. It should remain below the k-anonymity bound where P(specific member | observation) is at most 1/k where k is group size.

### Content Privacy

All content is end-to-end encrypted. Only parties with the appropriate key material can decrypt.

Message content, stored documents, and application data are encrypted such that only authorized parties can decrypt. Messages use authenticated encryption with AES-GCM with per-message keys derived from shared secrets. Stored content uses content-addressed encryption where the encryption key is derived from the content hash plus a secret salt. Session keys are ephemeral. Compromise of long-term identity keys does not compromise past session content.

Content-level attacks reduce to brute force cryptanalysis or key material compromise. Aura uses well-studied primitives like AES-256-GCM, Ed25519, and HPKE that provide 128-bit security.

### Metadata Privacy

Content privacy is useless if metadata leaks communication patterns. Metadata includes timing, frequency, size, and participation.

Aura employs multiple metadata privacy techniques. Per-context per-peer budgets bound observable rate and volume. All envelopes are padded to a standard size, preventing size-based fingerprinting. Outgoing messages are batched and sent at fixed intervals, obscuring precise timing. Routing tags rotate periodically, preventing longitudinal tracking. Envelopes are onion-routed through multiple hops, where each hop sees only the previous and next hop.

Metadata privacy degrades gracefully under increasingly powerful adversaries. A passive neighborhood observer might infer that someone is communicating frequently but cannot identify specific parties. An active adversary controlling multiple gossip hops has better inference capability but faces computational challenges at scale. A global passive adversary can perform traffic analysis but faces challenges distinguishing real traffic from cover traffic.

The testing metric `metadata_leakage` measures information leakage in bits. For timing, H(actual_send_time | observed_traffic) should exceed 4 bits. For participation, P(user active | observed_neighborhood_traffic) should be within 10% of base rate.

## Practical Leakage Tracking Examples

### Basic LeakageEffects Usage

```rust
use aura_core::effects::LeakageEffects;
use aura_core::{AuthorityId, ContextId, ObserverClass, LeakageEvent};

async fn record_message_send(
    leakage: &impl LeakageEffects,
    context: ContextId,
    source: AuthorityId,
    dest: AuthorityId,
) -> aura_core::Result<()> {
    // Record that neighbors can observe encrypted envelope metadata
    let event = LeakageEvent {
        source,
        destination: dest,
        context_id: context,
        leakage_amount: 1, // 1 message observed
        observer_class: ObserverClass::Neighbor,
        operation: "send_message".to_string(),
        timestamp_ms: current_timestamp_ms(),
    };

    leakage.record_leakage(event).await?;

    // Check if we're within budget before continuing
    let within_budget = leakage.check_leakage_budget(
        context,
        ObserverClass::Neighbor,
        10, // planning to send 10 more messages
    ).await?;

    if !within_budget {
        return Err(aura_core::AuraError::privacy_budget_exceeded(
            "Would exceed neighbor observation budget"
        ));
    }

    Ok(())
}
```

### LeakageTracker with Policy Settings

```rust
use aura_mpst::{LeakageTracker, LeakageBudget, LeakageType, UndefinedBudgetPolicy};
use aura_core::DeviceId;
use chrono::Utc;

// Secure default: deny undefined budgets
async fn create_secure_tracker() -> LeakageTracker {
    let tracker = LeakageTracker::new(); // Uses UndefinedBudgetPolicy::Deny

    // Any leakage without explicit budget will be denied
    // This is the recommended approach for production
    tracker
}

// Legacy permissive mode for backward compatibility
async fn create_permissive_tracker() -> LeakageTracker {
    let tracker = LeakageTracker::legacy_permissive();

    // Allows unlimited leakage when no budget is defined
    // Only use during migration period
    tracker
}

// Custom default budget for undefined observers
async fn create_default_budget_tracker() -> LeakageTracker {
    let tracker = LeakageTracker::with_undefined_policy(
        UndefinedBudgetPolicy::DefaultBudget(1000) // 1000 units default
    );

    // Falls back to 1000 unit budget when undefined
    tracker
}

// Typical production setup with explicit budgets
async fn create_production_tracker() -> LeakageTracker {
    let mut tracker = LeakageTracker::new();
    let now = Utc::now();

    // Define budgets for each observer class
    let neighbor_device = DeviceId::new();

    // Neighbor can observe 100 metadata events per hour
    let neighbor_budget = LeakageBudget::with_refresh(
        neighbor_device,
        LeakageType::Metadata,
        100,
        chrono::Duration::hours(1),
        now,
    );
    tracker.add_budget(neighbor_budget);

    // Neighbor can observe 50 timing events per hour
    let timing_budget = LeakageBudget::with_refresh(
        neighbor_device,
        LeakageType::Timing,
        50,
        chrono::Duration::hours(1),
        now,
    );
    tracker.add_budget(timing_budget);

    tracker
}
```

### Choreography Integration

```rust
use aura_mpst::{LeakageTracker, LeakageType};
use aura_core::DeviceId;
use chrono::Utc;

async fn choreography_with_leakage_tracking(
    tracker: &mut LeakageTracker,
    relay: DeviceId,
) -> aura_core::Result<()> {
    let now = Utc::now();

    // Before sending through relay, check budget
    if !tracker.check_leakage(&LeakageType::Metadata, 1, relay) {
        return Err(aura_core::AuraError::privacy_budget_exceeded(
            "Relay metadata budget exhausted"
        ));
    }

    // Record the leakage
    tracker.record_leakage(
        LeakageType::Metadata,
        1,
        relay,
        now,
        "Message forwarded through relay - envelope metadata visible"
    )?;

    // Send actual message (budget already charged)
    // ... transport operation ...

    Ok(())
}
```

### Real-World Example: Multi-Hop Forwarding

```rust
use aura_mpst::{LeakageTracker, LeakageType, LeakageBudget};
use aura_core::DeviceId;
use chrono::Utc;

struct RelayChain {
    tracker: LeakageTracker,
    relays: Vec<DeviceId>,
}

impl RelayChain {
    fn new(relays: Vec<DeviceId>) -> Self {
        let mut tracker = LeakageTracker::new();
        let now = Utc::now();

        // Each relay gets limited metadata observation budget
        for relay in &relays {
            let budget = LeakageBudget::with_refresh(
                *relay,
                LeakageType::Metadata,
                100, // 100 envelopes per hour per relay
                chrono::Duration::hours(1),
                now,
            );
            tracker.add_budget(budget);
        }

        Self { tracker, relays }
    }

    async fn forward_message(&mut self, message: Vec<u8>) -> aura_core::Result<()> {
        let now = Utc::now();

        // Check budget for entire relay chain before sending
        for relay in &self.relays {
            if !self.tracker.check_leakage(&LeakageType::Metadata, 1, *relay) {
                return Err(aura_core::AuraError::privacy_budget_exceeded(
                    format!("Relay {} metadata budget exhausted", relay)
                ));
            }
        }

        // Forward through each hop, recording leakage
        for relay in &self.relays {
            self.tracker.record_leakage(
                LeakageType::Metadata,
                1,
                *relay,
                now,
                format!("Envelope forwarded through relay {}", relay)
            )?;

            // Actual network send would happen here
            // send_to_relay(relay, &message).await?;
        }

        Ok(())
    }

    fn get_relay_status(&self) -> Vec<(DeviceId, Option<u64>)> {
        self.relays.iter().map(|relay| {
            let remaining = self.tracker.remaining_budget(*relay, &LeakageType::Metadata);
            (*relay, remaining)
        }).collect()
    }
}
```

### Integration with Effect System

```rust
use aura_core::effects::{LeakageEffects, LeakageChoreographyExt};
use aura_core::{AuthorityId, ContextId, ObserverClass};

async fn protocol_step_with_leakage(
    effects: &impl LeakageEffects,
    context: ContextId,
    source: AuthorityId,
    dest: AuthorityId,
    flow_cost: u64,
) -> aura_core::Result<()> {
    // Use extension trait for common patterns
    effects.record_send_leakage(
        source,
        dest,
        context,
        flow_cost,
        &[ObserverClass::Neighbor, ObserverClass::InGroup],
    ).await?;

    // Query leakage history for debugging
    let history = effects.get_leakage_history(context, None).await?;

    for event in history.iter().take(5) {
        tracing::debug!(
            "Leakage: {} units to {:?} observer for {}",
            event.leakage_amount,
            event.observer_class,
            event.operation
        );
    }

    Ok(())
}
```

## Implementation Metrics

Aura 1.0 ties privacy guarantees to the FlowBudget system described in [Flow Budget System](003_information_flow_contract.md#flow-budget-system-reference).

Per-context activity requires `spent(ctx)` not exceed `limit(ctx)` per epoch, enforced by FlowGuard charge before every transport call. Leakage per observer class requires `L(τ, class)` not exceed `Budget(class)`, enforced by [guard mechanisms](109_authorization.md). Timing dispersion requires high-sensitivity protocols add at least 2 seconds jitter via optional delay guard. Reservation fairness requires at most one outstanding FlowBudget reservation per context and authority.

## Flow Budget System Reference

The FlowBudget system enforces both spam resistance and privacy budgets by combining replicated `spent` counters with runtime-computed `limit` values:

- **Replicated facts**: Each successful charge emits a `FlowBudgetFact` containing `(ContextId, peer, epoch, spent_delta)`. These facts merge via max, so replicas converge even under partition.
- **Runtime limits**: Every authority recomputes `limit(ctx, peer)` from Biscuit tokens, sovereign policy, peer health, relay factors, and role metadata whenever a send/forward is attempted. Because all replicas apply the same meet-semilattice evaluation, they derive identical limits without storing them.
- **Epoch rotation**: Journals carry `Epoch(ctx)` facts. When the epoch increments, transport handlers discard pending receipts and reset `spent` counters for the new epoch.
- **Receipts**: Each send/forward that succeeds produces a Receipt fact scoped to `(context, epoch)` with chained hashes. Relays validate receipts before forwarding and record them as relational facts so downstream peers can audit budget usage.
- **Multi-hop enforcement**: Every hop independently executes CapGuard → FlowGuard → JournalCoupler. Headroom must exist at each hop; failures block locally and leak no metadata. Because `spent` is monotone, convergence holds even if later hops fail.
- **Charge-before-send invariant**: FlowGuard charges occur before any observable transport action. JournalCoupler atomically merges the charge fact and optional protocol deltas, ensuring the calculus guarantees from `docs_2/002_theoretical_model.md` hold in implementation.

Together these rules ensure that spam limits, leakage budgets, and receipt accountability share the same semilattice foundation described throughout this document.

## Threat Model by Adversary Position

Privacy properties vary based on adversary capabilities and position in the network. Aura considers five adversary types.

### Individual Relationship Partner

An individual with a direct relationship with you sees everything within that relationship context. This is consensual disclosure. They cannot see your activity in other contexts, link your identity across contexts, or access content you have not granted them.

The attack vector is social engineering or context correlation. Technical mechanisms cannot prevent social-layer correlation. User education and interface design mitigate this. The UI clearly indicates which context you operate in.

### Group Insider

A group member sees all group activity including member identities, group content, and participation patterns. They cannot determine members' identities outside the group, access other groups, or see person-to-person relationships not disclosed within the group.

Groups enforce k-anonymity for sensitive operations. When anonymity is required at least k members participate in threshold signing and individual signers are not revealed. Signing rounds include random delays to prevent timing correlation.

### Gossip Neighborhood Participant

Devices in your gossip neighborhood forward encrypted envelopes and observe metadata like size (which is fixed), rotating rtags, timing, and frequency. With sustained observation, they might infer traffic patterns. They cannot decrypt content, determine ultimate sender or receiver due to onion routing, link rtags to real identities, or distinguish real traffic from cover traffic without statistical analysis.

The primary attack is traffic correlation. Onion routing prevents single-hop correlation. Cover traffic obscures activity patterns. Batching and random delays break timing correlation.

### Network-Level Observer

An adversary who can observe network traffic like an ISP sees IP-level connections and packet timing. Without Tor, they can observe that you are connecting to known Aura nodes. With Tor, they see only that you are using Tor.

Without Tor, confirmation attacks are possible. With Tor, correlation requires breaking Tor's anonymity properties, which is a hard problem. Even with Tor compromised, fixed-size envelopes and cover traffic prevent precise correlation.

### Compromised Device

If a single device in a multi-device account is compromised, an adversary gains access to that device's key share and the account's journal state as synced to that device. A single compromised device cannot perform threshold operations (which requires M of N devices), derive the account's root key, or access content not synced to that device.

If the adversary compromises M of N devices, they gain full account control. Threshold cryptography limits single-device compromise impact. Users can revoke compromised devices through resharing, which requires M devices. Session epochs invalidate old credentials when compromise is detected.

## Hub Node Problem and Mitigation

Hub nodes with high connectivity forward traffic for many peers. While they cannot decrypt content, they observe envelope metadata for many relationships and might infer communication patterns.

Aura employs several mitigation techniques. Routes are selected to minimize the fraction of a path observed by any single node. The system tracks node connectivity and identifies hubs. Users can choose to avoid routing through hubs if privacy is a higher priority than efficiency. The routing algorithm accepts a privacy parameter. Higher privacy settings avoid hubs even at the cost of longer routes.

To prevent hubs from distinguishing real traffic from cover traffic, the protocol supports optional decoy envelopes. Fixed-size envelopes prevent size correlation. Rotating rtags limit tracking duration. Per-hop budgets bound the rate at which any hub can forward traffic for a given context.

## Cover Traffic Strategy

Cover traffic strengthens metadata privacy but is expensive if done naively, so Aura 1.0 treats it as an optional enhancement layered on top of the mandatory flow-budget enforcement. When enabled, cover traffic adapts to real usage patterns. If you typically send 20 messages per hour during work hours, the system maintains approximately that rate during idle periods. Rather than individual cover messages, the system leverages groups for efficient cover traffic. Groups naturally have steady traffic rates due to multiple members, obscuring individual patterns.

Cover traffic sends at scheduled intervals. Real messages are inserted into scheduled slots, making them indistinguishable from cover traffic. Cover traffic envelopes are cryptographically indistinguishable from real envelopes. Only the recipient can determine that an envelope is cover traffic by attempting to decrypt. Effective deployments aim to keep the probability of distinguishing real from dummy traffic close to 0.5.

## Integration with RFC 130 Testing Framework

Each privacy or flow property translates to a test invariant. For example, identity unlinkability across contexts becomes `assert!(observer.identity_linkability_score(context_a, context_b) < 0.05)`.

Each adversary type is implemented as a test observer with specific capabilities. Tests instantiate observers with varying capabilities and measure what they can infer. Privacy properties are probabilistic. Tests measure inference confidence and verify it remains below specified bounds.

Tests run in the simulation framework with deterministic time and network conditions. The simulation maintains a test-only ground truth oracle strictly isolated from adversary observers. Privacy tests are part of CI. Any change that degrades privacy properties beyond thresholds fails the test.

## Implementation Guidance

Key derivation must be cryptographically sound. Use HKDF with domain separation. Never reuse keys across contexts. The derivation path should include account root key, app_id, context_label, and a domain separator like `aura.key.derive.v1`.

Envelopes must be fixed-size, encrypted, authenticated, and onion-routed. Padding must be non-distinguishable using random bytes. Rtags must rotate on a schedule negotiated in-band. Test that envelopes are indistinguishable at the byte level.

All transport calls must pass through `FlowGuard` that charges the per-context per-peer budget before any network side effect. When charging fails, the choreography must branch locally with no packet emitted. For multi-hop forwarding, attach and validate per-hop receipts.

The CRDT journal contains sensitive metadata about account structure. Journal events should be encrypted when synced across untrusted paths. Within a relationship or group, journal events can be plaintext through consensual disclosure.

Capabilities grant access to resources. Ensure they do not leak information to unauthorized parties. A capability should be opaque. All network traffic should route through Tor by default with clear UI indication when Tor is disabled.

Use platform-provided secure storage for key shares like Keychain on macOS or iOS, Secret Service on Linux, or Keystore on Android. Never store keys in plaintext files or standard databases.

For security-critical operations like key derivation and threshold signing, maintain audit logs in the journal visible to all account devices. This enables detection of compromised devices.

## Conclusion

Aura's model is consent-based, context-specific, and relational expressed as a unified information-flow system. Privacy boundaries align with social relationships. Information shared within consensual relationships is not a leak. Protections focus on preventing information from crossing boundaries without consent and on bounding what observers can learn through rate and volume controls.

The model recognizes that perfect privacy is impossible in a functional system. Instead, it provides tunable privacy properties that degrade gracefully under increasingly powerful adversaries. Measurement and testing ensure that privacy properties hold in practice.

Implementation requires careful attention to cryptographic details, metadata handling, and adversary modeling. For 1.0, the information-flow budget is the primary enforcement surface. Optional mechanisms like Tor, onion routing, and cover traffic strengthen privacy further.

## See Also

- [Theoretical Model](002_theoretical_model.md)
- [System Architecture](001_system_architecture.md)
- [Authority and Identity](100_authority_and_identity.md)
- [Relational Contexts](103_relational_contexts.md)
- [Authorization](109_authorization.md)
- [Identifiers and Boundaries](105_identifiers_and_boundaries.md)
- [Transport and Information Flow](108_transport_and_information_flow.md) - Privacy-by-design patterns
- [Privacy Checklist](privacy_checklist.md) - Development checklist for privacy-preserving code

## Implementation References

- **Guard Chain Implementation**: `crates/aura-protocol/src/guards/` (see [Authorization](109_authorization.md))
- **Authority Privacy**: `crates/aura-effects/src/authority/`
- **Context Isolation**: `crates/aura-relational/src/privacy/`
- **Flow Budget System**: `crates/aura-protocol/src/flow_budget/`
- **Privacy Testing**: `crates/aura-testkit/src/privacy/`
- **Transport Privacy Patterns**: `crates/aura-transport/` (reference implementation)
