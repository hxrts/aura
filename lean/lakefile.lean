-- Lake build definition for Aura Lean verification modules
import Lake
open Lake DSL

package auraLean where
  precompileModules := true

@[default_target]
lean_lib Aura

-- CLI runner for differential testing (Lean model vs Rust implementation)
-- TODO: Fix executable build
-- lean_exe aura_verifier where
--   root := `Aura.Runner
--   supportInterpreter := true
