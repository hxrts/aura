-- Lake build configuration for Aura Lean verification modules.
-- Lake is Lean 4's package manager (similar to Cargo for Rust).
import Lake
open Lake DSL

-- Package definition: precompileModules speeds up incremental builds
-- by caching compiled .olean files.
package «auraLean» where
  precompileModules := true

-- Default library target: builds all Aura.* modules.
-- Running `lake build` compiles proofs and fails if any use `sorry`.
@[default_target]
lean_lib «Aura»

-- Executable for differential testing: compares Lean model outputs to Rust.
-- Reads JSON from stdin, runs the Lean model, outputs JSON to stdout.
-- Usage: `echo '{"budget":100,"cost":30}' | aura_verifier flow-charge`
lean_exe «aura_verifier» where
  root := `Aura.Runner
  supportInterpreter := true
