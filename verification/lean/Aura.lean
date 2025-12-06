-- Top-level module for Aura formal verification.
-- Re-exports all verification components for a single import point.
-- Each submodule proves invariants about a specific Aura subsystem.

import Aura.Journal       -- CRDT semilattice proofs (merge commutativity, associativity, idempotence)
import Aura.KeyDerivation -- Context-specific key derivation isolation (PRF security assumption)
import Aura.GuardChain    -- Guard evaluation cost calculation correctness
import Aura.FlowBudget    -- Budget charging monotonicity and exactness
import Aura.Frost         -- FROST threshold signing session/round consistency
import Aura.TimeSystem    -- Timestamp comparison reflexivity, transitivity, and privacy
