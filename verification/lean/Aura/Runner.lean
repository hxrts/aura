-- CLI runner for Aura Lean verification
-- This is used for differential testing against Rust implementations

import Lean.Data.Json
import Aura.Journal
import Aura.FlowBudget
import Aura.TimeSystem

namespace Aura.Runner

open Lean
open Aura.Journal (Fact FactId Journal merge reduce)
open Aura.FlowBudget (Budget charge)
open Aura.TimeSystem (TimeStamp Policy Ordering compare)

/-!
# Aura Verification Runner

This CLI tool allows running Lean models from Rust tests for differential testing.
It reads JSON from stdin and outputs model results as JSON to stdout.

## Protocol

Each command reads a JSON object from stdin and writes a JSON result to stdout.

### journal-merge
Input: `{"journal1": [...], "journal2": [...]}`
Output: `{"result": [...], "count": n}`

### flow-charge
Input: `{"budget": n, "cost": n}`
Output: `{"success": bool, "remaining": n | null}`

### timestamp-compare
Input: `{"policy": {...}, "a": {...}, "b": {...}}`
Output: `{"ordering": "lt" | "eq" | "gt"}`

### version
Output: `{"version": "0.2.0", "modules": [...]}`
-/

def version : String := "0.2.0"

-- JSON instances for FactId
instance : ToJson FactId where
  toJson f := Json.num f.id

instance : FromJson FactId where
  fromJson? j := do
    let n ← j.getNat?
    pure { id := n }

-- JSON instances for Fact
instance : ToJson Fact where
  toJson f := Json.mkObj [("id", toJson f.id)]

instance : FromJson Fact where
  fromJson? j := do
    let idVal ← j.getObjVal? "id"
    let id ← FromJson.fromJson? idVal
    pure { id := id }

-- JSON instances for Journal
instance : ToJson Journal where
  toJson j := Json.arr (j.map toJson).toArray

instance : FromJson Journal where
  fromJson? j := do
    let arr ← j.getArr?
    let facts ← arr.toList.mapM FromJson.fromJson?
    pure facts

-- JSON instances for Budget
instance : ToJson Budget where
  toJson b := Json.mkObj [("available", Json.num b.available)]

instance : FromJson Budget where
  fromJson? j := do
    let avail ← j.getObjVal? "available"
    let n ← avail.getNat?
    pure { available := n }

-- JSON instances for TimeStamp
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

-- JSON instances for Policy
instance : ToJson Policy where
  toJson p := Json.mkObj [("ignorePhysical", Json.bool p.ignorePhysical)]

instance : FromJson Policy where
  fromJson? j := do
    let ipVal ← j.getObjVal? "ignorePhysical"
    let ip ← ipVal.getBool?
    pure { ignorePhysical := ip }

-- JSON instances for TimeSystem.Ordering
instance : ToJson TimeSystem.Ordering where
  toJson o := match o with
    | .lt => Json.str "lt"
    | .eq => Json.str "eq"
    | .gt => Json.str "gt"

-- Command handlers

/-- Handle journal-merge command -/
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

/-- Handle journal-reduce command -/
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

/-- Handle flow-charge command -/
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

/-- Handle timestamp-compare command -/
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

/-- Handle version command -/
def handleVersion : IO String := do
  let resultJson := Json.mkObj [
    ("version", Json.str version),
    ("modules", Json.arr #[
      Json.str "Journal",
      Json.str "FlowBudget",
      Json.str "TimeSystem",
      Json.str "GuardChain",
      Json.str "Frost",
      Json.str "KeyDerivation"
    ])
  ]
  pure resultJson.compress

/-- Read all input from stdin -/
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

/-- Run a command with stdin/stdout -/
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
    IO.println ""
    IO.println "All commands read JSON from stdin and write JSON to stdout."

end Aura.Runner

/-- Main entry point for the CLI -/
def main (args : List String) : IO UInt32 := do
  Aura.Runner.runCommand args
  pure 0
