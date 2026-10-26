# Structural tree scalability input

`tree_scalability_history.json` contains `(operations, expected TreeState)`: 100
connected device additions and 1,000 connected epoch rotations. Every parent
comes from successful canonical journal `reduce` of the preceding full history.
The combined benchmark decodes all input, populates the entire OpLog and creates
50 peers during measured setup; measured reduction still executes every selected
operation and compares the complete resulting state.

These synthetic attestations test structural reduction only. This data is not
cryptographic verification, credentials, live ownership, checkpoint restoration,
or evidence of host resource settlement.

Regenerate from the workspace root in the supported development environment:

```sh
AURA_BUILD_PACKAGES=aura-testkit scripts/dev/build-budget.sh --lane tree-scalability-generate --no-prune -- \
  bash -c 'cargo run -p aura-testkit --example generate_tree_scalability_fixture > /tmp/tree-scalability-history.json'
cmp /tmp/tree-scalability-history.json crates/aura-testkit/tests/fixtures/tree_scalability_history.json
```

The generator and the live 100-device fixture share
`tree_scalability_history.rs`. Regeneration deliberately performs the expensive
prefix replay outside the benchmark's timed setup. Review any changed fixture
before replacement and execute all five `tree_scalability` fixtures afterward.
