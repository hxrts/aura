# Aura

Aura is a private peer-to-peer social network. Its design takes the following as points of departure:

*Identity is relational* • Identity emerges through shared contexts rather than global identifiers. Each relationship forms its own identity boundary, an opaque authority. Threshold policies and device membership are maintained through commitment tree reduction. This enables social recovery without exposing private keys. 

*Your friends are the network* • Aura runs as an encrypted mesh across the social graph. Distributed protocols run within scoped session and channel contexts that conceal participant structure. Storage, gossip, rendezvous, and consensus all operate through these context boundaries.

*Agency from consent* • Relationships are expressed through a web-of-trust. Capabilities form a semilattice that restricts authority through attenuation. Information flow is governed by explicit consent predicates. Consent enables participants to coordinate more flexibly by ensuring boundaries are by design.

## Architecture

Aura implements a choreographic programming model that projects global protocols into local session types. The architecture is organized into layers that separate interfaces from implementations and isolates impure evaluation through algebraic effects. This enables deterministic testing and simulation.

Most state evolves through CRDT merges that require no coordination. Journals store facts that merge via set union and reduce deterministically. When operations require linearization beyond CRDT convergence, Aura runs single-shot consensus scoped to a context-level witness group, with leaderless fallback. Each instance binds an operation to an explicit prestate hash. Witnesses produce threshold signature shares over the deterministic result. Compact commit facts produced by consensus are then merged into the journal.

Pure evaluation enforces authorization, consent predicates, and resource budgets, returning effects as data. Effect commands are executed by an async interpreter. The separation between pure decision logic and effectful execution enables deterministic testing. The simulator runs protocol code with mock interpreters that provide full control over network conditions, fault injection, and state inspection.

For more details see [System Architecture](docs/001_system_architecture.md) and [Project Structure](docs/999_project_structure.md).

## Quick Start

```sh
# Enter dev shell
nix develop

# Build CLI with development features (TUI + simulator + testkit)
cargo build -p aura-cli --bin aura --features development --release

# Demo
./target/release/aura demo human-agent
```
