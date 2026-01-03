-- Domain types and operations
import Aura.Domain.Consensus.Types
import Aura.Domain.Journal.Types
import Aura.Domain.Journal.Operations
import Aura.Domain.FlowBudget
import Aura.Domain.GuardChain
import Aura.Domain.TimeSystem
import Aura.Domain.KeyDerivation

-- Proofs (aggregated via Aura.Proofs)
import Aura.Proofs

-- Legacy: FROST types still in old location
import Aura.Frost

/-!
# Aura Formal Verification

Top-level module re-exporting all verification components.

## Directory Structure

```
Aura/
├── Assumptions.lean      -- Cryptographic axioms
├── Types/                -- Primitive shared types
├── Domain/               -- Domain types and operations
│   ├── Consensus/Types   -- Consensus message types
│   ├── Journal/          -- Fact and Journal structures
│   ├── FlowBudget        -- Budget charging
│   ├── GuardChain        -- Guard evaluation
│   ├── TimeSystem        -- Timestamp comparison
│   └── KeyDerivation     -- Key derivation
├── Proofs/               -- All proofs centralized
│   ├── Journal           -- CRDT semilattice proofs
│   ├── FlowBudget        -- Charging correctness
│   ├── GuardChain        -- Evaluation determinism
│   ├── TimeSystem        -- Ordering properties
│   └── KeyDerivation     -- PRF isolation
├── Consensus/            -- Consensus proofs (to be moved to Proofs/)
├── Proofs.lean           -- Reviewer entry point
└── Runner.lean           -- CLI for differential testing
```

## Review Entry Points

For reviewers, start with `Aura.Proofs` which aggregates all Claims bundles:
- `Aura.Proofs.journalClaims`
- `Aura.Proofs.flowBudgetClaims`
- `Aura.Proofs.guardChainClaims`
- `Aura.Proofs.timeSystemClaims`
- `Aura.Proofs.keyDerivationClaims`
- `Aura.Proofs.agreementClaims`
- `Aura.Proofs.validityClaims`
- `Aura.Proofs.evidenceClaims`
- `Aura.Proofs.equivocationClaims`

Then check axioms in `Aura.Assumptions`.
-/
