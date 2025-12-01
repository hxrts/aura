# Aura Lean Verification

This directory contains Lean 4 formal verification modules for Aura's kernel components.

## Setup

The Lean toolchain is managed through Nix. To enter the development environment:

```bash
nix develop
```

## Building

Build the Lean verification modules:

```bash
just lean-build
```

Or directly with lake:

```bash
cd lean && lake build
```

## Module Structure

- `Aura/Journal/` - CRDT and journal reduction verification
  - `Core.lean` - Journal semilattice definitions
  - `Semilattice.lean` - Semilattice proofs
- `Aura/KeyDerivation/` - Contextual key derivation isolation
- `Aura/GuardChain/` - Guard chain evaluation correctness
- `Aura/FlowBudget/` - Flow budget mathematics
- `Aura/Frost/` - FROST protocol state machine
- `Aura/TimeSystem/` - TimeStamp ordering and privacy
- `Aura/Runner.lean` - CLI runner for differential testing (TODO: fix executable build)

## Verification Priorities

Based on the formal verification strategy document:

### High Priority (Lean)
1. **CRDT/Journal** - Prove merge is associative, commutative, idempotent
2. **Key Derivation** - Prove contextual isolation (uniqueness across contexts)
3. **Guard Chain** - Prove cost calculation correctness
4. **Flow Budget** - Prove budget charging properties

### Medium Priority (Lean)
5. **FROST** - Prove aggregate is never called with mixed sessions/rounds
6. **TimeStamp** - Prove transitivity, reflexivity, and privacy properties

## Current Status

- Lean 4.23.0 installed via Nix
- Lake project initialized
- Core module structure created
- Basic theorem statements defined (with `sorry` placeholders)
- Executable runner

## Next Steps

1. Complete the proofs marked with `sorry`
2. Fix the executable build for differential testing
3. Add JSON serialization for Rust↔Lean integration
4. Write differential tests in Rust that call the Lean verifier
5. Add CI integration

## Justfile Commands

- `just lean-init` - Initialize/update Lean project
- `just lean-build` - Build Lean modules
- `just lean-clean` - Clean build artifacts
- `just lean-full` - Full workflow (clean + build + check)

## Notes

- All theorem statements use `sorry` placeholders for now
- The executable build is temporarily disabled - focus is on the library
- Warnings about macOS version mismatches can be ignored (Nix issue)
