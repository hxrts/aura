import Aura.Proofs.Consensus.Agreement
import Aura.Proofs.Consensus.Validity
import Aura.Proofs.Consensus.Evidence
import Aura.Proofs.Consensus.Equivocation
import Aura.Proofs.Consensus.Frost
import Aura.Proofs.Consensus.Liveness
import Aura.Proofs.Consensus.Adversary

/-!
# Consensus Proofs Summary

This module re-exports all Claims bundles for consensus verification.
Reviewers can use this as an entry point to audit what has been proven.

## Claims Bundles

Each module provides a Claims bundle that collects its theorems:

| Bundle | Module | Properties Proven |
|--------|--------|-------------------|
| `agreementClaims` | Agreement | Agreement, unique commit, determinism |
| `validityClaims` | Validity | Threshold, prestate binding |
| `evidenceClaims` | Evidence | CRDT merge properties |
| `equivocationClaims` | Equivocation | Detection soundness/completeness |
| `frostClaims` | Frost | Session consistency, threshold |
| `frostOrchestratorClaims` | Frost | Aggregation safety |
| `livenessClaims` | Liveness | Termination under synchrony |
| `adversaryClaims` | Adversary | Byzantine tolerance |

## Proof Status

All proofs are complete (no `sorry` placeholders):

**Agreement**:
- `agreement`: Valid commits for same consensus have same result
- `unique_commit`: At most one CommitFact per ConsensusId
- `commit_determinism`: Same shares produce same commit

**Validity**:
- `threshold_reflexivity`: Threshold check is reflexive
- `prestate_binding`: Prestate hash binding is reflexive

**Evidence CRDT**:
- `merge_comm`: Merge is commutative (membership-wise)
- `merge_assoc`: Merge is associative
- `merge_idem`: Merge is idempotent
- `merge_preserves_commit`: Merge preserves commit facts
- `equivocator_preserved`: Equivocators grow monotonically
- `votes_preserved`: Votes grow monotonically

**Equivocation**:
- `detection_soundness`: Detection only on actual equivocation
- `detection_completeness`: All equivocations detectable
- `exclusion_correctness`: Equivocators excluded from aggregation
- `honest_never_detected`: Honest witnesses never falsely accused

**FROST Integration**:
- `share_session_consistency`: All shares have same session
- `share_result_consistency`: All shares have same result
- `aggregation_threshold`: Aggregation requires ≥k shares
- `aggregatable_implies_valid_commit`: Aggregatable shares form valid commit

**FROST Orchestrator**:
- `aggregate_same_session_round`: Aggregation ensures session/round consistency

**Liveness** (axiomatized, verified in Quint):
- `terminationUnderSynchrony`: Eventually commits under synchrony
- `fastPathBound`: Fast path timing bound
- `fallbackBound`: Fallback timing bound

**Adversary**:
- `byzantine_threshold_consistent`: Threshold check consistency
- `byzantine_count_bound`: Byzantine count bounded by set size
- `honest_can_commit`: Honest witnesses can commit

## Usage

```lean
import Aura.Proofs.Consensus.Summary

-- Access all claims bundles
#check Aura.Proofs.Consensus.Agreement.agreementClaims
#check Aura.Proofs.Consensus.Validity.validityClaims
#check Aura.Proofs.Consensus.Evidence.evidenceClaims
#check Aura.Proofs.Consensus.Equivocation.equivocationClaims
#check Aura.Proofs.Consensus.Frost.frostClaims
#check Aura.Proofs.Consensus.Frost.frostOrchestratorClaims
#check Aura.Proofs.Consensus.Liveness.livenessClaims
#check Aura.Proofs.Consensus.Adversary.adversaryClaims
```
-/

namespace Aura.Proofs.Consensus.Summary

-- Re-export Claims bundles for convenient access
open Aura.Proofs.Consensus.Agreement (agreementClaims AgreementClaims)
open Aura.Proofs.Consensus.Validity (validityClaims ValidityClaims)
open Aura.Proofs.Consensus.Evidence (evidenceClaims EvidenceClaims)
open Aura.Proofs.Consensus.Equivocation (equivocationClaims EquivocationClaims)
open Aura.Proofs.Consensus.Frost (frostClaims FrostClaims frostOrchestratorClaims FrostOrchestratorClaims)
open Aura.Proofs.Consensus.Liveness (livenessClaims LivenessClaims)
open Aura.Proofs.Consensus.Adversary (adversaryClaims AdversaryClaims)

/-- Master bundle containing all consensus verification claims. -/
structure ConsensusClaims where
  agreement : AgreementClaims
  validity : ValidityClaims
  evidence : EvidenceClaims
  equivocation : EquivocationClaims
  frost : FrostClaims
  frostOrchestrator : FrostOrchestratorClaims
  liveness : LivenessClaims
  adversary : AdversaryClaims

/-- The complete consensus verification bundle. -/
def consensusClaims : ConsensusClaims where
  agreement := agreementClaims
  validity := validityClaims
  evidence := evidenceClaims
  equivocation := equivocationClaims
  frost := frostClaims
  frostOrchestrator := frostOrchestratorClaims
  liveness := livenessClaims
  adversary := adversaryClaims

end Aura.Proofs.Consensus.Summary
