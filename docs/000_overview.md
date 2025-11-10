# 000 · Motivation & Goals

## Documentation Index

This document provides an overview of Aura's architecture and goals. Additional documentation covers specific aspects of the system:

### Core Architecture
- **[001_theoretical_foundations.md](001_theoretical_foundations.md)** - Mathematical foundation including formal calculus, algebraic types, and semilattice semantics. Presents the complete theoretical model underlying all system components.
- **[002_system_architecture.md](002_system_architecture.md)** - Implementation architecture including effect system patterns, CRDT implementations, and choreographic protocols. Covers practical system design and crate organization.
- **[003_distributed_applications.md](003_distributed_applications.md)** - Concrete examples showing theory and architecture in practice. Demonstrates distributed systems, CRDT usage, testing strategies, and integration patterns.
- **[004_info_flow_model.md](004_info_flow_model.md)** - Consent-based privacy framework with trust boundaries aligned to social relationships. Defines privacy violations and information flow controls.

### Domain Specifications
- **[101_auth_authz.md](101_auth_authz.md)** - Authentication and authorization architecture separating WHO and WHAT concerns. Provides clean integration patterns between identity verification and capability evaluation.
- **[102_dist_maintenance.md](102_dist_maintenance.md)** - Distributed maintenance stack for day-one reliability including snapshots, garbage collection, and over-the-air updates. Defines operational procedures for system maintenance.
- **[103_info_flow_budget.md](103_info_flow_budget.md)** - Unified information flow budget system enforcing privacy limits and spam prevention. Uses semilattice primitives for monotone and deterministic communication controls.
- **[104_rendezvous.md](104_rendezvous.md)** - Peer discovery and connection setup specification for Aura 1.0. Provides dependable peer discovery while reusing core system primitives.
- **[105_journal.md](105_journal.md)** - Journal and ledger implementation details. Covers CRDT-based state management and conflict resolution.

### Implementation Details
- **[123_ratchet_tree.md](123_ratchet_tree.md)** - Ratchet tree structure for threshold identity management. Defines cryptographic commitments and tree operations for secure account management.
- **[125_identifiers.md](125_identifiers.md)** - Identifier system and context isolation. Specifies how different types of identifiers maintain privacy boundaries.
- **[600_simulation_framework.md](600_simulation_framework.md)** - Deterministic simulation framework with Quint formal verification integration. Provides controlled execution environments for testing distributed protocols.
- **[601_testing_debugging.md](601_testing_debugging.md)** - Testing strategies and debugging tools for distributed systems. Covers property-based testing and Byzantine fault scenarios.

### Developer Resources
- **[800_building_on_aura.md](800_building_on_aura.md)** - Complete developer guide for building applications on Aura. Covers setup, basic usage, and advanced distributed systems implementation.
- **[099_glossary.md](099_glossary.md)** - Canonical definitions for architectural concepts ensuring consistent terminology. Reference for all core system concepts and terms.
- **[999_crate_wiring.md](999_crate_wiring.md)** - Comprehensive crate structure overview with dependency graph and API reference. Details project organization and exposed interfaces.

## Project Goals

Aura aims to demonstrate that web-of-trust architectures can be practical, delightful, and aligned with user interests rather than platform interests.

### Social Architecture as Design Space

Secure Scuttlebutt represents the high-water mark for web-of-trust systems, but there remains a large, untapped design space. Interesting emergent behaviors arise from bottom-up social architecture rather than top-down platform control. Aura explores this space by making the network itself the platform, aiming to create a system where users feel aligned with both their social network and the infrastructure supporting it.

### Real Usage, Real Stakes

The target: 20 close friends using the system twice weekly. This constraint forces us to deliver something unique and valuable, not just technically interesting. Users must feel confident that:
- Their privacy expectations won't be violated
- Their data is durable and won't disappear
- The system is secure enough to trust with real relationships and real information

### Network as Platform

Very few digital systems align platform interests with social network interests. Platforms are a product of their environment and how they operate as a business impacts how they operate as infrastructure. Aura directly couples the network with the infrastructure, making peers host both the digital and physical infrastructure. When your friends are the platform, incentives align naturally.

### Start Small, Build for Scale

The project starts with friends—small, focused, and fun. But architectural choices must support eventual scale. Multi-platform support and over-the-air upgrades aren't "nice to have" features; they're foundational. Without them from day one, we'd need to reboot the entire system from scratch, fracturing the social network we built.

## Project Constraints

These constraints shape every architectural decision in Aura.

### Consent-Based Design

- **Selective disclosure**: Information flows through your social graph based on explicit choices, not platform defaults
- **Cooperative utility**: Peer discovery and storage are performed by your social network, not centralized services
- **Transparent trust trade-offs**: We take inspiration from the principle of least authority, but break it intentionally when one kind of trust naturally implies another (e.g., guardians for recovery likely implies trust for relay). When the principle is broken, the choice is deliberate and security implications are weighed carefully.
- **Trust-weighted flow budgets**: Every relationship carries a small information-flow budget that governs both privacy leakage and spam rate. Budgets are derived from the same web-of-trust semilattice as capabilities, so limiting abuse is the same operation as protecting metadata.

### Fully Peer-to-Peer

- No servers, not even for peer discovery or bootstrapping
- All coordination happens through the social graph
- Network must be self-sustaining from initial invitation onward

### Cross-Platform from Day One

The system must run on:
- **Web**: Chrome, Firefox, Safari (via WebAssembly)
- **Mobile**: iOS, Android
- **Desktop**: macOS, Linux

This isn't a "later phase" concern—it's a foundational requirement. Multi-platform support enables the 20-friend network to actually form and persist.

### Performance for Daily Use

- **Fast enough for daily driver status**: Latency must not create friction in common operations
- **Offline-first UX advantage**: High-latency scenarios become UX features, not bugs. The app works in airplane mode, syncs seamlessly when connectivity returns, and never blocks on network operations.

### Real Security for Real Secrets

Users must be able to store secrets without external backups (aside from snapshots). This means:
- Threshold cryptography prevents single-device compromise
- Guardian recovery enables key recovery without escrow
- Forward secrecy limits damage from historical compromises

### Upgrade Safety

- **Low catastrophe risk**: No existential dread that an upgrade will partition the network
- **Snapshot recovery**: Most failure scenarios are recoverable through snapshots
- **Ephemeral data tolerance**: System can start with ephemeral data so the web of trust is the primary durable artifact
- **Graceful degradation**: Older clients can interact with newer ones within semantic version compatibility bounds

## Why Aura?

The constraints above—fully P2P, cross-platform, offline-first, upgrade-safe, consent-based—collectively rule out most existing approaches:

- **Centralized services** violate P2P and alignment constraints (platform interests diverge from user interests)
- **Blockchain consensus** violates performance constraints (high latency, can't work offline)
- **Eventually-consistent systems** without strong semantics fail unpredictably during partitions
- **Ad-hoc P2P protocols** can't safely evolve (protocol changes fragment the network)

Meeting all constraints simultaneously requires a principled foundation. Aura combines:
- **Threshold cryptography** for security without single points of failure
- **Choreographic protocols** for deadlock-free coordination across platforms
- **Semilattice CRDTs** for offline operation with automatic convergence
- **Session types** for safe protocol evolution and cross-version compatibility
- **Deterministic simulation** for testing Byzantine faults before deployment

This isn't overengineering, it's the minimal architecture that satisfies the constraints while enabling emergent social behaviors.

### From Constraints to Capabilities

The project constraints directly motivate each architectural choice:

| Constraint                          | Required Capability                      | Aura's Solution                          |
|-------------------------------------|------------------------------------------|------------------------------------------|
| Fully P2P (no servers)              | Self-sustaining peer discovery           | Social Bulletin Board via web-of-trust   |
| Cross-platform (6 targets)          | Platform-agnostic protocol layer         | WebAssembly + typed messages             |
| Performance (daily driver)          | Offline-first, non-blocking ops          | CRDTs + local-first architecture         |
| Security (real secrets)             | No single point of compromise            | Threshold cryptography (M-of-N)          |
| Upgrade safety (low risk)           | Protocol versioning + compatibility      | Semantic versioning + typed messages     |
| Consent-based design                | Selective disclosure via social graph    | Content-addressed invites + relationship keys |
| Start small, scale later            | Protocols tested before deployment       | Deterministic simulation engine          |
| Network is platform                 | Peers host infrastructure                | P2P choreographic coordination           |

### Threshold Identity as Validation

Threshold identity validates that these capabilities work together for a real problem. Current identity systems force impossible choices: trust a single device (which can be lost or compromised) or trust a corporation (which can lock you out).

Aura's approach uses social trust—your devices, friends, and chosen guardians—with M-of-N cryptographic guarantees:
- Multi-device support without single points of failure (threshold signatures enable M-of-N participation across your laptop, phone, tablet, etc.)
- No central gatekeeper (fully P2P)
- Recovery through real relationships (guardian system)
- Works offline (CRDT-based account state)
- Evolves safely (versioned protocol messages)

If 20 close friends use this system twice weekly for identity and recovery, the same foundations enable collaborative editing, secure messaging, decentralized storage, multi-party computation, or any other application requiring distributed coordination through a social graph.

## Architecture: Whole System + Application Layers

Aura is divided into two architectural layers, and three implementation layers:

### Layer Map (Canonical)

- Data Layer (Semilattices + Journal)
  - What: CRDT facts (⊔) and capabilities (⊓), journal state, GC/snapshots
  - Where: `docs/001_theoretical_foundations.md` (§2, §4), `docs/002_system_architecture.md` (§2), `docs/105_journal.md`
  - Invariants: join‑only commits, meet‑only constraints, convergence

- Process Layer (Sessions + Effects)
  - What: MPST choreographies, projection, guard chain, effect execution
  - Where: `docs/001_theoretical_foundations.md` (§3, §2.4), `docs/002_system_architecture.md` (§1, §4), `docs/003_distributed_applications.md` (§3)
  - Invariants: CapGuard → FlowGuard → JournalCoupler; charge‑before‑send

- Edge Layer (Transport + SecureChannel)
  - What: Rendezvous, forwarding, per‑hop receipts, epoch rotation
  - Where: `docs/104_rendezvous.md`, `docs/004_info_flow_model.md` (receipts/epochs), `docs/002_system_architecture.md` (§1.5)
  - Invariants: one channel per (ContextId, peer), receipts don’t cross epochs

### Whole System Model (Foundation)

The **whole system model** (`docs/001_theoretical_foundations.md`) defines the formal mathematical foundation. This layer provides:

**A. Core Algebraic Types**:
- `Cap` (meet-semilattice ⊓): Authority/capability refinement
- `Fact` (join-semilattice ⊔): Knowledge accumulation
- `Journal { facts: Fact, caps: Cap }`: Unified CRDT state
- Context types (`RID`, `GID`, `DKD`): Privacy partitions

**B. Effect Interfaces** (foundational surfaces with extended implementations):
- Core traits (`TimeEffects`, `CryptoEffects`, `StorageEffects`, `NetworkEffects`, `JournalEffects`, `ConsoleEffects`, `RandomEffects`): Foundational effect capabilities
- Extended traits (`SystemEffects`, `LedgerEffects`, `ChoreographicEffects`, `TreeEffects`, `AgentEffects`): Higher-level compositions
- Runtime system: `AuraEffectSystem` provides unified access to all effect categories

**See**: [`docs/099_glossary.md`](docs/099_glossary.md) for complete terminology and [`docs/002_system_architecture.md`](docs/002_system_architecture.md) for implementation details.

**C. Multi-Party Session Types (MPST)**:
- Global protocol specifications with capability guards
- Automatic projection to local roles
- Deadlock freedom by construction
- Extensions: journal-coupling, leakage budgets



**D. Privacy & Security Invariants**:
- Context isolation: no cross-context message flow
- Unlinkability: observer cannot distinguish contexts
- Capability soundness: `need(effect) ≤ caps` enforced
- CRDT convergence: monotone merge guarantees

This layer is **implementation-agnostic**: it specifies what must exist and what laws must hold, not how to build it.

### Application Layer (User-Facing Features)

The **application layer** builds on the whole system model to deliver user-facing functionality:

**Identity & Recovery**:
- Threshold account management (ratchet tree)
- Guardian-based recovery ceremonies
- Device addition/removal protocols
- Policy enforcement

**Storage & Sync**:
- Encrypted chunk storage with capability-based access
- CRDT synchronization across devices
- Garbage collection and compaction
- Search and indexing

**Social Coordination**:
- Invitation system (content-addressed links)
- Peer discovery via SBB
- Relationship management
- Group formation

**User Interfaces**:
- CLI tools for operators and testing

**Developer Tools**:
- Deterministic simulator
- Formal verification (Quint integration)
- Property-based testing

The application layer **implements protocols as choreographies** to deliver features. All protocols in 1.0 are reference implementations demonstrating the whole system model in practice. For example:
- **Guardian recovery** = `G_recovery` choreography using threshold signatures and journal merge
- **Peer discovery** = `G_rendezvous` choreography using relationship keys and SBB flooding
- **Tree operations** = `G_tree_op` choreography coordinating threshold updates
- **Synchronization** = `G_sync` choreography using CRDT merge semantics
- **Search** = `G_search` choreography with capability filtering
- **Garbage collection** = `G_gc` choreography with threshold-signed snapshots

This division ensures:
1. **Formal correctness** via the whole system model (laws and types)
2. **Concrete validation** via choreographic protocol implementations
3. **Feature completeness** via composition of verified protocols
4. **Clean separation** between "what laws hold" and "how protocols implement them"

## Current Implementation Status

**Legend:**
- ✅ **COMPLETE**: Fully implemented and tested
- ⚠️ **IN PROGRESS**: Partial implementation exists
- ❌ **NOT STARTED**: Not yet implemented
- 🗑️ **REMOVED**: Intentionally deleted or deprecated

### Workspace Structure

**Active Crates** (22 total):
```
crates/
├── aura-agent           ✅ Main agent entry point
├── aura-authenticate    ⚠️  Auth system
├── aura-cli             ✅ Command-line interface
├── aura-core            ✅ Foundation types (ID system, effects, semilattice)
├── aura-crypto          ✅ Crypto primitives (FROST, HPKE, key derivation)
├── aura-frost           ⚠️  FROST threshold signatures
├── aura-identity        ⚠️  Identity management
├── aura-invitation      ⚠️  Invitation system
├── aura-journal         ✅ CRDT-based journal/ledger
├── aura-mpst            ⚠️  Multi-party session types
├── aura-protocol        ✅ Effect system and coordination layer
├── aura-quint-api       ⚠️  Quint formal verification integration
├── aura-recovery        ⚠️  Recovery ceremonies
├── aura-rendezvous      ✅ Social Bulletin Board peer discovery
├── aura-simulator       ✅ Deterministic simulation engine
├── aura-storage         ⚠️  High-level storage orchestration
├── aura-store           ✅ Low-level encrypted chunk storage
├── aura-sync            ⚠️  CRDT synchronization protocols
├── aura-testkit         ✅ Shared testing utilities
├── aura-transport       ✅ P2P transport layer
├── aura-verify          ⚠️  Identity verification
└── aura-wot             ⚠️  Web-of-trust and capability system
```

## 1.0 Feature Set

We consider 1.0 "shipped" when the **whole system model is fully implemented** and the **application layer delivers threshold identity with social recovery**.

### Whole System Model (Foundation) - Required for 1.0

1. **Core Algebraic Types** ✅ **COMPLETE**
   - ✅ `Cap` (meet-semilattice) with refinement operations - `aura-core/src/journal.rs`
   - ✅ `Fact` (join-semilattice) with merge operations - `aura-core/src/journal.rs`
   - ✅ `Journal { facts, caps }` as unified CRDT state - `aura-core/src/journal.rs`
   - ✅ Context types (`RelayId`, `GroupId`, `DkdContextId`) for privacy partitions - `aura-core/src/identifiers.rs`
   - ✅ Typed message envelopes `Msg<Ctx, Payload, Version>` - `aura-core/src/messages.rs`
   - ✅ `MessageContext` enum enforcing context isolation - `aura-core/src/identifiers.rs`

2. **Effect System** ✅ **COMPLETE**
   - ✅ Pure effect interfaces: `JournalEffects`, `CryptoEffects`, `TransportEffects`, `TimeEffects`, `RandomEffects`, `ConsoleEffects` - `aura-core/src/effects.rs`
   - ✅ Effect handlers for production, testing, and simulation - `aura-protocol/src/handlers/*`
   - ✅ Composable handler architecture - `aura-protocol/src/handlers/composite.rs`, `registry.rs`, `factory.rs`
   - ✅ Deterministic simulation via injectable effects - `aura-protocol/src/handlers/time/simulated.rs`
   - ✅ Type-erased handlers for dynamic dispatch - `aura-protocol/src/handlers/erased.rs`

3. **Multi-Party Session Types (MPST)** ✅ **COMPLETE**
   - ✅ Choreographic DSL (`rumpsteak-aura`) with global protocol definitions and automatic projection
   - ✅ Effect-sequencing extensions for capability guards, journal coupling, and leakage budgets documented in `docs/001_theoretical_foundations.md`
   - ✅ Guard enforcement runtime (`crates/aura-mpst/src/guards/`) ensures `need(m) ≤ caps(ctx)` before every send
   - ✅ Journal coupling handlers (`crates/aura-mpst/src/journal_coupling.rs`) merge CRDT deltas atomically with message emission
   - ✅ Leakage budget tracking (`crates/aura-mpst/src/leakage.rs` + `crates/aura-protocol/src/guards/privacy.rs`) enforces `(ℓ_ext, ℓ_ngh, ℓ_grp)` annotations during choreography execution
   - ✅ Rumpsteak bridge + handler integration (`crates/aura-mpst/src/runtime.rs`, `crates/aura-protocol/src/handlers/rumpsteak_handler.rs`) execute choreographies against `AuraEffectSystem`

4. **Semilattice CRDT Implementation** ✅ **COMPLETE**
   - ✅ Join/meet semilattice traits with property tests - `aura-core/src/semilattice/mod.rs`
   - ✅ CRDT handlers: `CvHandler`, `DeltaHandler`, `CmHandler`, `MvHandler` - `aura-protocol/src/effects/semilattice/*`
   - ✅ Convergence guarantees: commutativity, associativity, idempotence - verified in tests
   - ✅ Concrete CRDT types: `CvState`, `DeltaState`, `CmState`, `MvState` - `aura-core/src/semilattice/mod.rs`
   - ✅ Journal-specific CRDTs - `aura-journal/src/semilattice/*`
   - ✅ Anti-entropy protocols for synchronization - implemented in `aura-sync/anti_entropy.rs`

5. **Privacy & Security Contracts** ⚠️ **IN PROGRESS**
   - ✅ FlowBudget CRDT + FlowGuard runtime (`docs/103_info_flow_budget.md`, `aura-protocol/src/guards/privacy.rs`)
   - ✅ Leakage guard integration in choreographies (`aura-mpst/src/leakage.rs`)
   - ✅ Quantitative privacy metrics (context budgets, leakage limits) documented in `docs/004_info_flow_model.md`
   - ❌ Observer simulation + unlinkability tests `τ[κ₁↔κ₂] ≈_ext τ` (framework not started)
   - ❌ Capability soundness verification harness (guard instrumentation + property tests) not started
   - ❌ Cover traffic/adaptive flow cost – explicitly out of scope for 1.0
   - **Status**: FlowBudget enforcement in place; validation tooling still needed

### Application Layer - Required for 1.0

All application protocols are choreography-based reference implementations demonstrating the whole system model.

6. **Threshold Identity Core** ⚠️ **IN PROGRESS**
   - ✅ Account identity is threshold-based: M-of-N participation required - `aura-identity/`
   - ✅ Ratchet tree structure with cryptographic commitments - `aura-journal/src/ratchet_tree/*`
   - ✅ FROST threshold signatures for identity operations - `aura-frost/`, `aura-crypto/src/frost/`
   - ✅ Deterministic key derivation (DKD) for app-specific keys - `aura-crypto/src/key_derivation.rs`
   - ✅ Session epoch system for forward secrecy - `aura-core/src/session_epochs.rs` and choreography integration in `aura-identity/`

7. **Ratchet Tree & Journal** ✅ **COMPLETE**
   - ✅ Tree operations (`TreeOp`) stored in CRDT ledger - `aura-core/src/tree.rs`
   - ✅ Leaf nodes for devices and guardians - `aura-journal/src/ratchet_tree/state.rs`
   - ✅ Branch nodes for threshold policies - `aura-journal/src/ratchet_tree/state.rs`
   - ✅ Threshold-signed attestations for all mutations - `aura-core/src/tree.rs::AttestedOp`
   - ✅ Intent pool (OR-set) for high availability - `aura-journal/src/ledger/intent.rs`
   - ✅ Tree commitment and verification - `aura-journal/src/ratchet_tree/reduction.rs`
   - ✅ Compaction support - `aura-journal/src/ratchet_tree/compaction.rs`

8. **Guardian-Based Recovery** ⚠️ **IN PROGRESS**
   - ✅ Guardian invitation infrastructure - `crates/aura-invitation/src/guardian_invitation.rs`
   - ✅ Recovery choreography framework and types - `crates/aura-recovery/src/choreography_impl.rs`, `guardian_recovery.rs`, `key_recovery.rs`
   - ✅ Account recovery protocols - `crates/aura-recovery/src/account_recovery.rs`
   - ✅ Emergency recovery operations - `crates/aura-recovery/src/emergency_recovery.rs`
   - ✅ Guardian approval path wired through `RecoveryOperations` + CLI cooldown telemetry
   - ✅ Dispute windows with guardian-filed disputes surfaced via CLI/agent status
   - **Status**: Ceremony now supports cooldown + dispute phases; remaining work focuses on richer escalation tooling and ledger persistence

9. **Invitation System** ⚠️ **IN PROGRESS**
    - ✅ Core invitation types and data structures - `crates/aura-invitation/src/device_invitation.rs`, `guardian_invitation.rs`
    - ✅ Invitation acceptance choreography framework - `crates/aura-invitation/src/invitation_acceptance.rs`
    - ✅ Relationship formation protocols - `crates/aura-invitation/src/relationship_formation.rs`
    - ✅ Content-addressed invitation envelopes with TTL, hashes, and FlowGuard wiring
    - ✅ Rendezvous/SBB publishing hook for envelopes + acceptance acks (awaiting transport backend)
    - ⚠️ Bidirectional relationship key establishment ceremony pending
    - **Status**: Invitation lifecycle + rendezvous wiring implemented; key exchange + ledger integration remain

10. **Peer Discovery (Rendezvous / SBB)** ✅ **COMPLETE**
    - ✅ Rendezvous 1.0 spec (FlowBudget + SecureChannel lifecycle) - `docs/104_rendezvous.md`
    - ✅ Core crate structure with module organization - `crates/aura-rendezvous/src/` with sbb.rs, discovery.rs, messaging.rs, relay.rs
    - ✅ SBB flooding protocol with TTL tracking and duplicate detection - `crates/aura-rendezvous/src/sbb.rs`
    - ✅ Content-addressed envelope system with Blake3 hashing - `crates/aura-rendezvous/src/sbb.rs`
    - ✅ Relationship key derivation extending DKD system for bidirectional Alice↔Bob keys - `crates/aura-rendezvous/src/relationship_keys.rs`
    - ✅ HPKE-style envelope encryption with traffic analysis resistant padding - `crates/aura-rendezvous/src/envelope_encryption.rs`
    - ✅ Capability-aware flooding with Web-of-Trust integration and flow budget enforcement - `crates/aura-rendezvous/src/capability_aware_sbb.rs`
    - ✅ Transport layer integration with NetworkTransport bridge - `crates/aura-rendezvous/src/messaging.rs`
    - ✅ Complete integrated SBB system with builder pattern API - `crates/aura-rendezvous/src/integrated_sbb.rs`
    - ✅ Comprehensive end-to-end integration tests for Alice→Bob connection scenarios - `crates/aura-rendezvous/src/integration_tests.rs`
    - **Status**: Complete SBB system implementation with privacy-preserving peer discovery, relationship-based encryption, and capability enforcement

11. **Transport Implementation & SecureChannel** ✅ **COMPLETE**
    - ✅ In-memory transport for testing - `aura-transport/src/memory.rs`
    - ✅ SecureChannel registration + reuse (`aura-transport/src/network.rs`, `docs/104_rendezvous.md`)
    - ✅ Message authentication and encryption integrated with effect system
    - ✅ QUIC transport implementation (UDP-based, multiplexed streams)
    - ✅ WebSocket transport for browser compatibility - `aura-transport/src/websocket.rs` (269 lines)
    - ✅ STUN client integration for NAT reflexive address discovery - `aura-transport/src/stun.rs` (298 lines)
    - ✅ Coordinated hole-punching for QUIC - `aura-transport/src/hole_punch.rs` (459 lines)
    - ✅ Connection priority logic (direct → STUN reflexive → hole-punch → relay) - `aura-rendezvous/src/connection_manager.rs` (815 lines)
    - ✅ Contact-mediated relay protocol with flow budget enforcement - `aura-rendezvous/src/relay.rs` (357 lines)
    - ✅ Relay selection heuristics (guardian preference) - `aura-rendezvous/src/relay_selection.rs` (384 lines)
    - ✅ Comprehensive integration tests - `tests/transport_integration.rs` (814 lines), `tests/nat_scenarios.rs` (578 lines), `tests/flow_budget_enforcement.rs` (423 lines)
    - ✅ Complete documentation and examples - `docs/transport_selection_guide.md` (421 lines), `examples/transport_usage.rs` (573 lines)
    - ❌ WebRTC deferred to post-1.0 (requires full ICE, adds complexity)
    - ❌ Raw TCP deferred to post-1.0 (QUIC is strictly better, WebSocket covers firewall-friendly fallback)
    - **Status**: Complete transport layer with universal connectivity, NAT traversal, and relay fallback for "20 friends, twice weekly" validation scenario

12. **Maintenance (Snapshots / GC / OTA)** ⚠️ **IN PROGRESS**
    - ✅ Day-one maintenance plan (manual snapshots + GC, cache invalidation, OTA soft/hard forks) captured in `docs/102_dist_maintenance.md`
    - ✅ Threshold-signed snapshot proposal types - `aura-core/src/tree.rs::Snapshot`
    - ✅ Snapshot proposal/completion CLI wiring + writer fencing (`aura-agent::MaintenanceController`, `aura-cli snapshot propose`)
    - ✅ Admin override stub (journal fact, CLI `aura admin replace`, local enforcement helpers)
    - ⚠️ Cache invalidation events + local enforcement hooks in apps
    - ⚠️ OTA upgrade orchestration (Auto vs Manual opt-in, identity epoch fences) needs implementation + testing
    - **Status**: Snapshot + GC flow wired through CLI/daemon; cache invalidation + OTA still pending

13. **Deterministic Simulator** ✅ **COMPLETE**
    - ✅ Injectable effects for reproducible testing - `aura-simulator/src/effects/system.rs`
    - ✅ Seeded PRNG for deterministic randomness - effect handlers support seeding
    - ✅ Controllable simulated time - `aura-protocol/src/handlers/time/simulated.rs`
    - ✅ Fault injection for Byzantine behavior - middleware in `aura-simulator/`
    - ✅ Property-based testing harness - `aura-testkit/` + proptest integration
    - ✅ Chaos testing framework - `aura-simulator/src/effects/middleware/chaos_coordination.rs`

### Implementation Status Summary

**Foundation (Whole System Model):**
- Core types: ✅ 100% complete
- Effect system: ✅ 100% complete
- MPST infrastructure: ✅ 100% complete (guards, journal coupling, leakage budgets, runtime bridge)
- Semilattice CRDTs: ✅ 95% complete (anti-entropy needs work)
- Privacy contracts: ⚠️ ~50% complete (FlowBudget + FlowGuard enforcement wired; observer simulation + capability soundness tests not started)

**Application Layer:**
- Threshold identity core: ✅ 100% complete (FlowBudget allocator + guardian trust transitions spec'd, recovery flow still partial)
- Ratchet tree & journal: ✅ 100% complete
- Guardian recovery: ⚠️ 40% complete (infrastructure and choreography framework in place; ceremony logic and cooldowns pending)
- Invitation system: ⚠️ 35% complete (choreography framework and types in place; content-addressing, key exchange, and full acceptance flow pending)
- Peer discovery (SBB): ✅ 100% complete (complete SBB flooding, relationship encryption, capability-aware routing, and end-to-end testing)
- Transport: ✅ 100% complete (QUIC + in-memory + WebSocket + STUN + hole-punching + relay complete)
- Maintenance (snapshots / GC / OTA): ⚠️ 60% complete (snapshot workflow + admin override stub done; cache invalidation + OTA pending)
- Simulator: ✅ 100% complete

**Overall 1.0 Progress: ~70% complete**

### Critical Path to 1.0

**Immediate priorities** (blocking 1.0):

1. **Finalize Maintenance Pipeline** (~3 weeks)
   - ✅ Implement `Snapshot_v1` choreography + writer fencing in CLI/daemon (`aura-agent` maintenance controller, `aura-cli snapshot propose`)
   - ✅ Admin override command + stub enforcement
   - Emit `CacheInvalidated` events and enforce local epoch floors
   - Ship OTA upgrade orchestration (soft/hard forks, opt-in policies, CLI tooling)

2. **Finish Privacy Contracts** (~3 weeks)
   - ✅ FlowGuard enforcement wired into transport layer (default hints + overrides)
   - ✅ Observer simulation + unlinkability harness (`aura-testkit::privacy`)
   - ✅ Capability soundness tests (`aura-protocol/tests/capability_soundness.rs`)
   - (Cover traffic + adaptive flow costs remain future work)

3. **Complete Guardian Recovery** (~2 weeks)
   - Implement recovery ceremony choreography
   - Add cooldown and dispute windows
   - Policy enforcement for recovery operations

4. **Implement Invitation System** (~2 weeks)
   - Content-addressed invitation format
   - Time-bounded invitation tokens
   - Bidirectional key establishment ceremony
   - Acceptance protocol with mutual auth

5. **Implement Peer Discovery (SBB)** ✅ **COMPLETE**
   - ✅ SBB flooding protocol with TTL tracking and duplicate detection
   - ✅ Bidirectional relationship key derivation extending DKD system
   - ✅ HPKE-style envelope encryption with traffic analysis resistant padding
   - ✅ Capability-aware flooding with Web-of-Trust integration
   - ✅ Complete integrated system with comprehensive end-to-end testing

6. **Complete Transport Layer** ✅ **COMPLETE**
   - ✅ WebSocket implementation for browser compatibility (required for web platform)
   - ✅ STUN client integration for NAT reflexive address discovery
   - ✅ Coordinated hole-punching for QUIC (simultaneous open with nonce coordination)
   - ✅ Contact-mediated relay formalization with flow budget enforcement
   - ✅ Comprehensive integration testing and documentation

7. **Choreography Projection Pilot** (~2 weeks overlap)
   - Project `AddDevice` choreography end-to-end (macro → rumpsteak → runtime)
   - Replace manual async implementation once tests pass
   - Document projection tooling + graduation criteria

**Total estimated time to 1.0: ~8 weeks (2 months)**

### What Makes 1.0 "Complete"

1. **Formal Foundation**: Whole system model fully implemented with:
   - ✅ All algebraic types and laws enforced
   - ✅ Effect system with clean handler separation
   - ✅ MPST with capability guards, journal-coupling, and leakage budgets
   - ❌ Privacy contracts verified in simulation (not started)

2. **Reference Implementation**: Application layer demonstrates:
   - ⚠️ Threshold identity with social recovery works end-to-end (60% complete)
   - ❌ 20 friends can create accounts, add guardians, and recover (not yet functional)
   - ✅ Offline-first with automatic sync (CRDT-based, works)
   - ⚠️ Safe protocol upgrades via semantic versioning (spec in `docs/102_dist_maintenance.md`, implementation/testing pending)

3. **Validation**: System meets all project constraints:
   - ✅ Fully P2P (no servers) - architecture supports it
   - ✅ Offline-first (CRDT-based) - complete
   - ✅ Real security (threshold crypto) - FROST implemented
   - ⚠️ Upgrade safety (versioned protocols) - types ready, testing needed
   - ⚠️ Consent-based (invitation system) - in progress

## Out of Scope (for 1.0)

### Whole System Model Extensions

These would enhance the formal model but aren't required for initial validation:

- Advanced privacy budgets with differential privacy integration
- Multi-level security (MLS) tree structures
- Zero-knowledge proofs for capability delegation
- Formal verification of all protocols in Quint (partial verification sufficient for 1.0)

### Application Layer Features

Useful features that can be built after 1.0 using the same foundations:

- **Advanced Rendezvous**
  - Group RIDs and multi-account rendezvous answers
  - Guardian-backed rendezvous caching
  - Onion routing and cover traffic for envelopes

- **Guardian Enhancements**
  - Automated guardian rotation incentives
  - Reputation scoring and incident analytics
  - Remote attestations for guardian devices

- **Distributed Storage**:
  - Erasure coding and replication strategies
  - Content-addressed storage with deduplication
  - Quota management and garbage collection policies

- **Advanced Search**:
  - Full-text search with privacy preservation
  - Aggregation queries across social graph
  - Relevance ranking and personalization

- **Multiple Transports**:
  - BLE mesh for offline local discovery
  - WebRTC for NAT traversal
  - Tor integration

- **Complex Policies**:
  - Service signers (delegated authority)
  - Fine-grained rate limits and quotas
  - Conditional capabilities (time-based, location-based)

- **Invitation Delivery Mechanisms**:
  - QR code generation and scanning
  - Deep links for mobile apps
  - Integration with messaging platforms (Signal, email, etc.)
  - NFC tap-to-invite

- **Cross-App Integration**:
  - Secure messaging built on threshold identity
  - Collaborative editing with CRDT state
  - Multi-party computation protocols
  - Decentralized social features

- **Advanced Developer Tools**:
  - Visual protocol debugger
  - Trace replay and time-travel debugging
  - Automated property-based test generation
  - Performance profiling and optimization

- **Graphical User Interfaces**:
  - Web-based console (Leptos + WebAssembly)
  - Mobile native apps (iOS, Android)
  - Desktop GUI applications
  - Real-time visualization and monitoring

### Why These Are Out of Scope

The 1.0 goal is to **validate the whole system model with a reference application** (threshold identity + social recovery). Once we prove:
1. The formal model is sound and implementable
2. The application layer can compose primitives to deliver features
3. 20 friends can use it twice weekly

...then we have validation that the architecture works. All "out of scope" features can be built using the same formal foundations without changing the core model.

This approach de-risks the project: if the whole system model is wrong, we find out early. If it's right, we can build anything on top of it.
