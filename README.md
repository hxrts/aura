# Aura

[![CI](https://github.com/your-org/aura/actions/workflows/ci.yml/badge.svg)](https://github.com/your-org/aura/actions/workflows/ci.yml)
[![Quint Model Checking](https://github.com/your-org/aura/actions/workflows/ci.yml/badge.svg?job=quint-model-checking)](https://github.com/your-org/aura/actions/workflows/ci.yml)
[![Lean Proofs](https://github.com/your-org/aura/actions/workflows/ci.yml/badge.svg?job=lean-proofs)](https://github.com/your-org/aura/actions/workflows/ci.yml)
[![Consensus Conformance](https://github.com/your-org/aura/actions/workflows/ci.yml/badge.svg?job=consensus-conformance)](https://github.com/your-org/aura/actions/workflows/ci.yml)

Aura is a private peer-to-peer social network designed around a few novel concepts:

Identity is relational • Identity emerges bottom-up between parties that share context. Aura has no transparent state and no global singleton, rather each context is encrypted and governed by its own threshold authority. Aura's relational model enables social recovery and complete account rehydration without transient decryption in scenarios where users have lost all devices.

Your friends are the network • Aura forms an encrypted mesh across the social graph. Distributed protocols run within scoped sessions and channels that conceal participant structure. Gossip, rendezvous, consensus, and storage all operate through these bounded contexts.

Autonomy from consent • Relationships are expressed through a web-of-trust. Capabilities form a semilattice that attenuates authority. Information is governed via consent predicates, enabling participants to coordinate freely by ensuring boundaries are respected by design.

## Architecture

Aura implements a choreographic programming model that projects global protocols into local session types. The architecture is organized into layers that separate interfaces from implementations and isolates impure evaluation through algebraic effects. This enables deterministic testing and simulation.

Most state evolves through CRDT merges that require no coordination. Journals store facts that merge via set union and reduce deterministically. When operations require linearization beyond CRDT convergence, Aura runs single-shot consensus scoped to a context-level witness group, with leaderless fallback. Each instance binds an operation to an explicit prestate hash. Witnesses produce threshold signature shares over the deterministic result. Compact commit facts produced by consensus are then merged into the journal.

Pure evaluation enforces authorization, consent predicates, and resource budgets, returning effects as data. Effect commands are executed by an async interpreter. The separation between pure decision logic and effectful execution enables deterministic testing. The simulator runs protocol code with mock interpreters that provide full control over network conditions, fault injection, and state inspection.

For more details see [System Architecture](docs/001_system_architecture.md) and [Project Structure](docs/999_project_structure.md).

## Quick Start

```sh
# Enter dev shell
nix develop

# Build terminal interface with development features (TUI + simulator + testkit)
cargo build -p aura-terminal --bin aura --features development --release

# Demo
./target/release/aura demo human-agent
```
