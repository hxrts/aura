-- Infrastructure proofs
import Aura.Proofs.Journal
import Aura.Proofs.FlowBudget
import Aura.Proofs.GuardChain
import Aura.Proofs.TimeSystem
import Aura.Proofs.KeyDerivation

-- Consensus proofs (still in old location, to be moved in Phase 3 remainder)
import Aura.Consensus.Agreement
import Aura.Consensus.Validity
import Aura.Consensus.Evidence
import Aura.Consensus.Equivocation
import Aura.Consensus.Frost

/-!
# Aura Proof Entry Point

Top-level module re-exporting all proofs for reviewer inspection.
Each proof module provides a Claims bundle collecting its theorems.

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

### Consensus Proofs
- `Aura.Consensus.Agreement.agreementClaims` - Agreement safety
- `Aura.Consensus.Validity.validityClaims` - Valid value acceptance
- `Aura.Consensus.Evidence.evidenceClaims` - Evidence CRDT properties
- `Aura.Consensus.Equivocation.equivocationClaims` - Equivocation detection

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

/-- Consensus agreement proofs. -/
def agreementClaims := Aura.Consensus.Agreement.agreementClaims

/-- Consensus validity proofs. -/
def validityClaims := Aura.Consensus.Validity.validityClaims

/-- Evidence CRDT proofs. -/
def evidenceClaims := Aura.Consensus.Evidence.evidenceClaims

/-- Equivocation detection proofs. -/
def equivocationClaims := Aura.Consensus.Equivocation.equivocationClaims

end Aura.Proofs
