# Aura

Aura is a private peer-to-peer social network designed around a few novel concepts:

Social infrastructure • Aura forms an encrypted mesh across the social graph. Gossip, rendezvous, consensus, and storage all operate through scoped contexts.

Relational identity • Identity emerges bottom-up between parties that share encrypted context. Social recovery enables restoring full account access through their relationships.

Bounded autonomy • Coordination becomes intuitive when capabilities are composable and legible to the system, ensuring consent by design.

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/hxrts/aura)

## Architecture

Aura implements a choreographic programming model that projects global protocols into local session types. The architecture is organized into layers that separate interfaces from implementations and isolates impure evaluation through algebraic effects. This enables deterministic testing and simulation.

Most state evolves though CRDT operations, stored facts merge via set union and reduce into a deterministic journal. When operations require linearization beyond CRDT convergence, Aura runs single-shot consensus scoped to a context-level witness group, with leaderless fallback. Each instance binds an operation to an explicit prestate hash. Witnesses produce threshold signature shares over the deterministic result. Compact commit facts produced by consensus are then merged into the journal.

Pure evaluation enforces authorization, consent predicates, and resource budgets, returning effects as data. Effect commands are executed by an async interpreter. The separation between pure decision logic and effectful execution enables deterministic testing. Simulation executes protocol code with mock interpreters that provide full control over network conditions, fault injection, and state inspection.

For more details see [System Architecture](docs/001_system_architecture.md) and [Project Structure](docs/999_project_structure.md).

## Quick Start

```sh
# Enter dev shell
nix develop

# Build binary with development features and start TUI in demo mode
just demo

# Build production binary
just build

# Start the production release with TUI
./bin/aura tui
```
