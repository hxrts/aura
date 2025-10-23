# 060 · Phased Roadmap

This document outlines the iterative development plan for Aura, from the foundational identity core to a rich ecosystem of integrations. Each phase builds upon the last, delivering a coherent set of features with clear exit criteria.

## Phase 0 · Identity Core

**Context:** This phase is the foundation of the entire system. It focuses on creating a secure, decentralized identity that is not tied to a single device. Everything else in Aura builds on the guarantees established here.

**Goals**
- **Threshold Ed25519 root key (FROST):** Provide a single MPC-managed root for the prototype while deferring P-256/WebAuthn compatibility to a later iteration.
- **Deterministic key derivation (`derive_simple_identity`):** Allow applications to derive context-specific, unlinkable keys from the root identity, preserving user privacy across different apps.
- **Session epoch + presence ticket infrastructure:** Prevent replay attacks and probing of offline devices by ensuring that only currently-active and authorized devices can communicate.
- **Authenticated CRDT ledger:** Use an `automerge`-based CRDT as the distributed source of truth for account state (devices, guardians, policies), with all high-impact changes authorized by threshold signatures.

**Recommended Rust Crates**
- Cryptography: `frost-core`, `ed25519-dalek`, `blake3`, `hkdf`
- CRDT & serialization: `automerge`, `serde`, `serde_cbor`, `serde_json`
- Noise / networking: `snow`, `tokio`, `reqwest` (for bootstrap), `axum` (CLI/API)
- Certificates & tokens: `biscuit-auth`, `time`, `uuid`

**Deliverables**
- **DeviceAgent API:** A clear, high-level API (`derive_simple_identity`, `issue_presence_ticket`) that abstracts the complexity of the underlying FROST control plane from application developers.
- **Threshold Signing Orchestrator:** A service that manages the multi-party computation (MPC) for Distributed Key Generation (DKG), signing, and resharing operations.
- **CRDT Implementation:** A robust CRDT that stores the account state and enforces that all critical changes are backed by a threshold-signed event.
- **CLI Smoke Tests:** A command-line interface to perform and verify core operations: adding/removing devices, adding a guardian, and manually bumping the session epoch.

**Key Challenges**
- Implementing the MPC protocols for threshold signatures correctly and securely.
- Ensuring the CRDT-based state machine is robust against network partitions and concurrent edits.
- Designing a clean `DeviceAgent` API that is powerful yet easy to use.

**Exit Criteria**
- A new device can be added to an account, participate in signing, and be removed, with all state changes propagated via the CRDT.
- A test transport implementation correctly rejects connections from devices that do not present a valid, current presence ticket.
- Operators can manually bump the session epoch (without guardian workflow) and observe that previously issued presence tickets are invalidated.

## Phase 1 · Storage MVP

**Context:** With a secure identity in place, this phase makes it useful by adding a minimal, but complete, encrypted storage layer. This allows users to store and replicate data across their device network.

**Goals**
- **One transport adapter (iroh or HTTPS relay) integrated:** Start with a single, well-understood transport to simplify the MVP, while building the infrastructure to support more transports later.
- **Encrypted chunk store with inline metadata manifests:** All data is chunked and encrypted client-side. Object manifests contain inline metadata to allow for atomic, single-pass writes.
- **Proof-of-storage challenge/response:** A mechanism to cryptographically verify that a peer is actually storing the data it claims to be, preventing faulty or malicious peers from breaking availability guarantees.
- **Basic quota tracking + LRU eviction:** Simple mechanisms to manage storage space and prevent abuse.
- **Guardian-based recovery end-to-end (with cooldown):** A full, working recovery flow where guardians can approve the addition of a new device after a mandatory cooldown period.

**Recommended Rust Crates**
- Transport: `quinn`, `reqwest` (HTTPS fallback)
- Storage: `redb` (native), `idb` (wasm IndexedDB)
- Streaming & async: `tokio`, `futures`
- Hashing & crypto: `blake3`, `sha2`, `aes-gcm`, `hpke`
- Caching/quota utilities: `lru`, `dashmap`

**Deliverables**
- **Indexer API (`store_encrypted`, `fetch_encrypted`):** A high-level API for applications to store and retrieve encrypted data.
- **Transport Adapter:** A pluggable component that handles the physical transmission of data and respects the presence ticket security model.
- **Storage Integration Tests:** A suite of tests that cover the full data lifecycle: write, read, replication, and replica verification.
- **Guardian Approval UI:** A basic user interface for guardians to approve or deny recovery requests.

**Key Challenges**
- Building a transactional storage index that works efficiently across both native (`redb`) and browser (`IndexedDB`) environments.
- Securing the transport layer and ensuring the proof-of-storage mechanism is reliable.

**Exit Criteria**
- An example application can successfully store and fetch a piece of data using the `DeviceAgent` and `Indexer` APIs.
- The full recovery flow, including guardian approval and the cooldown period, successfully reissues shares to a new device and restores access to the account's data.
- The quota system correctly prevents a peer's cache from growing beyond its limit.

## Phase 2 · Policy & Integration Enhancements (Optional)

**Context:** This phase matures the platform by adding more sophisticated policy controls and preparing for a wider range of integrations. These features are optional and can be enabled via feature flags.

**Goals**
- **Expand Cedar policies (risk tiers, device posture):** Introduce more granular policies, such as requiring more guardians for high-value transactions or restricting certain operations to "native" devices.
- **Biscuit capabilities for transport delegation:** Use Biscuit tokens to allow for delegated, offline authorization. For example, a peer could prove it has the right to replicate a specific chunk of data without needing to contact the owner.
- **Optional erasure coding and friend caching tiers:** Add more advanced storage strategies to improve durability and performance.
- **Additional transport adapters (BitChat mesh, WebRTC):** Demonstrate the pluggability of the transport layer by adding new options.
- **Passkey/WebAuthn compatibility:** Introduce a P-256 root and hardware-backed sealing once the prototype’s single-key flows are hardened.

**Recommended Rust Crates**
- Policy: `cedar-policy`, `biscuit-auth`
- Erasure coding: `reed-solomon-erasure`
- Alternate transports: `libp2p`, `webrtc`, `btleplug`
- Metrics/observability: `tracing`, `prometheus-client`

**Deliverables**
- **Policy Authoring Docs + Templates:** Documentation and examples to help developers write their own Cedar policies.
- **Capability Verification in Transport:** The transport layer will be updated to understand and enforce Biscuit-based capabilities.
- **Feature Flags:** All new, optional features will be controlled by feature flags to keep the core stable.

**Key Challenges**
- Integrating the Cedar policy engine in a way that is both flexible and performant.
- Designing a clean and generic API for pluggable transports that can accommodate different network topologies (e.g., mesh vs. client-server).

**Exit Criteria**
- A change to a Cedar policy, propagated via the CRDT, successfully changes the requirements for a high-risk operation (e.g., increasing the number of required guardians).
- A delegated capability (Biscuit token) allows one peer to fetch data from another without the data owner being online.
- The optional features (erasure coding, new transports) can be toggled on and off via feature flags without affecting the stability of the Phase 0/1 feature set.

## Phase 3 · Ecosystem Integrations (Nice to have)

**Context:** With a stable and feature-rich core platform, this phase focuses on expanding the Aura ecosystem by building integrations with other systems and applications.

**Ideas**
- **Fully-featured BitChat integration:** Use the BitChat BLE mesh transport for true offline, location-aware data sync between mobile devices.
- **Enterprise connectors (SSO, device attestations):** Integrate with enterprise systems, for example by using device attestation to enforce that only corporate-managed devices can be added to an account.
- **Automated mass recovery tooling:** Build tools for service providers to help users recover accounts at scale.
- **Wallet integrations across chains:** Use the deterministic key derivation feature to create chain-specific signing keys for different cryptocurrency wallets, all managed under a single Aura identity.

**Key Challenges**
- Balancing the generic nature of the Aura platform with the specific needs of each integration.
- Ensuring that integrations do not compromise the core security guarantees of Aura.

## Program Management Tips

- **Scope PRs:** Keep pull requests focused on a single feature or phase to simplify review.
- **Feature Flag:** Default all optional or experimental features to **off** until they are stable and have earned a place in a release.
- **Documentation Parity:** Ensure that any code change is reflected in the relevant specification documents (`docs/` and `docs2/`).
- **Release Branches:** Use release branches to stabilize deliverables for each phase before moving on to the next.
- **Test Everything:** Each new feature should be accompanied by a comprehensive suite of unit and integration tests.
- **Use Detailed Specs:** The documents in `docs2/` contain more detailed specifications that should be used as a reference during implementation.

## Proposed Repository Layout

```
.
├── Cargo.toml             # Top-level crate aggregator
├── crates/
│   ├── agent/             # DeviceAgent implementation (FROST MPC, DKD helpers)
│   ├── orchestrator/      # Threshold signing/DKG coordinator services
│   ├── ledger/            # Authenticated CRDT and event validation logic
│   ├── storage/           # Encrypted chunk store + proof-of-storage
│   ├── transport/         # Pluggable transport adapters (iroh, HTTPS)
│   └── ui-cli/            # CLI for smoke tests and operator tooling
├── examples/
│   └── bitchat-lite/      # Reference client using DeviceAgent + storage APIs
├── docs/                  # MVP documentation set (this directory)
├── docs2/                 # Future-phase specifications (out of scope for MVP)
├── justfile               # Task automation (replaces ad-hoc shell scripts)
├── flake.nix              # Nix flake entrypoint for reproducible dev environments
└── tests/                 # Cross-crate integration and end-to-end scenarios
```

Use feature flags in `Cargo.toml` to isolate experimental crates, and keep cross-cutting utilities (logging, config) inside dedicated sub-crates (`crates/common/`) if they grow beyond simple modules. The Nix flake should expose dev shells and CI builds that wire in the Justfile tasks so contributors share a single entrypoint. *Note: we do not use Docker, we only use Nix*

The roadmap is a living document; adjust phase contents as we learn from real deployments.

> **MVP bar:** ship the smallest surface that still exercises the full loop end-to-end (derive identity → sync CRDT → store & fetch encrypted data → run guardian recovery). Every phase should converge on functional demos, not just APIs.
