# aura-consensus

Layer 4 strong-agreement consensus implementation. Core state machine lives under `src/consensus/core/` with protocol orchestration in `src/consensus/`.

## Tests
- Unit + property tests live under `crates/aura-consensus/tests/`.
- Run: `cargo test -p aura-consensus`.
- ITF conformance: `cargo test -p aura-consensus --test consensus_itf_conformance`.

## Notes
- Reference-model and Lean correspondence tests live in `crates/aura-consensus/tests/common/`.
