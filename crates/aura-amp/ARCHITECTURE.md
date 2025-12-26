# Aura AMP (Layer 4) - Architecture and Invariants

## Purpose
Orchestrate AMP channel lifecycle and message transport coordination on top of
relational journal facts.

## Inputs
- JournalEffects + OrderClockEffects for canonical fact storage and ordering.
- Guard chain effects from callers for send authorization and budgeting.
- Transport effects for envelope delivery (via higher-level protocols).

## Outputs
- Relational facts (channel checkpoints, epoch bumps, policies).
- Deterministic channel state via reduction.
- Non-canonical evidence cache for consensus provenance (explicitly scoped).

## Invariants
- AMP facts are stored in the context journal using OrderClock ordering.
- Channel epochs are monotone; committed bumps supersede proposals.
- Evidence is optional and does not affect channel state reconstruction.

## Boundaries
- No direct StorageEffects for channel facts (journal only).
- Evidence storage is isolated behind AmpEvidenceEffects.
- Pure state is derived via `aura-journal` reduction.

## Core + Orchestrator Rule
- Pure helpers live under `amp/core`.
- Orchestrators must depend on effects explicitly.
