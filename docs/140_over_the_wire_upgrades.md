# RFC 140: A Typed Framework for Over-the-Wire Upgrades

**Related**: [workspace_cleanup.md](../work/workspace_cleanup.md)

## 1. Executive Summary

This document proposes a framework for enabling safe, verifiable, and platform-agnostic Over-the-Wire (OTW) upgrades for the Aura network. The design leverages the unified `ProtocolLifecycle` trait to manage different protocol versions and introduces a dedicated `UpgradeCoordinator` protocol to orchestrate network-wide updates. 

To ensure security and portability, this proposal strongly recommends a WebAssembly (WASM)-based execution environment. This allows new protocol logic to be downloaded and executed safely in a deterministic sandbox across all supported platforms (native and web). The framework defines a typed versioning scheme to distinguish between backwards-compatible "soft forks" and consensus-breaking "hard forks," enabling robust version negotiation between peers.

## 2. Forking Model & Typed Versioning

To reason about compatibility, we must formally define what constitutes a breaking change. We will adopt Semantic Versioning (`MAJOR.MINOR.PATCH`) and map it directly to the concept of soft and hard forks.

**Definitions:**
*   **Hard Fork (`MAJOR` version change):** A change that expands the protocol's action space. This includes adding new message types or state transitions that older clients cannot understand. Hard forks are consensus-breaking.
*   **Soft Fork (`MINOR` version change):** A change that restricts the protocol's action space or adds a backwards-compatible feature. Older clients can still safely interoperate with newer clients, as the new logic is a subset of what they already understand.
*   **Patch (`PATCH` version change):** A bug fix or performance improvement that does not alter the protocol's observable behavior or state machine.

This will be captured in a `ProtocolVersion` struct within `protocol-core`:

```rust
// in crates/protocol-core/src/version.rs
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProtocolVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

impl ProtocolVersion {
    /// A hard fork occurs when MAJOR versions differ.
    pub fn is_hard_fork_compatible(&self, other: &Self) -> bool {
        self.major == other.major
    }

    /// A soft fork is backwards compatible. A peer can interact with any peer
    /// that has an equal or lower MINOR version (for the same MAJOR version).
    pub fn is_soft_fork_compatible(&self, other: &Self) -> bool {
        self.is_hard_fork_compatible(other) && self.minor >= other.minor
    }
}
```

## 3. Version Signalling & Negotiation

For peers to safely interact, they must be aware of each other's capabilities. We will achieve this asynchronously by making protocol support part of the replicated, signed state.

**Proposal: Replicated Version Manifest**

The `DeviceMetadata` struct within the `AccountLedger` CRDT will be extended to include a manifest of supported protocols and their versions.

```rust
// In the CRDT state for each device/account
pub struct DeviceMetadata {
    // ... other fields
    pub supported_protocols: BTreeMap<ProtocolName, Vec<ProtocolVersion>>,
}
```

This design allows for deterministic, asynchronous version negotiation. When Alice wants to initiate a protocol with Bob, her `ProtocolOrchestrator` will:
1.  Read its local, replicated copy of Bob's `DeviceMetadata`.
2.  Find the highest `ProtocolVersion` that is compatible for both participants.
3.  If no compatible version exists (due to a hard fork where Bob has not yet upgraded), the orchestrator can make an informed decision to wait or fail gracefully, without requiring a synchronous, per-protocol handshake.

## 4. The Upgrade Coordinator Protocol

To manage the rollout of new versions, we will introduce a new, dedicated `UpgradeCoordinator` protocol. This protocol will itself be a `ChoreographicProtocol`, ensuring the upgrade process is as robust and verifiable as any other network operation.

**Protocol States & Lifecycle:**

1.  `Proposing`: A participant initiates the protocol, proposing an upgrade to a new `ProtocolVersion`. The proposal includes a URL and cryptographic hash for the new protocol's WASM module.
2.  `Acknowledging`: Peers receive the proposal and begin downloading the new module in the background, verifying its hash upon completion.
3.  `SignallingReady`: Once a peer has successfully fetched and verified the new module, it updates the `supported_protocols` manifest in its own ledger. This acts as a public, signed signal of readiness.
4.  `AwaitingQuorum`: The protocol observes the ledgers of all participants, waiting for a predefined threshold (e.g., M-of-N) to signal readiness.
5.  `Activating`: Once quorum is reached, a final `ActivateVersion` event is committed to the ledger. From this point on, all orchestrators on the network will use the new version for any new protocol instances.

## 5. Secure & Portable Execution with WASM

The most critical piece of this proposal is *how* new protocol logic is executed. To enable true, secure OTW upgrades, a VM-based approach is essential.

**Recommendation: Adopt a WebAssembly (WASM) Execution Environment.**

While this introduces engineering complexity, the benefits are immense and align perfectly with Aura's security and portability goals.

**Key Benefits:**
*   **Security through Sandboxing**: New protocol logic downloaded from the network is executed in a memory-safe sandbox. The WASM module has no inherent access to the network, filesystem, or any other host resources. All capabilities are explicitly and safely injected via the `ProtocolCapabilities` bundle during the `step` call.
*   **True Portability**: The *exact same* `my_protocol.wasm` binary can be executed on all target platforms: iOS, Android, macOS, Linux, and in modern web browsers. The upgrade payload becomes completely platform-agnostic.
*   **Flexibility and Extensibility**: This architecture allows the network to evolve safely over time. It also opens the door for third-party protocol development in the future.

**The Alternative (Not Recommended):**
The only alternative is a "feature flag" model where new logic is shipped in a full application update (via an App Store or package manager) and the `UpgradeCoordinator` simply coordinates when to activate it. This is far less flexible, much slower to deploy, and does not support the dynamic addition of new protocols.

## 6. Integrated Upgrade & Execution Workflow

This is how the full lifecycle would work:

1.  The `UpgradeCoordinator` protocol completes, and an `ActivateVersion` event is committed to the ledger. This event contains the location (URL) and hash of the new `cool_protocol_v2.wasm` module.
2.  The `ProtocolOrchestrator` on each device sees this event and instructs a background service to download and cache the WASM module, verifying its hash.
3.  Later, Alice wants to start `cool_protocol` with Bob.
4.  Her orchestrator checks her and Bob's `supported_protocols` manifests, sees that they both support `v2.0.0`, and selects it.
5.  The orchestrator loads the `cool_protocol_v2.wasm` module from its cache into a WASM runtime (e.g., Wasmer, Wasmtime).
6.  It calls the concrete `CoolProtocol::new()` constructor (which is an exported function from the WASM module) to create a new instance.
7.  From this point on, the orchestrator interacts with the protocol via the `ProtocolLifecycle` trait. Each call to `step()` passes the `ProtocolInput` *into* the WASM sandbox and receives the `ProtocolStep` result *out* of the sandbox, with the runtime ensuring safe memory management and communication.

## 7. Conclusion

This framework provides a comprehensive solution for evolving the Aura network. By combining typed versioning, a robust coordination protocol, and a secure WASM-based execution environment, we can achieve safe, verifiable, and truly platform-agnostic Over-the-Wire upgrades. This is a foundational investment in the long-term health and flexibility of the network.
