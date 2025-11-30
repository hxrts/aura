# Aura

Aura is a private peer-to-peer social network. Its design takes the following as points of departure:

**Identity is relational**

Identity emerges through shared contexts rather than global identifiers. Each relationship forms its own identity boundary, an opaque authority. Threshold policies and device membership are maintained through commitment tree reduction. This enables social recovery without exposing private keys. 

**Your friends are the network**

Aura runs as an encrypted mesh across the social graph. Distributed protocols run within scoped session and channel contexts that conceal participant structure. Storage, gossip, rendezvous, and consensus all operate through these context boundaries.

**Agency from consent**

Relationships are expressed through a web-of-trust. Capabilities form a semilattice that restricts authority through attenuation. Information flow is governed by explicit consent predicates. Consent enables participants to coordinate more flexibly by ensuring boundaries are by design.

## Architecture

Aura implements a choreographic programming model that projects global protocols into local session types. The architecture is organized into layers that separate interfaces from implementations and isolates impure evaluation through algebraic effects. This enables deterministic testing and simulation.

Most state evolves through CRDT merges that require no coordination. Journals store facts that merge via set union and reduce deterministically. When operations require linearization beyond CRDT convergence, Aura runs single-shot consensus scoped to a context-level witness group, with leaderless fallback. Each instance binds an operation to an explicit prestate hash. Witnesses produce threshold signature shares over the deterministic result. Compact commit facts produced by consensus are then merged into the journal.

Pure evaluation enforces authorization, consent predicates, and resource budgets, returning effects as data. Effect commands are executed by an async interpreter. The separation between pure decision logic and effectful execution enables deterministic testing. The simulator runs protocol code with mock interpreters that provide full control over network conditions, fault injection, and state inspection.

For more details see [System Architecture](docs/001_system_architecture.md) and [Project Structure](docs/999_project_structure.md).

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

## Development TUI

The TUI provides an interactive terminal interface for demonstrating Aura's recovery flows with the deterministic simulator.

### Building the Development Binary

The TUI requires the `development` feature which includes the simulator and testkit:

```bash
# Enter development shell
nix develop

# Build CLI with development features (TUI + simulator + testkit)
cargo build -p aura-cli --features development --release

# The binary is at: target/release/aura
```

### Running the TUI

#### Demo Mode (Guardian Recovery Workflow)

The interactive demo walks through Bob's guardian recovery journey:

```bash
# Interactive demo (default - press Enter/Space to advance through phases)
./target/release/aura demo human-agent

# With auto-advance (non-interactive):
./target/release/aura demo human-agent \
  --seed 42 \              # Deterministic seed for reproducibility
  --verbose \              # Enable detailed logging
  --auto-advance \         # Auto-progress through phases
  --timeout-minutes 15 \   # Demo timeout
  --guardian-delay-ms 3000 # Delay for guardian responses

# Recovery workflow demo (CLI-based)
./target/release/aura demo recovery-workflow

# Scenario-driven demo setup
./target/release/aura demo scenario \
  --participants 3 \
  --threshold 2 \
  --setup-chat

# View demo statistics
./target/release/aura demo stats --detailed
```

**Demo Controls:**
- `Enter` or `Space`: Advance to next phase (on information screens)
- `i`: Send a message (in GroupChat phase)
- `s`: Start recovery (in DataLoss phase)
- `a`: Alice approves recovery
- `c`: Charlie approves recovery
- `q`: Exit demo
- `h`: Toggle help

#### Normal TUI Mode (Interactive Application)

The normal TUI provides full interactive access to Aura's features:

**Note:** The production TUI is under development. Currently, individual features are available via CLI commands:

```bash
# Chat operations
./target/release/aura chat list
./target/release/aura chat send --group-id <id> --message "Hello"

# Guardian management
./target/release/aura recovery list-guardians
./target/release/aura recovery initiate

# Account management
./target/release/aura status
./target/release/aura authority show --authority-id <id>
```

A unified TUI interface integrating all features is planned for a future release.

### TUI Controls (General)

- `q` or `Ctrl+C`: Exit the TUI
- Arrow keys: Navigate between screens
- `Enter`: Select/confirm
- `Tab`: Cycle focus between components
- `/`: Open command palette

### Available Demo Subcommands

| Command | Description |
|---------|-------------|
| `human-agent` | Interactive TUI for Bob's guardian recovery journey |
| `recovery-workflow` | CLI-based recovery demo with simulator |
| `orchestrator` | Run orchestrator in interactive mode |
| `scenario` | Scenario-driven demo setup |
| `stats` | View demo statistics and history |
