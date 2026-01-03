-- Domain types and operations
import Aura.Domain.Consensus.Types
import Aura.Domain.Consensus.Frost
import Aura.Domain.Journal.Types
import Aura.Domain.Journal.Operations
import Aura.Domain.FlowBudget
import Aura.Domain.GuardChain
import Aura.Domain.TimeSystem
import Aura.Domain.KeyDerivation

-- Proofs (aggregated via Aura.Proofs)
import Aura.Proofs

/-!
# Aura Formal Verification

Top-level module re-exporting all verification components.

## Directory Structure

```
Aura/
├── Assumptions.lean          -- Cryptographic axioms
├── Types/                    -- Primitive shared types
├── Domain/                   -- Domain types and operations (no proofs)
│   ├── Consensus/
│   │   ├── Types.lean        -- Consensus message types
│   │   └── Frost.lean        -- FROST types and operations
│   ├── Journal/
│   │   ├── Types.lean        -- Fact, Journal structures
│   │   └── Operations.lean   -- merge, reduce, factsEquiv
│   ├── FlowBudget.lean       -- Budget types and charging
│   ├── GuardChain.lean       -- Guard types and evaluation
│   ├── TimeSystem.lean       -- Timestamp types and comparison
│   └── KeyDerivation.lean    -- Key derivation types
├── Proofs/                   -- All proofs centralized
│   ├── Consensus/
│   │   ├── Agreement.lean    -- Agreement safety proofs
│   │   ├── Validity.lean     -- Validity proofs
│   │   ├── Evidence.lean     -- CRDT semilattice proofs
│   │   ├── Equivocation.lean -- Equivocation detection proofs
│   │   ├── Frost.lean        -- FROST integration proofs
│   │   ├── Liveness.lean     -- Liveness claims
│   │   ├── Adversary.lean    -- Byzantine model
│   │   └── Summary.lean      -- Claims bundle aggregation
│   ├── Journal.lean          -- CRDT semilattice proofs
│   ├── FlowBudget.lean       -- Charging correctness
│   ├── GuardChain.lean       -- Evaluation determinism
│   ├── TimeSystem.lean       -- Ordering properties
│   └── KeyDerivation.lean    -- PRF isolation proofs
├── Proofs.lean               -- Reviewer entry point
└── Runner.lean               -- CLI for differential testing
```

## Import Discipline

Domain modules don't import from Proofs:
- `Domain/* ←── Proofs/*`
- Types flow upward, proofs import types

## Review Entry Points

For reviewers, start with `Aura.Proofs` which aggregates all Claims bundles:

### Infrastructure
- `Aura.Proofs.journalClaims`
- `Aura.Proofs.flowBudgetClaims`
- `Aura.Proofs.guardChainClaims`
- `Aura.Proofs.timeSystemClaims`
- `Aura.Proofs.keyDerivationClaims`

### Consensus
- `Aura.Proofs.agreementClaims`
- `Aura.Proofs.validityClaims`
- `Aura.Proofs.evidenceClaims`
- `Aura.Proofs.equivocationClaims`
- `Aura.Proofs.frostClaims`
- `Aura.Proofs.livenessClaims`
- `Aura.Proofs.adversaryClaims`
- `Aura.Proofs.consensusClaims` (master bundle)

Then check axioms in `Aura.Assumptions`.
-/
