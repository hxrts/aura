# Aura

Aura is a private peer-to-peer communication system built around three core requirements. ① The network must be fully P2P with no dedicated servers, DNS, or central software distribution authority. ② The system must tolerate intermittent connectivity and device loss. ③ All channels must be end-to-end encrypted with bounded forward secrecy. Everything else in the design follows from these constraints.

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/hxrts/aura)

## Architecture

Aura implements a choreographic programming model that projects global protocols into local session types. The architecture is organized into layers, separating interfaces from implementations and isolating impure evaluation through algebraic effects. This enables deterministic testing and simulation.

State evolves though CRDT operations, stored facts merge via set union and reduce into a deterministic journal. When operations require linearization beyond CRDT convergence, Aura runs single-shot consensus among a context-scoped witness group, with leaderless fallback. Each instance binds an operation to an explicit prestate hash. Witnesses produce threshold signature shares over the deterministic result. Compact commit facts produced by consensus are then merged into the journal.

Pure evaluation enforces authorization, consent predicates, and resource budgets, returning effects as data. Effect commands are executed by an async interpreter. The separation between pure decision logic and effectful execution enables deterministic testing. Simulation executes protocol code with mock interpreters that provide full control over network conditions, fault injection, and state inspection.

For more details see [System Architecture](docs/001_system_architecture.md) and [Project Structure](docs/999_project_structure.md).

## Quick Start

```sh
# Enter dev shell
nix develop

# Launch the cross-frontend developer demo UX (TUI + web)
just demo

# Build production binary
just build

# Start the production release with TUI
./bin/aura tui
```
