# Aura

Aura is a private peer-to-peer social network which takes the following as a point of departure:

**Identity is relational**

Each account is a opaque threshold authority described by a commitment tree reduction. Social recovery possible via Guardian nomination without requiring decrypted data.

**Your friends are the network**

Aura runs as an encrypted mesh across the social graph. Distributed protocols run within scoped session and channel contexts that never reveal participant structure. Storage, gossip, rendezvous, and consensus all operate through these context boundaries.

**Agency from consent**

Capabilities are expressed through attenuation with sovereign policy. Authorization is handled at send-time, i.e. packets cannot be transmitted without satisfying consent predicates.

## Architecture

Aura’s architecture is built from four interacting systems: journals, session types, consensus, and the effect runtime.

The journal system is a fact-only CRDT. Facts merge through set union and reduce into account state and relational state deterministically. Account state is defined by the commitment tree semilattice. Contexts use the same reduction pipeline for relational facts. This ensures that all replicas converge once they share the same facts.

The choreography and session type system specifies distributed protocols globally and projects them into per-role local types. Projection establishes ordering, duality, and deadlock-freedom. The interpreter executes steps via effect calls, embedding capability checks, journal updates, and flow-budget charges before each send. This couples protocol safety with authorization and budgeting semantics.

Consensus provides single-shot agreement for operations that cannot be expressed as monotone fact growth. Each instance binds an operation to an explicit prestate hash and outputs a commit fact containing a threshold signature. Commit facts merge into journals and are interpreted during reduction. Fast-path and fallback execute through session-typed flows. Consensus is scoped to authorities or relational contexts and never produces a global log.

The effect system supplies the operational substrate. Handlers implement cryptography, storage, transport, and journal operations. All handlers run under an explicit context object. The runtime enforces guard-chain order, capability refinement, and deterministic charging. This keeps side effects isolated, testable, and uniform across native and WASM targets.

For detailed system architecture see the system-architecture document .

See [](docs/001_system_architecture.md) and [Project Structure](docs/999_project_structure.md) for more details.

## Quick Start

Aura builds with Nix.

```
# Enter development shell
nix develop

# Build all crates
nix build .#aura

# Run tests
nix flake check
```
