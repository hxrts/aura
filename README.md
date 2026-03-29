# Aura

Aura is a private peer-to-peer communication system built around three core requirements. ① The network must be fully P2P with no dedicated servers, DNS, or central software distribution authority. ② The system must tolerate intermittent connectivity and device loss. ③ All channels must be end-to-end encrypted with bounded forward secrecy. Everything else in the design follows from these constraints.

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/hxrts/aura)

## Architecture

Aura implements a choreographic programming model that projects global protocols into local session types. The architecture is organized into layers, separating interfaces from implementations and isolating impure evaluation through algebraic effects.

State evolves through CRDT operations. Stored facts merge by set union and reduce into a deterministic journal. When operations require linearization beyond CRDT convergence, context-scoped witness groups run single-shot threshold consensus with leaderless fallback.

Authorization, consent, and resource budgets are enforced in a pure evaluation pass that returns effects as data. Effect handlers execute through an async interpreter. This separation enables deterministic testing and simulation.

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
