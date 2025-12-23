import Aura.Consensus.Types
import Aura.Consensus.Agreement
import Aura.Consensus.Validity
import Aura.Consensus.Evidence
import Aura.Consensus.Equivocation
import Aura.Consensus.Frost
import Aura.Journal       -- CRDT semilattice proofs (merge commutativity, associativity, idempotence)
import Aura.KeyDerivation -- Context-specific key derivation isolation (PRF security assumption)
import Aura.GuardChain    -- Guard evaluation cost calculation correctness
import Aura.FlowBudget    -- Budget charging monotonicity and exactness
import Aura.Frost         -- FROST threshold signing session/round consistency
import Aura.TimeSystem    -- Timestamp comparison reflexivity, transitivity, and privacy

/-!
# Aura Formal Verification

Top-level module re-exporting all verification components.
Each submodule proves invariants about a specific Aura subsystem.

## Module Overview

### Consensus
- `Aura.Consensus`: Threshold consensus protocol proofs
  - Agreement, validity, equivocation detection, FROST integration
  - Evidence CRDT semilattice properties

### Infrastructure
- `Aura.Journal`: CRDT semilattice proofs
- `Aura.KeyDerivation`: Context-specific key derivation isolation
- `Aura.GuardChain`: Guard evaluation cost calculation
- `Aura.FlowBudget`: Budget charging monotonicity
- `Aura.Frost`: FROST session/round consistency
- `Aura.TimeSystem`: Timestamp ordering properties

## Review Entry Points

For reviewers, start with the Claims bundles in each module:
- `Consensus.Agreement.agreementClaims`
- `Consensus.Validity.validityClaims`
- `Consensus.Evidence.evidenceClaims`
- `Consensus.Equivocation.equivocationClaims`

Then check axioms in `Aura.Assumptions`.
-/
