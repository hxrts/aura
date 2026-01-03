-- Infrastructure proofs
import Aura.Proofs.Journal
import Aura.Proofs.FlowBudget
import Aura.Proofs.GuardChain
import Aura.Proofs.TimeSystem
import Aura.Proofs.KeyDerivation
import Aura.Proofs.ContextIsolation

-- Consensus proofs
import Aura.Proofs.Consensus.Agreement
import Aura.Proofs.Consensus.Validity
import Aura.Proofs.Consensus.Evidence
import Aura.Proofs.Consensus.Equivocation
import Aura.Proofs.Consensus.Frost
import Aura.Proofs.Consensus.Liveness
import Aura.Proofs.Consensus.Adversary
import Aura.Proofs.Consensus.Summary

/-!
# Aura Proof Entry Point

Top-level module re-exporting all proofs for reviewer inspection.
Each proof module provides a Claims bundle collecting its theorems.

## Directory Structure

```
Aura/Proofs/
├── Journal.lean              -- CRDT semilattice proofs
├── FlowBudget.lean           -- Budget charging proofs
├── GuardChain.lean           -- Guard evaluation proofs
├── TimeSystem.lean           -- Timestamp ordering proofs
├── KeyDerivation.lean        -- PRF isolation proofs
├── ContextIsolation.lean     -- Context isolation proofs
└── Consensus/
    ├── Agreement.lean        -- Agreement safety proofs
    ├── Validity.lean         -- Validity proofs
    ├── Evidence.lean         -- Evidence CRDT proofs
    ├── Equivocation.lean     -- Equivocation detection proofs
    ├── Frost.lean            -- FROST integration proofs
    ├── Liveness.lean         -- Liveness claims (axiomatized)
    ├── Adversary.lean        -- Byzantine model proofs
    └── Summary.lean          -- Claims bundle aggregation
```

## Quint Correspondence
- Directory: verification/quint/
- All property specifications have corresponding Lean proofs

## Rust Correspondence
- Directory: crates/aura-core/, crates/aura-protocol/
- Critical invariants proven here match runtime assertions

## Review Guide

Start by inspecting the Claims bundles in each module:

### Infrastructure Proofs
- `Aura.Proofs.Journal.journalClaims` - CRDT semilattice properties
- `Aura.Proofs.FlowBudget.flowBudgetClaims` - Budget charging correctness
- `Aura.Proofs.GuardChain.guardChainClaims` - Guard evaluation determinism
- `Aura.Proofs.TimeSystem.timeSystemClaims` - Timestamp ordering properties
- `Aura.Proofs.KeyDerivation.keyDerivationClaims` - Key isolation from PRF security
- `Aura.Proofs.ContextIsolation.contextIsolationClaims` - Context separation and bridge authorization

### Consensus Proofs
- `Aura.Proofs.Consensus.Agreement.agreementClaims` - Agreement safety
- `Aura.Proofs.Consensus.Validity.validityClaims` - Valid value acceptance
- `Aura.Proofs.Consensus.Evidence.evidenceClaims` - Evidence CRDT properties
- `Aura.Proofs.Consensus.Equivocation.equivocationClaims` - Equivocation detection
- `Aura.Proofs.Consensus.Frost.frostClaims` - FROST integration
- `Aura.Proofs.Consensus.Frost.frostOrchestratorClaims` - Aggregation safety
- `Aura.Proofs.Consensus.Liveness.livenessClaims` - Liveness (axiomatized)
- `Aura.Proofs.Consensus.Adversary.adversaryClaims` - Byzantine tolerance
- `Aura.Proofs.Consensus.Summary.consensusClaims` - Main consensus bundle

### Axioms
- `Aura.Assumptions` - Cryptographic primitives (SHA256, FROST, PRF)

Each Claims bundle is a structure containing theorem statements.
The bundle itself is proof that all theorems hold.
-/

namespace Aura.Proofs

/-!
## Aggregate Claims

Re-export all claims bundles for easy access.
-/

-- Infrastructure claims
/-- Journal CRDT semilattice proofs. -/
def journalClaims := Aura.Proofs.Journal.journalClaims

/-- Flow budget charging correctness proofs. -/
def flowBudgetClaims := Aura.Proofs.FlowBudget.flowBudgetClaims

/-- Guard chain evaluation proofs. -/
def guardChainClaims := Aura.Proofs.GuardChain.guardChainClaims

/-- Timestamp ordering proofs. -/
def timeSystemClaims := Aura.Proofs.TimeSystem.timeSystemClaims

/-- Key derivation isolation proofs. -/
def keyDerivationClaims := Aura.Proofs.KeyDerivation.keyDerivationClaims

/-- Context isolation and bridge authorization proofs. -/
def contextIsolationClaims := Aura.Proofs.ContextIsolation.contextIsolationClaims

-- Consensus claims
/-- Consensus agreement proofs. -/
def agreementClaims := Aura.Proofs.Consensus.Agreement.agreementClaims

/-- Consensus validity proofs. -/
def validityClaims := Aura.Proofs.Consensus.Validity.validityClaims

/-- Evidence CRDT proofs. -/
def evidenceClaims := Aura.Proofs.Consensus.Evidence.evidenceClaims

/-- Equivocation detection proofs. -/
def equivocationClaims := Aura.Proofs.Consensus.Equivocation.equivocationClaims

/-- FROST consensus integration proofs. -/
def frostClaims := Aura.Proofs.Consensus.Frost.frostClaims

/-- FROST orchestrator aggregation proofs. -/
def frostOrchestratorClaims := Aura.Proofs.Consensus.Frost.frostOrchestratorClaims

/-- Liveness claims (axiomatized, verified in Quint). -/
def livenessClaims := Aura.Proofs.Consensus.Liveness.livenessClaims

/-- Adversary model proofs. -/
def adversaryClaims := Aura.Proofs.Consensus.Adversary.adversaryClaims

/-- Main consensus bundle containing all consensus verification claims. -/
def consensusClaims := Aura.Proofs.Consensus.Summary.consensusClaims

end Aura.Proofs
