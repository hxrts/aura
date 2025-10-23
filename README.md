# Aura

Threshold identity and encrypted storage platform built on relational security principles.

## Overview

Current identity systems force an impossible choice: trust a single device that can be lost or compromised, or trust a corporation that can lock you out. Aura rejects both. Instead, it builds on the trust you already have: friends, family, and your own devices working together through threshold cryptography and social recovery.

## Phase 0 Status

This implementation is in **Phase 0**: Identity Core

**Implemented:**
- Threshold Ed25519 root key (FROST)
- Deterministic Key Derivation (DKD)
- Session epoch + presence ticket infrastructure
- Authenticated CRDT ledger (Automerge-based)
- CLI for smoke tests

**In Progress:**
- Full device add/remove workflow (requires peer coordination)
- Guardian invitation and recovery flow
- Multi-device threshold signing coordination

**Planned (Phase 1):**
- Storage MVP with encrypted chunk store
- Transport adapter (HTTPS relay)
- Proof-of-storage verification
- End-to-end recovery flow

## Quick Start

### Prerequisites

- Nix with flakes enabled (recommended)
- OR: Rust 1.75+ with Cargo

### Using Nix (Recommended)

```bash
# Enter development shell
nix develop

# Build the project
just build

# Run smoke tests
just smoke-test
```

### Using Cargo Directly

```bash
# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Install CLI
cargo install --path crates/ui-cli
```

## Phase 0 Demo

```bash
# 1. Initialize a new account with 2-of-3 threshold
aura init -n 3 -t 2 -o .aura

# 2. Check account status
aura status -c .aura/config_1.toml

# 3. Test key derivation
aura test-dkd --app-id "my-app" --context "test-context" -f .aura/config_1.toml
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ Application Clients (BitChat-lite, wallets, etc.)           │
└──────────────▲──────────────────────────────────────────────┘
               │
┌──────────────┴──────────────────────────────────────────────┐
│ DeviceAgent API (derive_simple_identity, presence tickets)  │
└──────────────▲──────────────────────────────────────────────┘
               │
┌──────────────┴──────────────────────────────────────────────┐
│ Threshold Signing Orchestrator (FROST DKG, signing)         │
└──────────────▲──────────────────────────────────────────────┘
               │
┌──────────────┴──────────────────────────────────────────────┐
│ Authenticated CRDT Ledger (Automerge)                       │
└─────────────────────────────────────────────────────────────┘
```

## Project Structure

```
.
├── crates/
│   ├── agent/          # DeviceAgent API
│   ├── orchestrator/   # FROST threshold signing
│   ├── ledger/         # Authenticated CRDT
│   ├── storage/        # Encrypted storage (Phase 1)
│   ├── transport/      # Transport layer (Phase 1)
│   └── ui-cli/         # CLI tool
├── docs/               # MVP documentation
├── docs2/              # Future specifications
├── flake.nix           # Nix development environment
└── justfile            # Task automation
```

## Development

### Available Commands

```bash
just build              # Build all crates
just test               # Run all tests
just check              # Run checks (format, clippy, test)
just smoke-test         # Run Phase 0 smoke tests
just docs               # Generate documentation
just watch              # Watch and rebuild on changes
```

### Running Tests

```bash
# All tests
cargo test --workspace

# Specific crate
cargo test -p aura-orchestrator

# With output
cargo test --workspace -- --nocapture
```

## Phase 0 Exit Criteria

- [x] FROST-based DKG generates threshold shares
- [x] Threshold signing produces valid signatures
- [x] DKD derives app-specific keys deterministically
- [x] Presence tickets enforce session epoch
- [x] CRDT stores and replicates account state
- [ ] Device add/remove via threshold-signed events (partial - requires peer coordination)
- [ ] Session epoch bump invalidates old tickets (implemented, needs integration testing)

## Documentation

See the `docs/` directory for detailed specifications:

- [010_motivation.md](docs/010_motivation.md) - Why Aura exists
- [020_architecture.md](docs/020_architecture.md) - System architecture
- [030_identity_spec.md](docs/030_identity_spec.md) - Identity specification
- [060_phased_roadmap.md](docs/060_phased_roadmap.md) - Implementation roadmap

## Security Note

**This is a prototype implementation for Phase 0.**

- Key shares are stored unencrypted in test files
- No hardware sealing or OS keystore integration
- Single-participant DKD (multi-party aggregation not yet implemented)
- No peer-to-peer coordination yet

**Do not use with real secrets or in production.**

## License

MIT OR Apache-2.0

## Contributing

This project is in early development. See `docs/060_phased_roadmap.md` for the implementation plan.

