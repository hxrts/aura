-- CLI runner for Aura Lean verification
-- This will be used for differential testing against Rust implementations

import Lean.Data.Json
import Aura

namespace Aura.Runner

open Lean

/-!
# Aura Verification Runner

This CLI tool allows running Lean models from Rust tests for differential testing.
It reads serialized inputs (JSON/CBOR) and outputs model results.

Subcommands:
- journal-merge: Test journal merge operation
- journal-reduce: Test journal reduction
- guard-evaluate: Test guard chain evaluation
- frost-check: Verify FROST state machine properties
-/

def runCommand (args : List String) : IO Unit := do
  match args with
  | ["version"] =>
    IO.println "Aura Lean Verifier v0.1.0"
  | ["journal-merge"] =>
    IO.println "Journal merge verification (not yet implemented)"
  | ["journal-reduce"] =>
    IO.println "Journal reduce verification (not yet implemented)"
  | ["guard-evaluate"] =>
    IO.println "Guard chain evaluation verification (not yet implemented)"
  | ["frost-check"] =>
    IO.println "FROST state machine verification (not yet implemented)"
  | _ =>
    IO.println "Usage: aura_verifier <command>"
    IO.println "Commands:"
    IO.println "  version          - Show version"
    IO.println "  journal-merge    - Verify journal merge"
    IO.println "  journal-reduce   - Verify journal reduction"
    IO.println "  guard-evaluate   - Verify guard evaluation"
    IO.println "  frost-check      - Verify FROST protocol"

/-- Main entry point for the CLI -/
def main (args : List String) : IO UInt32 := do
  runCommand args
  pure 0

end Aura.Runner
