# Quint Specs Used By Simulator Tests

This directory holds the lightweight Quint specs exercised by the simulator test suite and CI smoke checks. Full protocol models now live under `verification/quint/`.

## Contents

- **dkd_minimal.qnt** — Minimal deterministic key-derivation harness for quick bounded checks.

> Note: The guard-chain invariants now reside in `verification/quint/authorization.qnt`, and the full FROST protocol specification lives in `verification/quint/consensus/frost.qnt` alongside the other primary specs.

## Running

```bash
quint verify dkd_minimal.qnt
```

Use `just quint-typecheck-all` from the repo root to typecheck both the primary specs and these test harnesses.
