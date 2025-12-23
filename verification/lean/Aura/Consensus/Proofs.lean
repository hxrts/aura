import Aura.Consensus.Agreement
import Aura.Consensus.Validity
import Aura.Consensus.Evidence
import Aura.Consensus.Equivocation
import Aura.Consensus.Frost

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

## Usage

```lean
import Aura.Consensus.Proofs

-- Access all claims bundles
#check Aura.Consensus.Agreement.agreementClaims
#check Aura.Consensus.Validity.validityClaims
#check Aura.Consensus.Evidence.evidenceClaims
#check Aura.Consensus.Equivocation.equivocationClaims
#check Aura.Consensus.Frost.frostClaims
```
-/

namespace Aura.Consensus.Proofs

-- Re-export Claims bundles for convenient access
open Aura.Consensus.Agreement (agreementClaims AgreementClaims)
open Aura.Consensus.Validity (validityClaims ValidityClaims)
open Aura.Consensus.Evidence (evidenceClaims EvidenceClaims)
open Aura.Consensus.Equivocation (equivocationClaims EquivocationClaims)
open Aura.Consensus.Frost (frostClaims FrostClaims)

/-- Master bundle containing all consensus verification claims. -/
structure ConsensusClaims where
  agreement : AgreementClaims
  validity : ValidityClaims
  evidence : EvidenceClaims
  equivocation : EquivocationClaims
  frost : FrostClaims

/-- The complete consensus verification bundle. -/
def consensusClaims : ConsensusClaims where
  agreement := agreementClaims
  validity := validityClaims
  evidence := evidenceClaims
  equivocation := equivocationClaims
  frost := frostClaims

end Aura.Consensus.Proofs
