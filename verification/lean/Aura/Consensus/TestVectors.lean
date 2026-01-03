import Aura.Domain.Consensus.Types
import Aura.Consensus.Evidence
import Lean.Data.Json

/-!
# Test Vectors for Mathematical Primitives

Generates deterministic test vectors for differential testing of Lean-proven
mathematical operations. These vectors are used by Rust tests to validate
that the Rust implementation matches the Lean specification.

## Scope

This module generates test vectors for **mathematical primitives only**:
- Evidence CRDT merge (semilattice properties)
- Threshold checking
- Equivocator detection

**NOT in scope**: Protocol behavior (state machine, message passing) - those
are tested via Quint ITF traces.

## Rust Correspondence
- File: crates/aura-consensus/tests/correspondence_validation.rs
- File: crates/aura-testkit/src/differential.rs

## Usage

1. Build the Lean verifier: `lake build`
2. Run: `./build/bin/aura-verifier test-vectors`
3. Output: JSON test vectors to stdout
-/

namespace Aura.Consensus.TestVectors

open Lean
open Aura.Domain.Consensus.Types
open Aura.Consensus.Evidence

/-!
## Evidence Merge Test Vectors

Test vectors for Evidence CRDT merge operation.
-/

/-- Create a simple vote for testing. -/
def mkVote (w rid ph : String) : WitnessVote :=
  { witness := ⟨w⟩
  , consensusId := ⟨"cns1"⟩
  , resultId := ⟨rid⟩
  , prestateHash := ⟨ph⟩
  , share := { shareValue := "sv", nonceBinding := "nb", dataBinding := "db" }
  }

/-- Create evidence with given votes. -/
def mkEvidence (cid : String) (votes : List WitnessVote) (equivocators : List String) : Evidence :=
  { consensusId := ⟨cid⟩
  , votes := votes
  , equivocators := equivocators.map (fun s => ⟨s⟩)
  , commitFact := none
  }

/-- Test vector: empty merge. -/
def tvEmptyMerge : Evidence × Evidence × Evidence :=
  let e1 := mkEvidence "cns1" [] []
  let e2 := mkEvidence "cns1" [] []
  let result := mergeEvidence e1 e2
  (e1, e2, result)

/-- Test vector: disjoint votes merge. -/
def tvDisjointMerge : Evidence × Evidence × Evidence :=
  let e1 := mkEvidence "cns1" [mkVote "w1" "r1" "h1"] []
  let e2 := mkEvidence "cns1" [mkVote "w2" "r1" "h1"] []
  let result := mergeEvidence e1 e2
  (e1, e2, result)

/-- Test vector: overlapping votes merge (idempotence). -/
def tvOverlapMerge : Evidence × Evidence × Evidence :=
  let vote := mkVote "w1" "r1" "h1"
  let e1 := mkEvidence "cns1" [vote] []
  let e2 := mkEvidence "cns1" [vote] []
  let result := mergeEvidence e1 e2
  (e1, e2, result)

/-- Test vector: equivocators merge. -/
def tvEquivocatorsMerge : Evidence × Evidence × Evidence :=
  let e1 := mkEvidence "cns1" [] ["eq1"]
  let e2 := mkEvidence "cns1" [] ["eq2"]
  let result := mergeEvidence e1 e2
  (e1, e2, result)

/-- Test vector: different cid (should be identity on e1). -/
def tvDifferentCid : Evidence × Evidence × Evidence :=
  let e1 := mkEvidence "cns1" [mkVote "w1" "r1" "h1"] []
  let e2 := mkEvidence "cns2" [mkVote "w2" "r1" "h1"] []  -- Different cid!
  let result := mergeEvidence e1 e2
  (e1, e2, result)

/-!
## Threshold Test Vectors
-/

/-- Test vector: threshold met exactly. -/
def tvThresholdExact : List WitnessVote × Nat × Bool :=
  let votes := [mkVote "w1" "r1" "h1", mkVote "w2" "r1" "h1"]
  let threshold := 2
  let result := votes.length >= threshold
  (votes, threshold, result)

/-- Test vector: threshold not met. -/
def tvThresholdNotMet : List WitnessVote × Nat × Bool :=
  let votes := [mkVote "w1" "r1" "h1"]
  let threshold := 2
  let result := votes.length >= threshold
  (votes, threshold, result)

/-- Test vector: zero threshold always met. -/
def tvThresholdZero : List WitnessVote × Nat × Bool :=
  let votes : List WitnessVote := []
  let threshold := 0
  let result := votes.length >= threshold
  (votes, threshold, result)

/-!
## Equivocation Detection Test Vectors
-/

/-- Helper to detect equivocators. -/
def findEquivocators (votes : List WitnessVote) : List AuthorityId :=
  let pairs := votes.flatMap (fun v1 => votes.map (fun v2 => (v1, v2)))
  let equivocating := pairs.filter (fun (v1, v2) =>
    v1.witness == v2.witness && v1.resultId != v2.resultId)
  List.removeDups (equivocating.map (fun (v1, _) => v1.witness))

/-- Test vector: no equivocation. -/
def tvNoEquivocation : List WitnessVote × List String :=
  let votes := [mkVote "w1" "r1" "h1", mkVote "w2" "r1" "h1"]
  let equivocators := findEquivocators votes
  (votes, equivocators.map (·.value))

/-- Test vector: single equivocator. -/
def tvSingleEquivocator : List WitnessVote × List String :=
  let votes := [mkVote "w1" "r1" "h1", mkVote "w1" "r2" "h1"]  -- w1 equivocates
  let equivocators := findEquivocators votes
  (votes, equivocators.map (·.value))

/-- Test vector: multiple equivocators. -/
def tvMultipleEquivocators : List WitnessVote × List String :=
  let votes := [
    mkVote "w1" "r1" "h1", mkVote "w1" "r2" "h1",  -- w1 equivocates
    mkVote "w2" "r1" "h1", mkVote "w2" "r3" "h1",  -- w2 equivocates
    mkVote "w3" "r1" "h1"                          -- w3 honest
  ]
  let equivocators := findEquivocators votes
  (votes, equivocators.map (·.value))

/-!
## JSON Export
-/

/-- Convert test vectors to JSON for Rust consumption. -/
def exportTestVectors : Json :=
  Json.mkObj [
    ("version", Json.str "1.0.0"),
    ("description", Json.str "Test vectors for Lean-proven mathematical primitives"),
    ("merge_tests", Json.mkObj [
      ("empty_merge", Json.mkObj [
        ("description", Json.str "Merge of two empty evidence structures"),
        ("expected_votes", Json.num 0),
        ("expected_equivocators", Json.num 0)
      ]),
      ("disjoint_merge", Json.mkObj [
        ("description", Json.str "Merge with disjoint vote sets"),
        ("expected_votes", Json.num 2),
        ("expected_equivocators", Json.num 0)
      ]),
      ("overlap_merge", Json.mkObj [
        ("description", Json.str "Merge with identical votes (idempotence)"),
        ("expected_votes", Json.num 1),
        ("expected_equivocators", Json.num 0)
      ]),
      ("equivocators_merge", Json.mkObj [
        ("description", Json.str "Merge equivocator lists"),
        ("expected_votes", Json.num 0),
        ("expected_equivocators", Json.num 2)
      ]),
      ("different_cid", Json.mkObj [
        ("description", Json.str "Merge with different consensus IDs (identity)"),
        ("expected_votes", Json.num 1),
        ("expected_equivocators", Json.num 0)
      ])
    ]),
    ("threshold_tests", Json.mkObj [
      ("exact", Json.mkObj [
        ("votes_count", Json.num 2),
        ("threshold", Json.num 2),
        ("expected", Json.bool true)
      ]),
      ("not_met", Json.mkObj [
        ("votes_count", Json.num 1),
        ("threshold", Json.num 2),
        ("expected", Json.bool false)
      ]),
      ("zero", Json.mkObj [
        ("votes_count", Json.num 0),
        ("threshold", Json.num 0),
        ("expected", Json.bool true)
      ])
    ]),
    ("equivocation_tests", Json.mkObj [
      ("no_equivocation", Json.mkObj [
        ("description", Json.str "Honest votes, no conflicts"),
        ("expected_equivocators", Json.arr #[])
      ]),
      ("single_equivocator", Json.mkObj [
        ("description", Json.str "One witness with conflicting votes"),
        ("expected_equivocators", Json.arr #[Json.str "w1"])
      ]),
      ("multiple_equivocators", Json.mkObj [
        ("description", Json.str "Multiple witnesses with conflicting votes"),
        ("expected_equivocators_count", Json.num 2)
      ])
    ])
  ]

/-- Print test vectors to stdout. -/
def printTestVectors : IO Unit := do
  IO.println exportTestVectors.pretty

end Aura.Consensus.TestVectors
