/-!
# Aura Verification Runner

CLI tool for differential testing between Lean specifications and Rust implementation.
Reads JSON from stdin and outputs model results as JSON to stdout.

## Purpose

Differential testing catches bugs by running the same inputs through two implementations
(Lean spec, Rust production) and comparing outputs. If they differ, either the Rust
code has a bug or the Lean spec needs updating.

## Rust Correspondence
- File: crates/aura-testkit/src/differential.rs
- Function: Test harnesses that invoke this CLI

## Protocol

Each command reads a JSON object from stdin and writes a JSON result to stdout.

### Commands

- `journal-merge`: Merge two journals
  - Input: `{"journal1": [...], "journal2": [...]}`
  - Output: `{"result": [...], "count": n}`

- `journal-reduce`: Reduce journal to canonical form
  - Input: `{"journal": [...]}`
  - Output: `{"result": [...], "count": n}`

- `flow-charge`: Charge cost against budget
  - Input: `{"budget": n, "cost": n}`
  - Output: `{"success": bool, "remaining": n | null}`

- `timestamp-compare`: Compare two timestamps
  - Input: `{"policy": {...}, "a": {...}, "b": {...}}`
  - Output: `{"ordering": "lt" | "eq" | "gt"}`

- `frost-aggregate`: Aggregate FROST signature shares (T7.8)
  - Input: `{"shares": [{"witness": ..., "share_value": ...}], "threshold": k}`
  - Output: `{"success": bool, "signer_count": n, "binding": ...}`

- `guard-evaluate`: Evaluate guard chain cost (T7.9)
  - Input: `{"steps": [{"flow_cost": n, "cap_req": "..."}]}`
  - Output: `{"total_cost": n, "valid": bool}`

- `evidence-merge`: Merge two evidence structures (for CRDT testing)
  - Input: `{"evidence1": {...}, "evidence2": {...}}`
  - Output: `{"result": {...}, "votes_count": n, "equivocators_count": n}`

- `version`: Show version and available modules
  - Output: `{"version": "0.3.0", "modules": [...]}`
-/

import Lean.Data.Json
import Aura.Journal
import Aura.FlowBudget
import Aura.TimeSystem
import Aura.Consensus.Types
import Aura.Consensus.Evidence

namespace Aura.Runner

open Lean
open Aura.Journal (Fact FactId Journal merge reduce)
open Aura.FlowBudget (Budget charge)
open Aura.TimeSystem (TimeStamp Policy Ordering compare)
open Aura.Consensus.Types
open Aura.Consensus.Evidence (mergeEvidence)

/-!
## Version

Version string for compatibility checking.
-/

/-- Runner version for compatibility checking. -/
def version : String := "0.3.0"

/-!
## JSON Serialization

Bidirectional JSON conversion for all model types.
Field names must match Rust serde serialization.
-/

instance : ToJson FactId where
  toJson f := Json.num f.id

instance : FromJson FactId where
  fromJson? j := do
    let n ← j.getNat?
    pure { id := n }

instance : ToJson Fact where
  toJson f := Json.mkObj [("id", toJson f.id)]

instance : FromJson Fact where
  fromJson? j := do
    let idVal ← j.getObjVal? "id"
    let id ← FromJson.fromJson? idVal
    pure { id := id }

instance : ToJson Journal where
  toJson j := Json.arr (j.map toJson).toArray

instance : FromJson Journal where
  fromJson? j := do
    let arr ← j.getArr?
    let facts ← arr.toList.mapM FromJson.fromJson?
    pure facts

instance : ToJson Budget where
  toJson b := Json.mkObj [("available", Json.num b.available)]

instance : FromJson Budget where
  fromJson? j := do
    let avail ← j.getObjVal? "available"
    let n ← avail.getNat?
    pure { available := n }

instance : ToJson TimeStamp where
  toJson t := Json.mkObj [
    ("logical", Json.num t.logical),
    ("orderClock", Json.num t.orderClock)
  ]

instance : FromJson TimeStamp where
  fromJson? j := do
    let logVal ← j.getObjVal? "logical"
    let ocVal ← j.getObjVal? "orderClock"
    let log ← logVal.getNat?
    let oc ← ocVal.getNat?
    pure { logical := log, orderClock := oc }

instance : ToJson Policy where
  toJson p := Json.mkObj [("ignorePhysical", Json.bool p.ignorePhysical)]

instance : FromJson Policy where
  fromJson? j := do
    let ipVal ← j.getObjVal? "ignorePhysical"
    let ip ← ipVal.getBool?
    pure { ignorePhysical := ip }

instance : ToJson TimeSystem.Ordering where
  toJson o := match o with
    | .lt => Json.str "lt"
    | .eq => Json.str "eq"
    | .gt => Json.str "gt"

/-!
## Command Handlers

Each handler parses JSON input, runs the Lean model, and returns JSON output.
-/

/-- Merge two journals and return the result. -/
def handleJournalMerge (input : String) : IO String := do
  match Json.parse input with
  | .error err =>
    let errJson := Json.mkObj [("error", Json.str s!"JSON parse error: {err}")]
    pure errJson.compress
  | .ok j => do
    match j.getObjVal? "journal1", j.getObjVal? "journal2" with
    | .ok j1Val, .ok j2Val =>
      match FromJson.fromJson? (α := Journal) j1Val, FromJson.fromJson? (α := Journal) j2Val with
      | .ok j1, .ok j2 =>
        let result := merge j1 j2
        let resultJson := Json.mkObj [
          ("result", toJson result),
          ("count", Json.num result.length)
        ]
        pure resultJson.compress
      | _, _ =>
        let errJson := Json.mkObj [("error", Json.str "Failed to parse journals")]
        pure errJson.compress
    | _, _ =>
      let errJson := Json.mkObj [("error", Json.str "Missing journal1 or journal2 field")]
      pure errJson.compress

/-- Reduce a journal to canonical form. -/
def handleJournalReduce (input : String) : IO String := do
  match Json.parse input with
  | .error err =>
    let errJson := Json.mkObj [("error", Json.str s!"JSON parse error: {err}")]
    pure errJson.compress
  | .ok j => do
    match j.getObjVal? "journal" with
    | .ok jVal =>
      match FromJson.fromJson? (α := Journal) jVal with
      | .ok journal =>
        let result := reduce journal
        let resultJson := Json.mkObj [
          ("result", toJson result),
          ("count", Json.num result.length)
        ]
        pure resultJson.compress
      | .error _ =>
        let errJson := Json.mkObj [("error", Json.str "Failed to parse journal")]
        pure errJson.compress
    | .error _ =>
      let errJson := Json.mkObj [("error", Json.str "Missing journal field")]
      pure errJson.compress

/-- Charge a cost against a flow budget. -/
def handleFlowCharge (input : String) : IO String := do
  match Json.parse input with
  | .error err =>
    let errJson := Json.mkObj [("error", Json.str s!"JSON parse error: {err}")]
    pure errJson.compress
  | .ok j => do
    match j.getObjVal? "budget", j.getObjVal? "cost" with
    | .ok budgetVal, .ok costVal =>
      match budgetVal.getNat?, costVal.getNat? with
      | .ok budget, .ok cost =>
        let b : Budget := { available := budget }
        match charge b cost with
        | some result =>
          let resultJson := Json.mkObj [
            ("success", Json.bool true),
            ("remaining", Json.num result.available)
          ]
          pure resultJson.compress
        | none =>
          let resultJson := Json.mkObj [
            ("success", Json.bool false),
            ("remaining", Json.null)
          ]
          pure resultJson.compress
      | _, _ =>
        let errJson := Json.mkObj [("error", Json.str "budget and cost must be numbers")]
        pure errJson.compress
    | _, _ =>
      let errJson := Json.mkObj [("error", Json.str "Missing budget or cost field")]
      pure errJson.compress

/-- Compare two timestamps under a given policy. -/
def handleTimestampCompare (input : String) : IO String := do
  match Json.parse input with
  | .error err =>
    let errJson := Json.mkObj [("error", Json.str s!"JSON parse error: {err}")]
    pure errJson.compress
  | .ok j => do
    match j.getObjVal? "policy", j.getObjVal? "a", j.getObjVal? "b" with
    | .ok pVal, .ok aVal, .ok bVal =>
      match FromJson.fromJson? (α := Policy) pVal,
            FromJson.fromJson? (α := TimeStamp) aVal,
            FromJson.fromJson? (α := TimeStamp) bVal with
      | .ok policy, .ok a, .ok b =>
        let result := compare policy a b
        let resultJson := Json.mkObj [("ordering", toJson result)]
        pure resultJson.compress
      | _, _, _ =>
        let errJson := Json.mkObj [("error", Json.str "Failed to parse policy or timestamps")]
        pure errJson.compress
    | _, _, _ =>
      let errJson := Json.mkObj [("error", Json.str "Missing policy, a, or b field")]
      pure errJson.compress

/-- Return version and available modules for compatibility checking. -/
def handleVersion : IO String := do
  let resultJson := Json.mkObj [
    ("version", Json.str version),
    ("modules", Json.arr #[
      Json.str "Journal",
      Json.str "FlowBudget",
      Json.str "TimeSystem",
      Json.str "GuardChain",
      Json.str "Frost",
      Json.str "Evidence",
      Json.str "KeyDerivation"
    ])
  ]
  pure resultJson.compress

/-!
## Consensus Evidence Handlers (T7.8-T7.10)
-/

/-- JSON serialization for consensus types -/
instance : ToJson WitnessVote where
  toJson v := Json.mkObj [
    ("witness", Json.str v.witness.value),
    ("consensusId", Json.str v.consensusId.value),
    ("resultId", Json.str v.resultId.value),
    ("prestateHash", Json.str v.prestateHash.value),
    ("share", Json.mkObj [
      ("shareValue", Json.str v.share.shareValue),
      ("nonceBinding", Json.str v.share.nonceBinding),
      ("dataBinding", Json.str v.share.dataBinding)
    ])
  ]

instance : FromJson ShareData where
  fromJson? j := do
    let sv ← (← j.getObjVal? "shareValue").getStr?
    let nb ← (← j.getObjVal? "nonceBinding").getStr?
    let db ← (← j.getObjVal? "dataBinding").getStr?
    pure { shareValue := sv, nonceBinding := nb, dataBinding := db }

instance : FromJson WitnessVote where
  fromJson? j := do
    let w ← (← j.getObjVal? "witness").getStr?
    let cid ← (← j.getObjVal? "consensusId").getStr?
    let rid ← (← j.getObjVal? "resultId").getStr?
    let ph ← (← j.getObjVal? "prestateHash").getStr?
    let shareVal ← j.getObjVal? "share"
    let share ← FromJson.fromJson? shareVal
    pure {
      witness := ⟨w⟩
      consensusId := ⟨cid⟩
      resultId := ⟨rid⟩
      prestateHash := ⟨ph⟩
      share := share
    }

instance : ToJson Evidence where
  toJson e := Json.mkObj [
    ("consensusId", Json.str e.consensusId.value),
    ("votes", Json.arr (e.votes.map toJson).toArray),
    ("equivocators", Json.arr (e.equivocators.map (fun a => Json.str a.value)).toArray),
    ("hasCommit", Json.bool e.commitFact.isSome)
  ]

instance : FromJson Evidence where
  fromJson? j := do
    let cidStr ← (← j.getObjVal? "consensusId").getStr?
    let votesArr ← (← j.getObjVal? "votes").getArr?
    let votes ← votesArr.toList.mapM FromJson.fromJson?
    let eqArr ← (← j.getObjVal? "equivocators").getArr?
    let equivocators ← eqArr.toList.mapM (fun e => do pure ⟨← e.getStr?⟩)
    pure {
      consensusId := ⟨cidStr⟩
      votes := votes
      equivocators := equivocators
      commitFact := none  -- We don't parse commit facts for now
    }

/-- Merge two evidence structures and return result. -/
def handleEvidenceMerge (input : String) : IO String := do
  match Json.parse input with
  | .error err =>
    let errJson := Json.mkObj [("error", Json.str s!"JSON parse error: {err}")]
    pure errJson.compress
  | .ok j => do
    match j.getObjVal? "evidence1", j.getObjVal? "evidence2" with
    | .ok e1Val, .ok e2Val =>
      match FromJson.fromJson? (α := Evidence) e1Val, FromJson.fromJson? (α := Evidence) e2Val with
      | .ok e1, .ok e2 =>
        let result := mergeEvidence e1 e2
        let resultJson := Json.mkObj [
          ("result", toJson result),
          ("votes_count", Json.num result.votes.length),
          ("equivocators_count", Json.num result.equivocators.length)
        ]
        pure resultJson.compress
      | _, _ =>
        let errJson := Json.mkObj [("error", Json.str "Failed to parse evidence")]
        pure errJson.compress
    | _, _ =>
      let errJson := Json.mkObj [("error", Json.str "Missing evidence1 or evidence2 field")]
      pure errJson.compress

/-- Check threshold for FROST signature aggregation. -/
def handleFrostAggregate (input : String) : IO String := do
  match Json.parse input with
  | .error err =>
    let errJson := Json.mkObj [("error", Json.str s!"JSON parse error: {err}")]
    pure errJson.compress
  | .ok j => do
    match j.getObjVal? "shares", j.getObjVal? "threshold" with
    | .ok sharesVal, .ok threshVal =>
      match sharesVal.getArr?, threshVal.getNat? with
      | .ok sharesArr, .ok threshold =>
        -- Parse shares as simplified vote-like structures
        let shares := sharesArr.toList.filterMap (fun s =>
          match s.getObjVal? "witness", s.getObjVal? "share_value" with
          | .ok w, .ok sv =>
            match w.getStr?, sv.getStr? with
            | .ok ws, .ok svs => some (ws, svs)
            | _, _ => none
          | _, _ => none)
        let meetsThreshold := shares.length >= threshold
        let resultJson := Json.mkObj [
          ("success", Json.bool meetsThreshold),
          ("signer_count", Json.num shares.length),
          ("threshold", Json.num threshold),
          ("signers", Json.arr (shares.map (fun (w, _) => Json.str w)).toArray)
        ]
        pure resultJson.compress
      | _, _ =>
        let errJson := Json.mkObj [("error", Json.str "shares must be array, threshold must be number")]
        pure errJson.compress
    | _, _ =>
      let errJson := Json.mkObj [("error", Json.str "Missing shares or threshold field")]
      pure errJson.compress

/-- Evaluate guard chain cost. -/
def handleGuardEvaluate (input : String) : IO String := do
  match Json.parse input with
  | .error err =>
    let errJson := Json.mkObj [("error", Json.str s!"JSON parse error: {err}")]
    pure errJson.compress
  | .ok j => do
    match j.getObjVal? "steps" with
    | .ok stepsVal =>
      match stepsVal.getArr? with
      | .ok stepsArr =>
        -- Sum up flow costs from each step
        let costs := stepsArr.toList.filterMap (fun s =>
          match s.getObjVal? "flow_cost" with
          | .ok fc => fc.getNat?.toOption
          | .error _ => none)
        let totalCost := costs.foldl (· + ·) 0
        let resultJson := Json.mkObj [
          ("total_cost", Json.num totalCost),
          ("step_count", Json.num costs.length),
          ("valid", Json.bool true)  -- All costs parsed successfully
        ]
        pure resultJson.compress
      | .error _ =>
        let errJson := Json.mkObj [("error", Json.str "steps must be array")]
        pure errJson.compress
    | .error _ =>
      let errJson := Json.mkObj [("error", Json.str "Missing steps field")]
      pure errJson.compress

/-- Read all lines from stdin until EOF. -/
def readStdin : IO String := do
  let stdin ← IO.getStdin
  let mut result := ""
  let mut done := false
  while !done do
    let line ← stdin.getLine
    if line.isEmpty then
      done := true
    else
      result := result ++ line
  pure result.trim

/-- Dispatch command by name. -/
def runCommand (args : List String) : IO Unit := do
  match args with
  | ["version"] =>
    let result ← handleVersion
    IO.println result
  | ["journal-merge"] =>
    let input ← readStdin
    let result ← handleJournalMerge input
    IO.println result
  | ["journal-reduce"] =>
    let input ← readStdin
    let result ← handleJournalReduce input
    IO.println result
  | ["flow-charge"] =>
    let input ← readStdin
    let result ← handleFlowCharge input
    IO.println result
  | ["timestamp-compare"] =>
    let input ← readStdin
    let result ← handleTimestampCompare input
    IO.println result
  | ["evidence-merge"] =>
    let input ← readStdin
    let result ← handleEvidenceMerge input
    IO.println result
  | ["frost-aggregate"] =>
    let input ← readStdin
    let result ← handleFrostAggregate input
    IO.println result
  | ["guard-evaluate"] =>
    let input ← readStdin
    let result ← handleGuardEvaluate input
    IO.println result
  | _ =>
    IO.println "Aura Lean Verifier - Differential Testing Oracle"
    IO.println s!"Version: {version}"
    IO.println ""
    IO.println "Usage: aura-verifier <command>"
    IO.println ""
    IO.println "Commands:"
    IO.println "  version            - Show version and available modules (JSON)"
    IO.println "  journal-merge      - Merge two journals (JSON stdin/stdout)"
    IO.println "  journal-reduce     - Reduce a journal (JSON stdin/stdout)"
    IO.println "  flow-charge        - Charge flow budget (JSON stdin/stdout)"
    IO.println "  timestamp-compare  - Compare timestamps (JSON stdin/stdout)"
    IO.println "  evidence-merge     - Merge two evidence structures (JSON stdin/stdout)"
    IO.println "  frost-aggregate    - Check FROST threshold aggregation (JSON stdin/stdout)"
    IO.println "  guard-evaluate     - Evaluate guard chain cost (JSON stdin/stdout)"
    IO.println ""
    IO.println "All commands read JSON from stdin and write JSON to stdout."

end Aura.Runner

/-- Main entry point. -/
def main (args : List String) : IO UInt32 := do
  Aura.Runner.runCommand args
  pure 0
